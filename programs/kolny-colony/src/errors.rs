//! Program errors.

use anchor_lang::prelude::*;

#[error_code]
pub enum ColonyError {
    #[msg("Arithmetic overflow")]
    Overflow,
    #[msg("Parameter outside the published range")]
    ParamOutOfRange,
    #[msg("Colony is paused")]
    Paused,
    #[msg("Signer is not the colony authority")]
    NotAuthority,
    #[msg("No pending authority to accept")]
    NoPendingAuthority,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Mint does not match the colony base mint")]
    BaseMintMismatch,
    #[msg("Token account owner or mint does not match the expected vault")]
    VaultMismatch,
}
