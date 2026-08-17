//! Structural invariants that no single file can assert about itself.
//!
//! One claim is defended here, and it is the claim the whole project rests on:
//! **the admission burn is a binary eligibility gate and nothing more.** An
//! operator burns a fixed count of $KOLNY to be admitted as a Scout, and what
//! it burned then buys nothing -- no pheromone, no seed trail, no weight, no
//! larger scout ticket, no earlier promotion. Capital follows realized
//! performance. If burning more could buy allocation, every realized-performance
//! figure the product publishes would be a lie, so this is not a preference to
//! be re-litigated in a later refactor; it is enforced two ways.
//!
//!  1. `allocation_sources_never_mention_the_admission_fields` -- the files that
//!     decide how much capital a forager gets cannot so much as name the
//!     admission fields. This is the one with teeth: it fails on the attempt,
//!     not on the consequence, so a reviewer does not have to reason about
//!     whether some new read is harmless.
//!  2. `changing_the_admission_burn_changes_no_allocation_output` -- the
//!     allocation math, driven by a real `ColonyConfig`, produces identical
//!     output at every admission amount in the published band. This is
//!     supporting evidence rather than proof: it exercises `math.rs` through a
//!     harness, not the instruction handlers, which need a validator. Read it
//!     together with (1), which is what actually covers the handlers.
//!
//! This file is deliberately NOT one of the files it inspects. A scanner that
//! scans itself counts its own vocabulary as evidence, which is how a gate ends
//! up reporting inflated numbers that bury the real ones.

use anchor_lang::prelude::Pubkey;

use crate::constants::*;
use crate::math::{self, CapLevel, TOP_TRAILS_LEN};
use crate::state::ColonyConfig;

// ---------------------------------------------------------------------------
// Source scan
// ---------------------------------------------------------------------------

/// Lines with comments stripped, blank lines dropped.
///
/// Comments are excluded on purpose. A gate that fails an honest note in
/// `settlement.rs` saying "the admission burn never reaches here" teaches the
/// next author to leave the note out, and a rule that punishes accurate writing
/// makes the codebase less honest, not more.
///
/// Truncating at the first `//` can also cut a `//` inside a string literal.
/// The only cost is searching slightly less text on such a line, and no line in
/// these files puts one of the scanned symbols after a URL.
fn code_lines(src: &str) -> Vec<(usize, &str)> {
    src.lines()
        .enumerate()
        .map(|(i, l)| {
            (
                i + 1,
                match l.find("//") {
                    Some(at) => &l[..at],
                    None => l,
                },
            )
        })
        .filter(|(_, l)| !l.trim().is_empty())
        .collect()
}

/// Every symbol that carries the admission burn.
const ADMISSION_SYMBOLS: [&str; 7] = [
    "admission_burn_amount",
    "total_kolny_burned",
    "kolny_mint",
    "ADMISSION_BURN",
    "record_admission_burn",
    "burn_from_user",
    "KolnyBurned",
];

/// The files that decide how much capital a forager gets, each with a floor on
/// the number of code lines the scan must actually see.
///
/// The floor is not decoration. A scan of an empty string reports zero hits and
/// reads exactly like a clean file, so "found nothing" has to be distinguished
/// from "looked at nothing" before the absence of hits means anything.
const ALLOCATION_SOURCES: [(&str, &str, usize); 4] = [
    (
        "instructions/settlement.rs",
        include_str!("instructions/settlement.rs"),
        400,
    ),
    ("instructions/risk.rs", include_str!("instructions/risk.rs"), 250),
    (
        "instructions/vault.rs",
        include_str!("instructions/vault.rs"),
        250,
    ),
    ("math.rs", include_str!("math.rs"), 400),
];

