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

// ---------------------------------------------------------------------------
// Epoch  (allocation-spec 11: epoch_duration_secs)
// ---------------------------------------------------------------------------

/// 7 days.
pub const DEFAULT_EPOCH_DURATION_SECS: i64 = 604_800;
/// 1 day.
pub const MIN_EPOCH_DURATION_SECS: i64 = 86_400;
/// 30 days.
pub const MAX_EPOCH_DURATION_SECS: i64 = 2_592_000;

// ---------------------------------------------------------------------------
// Pheromone update  (allocation-spec 11)
// ---------------------------------------------------------------------------

/// Evaporation rate. 1600 = 0.16, a half-life of about 4 epochs.
pub const DEFAULT_RHO_BPS: u16 = 1_600;
pub const MIN_RHO_BPS: u16 = 100;
pub const MAX_RHO_BPS: u16 = 5_000;

/// Deposit scale `Q` in FP6. The largest change one epoch can make to a trail.
pub const DEFAULT_DEPOSIT_SCALE_Q: u64 = 1_000_000;
pub const MIN_DEPOSIT_SCALE_Q: u64 = 100_000;
pub const MAX_DEPOSIT_SCALE_Q: u64 = 10_000_000;

/// Performance normalization scale `s`. 1000 = 10%.
pub const DEFAULT_PERF_NORM_S_BPS: u16 = 1_000;
pub const MIN_PERF_NORM_S_BPS: u16 = 100;
pub const MAX_PERF_NORM_S_BPS: u16 = 5_000;

/// Risk aversion `lambda`. 10000 = 1.0x drawdown penalty.
pub const DEFAULT_RISK_AVERSION_BPS: u16 = 10_000;
pub const MIN_RISK_AVERSION_BPS: u16 = 0;
pub const MAX_RISK_AVERSION_BPS: u16 = 50_000;

/// Fixed-point scale for pheromone and deposits.
pub const FP6_SCALE: u64 = 1_000_000;

/// Pheromone floor. MUST stay 0 so a dead trail can actually evaporate away.
pub const PHEROMONE_FLOOR: u64 = 0;
/// Overflow bound on a trail, far above any economically reachable value
/// (steady state is `Q / rho`, about 6.25e6 at the defaults).
pub const PHEROMONE_CEIL: u64 = 1_000_000_000_000;

// ---------------------------------------------------------------------------
// Weights  (allocation-spec 11)
// ---------------------------------------------------------------------------

/// Single-forager concentration cap. 2000 = 20%.
pub const DEFAULT_W_MAX_BPS: u16 = 2_000;
pub const MIN_W_MAX_BPS: u16 = 500;
pub const MAX_W_MAX_BPS: u16 = 4_000;

/// Demotion threshold, not a floor. 300 = 3%.
pub const DEFAULT_W_DROP_BPS: u16 = 300;
pub const MIN_W_DROP_BPS: u16 = 0;
pub const MAX_W_DROP_BPS: u16 = 1_000;

/// Margin added when the cap has to relax because `n_active * w_max < 1`.
pub const CAP_RELAX_MARGIN_BPS: u16 = 100;

/// Exploration reserve. 1000 = 10% of value under colony.
pub const DEFAULT_SCOUT_BUDGET_BPS: u16 = 1_000;
pub const MIN_SCOUT_BUDGET_BPS: u16 = 500;
pub const MAX_SCOUT_BUDGET_BPS: u16 = 2_000;

/// No-trade band. 200 = 2%.
pub const DEFAULT_REBAND_BAND_BPS: u16 = 200;
pub const MIN_REBAND_BAND_BPS: u16 = 0;
pub const MAX_REBAND_BAND_BPS: u16 = 1_000;

/// Turnover cap. 2500 = 25% of the pool per epoch.
pub const DEFAULT_TURNOVER_CAP_BPS: u16 = 2_500;
pub const MIN_TURNOVER_CAP_BPS: u16 = 500;
pub const MAX_TURNOVER_CAP_BPS: u16 = 10_000;

// ---------------------------------------------------------------------------
// Scout promotion  (allocation-spec 11)
// ---------------------------------------------------------------------------

