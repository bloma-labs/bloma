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