/// The files that are supposed to carry the admission burn. Scanned by the same
/// code, so a rename that silences the scan above shows up here as a symbol
/// that has stopped being found anywhere.
const ADMISSION_SOURCES: [(&str, &str); 6] = [
    ("state.rs", include_str!("state.rs")),
    ("constants.rs", include_str!("constants.rs")),
    ("events.rs", include_str!("events.rs")),
    ("utils.rs", include_str!("utils.rs")),
    ("instructions/admin.rs", include_str!("instructions/admin.rs")),
    (
        "instructions/forager.rs",
        include_str!("instructions/forager.rs"),
    ),
];

#[test]
fn allocation_sources_never_mention_the_admission_fields() {
    for (name, src, min_code_lines) in ALLOCATION_SOURCES {
        let lines = code_lines(src);
        assert!(
            lines.len() >= min_code_lines,
            "{} scanned only {} code lines (floor {}); the scan is reading the \
             wrong thing, and a clean result here would mean nothing",
            name,
            lines.len(),
            min_code_lines
        );

        for symbol in ADMISSION_SYMBOLS {
            let hits: Vec<usize> = lines
                .iter()
                .filter(|(_, l)| l.contains(symbol))
                .map(|(n, _)| *n)
                .collect();
            assert!(
                hits.is_empty(),
                "{} names `{}` at line(s) {:?}. Allocation must depend on \
                 realized performance alone: a capital path that can read what \
                 an operator burned lets tokens buy weight, and every realized \
                 figure the product publishes stops being true. Put the \
                 admission burn back at `register_forager`, which is the only \
                 gate it belongs on.",
                name,
                symbol,
                hits
            );
        }
    }
}

#[test]
fn the_scan_still_finds_these_symbols_where_they_belong() {
    // Without this, a rename would empty the scan above and it would pass
    // forever on a vocabulary nothing uses any more. `expect_clean` checks rot
    // silently; only a paired positive control keeps them honest.
    for symbol in ADMISSION_SYMBOLS {
        let total: usize = ADMISSION_SOURCES
            .iter()
            .map(|(_, src)| {
                code_lines(src)
                    .iter()
                    .filter(|(_, l)| l.contains(symbol))
                    .count()
            })
            .sum();
        assert!(
            total > 0,
            "`{}` is no longer found in any admission source. Either it was \
             renamed -- in which case ADMISSION_SYMBOLS must be updated or the \
             allocation scan is now blind to it -- or the burn was removed.",
            symbol
        );
    }

    // The three that carry the whole mechanism must be present in the specific
    // file that owns them, not merely somewhere in the union.
    let owners: [(&str, &str); 3] = [
        ("state.rs", "total_kolny_burned"),
        ("constants.rs", "MAX_ADMISSION_BURN_BASE_UNITS"),
        ("instructions/forager.rs", "burn_from_user"),
    ];
    for (file, symbol) in owners {
        let (_, src) = ADMISSION_SOURCES
            .iter()
            .find(|(n, _)| *n == file)
            .expect("owner file must be in ADMISSION_SOURCES");
        let hits = code_lines(src)
            .iter()
            .filter(|(_, l)| l.contains(symbol))
            .count();
        assert!(hits > 0, "{} no longer contains `{}`", file, symbol);
    }
}

// ---------------------------------------------------------------------------
// Numeric invariance
// ---------------------------------------------------------------------------

/// The forager state the allocation path reads.
#[derive(Clone)]
struct SampleForager {
    pheromone: u64,
    principal: u64,
    bond: u64,
    realized_pnl_epoch: i64,
    drawdown_bps: u16,
    scout_epochs: u64,
    realized_epochs: u64,
    scout_perf_cum_bps: i64,
    realized_pnl_cumulative: i64,
}

