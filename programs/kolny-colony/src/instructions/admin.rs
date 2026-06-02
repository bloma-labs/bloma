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
    pub promote_min_trades: Option<u16>,
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
            self.promote_min_trades,
            MIN_PROMOTE_MIN_TRADES,
            MAX_PROMOTE_MIN_TRADES
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
        if let Some(v) = self.promote_min_trades {
            base.promote_min_trades = v;
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
            promote_min_trades: DEFAULT_PROMOTE_MIN_TRADES,
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
            promote_min_trades: None,
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
        }
    }

    /// A default that sits outside its own published range would make genesis
    /// with the documented parameters impossible. Cheap check, real failure.
    #[test]
    fn published_defaults_pass_the_range_gate() {
        assert!(published_defaults().validate().is_ok());
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
