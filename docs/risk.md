# KOLNY Risk Specification

KOLNY runs real capital through autonomous agents that trade real markets.
Agents lose money. This document defines how KOLNY bounds those losses, contains
failures so one agent cannot sink the colony, funds an insurance reserve, and
discloses the damage honestly. It pairs with `allocation.md` (which decides
how much capital each forager receives) and `security.md` (which defends the
mechanism from manipulation).

The organizing idea: allocation decides how much a forager gets; risk decides
what happens when that capital is lost. The same realized numbers drive both.

---

## 1. Forager bond and slashing

Every forager posts a `$KOLNY` bond to register. The bond is skin in the game: it
makes bad behavior and reckless loss expensive to the operator, and it is the
first external capital used to make depositors whole when a forager loses beyond
tolerance.

### 1.1 Bond sizing

Required bond scales with the capital a forager can be allocated, so that
operators managing more principal have more at stake:

```
required_bond_f  =  max( min_bond ,  bond_ratio * allocation_f )
```

`bond_ratio` (default 0.10) means a forager allocated 300,000 of base asset must
keep a bond worth at least 30,000. If pheromone would raise a forager's
allocation above what its bond supports, **the allocation is capped at
`posted_bond / bond_ratio`**, not the bond raised automatically. Skin in the game
gates capital, not the other way around.

### 1.2 The bond is volatile collateral (stated plainly)

The bond is denominated in `$KOLNY`, whose price moves, while losses are
denominated in the base asset. The bond is therefore valued at the oracle
`$KOLNY` price with a conservative haircut:

```
bond_value_f  =  (1 - haircut) * oracle_price(KOLNY) * bond_tokens_f
```

`haircut` (default 0.30) over-collateralizes against token price swings. If
`bond_value_f` falls below `required_bond_f`, the forager enters a top-up window;
failing to top up within the window freezes new allocation and, past a second
window, is treated as a rule breach. This is disclosed because it is a real
weakness: in a sharp `$KOLNY` drawdown, bonds are worth less exactly when they
may be needed. The haircut, the cache, and correlation limits mitigate it; they
do not remove it.

### 1.3 Slash conditions

A forager is slashed when any of the following fires. The runtime and the
program evaluate them from committed, realized, on-chain state.

- **Loss threshold.** The forager's current realized drawdown exceeds
  `dd_slash` (default 30 percent of its allocated capital), or a single epoch's
  realized loss exceeds `epoch_loss_limit` (default 10 percent). A milder
  drawdown at `dd_probation` (default 15 percent) triggers probation first, not a
  slash.
- **Rule violation.** Trading outside the position-limit profile (section 3):
  using leverage, touching a non-whitelisted asset or venue, exceeding
  single-asset concentration, or attempting to move capital anywhere except back
  to the Brood Vault.
- **Non-response.** The operator fails to submit performance commits or a
  liveness heartbeat for longer than `nonresponse_timeout` (default 2 epochs), or
  fails to unwind when instructed at demotion or shutdown.

### 1.4 Slash amount and split

Slashing is graduated by cause:

- Rule violation and non-response seize the **whole** bond (`slash_fraction =
  100 percent`), because these are integrity failures, not market losses.
- A loss-threshold slash seizes a fraction of the bond proportional to how far
  past the threshold the loss went, up to 100 percent.

Of the seized bond, **50 percent is burned** (`slash_burn = 5000 bps`, transferred
to an incinerator PDA that exposes no withdrawal instruction, so the amount leaves
circulation permanently) and the remaining 50 percent is routed to the Risk Cache
to offset the depositor loss. The bond is denominated in the vault's `base_mint`,
the same asset as deposits and the cache, so that the loss-absorption waterfall of
section 4.2 settles without a swap. It follows that **this burn does not reduce
`$KOLNY` supply**; the token's link to colony performance runs through the
buyback-and-burn of section 4.3 instead, where surplus is used to acquire `$KOLNY`
on the market before burning it. Probation
itself burns nothing; it only freezes new allocation during a grace window
(`probation_grace`, default 1 epoch) in which the forager must recover or be
slashed. On any slash the forager is demoted, its remaining capital is unwound
back to the vault, and it must re-enter through the Scout Sandbox to trade again.

---

## 2. Isolated sub-account model

Each forager trades from its own program-derived sub-account, and this boundary
is enforced by the program, not promised by convention:

