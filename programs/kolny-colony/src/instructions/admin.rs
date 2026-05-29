//! Colony administration.
//!
//! Three jobs live here and nothing else: setting the tuning parameters, moving
//! the authority, and creating the singleton accounts once at genesis. No
//! instruction in this file moves a token.
//!
//! Every adjustable parameter is range-checked against the published bounds in
//! `constants.rs`, which are transcribed from the tables in
//! `docs/allocation-spec.md` section 11 and `docs/risk-spec.md` section 7. The
//! check runs through a single gate, `InitColonyParams::validate`, that both
//! `initialize_colony` and `update_config` call. Writing the same 27 bounds
//! twice is how the two paths eventually disagree, and an authority that has to
//! respect a bound at genesis but can step outside it afterwards makes the
//! published ranges worthless.
//!
//! Account creation is split across several instructions on purpose. A context
//! carrying more than one `init` generates enough stack frame to breach the
//! 4096-byte limit, so each singleton and each token account gets its own
//! instruction; a client bundles them into one transaction and the operator
//! experience is unchanged.

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::*;
use crate::errors::ColonyError;
use crate::events::*;
use crate::state::*;

// ---------------------------------------------------------------------------
// Parameter payloads
// ---------------------------------------------------------------------------

/// Every parameter an authority may set at genesis.
///
/// What is deliberately absent is as much a part of the contract as what is
/// present. `max_leverage_x` is pinned to `MAX_LEVERAGE_X` by the program and
/// is not settable here; neither are the fixed-point scale, the pheromone floor
/// and ceiling, the minimum deposit, or any epoch counter. An authority tunes
/// the colony, it does not redefine it.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct InitColonyParams {
    // -- epoch ---------------------------------------------------------------
    pub epoch_duration_secs: i64,

    // -- pheromone update ----------------------------------------------------
    pub rho_bps: u16,
    pub deposit_scale_q: u64,
    pub perf_norm_s_bps: u16,
    pub risk_aversion_bps: u16,

    // -- weights -------------------------------------------------------------
    pub w_max_bps: u16,
    pub w_drop_bps: u16,
    pub scout_budget_bps: u16,
    pub reband_band_bps: u16,
    pub turnover_cap_bps: u16,

    // -- scout promotion -----------------------------------------------------
    pub promote_min_epochs: u8,
    pub promote_min_trades: u16,
    pub promote_perf_bar_bps: i32,
    pub scout_ticket_base_units: u64,
    pub promote_tau_seed_cap: u64,

    // -- bond and slashing ---------------------------------------------------
    pub min_bond: u64,
    pub bond_ratio_bps: u16,
    pub bond_haircut_bps: u16,
    pub dd_probation_bps: u16,
    pub dd_slash_bps: u16,
    pub epoch_loss_limit_bps: u16,
    pub probation_grace_epochs: u8,
    pub nonresponse_timeout_epochs: u8,
    pub slash_burn_bps: u16,
    pub max_single_asset_bps: u16,

    // -- risk cache ----------------------------------------------------------
    pub cache_accrual_bps: u16,
    pub cache_reserve_target_bps: u16,
}

/// Inclusive `[min, max]` bound check against the published range.
macro_rules! bounded {
    ($value:expr, $min:expr, $max:expr) => {
        require!(
            $value >= $min && $value <= $max,
            ColonyError::ParamOutOfRange
        )
    };
}

