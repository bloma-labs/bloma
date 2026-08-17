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
    pub promote_min_realized_epochs: u16,
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

    // -- admission burn ------------------------------------------------------
    /// $KOLNY destroyed per registration, in base units. A count, not a value.
    /// The mint it destroys is NOT settable here: `kolny_mint` is written once
    /// by `set_kolny_mint` and an authority cannot repoint it afterwards.
    pub admission_burn_amount: u64,
}

/// A partial edit to the live configuration.
///
/// `None` means "leave this parameter where it is", which is not the same as
/// zero. Sending a full struct of concrete values on every edit would make a
/// forgotten field silently reset a parameter, so the patch carries only what
/// the caller actually intends to move.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct ConfigPatch {
    // -- epoch ---------------------------------------------------------------
    pub epoch_duration_secs: Option<i64>,

    // -- pheromone update ----------------------------------------------------
    pub rho_bps: Option<u16>,
    pub deposit_scale_q: Option<u64>,
    pub perf_norm_s_bps: Option<u16>,
    pub risk_aversion_bps: Option<u16>,

    // -- weights -------------------------------------------------------------
    pub w_max_bps: Option<u16>,
    pub w_drop_bps: Option<u16>,
    pub scout_budget_bps: Option<u16>,
    pub reband_band_bps: Option<u16>,
    pub turnover_cap_bps: Option<u16>,

    // -- scout promotion -----------------------------------------------------
    pub promote_min_epochs: Option<u8>,
    pub promote_min_realized_epochs: Option<u16>,
    pub promote_perf_bar_bps: Option<i32>,
    pub scout_ticket_base_units: Option<u64>,
    pub promote_tau_seed_cap: Option<u64>,

    // -- bond and slashing ---------------------------------------------------
    pub min_bond: Option<u64>,
    pub bond_ratio_bps: Option<u16>,
    pub bond_haircut_bps: Option<u16>,
    pub dd_probation_bps: Option<u16>,
    pub dd_slash_bps: Option<u16>,
    pub epoch_loss_limit_bps: Option<u16>,
    pub probation_grace_epochs: Option<u8>,
    pub nonresponse_timeout_epochs: Option<u8>,
    pub slash_burn_bps: Option<u16>,
    pub max_single_asset_bps: Option<u16>,

    // -- risk cache ----------------------------------------------------------
    pub cache_accrual_bps: Option<u16>,
    pub cache_reserve_target_bps: Option<u16>,

    // -- admission burn ------------------------------------------------------
    pub admission_burn_amount: Option<u64>,
}

