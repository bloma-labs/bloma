# KOLNY Allocation Specification

This is the core of KOLNY. It defines how realized performance becomes a
pheromone score, how that score decays over time, and how the colony turns
pheromone into per-forager capital weights each epoch. Every parameter here is
designed to be an on-chain config value; the parameter table in section 10 is
written so the Anchor engineer can implement it directly.

The design is a direct adaptation of Ant Colony Optimization. In Ant System
(Dorigo, Maniezzo, and Colorni, 1996) the pheromone on graph edge `xy` evolves as

```
tau_xy  <-  (1 - rho) * tau_xy  +  sum_k  delta_tau_xy^k
delta_tau_xy^k  =  Q / L_k   if ant k used edge xy in its tour,   else 0
```

where `rho` is the pheromone evaporation coefficient and `L_k` is the cost of
ant `k`'s tour, so a better (shorter) tour deposits more pheromone. KOLNY keeps
this structure exactly and reinterprets the terms:

- an edge becomes a **forager** (a trail the colony can send capital down),
- tour quality `Q / L_k` becomes **risk-adjusted realized performance**,
- **evaporation `rho`** becomes the time-decay that stops capital from chasing
  stale winners, and
- pheromone `tau` becomes the **allocation weight score**.

A note on `rho`, because the literature is inconsistent. The original 1996 Ant
System paper writes the update as `tau(t+n) = rho * tau(t) + delta_tau` and calls
`rho` the trail *persistence*, so evaporation there is `(1 - rho)`. The modern
convention used by later ACO writing and by this document is the opposite: `rho`
is the *evaporation rate* and persistence is `(1 - rho)`. KOLNY uses the modern
convention throughout: **larger `rho` means faster forgetting.** See
`references.md`.

---

## 1. Pheromone update

For each forager `f` and epoch `e`:

```
tau_f(e+1)  =  max( 0 ,  (1 - rho) * tau_f(e)  +  D_f(e) )
```

- `tau_f` is the forager's pheromone score, `tau_f >= 0`.
- `rho` in (0, 1] is the per-epoch evaporation rate (section 5).
- `D_f(e)` is the epoch's performance deposit (section 3). Unlike classic ACO,
  `D_f` may be **negative**: a losing epoch actively erodes the trail rather than
  merely failing to reinforce it. The `max(0, .)` floor keeps the score a valid
  allocation weight; a forager driven to `tau = 0` holds no trail and receives no
  main-pool capital until it earns positive deposits again (by then it is
  typically in probation or back in the Scout Sandbox, see section 6).

Everything downstream is a function of the `tau_f` vector: allocation weights are
just normalized, capped pheromone (section 7).

---

## 2. Realized performance score

The deposit is built from a single per-epoch, per-forager number, the
**risk-adjusted realized performance**:

```
perf_f(e)  =  r_f(e)  -  lambda * DD_f(e)
```

- `r_f(e)` is the forager's **realized net return rate** over epoch `e`: the PnL
  from positions that were closed and settled on-chain within the epoch, net of
  trading fees, slippage, and funding, divided by the average capital deployed to
  that forager during the epoch. It is a unitless rate, so it compares foragers
  fairly regardless of how much capital each held.
- `DD_f(e) >= 0` is the **realized maximum drawdown** inside the epoch: the
  largest peak-to-trough decline of the forager's realized equity curve.
- `lambda >= 0` is the **risk-aversion coefficient**. It penalizes returns that
  were earned through deep intra-epoch drawdowns, so a forager cannot climb the
  trail by taking ruinous risk that happened to pay off this once.

The subtractive drawdown penalty is chosen over a divisive Sharpe/Sortino ratio
because it is cheap and unambiguous to compute in on-chain fixed-point and it
stays well-defined when volatility is near zero. A Sortino-style denominator
(dividing by downside deviation) is a valid alternative if richer trade-level
data is committed; it is noted here as a future refinement, not the default.

### 2.1 Realized-only rule (mandatory)