pub const DEFAULT_PROMOTE_MIN_EPOCHS: u8 = 4;
pub const MIN_PROMOTE_MIN_EPOCHS: u8 = 1;
pub const MAX_PROMOTE_MIN_EPOCHS: u8 = 52;

/// Scout epochs that must have closed with a non-zero realized result.
///
/// This is deliberately NOT a fill count. The specification originally asked
/// for `promote_min_trades`, a number of realized closed trades, but the program
/// cannot observe individual fills -- only settled epoch outcomes. So the gate
/// is stated in the unit the chain can actually verify, and the parameter is
/// named for what it counts. The specification has since been corrected to
/// match.
///
/// The default is chosen against `DEFAULT_PROMOTE_MIN_EPOCHS`, not against the
/// original trade count. Carrying 20 over would have meant 20 active
/// epochs, which at a 7-day epoch is about 140 days, and it would have made the
/// tenure requirement dead code because it would always bind first. Three of the
/// four required scout epochs must show real activity, so a scout cannot idle
/// its way to promotion, and the timeline stays the roughly one month the
/// specification intended.
pub const DEFAULT_PROMOTE_MIN_REALIZED_EPOCHS: u16 = 3;
pub const MIN_PROMOTE_MIN_REALIZED_EPOCHS: u16 = 1;
pub const MAX_PROMOTE_MIN_REALIZED_EPOCHS: u16 = 52;

pub const DEFAULT_PROMOTE_PERF_BAR_BPS: i32 = 0;
pub const MIN_PROMOTE_PERF_BAR_BPS: i32 = -5_000;
pub const MAX_PROMOTE_PERF_BAR_BPS: i32 = 20_000;

/// Cap on the pheromone a scout carries into the main pool at promotion, so it
/// cannot enter at the top of the trail. FP6.
pub const DEFAULT_PROMOTE_TAU_SEED_CAP: u64 = 1_000_000;
pub const MAX_PROMOTE_TAU_SEED_CAP: u64 = 5_000_000;

// ---------------------------------------------------------------------------
// Bond and slashing  (risk-spec 7)
// ---------------------------------------------------------------------------

/// Bond as a fraction of allocation. 1000 = 10%.
pub const DEFAULT_BOND_RATIO_BPS: u16 = 1_000;
pub const MIN_BOND_RATIO_BPS: u16 = 500;
pub const MAX_BOND_RATIO_BPS: u16 = 5_000;

/// Share of a posted bond not recognized as allocation capacity. 3000 = 30%.
///
/// **Not a price haircut.** Bond, principal, cache and losses are all the same
/// base asset and this program reads no price oracle. Earlier drafts denominated
/// the bond in a separate token and discounted it against that token's price;
/// that design was replaced by a single-base-asset bond so the loss waterfall
/// can settle without a swap, and this parameter's justification changed with
/// it.
///
/// What it actually guards: the bond sits inside the forager's own sub-account,
/// which is the account the operator trades from, so a loss that exhausts
/// principal continues into the bond itself (see `math::recoverable_bond`).
/// `bond_capacity` is evaluated between settlements against a recorded bond
/// figure that is accurate only as of the last settlement, so recognizing part
/// of it rather than all of it keeps capital from being extended against
/// collateral that may already be partly consumed.
///
/// Direction, because it reads backwards easily: a haircut makes the
/// requirement STRICTER. At a 10 percent bond ratio a forager posts 10 percent
/// of its allocation with no haircut and about 14.3 percent at 30 percent.
pub const DEFAULT_BOND_HAIRCUT_BPS: u16 = 3_000;
pub const MIN_BOND_HAIRCUT_BPS: u16 = 1_000;
pub const MAX_BOND_HAIRCUT_BPS: u16 = 7_000;

/// Current drawdown that puts a forager on probation. 1500 = 15%.
pub const DEFAULT_DD_PROBATION_BPS: u16 = 1_500;
pub const MIN_DD_PROBATION_BPS: u16 = 500;
pub const MAX_DD_PROBATION_BPS: u16 = 4_000;