/// Inclusive `[min, max]` bound check against the published range.
macro_rules! in_range {
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
        in_range!(
            self.epoch_duration_secs,
            MIN_EPOCH_DURATION_SECS,
            MAX_EPOCH_DURATION_SECS
        );

        // -- pheromone update ------------------------------------------------
        in_range!(self.rho_bps, MIN_RHO_BPS, MAX_RHO_BPS);
        in_range!(
            self.deposit_scale_q,
            MIN_DEPOSIT_SCALE_Q,
            MAX_DEPOSIT_SCALE_Q
        );
        in_range!(
            self.perf_norm_s_bps,
            MIN_PERF_NORM_S_BPS,
            MAX_PERF_NORM_S_BPS
        );
        in_range!(
            self.risk_aversion_bps,
            MIN_RISK_AVERSION_BPS,
            MAX_RISK_AVERSION_BPS
        );

        // -- weights ---------------------------------------------------------
        in_range!(self.w_max_bps, MIN_W_MAX_BPS, MAX_W_MAX_BPS);
        in_range!(self.w_drop_bps, MIN_W_DROP_BPS, MAX_W_DROP_BPS);
        in_range!(
            self.scout_budget_bps,
            MIN_SCOUT_BUDGET_BPS,
            MAX_SCOUT_BUDGET_BPS
        );
        in_range!(
            self.reband_band_bps,
            MIN_REBAND_BAND_BPS,
            MAX_REBAND_BAND_BPS
        );
        in_range!(
            self.turnover_cap_bps,
            MIN_TURNOVER_CAP_BPS,
            MAX_TURNOVER_CAP_BPS
        );

        // -- scout promotion -------------------------------------------------
        in_range!(
            self.promote_min_epochs,
            MIN_PROMOTE_MIN_EPOCHS,
            MAX_PROMOTE_MIN_EPOCHS
        );
        in_range!(
            self.promote_min_realized_epochs,
            MIN_PROMOTE_MIN_REALIZED_EPOCHS,
            MAX_PROMOTE_MIN_REALIZED_EPOCHS
        );
        in_range!(
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
        in_range!(self.bond_ratio_bps, MIN_BOND_RATIO_BPS, MAX_BOND_RATIO_BPS);
        in_range!(
            self.bond_haircut_bps,
            MIN_BOND_HAIRCUT_BPS,
            MAX_BOND_HAIRCUT_BPS
        );
        in_range!(
            self.dd_probation_bps,
            MIN_DD_PROBATION_BPS,
            MAX_DD_PROBATION_BPS
        );
        in_range!(self.dd_slash_bps, MIN_DD_SLASH_BPS, MAX_DD_SLASH_BPS);
        in_range!(
            self.epoch_loss_limit_bps,
            MIN_EPOCH_LOSS_LIMIT_BPS,
            MAX_EPOCH_LOSS_LIMIT_BPS
        );
        in_range!(
            self.probation_grace_epochs,
            MIN_PROBATION_GRACE_EPOCHS,
            MAX_PROBATION_GRACE_EPOCHS
        );
        in_range!(
            self.nonresponse_timeout_epochs,
            MIN_NONRESPONSE_TIMEOUT_EPOCHS,
            MAX_NONRESPONSE_TIMEOUT_EPOCHS
        );
        in_range!(self.slash_burn_bps, MIN_SLASH_BURN_BPS, MAX_SLASH_BURN_BPS);
        in_range!(
            self.max_single_asset_bps,
            MIN_MAX_SINGLE_ASSET_BPS,
            MAX_MAX_SINGLE_ASSET_BPS
        );

        // -- risk cache ------------------------------------------------------
        in_range!(
            self.cache_accrual_bps,
            MIN_CACHE_ACCRUAL_BPS,
            MAX_CACHE_ACCRUAL_BPS
        );
        in_range!(
            self.cache_reserve_target_bps,
            MIN_CACHE_RESERVE_TARGET_BPS,
            MAX_CACHE_RESERVE_TARGET_BPS
        );

        // -- admission burn --------------------------------------------------
        // The bound that matters here is the ceiling, and it lives in
        // `constants.rs` rather than in a config field precisely so the
        // authority cannot raise its own limit. An authority free to set any
        // admission it liked could price entry into a ban, or into extraction;
        // inside the published band it can only track a token price that moved.
        // The floor exists so admission can never be free, which would leave
        // registration succeeding while nothing was destroyed.
        in_range!(
            self.admission_burn_amount,
            MIN_ADMISSION_BURN_BASE_UNITS,
            MAX_ADMISSION_BURN_BASE_UNITS
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

        // The activity requirement cannot exceed the tenure requirement. A
        // forager accrues at most one realized epoch per scout epoch, so
        // demanding more realized epochs than scout epochs silently makes the
        // tenure gate dead code and stretches promotion by the difference.
        //
        // This invariant exists because that is exactly what happened: this
        // parameter used to be the specification's `promote_min_trades` with a
        // default of 20, and once it was being evaluated against epochs rather
        // than fills, promotion quietly required 20 epochs -- about 140 days at
        // a 7-day epoch -- instead of the intended four. The rename fixed the
        // name and the default; this check makes the failure unreachable.
        require!(
            self.promote_min_realized_epochs as u64 <= self.promote_min_epochs as u64,
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
            promote_min_realized_epochs: config.promote_min_realized_epochs,
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

            admission_burn_amount: config.admission_burn_amount,
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
        config.promote_min_realized_epochs = self.promote_min_realized_epochs;
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

        // Only the amount. `kolny_mint` and `total_kolny_burned` are state, not
        // parameters, and are deliberately unreachable from this path: one is
        // written once by `set_kolny_mint` and the other only ever grows by what
        // a burn actually destroyed.
        config.admission_burn_amount = self.admission_burn_amount;
    }
}

impl ConfigPatch {
    /// Lay every field the caller actually set over a parameter snapshot.
    fn overlay(&self, base: &mut InitColonyParams) {
        if let Some(v) = self.epoch_duration_secs {
            base.epoch_duration_secs = v;
        }

        if let Some(v) = self.rho_bps {
            base.rho_bps = v;
        }
        if let Some(v) = self.deposit_scale_q {
            base.deposit_scale_q = v;
        }
        if let Some(v) = self.perf_norm_s_bps {
            base.perf_norm_s_bps = v;
        }
        if let Some(v) = self.risk_aversion_bps {
            base.risk_aversion_bps = v;
        }

        if let Some(v) = self.w_max_bps {
            base.w_max_bps = v;
        }
        if let Some(v) = self.w_drop_bps {
            base.w_drop_bps = v;
        }
        if let Some(v) = self.scout_budget_bps {
            base.scout_budget_bps = v;
        }
        if let Some(v) = self.reband_band_bps {
            base.reband_band_bps = v;
        }
        if let Some(v) = self.turnover_cap_bps {
            base.turnover_cap_bps = v;
        }

        if let Some(v) = self.promote_min_epochs {
            base.promote_min_epochs = v;
        }
        if let Some(v) = self.promote_min_realized_epochs {
            base.promote_min_realized_epochs = v;
        }
        if let Some(v) = self.promote_perf_bar_bps {
            base.promote_perf_bar_bps = v;
        }
        if let Some(v) = self.scout_ticket_base_units {
            base.scout_ticket_base_units = v;
        }
        if let Some(v) = self.promote_tau_seed_cap {
            base.promote_tau_seed_cap = v;
        }

        if let Some(v) = self.min_bond {
            base.min_bond = v;
        }
        if let Some(v) = self.bond_ratio_bps {
            base.bond_ratio_bps = v;
        }
        if let Some(v) = self.bond_haircut_bps {
            base.bond_haircut_bps = v;
        }
        if let Some(v) = self.dd_probation_bps {
            base.dd_probation_bps = v;
        }
        if let Some(v) = self.dd_slash_bps {
            base.dd_slash_bps = v;
        }
        if let Some(v) = self.epoch_loss_limit_bps {
            base.epoch_loss_limit_bps = v;
        }
        if let Some(v) = self.probation_grace_epochs {
            base.probation_grace_epochs = v;
        }
        if let Some(v) = self.nonresponse_timeout_epochs {
            base.nonresponse_timeout_epochs = v;
        }
        if let Some(v) = self.slash_burn_bps {
            base.slash_burn_bps = v;
        }
        if let Some(v) = self.max_single_asset_bps {
            base.max_single_asset_bps = v;
        }

        if let Some(v) = self.cache_accrual_bps {
            base.cache_accrual_bps = v;
        }
        if let Some(v) = self.cache_reserve_target_bps {
            base.cache_reserve_target_bps = v;
        }

        if let Some(v) = self.admission_burn_amount {
            base.admission_burn_amount = v;
        }
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

    // The $KOLNY mint is not known at genesis and may not exist yet, so it
    // starts unset and `set_kolny_mint` writes it once. Until then
    // `register_forager` refuses to run: admission is priced in a token the
    // colony cannot yet name, and the alternative -- admitting foragers and
    // skipping the burn -- would leave a colony that looks like it is burning
    // and is not. `admission_burn_amount` itself is set by `params` below and
    // is range-checked at genesis exactly as it is on every later edit.
    config.kolny_mint = Pubkey::default();
    config.total_kolny_burned = 0;

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

// ---------------------------------------------------------------------------
// 2. update_config
// ---------------------------------------------------------------------------

#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    #[account(
        mut,
        seeds = [SEED_COLONY],
        bump = config.bump,
        has_one = authority @ ColonyError::NotAuthority,
    )]
    pub config: Box<Account<'info, ColonyConfig>>,

    pub authority: Signer<'info>,
}

/// Move one or more parameters.
///
/// The patch is merged onto a snapshot of the live configuration and the merged
/// result goes through the same gate genesis used, so every changed value is
/// range-checked and every ordering invariant is re-confirmed against the
/// values it will actually sit beside.
///
/// Changing `epoch_duration_secs` does not move `epoch_end_ts`. The epoch that
/// is already running keeps the deadline depositors saw when it opened; a new
/// duration that took effect immediately would let an authority end an epoch on
/// demand, or postpone one indefinitely.
pub fn update_config(ctx: Context<UpdateConfig>, patch: ConfigPatch) -> Result<()> {
    let config: &mut ColonyConfig = &mut ctx.accounts.config;

    let mut merged = InitColonyParams::from_config(config);
    patch.overlay(&mut merged);
    merged.validate()?;
    merged.store_into(config);

    emit!(ConfigUpdated {
        authority: config.authority,
        epoch: config.epoch,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// 2b. set_kolny_mint
// ---------------------------------------------------------------------------

#[derive(Accounts)]
pub struct SetKolnyMint<'info> {
    #[account(
        mut,
        seeds = [SEED_COLONY],
        bump = config.bump,
        has_one = authority @ ColonyError::NotAuthority,
    )]
    pub config: Box<Account<'info, ColonyConfig>>,

    /// The $KOLNY mint.
    ///
    /// Taken as a real mint account rather than a bare `Pubkey` argument on
    /// purpose. This write happens once and can never be undone, so a mistyped
    /// address would leave admission pointing at nothing and registration
    /// permanently refused. Passing the account makes the token program's own
    /// layout the check: an address that is not an initialized mint cannot
    /// deserialize here, so the typo fails now rather than at the first
    /// registration.
    pub kolny_mint: Box<InterfaceAccount<'info, Mint>>,

    pub authority: Signer<'info>,
}

/// Name the mint that admission destroys. Once.
///
/// Separate from `update_config` because the $KOLNY mint does not exist yet:
/// the program has to be deployable before the token is issued, and this is the
/// one write that closes that gap afterwards.
///
/// One-way by construction. A repointable mint would let an authority swap the
/// burned asset for a worthless one it controls while registration kept
/// succeeding, which turns the admission gate into theatre and makes the
/// published burn figure meaningless. Irreversibility is what makes the
/// on-chain mint worth reading. The cost of that choice, stated plainly: a mint
/// set to the wrong address cannot be corrected without a program upgrade, and
/// the account constraint above exists to make that address impossible to
/// mistype into something that is not a mint at all.
///
/// What this instruction is NOT: it does not price admission and it does not
/// touch any allocation input. The amount lives in `admission_burn_amount` and
/// moves only inside the published band.
pub fn set_kolny_mint(ctx: Context<SetKolnyMint>) -> Result<()> {
    require!(
        !ctx.accounts.config.kolny_mint_is_set(),
        ColonyError::KolnyMintAlreadySet
    );

    // What the account type already guaranteed before this line ran, so the
    // checks below do not repeat it (an unreachable `require!` reads like a
    // live defence and is worse than none):
    //
    //   - the account EXISTS. `InterfaceAccount::try_from` rejects an account
    //     owned by the system program with zero lamports as
    //     `AccountNotInitialized`, which is exactly what a not-yet-launched
    //     address looks like on chain
    //     (anchor-lang `accounts/interface_account.rs:220`).
    //   - it is owned by SPL Token or Token-2022 (`check_owner` against
    //     anchor-spl `token_interface.rs:12`), so a wallet or a PDA is refused.
    //   - it really is a MINT, not a token account: `StateWithExtensions::
    //     <Mint>::unpack` -- the checked variant -- rejects a wrong data length
    //     and an uninitialized mint (anchor-spl `token_interface.rs:46`).
    //
    // That covers "is this a real mint that exists". It does not cover whether
    // this mint can carry the claims the product makes about it, which is what
    // `check_admission_mint` decides.
    let decimals = ctx.accounts.kolny_mint.decimals;
    let mint_authority_is_none = ctx.accounts.kolny_mint.mint_authority.is_none();
    let freeze_authority_is_none = ctx.accounts.kolny_mint.freeze_authority.is_none();

    if let Some(rejection) =
        check_admission_mint(decimals, mint_authority_is_none, freeze_authority_is_none)
    {
        return Err(match rejection {
            // The admission constants are base units computed at
            // `KOLNY_DECIMALS`. At other decimals every one of them silently
            // means a different number of tokens -- at 9 decimals the 100,000
            // $KOLNY admission would destroy 100.
            //
            // This does NOT replace reading the chain. `utils::burn_from_user`
            // still hands `burn_checked` the decimals off the mint account.
            // This check exists so the CONSTANTS cannot be wrong; that one
            // exists so the CPI cannot be. Collapsing the two into "we checked
            // it, so we can hard-code it" is how the thousand-fold bug comes
            // back.
            AdmissionMintRejection::Decimals => ColonyError::KolnyMintDecimalsMismatch.into(),
            // Refused rather than recorded. The product says burning reduces
            // supply permanently; with a live mint authority that sentence is
            // false, and a program that binds anyway lets the false sentence
            // ship. Failing here is the cheap failure: pre-mainnet, a rebuild.
            AdmissionMintRejection::MintAuthorityLive => {
                ColonyError::KolnyMintAuthorityNotRevoked.into()
            }
            // Same shape, different promise: a frozen account cannot burn, so a
            // live freeze authority makes admission revocable per operator.
            AdmissionMintRejection::FreezeAuthorityLive => {
                ColonyError::KolnyMintFreezeAuthorityNotRevoked.into()
            }
        });
    }

    let kolny_mint = ctx.accounts.kolny_mint.key();

    let config: &mut ColonyConfig = &mut ctx.accounts.config;
    config.kolny_mint = kolny_mint;

    // Published even though all three were just required, so the claims that
    // rest on them stay checkable from the event stream without re-reading the
    // mint. Both authorities are revoked at this point, so neither can return.
    emit!(KolnyMintSet {
        authority: config.authority,
        kolny_mint,
        decimals,
        mint_authority_is_none,
        freeze_authority_is_none,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// 3. propose_authority  /  4. accept_authority
// ---------------------------------------------------------------------------

#[derive(Accounts)]
pub struct ProposeAuthority<'info> {
    #[account(
        mut,
        seeds = [SEED_COLONY],
        bump = config.bump,
        has_one = authority @ ColonyError::NotAuthority,
    )]
    pub config: Box<Account<'info, ColonyConfig>>,

    pub authority: Signer<'info>,
}

/// Nominate the next authority. Nothing changes until the nominee accepts, so
/// a mistyped key cannot strand the colony.
///
/// Proposing the default key clears a pending nomination.
pub fn propose_authority(ctx: Context<ProposeAuthority>, new_authority: Pubkey) -> Result<()> {
    let config: &mut ColonyConfig = &mut ctx.accounts.config;
    config.pending_authority = new_authority;

    emit!(AuthorityProposed {
        current: config.authority,
        pending: new_authority,
    });

    Ok(())
}

#[derive(Accounts)]
pub struct AcceptAuthority<'info> {
    #[account(mut, seeds = [SEED_COLONY], bump = config.bump)]
    pub config: Box<Account<'info, ColonyConfig>>,

    /// Must be the pending authority, and must sign. That signature is the
    /// whole point of the two-step handover: it proves the key exists and its
    /// holder can transact before it takes over.
    pub new_authority: Signer<'info>,
}