`r_f(e)` and `DD_f(e)` are computed **only** from positions closed and settled
before the epoch-close slot. Open positions are marked, at a conservative oracle
price, for display of current drawdown on the Trail Board, but that mark **never**
enters `perf_f` and never moves capital. Backtests and paper results never enter
`perf_f`. This rule is what lets the header advertise realized performance
honestly, and it removes the largest manipulation surface: you cannot inflate
your trail by holding a losing position open or by marking your own book. See
`risk-spec.md` and `security.md` for the settlement-finality and oracle defenses.

---

## 3. The deposit transform

```
D_f(e)  =  Q * tanh( perf_f(e) / s )
```

- `Q > 0` is the **deposit scale**: the maximum magnitude a single epoch can add
  to or remove from a trail. `D_f` is bounded to the open interval `(-Q, +Q)`.
- `s > 0` is the **performance normalization scale**: the risk-adjusted return at
  which the deposit reaches most of its magnitude. At `perf = s` the deposit is
  `Q * tanh(1) ~ 0.76 Q`; at `perf = 2s` it is `~ 0.96 Q`.

Why a bounded, sign-preserving transform:

- **Bounded** so one enormous epoch, whether a lucky fat tail or a manipulation
  attempt, cannot dominate a trail. Per-epoch influence is capped at `Q`. This is
  the single most important robustness property of the deposit.
- **Sign-preserving** so a loss subtracts pheromone (the trail erodes faster than
  passive evaporation), while classic ACO would only withhold reinforcement.
- **Smooth and monotone** so small changes in performance produce small,
  predictable changes in the trail, which keeps rebalancing stable.

On-chain, `tanh` is approximated in fixed-point; the deposit is submitted with
the realized input and the program re-checks that it lies in `(-Q, +Q)` and
carries the correct sign before accepting it (see `architecture.md` section 5).

---

## 4. Realized performance, not marks (restated as an invariant)

The allocation engine must reject any epoch commit whose `perf_f` is not derived
purely from settled, closed trades. This is restated here as a hard invariant
because it is the rule most likely to be quietly bent under pressure to make the
numbers look better: **if it is not realized and settled on-chain, it does not
move capital and it does not appear in a trust indicator.**

---

## 5. Time decay and half-life

Evaporation is the mechanism that makes old performance fade. With no new
deposit, a trail retains `(1 - rho)` of its pheromone each epoch, so after `n`
epochs it holds `(1 - rho)^n` of where it started. The **half-life** `H`, the
number of epochs for an un-replenished trail to lose half its pheromone, is

```
(1 - rho)^H = 1/2      =>      H = ln 2 / ( -ln(1 - rho) )
rho = 1 - 2^(-1/H)
```

Choose the half-life in real time, then derive `rho` from the epoch length:

| Half-life H (epochs) | rho | With weekly epochs |
|---|---|---|
| 1 | 0.5000 | half-life 1 week |
| 2 | 0.2929 | half-life 2 weeks |
| 3 | 0.2063 | half-life 3 weeks |
| 4 | 0.1591 | half-life ~1 month |
| 6 | 0.1091 | half-life ~1.5 months |
| 8 | 0.0830 | half-life ~2 months |
| 13 | 0.0519 | half-life ~3 months |

**Default: `rho = 0.16` on a weekly epoch, a half-life of about four weeks.**
The reasoning: a strategy edge in crypto markets is non-stationary and regimes
shift on the scale of weeks, so the colony must forget an edge that has died; but
a single bad week is mostly noise, so the half-life must be long enough that one
epoch does not erase a genuinely good trail. Four weeks sits between those two
failure modes. At `rho = 0.16`, an un-replenished trail falls to 10 percent of
its pheromone after about 13 epochs (`ln 0.1 / ln 0.84 ~ 13.2`), roughly three
months, which is a reasonable "fully forgotten" horizon. `rho` is an on-chain
config value and can be retuned.

