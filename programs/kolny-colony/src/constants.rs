//! Tuning parameters for the colony.
//!
//! Defaults and ranges are taken from the parameter tables in
//! `docs/allocation-spec.md` section 11 and `docs/risk-spec.md` section 7.
//! Those tables are the source of truth; nothing here may drift from them.
//!
//! Every mutable parameter is range-checked on `initialize_colony` and on every
//! `update_config`, so an authority cannot move the colony outside the published
//! bounds even by mistake.

pub const BPS_DENOM: u16 = 10_000;

/// PDA seeds. These strings are the contract between the program, the indexer
/// and the front end; the README carries the same table and the two must match
/// character for character.
pub const SEED_COLONY: &[u8] = b"colony";
pub const SEED_BROOD: &[u8] = b"brood";
pub const SEED_CACHE: &[u8] = b"cache";
pub const SEED_TRAIL_BOARD: &[u8] = b"trail_board";
pub const SEED_FORAGER: &[u8] = b"forager";
pub const SEED_FORAGER_VAULT: &[u8] = b"forager_vault";
pub const SEED_BROOD_VAULT: &[u8] = b"brood_vault";
pub const SEED_CACHE_VAULT: &[u8] = b"cache_vault";
pub const SEED_INCINERATOR: &[u8] = b"incinerator";
pub const SEED_POSITION: &[u8] = b"position";
pub const SEED_REDEEM: &[u8] = b"redeem";
