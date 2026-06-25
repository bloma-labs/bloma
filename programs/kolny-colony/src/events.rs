//! Events.
//!
//! The indexer rebuilds the public history from these alone: registration,
//! deposits, withdrawals, settlement, rebalancing, promotion, slashing and
//! cache coverage are all covered, so drawdown and slash history can be shown
//! continuously without trusting any off-chain record.

use anchor_lang::prelude::*;

#[event]
pub struct ColonyInitialized {
    pub authority: Pubkey,
    pub base_mint: Pubkey,
    pub epoch_duration_secs: i64,
    pub epoch_end_ts: i64,
}

#[event]
pub struct ConfigUpdated {
    pub authority: Pubkey,
    pub epoch: u64,
}

#[event]
pub struct AuthorityProposed {
    pub current: Pubkey,
    pub pending: Pubkey,
}

#[event]
pub struct AuthorityAccepted {
    pub previous: Pubkey,
    pub current: Pubkey,
}

#[event]
pub struct PausedSet {
    pub authority: Pubkey,
    pub paused: bool,
}

#[event]
pub struct BroodInitialized {
    pub base_mint: Pubkey,
    pub vault_base: Pubkey,
}

#[event]
pub struct RiskCacheInitialized {
    pub cache_vault: Pubkey,
    pub incinerator_vault: Pubkey,
}

#[event]
pub struct ForagerRegistered {
    pub forager: Pubkey,
    pub operator: Pubkey,
    pub forager_id: u64,
    pub bond: u64,
    pub registered_epoch: u64,
}

#[event]
pub struct ForagerVaultOpened {
    pub forager: Pubkey,
    pub forager_vault: Pubkey,
}

#[event]
pub struct ForagerPromoted {
    pub forager: Pubkey,
    pub epoch: u64,
    pub seeded_pheromone: u64,
}

#[event]
pub struct ForagerDemoted {
    pub forager: Pubkey,
    pub epoch: u64,
    pub weight_bps: u16,
    pub returned_principal: u64,
}

#[event]
pub struct ForagerRetired {
    pub forager: Pubkey,
    pub operator: Pubkey,
    pub returned_bond: u64,
    pub swept_base: u64,
}

#[event]
pub struct BondToppedUp {
    pub forager: Pubkey,
    pub amount: u64,
    pub bond: u64,
}

#[event]
pub struct Deposited {
    pub depositor: Pubkey,
    pub assets: u64,
    pub shares: u128,
    pub nav_after: u64,
    pub total_shares_after: u128,
}

#[event]
pub struct Withdrawn {
    pub depositor: Pubkey,
    pub shares: u128,
    pub assets: u64,
    pub nav_after: u64,
    pub total_shares_after: u128,
}

#[event]
pub struct RedemptionRequested {
    pub depositor: Pubkey,
    pub request_id: u64,
    pub shares: u128,
    pub requested_epoch: u64,
}

#[event]
pub struct RedemptionFulfilled {
    pub depositor: Pubkey,
    pub request_id: u64,
    pub shares_burned: u128,
    pub assets_paid: u64,
    pub fully_settled: bool,
}

#[event]
pub struct CacheFunded {
    pub funder: Pubkey,
    pub amount: u64,
    pub cache_balance: u64,
}