This is not decoration. Weighting recent observations more than old ones by a
geometric factor is exactly how Discounted-UCB handles non-stationary bandits
(Garivier and Moulines, 2011): it discounts a reward seen `k` steps ago by
`gamma^k`. Our `(1 - rho)^k` is the same geometric discount, so `(1 - rho)` plays
the role of the discount factor `gamma`. Section 9 develops the bandit view.

---

## 6. Exploration budget and cold start (Scout Sandbox)

A brand-new forager has `tau = 0`. Under pure pheromone weighting it would
receive zero capital forever and could never prove itself, which is the classic
cold-start and rich-get-richer failure. The fix is an explicit **exploration
budget**, the on-chain analog of the exploration term in a bandit policy.

- A fixed fraction `scout_budget` of total value under colony (default 10
  percent) is reserved for the **Scout Sandbox** and is never allocated by
  pheromone. The main pheromone-weighted pool is therefore
  `main_pool = (1 - scout_budget) * deployable_TVL`.
- New and probationary foragers trade only small, fixed-size **scout tickets**
  from this budget. A scout's downside is bounded to its ticket plus its posted
  bond, so exploration never risks a large slice of principal.
- A scout is promoted to Active when it clears all of: at least
  `promote_min_epochs` scout epochs, of which at least
  `promote_min_realized_epochs` closed with a non-zero realized result, and
  cumulative risk-adjusted realized performance at or above
  `promote_perf_bar`. On promotion its pheromone is seeded from its scout-phase
  performance, capped at `promote_tau_seed_cap` so a scout cannot enter the main
  pool at the top of the trail.

Reserving a fixed budget of small tickets is preferred over adding a UCB-style
optimism bonus to each forager's score. An on-chain confidence bonus would add
compute and, worse, a manipulation surface (gaming visit counts to inflate the
bonus). A reserved budget is simple, auditable, and hard-bounds new-agent risk.
The UCB-bonus approach is recorded as a possible future refinement in section 11.

---

## 7. From pheromone to weights

Given the pheromone vector over the Active foragers, weights are computed by
normalize, then cap, then drop, then renormalize:

```
1.  raw_f      = tau_f / sum_g tau_g                 over Active foragers
2.  cap:       any w_f > w_max is set to w_max; the removed excess is
               redistributed proportionally among the un-capped foragers;
               repeat until no un-capped forager exceeds w_max
               (bounded water-filling)
3.  drop:      any post-cap w_f < w_drop is set to 0; that forager is flagged
               for demotion to Scout and its capital is withdrawn to the vault;
               the freed weight is redistributed among the survivors
4.  final w_f  are the target weights;  sum_f w_f = 1
```

- `w_max` (default 0.20) caps single-forager concentration so that no one agent
  failing can take an outsized share of allocated capital, and so the pool is
  spread over at least `1 / w_max` foragers. It is the main systemic-risk control
  on the allocation side; it complements sub-account isolation and per-forager
  position limits in `risk-spec.md`.
- `w_drop` (default 0.03) is a **demotion threshold, not a floor.** A forager
  whose trail has decayed below it is removed from the main pool and sent back to
  the Scout Sandbox, rather than being propped up at a minimum weight. Propping
  up losers would fight the entire pheromone mechanism; dropping them is the
  point.

### 7.1 Feasibility constraint (edge case)

The cap is only feasible if `N_active * w_max >= 1`; otherwise the weights cannot
sum to 1 while every weight stays at or below `w_max`. Two consequences the
implementer must handle:

- If `N_active * w_max < 1`, the effective cap relaxes to
  `max(w_max, 1/N_active + margin)`, and any capital that still cannot be placed
  under the cap stays in the vault as reported un-deployed reserve. It is never
  hidden or silently over-concentrated.
- At exactly `N_active = 1/w_max` the cap forces every weight to `w_max`, so
  pheromone can no longer express any preference. Therefore `w_max` should be set
  comfortably above `1 / target_forager_count`. With the default `w_max = 0.20`,
  the colony wants well more than five Active foragers for weighting to breathe;
  the numeric example below deliberately uses a looser `w_max = 0.35` precisely
  because it has only five foragers.

