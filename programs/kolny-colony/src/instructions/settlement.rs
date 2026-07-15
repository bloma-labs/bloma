//! Epoch settlement crank and capital rebalancing.
//!
//! A colony with many foragers cannot be settled inside one transaction: Solana
//! bounds both the accounts a transaction may carry and the compute one
//! instruction may spend. An epoch therefore closes as a three-phase crank.
//!
//! ```text
//!   begin_settlement      Open -> Settling. Accumulators and the trail board
//!                         are cleared, and the epoch under settlement is fixed.
//!   settle_forager        Once per forager, in any order. Each forager is
//!                         measured independently and the accumulators are
//!                         sums, so the result does not depend on the order.
//!   finalize_settlement   The normalization denominator, the water-filling
//!                         divisor and the next epoch's pools are confirmed,
//!                         the epoch counter advances and the phase reopens.
//!   rebalance_forager     Open phase only. Moves capital toward the target the
//!                         solved divisor implies, under a turnover cap.
//! ```
//!
//! Every phase is permissionless. A forager whose operator never appears must
//! still be settleable by anyone, otherwise one absent operator could stall the
//! whole colony. Ordering cannot be gamed: each forager is measured from its own
//! state, the accumulators commute, and an early finalize is refused by a count
//! guard, so a partial normalization can never be committed.
//!
//! No instruction here accepts a performance number from an operator. Every
//! input to a trail is read from chain state -- the sub-account balance, the
//! recorded bond, the recorded principal. A signed performance figure would be
//! an unsecured oracle protecting more capital than the bond behind it, which is
//! the design this program deliberately rejects.

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::*;
use crate::errors::ColonyError;
use crate::events::*;
use crate::math::{self, TOP_TRAILS_LEN};
use crate::state::*;
use crate::utils;

// ===========================================================================
// Contexts
// ===========================================================================

#[derive(Accounts)]
pub struct BeginSettlement<'info> {
    #[account(mut, seeds = [SEED_COLONY], bump = config.bump)]
    pub config: Box<Account<'info, ColonyConfig>>,

    #[account(mut, seeds = [SEED_TRAIL_BOARD], bump = trail_board.bump)]
    pub trail_board: Box<Account<'info, TrailBoard>>,

    /// Permissionless crank. Pays for the transaction and holds no privilege.
    pub cranker: Signer<'info>,
}

/// Settlement of a single forager.
///
/// Every account is boxed: this context carries eleven accounts and an unboxed
/// one would push the handler's stack frame past the 4096-byte limit.
///
/// The vaults are pinned by `has_one` rather than by re-deriving their seeds
/// here. `forager.forager_vault`, `brood.vault_base` and `cache.cache_vault`
/// were recorded when those accounts were opened, so a substituted token account
/// is rejected before a single number is read from it. That check is what stops
/// a caller from measuring a forager against a balance it does not own.
#[derive(Accounts)]
#[instruction(forager_id: u64)]
pub struct SettleForager<'info> {
    #[account(mut, seeds = [SEED_COLONY], bump = config.bump, has_one = base_mint)]
    pub config: Box<Account<'info, ColonyConfig>>,

    #[account(
        mut,
        seeds = [SEED_FORAGER, forager.operator.as_ref(), &forager_id.to_le_bytes()],
        bump = forager.bump,
        has_one = forager_vault,
    )]
    pub forager: Box<Account<'info, ForagerState>>,

    /// The forager's isolated sub-account. Holds bond plus principal.
    #[account(mut)]
    pub forager_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, seeds = [SEED_BROOD], bump = brood.bump, has_one = vault_base)]
    pub brood: Box<Account<'info, BroodVaultState>>,

    #[account(mut)]
    pub vault_base: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, seeds = [SEED_CACHE], bump = cache.bump, has_one = cache_vault)]
    pub cache: Box<Account<'info, RiskCacheState>>,

    #[account(mut)]
    pub cache_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, seeds = [SEED_TRAIL_BOARD], bump = trail_board.bump)]
    pub trail_board: Box<Account<'info, TrailBoard>>,

    pub base_mint: Box<InterfaceAccount<'info, Mint>>,
    pub token_program: Interface<'info, TokenInterface>,

    /// Permissionless crank.
    pub cranker: Signer<'info>,
}