fn sample_foragers() -> Vec<SampleForager> {
    // Deliberately uneven: trails large enough to hit the concentration cap,
    // one losing, one thin, one bond-constrained, one dead. A flat set would
    // leave the cap solver and the bond ceiling untested and the fingerprint
    // would not be measuring the parts of the pipeline most likely to acquire a
    // new input.
    //
    // Six of them, and the count is load-bearing. `effective_max_weight_bps`
    // relaxes the cap whenever `n_active * w_max < 1`, so at four foragers the
    // default cap and the minimum cap both relax to the same 2600 bps and a
    // control that moved `w_max` moved nothing. That is how a control proves a
    // fingerprint is blind: the first version of this test asserted the
    // fingerprint reacted to the cap, and it did not.
    vec![
        SampleForager {
            pheromone: 9_000_000,
            principal: 400_000_000,
            bond: 90_000_000,
            realized_pnl_epoch: 24_000_000,
            drawdown_bps: 400,
            scout_epochs: 6,
            realized_epochs: 5,
            scout_perf_cum_bps: 1_800,
            realized_pnl_cumulative: 51_000_000,
        },
        SampleForager {
            pheromone: 3_100_000,
            principal: 150_000_000,
            bond: 12_000_000,
            realized_pnl_epoch: -9_000_000,
            drawdown_bps: 2_100,
            scout_epochs: 4,
            realized_epochs: 3,
            scout_perf_cum_bps: -400,
            realized_pnl_cumulative: -3_000_000,
        },
        SampleForager {
            pheromone: 700_000,
            principal: 20_000_000,
            bond: 2_000_000,
            realized_pnl_epoch: 300_000,
            drawdown_bps: 0,
            scout_epochs: 4,
            realized_epochs: 4,
            scout_perf_cum_bps: 250,
            realized_pnl_cumulative: 900_000,
        },
        SampleForager {
            pheromone: 0,
            principal: 0,
            bond: 1_000_000,
            realized_pnl_epoch: 0,
            drawdown_bps: 0,
            scout_epochs: 1,
            realized_epochs: 0,
            scout_perf_cum_bps: 0,
            realized_pnl_cumulative: 0,
        },
        SampleForager {
            pheromone: 8_400_000,
            principal: 380_000_000,
            bond: 4_000_000,
            realized_pnl_epoch: 11_000_000,
            drawdown_bps: 900,
            scout_epochs: 5,
            realized_epochs: 4,
            scout_perf_cum_bps: 950,
            realized_pnl_cumulative: 22_000_000,
        },
        SampleForager {
            pheromone: 5_200_000,
            principal: 210_000_000,
            bond: 40_000_000,
            realized_pnl_epoch: 2_000_000,
            drawdown_bps: 1_450,
            scout_epochs: 9,
            realized_epochs: 7,
            scout_perf_cum_bps: 3_100,
            realized_pnl_cumulative: 8_400_000,
        },
    ]
}

/// A colony carrying the published defaults, with the admission fields set to
/// whatever the caller wants to vary.
fn config_with(admission_burn_amount: u64, total_kolny_burned: u64, kolny_mint: Pubkey) -> ColonyConfig {
    let mut c = ColonyConfig::default();

    c.rho_bps = DEFAULT_RHO_BPS;
    c.deposit_scale_q = DEFAULT_DEPOSIT_SCALE_Q;
    c.perf_norm_s_bps = DEFAULT_PERF_NORM_S_BPS;
    c.risk_aversion_bps = DEFAULT_RISK_AVERSION_BPS;

    c.w_max_bps = DEFAULT_W_MAX_BPS;
    c.w_drop_bps = DEFAULT_W_DROP_BPS;
    c.scout_budget_bps = DEFAULT_SCOUT_BUDGET_BPS;
    c.reband_band_bps = DEFAULT_REBAND_BAND_BPS;
    c.turnover_cap_bps = DEFAULT_TURNOVER_CAP_BPS;

    c.promote_min_epochs = DEFAULT_PROMOTE_MIN_EPOCHS;
    c.promote_min_realized_epochs = DEFAULT_PROMOTE_MIN_REALIZED_EPOCHS;
    c.promote_perf_bar_bps = DEFAULT_PROMOTE_PERF_BAR_BPS;
    c.promote_tau_seed_cap = DEFAULT_PROMOTE_TAU_SEED_CAP;
    c.scout_ticket_base_units = 1_000_000;

    c.min_bond = 1_000_000;
    c.bond_ratio_bps = DEFAULT_BOND_RATIO_BPS;
    c.bond_haircut_bps = DEFAULT_BOND_HAIRCUT_BPS;

    c.admission_burn_amount = admission_burn_amount;
    c.total_kolny_burned = total_kolny_burned;
    c.kolny_mint = kolny_mint;

    c
}

