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

// ---------------------------------------------------------------------------
// Pheromone -> weights (bounded water-filling)
// ---------------------------------------------------------------------------

/// The most foragers that can simultaneously sit at the concentration cap.
/// `m * w_max <= 1`, so at most `floor(BPS_DENOM / w_max_bps)` are capped. With
/// the minimum allowed `w_max_bps` of 500 that is 20, so tracking the top 21
/// trails is always enough to solve the cap exactly.
pub const MAX_CAPPED_TRAILS: usize = 20;
pub const TOP_TRAILS_LEN: usize = MAX_CAPPED_TRAILS + 1;

/// Effective concentration cap.
///
/// The cap is only feasible if `n_active * w_max >= 1`; below that the weights
/// cannot sum to 1 while every weight stays at or under the cap. In that case
/// the effective cap relaxes to `max(w_max, 1/n_active + margin)` so a small
/// colony can still deploy, and any capital that STILL cannot be placed is
/// reported as un-deployed reserve rather than silently over-concentrated.
pub fn effective_max_weight_bps(w_max_bps: u16, active_count: u32, relax_margin_bps: u16) -> u16 {
    if active_count == 0 {
        return w_max_bps;
    }
    let even = (BPS_DENOM / active_count as u128) as u128;
    let relaxed = (even + relax_margin_bps as u128).min(BPS_DENOM) as u16;
    w_max_bps.max(relaxed)
}

/// The solved water-filling level.
///
/// `remaining_bps` is the share of the pool left for the uncapped foragers
/// (`BPS_DENOM - capped_count * w_max`), and `rest_sum` is their total
/// pheromone. Together they define every uncapped weight as
/// `remaining_bps * tau / rest_sum`, with no quotient formed anywhere, so no
/// division rounding enters either the cap decision or the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapLevel {
    pub capped_count: u16,
    pub remaining_bps: u16,
    pub rest_sum: u128,
}

/// Solve the bounded water-filling level.
///
/// A forager is capped exactly when its share of the pool still available
/// reaches the cap, which is the division-free integer test
///
/// ```text
/// remaining_bps * tau  >=  w_max * rest_sum
/// ```
///
/// Walk the pheromone ranking once, admitting trails to the capped set while
/// that holds. The result is the same vector the specification's "trim, then
/// redistribute the excess, then repeat" loop converges to, but with no
/// termination condition for two implementations to disagree about and no
/// quotient to round.
///
/// `top` must be the largest pheromone values in descending order.
pub fn solve_cap_level(
    total_pheromone: u128,
    top: &[u64],
    active_count: u32,
    eff_w_max_bps: u16,
) -> CapLevel {
    let w = eff_w_max_bps as u128;
    if total_pheromone == 0 || active_count == 0 || eff_w_max_bps == 0 {
        return CapLevel {
            capped_count: 0,
            remaining_bps: BPS_DENOM as u16,
            rest_sum: total_pheromone,
        };
    }

    // Keep the remaining share strictly positive.
    let max_m = (((BPS_DENOM - 1) / w) as usize)
        .min(top.len())
        .min(active_count as usize);

    let mut m: usize = 0;
    let mut prefix: u128 = 0;
    while m < max_m {
        let remaining = BPS_DENOM - (m as u128) * w;
        let rest = total_pheromone.saturating_sub(prefix);
        if rest == 0 {
            break;
        }
        if remaining.saturating_mul(top[m] as u128) >= w.saturating_mul(rest) {
            prefix = prefix.saturating_add(top[m] as u128);
            m += 1;
        } else {
            break;
        }
    }

    CapLevel {
        capped_count: m as u16,
        remaining_bps: (BPS_DENOM - (m as u128) * w) as u16,
        rest_sum: total_pheromone.saturating_sub(prefix),
    }
}

/// Whether a trail sits at the concentration cap under the solved level.
pub fn is_capped(pheromone: u64, level: &CapLevel, eff_w_max_bps: u16) -> bool {
    if pheromone == 0 {
        return false;
    }
    (level.remaining_bps as u128).saturating_mul(pheromone as u128)
        >= (eff_w_max_bps as u128).saturating_mul(level.rest_sum)
}