/// Current drawdown that makes a forager slashable. 3000 = 30%.
pub const DEFAULT_DD_SLASH_BPS: u16 = 3_000;
pub const MIN_DD_SLASH_BPS: u16 = 1_000;
pub const MAX_DD_SLASH_BPS: u16 = 6_000;

/// Per-epoch realized loss that pauses a forager. 1000 = 10%.
pub const DEFAULT_EPOCH_LOSS_LIMIT_BPS: u16 = 1_000;
pub const MIN_EPOCH_LOSS_LIMIT_BPS: u16 = 300;
pub const MAX_EPOCH_LOSS_LIMIT_BPS: u16 = 3_000;

pub const DEFAULT_PROBATION_GRACE_EPOCHS: u8 = 1;
pub const MIN_PROBATION_GRACE_EPOCHS: u8 = 1;
pub const MAX_PROBATION_GRACE_EPOCHS: u8 = 8;

pub const DEFAULT_NONRESPONSE_TIMEOUT_EPOCHS: u8 = 2;
pub const MIN_NONRESPONSE_TIMEOUT_EPOCHS: u8 = 1;
pub const MAX_NONRESPONSE_TIMEOUT_EPOCHS: u8 = 8;

/// Share of a seized bond that is burned; the rest goes to the Risk Cache.
pub const DEFAULT_SLASH_BURN_BPS: u16 = 5_000;
pub const MIN_SLASH_BURN_BPS: u16 = 0;
pub const MAX_SLASH_BURN_BPS: u16 = 10_000;

/// Unlevered only. Fixed at 1 by policy; the field exists so the rule is
/// explicit and auditable on-chain, not so it can be raised.
pub const MAX_LEVERAGE_X: u8 = 1;

/// Largest single asset inside a sub-account. 4000 = 40%.
pub const DEFAULT_MAX_SINGLE_ASSET_BPS: u16 = 4_000;
pub const MIN_MAX_SINGLE_ASSET_BPS: u16 = 1_000;
pub const MAX_MAX_SINGLE_ASSET_BPS: u16 = 10_000;

// ---------------------------------------------------------------------------
// Risk Cache  (risk-spec 7)
// ---------------------------------------------------------------------------

/// Realized profit routed to the cache. 1000 = 10%.
pub const DEFAULT_CACHE_ACCRUAL_BPS: u16 = 1_000;
pub const MIN_CACHE_ACCRUAL_BPS: u16 = 0;
pub const MAX_CACHE_ACCRUAL_BPS: u16 = 5_000;

/// Target cache balance as a fraction of value under colony. 400 = 4%.
pub const DEFAULT_CACHE_RESERVE_TARGET_BPS: u16 = 400;
pub const MIN_CACHE_RESERVE_TARGET_BPS: u16 = 100;
pub const MAX_CACHE_RESERVE_TARGET_BPS: u16 = 1_000;

// ---------------------------------------------------------------------------
// Deposits
// ---------------------------------------------------------------------------

/// Smallest accepted deposit. Together with the virtual share offset this
/// removes the first-depositor rounding attack.
pub const MINIMUM_DEPOSIT: u64 = 1_000;

// ---------------------------------------------------------------------------
// Admission burn  (docs/token-utility-design.md 2.1, 6.1, 6.4)
// ---------------------------------------------------------------------------

// UNITS, because getting this wrong is the expensive mistake in this file.
//
// `$KOLNY` amounts appear here in two units and every name says which:
//   `*_KOLNY`       whole tokens, the numbers a person approved
//   `*_BASE_UNITS`  what the SPL token program actually moves, and the only
//                   unit that is ever stored on-chain or handed to a CPI
//
// The two differ by `10^decimals`, and a mix-up is not a rounding error but a
// factor of a thousand or a billion. This project has already shipped that bug:
// the CLI hard-coded 6 decimals against a 9-decimal asset, so `deposit 1` sent
// 0.001. The defence here is three-layered, and none of the layers is a
// comment:
//
//   1. Every base-unit constant is DERIVED from its whole-token constant times
//      `ONE_KOLNY_BASE_UNITS`. The conversion is written once, not transcribed.
//   2. `set_kolny_mint` REFUSES a mint whose decimals are not `KOLNY_DECIMALS`,
//      so the assumption behind (1) is verified against the real mint at the
//      one moment the mint becomes known, before any burn can happen.
//   3. The burn CPI still passes `mint.decimals` READ FROM THE MINT ACCOUNT,
//      never `KOLNY_DECIMALS`. (2) is a guard on the constants; it is not a
//      substitute for reading the chain, and the two must never be collapsed.