// ===========================================================================
// Phase 1: begin
// ===========================================================================

/// Open the settlement window for the epoch that has just ended.
///
/// Callable by anyone once `epoch_end_ts` has passed. Registration and
/// retirement freeze while the phase is Settling, which is what makes
/// `settleable_forager_count` a stable completion target for the crank.
pub fn begin_settlement(ctx: Context<BeginSettlement>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;

    let config = &mut ctx.accounts.config;

    require!(config.epoch_phase == PHASE_OPEN, ColonyError::WrongPhase);
    require!(now >= config.epoch_end_ts, ColonyError::EpochNotOver);
    require!(
        config.settleable_forager_count > 0,
        ColonyError::NoActiveForagers
    );

    // Epoch numbering has to start at 1. `ForagerState::last_settled_epoch`
    // defaults to 0, and settlement admits a forager only while
    // `last_settled_epoch < settling_epoch`, so an epoch numbered 0 could never
    // settle anyone: `settled_count` would never reach the target and the
    // colony would sit in Settling forever. Lifting a zero epoch here, before
    // any forager is measured against it, removes that deadlock at the source.
    if config.epoch == 0 {
        config.epoch = 1;
    }

    config.epoch_phase = PHASE_SETTLING;
    config.settling_epoch = config.epoch;
    config.pheromone_sum_acc = 0;
    config.active_count_acc = 0;
    config.settled_count = 0;

    let settling_epoch = config.settling_epoch;
    // The population this crank must cover: every forager that is not retired.
    let settleable = config.settleable_forager_count;

    ctx.accounts.trail_board.reset();

    emit!(SettlementBegan {
        settling_epoch,
        active_forager_count: settleable,
        started_at: now,
    });

    Ok(())
}

// ===========================================================================
// Phase 2: per-forager settlement
// ===========================================================================