pub fn accept_authority(ctx: Context<AcceptAuthority>) -> Result<()> {
    let signer_key = ctx.accounts.new_authority.key();
    let config: &mut ColonyConfig = &mut ctx.accounts.config;

    require_keys_neq!(
        config.pending_authority,
        Pubkey::default(),
        ColonyError::NoPendingAuthority
    );
    require_keys_eq!(
        config.pending_authority,
        signer_key,
        ColonyError::NotAuthority
    );

    let previous = config.authority;
    config.authority = signer_key;
    config.pending_authority = Pubkey::default();

    emit!(AuthorityAccepted {
        previous,
        current: config.authority,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// 5. set_paused
// ---------------------------------------------------------------------------

#[derive(Accounts)]
pub struct SetPaused<'info> {
    #[account(
        mut,
        seeds = [SEED_COLONY],
        bump = config.bump,
        has_one = authority @ ColonyError::NotAuthority,
    )]
    pub config: Box<Account<'info, ColonyConfig>>,

    pub authority: Signer<'info>,
}

/// Halt the instructions that push capital outward. The pause flag is read by
/// the deposit and allocation paths; it never blocks a depositor from getting
/// value back out, and it is not gated on the pause flag itself so a paused
/// colony can always be unpaused.
pub fn set_paused(ctx: Context<SetPaused>, paused: bool) -> Result<()> {
    let config: &mut ColonyConfig = &mut ctx.accounts.config;
    config.paused = paused;

    emit!(PausedSet {
        authority: config.authority,
        paused,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// 6. initialize_brood  /  7. open_vault_base
// ---------------------------------------------------------------------------

#[derive(Accounts)]
pub struct InitializeBrood<'info> {
    #[account(
        seeds = [SEED_COLONY],
        bump = config.bump,
        has_one = authority @ ColonyError::NotAuthority,
    )]
    pub config: Box<Account<'info, ColonyConfig>>,

    #[account(
        init,
        payer = authority,
        space = 8 + BroodVaultState::LEN,
        seeds = [SEED_BROOD],
        bump
    )]
    pub brood_state: Box<Account<'info, BroodVaultState>>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