/// Everything the colony decides about capital, for one epoch, as one vector.
///
/// Each step is the same `math` function the settlement crank and the rebalance
/// path call, reading its parameters from the same `ColonyConfig`. If any
/// allocation input ever started depending on the admission burn, one of these
/// numbers would move.
fn allocation_fingerprint(config: &ColonyConfig, foragers: &[SampleForager]) -> Vec<i128> {
    let nav: u64 = 1_000_000_000;
    let mut out: Vec<i128> = Vec::new();

    // -- settle: pheromone update ------------------------------------------
    let mut top = [0u64; TOP_TRAILS_LEN];
    let mut count: u8 = 0;
    let mut pheromone_sum: u128 = 0;
    let mut updated: Vec<u64> = Vec::new();

    for f in foragers {
        let r = math::return_bps(f.realized_pnl_epoch, f.principal);
        let perf = math::risk_adjusted_perf_bps(r, f.drawdown_bps, config.risk_aversion_bps);
        let deposit = math::deposit_fp6(perf, config.perf_norm_s_bps, config.deposit_scale_q);
        let tau = math::update_pheromone(f.pheromone, deposit, config.rho_bps, PHEROMONE_CEIL);

        out.push(r as i128);
        out.push(perf as i128);
        out.push(deposit as i128);
        out.push(tau as i128);

        updated.push(tau);
        count = math::top_insert(&mut top, count, tau);
        pheromone_sum = pheromone_sum.saturating_add(tau as u128);
    }

    // -- finalize: solve the level and size the pools -----------------------
    let active_count = foragers.len() as u32;
    let eff_w_max = math::effective_max_weight_bps(config.w_max_bps, active_count, CAP_RELAX_MARGIN_BPS);
    let occupied = (count as usize).min(TOP_TRAILS_LEN);
    let level: CapLevel = math::solve_cap_level(pheromone_sum, &top[..occupied], active_count, eff_w_max);

    let scout_pool = ((nav as u128) * (config.scout_budget_bps as u128) / math::BPS_DENOM) as u64;
    let allocatable_pool = nav.saturating_sub(scout_pool);

    out.push(eff_w_max as i128);
    out.push(level.capped_count as i128);
    out.push(level.remaining_bps as i128);
    out.push(level.rest_sum as i128);
    out.push(math::alloc_divisor_of(&level) as i128);
    out.push(scout_pool as i128);
    out.push(allocatable_pool as i128);

    // -- rebalance: targets, band, turnover ---------------------------------
    let mut turnover_used: u64 = 0;
    for (f, tau) in foragers.iter().zip(updated.iter()) {
        let weight = math::weight_bps_from_level(*tau, &level, eff_w_max);
        let target = math::allocation_target(*tau, &level, allocatable_pool, eff_w_max)
            .min(math::bond_capacity(f.bond, config.bond_ratio_bps, config.bond_haircut_bps));
        let banded = math::within_no_trade_band(f.principal, target, allocatable_pool, config.reband_band_bps);
        let moved = math::apply_turnover_cap(
            f.principal.abs_diff(target),
            turnover_used,
            allocatable_pool,
            config.turnover_cap_bps,
        );
        turnover_used = turnover_used.saturating_add(moved);

        out.push(weight as i128);
        out.push(target as i128);
        out.push(banded as i128);
        out.push(moved as i128);
    }

    // -- exploration budget and promotion -----------------------------------
    out.push(config.scout_ticket_base_units as i128);
    for (f, tau) in foragers.iter().zip(updated.iter()) {
        let criteria_met = f.scout_epochs >= config.promote_min_epochs as u64
            && f.realized_epochs >= config.promote_min_realized_epochs as u64
            && f.scout_perf_cum_bps >= config.promote_perf_bar_bps as i64
            && f.realized_pnl_cumulative >= 0;
        let seeded = (*tau).min(config.promote_tau_seed_cap);
        let bond_ok = f.bond >= config.min_bond;

        out.push(criteria_met as i128);
        out.push(seeded as i128);
        out.push(bond_ok as i128);
    }

    out
}

