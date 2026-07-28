//! Exploration budget and punitive slashing.
//!
//! Two instructions live here, and they are the two ends of the same policy:
//! `fund_scout` is how an unproven forager is given a bounded chance to prove
//! itself, and `slash_forager` is how a forager that broke the rules or ran
//! past the loss threshold pays for it.
//!
//! Single base asset. Bond, principal, deposits, the Risk Cache and every
//! slashed amount are denominated in `base_mint`. The loss waterfall (bond,
//! then cache, then depositor NAV) only closes without a swap if all three are
//! the same asset, and this program contains no DEX, so a second collateral
//! token would break the waterfall rather than diversify it.
//!
//! What "burn" means here. The burned share of a seized bond is transferred to
//! the incinerator vault: a program-owned token account that no instruction in
//! this program can withdraw from, because no such instruction exists. That
//! makes it an economic burn -- the tokens are unrecoverable by anyone,
//! including the authority -- and not a token-supply burn. If `base_mint` is
//! not the project token, moving tokens there does not reduce the project
//! token's supply, and nothing in this program or its events claims that it
//! does.

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::*;
use crate::errors::ColonyError;
use crate::events::{ForagerSlashed, ScoutFunded};
use crate::math;
use crate::state::{BroodVaultState, ColonyConfig, ForagerState, RiskCacheState};
use crate::utils;

// ---------------------------------------------------------------------------
// fund_scout
// ---------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(forager_id: u64)]
pub struct FundScout<'info> {
    /// The operator that will trade the ticket.
    ///
    /// A ticket is drawn, not pushed: requiring the operator's signature means
    /// nobody can force colony capital into a sub-account that is not asking
    /// for it, and it ties the forager PDA below to the signer.
    pub operator: Signer<'info>,

    #[account(
        mut,
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
        has_one = vault_base @ ColonyError::VaultMismatch,
    )]
    pub brood: Box<Account<'info, BroodVaultState>>,

    #[account(
        mut,
        seeds = [SEED_FORAGER, operator.key().as_ref(), &forager_id.to_le_bytes()],
        bump = forager.bump,
        has_one = operator,
        has_one = forager_vault @ ColonyError::VaultMismatch,
    )]
    pub forager: Box<Account<'info, ForagerState>>,

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
        seeds = [SEED_FORAGER_VAULT, forager.key().as_ref()],
        bump = forager.vault_bump,
        token::mint = base_mint,
        token::authority = forager,
    )]
    pub forager_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub base_mint: Box<InterfaceAccount<'info, Mint>>,
    pub token_program: Interface<'info, TokenInterface>,
}

/// Hand a scout one exploration ticket from the scout pool.
///
/// A newly registered forager has zero pheromone, so under pure trail weighting
/// it would receive nothing forever and could never build a trail: the
/// cold-start and rich-get-richer failure. The colony answers it by reserving
/// `scout_budget_bps` of value under colony as an exploration budget that
/// pheromone never touches, and paying it out as fixed-size tickets
/// (`scout_ticket_base_units`), one per scout per epoch.
///
/// A scout's exposure is bounded to its ticket plus its posted bond. That is a
/// bound on the size of the exposure, not on the outcome: a ticket can be lost
/// in full, and the loss reaches depositors exactly as
/// `settlement::settle_forager` accounts for it.
///
/// NAV does not move here. `idle_base` falls and `outstanding_principal` rises
/// by the same ticket, so `nav = idle_base + outstanding_principal` is
/// unchanged and no share price moves when capital is deployed.
///
/// `forager_id` is consumed by the `FundScout` seed derivation rather than by
/// this body, which is why it is bound with a leading underscore.
pub fn fund_scout(ctx: Context<FundScout>, _forager_id: u64) -> Result<()> {
    let epoch = ctx.accounts.config.epoch;
    let ticket = ctx.accounts.config.scout_ticket_base_units;

    // -- checks -------------------------------------------------------------

    // Allocation may only move while the colony is Open. During a settlement
    // the pheromone sum and the pools are mid-flight, and paying a ticket out
    // of `scout_pool` then would race the crank that is recomputing it.
    require!(
        ctx.accounts.config.epoch_phase == PHASE_OPEN,
        ColonyError::WrongPhase
    );
    require!(!ctx.accounts.config.paused, ColonyError::Paused);
    require!(
        ctx.accounts.forager.status == STATUS_SCOUT,
        ColonyError::ForagerNotScout
    );
    // One ticket per scout per epoch. `last_scout_epoch` is stamped below, so a
    // second call in the same epoch fails this check rather than draining the
    // exploration budget through one forager.
    require!(
        ctx.accounts.forager.last_scout_epoch < epoch,
        ColonyError::ScoutTicketAlreadyDrawn
    );
    require!(
        ticket > 0 && ctx.accounts.config.scout_pool >= ticket,
        ColonyError::ScoutPoolExhausted
    );
    // The pool is an accounting reservation; the tokens still have to be idle
    // in the vault. Queued redemptions and deployed principal are not raidable
    // to pay a scout ticket.
    require!(
        ctx.accounts.brood.idle_base >= ticket,
        ColonyError::InsufficientIdleLiquidity
    );

    // -- effects ------------------------------------------------------------

    ctx.accounts.config.scout_pool = ctx.accounts.config.scout_pool.saturating_sub(ticket);
    ctx.accounts.brood.idle_base = ctx.accounts.brood.idle_base.saturating_sub(ticket);
    ctx.accounts.brood.outstanding_principal = ctx
        .accounts
        .brood
        .outstanding_principal
        .checked_add(ticket)
        .ok_or(ColonyError::Overflow)?;
    ctx.accounts.forager.principal = ctx
        .accounts
        .forager
        .principal
        .checked_add(ticket)
        .ok_or(ColonyError::Overflow)?;
    ctx.accounts.forager.last_scout_epoch = epoch;

    // -- interaction --------------------------------------------------------

    // The brood state PDA owns `vault_base`, so it signs the payout. A failed
    // CPI reverts the whole transaction, so the counters written above can
    // never drift away from the tokens that actually moved.
    let brood_bump = [ctx.accounts.brood.bump];
    let brood_seeds: &[&[u8]] = &[SEED_BROOD, &brood_bump];
    let signer_seeds: &[&[&[u8]]] = &[brood_seeds];

    utils::transfer_signed(
        &ctx.accounts.token_program,
        &ctx.accounts.vault_base,
        &ctx.accounts.forager_vault,
        &ctx.accounts.base_mint,
        &ctx.accounts.brood.to_account_info(),
        signer_seeds,
        ticket,
    )?;

    emit!(ScoutFunded {
        forager: ctx.accounts.forager.key(),
        epoch,
        ticket,
        scout_pool_remaining: ctx.accounts.config.scout_pool,
    });

    Ok(())
}