/// Legacy divisor form, kept only so the shape of the solution stays visible:
/// `K = rest_sum * BPS_DENOM / remaining_bps`. Nothing in the program forms
/// this quotient, because rounding it is what biases every uncapped target at
/// once.
pub fn alloc_divisor_of(level: &CapLevel) -> u128 {
    if level.remaining_bps == 0 {
        return 0;
    }
    level.rest_sum * BPS_DENOM / (level.remaining_bps as u128)
}

/// A forager's weight in bps under the solved level.
pub fn weight_bps_from_level(pheromone: u64, level: &CapLevel, eff_w_max_bps: u16) -> u16 {
    if pheromone == 0 {
        return 0;
    }
    if is_capped(pheromone, level, eff_w_max_bps) {
        return eff_w_max_bps;
    }
    if level.rest_sum == 0 {
        return 0;
    }
    ((level.remaining_bps as u128) * (pheromone as u128) / level.rest_sum) as u16
}

/// Capital target for a forager.
///
/// Uncapped: `pool * remaining_bps * tau / (BPS * rest_sum)`, widened to 128
/// bits and multiplied before dividing, so the weight is never materialized
/// and never rounded twice. Capped: exactly `pool * w_max / BPS`.
///
/// Truncation means the per-forager targets sum to slightly UNDER the pool.
/// That residue is dust and belongs in the vault. Do not redistribute it here:
/// this function is evaluated one forager at a time by the settlement crank
/// and has no visibility into any other forager's remainder.
pub fn allocation_target(
    pheromone: u64,
    level: &CapLevel,
    pool: u64,
    eff_w_max_bps: u16,
) -> u64 {
    if pheromone == 0 || pool == 0 {
        return 0;
    }
    if is_capped(pheromone, level, eff_w_max_bps) {
        return ((pool as u128) * (eff_w_max_bps as u128) / BPS_DENOM) as u64;
    }
    if level.rest_sum == 0 {
        return 0;
    }
    let num = (pool as u128)
        .saturating_mul(pheromone as u128)
        .saturating_mul(level.remaining_bps as u128);
    (num / (BPS_DENOM * level.rest_sum)) as u64
}

/// Insert a value into a descending-ordered fixed-size top list.
/// Returns the number of occupied slots after insertion.
pub fn top_insert(top: &mut [u64; TOP_TRAILS_LEN], count: u8, value: u64) -> u8 {
    let len = TOP_TRAILS_LEN;
    let occupied = (count as usize).min(len);

    if occupied == len && value <= top[len - 1] {
        return count;
    }
    let mut pos = occupied;
    for i in 0..occupied {
        if value > top[i] {
            pos = i;
            break;
        }
    }
    if pos >= len {
        return count;
    }
    let mut i = len - 1;
    while i > pos {
        top[i] = top[i - 1];
        i -= 1;
    }
    top[pos] = value;
    ((occupied + 1).min(len)) as u8
}

/// Whether a rebalance should be skipped because the target barely moved.
/// The no-trade band stops small pheromone wiggles from generating trades.
pub fn within_no_trade_band(current: u64, target: u64, pool: u64, band_bps: u16) -> bool {
    if band_bps == 0 || pool == 0 {
        return false;
    }
    let delta = current.abs_diff(target) as u128;
    let band = (pool as u128) * (band_bps as u128) / BPS_DENOM;
    delta < band
}

/// Clamp a rebalance move to the remaining per-epoch turnover budget.
pub fn apply_turnover_cap(desired: u64, moved_so_far: u64, pool: u64, turnover_cap_bps: u16) -> u64 {
    let budget = (pool as u128) * (turnover_cap_bps as u128) / BPS_DENOM;
    let used = moved_so_far as u128;
    if used >= budget {
        return 0;
    }
    (desired as u128).min(budget - used) as u64
}

