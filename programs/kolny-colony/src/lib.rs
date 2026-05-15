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
pub mod math;
pub mod state;

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

#[program]
pub mod kolny_colony {
    use super::*;
}
