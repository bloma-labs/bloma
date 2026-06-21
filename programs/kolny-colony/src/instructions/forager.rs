//! Forager lifecycle: registration, sub-account creation, bonding, promotion,
//! retirement and liveness.
//!
//! Bootstrap order is forced by the PDA graph and is not a matter of taste. The
//! sub-account derives from the forager record (`[b"forager_vault",
//! forager_state]`), so the record has to exist before the token account can be
//! addressed at all. That splits what would be one instruction into three:
//!
//!   1. `register_forager`   -- the record only. Bond is 0, status is Scout.
//!   2. `open_forager_vault` -- the isolated base-asset sub-account.
//!   3. `top_up_bond`        -- the operator posts skin in the game.
//!
//! Leaving a forager sitting between steps is safe: a Scout receives no capital
//! from the pheromone-weighted main pool, and promotion out of Scout is gated on
//! a bond at or above `min_bond`. The split also keeps every context to at most
//! one `init`, which is what keeps these handlers inside the 4096-byte stack
//! frame.
//!
//! Accounting note carried through this whole file: a forager's sub-account
//! holds `bond + principal` in one balance, and `ForagerState.bond` is the
//! separate record of how much of that balance is the operator's own collateral.
//! Settlement subtracts the bond before it measures performance, and retirement
//! returns exactly the bond to the operator and sweeps the rest to the Brood
//! Vault. Neither side may treat the raw token balance as colony capital.

use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::constants::{
    PHASE_OPEN, SEED_BROOD, SEED_BROOD_VAULT, SEED_COLONY, SEED_FORAGER, SEED_FORAGER_VAULT,
    STATUS_ACTIVE, STATUS_RETIRED, STATUS_SCOUT,
};
use crate::errors::ColonyError;
use crate::events::{
    BondToppedUp, ForagerPromoted, ForagerRegistered, ForagerRetired, ForagerVaultOpened,
};
use crate::state::{BroodVaultState, ColonyConfig, ForagerState};
use crate::utils;

// ---------------------------------------------------------------------------
// register_forager
// ---------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(forager_id: u64)]
pub struct RegisterForager<'info> {
    #[account(mut)]
    pub operator: Signer<'info>,

    #[account(
        mut,
        seeds = [SEED_COLONY],
        bump = config.bump,
    )]
    pub config: Box<Account<'info, ColonyConfig>>,

    #[account(
        init,
        payer = operator,
        space = 8 + ForagerState::LEN,
        seeds = [SEED_FORAGER, operator.key().as_ref(), &forager_id.to_le_bytes()],
        bump,
    )]
    pub forager: Box<Account<'info, ForagerState>>,

    pub system_program: Program<'info, System>,
}

/// Creates the forager record. Moves no tokens.
///
/// Registration is frozen while the colony is settling. The settlement crank
/// finishes on `settled_count == settleable_forager_count`, so allowing the
/// population to grow mid-crank would move the finish line underneath it.
pub fn register_forager(
    ctx: Context<RegisterForager>,
    forager_id: u64,
    strategy_meta: [u8; 32],
) -> Result<()> {
    require!(
        ctx.accounts.config.epoch_phase == PHASE_OPEN,
        ColonyError::RegistrationFrozenDuringSettlement
    );
    require!(!ctx.accounts.config.paused, ColonyError::Paused);

    let now = Clock::get()?.unix_timestamp;
    let current_epoch = ctx.accounts.config.epoch;
    let operator_key = ctx.accounts.operator.key();
    let forager_bump = ctx.bumps.forager;

    let forager = &mut ctx.accounts.forager;

    forager.operator = operator_key;
    // Set by `open_forager_vault`; there is no sub-account yet.
    forager.forager_vault = Pubkey::default();
    forager.forager_id = forager_id;
    forager.strategy_meta = strategy_meta;

    forager.bond = 0;
    forager.principal = 0;
    forager.pheromone = 0;
    forager.high_water = 0;

    forager.registered_epoch = current_epoch;
    // Provisional. `open_forager_vault` resets this to the value the settlement
    // crank actually needs, at the moment the forager starts counting toward
    // the completion target.
    forager.last_settled_epoch = current_epoch;

    forager.realized_epochs = 0;
    forager.scout_epochs = 0;
    forager.last_scout_epoch = 0;

    forager.realized_pnl_cumulative = 0;
    forager.scout_perf_cum_bps = 0;
    forager.probation_until_epoch = 0;
    forager.last_heartbeat_ts = now;

    forager.max_drawdown_bps = 0;
    forager.current_drawdown_bps = 0;
    forager.slash_count = 0;

    // Every forager enters through the Scout Sandbox on small fixed tickets.
    // There is no path that starts a new operator on main-pool capital.
    forager.status = STATUS_SCOUT;
    forager.bump = forager_bump;
    forager.vault_bump = 0;
    forager._padding = [0u8; 7];

    let forager_key = forager.key();

    // Deliberately NOT counted in `settleable_forager_count` here. See
    // `open_forager_vault`: a record with no sub-account has no balance to
    // measure, and counting it would let anyone permanently stall the crank.

    emit!(ForagerRegistered {
        forager: forager_key,
        operator: operator_key,
        forager_id,
        // Always 0 here. The bond is posted separately by `top_up_bond`, and
        // the indexer should read the `BondToppedUp` stream for the real figure.
        bond: 0,
        registered_epoch: current_epoch,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// open_forager_vault
// ---------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(forager_id: u64)]
pub struct OpenForagerVault<'info> {
    #[account(mut)]
    pub operator: Signer<'info>,

    #[account(
        mut,
        seeds = [SEED_COLONY],
        bump = config.bump,
    )]
    pub config: Box<Account<'info, ColonyConfig>>,

    #[account(
        mut,
        seeds = [SEED_FORAGER, operator.key().as_ref(), &forager_id.to_le_bytes()],
        bump = forager.bump,
        has_one = operator,
    )]
    pub forager: Box<Account<'info, ForagerState>>,

    #[account(
        constraint = base_mint.key() == config.base_mint @ ColonyError::BaseMintMismatch,
    )]
    pub base_mint: Box<InterfaceAccount<'info, Mint>>,

    /// The forager's isolated sub-account. Its authority is the forager record
    /// PDA, so the operator can never move base asset out of it by signing
    /// directly; only this program can, and only along the paths written here.
    #[account(
        init,
        payer = operator,
        seeds = [SEED_FORAGER_VAULT, forager.key().as_ref()],
        bump,
        token::mint = base_mint,
        token::authority = forager,
        token::token_program = token_program,
    )]
    pub forager_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