---

## 8. Epochs and rebalancing cost

- An **epoch** is a fixed wall-clock period, default 7 days, range 1 to 30 days.
  Long enough to realize meaningful performance and to amortize rebalancing cost,
  short enough to adapt to regime change.
- Rebalancing at the epoch boundary moves capital between sub-accounts and costs
  slippage and gas. Two controls keep churn down:
  - **No-trade band `reband_band`** (default 2 percent): if a forager's target
    weight moved less than the band from its current weight, it is not rebalanced
    this epoch. Small pheromone wiggles do not trigger costly trades.
  - **Turnover cap `turnover_cap`** (default 25 percent of the pool per epoch):
    total capital moved per epoch is bounded; if desired moves exceed it, all
    moves are scaled down and the target is approached gradually over several
    epochs. This bounds worst-case slippage.
- Rebalancing is executed in the vault base asset and netted, so only the delta
  between current and target moves, not gross positions. Rebalancing cost is
  socialized across the vault; the band and cap keep the expected cost small.

There is a real tension here: chasing pheromone perfectly every epoch maximizes
responsiveness but maximizes transaction cost, while a wide band and low turnover
cap minimize cost but lag the signal. The defaults lean slightly toward cost
control, on the view that the pheromone signal is itself smoothed (bounded
deposits plus a multi-week half-life) and does not need to be tracked tick for
tick.

---

## 9. The colony as a non-stationary bandit

Framed as a multi-armed bandit, each forager is an arm, and its realized return
is a noisy reward whose distribution drifts over time as market regimes change
and as the agent's own model ages. This framing explains every choice above:

- **Exploitation** is allocating more capital to trails with high recent
  performance (high pheromone).
- **Exploration** is the reserved Scout budget that keeps funding unproven arms
  so their value can be learned (section 6).
- **Non-stationarity** is why evaporation exists. Stationary policies such as
  UCB1 (Auer, Cesa-Bianchi, and Fischer, 2002) assume each arm's reward
  distribution is fixed and will keep exploiting an arm that was good long ago.
  Discounted-UCB (Garivier and Moulines, 2011) fixes this by discounting old
  rewards geometrically by `gamma^k`; KOLNY's `(1 - rho)^k` evaporation is that
  same discount, which is the formal justification for time decay.

KOLNY deliberately departs from textbook bandit policies in two ways, and the
departures are the design:

- A bandit policy usually **selects one arm** (argmax of an index). A fund must
  **diversify**, so KOLNY allocates capital **proportionally** to discounted,
  risk-adjusted performance, then applies hard caps. Concentration is a systemic
  risk, so putting all capital on the current-best arm is exactly what must be
  avoided.
- Exploration is a **reserved capital budget with bounded tickets**, not an
  additive optimism bonus, for the on-chain simplicity and risk-bounding reasons
  in section 6.

Thompson Sampling (Thompson, 1933; empirically revived by Chapelle and Li, 2011)
is a natural alternative weighting: allocate to each forager in proportion to its
probability of being the best, sampled from a posterior over returns. It is
recorded as a candidate for a future allocation mode; the pheromone rule is kept
as the default because it is transparent, cheap to verify on-chain, and maps
cleanly onto the colony metaphor that the whole product is built around.

---

## 10. Worked example: five foragers, three epochs

Illustrative parameters (chosen for clarity, not the production defaults):
`rho = 0.20` (retain 80 percent per epoch), deposit `D = Q * tanh(perf / s)` with
`Q = 1.0` and `s = 0.10`, all foragers seeded at `tau = 1.000`, `w_max = 0.35`,
`w_drop = 0.03`, total value under colony 1,000,000 with a 10 percent Scout
budget, so `main_pool = 900,000`.

The five foragers are written to show five behaviors: a steady star (A), an early
winner that goes flat (B), a mediocre steady hand (C), a loser (D), and a late
bloomer (E).

### 10.1 Realized performance, deposit, and pheromone