// ---------------------------------------------------------------------------
// Shares (ERC4626 style with a virtual offset)
// ---------------------------------------------------------------------------

/// Virtual offset that makes the first-depositor rounding attack unprofitable.
pub const VIRTUAL_SHARES: u128 = 1_000_000;
pub const VIRTUAL_ASSETS: u128 = 1;

/// Shares minted for a deposit. Rounds DOWN, favoring the vault.
///
/// `nav` is an accounting counter (`idle_base + outstanding_principal`), never
/// a live token balance. That is what makes a direct token transfer into a
/// vault account unable to move the share price: an unsolicited transfer is
/// simply uncredited and cannot inflate anyone's redemption value.
pub fn shares_for_deposit(assets: u64, total_shares: u128, nav: u64) -> u128 {
    (assets as u128) * (total_shares + VIRTUAL_SHARES) / (nav as u128 + VIRTUAL_ASSETS)
}

/// Assets returned for burning shares. Rounds DOWN, favoring the vault.
pub fn assets_for_shares(shares: u128, total_shares: u128, nav: u64) -> u64 {
    let a = shares * (nav as u128 + VIRTUAL_ASSETS) / (total_shares + VIRTUAL_SHARES);
    a.min(u64::MAX as u128) as u64
}

// ---------------------------------------------------------------------------
// Loss coverage and slashing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LossOutcome {
    pub from_bond: u64,
    pub from_cache: u64,
    pub to_vault_loss: u64,
}