/// Opens the forager's isolated base-asset sub-account.
///
/// `init` makes this idempotent in the only direction that matters: a second
/// call on an existing sub-account fails rather than replacing the address a
/// balance is already sitting behind.
pub fn open_forager_vault(ctx: Context<OpenForagerVault>, _forager_id: u64) -> Result<()> {
    // Opening the sub-account is what makes a forager settleable, so the
    // population must not grow while a settlement is in flight; that would move
    // the crank's finish line underneath it.
    require!(
        ctx.accounts.config.epoch_phase == PHASE_OPEN,
        ColonyError::RegistrationFrozenDuringSettlement
    );

    let vault_key = ctx.accounts.forager_vault.key();
    let vault_bump = ctx.bumps.forager_vault;
    let current_epoch = ctx.accounts.config.epoch;

    let forager = &mut ctx.accounts.forager;
    forager.forager_vault = vault_key;
    forager.vault_bump = vault_bump;

    // The crank guard is `last_settled_epoch < settling_epoch`, and the next
    // settlement runs with `settling_epoch == config.epoch`. Marking the
    // forager as last settled one epoch back is what lets its first
    // `settle_forager` call succeed. Marking it at the current epoch would make
    // that call permanently unsatisfiable while the forager still counted
    // toward the completion target, which would deadlock every future
    // finalize. Epoch numbering starts at 1 so this subtraction is always
    // meaningful. Settling a forager that has done nothing yet is harmless: it
    // measures zero principal and deposits no pheromone.
    forager.last_settled_epoch = current_epoch.saturating_sub(1);

    let forager_key = forager.key();

    // A forager counts toward the crank only once it has a sub-account to
    // measure. Counting it at registration instead would let anyone register a
    // record they never fund and permanently stall settlement, because
    // `settle_forager` cannot resolve a sub-account that does not exist and
    // `finalize_settlement` requires every counted forager to have settled.
    let config = &mut ctx.accounts.config;
    config.settleable_forager_count = config
        .settleable_forager_count
        .checked_add(1)
        .ok_or(ColonyError::Overflow)?;

    emit!(ForagerVaultOpened {
        forager: forager_key,
        forager_vault: vault_key,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// top_up_bond
// ---------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(forager_id: u64)]
pub struct TopUpBond<'info> {
    pub operator: Signer<'info>,

    #[account(
        mut,
        seeds = [SEED_FORAGER, operator.key().as_ref(), &forager_id.to_le_bytes()],
        bump = forager.bump,
        has_one = operator,
    )]
    pub forager: Box<Account<'info, ForagerState>>,

    pub base_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        seeds = [SEED_FORAGER_VAULT, forager.key().as_ref()],
        bump = forager.vault_bump,
        constraint = forager_vault.key() == forager.forager_vault @ ColonyError::VaultMismatch,
        token::mint = base_mint,
        token::authority = forager,
    )]
    pub forager_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = base_mint,
        token::authority = operator,
    )]
    pub operator_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
}

