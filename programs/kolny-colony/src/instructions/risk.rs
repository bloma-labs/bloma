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

// ---------------------------------------------------------------------------
// slash_forager
// ---------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(forager_id: u64)]
pub struct SlashForager<'info> {
    pub authority: Signer<'info>,

    #[account(
        seeds = [SEED_COLONY],
        bump = config.bump,
        has_one = authority @ ColonyError::NotAuthority,
        has_one = base_mint @ ColonyError::BaseMintMismatch,
    )]
    pub config: Box<Account<'info, ColonyConfig>>,

    /// CHECK: identity only. It is pinned by `has_one = operator` on the
    /// forager record and by the forager PDA seeds below; no data is read from
    /// this account and nothing is transferred to it.
    pub operator: UncheckedAccount<'info>,

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
        seeds = [SEED_CACHE],
        bump = cache.bump,
        has_one = cache_vault @ ColonyError::VaultMismatch,
        has_one = incinerator_vault @ ColonyError::VaultMismatch,
    )]
    pub cache: Box<Account<'info, RiskCacheState>>,

    #[account(
        mut,
        seeds = [SEED_FORAGER_VAULT, forager.key().as_ref()],
        bump = forager.vault_bump,
        token::mint = base_mint,
        token::authority = forager,
    )]
    pub forager_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [SEED_CACHE_VAULT],
        bump = cache.vault_bump,
        token::mint = base_mint,
    )]
    pub cache_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Locked sink. This program has no instruction that moves tokens out of
    /// it, which is the whole mechanism: what lands here is unrecoverable.
    #[account(
        mut,
        seeds = [SEED_INCINERATOR],
        bump = cache.incinerator_bump,
        token::mint = base_mint,
    )]
    pub incinerator_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub base_mint: Box<InterfaceAccount<'info, Mint>>,
    pub token_program: Interface<'info, TokenInterface>,
}

/// Bond fraction to seize, decided purely from the reason code and committed
/// state.
///
/// Split out of the handler so the graduated policy in `risk-spec.md` 1.4 can
/// be verified with `cargo test` instead of only on a validator. `Clock` stays
/// in the handler; the elapsed time is passed in.
fn punitive_slash_bps(
    reason_code: u8,
    drawdown_bps: u16,
    dd_slash_bps: u16,
    secs_since_heartbeat: i64,
    nonresponse_limit_secs: i64,
) -> Result<u16> {
    match reason_code {
        // A market loss is graduated: seize a fraction of the bond in
        // proportion to how far past `dd_slash` the drawdown went, up to the
        // whole bond. At or under the threshold nothing is seizable, and the
        // call is refused rather than seizing zero and recording a slash.
        SLASH_REASON_LOSS_THRESHOLD => {
            let ramped = math::loss_slash_bps(drawdown_bps, dd_slash_bps);
            require!(ramped > 0, ColonyError::NotSlashable);
            Ok(ramped)
        }
        // Trading outside the position-limit profile is an integrity failure,
        // not a market outcome, so it costs the whole bond.
        SLASH_REASON_RULE_VIOLATION => Ok(BPS_DENOM),
        // Silence past the configured timeout is likewise an integrity
        // failure: an operator that cannot be reached cannot be instructed to
        // unwind. The elapsed time is checked on-chain against the recorded
        // heartbeat, so this is not the authority's word for it.
        SLASH_REASON_NON_RESPONSE => {
            require!(
                secs_since_heartbeat > nonresponse_limit_secs,
                ColonyError::NotSlashable
            );
            Ok(BPS_DENOM)
        }
        _ => err!(ColonyError::NotSlashable),
    }
}

