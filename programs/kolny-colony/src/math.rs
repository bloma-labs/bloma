//! Pure stigmergy, share and loss math for the KOLNY colony.
//!
//! No Anchor imports and no floating point: every function here is integer-only
//! and deterministic, so it compiles for the host target and is verified with
//! `cargo test` without a validator.
//!
//! Conventions used throughout:
//!   - Ratios are basis points (`bps`, 1e-4).
//!   - Pheromone and deposits use a 1e6 fixed-point scale (`FP6`).
//!   - Every ratio is computed in 128-bit and multiplies BEFORE it divides, so
//!     integer precision is never thrown away by an early truncation.
//!   - Rounding is always toward zero (Rust integer division), and every
//!     rounding-sensitive path is oriented so the residual favors the vault.

pub const BPS_DENOM: u128 = 10_000;
pub const BPS_DENOM_I: i128 = 10_000;

/// Fixed-point scale for pheromone and deposits.
pub const FP6: i128 = 1_000_000;

// ---------------------------------------------------------------------------
// tanh in fixed point
// ---------------------------------------------------------------------------

/// Working scale for the tanh evaluation, FP12.
const TANH_W: i128 = 1_000_000_000_000;

/// tanh of an FP6 argument, returned in FP6.
///
/// Range reduction by halving, then an odd Taylor series through the 7th order,
/// then repeated double-angle to undo the reduction. Integer-only, so it is
/// reproducible bit for bit in any language that has 128-bit integers.
///
/// A lookup table with linear interpolation was tried first and rejected on
/// measurement. No 33-entry table reproduces the deposit column of the
/// specification's own worked example at the three decimals it prints, at any
/// domain: over `[0, 4]` the worst error is 1.4e-3, which carried through three
/// epochs moves a forager's capital by enough to matter. This series is under
/// 1e-6 across the whole domain and reproduces every deposit in that table
/// exactly. There is also no table to transcribe, so a mirror cannot drift by
/// copying one digit wrong.
///
/// Odd (`tanh(-x) == -tanh(x)`). `|x| >= 8` saturates; tanh(8) differs from 1
/// by 2.3e-7, which is below FP6 resolution. The result is clamped to
/// `999_999` so `|D| < Q` stays strictly true and one epoch can never deposit
/// the full deposit scale.
pub fn tanh_fp6(x_fp6: i64) -> i64 {
    if x_fp6 == 0 {
        return 0;
    }
    let negative = x_fp6 < 0;

    // Clamp the magnitude, then promote FP6 -> FP12.
    let ax = (x_fp6.unsigned_abs() as i128).min(8 * 1_000_000);
    let mut u = ax * 1_000_000;

    // Range reduction: halve until the series argument is small. k <= 5.
    let mut k = 0u32;
    while u >= TANH_W / 4 {
        u /= 2;
        k += 1;
    }

    // Odd series: u - u^3/3 + 2u^5/15 - 17u^7/315, evaluated by Horner in u^2.
    // The first omitted term is 62u^9/2835, under 1e-7 at |u| <= 0.25.
    let u2 = u * u / TANH_W;
    let mut acc = (-17 * TANH_W) / 315;
    acc = (2 * TANH_W) / 15 + acc * u2 / TANH_W;
    acc = -(TANH_W / 3) + acc * u2 / TANH_W;
    acc = TANH_W + acc * u2 / TANH_W;
    let mut t = u * acc / TANH_W;

    // Undo the reduction: tanh(2u) = 2 tanh(u) / (1 + tanh(u)^2).
    // Widest intermediate is about 2e36, inside i128.
    for _ in 0..k {
        t = (2 * t * TANH_W * TANH_W) / (TANH_W * TANH_W + t * t);
    }

    // FP12 -> FP6, rounding half up.
    let mut r = (t + 500_000) / 1_000_000;
    if r > 999_999 {
        r = 999_999;
    }

    if negative {
        -(r as i64)
    } else {
        r as i64
    }
}

// ---------------------------------------------------------------------------
// Realized performance measurement
// ---------------------------------------------------------------------------

