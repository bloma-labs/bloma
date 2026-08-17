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
pub struct SettlementBegan {
    pub settling_epoch: u64,
    pub active_forager_count: u32,
    pub started_at: i64,
}

#[event]
pub struct ForagerSettled {
    pub forager: Pubkey,
    pub settling_epoch: u64,
    pub realized_pnl_epoch: i64,
    pub return_bps: i64,
    pub perf_bps: i64,
    pub deposit_fp6: i64,
    pub pheromone: u64,
    pub principal_after: u64,
    pub drawdown_bps: u16,
}

#[event]
pub struct LossCovered {
    pub forager: Pubkey,
    pub loss: u64,
    pub from_bond: u64,
    pub from_cache: u64,
    pub to_depositors: u64,
}

#[event]
pub struct SettlementFinalized {
    pub epoch: u64,
    pub pheromone_sum: u128,
    pub alloc_divisor: u128,
    pub allocatable_pool: u64,
    pub scout_pool: u64,
    pub nav: u64,
}

#[event]
pub struct ForagerRebalanced {
    pub forager: Pubkey,
    pub epoch: u64,
    pub target: u64,
    pub principal_before: u64,
    pub principal_after: u64,
    pub weight_bps: u16,
}

#[event]
pub struct ScoutFunded {
    pub forager: Pubkey,
    pub epoch: u64,
    pub ticket: u64,
    pub scout_pool_remaining: u64,
}

#[event]
pub struct ForagerSlashed {
    pub forager: Pubkey,
    pub operator: Pubkey,
    pub reason_code: u8,
    pub slashed: u64,
    pub burned: u64,
    pub to_cache: u64,
    pub bond_after: u64,
    pub slash_count: u16,
}

#[event]
pub struct CacheFunded {
    pub funder: Pubkey,
    pub amount: u64,
    pub cache_balance: u64,
}

#[event]
pub struct CacheAccrued {
    pub epoch: u64,
    pub amount: u64,
    pub cache_balance: u64,
}

/// The one-time write that tells the colony which mint admission destroys.
///
/// `decimals` and `mint_authority_is_none` are what the program actually read
/// off the mint account at the moment it bound to it, recorded so the binding
/// is publicly auditable after the fact rather than asserted in copy.
///
/// All three are also REQUIRED, not merely recorded: `set_kolny_mint` refuses a
/// mint whose decimals disagree with `KOLNY_DECIMALS`, whose mint authority is
/// live, or whose freeze authority is live. They are published anyway so the
/// claims that rest on them stay checkable from the event stream alone, without
/// anyone having to re-read the mint and trust that it never changed. Since both
/// authorities are already revoked when this succeeds, neither can come back.
///
/// `mint_authority_is_none` carries the supply claim: an admission burn is a
/// one-way reduction only if nobody can mint more. `freeze_authority_is_none`
/// carries the admission claim: a frozen account cannot burn, so a live freeze
/// authority would let someone choose who is allowed into the colony.
#[event]
pub struct KolnyMintSet {
    pub authority: Pubkey,
    pub kolny_mint: Pubkey,
    pub decimals: u8,
    pub mint_authority_is_none: bool,
    pub freeze_authority_is_none: bool,
}

/// $KOLNY actually destroyed at a forager's admission.
///
/// Separate from `ForagerRegistered` on purpose: this is the stream the public
/// burn figure is rebuilt from, and it must carry only supply that a
/// `burn_checked` CPI really removed. Nothing else in this program reduces any
/// token's supply -- the slash "burn" moves base asset to a locked sink and
/// says so -- and the treasury buyback of `docs/risk-spec.md` 4.3 happens
/// outside this program entirely, so it never appears here.
#[event]
pub struct KolnyBurned {
    pub forager: Pubkey,
    pub operator: Pubkey,
    pub kolny_mint: Pubkey,
    pub amount: u64,
    pub total_kolny_burned: u64,
    pub epoch: u64,
}
