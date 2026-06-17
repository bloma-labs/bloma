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