/// Seize part or all of a forager's bond, split it between the incinerator and
/// the Risk Cache, and demote the forager.
///
/// Authority-only, because the rule-violation and non-response causes are
/// judged from evidence the program cannot see by itself. What the program does
/// enforce is that the authority cannot invent a cause: the loss-threshold
/// ramp is computed from the recorded drawdown, the non-response timeout is
/// measured against the recorded heartbeat, and an unrecognized reason code is
/// refused outright.
///
/// Settlement is unaffected by a slash. `settle_forager` measures
/// `(vault_balance - bond) - principal`, and this instruction lowers the vault
/// balance and `bond` by exactly the same amount, so the realized figure a
/// slash leaves behind is the same one it found. That is why no phase gate is
/// needed here: loss containment must stay available even mid-settlement.
///
/// The forager is demoted on every slash and has to earn its way back through
/// the Scout Sandbox; if what is left of the bond no longer clears `min_bond`
/// it is marked Slashed instead and cannot draw a scout ticket at all.
pub fn slash_forager(ctx: Context<SlashForager>, forager_id: u64, reason_code: u8) -> Result<()> {
    require!(
        ctx.accounts.forager.status != STATUS_RETIRED,
        ColonyError::ForagerInactive
    );

    let now = Clock::get()?.unix_timestamp;
    let nonresponse_limit_secs = (ctx.accounts.config.nonresponse_timeout_epochs as i64)
        .saturating_mul(ctx.accounts.config.epoch_duration_secs);

    let slash_bps = punitive_slash_bps(
        reason_code,
        ctx.accounts.forager.current_drawdown_bps,
        ctx.accounts.config.dd_slash_bps,
        now.saturating_sub(ctx.accounts.forager.last_heartbeat_ts),
        nonresponse_limit_secs,
    )?;

    let bond_before = ctx.accounts.forager.bond;
    let out = math::slash_split(bond_before, slash_bps, ctx.accounts.config.slash_burn_bps);
    // `slash_split` cannot exceed the bond for any `slash_bps <= 10000`, but a
    // slash that outran the posted bond would be taking depositor principal out
    // of the sub-account, so it is asserted rather than assumed.
    require!(out.slashed <= bond_before, ColonyError::SlashTooLarge);

    // -- effects ------------------------------------------------------------

    ctx.accounts.forager.bond = bond_before.saturating_sub(out.slashed);
    ctx.accounts.cache.balance = ctx
        .accounts
        .cache
        .balance
        .checked_add(out.to_cache)
        .ok_or(ColonyError::Overflow)?;
    ctx.accounts.cache.total_burned = ctx
        .accounts
        .cache
        .total_burned
        .checked_add(out.burn)
        .ok_or(ColonyError::Overflow)?;
    ctx.accounts.forager.slash_count = ctx
        .accounts
        .forager
        .slash_count
        .checked_add(1)
        .ok_or(ColonyError::Overflow)?;

    // Demotion. A slashed forager is out of the main pool either way; the
    // question is only whether it still has enough bond to scout again.
    ctx.accounts.forager.status = if ctx.accounts.forager.bond < ctx.accounts.config.min_bond {
        STATUS_SLASHED
    } else {
        STATUS_SCOUT
    };

    // Counters are deliberately untouched. `settleable_forager_count` is frozen
    // for the duration of a settlement so the crank's completion test stays
    // sound across a mid-crank demotion, and `active_forager_count` is
    // recomputed from `active_count_acc` at finalize.

    // -- interactions -------------------------------------------------------

    // The forager state PDA owns its sub-account, so it signs both legs. The
    // instruction argument is used for the seeds rather than the stored
    // `forager.forager_id`, because it is the value the account constraints
    // already validated the PDA against.
    //
    // If trading losses have already eaten into the tokens backing the bond,
    // the sub-account can hold less than `out.slashed` and the transfer fails,
    // reverting the whole instruction. That is deliberate: the settlement
    // waterfall is what writes such a loss down, and a slash must not paper
    // over a shortfall by seizing less than it reports.
    let operator_key = ctx.accounts.operator.key();
    let id_bytes = forager_id.to_le_bytes();
    let forager_bump = [ctx.accounts.forager.bump];
    let forager_seeds: &[&[u8]] = &[
        SEED_FORAGER,
        operator_key.as_ref(),
        &id_bytes,
        &forager_bump,
    ];
    let signer_seeds: &[&[&[u8]]] = &[forager_seeds];

    // Burn share -> incinerator. Economic burn: no instruction in this program
    // withdraws from that account, so the tokens are gone for good. It is not a
    // supply burn, and it reduces no token's supply.
    utils::transfer_signed(
        &ctx.accounts.token_program,
        &ctx.accounts.forager_vault,
        &ctx.accounts.incinerator_vault,
        &ctx.accounts.base_mint,
        &ctx.accounts.forager.to_account_info(),
        signer_seeds,
        out.burn,
    )?;

    // Remainder -> Risk Cache, where it offsets depositor losses. This is the
    // only half of a slash that returns any value to depositors.
    utils::transfer_signed(
        &ctx.accounts.token_program,
        &ctx.accounts.forager_vault,
        &ctx.accounts.cache_vault,
        &ctx.accounts.base_mint,
        &ctx.accounts.forager.to_account_info(),
        signer_seeds,
        out.to_cache,
    )?;

    emit!(ForagerSlashed {
        forager: ctx.accounts.forager.key(),
        operator: operator_key,
        reason_code,
        slashed: out.slashed,
        burned: out.burn,
        to_cache: out.to_cache,
        bond_after: ctx.accounts.forager.bond,
        slash_count: ctx.accounts.forager.slash_count,
    });

    Ok(())
}