/// Decimals the admission constants below were computed at.
///
/// This is an EXPECTATION THAT IS CHECKED, not a hard-coded substitute for
/// reading the mint. `set_kolny_mint` compares it against the real mint account
/// and refuses a mismatch; `utils::burn_from_user` passes the mint's own
/// decimals to `burn_checked` regardless. If the issued mint turns out to use
/// different decimals, this constant and the whole-token amounts have to be
/// restated together and the program rebuilt -- which is a loud, one-line,
/// pre-deploy fix, and is exactly what should happen. Silently burning a
/// thousandth of the intended admission is not.
///
/// 6 is the pump.fun standard, which is how $KOLNY is being issued.
pub const KOLNY_DECIMALS: u8 = 6;

/// One whole $KOLNY expressed in base units.
pub const ONE_KOLNY_BASE_UNITS: u64 = 10u64.pow(KOLNY_DECIMALS as u32);

/// Total $KOLNY supply, in whole tokens. Fixed at issuance.
///
/// The mint is issued through pump.fun with **no mint authority**, so supply
/// can never be increased. That is what makes the admission burn a one-way
/// reduction rather than a cycle: what `register_forager` destroys cannot be
/// re-minted by anyone, including the colony authority. The percentages below
/// are stated against this number.
pub const KOLNY_TOTAL_SUPPLY_KOLNY: u64 = 1_000_000_000;

/// Admission per registration, in whole $KOLNY. 0.01 percent of total supply.
///
/// A fixed COUNT, never a value. Defining admission as "X dollars of $KOLNY"
/// would need a price oracle; a count needs only the mint and the number. That
/// is the entire reason the token can carry a use here without reopening the
/// oracle surface the bond design was rewritten to close (`docs/risk-spec.md`
/// 1.2, `docs/security.md` 5.4). The burned tokens are destroyed outright: they
/// are not collateral, they never enter the loss waterfall of
/// `docs/risk-spec.md` 4.2, and they are not an input to any allocation.
pub const DEFAULT_ADMISSION_BURN_KOLNY: u64 = 100_000;

/// Floor on admission, in whole $KOLNY. 0.001 percent of total supply.
///
/// Two jobs. The first is to make free admission unrepresentable: at zero the
/// burn would move nothing while registration kept succeeding, and nothing
/// on-chain would distinguish that from a working colony until someone thought
/// to read the counter. The second is to keep the floor economically real
/// rather than nominal, so the authority cannot reduce admission to dust and
/// leave the token's only use in name only.
pub const MIN_ADMISSION_BURN_KOLNY: u64 = 10_000;

/// Ceiling on admission, in whole $KOLNY. 0.1 percent of total supply.
///
/// A compile-time constant rather than a config field on purpose: an authority
/// that could raise its own limit could price admission into a de facto entry
/// ban, or turn registration into extraction. Ten times the default is enough
/// headroom to track a token price that moved by an order of magnitude, which
/// is the only stated reason for the parameter to be mutable at all
/// (`docs/token-utility-design.md` 6.4). Moving this bound takes a program
/// upgrade, which is public and reviewable.
pub const MAX_ADMISSION_BURN_KOLNY: u64 = 1_000_000;

/// Admission amounts in base units. Derived, never transcribed.
///
/// These are the values `ColonyConfig.admission_burn_amount` is range-checked
/// against and the unit that field is stored in.
pub const DEFAULT_ADMISSION_BURN_BASE_UNITS: u64 =
    DEFAULT_ADMISSION_BURN_KOLNY * ONE_KOLNY_BASE_UNITS;
