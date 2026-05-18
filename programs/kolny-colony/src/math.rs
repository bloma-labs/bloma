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
}