/// Absorb a realized loss in waterfall order.
///
/// The forager's own sub-account has already taken the hit by the time this is
/// called (its balance is what produced the loss), so the remaining order is
/// bond, then Risk Cache, then depositor NAV. Step three is the honest limit:
/// the cushion is finite, and whatever the bond and cache cannot cover reduces
/// depositor share value.
pub fn cover_loss(loss: u64, bond: u64, cache_balance: u64) -> LossOutcome {
    let from_bond = loss.min(bond);
    let rem = loss - from_bond;
    let from_cache = rem.min(cache_balance);
    let to_vault_loss = rem - from_cache;
    LossOutcome {
        from_bond,
        from_cache,
        to_vault_loss,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashOutcome {
    pub slashed: u64,
    pub burn: u64,
    pub to_cache: u64,
}

/// Punitive slash: seize `slash_bps` of the bond, then split it between the
/// incinerator and the Risk Cache.
pub fn slash_split(bond: u64, slash_bps: u16, burn_share_bps: u16) -> SlashOutcome {
    let slashed = ((bond as u128) * (slash_bps as u128) / BPS_DENOM) as u64;
    let burn = ((slashed as u128) * (burn_share_bps as u128) / BPS_DENOM) as u64;
    let to_cache = slashed - burn;
    SlashOutcome {
        slashed,
        burn,
        to_cache,
    }
}

/// Fraction of bond seized by a loss-threshold slash, scaled by how far past
/// the slash threshold the drawdown went, up to the whole bond. Integrity
/// failures (rule violation, non-response) seize the whole bond and do not use
/// this ramp.
pub fn loss_slash_bps(drawdown_bps: u16, dd_slash_bps: u16) -> u16 {
    if drawdown_bps <= dd_slash_bps || dd_slash_bps == 0 {
        return 0;
    }
    let excess = (drawdown_bps - dd_slash_bps) as u128;
    let ramp = excess * BPS_DENOM / (dd_slash_bps as u128);
    ramp.min(BPS_DENOM) as u16
}

/// Share of realized profit routed to the Risk Cache while it is below target.
pub fn cache_accrual(profit: u64, cache_accrual_bps: u16) -> u64 {
    ((profit as u128) * (cache_accrual_bps as u128) / BPS_DENOM) as u64
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

    // -- normalization and water-filling ------------------------------------

    #[test]
    fn normalization_is_proportional_when_no_cap_binds() {
        // Three equal trails, cap 5000 bps never binds.
        let top = [1_000u64, 1_000, 1_000];
        let k = solve_cap_level(3_000, &top, 3, 5_000);
        assert_eq!(k.capped_count, 0, "no trail reaches the cap");
        assert_eq!(k.remaining_bps, 10_000);
        assert_eq!(k.rest_sum, 3_000);
        assert_eq!(weight_bps_from_level(1_000, &k, 5_000), 3_333);
        // Equal trails receive equal capital.
        assert_eq!(allocation_target(1_000, &k, 900_000, 5_000), 300_000);
    }

    #[test]
    fn normalization_is_proportional_to_pheromone() {
        // 3:1 pheromone ratio maps to a 3:1 capital ratio.
        let top = [3_000u64, 1_000];
        let k = solve_cap_level(4_000, &top, 2, 9_000);
        let a = allocation_target(3_000, &k, 1_000_000, 9_000);
        let b = allocation_target(1_000, &k, 1_000_000, 9_000);
        assert_eq!(a, 750_000);
        assert_eq!(b, 250_000);
    }

    #[test]
    fn cap_clamps_a_dominant_trail() {
        // One forager holds the entire trail; cap 2500 bps holds it to 25%.
        let top = [1_000_000u64];
        let k = solve_cap_level(1_000_000, &top, 1, 2_500);
        let t = allocation_target(1_000_000, &k, 1_000_000_000, 2_500);
        assert_eq!(t, 250_000_000);
        assert_eq!(weight_bps_from_level(1_000_000, &k, 2_500), 2_500);
    }

    #[test]
    fn water_filling_redistributes_excess_spec_example() {
        // allocation-spec 10.2, epoch 3. D has been dropped, so the Active set
        // is A/B/C/E with w_max = 3500 bps and main_pool = 900_000.
        // A's raw weight is 0.3612 -> capped at 0.35, and the 0.011 excess must
        // be redistributed across B, C and E.
        // Pheromone entering epoch 3, carried through the trajectory with the
        // real deposit values rather than the 3-decimal figures the document
        // prints for display. Rounding the inputs first is what produced an
        // earlier wrong expectation here.
        let a = 2_197_773u64;
        let b = 1_711_392u64;
        let c = 1_089_213u64;
        let e = 1_086_468u64;
        let s = (a + b + c + e) as u128;
        let top = [a, b, c, e];

        let k = solve_cap_level(s, &top, 4, 3_500);
        assert_eq!(k.capped_count, 1, "only A should be capped");
        assert_eq!(k.remaining_bps, 6_500);
        assert_eq!(k.rest_sum, (b + c + e) as u128);

        let pool = 900_000u64;
        let ta = allocation_target(a, &k, pool, 3_500);
        let tb = allocation_target(b, &k, pool, 3_500);
        let tc = allocation_target(c, &k, pool, 3_500);
        let te = allocation_target(e, &k, pool, 3_500);

        // Ground truth, computed independently at 60-digit precision:
        //   A 315000.0000  B 257562.6017  C 163925.2850  E 163512.1133
        // summing to exactly 900000. These are the truncations of that.
        assert_eq!(ta, 315_000);
        assert_eq!(tb, 257_562);
        assert_eq!(tc, 163_925);
        assert_eq!(te, 163_512);

        // A check that does not depend on the tanh implementation at all:
        //   tau_C - tau_E = 0.8 * 2 * tanh(0.2) + tanh(0.3) - tanh(0.7)
        // which fixes the C-to-E capital gap at 413 regardless of how tanh is
        // evaluated. An earlier lookup-table implementation gave 301 here.
        assert_eq!(tc - te, 413);

        // Forager D's trail was extinguished, so it draws nothing.
        assert_eq!(allocation_target(0, &k, pool, 3_500), 0);

        // The placed total falls one atom short of the pool. That atom is dust
        // from truncation and it stays in the vault. Do not redistribute it to
        // make the total land on the pool exactly: this function is evaluated
        // one forager at a time by the crank and cannot see any other
        // forager's remainder, so a largest-remainder rule is not computable
        // here at all. An off-chain mirror that applies one will disagree with
        // the chain about what each forager actually receives.
        let total = ta + tb + tc + te;
        assert_eq!(total, 899_999);
        assert_eq!(pool - total, 1, "dust stays in the vault");

        // A is exactly at the cap.
        assert_eq!(ta, pool * 3_500 / 10_000);

        // The excess really was redistributed: the uncapped three receive more
        // than their raw normalized share of the pool would have given them.
        let raw_b = (pool as u128 * b as u128 / s) as u64;
        assert!(tb > raw_b, "capped excess must flow to the uncapped");

        // Everything is placed, to within integer dust.
        let total = ta + tb + tc + te;
        assert!(pool - total <= 4, "unplaced dust was {}", pool - total);
    }

    #[test]
    fn water_filling_handles_two_capped_trails() {
        // Two dominant trails both exceed a 3000 bps cap.
        let top = [10_000u64, 9_000, 1_000, 500];
        let s = 20_500u128;
        let k = solve_cap_level(s, &top, 4, 3_000);

        let pool = 1_000_000u64;
        let t0 = allocation_target(top[0], &k, pool, 3_000);
        let t1 = allocation_target(top[1], &k, pool, 3_000);
        let t2 = allocation_target(top[2], &k, pool, 3_000);
        let t3 = allocation_target(top[3], &k, pool, 3_000);

        assert_eq!(t0, 300_000);
        assert_eq!(t1, 300_000);
        // The remaining 40% is split 1000:500 between the survivors.
        assert_eq!(t2, 266_666);
        assert_eq!(t3, 133_333);
        assert!(pool - (t0 + t1 + t2 + t3) <= 4);
    }

    #[test]
    fn water_filling_converges_to_full_allocation() {
        // Randomish distribution: the solved weights must sum to ~100%.
        let top = [900u64, 800, 700, 600, 500, 400, 300, 200, 100, 50];
        let s: u128 = top.iter().map(|v| *v as u128).sum();
        let k = solve_cap_level(s, &top, top.len() as u32, 2_000);
        let pool = 10_000_000u64;
        let total: u64 = top
            .iter()
            .map(|t| allocation_target(*t, &k, pool, 2_000))
            .sum();
        assert!(
            pool - total <= top.len() as u64,
            "allocated {} of {}",
            total,
            pool
        );
    }

    #[test]
    fn level_form_differs_from_rounding_a_shared_divisor() {
        // An earlier version formed the water-filling scalar K as a quotient
        // and rounded it. Rounding it DOWN over-allocated the pool; rounding it
        // UP fixed that but still biases every uncapped target at once, because
        // one rounded divisor is shared by all of them.
        //
        // The level form divides once per forager instead, so it agrees with
        // the exact rational answer where a rounded divisor does not. This test
        // pins a case where the two disagree, so that "just round K up" cannot
        // quietly come back, and so a mirror implementing a rounded divisor is
        // known to diverge rather than assumed to match.
        let taus = [2_794_609u64, 546_645, 4_837];
        let w_max = 3_500u16;
        let pool = 900_000u64;
        let s: u128 = taus.iter().map(|t| *t as u128).sum();
        let level = solve_cap_level(s, &taus, 3, w_max);

        // Exact: the two big trails cap, and the last one takes the whole
        // remaining 30% because it is the only uncapped trail left.
        let exact_last = allocation_target(taus[2], &level, pool, w_max);
        assert_eq!(exact_last, 270_000);

        // The same value via a ceiling-rounded shared divisor loses 12 atoms.
        let k_ceil = (level.rest_sum * BPS_DENOM).div_ceil(level.remaining_bps as u128);
        let via_divisor = ((pool as u128) * (taus[2] as u128) / k_ceil) as u64;
        assert_eq!(via_divisor, 269_988);
        assert_ne!(exact_last, via_divisor);
    }

    #[test]
    fn allocation_never_exceeds_the_pool() {
        // The invariant that matters on-chain: the targets handed to
        // rebalance_forager must never sum to more than the allocatable pool.
        //
        // This is a regression test for a real defect. Truncating the divisor
        // downward inflated every uncapped target, and in the near-infeasible
        // regime (the capped set almost filling the pool, one tiny trail left
        // over) the sum overshot the pool. The case below overshot by 49 units.
        let pool = 900_000u64;
        for (taus, w_max) in [
            (vec![547u64, 2_794_609, 546_645], 3_500u16),
            (vec![410_597, 5_324, 1_683_032], 3_500),
            (vec![1_958_076, 4_315_586, 6_047], 3_500),
            (vec![1, 1, 5_000_000], 3_500),
            (vec![1, 5_000_000], 5_000),
        ] {
            let s: u128 = taus.iter().map(|t| *t as u128).sum();
            let mut sorted = taus.clone();
            sorted.sort_unstable_by(|a, b| b.cmp(a));
            let k = solve_cap_level(s, &sorted, taus.len() as u32, w_max);
            let total: u64 = taus
                .iter()
                .map(|t| allocation_target(*t, &k, pool, w_max))
                .sum();
            assert!(
                total <= pool,
                "over-allocated {} of a {} pool for taus {:?}",
                total,
                pool,
                taus
            );
        }
    }

    #[test]
    fn allocation_never_exceeds_the_pool_across_many_shapes() {
        // Deterministic sweep over skewed distributions, including the
        // near-infeasible shapes where the rounding direction actually bites.
        let pool = 1_000_000u64;
        let mut seed = 12_345u64;
        let mut next = move || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (seed >> 33) as u64
        };
        for _ in 0..4_000 {
            let n = (next() % 11 + 2) as usize;
            let w_max = [500u16, 1_000, 2_000, 2_500, 3_500, 4_000][(next() % 6) as usize];
            let mut taus: Vec<u64> = (0..n).map(|_| next() % 5_000_000 + 1).collect();
            taus.sort_unstable_by(|a, b| b.cmp(a));
            let s: u128 = taus.iter().map(|t| *t as u128).sum();
            let k = solve_cap_level(s, &taus, n as u32, w_max);
            let total: u64 = taus
                .iter()
                .map(|t| allocation_target(*t, &k, pool, w_max))
                .sum();
            assert!(
                total <= pool,
                "over-allocated {} of {} for {:?} at w_max {}",
                total,
                pool,
                taus,
                w_max
            );
        }
    }

    #[test]
    fn dropped_trail_receives_nothing() {
        let top = [1_000u64, 0];
        let k = solve_cap_level(1_000, &top, 2, 9_000);
        assert_eq!(allocation_target(0, &k, 1_000_000, 9_000), 0);
    }

    #[test]
    fn empty_colony_allocates_nothing() {
        // No trails at all: nothing is allocatable, and a forager whose trail
        // has evaporated receives nothing regardless of the divisor.
        assert_eq!(solve_cap_level(0, &[], 0, 2_000).rest_sum, 0);
        assert_eq!(solve_cap_level(0, &[], 0, 2_000).capped_count, 0);
        let empty = solve_cap_level(0, &[], 0, 2_000);
        assert_eq!(allocation_target(0, &empty, 1_000_000, 2_000), 0);
        assert_eq!(allocation_target(0, &empty, 1_000_000, 2_000), 0);
        assert_eq!(weight_bps_from_level(0, &empty, 2_000), 0);
    }

    #[test]
    fn all_capped_leaves_undeployed_reserve() {
        // A single forager holds the whole trail but the cap is 25%, so the
        // cap binds, it receives exactly 25%, and the other 75% must stay in
        // the vault as reported un-deployed reserve rather than being forced
        // out to an over-concentrated position.
        let top = [1_000_000u64];
        let k = solve_cap_level(1_000_000, &top, 1, 2_500);
        assert_eq!(k.capped_count, 1);
        assert_eq!(k.rest_sum, 0, "the capped trail holds the entire sum");

        let pool = 1_000_000u64;
        let placed = allocation_target(1_000_000, &k, pool, 2_500);
        assert_eq!(placed, 250_000);
        assert_eq!(pool - placed, 750_000, "un-deployed reserve");
        assert_eq!(weight_bps_from_level(1_000_000, &k, 2_500), 2_500);
    }

    #[test]
    fn effective_cap_relaxes_only_when_infeasible() {
        // 10 foragers at 20% each is feasible: no relaxation.
        assert_eq!(effective_max_weight_bps(2_000, 10, 100), 2_000);
        // 3 foragers cannot fill the pool at 20% each: relax to 1/3 + margin.
        assert_eq!(effective_max_weight_bps(2_000, 3, 100), 3_433);
        // A single forager can take the whole pool once relaxed.
        assert_eq!(effective_max_weight_bps(2_000, 1, 100), 10_000);
        assert_eq!(effective_max_weight_bps(2_000, 0, 100), 2_000);
    }

    #[test]
    fn top_insert_keeps_descending_order() {
        let mut top = [0u64; TOP_TRAILS_LEN];
        let mut n = 0u8;
        for v in [5u64, 100, 3, 70, 1, 90] {
            n = top_insert(&mut top, n, v);
        }
        assert_eq!(n, 6);
        assert_eq!(&top[..6], &[100, 90, 70, 5, 3, 1]);
        for w in top.windows(2) {
            assert!(w[0] >= w[1]);
        }
    }

    #[test]
    fn top_insert_saturates_at_capacity() {
        let mut top = [0u64; TOP_TRAILS_LEN];
        let mut n = 0u8;
        for i in 0..100u64 {
            n = top_insert(&mut top, n, i);
        }
        assert_eq!(n as usize, TOP_TRAILS_LEN);
        assert_eq!(top[0], 99);
        // Only the largest survive.
        assert_eq!(top[TOP_TRAILS_LEN - 1], 99 - (TOP_TRAILS_LEN as u64 - 1));
        // A small value is rejected once full.
        let before = top;
        n = top_insert(&mut top, n, 1);
        assert_eq!(top, before);
        assert_eq!(n as usize, TOP_TRAILS_LEN);
    }

    #[test]
    fn no_trade_band_suppresses_small_moves() {
        // 2% band on a 1_000_000 pool = 20_000.
        assert!(within_no_trade_band(100_000, 110_000, 1_000_000, 200));
        assert!(!within_no_trade_band(100_000, 130_000, 1_000_000, 200));
        assert!(!within_no_trade_band(100_000, 130_000, 1_000_000, 0));
    }

    #[test]
    fn turnover_cap_bounds_epoch_movement() {
        // 25% of 1_000_000 = 250_000 budget.
        assert_eq!(apply_turnover_cap(100_000, 0, 1_000_000, 2_500), 100_000);
        assert_eq!(apply_turnover_cap(100_000, 200_000, 1_000_000, 2_500), 50_000);
        assert_eq!(apply_turnover_cap(100_000, 250_000, 1_000_000, 2_500), 0);
    }

    // -- shares -------------------------------------------------------------

    #[test]
    fn shares_round_trip_never_favors_the_withdrawer() {
        let s = shares_for_deposit(1_000_000, 0, 0);
        let back = assets_for_shares(s, s, 1_000_000);
        assert!(back <= 1_000_000, "round trip must not mint value");
    }

    #[test]
    fn share_price_is_immune_to_donated_tokens() {
        // NAV is an accounting counter, so a direct transfer into the vault
        // (modelled here as nav that was never credited) cannot change what a
        // holder redeems. Two depositors, then an uncredited donation.
        let mut total_shares = 0u128;
        let mut nav = 0u64;

        let s1 = shares_for_deposit(1_000_000, total_shares, nav);
        total_shares += s1;
        nav += 1_000_000;

        let s2 = shares_for_deposit(1_000_000, total_shares, nav);
        total_shares += s2;
        nav += 1_000_000;

        // Both paid the same price.
        assert_eq!(s1, s2);
        let redeem = assets_for_shares(s1, total_shares, nav);
        assert!(redeem <= 1_000_000);
        assert!(redeem >= 999_999);
    }

    #[test]
    fn first_depositor_cannot_grief_with_one_atom() {
        // Attacker seeds 1 atom, then a victim deposits. With the virtual
        // offset the victim still receives proportional shares.
        let attacker = shares_for_deposit(1, 0, 0);
        let total = attacker;
        let nav = 1u64;
        let victim = shares_for_deposit(1_000_000, total, nav);
        let victim_out = assets_for_shares(victim, total + victim, nav + 1_000_000);
        assert!(
            victim_out >= 999_000,
            "victim recovered only {}",
            victim_out
        );
    }

    #[test]
    fn profit_lifts_share_price_for_everyone() {
        let s = shares_for_deposit(1_000_000, 0, 0);
        // Colony realizes +10%: nav rises with no new shares.
        let out = assets_for_shares(s, s, 1_100_000);
        assert!(out > 1_090_000 && out <= 1_100_000, "got {}", out);
    }

    // -- loss waterfall and slashing ----------------------------------------

    #[test]
    fn loss_waterfall_order_is_bond_then_cache_then_nav() {
        // loss 100, bond 50, cache 30 -> bond 50, cache 30, depositors 20
        let o = cover_loss(100, 50, 30);
        assert_eq!(
            o,
            LossOutcome {
                from_bond: 50,
                from_cache: 30,
                to_vault_loss: 20
            }
        );

        // The bond alone covers it.
        let o2 = cover_loss(100, 500, 30);
        assert_eq!(
            o2,
            LossOutcome {
                from_bond: 100,
                from_cache: 0,
                to_vault_loss: 0
            }
        );

        // Bond plus cache exactly cover it: depositors are untouched.
        let o3 = cover_loss(100, 40, 60);
        assert_eq!(o3.to_vault_loss, 0);

        // The cushion is finite: with nothing posted, depositors take it all.
        let o4 = cover_loss(100, 0, 0);
        assert_eq!(o4.to_vault_loss, 100);
    }

    #[test]
    fn loss_waterfall_conserves_value() {
        for loss in [0u64, 1, 99, 1_000, u64::MAX / 4] {
            for bond in [0u64, 7, 500] {
                for cache in [0u64, 13, 900] {
                    let o = cover_loss(loss, bond, cache);
                    assert_eq!(o.from_bond + o.from_cache + o.to_vault_loss, loss);
                    assert!(o.from_bond <= bond);
                    assert!(o.from_cache <= cache);
                }
            }
        }
    }

    #[test]
    fn slash_split_sums_and_halves() {
        let o = slash_split(1_000, 2_000, 5_000);
        assert_eq!(
            o,
            SlashOutcome {
                slashed: 200,
                burn: 100,
                to_cache: 100
            }
        );
        assert_eq!(o.burn + o.to_cache, o.slashed);
    }

    #[test]
    fn integrity_slash_takes_the_whole_bond() {
        let o = slash_split(1_000, 10_000, 5_000);
        assert_eq!(o.slashed, 1_000);
        assert_eq!(o.burn + o.to_cache, 1_000);
    }

    #[test]
    fn loss_slash_ramps_past_the_threshold() {
        // At or under the threshold nothing is seized.
        assert_eq!(loss_slash_bps(2_000, 3_000), 0);
        assert_eq!(loss_slash_bps(3_000, 3_000), 0);
        // Halfway past the threshold seizes half the bond.
        assert_eq!(loss_slash_bps(4_500, 3_000), 5_000);
        // Twice the threshold seizes all of it, and it stays clamped.
        assert_eq!(loss_slash_bps(6_000, 3_000), 10_000);
        assert_eq!(loss_slash_bps(9_000, 3_000), 10_000);
    }

    #[test]
    fn cache_accrual_takes_the_configured_cut() {
        assert_eq!(cache_accrual(1_000_000, 1_000), 100_000);
        assert_eq!(cache_accrual(0, 1_000), 0);
        assert_eq!(cache_accrual(1_000_000, 0), 0);
    }
}