`perf` is the risk-adjusted realized performance of the epoch; `D = tanh(perf /
0.10)`; pheromone entering the next epoch is `tau' = max(0, 0.8 * tau + D)`.

| Forager | perf e1 | perf e2 | perf e3 | D e1 | D e2 | D e3 |
|---|---|---|---|---|---|---|
| A | +0.15 | +0.12 | +0.10 | +0.905 | +0.834 | +0.762 |
| B | +0.10 | +0.05 | 0.00 | +0.762 | +0.462 | 0.000 |
| C | +0.02 | +0.03 | +0.02 | +0.197 | +0.291 | +0.197 |
| D | -0.05 | -0.10 | -0.08 | -0.462 | -0.762 | -0.664 |
| E | -0.02 | +0.07 | +0.15 | -0.197 | +0.604 | +0.905 |

Pheromone trajectory (start of each epoch; `tau^1 = 1.000` for all):

| Forager | tau start e1 | tau start e2 | tau start e3 | tau start e4 |
|---|---|---|---|---|
| A | 1.000 | 1.705 | 2.198 | 2.520 |
| B | 1.000 | 1.562 | 1.711 | 1.369 |
| C | 1.000 | 0.997 | 1.089 | 1.069 |
| D | 1.000 | 0.338 | 0.000 | 0.000 |
| E | 1.000 | 0.603 | 1.086 | 1.774 |

The decimal table above is rounded to three places for reading. **The normative
input for every implementation is the FP6 integer vector below** (scale `1e-6`),
which is what the on-chain program actually stores and what the TypeScript and
Python mirrors must reproduce exactly. The decimal table is derivation evidence,
not a target: reproducing it alone is insufficient, because rounding an
intermediate step propagates. The `E / e3` cell reads `1.086` rather than `1.087`
for exactly that reason, since `0.8 * 0.602625` is `0.482100`, not `0.483`.

| Forager | tau e1 | tau e2 | tau e3 | tau e4 |
|---|---|---|---|---|
| A | 1000000 | 1705148 | 2197773 | 2519812 |
| B | 1000000 | 1561594 | 1711392 | 1369113 |
| C | 1000000 | 997375 | 1089213 | 1068745 |
| D | 1000000 | 337883 | 0 | 0 |
| E | 1000000 | 602625 | 1086468 | 1774322 |

Worked steps for the loser D and the late bloomer E, so the arithmetic is
checkable:

```
D:  tau^2 = 0.8*1.000 + (-0.462) = 0.338
    tau^3 = 0.8*0.338 + (-0.762) = 0.270 - 0.762 = -0.492 -> max(0,.) = 0.000
    D's trail is extinguished; it is dropped and demoted to Scout.

E:  tau^2 = 0.8*1.000 + (-0.197) = 0.603
    tau^3 = 0.8*0.6026 + (+0.6044) = 0.4821 + 0.6044 = 1.0865 -> 1.086
    tau^4 = 0.8*1.0865 + (+0.9051) = 0.8692 + 0.9051 = 1.7743 -> 1.774
    E starts negative, nearly drops, then overtakes B by epoch 4.
```

### 10.2 Resulting weights and capital

Weights come from normalizing the start-of-epoch pheromone, then capping at 0.35
and dropping below 0.03. Capital is `weight * 900,000`.

| Forager | w e1 | $ e1 | w e2 | $ e2 | w e3 | $ e3 |
|---|---|---|---|---|---|---|
| A | 0.200 | 180.0k | 0.328 | 294.9k | 0.350 (capped) | 315.0k |
| B | 0.200 | 180.0k | 0.300 | 270.0k | 0.286 | 257.6k |
| C | 0.200 | 180.0k | 0.192 | 172.5k | 0.182 | 163.9k |
| D | 0.200 | 180.0k | 0.065 | 58.4k | 0.000 (demoted) | 0.0 |
| E | 0.200 | 180.0k | 0.116 | 104.2k | 0.182 | 163.5k |