/// Realized PnL of a forager for one epoch, measured on-chain from its
/// isolated sub-account.
///
/// The sub-account holds **bond + principal**, so the bond MUST be removed
/// before the balance is compared against principal. Skipping that subtraction
/// would count posted bond as trading performance and let an operator inflate
/// its own trail by simply topping up its bond.
///
/// `realized = (balance - bond) - principal`
pub fn realized_epoch_pnl(vault_balance: u64, bond: u64, principal: u64) -> i64 {
    let gross = vault_balance.saturating_sub(bond) as i128;
    let realized = gross - principal as i128;
    realized.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// Realized net return rate for the epoch, in bps. Signed: losses are negative.
/// Zero principal yields a zero rate (no capital was at work, so there is no
/// rate to report) rather than a division by zero.
pub fn return_bps(realized_pnl_epoch: i64, principal: u64) -> i64 {
    if principal == 0 {
        return 0;
    }
    let r = (realized_pnl_epoch as i128) * BPS_DENOM_I / (principal as i128);
    r.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// Risk-adjusted realized performance: `perf = r - lambda * DD`.
///
/// The subtractive drawdown penalty makes a return earned through a deep
/// intra-epoch drawdown worth less trail than the same return earned smoothly,
/// so a forager cannot climb by taking ruinous risk that happened to pay off.
pub fn risk_adjusted_perf_bps(r_bps: i64, drawdown_bps: u16, risk_aversion_bps: u16) -> i64 {
    let penalty = (risk_aversion_bps as i128) * (drawdown_bps as i128) / BPS_DENOM_I;
    let perf = (r_bps as i128) - penalty;
    perf.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

// ---------------------------------------------------------------------------
// Pheromone: evaporate, deposit, update
// ---------------------------------------------------------------------------

/// Evaporation. `retained = tau * (BPS_DENOM - rho_bps) / BPS_DENOM`.
///
/// This is the geometric discount that makes the colony forget an edge that has
/// died: with no new deposit a trail holds `(1 - rho)^n` of its pheromone after
/// `n` epochs.
pub fn evaporate(pheromone: u64, rho_bps: u16) -> u64 {
    let retain = BPS_DENOM.saturating_sub(rho_bps as u128);
    ((pheromone as u128) * retain / BPS_DENOM) as u64
}

/// The epoch deposit `D = Q * tanh(perf / s)`, in FP6. Signed.
///
/// Bounded to `(-Q, +Q)` so no single epoch -- lucky fat tail or manipulation
/// attempt -- can dominate a trail, and sign-preserving so a loss actively
/// erodes the trail instead of merely failing to reinforce it.
pub fn deposit_fp6(perf_bps: i64, perf_norm_s_bps: u16, deposit_scale_q: u64) -> i64 {
    if perf_norm_s_bps == 0 {
        return 0;
    }
    // x = perf / s, expressed in FP6.
    let x_fp6 = ((perf_bps as i128) * FP6 / (perf_norm_s_bps as i128))
        .clamp(i64::MIN as i128, i64::MAX as i128) as i64;
    let t = tanh_fp6(x_fp6) as i128;
    let d = t * (deposit_scale_q as i128) / FP6;
    d.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// One epoch of the pheromone update:
/// `tau' = clamp( max(0, (1 - rho) * tau + D), 0, ceil )`.
///
/// The floor is 0 by design so a dead trail can actually die; propping every
/// forager up at a minimum weight would fight the whole mechanism. The ceiling
/// is an overflow bound, not an economic one.
pub fn update_pheromone(current: u64, deposit_fp6: i64, rho_bps: u16, ceil: u64) -> u64 {
    let retained = evaporate(current, rho_bps) as i128;
    let next = retained + deposit_fp6 as i128;
    if next <= 0 {
        return 0;
    }
    (next as u128).min(ceil as u128) as u64
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- tanh ---------------------------------------------------------------

    #[test]
    fn tanh_is_odd_and_bounded() {
        assert_eq!(tanh_fp6(0), 0);
        assert_eq!(tanh_fp6(-500_000), -tanh_fp6(500_000));
        assert_eq!(tanh_fp6(-3_000_000), -tanh_fp6(3_000_000));
        // Bounded strictly inside (-1, 1) in FP6.
        assert!(tanh_fp6(50_000_000) < 1_000_000);
        assert!(tanh_fp6(-50_000_000) > -1_000_000);
    }

    #[test]
    fn tanh_matches_reference_points() {
        // Correctly rounded to FP6 at every point.
        assert_eq!(tanh_fp6(200_000), 197_375); // tanh(0.2)
        assert_eq!(tanh_fp6(300_000), 291_313); // tanh(0.3)
        assert_eq!(tanh_fp6(500_000), 462_117); // tanh(0.5)
        assert_eq!(tanh_fp6(700_000), 604_368); // tanh(0.7)
        assert_eq!(tanh_fp6(1_000_000), 761_594); // tanh(1.0)
        assert_eq!(tanh_fp6(1_200_000), 833_655); // tanh(1.2)
        assert_eq!(tanh_fp6(1_500_000), 905_148); // tanh(1.5)
        assert_eq!(tanh_fp6(2_000_000), 964_028); // tanh(2.0)
        assert_eq!(tanh_fp6(3_000_000), 995_055); // tanh(3.0)
    }

    #[test]
    fn tanh_reproduces_the_specification_deposit_table() {
        // Every deposit in the worked example of docs/allocation-spec.md 10.1,
        // at the three decimals that document prints. This is the check that
        // rejected the lookup-table implementation: a 33-entry table missed
        // four of these, and the error compounds across epochs into capital
        // going to the wrong forager.
        let q = 1_000_000u64;
        let s = 1_000u16;
        // (perf_bps, printed deposit in FP6 at 3dp)
        for (perf_bps, expected_3dp) in [
            (1_500i64, 905_000i64),
            (1_200, 834_000),
            (1_000, 762_000),
            (500, 462_000),
            (200, 197_000),
            (300, 291_000),
            (-500, -462_000),
            (-1_000, -762_000),
            (-800, -664_000),
            (700, 604_000),
        ] {
            let d = deposit_fp6(perf_bps, s, q);
            // Round the computed deposit to 3 decimals and compare.
            let rounded = if d >= 0 {
                (d + 500) / 1_000 * 1_000
            } else {
                -((-d + 500) / 1_000 * 1_000)
            };
            assert_eq!(
                rounded, expected_3dp,
                "perf {} bps produced {} which rounds to {}, specification prints {}",
                perf_bps, d, rounded, expected_3dp
            );
        }
    }

    #[test]
    fn tanh_is_monotone() {
        let mut prev = tanh_fp6(-5_000_000);
        let mut x = -5_000_000i64;
        while x <= 5_000_000 {
            let y = tanh_fp6(x);
            assert!(y >= prev, "tanh not monotone at {}", x);
            prev = y;
            x += 37_000;
        }
    }

    // -- realized PnL: the bond MUST be excluded ----------------------------

    #[test]
    fn realized_pnl_excludes_bond() {
        // Sub-account holds bond 500 + principal 1000, and the forager made 200.
        // Balance 1700 => realized must be +200, not +700.
        assert_eq!(realized_epoch_pnl(1_700, 500, 1_000), 200);

        // Flat epoch: balance is exactly bond + principal => zero, not +bond.
        assert_eq!(realized_epoch_pnl(1_500, 500, 1_000), 0);

        // A pure bond top-up with no trading must NOT read as performance.
        // Bond rose 500 -> 900, balance rose with it; realized stays 0.
        assert_eq!(realized_epoch_pnl(1_900, 900, 1_000), 0);

        // Loss: balance 1200 = bond 500 + 700 left of 1000 principal => -300.
        assert_eq!(realized_epoch_pnl(1_200, 500, 1_000), -300);

        // Total wipeout of principal, bond intact.
        assert_eq!(realized_epoch_pnl(500, 500, 1_000), -1_000);
    }

    #[test]
    fn realized_pnl_forgetting_bond_would_be_exploitable() {
        // Demonstrates the exact manipulation the subtraction prevents: with a
        // flat book, a larger bond must not produce a larger realized number.
        let small_bond = realized_epoch_pnl(1_100, 100, 1_000);
        let large_bond = realized_epoch_pnl(9_000, 8_000, 1_000);
        assert_eq!(small_bond, 0);
        assert_eq!(large_bond, 0);
        assert_eq!(small_bond, large_bond);
    }

    #[test]
    fn return_bps_is_signed_and_safe() {
        assert_eq!(return_bps(100, 1_000), 1_000); // +10%
        assert_eq!(return_bps(-250, 1_000), -2_500); // -25%
        assert_eq!(return_bps(500, 0), 0); // no principal, no rate
    }

    #[test]
    fn risk_adjustment_penalizes_drawdown() {
        // r = +20%, DD = 10%, lambda = 1.0 => perf = 2000 - 1000 = 1000 bps
        assert_eq!(risk_adjusted_perf_bps(2_000, 1_000, 10_000), 1_000);
        // lambda = 0 disables the penalty
        assert_eq!(risk_adjusted_perf_bps(2_000, 1_000, 0), 2_000);
        // A deep drawdown can drive a positive return negative.
        assert!(risk_adjusted_perf_bps(500, 3_000, 10_000) < 0);
    }

    // -- evaporation / decay ------------------------------------------------

    #[test]
    fn evaporate_applies_geometric_decay() {
        assert_eq!(evaporate(1_000_000, 5_000), 500_000);
        assert_eq!(evaporate(0, 5_000), 0);
        assert_eq!(evaporate(1_000_000, 10_000), 0);
        // Default rho = 1600 retains 84%.
        assert_eq!(evaporate(1_000_000, 1_600), 840_000);
    }

    #[test]
    fn decay_half_life_matches_spec() {
        // rho = 1600 bps is documented as a half-life of about 4 epochs.
        let mut tau = 1_000_000u64;
        for _ in 0..4 {
            tau = evaporate(tau, 1_600);
        }
        // (0.84)^4 = 0.49787
        assert_eq!(tau, 497_871);
        assert!(tau < 500_000 && tau > 490_000);
    }

    #[test]
    fn unreplenished_trail_decays_toward_zero() {
        let mut tau = 1_000_000u64;
        for _ in 0..13 {
            tau = evaporate(tau, 1_600);
        }
        // ~10% left after 13 epochs, the documented "forgotten" horizon.
        assert!(tau < 110_000 && tau > 90_000, "tau was {}", tau);
    }

    // -- deposit transform --------------------------------------------------

    #[test]
    fn deposit_is_bounded_by_q() {
        let q = 1_000_000u64;
        // Enormous performance still cannot exceed Q.
        let huge = deposit_fp6(1_000_000, 1_000, q);
        assert!(huge < q as i64, "deposit {} must stay under Q", huge);
        assert!(huge > 990_000);
        // And symmetrically on the downside.
        let huge_neg = deposit_fp6(-1_000_000, 1_000, q);
        assert!(huge_neg > -(q as i64));
        assert_eq!(huge_neg, -huge);
    }

    #[test]
    fn deposit_is_sign_preserving() {
        let q = 1_000_000u64;
        assert!(deposit_fp6(500, 1_000, q) > 0);
        assert_eq!(deposit_fp6(0, 1_000, q), 0);
        assert!(deposit_fp6(-500, 1_000, q) < 0);
    }

    #[test]
    fn deposit_saturation_matches_spec_points() {
        let q = 1_000_000u64;
        // At perf = s the deposit is Q*tanh(1) ~ 0.76 Q.
        assert_eq!(deposit_fp6(1_000, 1_000, q), 761_594);
        // At perf = 2s it is ~0.96 Q.
        assert_eq!(deposit_fp6(2_000, 1_000, q), 964_028);
    }

    // -- pheromone update ---------------------------------------------------

    #[test]
    fn update_pheromone_evaporates_then_deposits() {
        let ceil = 1_000_000_000_000u64;
        // rho 2000 => retain 80%. tau 1.0 + deposit 0.905 => 1.705 (spec 10.1, forager A)
        let tau = update_pheromone(1_000_000, 905_000, 2_000, ceil);
        assert_eq!(tau, 1_705_000);
    }

    #[test]
    fn losing_epoch_erodes_trail_faster_than_decay() {
        let ceil = 1_000_000_000_000u64;
        let decay_only = update_pheromone(1_000_000, 0, 2_000, ceil);
        let with_loss = update_pheromone(1_000_000, -462_000, 2_000, ceil);
        assert_eq!(decay_only, 800_000);
        assert_eq!(with_loss, 338_000); // spec 10.1, forager D epoch 1
        assert!(with_loss < decay_only);
    }

    #[test]
    fn pheromone_floors_at_zero_and_never_goes_negative() {
        let ceil = 1_000_000_000_000u64;
        // spec 10.1 forager D epoch 2: 0.8*0.338 - 0.762 = -0.492 -> 0
        let tau = update_pheromone(338_000, -762_000, 2_000, ceil);
        assert_eq!(tau, 0);
        // A dead trail stays dead under further losses.
        assert_eq!(update_pheromone(0, -1_000_000, 2_000, ceil), 0);
    }

    #[test]
    fn pheromone_clamps_to_ceiling() {
        let ceil = 1_000_000u64;
        let tau = update_pheromone(1_000_000, 900_000, 0, ceil);
        assert_eq!(tau, ceil, "must clamp at the ceiling");
        // Repeated maximal deposits cannot climb past the ceiling.
        let mut t = 0u64;
        for _ in 0..50 {
            t = update_pheromone(t, 900_000, 1_600, ceil);
        }
        assert!(t <= ceil);
    }

    #[test]
    fn spec_worked_example_trajectory_reproduces() {
        // End to end against docs/allocation-spec.md 10.1, driven from the
        // published risk-adjusted performance figures rather than from the
        // deposit column, so the deposit transform is under test too.
        //
        // Example parameters, NOT the production defaults: rho = 0.20,
        // Q = 1.0, s = 0.10, and the colony seeded at tau = 1.000.
        let ceil = 1_000_000_000_000u64;
        let rho = 2_000u16;
        let q = 1_000_000u64;
        let s = 1_000u16;

        // (name, perf per epoch in bps, exact tau at the start of e2/e3/e4)
        //
        // The expected values are exact FP6, independently confirmed at 60-digit
        // precision, not the three-decimal figures the document prints. Fourteen
        // of the document's fifteen cells agree with these exactly. The one that
        // does not is forager E at epoch 3: the document's own worked steps
        // round the intermediate `0.8 * 0.603` to `0.483` instead of `0.4824`,
        // and reach `0.483 + 0.604 = 1.087`. Carried without that intermediate
        // rounding the value is 1.086468, which prints as 1.086. That is an
        // artifact of the document's presentation, not a disagreement about the
        // model, and it is exactly the kind of thing that gets mistaken for an
        // implementation bug -- hence this note.
        let foragers: [(&str, [i64; 3], [u64; 3]); 5] = [
            ("A", [1_500, 1_200, 1_000], [1_705_148, 2_197_773, 2_519_812]),
            ("B", [1_000, 500, 0], [1_561_594, 1_711_392, 1_369_113]),
            ("C", [200, 300, 200], [997_375, 1_089_213, 1_068_745]),
            ("D", [-500, -1_000, -800], [337_883, 0, 0]),
            ("E", [-200, 700, 1_500], [602_625, 1_086_468, 1_774_322]),
        ];

        let mut finals = [0u64; 5];
        for (i, (name, perfs, expected)) in foragers.iter().enumerate() {
            let mut tau = 1_000_000u64;
            for (epoch, perf) in perfs.iter().enumerate() {
                let d = deposit_fp6(*perf, s, q);
                tau = update_pheromone(tau, d, rho, ceil);
                assert_eq!(
                    tau,
                    expected[epoch],
                    "forager {} entering epoch {}",
                    name,
                    epoch + 2
                );
            }
            finals[i] = tau;
        }

        // The narrative the example is built to show:
        // D, the loser, is extinguished outright.
        assert_eq!(finals[3], 0);
        // E, the late bloomer, overtakes B, the early winner who went flat.
        // Standing still is punished, which is the point of the discount.
        assert!(finals[4] > finals[1]);
        // A, the steady star, ends on the strongest trail.
        assert!(finals[0] > finals[4]);
    }
}