#[test]
fn changing_the_admission_burn_changes_no_allocation_output() {
    let foragers = sample_foragers();
    let baseline = allocation_fingerprint(
        &config_with(DEFAULT_ADMISSION_BURN_BASE_UNITS, 0, Pubkey::default()),
        &foragers,
    );

    // The fingerprint has to be worth comparing before an equality over it
    // proves anything.
    assert!(
        baseline.len() >= 40,
        "fingerprint covers only {} values",
        baseline.len()
    );
    assert!(
        baseline.iter().filter(|v| **v != 0).count() >= 20,
        "fingerprint is mostly zeros; it is not exercising the pipeline"
    );

    // Controls. Parameters that DO govern allocation must move the fingerprint,
    // one per stage, otherwise the equality assertions below would hold over a
    // number that measures nothing. Each stage is checked separately: a single
    // control would prove the fingerprint reacts to something, not that it
    // reads the stage a future admission leak would land in.
    let stage_controls: [(&str, fn(&mut ColonyConfig)); 6] = [
        ("rho_bps (pheromone update)", |c| c.rho_bps = MAX_RHO_BPS),
        ("w_max_bps (concentration cap)", |c| {
            c.w_max_bps = MIN_W_MAX_BPS
        }),
        ("scout_budget_bps (pool sizing)", |c| {
            c.scout_budget_bps = MAX_SCOUT_BUDGET_BPS
        }),
        ("bond_ratio_bps (bond ceiling)", |c| {
            c.bond_ratio_bps = MAX_BOND_RATIO_BPS
        }),
        ("scout_ticket_base_units (exploration)", |c| {
            c.scout_ticket_base_units = 7_777_777
        }),
        ("promote_tau_seed_cap (promotion)", |c| {
            c.promote_tau_seed_cap = 1
        }),
    ];
    for (label, mutate) in stage_controls {
        let mut moved = config_with(DEFAULT_ADMISSION_BURN_BASE_UNITS, 0, Pubkey::default());
        mutate(&mut moved);
        assert_ne!(
            allocation_fingerprint(&moved, &foragers),
            baseline,
            "the fingerprint did not react to {}; that stage is not being \
             measured, so equality over it would prove nothing",
            label
        );
    }

    // The claim. Every admission amount the authority can reach, every burn
    // total, every mint: allocation is bit-identical.
    let mints = [
        Pubkey::default(),
        Pubkey::new_from_array([9u8; 32]),
        Pubkey::new_from_array([255u8; 32]),
    ];
    let totals = [0u64, 1, 500_000_000_000, u64::MAX / 2];
    let amounts = [
        MIN_ADMISSION_BURN_BASE_UNITS,
        MIN_ADMISSION_BURN_BASE_UNITS + 1,
        DEFAULT_ADMISSION_BURN_BASE_UNITS / 2,
        DEFAULT_ADMISSION_BURN_BASE_UNITS,
        DEFAULT_ADMISSION_BURN_BASE_UNITS * 3,
        MAX_ADMISSION_BURN_BASE_UNITS,
    ];

    let mut compared = 0usize;
    for amount in amounts {
        for total in totals {
            for mint in mints {
                let got = allocation_fingerprint(&config_with(amount, total, mint), &foragers);
                assert_eq!(
                    got, baseline,
                    "allocation moved when the admission burn was set to {} \
                     (total burned {}, mint {}). Admission is an eligibility \
                     gate; it must not price a single unit of capital.",
                    amount, total, mint
                );
                compared += 1;
            }
        }
    }
    assert_eq!(compared, 6 * 4 * 3);
}