In epoch 3, A's raw weight is 0.361, above the 0.35 cap, so it is capped and the
0.011 excess is redistributed across B, C, and E; that is why B, C, and E are
slightly higher than their raw normalized shares.

### 10.3 Reading the example

- **D, the loser, decays and exits.** Two negative epochs plus evaporation drive
  its trail from 180k of capital to 58k to zero and a demotion to Scout within
  two epochs, with no minimum weight to prop it up. Its losses also drive the
  risk path (probation, then a bond slash) described in `risk-spec.md`; the
  allocation engine and the risk module act on the same realized numbers.
- **B, the early winner, is caught by time decay.** B never loses, but its flat
  epoch 3 deposits nothing, so pure evaporation pulls its pheromone down and E
  overtakes it by epoch 4 (E 1.774 vs B 1.369). Standing still is punished, which
  is the whole point of a non-stationary discount.
- **A, the star, meets the concentration cap.** Its trail keeps thickening until
  `w_max` binds in epoch 3 and the excess is spread to others, capping how much
  of the colony can ride one agent.
- **E, the late bloomer, is saved by exploration.** E begins negative and nearly
  drops, but the system does not execute underperformers instantly; the grace
  built into the drop threshold and Scout path lets its improving performance
  compound into the fastest-rising trail by epoch 4. This is why exploration and
  patience are structural, not optional.
- **C is the ballast.** Steady mediocre performance yields a stable mid-size
  allocation and low turnover.

---

## 11. Parameters (Anchor config)

On-chain has no floating point. Rates are stored in basis points (`bps`,
1e-4). Fixed-point quantities such as pheromone and deposits use a `1e6` scale
(call it `FP6`). Returns and drawdown are committed in `bps`; note `r` can be
negative, so it is signed. The performance computation in fixed-point is
`perf_bps = r_bps - (lambda_bps * DD_bps) / 10000`, and the deposit argument is
`perf_bps / s_bps`.

| Parameter | On-chain field | Type | Default | Range | Meaning |
|---|---|---|---|---|---|
| Evaporation rate `rho` | `rho_bps` | u16 | 1600 | 100..5000 | Per-epoch forgetting; 1600 = 0.16, half-life ~4 epochs |
| Epoch length | `epoch_duration_secs` | i64 | 604800 | 86400..2592000 | 7 days; 1 to 30 days |
| Deposit scale `Q` | `deposit_scale_q` | u64 (FP6) | 1000000 | 100000..10000000 | Max per-epoch trail change; 1.0 |
| Perf scale `s` | `perf_norm_s_bps` | u16 | 1000 | 100..5000 | Return where deposit saturates; 1000 = 10% |
| Risk aversion `lambda` | `risk_aversion_bps` | u16 | 10000 | 0..50000 | Drawdown penalty weight; 10000 = 1.0x |
| Max weight `w_max` | `w_max_bps` | u16 | 2000 | 500..4000 | Single-forager cap; 2000 = 20% |
| Drop threshold `w_drop` | `w_drop_bps` | u16 | 300 | 0..1000 | Demote-and-withdraw below; 300 = 3% |
| Scout budget | `scout_budget_bps` | u16 | 1000 | 500..2000 | Exploration reserve of TVL; 1000 = 10% |
| No-trade band | `reband_band_bps` | u16 | 200 | 0..1000 | Skip rebalance if abs(delta w) below; 200 = 2% |
| Turnover cap | `turnover_cap_bps` | u16 | 2500 | 500..10000 | Max pool fraction moved per epoch; 2500 = 25% |
| Promote min epochs | `promote_min_epochs` | u8 | 4 | 1..52 | Scout epochs before promotion eligible |
| Promote min realized epochs | `promote_min_realized_epochs` | u16 | 3 | 1..52 | Scout epochs that closed with a non-zero realized result. The chain cannot observe individual fills, so activity is counted in epochs. Enforced invariant: must be `<= promote_min_epochs`, otherwise the tenure gate becomes unreachable |
| Promote perf bar | `promote_perf_bar_bps` | i32 | 0 | -5000..20000 | Min cumulative risk-adj realized perf to promote |
| Scout ticket size | `scout_ticket_base_units` | u64 | project-set | > 0 | Fixed scout allocation per ticket |
| Promotion seed cap | `promote_tau_seed_cap` | u64 (FP6) | 1000000 | 0..5000000 | Cap on initial pheromone at promotion |
| Fixed-point scale | constant | -- | 1e6 | -- | Scale for `tau` and deposits (`FP6`) |