/// Settle one forager for the epoch under settlement.
///
/// Permissionless and idempotent per epoch. The measurement is taken entirely
/// from on-chain state; `forager_id` is the only argument, and there is
/// deliberately no parameter through which an operator could report its own
/// result.
///
/// The sub-account balance is the measurement surface, so an unsolicited
/// transfer into it is read as colony capital: it raises the forager's principal
/// and the depositor NAV that principal backs. It cannot be withdrawn by the
/// sender, which makes inflating a trail this way a donation to depositors
/// rather than an exploit, and the bond gate still caps what the resulting trail
/// can be allocated.
pub fn settle_forager(ctx: Context<SettleForager>, forager_id: u64) -> Result<()> {
    // -- phase, idempotency and lifecycle guards ----------------------------
    let settling_epoch = ctx.accounts.config.settling_epoch;

    require!(
        ctx.accounts.config.epoch_phase == PHASE_SETTLING,
        ColonyError::NotSettling
    );
    // Idempotence. Without this a caller could settle the same forager twice and
    // double-count it into both accumulators.
    require!(
        ctx.accounts.forager.last_settled_epoch < settling_epoch,
        ColonyError::AlreadySettledThisEpoch
    );
    require!(
        ctx.accounts.forager.status != STATUS_RETIRED,
        ColonyError::ForagerInactive
    );

    // -- configuration snapshot ---------------------------------------------
    // Copied out before anything is mutated, so no borrow of the config account
    // has to stay alive across a token CPI.
    let rho_bps = ctx.accounts.config.rho_bps;
    let perf_norm_s_bps = ctx.accounts.config.perf_norm_s_bps;
    let deposit_scale_q = ctx.accounts.config.deposit_scale_q;
    let risk_aversion_bps = ctx.accounts.config.risk_aversion_bps;
    let dd_probation_bps = ctx.accounts.config.dd_probation_bps;
    let probation_grace_epochs = ctx.accounts.config.probation_grace_epochs as u64;
    let cache_accrual_bps = ctx.accounts.config.cache_accrual_bps;
    let cache_reserve_target_bps = ctx.accounts.config.cache_reserve_target_bps;
    let w_max_bps = ctx.accounts.config.w_max_bps;
    let w_drop_bps = ctx.accounts.config.w_drop_bps;
    let active_forager_count = ctx.accounts.config.active_forager_count;
    let level = math::CapLevel {
        capped_count: 0,
        remaining_bps: ctx.accounts.config.alloc_remaining_bps,
        rest_sum: ctx.accounts.config.alloc_rest_sum,
    };

    let forager_key = ctx.accounts.forager.key();

    // -- realized measurement, with the bond excluded -----------------------
    //
    // The sub-account holds bond + principal. Subtracting the bond before the
    // balance is compared against principal is the single most important line in
    // this instruction: without it a bond top-up would read as trading profit
    // and an operator could inflate its own trail with capital it still owns.
    let vault_balance = ctx.accounts.forager_vault.amount;
    let bond = ctx.accounts.forager.bond;
    let old_principal = ctx.accounts.forager.principal;

    let equity = vault_balance.saturating_sub(bond);
    let realized = math::realized_epoch_pnl(vault_balance, bond, old_principal);

    // -- drawdown, from the observable equity high-water --------------------
    //
    // Measured from realized equity that actually sits in the sub-account, never
    // from a figure an operator supplies and never from a mark on an open
    // position.
    let high_water = ctx.accounts.forager.high_water.max(equity);
    let drawdown_bps: u16 = if high_water == 0 {
        0
    } else {
        (((high_water - equity) as u128) * math::BPS_DENOM / (high_water as u128))
            .min(math::BPS_DENOM) as u16
    };

    // -- trail update -------------------------------------------------------
    let return_bps = math::return_bps(realized, old_principal);
    let perf_bps = math::risk_adjusted_perf_bps(return_bps, drawdown_bps, risk_aversion_bps);
    let deposit = math::deposit_fp6(perf_bps, perf_norm_s_bps, deposit_scale_q);
    let pheromone = math::update_pheromone(
        ctx.accounts.forager.pheromone,
        deposit,
        rho_bps,
        PHEROMONE_CEIL,
    );

    {
        let forager = &mut ctx.accounts.forager;
        forager.high_water = high_water;
        forager.current_drawdown_bps = drawdown_bps;
        if drawdown_bps > forager.max_drawdown_bps {
            forager.max_drawdown_bps = drawdown_bps;
        }
        forager.pheromone = pheromone;
        // The colony's claim on this sub-account is now its equity, whatever the
        // epoch did to it.
        forager.principal = equity;
        forager.realized_pnl_cumulative = forager
            .realized_pnl_cumulative
            .checked_add(realized)
            .ok_or(ColonyError::Overflow)?;

        if realized != 0 {
            forager.realized_epochs = forager
                .realized_epochs
                .checked_add(1)
                .ok_or(ColonyError::Overflow)?;
        }
        if forager.status == STATUS_SCOUT {
            forager.scout_epochs = forager
                .scout_epochs
                .checked_add(1)
                .ok_or(ColonyError::Overflow)?;
            forager.scout_perf_cum_bps = forager
                .scout_perf_cum_bps
                .checked_add(perf_bps)
                .ok_or(ColonyError::Overflow)?;
        }
    }

    // The vault's outstanding claim follows the same number.
    {
        let brood = &mut ctx.accounts.brood;
        brood.outstanding_principal = brood
            .outstanding_principal
            .saturating_sub(old_principal)
            .checked_add(equity)
            .ok_or(ColonyError::Overflow)?;
    }

    // -- PDA signers for the settlement transfers ---------------------------
    let operator = ctx.accounts.forager.operator;
    let forager_bump = ctx.accounts.forager.bump;
    let cache_bump = ctx.accounts.cache.bump;
    let id_le = forager_id.to_le_bytes();

    let forager_seeds: &[&[u8]] = &[
        SEED_FORAGER,
        operator.as_ref(),
        id_le.as_ref(),
        &[forager_bump],
    ];
    let forager_signer: &[&[&[u8]]] = &[forager_seeds];

    let cache_seeds: &[&[u8]] = &[SEED_CACHE, &[cache_bump]];
    let cache_signer: &[&[&[u8]]] = &[cache_seeds];

    let forager_authority = ctx.accounts.forager.to_account_info();
    let cache_authority = ctx.accounts.cache.to_account_info();

    if realized < 0 {
        // -- loss waterfall (risk-spec 4.2) ---------------------------------
        //
        // Step one already happened: the sub-account took the hit, which is what
        // produced this number. What remains is bond, then Risk Cache, then
        // depositor NAV. Step three is the honest limit -- the cushion is finite
        // and whatever it cannot absorb reduces share value.
        let loss = realized.unsigned_abs();
        let outcome = math::cover_loss(loss, bond, ctx.accounts.cache.balance);

        // Bond share: out of the forager's sub-account, into the vault.
        utils::transfer_signed(
            &ctx.accounts.token_program,
            &ctx.accounts.forager_vault,
            &ctx.accounts.vault_base,
            &ctx.accounts.base_mint,
            &forager_authority,
            forager_signer,
            outcome.from_bond,
        )?;
        ctx.accounts.forager.bond = bond.saturating_sub(outcome.from_bond);

        // Cache share: out of the insurance cache, into the vault.
        utils::transfer_signed(
            &ctx.accounts.token_program,
            &ctx.accounts.cache_vault,
            &ctx.accounts.vault_base,
            &ctx.accounts.base_mint,
            &cache_authority,
            cache_signer,
            outcome.from_cache,
        )?;
        {
            let cache = &mut ctx.accounts.cache;
            cache.balance = cache.balance.saturating_sub(outcome.from_cache);
            cache.total_covered = cache
                .total_covered
                .checked_add(outcome.from_cache)
                .ok_or(ColonyError::Overflow)?;
        }

        {
            let reimbursed = outcome
                .from_bond
                .checked_add(outcome.from_cache)
                .ok_or(ColonyError::Overflow)?;
            let brood = &mut ctx.accounts.brood;
            brood.idle_base = brood
                .idle_base
                .checked_add(reimbursed)
                .ok_or(ColonyError::Overflow)?;
        }

        emit!(LossCovered {
            forager: forager_key,
            loss,
            from_bond: outcome.from_bond,
            from_cache: outcome.from_cache,
            to_depositors: outcome.to_vault_loss,
        });

        // Probation (risk-spec 1.3). A milder drawdown freezes new allocation
        // for a grace window instead of seizing anything; the forager either
        // recovers inside it or becomes slashable.
        {
            let forager = &mut ctx.accounts.forager;
            if forager.status == STATUS_ACTIVE && drawdown_bps >= dd_probation_bps {
                forager.status = STATUS_PROBATION;
                forager.probation_until_epoch = settling_epoch
                    .checked_add(probation_grace_epochs)
                    .ok_or(ColonyError::Overflow)?;
            }
        }
    } else if realized > 0 {
        // -- cache accrual (risk-spec 4.1 and 4.3) ---------------------------
        //
        // A cut of realized profit builds the reserve, but only while the cache
        // is under its target. Above target nothing is taken.
        let nav = ctx.accounts.brood.nav();
        let reserve_target =
            ((nav as u128) * (cache_reserve_target_bps as u128) / math::BPS_DENOM) as u64;

        if ctx.accounts.cache.balance < reserve_target {
            let accrual = math::cache_accrual(realized as u64, cache_accrual_bps)
                .min(ctx.accounts.forager.principal);

            if accrual > 0 {
                utils::transfer_signed(
                    &ctx.accounts.token_program,
                    &ctx.accounts.forager_vault,
                    &ctx.accounts.cache_vault,
                    &ctx.accounts.base_mint,
                    &forager_authority,
                    forager_signer,
                    accrual,
                )?;

                let cache_balance = {
                    let cache = &mut ctx.accounts.cache;
                    cache.balance = cache
                        .balance
                        .checked_add(accrual)
                        .ok_or(ColonyError::Overflow)?;
                    cache.total_accrued = cache
                        .total_accrued
                        .checked_add(accrual)
                        .ok_or(ColonyError::Overflow)?;
                    cache.balance
                };

                {
                    let forager = &mut ctx.accounts.forager;
                    let before = forager.principal;
                    forager.principal = before.saturating_sub(accrual);
                    // The accrual leaves the sub-account for the cache, which is
                    // not depositor NAV. Rebase the high-water mark on the same
                    // move so the outflow is not read as a drawdown next epoch.
                    forager.high_water =
                        rescale_high_water(forager.high_water, before, forager.principal);
                }

                {
                    let brood = &mut ctx.accounts.brood;
                    brood.outstanding_principal =
                        brood.outstanding_principal.saturating_sub(accrual);
                }

                emit!(CacheAccrued {
                    epoch: settling_epoch,
                    amount: accrual,
                    cache_balance,
                });
            }
        }
    }

    // -- probation recovery (risk-spec 1.3) ----------------------------------
    //
    // Probation is a grace window, not a sentence. A forager that pulled its
    // drawdown back under the probation threshold returns to Active and can be
    // allocated to again. Without this, probation would be a one-way trap: a
    // probationary forager can only give capital back through
    // `rebalance_forager`, and `promote_forager` admits Scouts only, so one bad
    // epoch would sideline an operator permanently.
    //
    // Failing to recover inside the window does NOT auto-slash here. A slash is
    // an authority decision taken through `slash_forager`, which re-derives the
    // condition from chain state rather than trusting a flag set earlier.
    {
        let forager = &mut ctx.accounts.forager;
        if forager.status == STATUS_PROBATION && drawdown_bps < dd_probation_bps {
            forager.status = STATUS_ACTIVE;
            forager.probation_until_epoch = 0;
        }
    }

    // -- demotion (allocation-spec 7, step 3) --------------------------------
    //
    // Weighed against the divisor solved at the previous finalize. A trail that
    // has decayed under `w_drop` is removed from the main pool rather than
    // propped up at a minimum weight: dropping it is the point of the mechanism.
    // A demoted forager is not accumulated, so the weight it held is freed and
    // the surviving trails renormalize over it at the next finalize.
    let eff_max_weight_bps =
        math::effective_max_weight_bps(w_max_bps, active_forager_count, CAP_RELAX_MARGIN_BPS);
    let weight_bps = math::weight_bps_from_level(pheromone, &level, eff_max_weight_bps);

    let mut counts_active = ctx.accounts.forager.status == STATUS_ACTIVE;
    if counts_active && weight_bps < w_drop_bps {
        ctx.accounts.forager.status = STATUS_SCOUT;
        counts_active = false;

        emit!(ForagerDemoted {
            forager: forager_key,
            epoch: settling_epoch,
            weight_bps,
            // Capital is not moved here. It returns through rebalance_forager,
            // which is the only path that touches the vaults in the Open phase.
            returned_principal: 0,
        });
    }

    if counts_active {
        {
            let config = &mut ctx.accounts.config;
            config.pheromone_sum_acc = config
                .pheromone_sum_acc
                .checked_add(pheromone as u128)
                .ok_or(ColonyError::Overflow)?;
            config.active_count_acc = config
                .active_count_acc
                .checked_add(1)
                .ok_or(ColonyError::Overflow)?;
        }

        // Only the largest trails can sit at the concentration cap, so keeping
        // the top slots is enough to solve the water-filling level exactly at
        // finalize without a second pass over every forager.
        let board = &mut ctx.accounts.trail_board;
        let count = board.count;
        board.count = math::top_insert(&mut board.top, count, pheromone);
    }

    {
        let config = &mut ctx.accounts.config;
        config.settled_count = config
            .settled_count
            .checked_add(1)
            .ok_or(ColonyError::Overflow)?;
    }
    ctx.accounts.forager.last_settled_epoch = settling_epoch;

    let principal_after = ctx.accounts.forager.principal;

    emit!(ForagerSettled {
        forager: forager_key,
        settling_epoch,
        realized_pnl_epoch: realized,
        return_bps,
        perf_bps,
        deposit_fp6: deposit,
        pheromone,
        principal_after,
        drawdown_bps,
    });

    Ok(())
}

// ===========================================================================
// Helpers
// ===========================================================================

/// Rebase the equity high-water mark across a capital flow.
///
/// Drawdown is `(high_water - equity) / high_water`, and equity moves for two
/// unrelated reasons: what the forager did, and capital the colony added or took
/// back. Scaling the mark with principal removes the second, so the reported
/// drawdown stays a statement about performance. A forager rebased from zero
/// principal starts a fresh mark; its all-time maximum is retained separately in
/// `max_drawdown_bps` and is never lowered here.
fn rescale_high_water(high_water: u64, principal_before: u64, principal_after: u64) -> u64 {
    if principal_before == 0 {
        return principal_after;
    }
    (((high_water as u128) * (principal_after as u128)) / (principal_before as u128))
        .min(u64::MAX as u128) as u64
}
