//! Tuning parameters for the colony.
//!
//! Defaults and ranges are taken from the parameter tables in
//! `docs/allocation-spec.md` section 11 and `docs/risk-spec.md` section 7.
//! Those tables are the source of truth; nothing here may drift from them.
//!
//! Every mutable parameter is range-checked on `initialize_colony` and on every
//! `update_config`, so an authority cannot move the colony outside the published
//! bounds even by mistake.

pub const BPS_DENOM: u16 = 10_000;

/// PDA seeds. These strings are the contract between the program, the indexer
/// and the front end; the README carries the same table and the two must match
/// character for character.
pub const SEED_COLONY: &[u8] = b"colony";
pub const SEED_BROOD: &[u8] = b"brood";
pub const SEED_CACHE: &[u8] = b"cache";
pub const SEED_TRAIL_BOARD: &[u8] = b"trail_board";
pub const SEED_FORAGER: &[u8] = b"forager";
pub const SEED_FORAGER_VAULT: &[u8] = b"forager_vault";
pub const SEED_BROOD_VAULT: &[u8] = b"brood_vault";
pub const SEED_CACHE_VAULT: &[u8] = b"cache_vault";
pub const SEED_INCINERATOR: &[u8] = b"incinerator";
pub const SEED_POSITION: &[u8] = b"position";
pub const SEED_REDEEM: &[u8] = b"redeem";

// ---------------------------------------------------------------------------
// Epoch  (allocation-spec 11: epoch_duration_secs)
// ---------------------------------------------------------------------------

/// 7 days.
pub const DEFAULT_EPOCH_DURATION_SECS: i64 = 604_800;
/// 1 day.
pub const MIN_EPOCH_DURATION_SECS: i64 = 86_400;
/// 30 days.
pub const MAX_EPOCH_DURATION_SECS: i64 = 2_592_000;

// ---------------------------------------------------------------------------
// Pheromone update  (allocation-spec 11)
// ---------------------------------------------------------------------------

/// Evaporation rate. 1600 = 0.16, a half-life of about 4 epochs.
pub const DEFAULT_RHO_BPS: u16 = 1_600;
pub const MIN_RHO_BPS: u16 = 100;
pub const MAX_RHO_BPS: u16 = 5_000;

/// Deposit scale `Q` in FP6. The largest change one epoch can make to a trail.
pub const DEFAULT_DEPOSIT_SCALE_Q: u64 = 1_000_000;
pub const MIN_DEPOSIT_SCALE_Q: u64 = 100_000;
pub const MAX_DEPOSIT_SCALE_Q: u64 = 10_000_000;

/// Performance normalization scale `s`. 1000 = 10%.
pub const DEFAULT_PERF_NORM_S_BPS: u16 = 1_000;
pub const MIN_PERF_NORM_S_BPS: u16 = 100;
pub const MAX_PERF_NORM_S_BPS: u16 = 5_000;

/// Risk aversion `lambda`. 10000 = 1.0x drawdown penalty.
pub const DEFAULT_RISK_AVERSION_BPS: u16 = 10_000;
pub const MIN_RISK_AVERSION_BPS: u16 = 0;
pub const MAX_RISK_AVERSION_BPS: u16 = 50_000;

/// Fixed-point scale for pheromone and deposits.
pub const FP6_SCALE: u64 = 1_000_000;

/// Pheromone floor. MUST stay 0 so a dead trail can actually evaporate away.
pub const PHEROMONE_FLOOR: u64 = 0;
/// Overflow bound on a trail, far above any economically reachable value
/// (steady state is `Q / rho`, about 6.25e6 at the defaults).
pub const PHEROMONE_CEIL: u64 = 1_000_000_000_000;