- A forager operator can authorize trades **only** from its own sub-account. The
  sub-account PDA is derived from the forager record, and the program checks the
  signer is that forager's operator (`has_one`, seeds, bump; see `security.md`).
- A sub-account can send capital **only** to whitelisted venue programs for
  trading and **only back to the Brood Vault** for withdrawal. It cannot transfer
  to an arbitrary address. A forager cannot drain to itself.
- A forager's realized loss is bounded to its own sub-account balance plus its
  bond. It cannot reach another forager's sub-account or the un-deployed
  principal in the vault.

What isolation does and does not do, stated honestly:

- It **does** contain blast radius and control: one agent's bug, exploit, or
  blow-up cannot move or lose another agent's capital, and cannot exceed its own
  allocation weight (capped at `w_max`, default 20 percent, in
  `allocation.md`).
- It **does not** make depositors immune to aggregate performance. KOLNY is a
  pooled fund: total net asset value is the sum of all sub-accounts, so a large
  loss in one forager still lowers every depositor's share value by that
  forager's slice. Isolation caps how large that slice can be and the bond and
  cache cushion it; nothing makes a pooled fund's aggregate result immune to loss.

---

## 3. Position limits

Every forager has a position-limit profile, enforced pre-trade by
forager-runtime and bounded per-epoch on-chain:

- **Maximum allocation weight** `w_max` (default 20 percent of the main pool),
  the single-forager concentration cap from `allocation.md`.
- **Maximum leverage: zero.** Foragers trade unlevered spot only. No borrowing,
  no margin, no leveraged perpetual exposure, no collateralized loops. This is a
  hard rule, not a default, because leverage converts a bad epoch into a
  liquidation that isolation and bonds cannot fully contain.
- **Allowed assets: a whitelist with a liquidity floor.** Only assets and venues
  on the config whitelist, each meeting a minimum depth and volume threshold so
  positions can actually be unwound at rebalancing and demotion. No long-tail or
  illiquid tokens.
- **Single-asset concentration** within a sub-account is capped
  (`max_single_asset`, default 40 percent, base asset excepted) so a forager
  cannot put its whole slice into one thin market.
- **Per-epoch realized loss limit** (`epoch_loss_limit`, default 10 percent)
  which, if breached, triggers probation and pauses new trading for that forager.

---

## 4. Insurance cache (Risk Cache)

The cache is the colony's stored food: a reserve that absorbs losses beyond a
forager's own capital before they reach depositors.

### 4.1 Accrual sources

1. A cut of realized colony profit, `cache_accrual` (default 10 percent of
   realized profit), routed to the cache until the reserve target is met.
2. The cache-bound half of every slashed bond (section 1.4).
3. Optional `$KOLNY` staked into the cache by holders who choose to underwrite
   colony risk in exchange for a premium share. Stakers are paid from accrual and
   are first to absorb a shortfall among cache sources, which is disclosed to
   them plainly.

### 4.2 Loss-absorption waterfall

When a forager realizes a loss, it is absorbed in this order:

```
1. the forager's own sub-account   (its allocated capital falls first)
2. the forager's bond              (slashed portion reimburses the vault)
3. the Risk Cache                  (covers residual up to the cache balance)
4. depositor net asset value       (any remainder is borne by depositors)
```

Steps 2 and 3 are what give depositors a cushion. Step 4 is the honest limit:
**the cushion is finite.** If bonds and the cache are exhausted, the remaining
loss reduces depositor share value. KOLNY never claims otherwise.

### 4.3 Reserve target and profit routing

The cache targets a reserve ratio `cache_reserve_target` (default 4 percent of
value under colony). Below target, profit routing to the cache increases and new
risk-taking capacity is tightened. Above target, surplus can flow to `$KOLNY`
buyback-and-burn, which is the token's link to colony performance. The reserve
ratio, the cache balance, and total burned are public at all times.

### 4.4 Depletion scenario (disclosed, not hidden)

The cache is sized for idiosyncratic, uncorrelated forager failures. Its clear
failure mode is **correlated loss**: a market regime shock in which many foragers
lose at once can outrun both bonds and cache, so step 4 of the waterfall is
reached and depositors take a loss. Mitigations reduce but do not eliminate this:
the concentration cap and asset whitelist limit correlation, the zero-leverage
rule removes liquidation cascades, and the reserve target builds a buffer in good
regimes. The Trail Board must show the current reserve ratio so depositors can
see how much cushion actually remains at any moment.

---