pub const MIN_ADMISSION_BURN_BASE_UNITS: u64 = MIN_ADMISSION_BURN_KOLNY * ONE_KOLNY_BASE_UNITS;
pub const MAX_ADMISSION_BURN_BASE_UNITS: u64 = MAX_ADMISSION_BURN_KOLNY * ONE_KOLNY_BASE_UNITS;

/// Whether a mint's decimals match what the admission constants assume.
///
/// Split out as a function so the rule can be exercised without a validator:
/// `set_kolny_mint` calls exactly this, so the tests drive the deployed rule
/// rather than a second copy of the comparison.
pub const fn admission_decimals_match(mint_decimals: u8) -> bool {
    mint_decimals == KOLNY_DECIMALS
}

/// Total byte length of a `ColonyConfig` account written before the admission
/// burn existed: the 8-byte discriminator plus a 304-byte body.
///
/// This is a HISTORICAL constant and must never be recomputed from the current
/// struct. It is the one number `migrate_colony_config` uses to tell an
/// un-migrated account from a migrated one, which is also what makes that
/// instruction impossible to run twice.
pub const LEGACY_COLONY_CONFIG_ACCOUNT_LEN: usize = 8 + 304;

/// Why the colony refused to bind to a mint.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdmissionMintRejection {
    /// The constants would mean a different number of tokens.
    Decimals,
    /// Someone can still mint more, so a burn is not a permanent reduction.
    MintAuthorityLive,
    /// Someone can still freeze holders, so admission is not open to everyone.
    FreezeAuthorityLive,
}

/// The properties the $KOLNY mint must have before the colony will bind to it.
///
/// Every rejection here fails LOUDLY and EARLY, at the one admin call that
/// happens after the token is issued and before any operator can register. That
/// is the cheap moment to find out: the program is not on mainnet yet, so a
/// refusal costs a rebuild, while accepting a mint that breaks one of these
/// leaves a published claim quietly false instead.
///
/// Taken as a pure function of three observed values so the policy can be
/// exercised without a validator. `set_kolny_mint` calls exactly this, so the
/// tests drive the deployed rule rather than a second copy of it.
///
/// What each check protects, because they are not the same claim:
///
/// - **decimals** protects the ARITHMETIC. The admission constants are base
///   units computed at `KOLNY_DECIMALS`; at other decimals the same stored
///   number silently means a different count of tokens.
/// - **mint authority** protects the SUPPLY claim. "Burning permanently reduces
///   supply" is only true if nobody can mint more. $KOLNY is issued through
///   pump.fun at a fixed 1,000,000,000 with the authority revoked, so a live
///   mint authority is not a configuration to tolerate, it is a signal that the
///   wrong token was passed. Accepting it would let the site and the docs go on
///   saying "permanently" while it was not.
/// - **freeze authority** protects the ADMISSION claim. A live freeze authority
///   can freeze an operator's $KOLNY account, and a frozen account cannot burn,
///   so whoever holds it can decide who is allowed to enter the colony. That
///   does not falsify the supply claim, which is why it is a separate rejection
///   with its own error, but "anyone who burns is admitted" is exactly as public
///   a promise and exactly as false if it can be vetoed. pump.fun revokes this
///   one too, so the same reasoning applies: refusing costs a rebuild before
///   launch, accepting costs a censorable entry gate nobody notices.
pub fn check_admission_mint(
    decimals: u8,
    mint_authority_is_none: bool,
    freeze_authority_is_none: bool,
) -> Option<AdmissionMintRejection> {
    if !admission_decimals_match(decimals) {
        return Some(AdmissionMintRejection::Decimals);
    }
    if !mint_authority_is_none {
        return Some(AdmissionMintRejection::MintAuthorityLive);
    }
    if !freeze_authority_is_none {
        return Some(AdmissionMintRejection::FreezeAuthorityLive);
    }
    None
}

// ---------------------------------------------------------------------------
// Lifecycle enums, stored as u8
// ---------------------------------------------------------------------------

