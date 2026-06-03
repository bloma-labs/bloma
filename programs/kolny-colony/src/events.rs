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