/// Create the share-accounting state. The token account it will own is opened
/// separately by `open_vault_base`, because two `init` constraints in one
/// context overrun the stack frame.
pub fn initialize_brood(ctx: Context<InitializeBrood>) -> Result<()> {
    // Copied from the config rather than taken as an argument, so the brood
    // cannot be pointed at a mint the colony does not account in.
    let base_mint = ctx.accounts.config.base_mint;
    let brood_bump = ctx.bumps.brood_state;

    let brood: &mut BroodVaultState = &mut ctx.accounts.brood_state;

    brood.base_mint = base_mint;
    brood.vault_base = Pubkey::default();

    brood.total_shares = 0;
    brood.pending_redemption_shares = 0;

    brood.idle_base = 0;
    brood.outstanding_principal = 0;
    brood.next_redemption_id = 0;

    brood.bump = brood_bump;
    brood.vault_bump = 0;

    Ok(())
}

#[derive(Accounts)]
pub struct OpenVaultBase<'info> {
    #[account(
        seeds = [SEED_COLONY],
        bump = config.bump,
        has_one = authority @ ColonyError::NotAuthority,
        has_one = base_mint @ ColonyError::BaseMintMismatch,
    )]
    pub config: Box<Account<'info, ColonyConfig>>,

    #[account(
        mut,
        seeds = [SEED_BROOD],
        bump = brood_state.bump,
        has_one = base_mint @ ColonyError::BaseMintMismatch,
    )]
    pub brood_state: Box<Account<'info, BroodVaultState>>,

    /// The colony's single custody account for idle base asset. Owned by the
    /// brood PDA, so only this program can move anything out of it.
    #[account(
        init,
        payer = authority,
        seeds = [SEED_BROOD_VAULT],
        bump,
        token::mint = base_mint,
        token::authority = brood_state,
        token::token_program = token_program,
    )]
    pub vault_base: Box<InterfaceAccount<'info, TokenAccount>>,

    pub base_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn open_vault_base(ctx: Context<OpenVaultBase>) -> Result<()> {
    let vault_key = ctx.accounts.vault_base.key();
    let vault_bump = ctx.bumps.vault_base;

    let brood: &mut BroodVaultState = &mut ctx.accounts.brood_state;

    brood.vault_base = vault_key;
    brood.vault_bump = vault_bump;

    emit!(BroodInitialized {
        base_mint: brood.base_mint,
        vault_base: brood.vault_base,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// 8. initialize_risk_cache  /  9. open_cache_vault  /  10. open_incinerator_vault
// ---------------------------------------------------------------------------

#[derive(Accounts)]
pub struct InitializeRiskCache<'info> {
    #[account(
        seeds = [SEED_COLONY],
        bump = config.bump,
        has_one = authority @ ColonyError::NotAuthority,
    )]
    pub config: Box<Account<'info, ColonyConfig>>,

    #[account(
        init,
        payer = authority,
        space = 8 + RiskCacheState::LEN,
        seeds = [SEED_CACHE],
        bump
    )]
    pub cache_state: Box<Account<'info, RiskCacheState>>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn initialize_risk_cache(ctx: Context<InitializeRiskCache>) -> Result<()> {
    let cache_bump = ctx.bumps.cache_state;

    let cache: &mut RiskCacheState = &mut ctx.accounts.cache_state;

    cache.cache_vault = Pubkey::default();
    cache.incinerator_vault = Pubkey::default();

    cache.balance = 0;
    cache.total_covered = 0;
    cache.total_burned = 0;
    cache.total_accrued = 0;

    cache.bump = cache_bump;
    cache.vault_bump = 0;
    cache.incinerator_bump = 0;

    Ok(())
}