/// Settlement phase of the colony.
pub const PHASE_OPEN: u8 = 0;
pub const PHASE_SETTLING: u8 = 1;

/// Forager lifecycle state.
pub const STATUS_SCOUT: u8 = 0;
pub const STATUS_ACTIVE: u8 = 1;
pub const STATUS_PROBATION: u8 = 2;
pub const STATUS_SLASHED: u8 = 3;
pub const STATUS_RETIRED: u8 = 4;

/// Slash reason codes, recorded on the `ForagerSlashed` event so the public
/// slash history can state why each slash happened.
pub const SLASH_REASON_LOSS_THRESHOLD: u8 = 0;
pub const SLASH_REASON_RULE_VIOLATION: u8 = 1;
pub const SLASH_REASON_NON_RESPONSE: u8 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    /// The approved numbers, in the unit a person approved them in.
    ///
    /// Written as bare literals rather than recomputed from the constants,
    /// because a test that derives its expectation the same way the code does
    /// agrees with the code by construction and checks nothing.
    #[test]
    fn admission_amounts_are_the_approved_whole_token_figures() {
        assert_eq!(DEFAULT_ADMISSION_BURN_KOLNY, 100_000);
        assert_eq!(MIN_ADMISSION_BURN_KOLNY, 10_000);
        assert_eq!(MAX_ADMISSION_BURN_KOLNY, 1_000_000);
        assert_eq!(KOLNY_TOTAL_SUPPLY_KOLNY, 1_000_000_000);
        assert_eq!(KOLNY_DECIMALS, 6);

        // And the base-unit values the program actually stores and compares.
        assert_eq!(ONE_KOLNY_BASE_UNITS, 1_000_000);
        assert_eq!(DEFAULT_ADMISSION_BURN_BASE_UNITS, 100_000_000_000);
        assert_eq!(MIN_ADMISSION_BURN_BASE_UNITS, 10_000_000_000);
        assert_eq!(MAX_ADMISSION_BURN_BASE_UNITS, 1_000_000_000_000);

        // Round trip. If `KOLNY_DECIMALS` moves without the literals above
        // moving with it, both halves of this fail rather than one silently
        // meaning a different number of tokens.
        for (base, whole) in [
            (DEFAULT_ADMISSION_BURN_BASE_UNITS, DEFAULT_ADMISSION_BURN_KOLNY),
            (MIN_ADMISSION_BURN_BASE_UNITS, MIN_ADMISSION_BURN_KOLNY),
            (MAX_ADMISSION_BURN_BASE_UNITS, MAX_ADMISSION_BURN_KOLNY),
        ] {
            assert_eq!(base / ONE_KOLNY_BASE_UNITS, whole);
            assert_eq!(base % ONE_KOLNY_BASE_UNITS, 0, "not a whole token count");
        }

        assert!(MIN_ADMISSION_BURN_BASE_UNITS > 0, "admission is never free");
        assert!(MIN_ADMISSION_BURN_BASE_UNITS < DEFAULT_ADMISSION_BURN_BASE_UNITS);
        assert!(DEFAULT_ADMISSION_BURN_BASE_UNITS < MAX_ADMISSION_BURN_BASE_UNITS);
    }

    /// The band as a share of total supply, which is the form the economics
    /// were actually decided in: 0.001 / 0.01 / 0.1 percent.
    ///
    /// Expressed in basis points of a basis point (1e-8) so the small figures
    /// stay exact in integers.
    #[test]
    fn admission_amounts_are_the_intended_share_of_total_supply() {
        const UNITS_PER_HUNDRED_MILLIONTH: u64 = 100_000_000;
        let share = |whole: u64| whole * UNITS_PER_HUNDRED_MILLIONTH / KOLNY_TOTAL_SUPPLY_KOLNY;

        // 0.001 percent = 1e-5 = 1000 hundred-millionths.
        assert_eq!(share(MIN_ADMISSION_BURN_KOLNY), 1_000);
        // 0.01 percent.
        assert_eq!(share(DEFAULT_ADMISSION_BURN_KOLNY), 10_000);
        // 0.1 percent.
        assert_eq!(share(MAX_ADMISSION_BURN_KOLNY), 100_000);

        // Supply is fixed: the mint carries no mint authority, so admission
        // burns only ever move this denominator down. At the default, the
        // whole supply would take ten thousand registrations to consume, so
        // admission is a real sink without being an entry ban.
        assert_eq!(
            KOLNY_TOTAL_SUPPLY_KOLNY / DEFAULT_ADMISSION_BURN_KOLNY,
            10_000
        );
    }

    /// A mint whose decimals are not what the constants assume is refused, and
    /// the numbers show why that matters.
    #[test]
    fn a_mint_with_unexpected_decimals_is_refused_rather_than_silently_rescaling() {
        assert!(admission_decimals_match(KOLNY_DECIMALS));
        for d in [0u8, 1, 2, 5, 7, 8, 9, 18, 255] {
            assert!(
                !admission_decimals_match(d),
                "{} decimals must be refused; the constants would mean a \
                 different number of tokens",
                d
            );
        }

        // Why, in numbers. The stored amount is base units computed at 6
        // decimals. Hand the same number to a 9-decimal mint and the operator
        // destroys 100 whole tokens instead of 100,000 -- one thousandth of the
        // approved admission. That is the exact shape of a bug this project
        // already shipped once, in the CLI, where a hard-coded 6 against
        // 9-decimal wSOL turned `deposit 1` into 0.001.
        let intended = DEFAULT_ADMISSION_BURN_BASE_UNITS / 10u64.pow(KOLNY_DECIMALS as u32);
        let if_mint_had_nine = DEFAULT_ADMISSION_BURN_BASE_UNITS / 10u64.pow(9);
        assert_eq!(intended, 100_000);
        assert_eq!(if_mint_had_nine, 100);
        assert_eq!(intended / if_mint_had_nine, 1_000);
    }

    /// Only a mint that can carry the product's claims is accepted, and every
    /// rejection reason is reachable and distinguishable.
    ///
    /// Exhaustive over the two authority flags rather than spot-checked: with
    /// only four combinations there is no excuse for testing three, and a
    /// missed combination is exactly where a guard turns out to be unreachable.
    #[test]
    fn only_a_final_mint_is_accepted_as_the_admission_asset() {
        // The one shape that is allowed: right decimals, both authorities gone.
        assert_eq!(check_admission_mint(KOLNY_DECIMALS, true, true), None);

        // A live mint authority breaks "burning permanently reduces supply".
        assert_eq!(
            check_admission_mint(KOLNY_DECIMALS, false, true),
            Some(AdmissionMintRejection::MintAuthorityLive)
        );
        // A live freeze authority breaks "anyone who burns is admitted": a
        // frozen account cannot burn, so entry becomes revocable per operator.
        assert_eq!(
            check_admission_mint(KOLNY_DECIMALS, true, false),
            Some(AdmissionMintRejection::FreezeAuthorityLive)
        );
        // Both live: still refused, and the mint authority is reported first
        // because it is the claim with the larger blast radius.
        assert_eq!(
            check_admission_mint(KOLNY_DECIMALS, false, false),
            Some(AdmissionMintRejection::MintAuthorityLive)
        );

        // Decimals are checked before either authority, so a wrong-decimals
        // mint reports the arithmetic problem even when it is otherwise final.
        for d in [0u8, 1, 5, 7, 9, 18] {
            assert_eq!(
                check_admission_mint(d, true, true),
                Some(AdmissionMintRejection::Decimals),
                "{} decimals must be refused", d
            );
        }

        // Control: the accept path is not simply unreachable. Exactly one of
        // the sixteen (decimals, mint_auth, freeze_auth) shapes swept here is
        // accepted, so the function discriminates rather than always refusing.
        let mut accepted = 0;
        for d in [0u8, KOLNY_DECIMALS, 9, 18] {
            for m in [true, false] {
                for f in [true, false] {
                    if check_admission_mint(d, m, f).is_none() {
                        accepted += 1;
                    }
                }
            }
        }
        assert_eq!(accepted, 1, "exactly one shape of mint may be accepted");
    }
}
