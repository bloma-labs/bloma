//! KOLNY colony program.
//!
//! The colony allocates depositor capital across foragers -- operator-run
//! agents that trade from isolated sub-accounts -- in proportion to a decaying
//! trail score built from realized, on-chain performance.
//!
//! What this program does: accounting, allocation, settlement and loss
//! containment. What it deliberately does NOT do: trade, or price open
//! positions. There is no price oracle in the allocation core. Performance is
//! measured only as base-asset value that actually returned to a forager's
//! sub-account, which is what makes the published figures realized rather than
//! marked, and removes the largest manipulation surface. Foragers do not submit
//! their own performance numbers; every input to the trail is read from chain
//! state.
//!
//! Chain safety: no instruction here is wired into any automated deploy path,
//! and building this program never contacts a cluster.

use anchor_lang::prelude::*;

pub mod constants;
pub mod errors;
pub mod events;
pub mod instructions;
pub mod math;
pub mod state;
pub mod utils;

use instructions::*;

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

#[program]
pub mod kolny_colony {
    use super::*;

    // -- colony configuration ------------------------------------------------

    pub fn initialize_colony(ctx: Context<InitializeColony>, params: InitColonyParams) -> Result<()> {
        admin::initialize_colony(ctx, params)
    }

    pub fn update_config(ctx: Context<UpdateConfig>, patch: ConfigPatch) -> Result<()> {
        admin::update_config(ctx, patch)
    }

    pub fn propose_authority(ctx: Context<ProposeAuthority>, new_authority: Pubkey) -> Result<()> {
        admin::propose_authority(ctx, new_authority)
    }

    pub fn accept_authority(ctx: Context<AcceptAuthority>) -> Result<()> {
        admin::accept_authority(ctx)
    }

    pub fn set_paused(ctx: Context<SetPaused>, paused: bool) -> Result<()> {
        admin::set_paused(ctx, paused)
    }

    // -- singleton account creation (split so no context carries several inits)

    pub fn initialize_brood(ctx: Context<InitializeBrood>) -> Result<()> {
        admin::initialize_brood(ctx)
    }

    pub fn open_vault_base(ctx: Context<OpenVaultBase>) -> Result<()> {
        admin::open_vault_base(ctx)
    }

    pub fn initialize_risk_cache(ctx: Context<InitializeRiskCache>) -> Result<()> {
        admin::initialize_risk_cache(ctx)
    }

    pub fn open_cache_vault(ctx: Context<OpenCacheVault>) -> Result<()> {
        admin::open_cache_vault(ctx)
    }

    pub fn open_incinerator_vault(ctx: Context<OpenIncineratorVault>) -> Result<()> {
        admin::open_incinerator_vault(ctx)
    }

    pub fn initialize_trail_board(ctx: Context<InitializeTrailBoard>) -> Result<()> {
        admin::initialize_trail_board(ctx)
    }

    // -- forager lifecycle ---------------------------------------------------

    /// Creates the forager record only. The sub-account is opened by
    /// `open_forager_vault` (its address derives from this record, so it cannot
    /// exist first) and the bond is posted by `top_up_bond`. A forager starts
    /// as a Scout with a zero bond and receives no main-pool capital until it
    /// is promoted, so the three-step bootstrap is safe to interleave.
    pub fn register_forager(
        ctx: Context<RegisterForager>,
        forager_id: u64,
        strategy_meta: [u8; 32],
    ) -> Result<()> {
        forager::register_forager(ctx, forager_id, strategy_meta)
    }

    pub fn open_forager_vault(ctx: Context<OpenForagerVault>, forager_id: u64) -> Result<()> {
        forager::open_forager_vault(ctx, forager_id)
    }
}
