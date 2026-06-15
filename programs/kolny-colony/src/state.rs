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

use crate::math::TOP_TRAILS_LEN;

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

/// One forager: an operator-run trail the colony can send capital down.
/// Seed `[b"forager", operator, forager_id:u64 LE]`.
#[account]
#[derive(Default)]
pub struct ForagerState {
    // -- identity ------------------------------------------------------- 64
    pub operator: Pubkey,
    pub forager_vault: Pubkey,

    // -- accounting ----------------------------------------------------- 80
    pub forager_id: u64,
    /// Posted bond. Held inside `forager_vault` but accounted separately, so
    /// settlement can subtract it before measuring performance.
    pub bond: u64,
    /// Colony capital currently deployed to this forager.
    pub principal: u64,
    /// Trail strength, FP6.
    pub pheromone: u64,
    /// Highest principal-relative equity seen, for drawdown.
    pub high_water: u64,
    pub last_settled_epoch: u64,
    pub registered_epoch: u64,
    /// Epochs this forager closed with a non-zero realized result. Named for
    /// what it actually counts: the program cannot observe individual trades,
    /// only settled epoch outcomes, so it does not claim to be a trade count.
    pub realized_epochs: u64,
    pub scout_epochs: u64,
    pub last_scout_epoch: u64,

    // -- signed / temporal ---------------------------------------------- 32
    pub realized_pnl_cumulative: i64,
    /// Cumulative risk-adjusted realized performance, for the promotion bar.
    pub scout_perf_cum_bps: i64,
    pub probation_until_epoch: u64,
    pub last_heartbeat_ts: i64,

    // -- opaque strategy tag -------------------------------------------- 32
    pub strategy_meta: [u8; 32],

    // -- bps ------------------------------------------------------------- 6
    pub max_drawdown_bps: u16,
    pub current_drawdown_bps: u16,
    pub slash_count: u16,

    // -- small ----------------------------------------------------------- 3
    pub status: u8,
    pub bump: u8,
    pub vault_bump: u8,

    pub _padding: [u8; 7],
}

impl ForagerState {
    // 64 + 80 + 32 + 32 + 6 + 3 + 7 = 224
    pub const LEN: usize = 224;
}

/// Brood Vault share accounting. Singleton, seed `[b"brood"]`.
///
/// `nav = idle_base + outstanding_principal`. Both are accounting counters that
/// only move through credited deposits, withdrawals and settlement, never by
/// reading a live token balance. That is what makes an unsolicited transfer
/// into a vault account unable to move the share price.
#[account]
#[derive(Default)]
pub struct BroodVaultState {
    pub base_mint: Pubkey,
    pub vault_base: Pubkey,

    pub total_shares: u128,
    pub pending_redemption_shares: u128,

    pub idle_base: u64,
    pub outstanding_principal: u64,
    pub next_redemption_id: u64,

    pub bump: u8,
    pub vault_bump: u8,
    pub _padding: [u8; 6],
}

impl BroodVaultState {
    // 64 + 32 + 24 + 8 = 128
    pub const LEN: usize = 128;

    /// Net asset value. Realized only: an open position that has not been
    /// closed and settled contributes nothing here.
    pub fn nav(&self) -> u64 {
        self.idle_base.saturating_add(self.outstanding_principal)
    }

    /// Capital the concentration cap left unplaceable, which stays in the
    /// vault rather than being forced into an over-concentrated position.
    ///
    /// Derived rather than stored. A stored copy would have to be recomputed
    /// on every rebalance to stay true, and a stale one published as a reserve
    /// figure would be worse than no figure at all.
    pub fn undeployed_reserve(&self, allocatable_pool: u64) -> u64 {
        allocatable_pool.saturating_sub(self.outstanding_principal)
    }
}

/// A depositor's share balance. Seed `[b"position", depositor]`.
#[account]
#[derive(Default)]
pub struct DepositorPosition {
    pub owner: Pubkey,
    pub shares: u128,
    pub bump: u8,
    pub _padding: [u8; 7],
}

impl DepositorPosition {
    // 32 + 16 + 1 + 7 = 56
    pub const LEN: usize = 56;
}

/// Insurance cache. Singleton, seed `[b"cache"]`.
#[account]
#[derive(Default)]
pub struct RiskCacheState {
    pub cache_vault: Pubkey,
    /// Locked sink. No instruction ever withdraws from it.
    pub incinerator_vault: Pubkey,

    pub balance: u64,
    pub total_covered: u64,
    pub total_burned: u64,
    pub total_accrued: u64,

    pub bump: u8,
    pub vault_bump: u8,
    pub incinerator_bump: u8,
    pub _padding: [u8; 5],
}