#[derive(Accounts)]
pub struct OpenCacheVault<'info> {
    #[account(
        seeds = [SEED_COLONY],
        bump = config.bump,
        has_one = authority @ ColonyError::NotAuthority,
        has_one = base_mint @ ColonyError::BaseMintMismatch,
    )]
    pub config: Box<Account<'info, ColonyConfig>>,

    #[account(mut, seeds = [SEED_CACHE], bump = cache_state.bump)]
    pub cache_state: Box<Account<'info, RiskCacheState>>,

    /// Holds the insurance reserve that absorbs depositor losses.
    #[account(
        init,
        payer = authority,
        seeds = [SEED_CACHE_VAULT],
        bump,
        token::mint = base_mint,
        token::authority = cache_state,
        token::token_program = token_program,
    )]
    pub cache_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub base_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn open_cache_vault(ctx: Context<OpenCacheVault>) -> Result<()> {
    let vault_key = ctx.accounts.cache_vault.key();
    let vault_bump = ctx.bumps.cache_vault;

    let cache: &mut RiskCacheState = &mut ctx.accounts.cache_state;

    cache.cache_vault = vault_key;
    cache.vault_bump = vault_bump;

    Ok(())
}

#[derive(Accounts)]
pub struct OpenIncineratorVault<'info> {
    #[account(
        seeds = [SEED_COLONY],
        bump = config.bump,
        has_one = authority @ ColonyError::NotAuthority,
        has_one = base_mint @ ColonyError::BaseMintMismatch,
    )]
    pub config: Box<Account<'info, ColonyConfig>>,

    #[account(mut, seeds = [SEED_CACHE], bump = cache_state.bump)]
    pub cache_state: Box<Account<'info, RiskCacheState>>,

    /// One-way sink for the burned share of a seized bond.
    ///
    /// This program contains no instruction that transfers out of this account
    /// and none may ever be added. Its authority is a PDA whose signing seeds
    /// only ever appear on transfers into it, so the balance is economically
    /// destroyed: unreachable by the authority, by an operator, and by the
    /// program itself. Nothing here reduces the supply of any mint, and the
    /// public copy must not describe it as if it did.
    #[account(
        init,
        payer = authority,
        seeds = [SEED_INCINERATOR],
        bump,
        token::mint = base_mint,
        token::authority = cache_state,
        token::token_program = token_program,
    )]
    pub incinerator_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub base_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn open_incinerator_vault(ctx: Context<OpenIncineratorVault>) -> Result<()> {
    let incinerator_key = ctx.accounts.incinerator_vault.key();
    let incinerator_bump = ctx.bumps.incinerator_vault;

    let cache: &mut RiskCacheState = &mut ctx.accounts.cache_state;

    // The incinerator is the second of the two cache accounts to open, because
    // `RiskCacheInitialized` is where the indexer learns both addresses. Run
    // out of order and the event would publish a default cache vault.
    require_keys_neq!(
        cache.cache_vault,
        Pubkey::default(),
        ColonyError::VaultMismatch
    );

    cache.incinerator_vault = incinerator_key;
    cache.incinerator_bump = incinerator_bump;

    emit!(RiskCacheInitialized {
        cache_vault: cache.cache_vault,
        incinerator_vault: cache.incinerator_vault,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// 11. initialize_trail_board
// ---------------------------------------------------------------------------

#[derive(Accounts)]
pub struct InitializeTrailBoard<'info> {
    #[account(
        seeds = [SEED_COLONY],
        bump = config.bump,
        has_one = authority @ ColonyError::NotAuthority,
    )]
    pub config: Box<Account<'info, ColonyConfig>>,

    #[account(
        init,
        payer = authority,
        space = 8 + TrailBoard::LEN,
        seeds = [SEED_TRAIL_BOARD],
        bump
    )]
    pub trail_board: Box<Account<'info, TrailBoard>>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

