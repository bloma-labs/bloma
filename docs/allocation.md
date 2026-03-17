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
`risk.md` and `security.md` for the settlement-finality and oracle defenses.

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

