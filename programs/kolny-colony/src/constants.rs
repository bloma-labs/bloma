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

// ---------------------------------------------------------------------------
// Weights  (allocation-spec 11)
// ---------------------------------------------------------------------------

/// Single-forager concentration cap. 2000 = 20%.
pub const DEFAULT_W_MAX_BPS: u16 = 2_000;
pub const MIN_W_MAX_BPS: u16 = 500;
pub const MAX_W_MAX_BPS: u16 = 4_000;

/// Demotion threshold, not a floor. 300 = 3%.
pub const DEFAULT_W_DROP_BPS: u16 = 300;
pub const MIN_W_DROP_BPS: u16 = 0;
pub const MAX_W_DROP_BPS: u16 = 1_000;

/// Margin added when the cap has to relax because `n_active * w_max < 1`.
pub const CAP_RELAX_MARGIN_BPS: u16 = 100;

/// Exploration reserve. 1000 = 10% of value under colony.
pub const DEFAULT_SCOUT_BUDGET_BPS: u16 = 1_000;
pub const MIN_SCOUT_BUDGET_BPS: u16 = 500;
pub const MAX_SCOUT_BUDGET_BPS: u16 = 2_000;

/// No-trade band. 200 = 2%.
pub const DEFAULT_REBAND_BAND_BPS: u16 = 200;
pub const MIN_REBAND_BAND_BPS: u16 = 0;
pub const MAX_REBAND_BAND_BPS: u16 = 1_000;

/// Turnover cap. 2500 = 25% of the pool per epoch.
pub const DEFAULT_TURNOVER_CAP_BPS: u16 = 2_500;
pub const MIN_TURNOVER_CAP_BPS: u16 = 500;
pub const MAX_TURNOVER_CAP_BPS: u16 = 10_000;

// ---------------------------------------------------------------------------
// Scout promotion  (allocation-spec 11)
// ---------------------------------------------------------------------------

pub const DEFAULT_PROMOTE_MIN_EPOCHS: u8 = 4;
pub const MIN_PROMOTE_MIN_EPOCHS: u8 = 1;
pub const MAX_PROMOTE_MIN_EPOCHS: u8 = 52;

pub const DEFAULT_PROMOTE_MIN_TRADES: u16 = 20;
pub const MIN_PROMOTE_MIN_TRADES: u16 = 1;
pub const MAX_PROMOTE_MIN_TRADES: u16 = 1_000;

pub const DEFAULT_PROMOTE_PERF_BAR_BPS: i32 = 0;
pub const MIN_PROMOTE_PERF_BAR_BPS: i32 = -5_000;
pub const MAX_PROMOTE_PERF_BAR_BPS: i32 = 20_000;

/// Cap on the pheromone a scout carries into the main pool at promotion, so it
/// cannot enter at the top of the trail. FP6.
pub const DEFAULT_PROMOTE_TAU_SEED_CAP: u64 = 1_000_000;
pub const MAX_PROMOTE_TAU_SEED_CAP: u64 = 5_000_000;

// ---------------------------------------------------------------------------
// Bond and slashing  (risk-spec 7)
// ---------------------------------------------------------------------------

/// Bond as a fraction of allocation. 1000 = 10%.
pub const DEFAULT_BOND_RATIO_BPS: u16 = 1_000;
pub const MIN_BOND_RATIO_BPS: u16 = 500;
pub const MAX_BOND_RATIO_BPS: u16 = 5_000;

/// Over-collateralization against collateral price swings. 3000 = 30%.
pub const DEFAULT_BOND_HAIRCUT_BPS: u16 = 3_000;
pub const MIN_BOND_HAIRCUT_BPS: u16 = 1_000;
pub const MAX_BOND_HAIRCUT_BPS: u16 = 7_000;

/// Current drawdown that puts a forager on probation. 1500 = 15%.
pub const DEFAULT_DD_PROBATION_BPS: u16 = 1_500;
pub const MIN_DD_PROBATION_BPS: u16 = 500;
pub const MAX_DD_PROBATION_BPS: u16 = 4_000;

/// Current drawdown that makes a forager slashable. 3000 = 30%.
pub const DEFAULT_DD_SLASH_BPS: u16 = 3_000;
pub const MIN_DD_SLASH_BPS: u16 = 1_000;
pub const MAX_DD_SLASH_BPS: u16 = 6_000;

/// Per-epoch realized loss that pauses a forager. 1000 = 10%.
pub const DEFAULT_EPOCH_LOSS_LIMIT_BPS: u16 = 1_000;
pub const MIN_EPOCH_LOSS_LIMIT_BPS: u16 = 300;
pub const MAX_EPOCH_LOSS_LIMIT_BPS: u16 = 3_000;

pub const DEFAULT_PROBATION_GRACE_EPOCHS: u8 = 1;
pub const MIN_PROBATION_GRACE_EPOCHS: u8 = 1;
pub const MAX_PROBATION_GRACE_EPOCHS: u8 = 8;

pub const DEFAULT_NONRESPONSE_TIMEOUT_EPOCHS: u8 = 2;
pub const MIN_NONRESPONSE_TIMEOUT_EPOCHS: u8 = 1;
pub const MAX_NONRESPONSE_TIMEOUT_EPOCHS: u8 = 8;

/// Share of a seized bond that is burned; the rest goes to the Risk Cache.
pub const DEFAULT_SLASH_BURN_BPS: u16 = 5_000;
pub const MIN_SLASH_BURN_BPS: u16 = 0;
pub const MAX_SLASH_BURN_BPS: u16 = 10_000;