impl InitColonyParams {
    /// The one range gate in the program.
    ///
    /// `initialize_colony` runs it on the caller's parameters; `update_config`
    /// runs it on the merged result of the current configuration and the patch,
    /// so a patched field is checked exactly as it would be at genesis and an
    /// untouched field is re-confirmed rather than trusted.
    pub fn validate(&self) -> Result<()> {
        // -- epoch -----------------------------------------------------------
        bounded!(
            self.epoch_duration_secs,
            MIN_EPOCH_DURATION_SECS,
            MAX_EPOCH_DURATION_SECS
        );

        // -- pheromone update ------------------------------------------------
        bounded!(self.rho_bps, MIN_RHO_BPS, MAX_RHO_BPS);
        bounded!(
            self.deposit_scale_q,
            MIN_DEPOSIT_SCALE_Q,
            MAX_DEPOSIT_SCALE_Q
        );
        bounded!(
            self.perf_norm_s_bps,
            MIN_PERF_NORM_S_BPS,
            MAX_PERF_NORM_S_BPS
        );
        bounded!(
            self.risk_aversion_bps,
            MIN_RISK_AVERSION_BPS,
            MAX_RISK_AVERSION_BPS
        );

        // -- weights ---------------------------------------------------------
        bounded!(self.w_max_bps, MIN_W_MAX_BPS, MAX_W_MAX_BPS);
        bounded!(self.w_drop_bps, MIN_W_DROP_BPS, MAX_W_DROP_BPS);
        bounded!(
            self.scout_budget_bps,
            MIN_SCOUT_BUDGET_BPS,
            MAX_SCOUT_BUDGET_BPS
        );
        bounded!(
            self.reband_band_bps,
            MIN_REBAND_BAND_BPS,
            MAX_REBAND_BAND_BPS
        );
        bounded!(
            self.turnover_cap_bps,
            MIN_TURNOVER_CAP_BPS,
            MAX_TURNOVER_CAP_BPS
        );

        // -- scout promotion -------------------------------------------------
        bounded!(
            self.promote_min_epochs,
            MIN_PROMOTE_MIN_EPOCHS,
            MAX_PROMOTE_MIN_EPOCHS
        );
        bounded!(
            self.promote_min_trades,
            MIN_PROMOTE_MIN_TRADES,
            MAX_PROMOTE_MIN_TRADES
        );
        bounded!(
            self.promote_perf_bar_bps,
            MIN_PROMOTE_PERF_BAR_BPS,
            MAX_PROMOTE_PERF_BAR_BPS
        );
        // Project-set, spec range "> 0". A zero ticket would fund a scout with
        // nothing and let it collect scout epochs toward promotion for free.
        require!(
            self.scout_ticket_base_units > 0,
            ColonyError::ParamOutOfRange
        );
        // Upper bound only; a zero seed is legitimate and means a promoted
        // scout starts its trail from nothing.
        require!(
            self.promote_tau_seed_cap <= MAX_PROMOTE_TAU_SEED_CAP,
            ColonyError::ParamOutOfRange
        );

        // -- bond and slashing -----------------------------------------------
        // Project-set, spec range "> 0". A zero floor bond would let a forager
        // take colony capital with nothing at stake.
        require!(self.min_bond > 0, ColonyError::ParamOutOfRange);
        bounded!(self.bond_ratio_bps, MIN_BOND_RATIO_BPS, MAX_BOND_RATIO_BPS);
        bounded!(
            self.bond_haircut_bps,
            MIN_BOND_HAIRCUT_BPS,
            MAX_BOND_HAIRCUT_BPS
        );
        bounded!(
            self.dd_probation_bps,
            MIN_DD_PROBATION_BPS,
            MAX_DD_PROBATION_BPS
        );
        bounded!(self.dd_slash_bps, MIN_DD_SLASH_BPS, MAX_DD_SLASH_BPS);
        bounded!(
            self.epoch_loss_limit_bps,
            MIN_EPOCH_LOSS_LIMIT_BPS,
            MAX_EPOCH_LOSS_LIMIT_BPS
        );
        bounded!(
            self.probation_grace_epochs,
            MIN_PROBATION_GRACE_EPOCHS,
            MAX_PROBATION_GRACE_EPOCHS
        );
        bounded!(
            self.nonresponse_timeout_epochs,
            MIN_NONRESPONSE_TIMEOUT_EPOCHS,
            MAX_NONRESPONSE_TIMEOUT_EPOCHS
        );
        bounded!(self.slash_burn_bps, MIN_SLASH_BURN_BPS, MAX_SLASH_BURN_BPS);
        bounded!(
            self.max_single_asset_bps,
            MIN_MAX_SINGLE_ASSET_BPS,
            MAX_MAX_SINGLE_ASSET_BPS
        );

        // -- risk cache ------------------------------------------------------
        bounded!(
            self.cache_accrual_bps,
            MIN_CACHE_ACCRUAL_BPS,
            MAX_CACHE_ACCRUAL_BPS
        );
        bounded!(
            self.cache_reserve_target_bps,
            MIN_CACHE_RESERVE_TARGET_BPS,
            MAX_CACHE_RESERVE_TARGET_BPS
        );

        // -- ordering invariants ---------------------------------------------
        // The published ranges of these pairs overlap, so each value can be
        // individually legal while the pair is nonsense. Both orderings come
        // straight from the specs and both defaults satisfy them.

        // risk-spec 1.3: the probation drawdown is the milder threshold that
        // fires first. At or above the slash threshold, probation could never
        // be reached and every drawdown would go straight to a slash.
        require!(
            self.dd_probation_bps < self.dd_slash_bps,
            ColonyError::ParamOutOfRange
        );

        // allocation-spec 7: the drop threshold sits below the concentration
        // cap. At or above it, a forager pinned at the cap would be demoted in
        // the same pass that capped it.
        require!(
            self.w_drop_bps < self.w_max_bps,
            ColonyError::ParamOutOfRange
        );

        Ok(())
    }

