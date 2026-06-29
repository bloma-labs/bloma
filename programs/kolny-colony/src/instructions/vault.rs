//! Depositor flows: deposit, withdraw, the redemption queue and cache funding.
//!
//! Share accounting is ERC-4626 shaped, but over an ACCOUNTING net asset value:
//! `nav = idle_base + outstanding_principal`, both of which move only through
//! credited deposits, credited withdrawals and settlement. No path here reads a
//! live token balance into NAV. That single decision is what makes an
//! unsolicited transfer straight into a vault account unable to move the share
//! price: the tokens are simply uncredited, so the classic donation-inflation
//! setup has nothing to inflate. The virtual share offset in `math` closes the
//! remaining first-depositor rounding edge, and `MINIMUM_DEPOSIT` keeps a
//! one-atom seed out of the vault entirely.
//!
//! Rounding always favors the vault, meaning the depositors who stay: shares
//! minted on a deposit round down, assets released on a withdrawal round down.
//! `math` already rounds that way, so these handlers never hand-roll the ratio.
//!
//! Liquidity is stated honestly. A withdrawal is paid only out of `idle_base`.
//! Capital that has been deployed to a forager is not recallable by this
//! program -- it sits in an isolated sub-account that an operator trades -- so a
//! withdrawal larger than idle liquidity fails and the depositor queues a
//! `RedemptionRequest` instead. The queue is drained by a permissionless
//! `fulfill_redemption` crank as principal comes back through settlement. There
//! is no promise of instant liquidity anywhere in this file, because the
//! program could not keep one.
//!
//! Share conservation invariant, maintained by every handler below:
//!
//! ```text
//! sum(position.shares) + brood.pending_redemption_shares == brood.total_shares
//! ```
//!
//! Queued shares are moved out of the position but are NOT burned at request
//! time. The depositor keeps the economic exposure until the request is
//! actually paid, so queuing changes nobody's share price.

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::{
    MINIMUM_DEPOSIT, SEED_BROOD, SEED_BROOD_VAULT, SEED_CACHE, SEED_CACHE_VAULT, SEED_COLONY,
    SEED_POSITION, SEED_REDEEM,
};
use crate::errors::ColonyError;
use crate::events::{CacheFunded, Deposited, RedemptionFulfilled, RedemptionRequested, Withdrawn};
use crate::math;
use crate::state::{
    BroodVaultState, ColonyConfig, DepositorPosition, RedemptionRequest, RiskCacheState,
};
use crate::utils;

// ===========================================================================
// deposit
// ===========================================================================

/// Every account is boxed: nine accounts plus an `init_if_needed` is well past
/// the point where the 4096-byte stack frame starts to matter.
///
/// `position` is the only account created here. Anchor's re-initialization
/// warning does not apply to it, because the handler never resets a field it
/// did not compute -- shares are added, never overwritten -- and the seed
/// derivation pins the account to the signer, so a second call can only reach
/// the depositor's own position.
#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,

    pub base_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        seeds = [SEED_COLONY],
        bump = config.bump,
        has_one = base_mint @ ColonyError::BaseMintMismatch,
    )]
    pub config: Box<Account<'info, ColonyConfig>>,

    #[account(
        mut,
        seeds = [SEED_BROOD],
        bump = brood.bump,
        has_one = base_mint @ ColonyError::BaseMintMismatch,
    )]
    pub brood: Box<Account<'info, BroodVaultState>>,

    #[account(
        init_if_needed,
        payer = depositor,
        space = 8 + DepositorPosition::LEN,
        seeds = [SEED_POSITION, depositor.key().as_ref()],
        bump,
    )]
    pub position: Box<Account<'info, DepositorPosition>>,

    /// Source of the base asset. Any token account of the base mint owned by
    /// the signer is accepted; canonically this is the depositor's associated
    /// token account.
    #[account(
        mut,
        token::mint = base_mint,
        token::authority = depositor,
    )]
    pub depositor_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [SEED_BROOD_VAULT],
        bump = brood.vault_bump,
        token::mint = base_mint,
        token::authority = brood,
    )]
    pub vault_base: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn deposit(ctx: Context<Deposit>, assets: u64) -> Result<()> {
    require!(!ctx.accounts.config.paused, ColonyError::Paused);
    require!(assets >= MINIMUM_DEPOSIT, ColonyError::BelowMinimumDeposit);

    // Accounting NAV, never `vault_base.amount`. See the module header.
    let nav = ctx.accounts.brood.nav();
    let total_shares = ctx.accounts.brood.total_shares;
    let shares = math::shares_for_deposit(assets, total_shares, nav);

    // A deposit that would mint zero shares is rejected instead of quietly
    // accepted. Crediting the assets for nothing would hand the whole deposit
    // to the existing holders, which is a loss no depositor asked for.
    require!(shares > 0, ColonyError::ZeroAmount);

    let depositor_key = ctx.accounts.depositor.key();
    let position_bump = ctx.bumps.position;

    // -- effects -----------------------------------------------------------
    let brood = &mut ctx.accounts.brood;
    brood.idle_base = brood
        .idle_base
        .checked_add(assets)
        .ok_or(ColonyError::Overflow)?;
    brood.total_shares = brood
        .total_shares
        .checked_add(shares)
        .ok_or(ColonyError::Overflow)?;
    let nav_after = brood.nav();
    let total_shares_after = brood.total_shares;

    let position = &mut ctx.accounts.position;
    position.owner = depositor_key;
    position.shares = position
        .shares
        .checked_add(shares)
        .ok_or(ColonyError::Overflow)?;
    position.bump = position_bump;

    // -- interaction -------------------------------------------------------
    utils::transfer_from_user(
        &ctx.accounts.token_program,
        &ctx.accounts.depositor_ata,
        &ctx.accounts.vault_base,
        &ctx.accounts.base_mint,
        &ctx.accounts.depositor,
        assets,
    )?;

    emit!(Deposited {
        depositor: depositor_key,
        assets,
        shares,
        nav_after,
        total_shares_after,
    });

    Ok(())
}
