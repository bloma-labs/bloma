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
    #[msg("Epoch has not ended yet")]
    EpochNotOver,
    #[msg("Colony is not settling")]
    NotSettling,
    #[msg("Not enough idle liquidity; use request_redemption")]
    InsufficientIdleLiquidity,
    #[msg("Bond is below the configured minimum")]
    BelowMinBond,
    #[msg("Deposit is below the configured minimum")]
    BelowMinimumDeposit,
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
    #[msg("Redemption request is not ready to be fulfilled")]
    RedemptionNotReady,
    #[msg("Redemption request has already been fully paid")]
    RedemptionAlreadyFulfilled,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Insufficient shares in this position")]
    InsufficientShares,
    #[msg("Mint does not match the colony base mint")]
    BaseMintMismatch,
    #[msg("Token account owner or mint does not match the expected vault")]
    VaultMismatch,
    #[msg("Forager still holds principal; settle and unwind before retiring")]
    ForagerStillDeployed,
    #[msg("Colony must have at least one active forager")]
    NoActiveForagers,
}
