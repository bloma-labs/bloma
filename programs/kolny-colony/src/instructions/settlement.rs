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