Bond size, slash ratio, drawdown thresholds, and cache accrual are also on-chain
config but are specified in `risk-spec.md` because they belong to the
loss-containment model.

---

## 12. Alternatives considered and rejected

- **Non-negative deposits (pure ACO).** Faithful to ants, where a bad path is
  simply not reinforced. Rejected as the default because in a fund a loss must
  erode a trail *faster* than passive evaporation and must be visible in the
  pheromone signal. KOLNY keeps the ACO update structure but generalizes the
  deposit to a signed, bounded value.
- **Unbounded linear deposit `D = perf`.** Rejected: a single huge epoch, whether
  a lucky tail or a manipulation, could dominate a trail. The `tanh` bound caps
  per-epoch influence at `Q`, which is the main defense against grinding and
  outliers.
- **Raw return with no risk adjustment.** Rejected: it rewards variance and
  hidden tail risk. The drawdown penalty makes risk-taking that only paid off by
  luck cost pheromone.
- **Softmax / Boltzmann allocation over scores.** A smooth alternative to
  normalize-and-cap, but its temperature is hard to reason about against hard
  concentration caps and it never allocates exactly zero to a dead trail.
  Explicit normalize, hard cap, and drop give auditable bounds and clean
  demotion. The softmax temperature is noted as an alternative exploration knob.
- **UCB optimism bonus per forager.** A principled exploration mechanism, but it
  adds on-chain compute and a manipulation surface around visit counts. Deferred
  in favor of a reserved exploration budget; a candidate future refinement.
- **Winner-take-all or top-k selection.** Rejected: brittle at rank boundaries,
  high turnover, and it defeats diversification, which is the reason a colony
  exists rather than a single agent.

---

## 13. Edge cases the implementer must handle

- **Too few foragers for the cap.** Relax the effective cap to
  `max(w_max, 1/N_active + margin)` and leave unplaceable capital as reported
  un-deployed reserve. Never silently over-concentrate. See section 7.1.
- **All foragers negative.** All deposits are negative and all trails decay;
  weights still normalize among the least-bad, but the drop threshold and the
  risk triggers pull capital into the cache and Scout and demote the worst. The
  header shows the negative realized number; the system does not pretend.
- **New colony cold start.** With no history, all foragers are Scouts, most
  capital sits un-deployed or in the cache, and only the exploration budget moves
  until the first promotions establish trails. **The first epoch allocates
  nothing at all**, including the exploration budget: `allocatable_pool` and the
  scout budget are both set by `finalize_settlement`, so they read zero until the
  first settlement closes. A deposit made on day one is held, accounted and
  withdrawable from the moment it lands, but it is not deployed to any forager
  until that settlement runs, one full epoch later. Promotion to Active then
  needs `promote_min_epochs` further epochs, so a colony starting from nothing
  reaches its main allocation path in roughly five epochs. This is stated
  explicitly because a depositor sees "deposited" immediately and "deployed"
  considerably later, and the gap is a property of the design rather than a
  delay in the system.
- **Determinism.** All arithmetic is fixed-point with a fixed rounding rule so
  the on-chain update and the off-chain mirror agree exactly. Pheromone ties are
  broken by forager registration order.
- **Inactive forager.** No closed trades gives `perf = 0`, deposit 0, and pure
  evaporation; prolonged inactivity is separately a non-response slash trigger in
  `risk-spec.md`.
- **Rebalancing churn.** The no-trade band and turnover cap prevent small
  pheromone changes from generating trades and bound per-epoch slippage.