impl RiskCacheState {
    // 64 + 32 + 3 + 5 = 104
    pub const LEN: usize = 104;
}

/// A queued redemption. Seed `[b"redeem", depositor, request_id:u64 LE]`.
///
/// The program cannot force-liquidate capital that is already deployed to a
/// forager, so a withdrawal larger than idle liquidity queues here instead of
/// promising an immediate payout it cannot honor.
#[account]
#[derive(Default)]
pub struct RedemptionRequest {
    pub owner: Pubkey,
    pub shares: u128,
    pub request_id: u64,
    pub requested_epoch: u64,
    pub assets_paid: u64,
    pub bump: u8,
    pub _padding: [u8; 7],
}

impl RedemptionRequest {
    // 32 + 16 + 24 + 1 + 7 = 80
    pub const LEN: usize = 80;
}

/// The largest trails seen during the current settlement.
/// Singleton, seed `[b"trail_board"]`.
///
/// Only the largest `floor(1 / w_max)` trails can ever sit at the concentration
/// cap, so keeping the top `TOP_TRAILS_LEN` values is enough to solve the
/// water-filling level exactly at finalize, without a pass over every forager.
#[account]
pub struct TrailBoard {
    pub top: [u64; TOP_TRAILS_LEN],
    pub count: u8,
    pub bump: u8,
    pub _padding: [u8; 6],
}

impl Default for TrailBoard {
    fn default() -> Self {
        Self {
            top: [0u64; TOP_TRAILS_LEN],
            count: 0,
            bump: 0,
            _padding: [0u8; 6],
        }
    }
}

impl TrailBoard {
    // 21*8 + 1 + 1 + 6 = 176
    pub const LEN: usize = 8 * TOP_TRAILS_LEN + 8;

    pub fn reset(&mut self) {
        self.top = [0u64; TOP_TRAILS_LEN];
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::AnchorSerialize;

    /// The serialized length of a default instance must equal `LEN` exactly.
    /// Too small and account creation fails at runtime; too large and rent is
    /// wasted. Either way this catches it before a validator ever sees it.
    macro_rules! assert_len {
        ($t:ty, $name:literal) => {{
            let v = <$t>::default();
            let bytes = v.try_to_vec().unwrap();
            assert_eq!(
                bytes.len(),
                <$t>::LEN,
                "{} serialized to {} bytes but LEN is {}",
                $name,
                bytes.len(),
                <$t>::LEN
            );
            assert_eq!(
                <$t>::LEN % 8,
                0,
                "{} LEN {} is not 8-byte aligned",
                $name,
                <$t>::LEN
            );
        }};
    }

    #[test]
    fn serialized_len_matches_colony_config() {
        assert_len!(ColonyConfig, "ColonyConfig");
        assert_eq!(ColonyConfig::LEN, 304);
    }

    #[test]
    fn serialized_len_matches_forager_state() {
        assert_len!(ForagerState, "ForagerState");
        assert_eq!(ForagerState::LEN, 224);
    }

    #[test]
    fn serialized_len_matches_brood_vault_state() {
        assert_len!(BroodVaultState, "BroodVaultState");
        assert_eq!(BroodVaultState::LEN, 128);
    }

    #[test]
    fn serialized_len_matches_depositor_position() {
        assert_len!(DepositorPosition, "DepositorPosition");
        assert_eq!(DepositorPosition::LEN, 56);
    }

    #[test]
    fn serialized_len_matches_risk_cache_state() {
        assert_len!(RiskCacheState, "RiskCacheState");
        assert_eq!(RiskCacheState::LEN, 104);
    }

    #[test]
    fn serialized_len_matches_redemption_request() {
        assert_len!(RedemptionRequest, "RedemptionRequest");
        assert_eq!(RedemptionRequest::LEN, 80);
    }

    #[test]
    fn serialized_len_matches_trail_board() {
        assert_len!(TrailBoard, "TrailBoard");
        assert_eq!(TrailBoard::LEN, 176);
    }

    #[test]
    fn nav_is_idle_plus_outstanding() {
        let mut b = BroodVaultState::default();
        b.idle_base = 400;
        b.outstanding_principal = 600;
        assert_eq!(b.nav(), 1_000);
    }

    #[test]
    fn trail_board_holds_enough_slots_for_the_tightest_cap() {
        // The minimum allowed w_max is 500 bps, so at most 20 trails can be
        // capped and the board needs 21 slots to see the first uncapped one.
        assert!(TOP_TRAILS_LEN >= (10_000 / crate::constants::MIN_W_MAX_BPS as usize) + 1);
    }
}
