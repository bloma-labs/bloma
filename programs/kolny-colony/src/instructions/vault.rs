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

// ===========================================================================
// withdraw
// ===========================================================================

/// A withdrawal is not blocked while the colony is paused. Pausing stops new
/// capital from entering and stops capital from being moved out to foragers; it
/// is not a mechanism for holding depositors in.
#[derive(Accounts)]
pub struct Withdraw<'info> {
    pub depositor: Signer<'info>,

    pub base_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        seeds = [SEED_BROOD],
        bump = brood.bump,
        has_one = base_mint @ ColonyError::BaseMintMismatch,
    )]
    pub brood: Box<Account<'info, BroodVaultState>>,

    /// The seed derivation binds this position to the signer, which is the same
    /// guarantee `has_one = owner` gives; the stored owner is compared as well
    /// so a position can never be operated by anyone but its holder.
    ///
    /// The position is deliberately NOT closed when its balance reaches zero.
    /// A `close` constraint is unconditional -- Anchor would close the account
    /// at the end of every withdrawal, partial ones included, destroying a
    /// still-positive share balance along with it. Closing only on a full exit
    /// would mean a conditional close in the handler, which buys back a small
    /// rent deposit in exchange for a re-creation charge on the next deposit
    /// and one more failure mode. Leaving the account alive at zero shares is
    /// cheaper for anyone who re-deposits and keeps an honest, readable record
    /// of a zero balance.
    #[account(
        mut,
        seeds = [SEED_POSITION, depositor.key().as_ref()],
        bump = position.bump,
        constraint = position.owner == depositor.key(),
    )]
    pub position: Box<Account<'info, DepositorPosition>>,

    #[account(
        mut,
        seeds = [SEED_BROOD_VAULT],
        bump = brood.vault_bump,
        token::mint = base_mint,
        token::authority = brood,
    )]
    pub vault_base: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = base_mint,
        token::authority = depositor,
    )]
    pub depositor_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn withdraw(ctx: Context<Withdraw>, shares: u128) -> Result<()> {
    require!(shares > 0, ColonyError::ZeroAmount);
    require!(
        ctx.accounts.position.shares >= shares,
        ColonyError::InsufficientShares
    );

    let nav = ctx.accounts.brood.nav();
    let total_shares = ctx.accounts.brood.total_shares;

    // Rounds down, so any residual atom stays with the depositors who remain.
    let assets = math::assets_for_shares(shares, total_shares, nav);

    // Idle liquidity is the honest limit. Capital already deployed to a forager
    // cannot be recalled by this program, so a larger exit queues instead.
    require!(
        assets <= ctx.accounts.brood.idle_base,
        ColonyError::InsufficientIdleLiquidity
    );

    let depositor_key = ctx.accounts.depositor.key();
    let brood_bump = ctx.accounts.brood.bump;

    // -- effects -----------------------------------------------------------
    let brood = &mut ctx.accounts.brood;
    brood.idle_base = brood.idle_base.saturating_sub(assets);
    brood.total_shares = brood.total_shares.saturating_sub(shares);
    let nav_after = brood.nav();
    let total_shares_after = brood.total_shares;

    let position = &mut ctx.accounts.position;
    position.shares = position.shares.saturating_sub(shares);

    // -- interaction -------------------------------------------------------
    // The vault token account is owned by the brood state PDA, so the transfer
    // out is signed with that PDA's seeds and its stored bump.
    let brood_seeds: &[&[u8]] = &[SEED_BROOD, &[brood_bump]];
    let signer_seeds: &[&[&[u8]]] = &[brood_seeds];
    let brood_authority = ctx.accounts.brood.to_account_info();

    utils::transfer_signed(
        &ctx.accounts.token_program,
        &ctx.accounts.vault_base,
        &ctx.accounts.depositor_ata,
        &ctx.accounts.base_mint,
        &brood_authority,
        signer_seeds,
        assets,
    )?;

    emit!(Withdrawn {
        depositor: depositor_key,
        shares,
        assets,
        nav_after,
        total_shares_after,
    });

    Ok(())
}

// ===========================================================================
// request_redemption
// ===========================================================================

/// The request PDA takes its id from `brood.next_redemption_id` rather than
/// from an instruction argument, so a depositor cannot pick the id of a request
/// that already exists or skip ahead of the queue counter.
#[derive(Accounts)]
pub struct RequestRedemption<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,

    #[account(seeds = [SEED_COLONY], bump = config.bump)]
    pub config: Box<Account<'info, ColonyConfig>>,

    #[account(mut, seeds = [SEED_BROOD], bump = brood.bump)]
    pub brood: Box<Account<'info, BroodVaultState>>,

    #[account(
        mut,
        seeds = [SEED_POSITION, depositor.key().as_ref()],
        bump = position.bump,
        constraint = position.owner == depositor.key(),
    )]
    pub position: Box<Account<'info, DepositorPosition>>,

    #[account(
        init,
        payer = depositor,
        space = 8 + RedemptionRequest::LEN,
        seeds = [
            SEED_REDEEM,
            depositor.key().as_ref(),
            &brood.next_redemption_id.to_le_bytes(),
        ],
        bump,
    )]
    pub request: Box<Account<'info, RedemptionRequest>>,

    pub system_program: Program<'info, System>,
}

pub fn request_redemption(ctx: Context<RequestRedemption>, shares: u128) -> Result<()> {
    require!(shares > 0, ColonyError::ZeroAmount);
    require!(
        ctx.accounts.position.shares >= shares,
        ColonyError::InsufficientShares
    );

    let depositor_key = ctx.accounts.depositor.key();
    let requested_epoch = ctx.accounts.config.epoch;
    let request_id = ctx.accounts.brood.next_redemption_id;
    let request_bump = ctx.bumps.request;

    // The shares leave the position immediately so the same shares cannot be
    // queued twice, or queued and then withdrawn through the idle path. They
    // are NOT burned here: the depositor keeps the exposure until the request
    // is actually paid, so queuing does not move anyone's share price.
    let position = &mut ctx.accounts.position;
    position.shares = position.shares.saturating_sub(shares);

    let brood = &mut ctx.accounts.brood;
    brood.pending_redemption_shares = brood
        .pending_redemption_shares
        .checked_add(shares)
        .ok_or(ColonyError::Overflow)?;
    brood.next_redemption_id = request_id.checked_add(1).ok_or(ColonyError::Overflow)?;

    let request = &mut ctx.accounts.request;
    request.owner = depositor_key;
    request.shares = shares;
    request.request_id = request_id;
    request.requested_epoch = requested_epoch;
    request.assets_paid = 0;
    request.bump = request_bump;

    emit!(RedemptionRequested {
        depositor: depositor_key,
        request_id,
        shares,
        requested_epoch,
    });

    Ok(())
}