    /// Snapshot the parameters currently stored on a colony.
    fn from_config(config: &ColonyConfig) -> Self {
        Self {
            epoch_duration_secs: config.epoch_duration_secs,

            rho_bps: config.rho_bps,
            deposit_scale_q: config.deposit_scale_q,
            perf_norm_s_bps: config.perf_norm_s_bps,
            risk_aversion_bps: config.risk_aversion_bps,

            w_max_bps: config.w_max_bps,
            w_drop_bps: config.w_drop_bps,
            scout_budget_bps: config.scout_budget_bps,
            reband_band_bps: config.reband_band_bps,
            turnover_cap_bps: config.turnover_cap_bps,

            promote_min_epochs: config.promote_min_epochs,
            promote_min_trades: config.promote_min_trades,
            promote_perf_bar_bps: config.promote_perf_bar_bps,
            scout_ticket_base_units: config.scout_ticket_base_units,
            promote_tau_seed_cap: config.promote_tau_seed_cap,

            min_bond: config.min_bond,
            bond_ratio_bps: config.bond_ratio_bps,
            bond_haircut_bps: config.bond_haircut_bps,
            dd_probation_bps: config.dd_probation_bps,
            dd_slash_bps: config.dd_slash_bps,
            epoch_loss_limit_bps: config.epoch_loss_limit_bps,
            probation_grace_epochs: config.probation_grace_epochs,
            nonresponse_timeout_epochs: config.nonresponse_timeout_epochs,
            slash_burn_bps: config.slash_burn_bps,
            max_single_asset_bps: config.max_single_asset_bps,

            cache_accrual_bps: config.cache_accrual_bps,
            cache_reserve_target_bps: config.cache_reserve_target_bps,
        }
    }

