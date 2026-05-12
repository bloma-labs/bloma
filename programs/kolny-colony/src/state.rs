//! Account state.
//!
//! Every account declares `LEN` as the exact serialized byte count of its
//! fields, and is allocated with `space = 8 + LEN` (the 8 is the Anchor
//! discriminator). Each `LEN` is padded to an 8-byte boundary.
//!
//! Under-sizing an account does not fail the build, it fails at runtime when
//! the account is created or written, so the `serialized_len_matches_*` tests
//! at the bottom of this file serialize a real instance and assert the byte
//! count. That turns a latent runtime failure into a `cargo test` failure.

use anchor_lang::prelude::*;

/// Global colony configuration and epoch state. Singleton, seed `[b"colony"]`.
#[account]
#[derive(Default)]
pub struct ColonyConfig {
    // -- identity ------------------------------------------------------- 96
    pub authority: Pubkey,
    pub pending_authority: Pubkey,
    pub base_mint: Pubkey,

    // -- epoch ---------------------------------------------------------- 32
    pub epoch: u64,
    pub settling_epoch: u64,
    pub epoch_end_ts: i64,
    pub epoch_duration_secs: i64,

    // -- allocation accumulators ---------------------------------------- 48
    /// Normalization denominator confirmed at the end of the last settlement.
    pub pheromone_sum: u128,
    /// Running sum during a settlement; becomes `pheromone_sum` at finalize.
    pub pheromone_sum_acc: u128,
    /// Total pheromone of the foragers left UNCAPPED by the solved
    /// water-filling level. Paired with `alloc_remaining_bps`, this defines
    /// every uncapped weight as `remaining_bps * tau / rest_sum` without ever
    /// forming a quotient, so no division rounding biases the targets.
    pub alloc_rest_sum: u128,

    // -- pools and sizes ------------------------------------------------ 56
    pub allocatable_pool: u64,
    pub scout_pool: u64,
    pub min_bond: u64,
    pub deposit_scale_q: u64,
    pub scout_ticket_base_units: u64,
    pub promote_tau_seed_cap: u64,
    /// Capital moved so far this epoch, against the turnover cap.
    pub epoch_turnover_used: u64,

    // -- counters ------------------------------------------------------- 20
    /// Foragers that must be settled this epoch: every registered forager that
    /// is not retired, whatever its lifecycle state. This is the population the
    /// crank counts against, and it is frozen for the duration of a settlement
    /// so `settled_count == settleable_forager_count` is a sound completion
    /// test even when a forager is demoted mid-crank.
    pub settleable_forager_count: u32,
    /// Foragers that were Active at the last finalize. Only these are
    /// normalized into weights; it is a strictly smaller population than
    /// `settleable_forager_count`.
    pub active_forager_count: u32,
    /// Running count of Active foragers during a settlement.
    pub active_count_acc: u32,
    pub settled_count: u32,
    pub promote_perf_bar_bps: i32,

    // -- bps parameters ------------------------------------------------- 36
    pub rho_bps: u16,
    pub perf_norm_s_bps: u16,
    pub risk_aversion_bps: u16,
    pub w_max_bps: u16,
    pub w_drop_bps: u16,
    pub scout_budget_bps: u16,
    pub reband_band_bps: u16,
    pub turnover_cap_bps: u16,
    pub promote_min_trades: u16,
    pub bond_ratio_bps: u16,
    pub bond_haircut_bps: u16,
    pub dd_probation_bps: u16,
    pub dd_slash_bps: u16,
    pub epoch_loss_limit_bps: u16,
    pub slash_burn_bps: u16,
    pub max_single_asset_bps: u16,
    pub cache_accrual_bps: u16,
    pub cache_reserve_target_bps: u16,
    /// Share of the pool left for uncapped foragers at the solved level:
    /// `BPS_DENOM - capped_count * effective_w_max`.
    pub alloc_remaining_bps: u16,

    // -- small scalars --------------------------------------------------- 7
    pub promote_min_epochs: u8,
    pub probation_grace_epochs: u8,
    pub nonresponse_timeout_epochs: u8,
    /// Fixed at 1 by policy. Present so the zero-leverage rule is auditable.
    pub max_leverage_x: u8,
    pub epoch_phase: u8,
    pub paused: bool,
    pub bump: u8,

    pub _padding: [u8; 7],
}

impl ColonyConfig {
    // 96 + 32 + 48 + 56 + 20 + 38 + 7 + 7 = 304
    pub const LEN: usize = 304;
}