/// Create the scratch board the settlement crank fills with the largest trails
/// it sees, so the water-filling level can be solved at finalize without a pass
/// over every forager. It starts empty and is reset at the start of each
/// settlement.
pub fn initialize_trail_board(ctx: Context<InitializeTrailBoard>) -> Result<()> {
    let board_bump = ctx.bumps.trail_board;

    let board: &mut TrailBoard = &mut ctx.accounts.trail_board;

    board.reset();
    board.bump = board_bump;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Every published default, assembled through the same struct the
    /// instruction takes. `min_bond` and `scout_ticket_base_units` are
    /// project-set with no default in the specs, so they carry plausible
    /// non-zero stand-ins here.
    fn published_defaults() -> InitColonyParams {
        InitColonyParams {
            epoch_duration_secs: DEFAULT_EPOCH_DURATION_SECS,

            rho_bps: DEFAULT_RHO_BPS,
            deposit_scale_q: DEFAULT_DEPOSIT_SCALE_Q,
            perf_norm_s_bps: DEFAULT_PERF_NORM_S_BPS,
            risk_aversion_bps: DEFAULT_RISK_AVERSION_BPS,

            w_max_bps: DEFAULT_W_MAX_BPS,
            w_drop_bps: DEFAULT_W_DROP_BPS,
            scout_budget_bps: DEFAULT_SCOUT_BUDGET_BPS,
            reband_band_bps: DEFAULT_REBAND_BAND_BPS,
            turnover_cap_bps: DEFAULT_TURNOVER_CAP_BPS,

            promote_min_epochs: DEFAULT_PROMOTE_MIN_EPOCHS,
            promote_min_realized_epochs: DEFAULT_PROMOTE_MIN_REALIZED_EPOCHS,
            promote_perf_bar_bps: DEFAULT_PROMOTE_PERF_BAR_BPS,
            scout_ticket_base_units: 1_000_000,
            promote_tau_seed_cap: DEFAULT_PROMOTE_TAU_SEED_CAP,

            min_bond: 1_000_000,
            bond_ratio_bps: DEFAULT_BOND_RATIO_BPS,
            bond_haircut_bps: DEFAULT_BOND_HAIRCUT_BPS,
            dd_probation_bps: DEFAULT_DD_PROBATION_BPS,
            dd_slash_bps: DEFAULT_DD_SLASH_BPS,
            epoch_loss_limit_bps: DEFAULT_EPOCH_LOSS_LIMIT_BPS,
            probation_grace_epochs: DEFAULT_PROBATION_GRACE_EPOCHS,
            nonresponse_timeout_epochs: DEFAULT_NONRESPONSE_TIMEOUT_EPOCHS,
            slash_burn_bps: DEFAULT_SLASH_BURN_BPS,
            max_single_asset_bps: DEFAULT_MAX_SINGLE_ASSET_BPS,

            cache_accrual_bps: DEFAULT_CACHE_ACCRUAL_BPS,
            cache_reserve_target_bps: DEFAULT_CACHE_RESERVE_TARGET_BPS,

            admission_burn_amount: DEFAULT_ADMISSION_BURN_BASE_UNITS,
        }
    }

    fn empty_patch() -> ConfigPatch {
        ConfigPatch {
            epoch_duration_secs: None,

            rho_bps: None,
            deposit_scale_q: None,
            perf_norm_s_bps: None,
            risk_aversion_bps: None,

            w_max_bps: None,
            w_drop_bps: None,
            scout_budget_bps: None,
            reband_band_bps: None,
            turnover_cap_bps: None,

            promote_min_epochs: None,
            promote_min_realized_epochs: None,
            promote_perf_bar_bps: None,
            scout_ticket_base_units: None,
            promote_tau_seed_cap: None,

            min_bond: None,
            bond_ratio_bps: None,
            bond_haircut_bps: None,
            dd_probation_bps: None,
            dd_slash_bps: None,
            epoch_loss_limit_bps: None,
            probation_grace_epochs: None,
            nonresponse_timeout_epochs: None,
            slash_burn_bps: None,
            max_single_asset_bps: None,

            cache_accrual_bps: None,
            cache_reserve_target_bps: None,

            admission_burn_amount: None,
        }
    }

    /// A default that sits outside its own published range would make genesis
    /// with the documented parameters impossible. Cheap check, real failure.
    #[test]
    fn published_defaults_pass_the_range_gate() {
        assert!(published_defaults().validate().is_ok());
    }

    /// Promotion must not be able to demand more activity than tenure.
    ///
    /// This pins a defect that actually shipped into the parameter table. The
    /// field began life as the specification's `promote_min_trades`, a count of
    /// realized closed trades with a default of 20. The program cannot observe
    /// fills, so the gate was evaluated against epochs instead, and the 20 came
    /// along with it: promotion silently required 20 active epochs, roughly 140
    /// days at a 7-day epoch, rather than the intended four, and the tenure
    /// requirement became unreachable dead code because the activity count
    /// always bound first.
    ///
    /// A forager can accrue at most one realized epoch per scout epoch, so the
    /// two are ordered by construction and the ordering is now enforced.
    #[test]
    fn activity_requirement_cannot_exceed_tenure_requirement() {
        // The historical value is rejected outright rather than silently
        // stretching the promotion timeline.
        let mut p = published_defaults();
        p.promote_min_realized_epochs = 20;
        assert!(
            p.validate().is_err(),
            "20 realized epochs against 4 scout epochs must be rejected"
        );

        // Equality is allowed: every scout epoch must be productive.
        let mut p = published_defaults();
        p.promote_min_realized_epochs = p.promote_min_epochs as u16;
        assert!(p.validate().is_ok());

        // One past tenure is not.
        let mut p = published_defaults();
        p.promote_min_realized_epochs = p.promote_min_epochs as u16 + 1;
        assert!(p.validate().is_err());

        // Raising tenure re-admits a higher activity bar.
        let mut p = published_defaults();
        p.promote_min_epochs = 12;
        p.promote_min_realized_epochs = 10;
        assert!(p.validate().is_ok());

        // The shipped defaults leave promotion reachable in roughly a month at
        // a 7-day epoch, which is what the specification intended.
        let d = published_defaults();
        assert_eq!(d.promote_min_epochs, 4);
        assert_eq!(d.promote_min_realized_epochs, 3);
    }

    #[test]
    fn out_of_range_values_are_rejected() {
        let mut p = published_defaults();
        p.rho_bps = MAX_RHO_BPS + 1;
        assert!(p.validate().is_err());

        let mut p = published_defaults();
        p.epoch_duration_secs = MIN_EPOCH_DURATION_SECS - 1;
        assert!(p.validate().is_err());

        let mut p = published_defaults();
        p.promote_perf_bar_bps = MIN_PROMOTE_PERF_BAR_BPS - 1;
        assert!(p.validate().is_err());

        let mut p = published_defaults();
        p.promote_tau_seed_cap = MAX_PROMOTE_TAU_SEED_CAP + 1;
        assert!(p.validate().is_err());
    }

    #[test]
    fn zero_is_rejected_where_the_spec_requires_a_positive_value() {
        let mut p = published_defaults();
        p.min_bond = 0;
        assert!(p.validate().is_err());

        let mut p = published_defaults();
        p.scout_ticket_base_units = 0;
        assert!(p.validate().is_err());
    }

    #[test]
    fn ordering_invariants_are_enforced() {
        // Individually legal, jointly nonsense: probation at or above slash.
        let mut p = published_defaults();
        p.dd_probation_bps = 3_500;
        p.dd_slash_bps = 3_000;
        assert!(p.validate().is_err());

        // Individually legal, jointly nonsense: drop at or above the cap.
        let mut p = published_defaults();
        p.w_drop_bps = 1_000;
        p.w_max_bps = 1_000;
        assert!(p.validate().is_err());
    }

    #[test]
    fn an_empty_patch_changes_nothing() {
        let mut merged = published_defaults();
        empty_patch().overlay(&mut merged);

        let base = published_defaults();
        assert_eq!(merged.epoch_duration_secs, base.epoch_duration_secs);
        assert_eq!(merged.rho_bps, base.rho_bps);
        assert_eq!(merged.w_max_bps, base.w_max_bps);
        assert_eq!(merged.min_bond, base.min_bond);
        assert_eq!(
            merged.cache_reserve_target_bps,
            base.cache_reserve_target_bps
        );
    }

    #[test]
    fn a_patch_moves_only_the_fields_it_sets() {
        let mut patch = empty_patch();
        patch.rho_bps = Some(MAX_RHO_BPS);
        patch.turnover_cap_bps = Some(MIN_TURNOVER_CAP_BPS);

        let mut merged = published_defaults();
        patch.overlay(&mut merged);

        assert_eq!(merged.rho_bps, MAX_RHO_BPS);
        assert_eq!(merged.turnover_cap_bps, MIN_TURNOVER_CAP_BPS);
        assert_eq!(merged.w_max_bps, DEFAULT_W_MAX_BPS);
        assert_eq!(merged.dd_slash_bps, DEFAULT_DD_SLASH_BPS);
        assert!(merged.validate().is_ok());
    }

    /// The admission burn is bounded at both ends, and neither bound is
    /// reachable by the authority.
    ///
    /// The ceiling is what makes the parameter safe to leave mutable at all:
    /// an authority that could raise it without limit could price entry into a
    /// ban, or into extraction. The floor is what makes the burn real: at zero,
    /// registration would keep succeeding while nothing was destroyed, and the
    /// colony would look identical to one that was burning.
    #[test]
    fn the_admission_burn_cannot_leave_its_published_band() {
        // Free admission is unrepresentable.
        let mut p = published_defaults();
        p.admission_burn_amount = 0;
        assert!(
            p.validate().is_err(),
            "a zero admission burn must be rejected: registration would succeed \
             while destroying nothing"
        );

        // The ceiling holds, including one unit past it.
        let mut p = published_defaults();
        p.admission_burn_amount = MAX_ADMISSION_BURN_BASE_UNITS + 1;
        assert!(p.validate().is_err());

        let mut p = published_defaults();
        p.admission_burn_amount = u64::MAX;
        assert!(p.validate().is_err());

        // Both endpoints and the default are legal, so the band is not empty
        // and the rejections above are the bound firing rather than everything
        // failing for some unrelated reason.
        for amount in [
            MIN_ADMISSION_BURN_BASE_UNITS,
            DEFAULT_ADMISSION_BURN_BASE_UNITS,
            MAX_ADMISSION_BURN_BASE_UNITS,
        ] {
            let mut p = published_defaults();
            p.admission_burn_amount = amount;
            assert!(
                p.validate().is_ok(),
                "{} sits inside the published band and must be accepted",
                amount
            );
        }

        // The published default is inside its own band. A default outside it
        // would make genesis with the documented parameters impossible.
        assert!(MIN_ADMISSION_BURN_BASE_UNITS <= DEFAULT_ADMISSION_BURN_BASE_UNITS);
        assert!(DEFAULT_ADMISSION_BURN_BASE_UNITS <= MAX_ADMISSION_BURN_BASE_UNITS);
    }

    /// `update_config` is the only path that can move the amount, and it runs
    /// through the same gate genesis used, so the ceiling is not something the
    /// authority can step over after launch.
    #[test]
    fn an_authority_cannot_patch_the_admission_burn_past_the_ceiling() {
        let mut patch = empty_patch();
        patch.admission_burn_amount = Some(MAX_ADMISSION_BURN_BASE_UNITS + 1);

        let mut merged = published_defaults();
        patch.overlay(&mut merged);
        assert!(merged.validate().is_err());

        // Control: the same patch shape inside the band is accepted, so the
        // rejection above is the bound and not the patch mechanism failing.
        let mut patch = empty_patch();
        patch.admission_burn_amount = Some(MAX_ADMISSION_BURN_BASE_UNITS);

        let mut merged = published_defaults();
        patch.overlay(&mut merged);
        assert_eq!(merged.admission_burn_amount, MAX_ADMISSION_BURN_BASE_UNITS);
        assert!(merged.validate().is_ok());

        // And an untouched patch leaves it where it was.
        let mut merged = published_defaults();
        empty_patch().overlay(&mut merged);
        assert_eq!(merged.admission_burn_amount, DEFAULT_ADMISSION_BURN_BASE_UNITS);
    }

    /// The mint is state, not a parameter: no patch can reach it, and no
    /// parameter snapshot carries it. The only writer is `set_kolny_mint`.
    #[test]
    fn the_kolny_mint_is_not_reachable_from_a_config_patch() {
        let mut config = ColonyConfig::default();
        published_defaults().store_into(&mut config);
        assert_eq!(
            config.kolny_mint,
            Pubkey::default(),
            "storing parameters must not write the mint"
        );

        config.kolny_mint = Pubkey::new_from_array([4u8; 32]);
        config.total_kolny_burned = 12_345;

        // A full parameter round trip -- snapshot, overlay, store -- must leave
        // both untouched. `store_into` writes every parameter it knows about,
        // so if the mint or the counter were ever added to that set this would
        // reset them.
        let mut merged = InitColonyParams::from_config(&config);
        let mut patch = empty_patch();
        patch.admission_burn_amount = Some(MIN_ADMISSION_BURN_BASE_UNITS);
        patch.overlay(&mut merged);
        merged.validate().unwrap();
        merged.store_into(&mut config);

        assert_eq!(config.kolny_mint, Pubkey::new_from_array([4u8; 32]));
        assert_eq!(config.total_kolny_burned, 12_345);
        assert_eq!(config.admission_burn_amount, MIN_ADMISSION_BURN_BASE_UNITS);
    }

    /// A patch that is legal on its own but illegal beside the values it lands
    /// next to must still be rejected. This is the case that a per-field check
    /// on the patch alone would let through.
    #[test]
    fn a_patch_is_validated_against_the_values_it_lands_beside() {
        let mut patch = empty_patch();
        // In range on its own (500..4000), but above the default slash
        // threshold of 3000.
        patch.dd_probation_bps = Some(4_000);

        let mut merged = published_defaults();
        patch.overlay(&mut merged);

        assert!(merged.validate().is_err());
    }
}