    /// Write a validated parameter set onto the colony. Touches parameters
    /// only: no epoch counter, pool balance or accumulator is written here.
    fn store_into(&self, config: &mut ColonyConfig) {
        config.epoch_duration_secs = self.epoch_duration_secs;

        config.rho_bps = self.rho_bps;
        config.deposit_scale_q = self.deposit_scale_q;
        config.perf_norm_s_bps = self.perf_norm_s_bps;
        config.risk_aversion_bps = self.risk_aversion_bps;

        config.w_max_bps = self.w_max_bps;
        config.w_drop_bps = self.w_drop_bps;
        config.scout_budget_bps = self.scout_budget_bps;
        config.reband_band_bps = self.reband_band_bps;
        config.turnover_cap_bps = self.turnover_cap_bps;

        config.promote_min_epochs = self.promote_min_epochs;
        config.promote_min_trades = self.promote_min_trades;
        config.promote_perf_bar_bps = self.promote_perf_bar_bps;
        config.scout_ticket_base_units = self.scout_ticket_base_units;
        config.promote_tau_seed_cap = self.promote_tau_seed_cap;

        config.min_bond = self.min_bond;
        config.bond_ratio_bps = self.bond_ratio_bps;
        config.bond_haircut_bps = self.bond_haircut_bps;
        config.dd_probation_bps = self.dd_probation_bps;
        config.dd_slash_bps = self.dd_slash_bps;
        config.epoch_loss_limit_bps = self.epoch_loss_limit_bps;
        config.probation_grace_epochs = self.probation_grace_epochs;
        config.nonresponse_timeout_epochs = self.nonresponse_timeout_epochs;
        config.slash_burn_bps = self.slash_burn_bps;
        config.max_single_asset_bps = self.max_single_asset_bps;

        config.cache_accrual_bps = self.cache_accrual_bps;
        config.cache_reserve_target_bps = self.cache_reserve_target_bps;
    }
}

// ---------------------------------------------------------------------------
// 1. initialize_colony
// ---------------------------------------------------------------------------

/// Genesis. Creates the colony configuration and fixes the base asset.
///
/// The signer becomes the authority, because there is no prior state to check
/// it against. This instruction can only ever succeed once for a given program
/// id: the config PDA has no variable seed, so the second attempt fails at
/// account creation.
#[derive(Accounts)]
pub struct InitializeColony<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + ColonyConfig::LEN,
        seeds = [SEED_COLONY],
        bump
    )]
    pub config: Box<Account<'info, ColonyConfig>>,

    /// The single asset the colony accounts in. Deposits, allocations, bonds,
    /// slashing and cache coverage are all denominated in this mint, which is
    /// what keeps the program free of any price oracle.
    pub base_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn initialize_colony(ctx: Context<InitializeColony>, params: InitColonyParams) -> Result<()> {
    params.validate()?;

    let now = Clock::get()?.unix_timestamp;
    let epoch_end_ts = now
        .checked_add(params.epoch_duration_secs)
        .ok_or(ColonyError::Overflow)?;

    let authority_key = ctx.accounts.authority.key();
    let base_mint_key = ctx.accounts.base_mint.key();
    let config_bump = ctx.bumps.config;

    let config: &mut ColonyConfig = &mut ctx.accounts.config;

    config.authority = authority_key;
    config.pending_authority = Pubkey::default();
    config.base_mint = base_mint_key;

    // Epoch numbering starts at 1, not 0, and this is load-bearing. The
    // settlement crank admits a forager with `last_settled_epoch <
    // settling_epoch`, and a forager is initialized one epoch behind the
    // current one. At epoch 0 that subtraction saturates to 0, the guard reads
    // `0 < 0`, and no forager could ever be settled while still counting toward
    // the completion target, so `finalize_settlement` could never succeed.
    config.epoch = 1;
    config.settling_epoch = 0;
    config.epoch_end_ts = epoch_end_ts;

    config.pheromone_sum = 0;
    config.pheromone_sum_acc = 0;
    config.alloc_rest_sum = 0;
    config.alloc_remaining_bps = BPS_DENOM;

    config.allocatable_pool = 0;
    config.scout_pool = 0;
    config.epoch_turnover_used = 0;

    config.settleable_forager_count = 0;
    config.active_forager_count = 0;
    config.active_count_acc = 0;
    config.settled_count = 0;

    // Pinned by policy, not settable through params or a patch. The field
    // exists so the unlevered rule is readable on-chain by anyone.
    config.max_leverage_x = MAX_LEVERAGE_X;

    config.epoch_phase = PHASE_OPEN;
    config.paused = false;
    config.bump = config_bump;

    params.store_into(config);

    emit!(ColonyInitialized {
        authority: config.authority,
        base_mint: config.base_mint,
        epoch_duration_secs: config.epoch_duration_secs,
        epoch_end_ts: config.epoch_end_ts,
    });

    Ok(())
}
