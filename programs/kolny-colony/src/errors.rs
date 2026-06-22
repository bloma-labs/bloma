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
    #[msg("Wrong settlement phase for this instruction")]
    WrongPhase,
    #[msg("Bond is below the configured minimum")]
    BelowMinBond,
    #[msg("Forager is not a scout")]
    ForagerNotScout,
    #[msg("Forager is not active")]
    ForagerNotActive,
    #[msg("Forager is retired or slashed")]
    ForagerInactive,
    #[msg("Signer is not the colony authority")]
    NotAuthority,
    #[msg("No pending authority to accept")]
    NoPendingAuthority,
    #[msg("Promotion criteria not met")]
    PromotionCriteriaNotMet,
    #[msg("Registration and retirement are frozen during settlement")]
    RegistrationFrozenDuringSettlement,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Mint does not match the colony base mint")]
    BaseMintMismatch,
    #[msg("Token account owner or mint does not match the expected vault")]
    VaultMismatch,
    #[msg("Forager still holds principal; settle and unwind before retiring")]
    ForagerStillDeployed,
}