/// Posts additional bond into the forager's sub-account.
///
/// The transferred amount lands in the same token account the forager trades
/// from, and `forager.bond` records how much of that balance belongs to the
/// operator. Settlement subtracts `bond` before measuring realized performance,
/// which is what stops a bond top-up from reading as a trading profit and
/// inflating the trail.
pub fn top_up_bond(ctx: Context<TopUpBond>, _forager_id: u64, amount: u64) -> Result<()> {
    require!(amount > 0, ColonyError::ZeroAmount);

    // A retired forager's sub-account has already been drained and swept, and
    // `retire_forager` refuses to run twice. Base asset sent in after that
    // point would have no way back out, so the deposit is refused instead.
    require!(
        ctx.accounts.forager.status != STATUS_RETIRED,
        ColonyError::ForagerInactive
    );

    utils::transfer_from_user(
        &ctx.accounts.token_program,
        &ctx.accounts.operator_token_account,
        &ctx.accounts.forager_vault,
        &ctx.accounts.base_mint,
        &ctx.accounts.operator,
        amount,
    )?;

    let forager = &mut ctx.accounts.forager;
    forager.bond = forager
        .bond
        .checked_add(amount)
        .ok_or(ColonyError::Overflow)?;

    emit!(BondToppedUp {
        forager: forager.key(),
        amount,
        bond: forager.bond,
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// promote_forager
// ---------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(forager_id: u64)]
pub struct PromoteForager<'info> {
    /// Permissionless caller. Promotion is decided entirely from committed
    /// on-chain state, so anyone may push a qualifying scout through.
    pub cranker: Signer<'info>,

    /// CHECK: not read and never signs. It is pinned to `forager.operator` by
    /// `has_one` and by the forager PDA seeds, and exists only so the record's
    /// address can be derived.
    pub operator: UncheckedAccount<'info>,

    #[account(
        seeds = [SEED_COLONY],
        bump = config.bump,
    )]
    pub config: Box<Account<'info, ColonyConfig>>,

    #[account(
        mut,
        seeds = [SEED_FORAGER, operator.key().as_ref(), &forager_id.to_le_bytes()],
        bump = forager.bump,
        has_one = operator,
    )]
    pub forager: Box<Account<'info, ForagerState>>,
}

/// Promotes a Scout into the pheromone-weighted main pool.
///
/// Permissionless by design. Every criterion below is read from state this
/// program wrote itself during settlement, so there is nothing for an operator
/// to submit and nothing for the authority to wave through.
pub fn promote_forager(ctx: Context<PromoteForager>, _forager_id: u64) -> Result<()> {
    require!(
        ctx.accounts.forager.status == STATUS_SCOUT,
        ColonyError::ForagerNotScout
    );

    let config = &ctx.accounts.config;
    let forager = &mut ctx.accounts.forager;

    // `promote_min_trades` is evaluated against `realized_epochs`, the number of
    // settled epochs that closed with a non-zero realized result. The program
    // cannot observe individual trades: it sees sub-account balances at
    // settlement, not fills. The alternative -- having the operator submit a
    // trade count -- would create a trusted oracle over a promotion gate, which
    // is exactly the manipulation surface this design removes elsewhere. So the
    // gate is stated in the units the chain can actually verify.
    let criteria_met = forager.scout_epochs >= config.promote_min_epochs as u64
        && forager.realized_epochs >= config.promote_min_trades as u64
        && forager.scout_perf_cum_bps >= config.promote_perf_bar_bps as i64
        && forager.realized_pnl_cumulative >= 0;
    require!(criteria_met, ColonyError::PromotionCriteriaNotMet);

    // Skin in the game gates capital. A scout with a thin bond stays a scout.
    require!(forager.bond >= config.min_bond, ColonyError::BelowMinBond);

    // Seed the trail from scout-phase performance but cap it, so a promotion
    // cannot place a brand-new operator at the top of the trail and hand it the
    // concentration cap on its first main-pool epoch.
    let seeded_pheromone = forager.pheromone.min(config.promote_tau_seed_cap);
    forager.pheromone = seeded_pheromone;
    forager.status = STATUS_ACTIVE;

    // `active_forager_count` is deliberately untouched: it is the population
    // that was Active at the last finalize, and only settlement may move it.
    // `settleable_forager_count` already counts this forager from registration.

    emit!(ForagerPromoted {
        forager: forager.key(),
        epoch: config.epoch,
        seeded_pheromone,
    });

    Ok(())
}
