# KOLNY Risk Specification

KOLNY runs real capital through autonomous agents that trade real markets.
Agents lose money. This document defines how KOLNY bounds those losses, contains
failures so one agent cannot sink the colony, funds an insurance reserve, and
discloses the damage honestly. It pairs with `allocation-spec.md` (which decides
how much capital each forager receives) and `security.md` (which defends the
mechanism from manipulation).

The organizing idea: allocation decides how much a forager gets; risk decides
what happens when that capital is lost. The same realized numbers drive both.

---

## 1. Forager bond and slashing

Every forager posts a bond, denominated in the vault's `base_mint`. The bond is
skin in the game: it makes bad behavior and reckless loss expensive to the
operator, and it is the first external capital used to make depositors whole when
a forager loses beyond tolerance.

It is not posted at registration. `register_forager` opens the record with a zero
bond and `top_up_bond` funds it afterwards, which is safe because a new forager
is a Scout and a Scout receives no main-pool capital. The bond gates the two
things that matter: promotion requires `bond >= min_bond`, and allocation
capacity is bounded by the bond thereafter (section 1.1).

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

### 1.2 The bond is discounted collateral (stated plainly)

The bond is denominated in the vault's `base_mint`, the same asset as deposits,
the Risk Cache and realized losses. That is deliberate rather than incidental:
the loss-absorption waterfall of section 4.2 has to settle without a swap, and
the program holds no DEX route. A bond posted in a separate floating token would
have to be sold under exactly the conditions that trigger a slash, which is when
selling is worst. **The program therefore reads no price oracle at all**, and the
bond carries no token-price exposure of its own.

The bond is still recognized conservatively rather than at face value:

```
bond_value_f  =  (1 - haircut) * bond_tokens_f
```

The bond sits inside the forager's own sub-account, which is the account the
operator trades from, so a loss that exhausts principal continues into the bond
itself (section 2). Allocation capacity is evaluated between settlements against
a recorded bond figure that is accurate only as of the last settlement, so
`haircut` (default 0.30, range 0.10..0.70) recognizes part of that figure rather
than all of it, and capital is not extended against collateral that may already
be partly consumed. **It is not a price haircut: the program reads no price
oracle.**

Note the direction. The haircut **tightens** the collateral requirement rather
than loosening it: with `bond_ratio` 0.10, a 0.30 haircut means a forager must
post `0.10 / (1 - 0.30)` = **14.3 percent** of its allocation, not 10, and a
posted bond of 1,000 supports 7,000 of allocation instead of 10,000.

If `bond_value_f` falls below `required_bond_f`, the forager enters a top-up
window; failing to top up within the window freezes new allocation and, past a
second window, is treated as a rule breach.

What the bond does not do, stated plainly: it bounds one forager's damage, not
the colony's. Correlated losses across many foragers drain bonds and cache
together, and section 4.2 step 4 is where depositors take whatever remains.

### 1.3 Slash conditions

A forager is slashed when any of the following fires. Every condition is
evaluated from committed, realized, on-chain state. Today that evaluation is a
call to the program's `slash_forager` instruction with a reason code; the
`risk-cache` package that would detect the condition and raise the proposal
automatically is a library with no host process yet
(`architecture.md` section 3.2). Detection is therefore manual; the enforcement,
the split and the accounting are not.

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
  `allocation-spec.md`).
- It **does not** make depositors immune to aggregate performance. KOLNY is a
  pooled fund: total net asset value is the sum of all sub-accounts, so a large
  loss in one forager still lowers every depositor's share value by that
  forager's slice. Isolation caps how large that slice can be and the bond and
  cache cushion it; nothing makes a pooled fund's aggregate result immune to loss.

---

## 3. Position limits

Every forager has a position-limit profile. The design is two-layer: checked
pre-trade by forager-runtime and bounded per-epoch on-chain. Only the second
layer is live. `forager-runtime` is an implemented, tested library with no host
process (`architecture.md` section 3.2), so **the pre-trade layer is not
currently enforcing anything**, and what actually binds today is the program's
own per-epoch bound plus the sub-account boundary of section 2. The limits below
are stated as the profile; where a limit depends on the pre-trade layer, it is
not yet enforced.

- **Maximum allocation weight** `w_max` (default 20 percent of the main pool),
  the single-forager concentration cap from `allocation-spec.md`.
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
3. Voluntary contributions in `base_mint` through the permissionless
   `fund_cache` instruction, which any address may call. Stated plainly, because
   the alternative is easy to assume: a contribution buys **no** premium, no
   claim on the cache and no redemption right. It raises `balance` and nothing
   else, and it is deliberately excluded from `total_accrued`, which counts only
   what settlement routed in from realized profit. There is no staking
   instruction and no underwriter role in the program; if either is ever added it
   will be specified here first.

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
buyback-and-burn, which is the token's link to colony performance. Stated as
plainly as the mechanism it replaces: **the buyback is not a program
instruction.** The colony program holds no DEX route and reads no price, so a
buyback is a treasury action taken outside it, and the on-chain record of it is
the resulting transfer, not a colony instruction. The reserve ratio, the cache
balance, and total burned are public at all times.

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

## 5. Disclosure: drawdown and slash history are always public

Everything a depositor needs to judge risk is computed from on-chain state by the
indexer and shown continuously, never buried:

- **Per forager:** all-time maximum drawdown, current drawdown, trailing 30-day
  realized return, full slash history, bond amount and current collateralization,
  days active, and strategy-type tag.
- **Per colony:** aggregate realized return, current aggregate drawdown, cache
  balance and reserve ratio, total bond slashed and total bond burned (in
  `base_mint`, the figure the program keeps as `total_burned`), and the count of
  foragers in each lifecycle state.

The header trust indicators show realized on-chain figures only. A backtest or a
projected return never appears in a trust indicator or a header.

---

## 6. Honesty and prohibited language (CRITICAL)

Autonomous agent trading loses money sometimes. KOLNY's credibility depends on
saying so. Copy that promises certainty KOLNY cannot deliver is not allowed
anywhere in the product: website, documentation, CLI output, embed badges, and
marketing.

### 6.1 Prohibited terms (this is a prohibition list)

The following terms are **prohibited** across all KOLNY surfaces. This subsection
is the single place they are written down, precisely so they are never written
anywhere else. Do not use, in any copy:

- the word "guaranteed" applied to returns or safety,
- the phrase "risk-free",
- the phrase "can't lose", or any equivalent claim that loss is impossible,
- any claim that an agent will reliably or automatically make money for users.

No KOLNY surface may state or imply that deposits are protected against loss, or
that the colony cannot have a losing period.

### 6.2 What KOLNY discloses instead

In place of safety claims, KOLNY publishes the truth and lets it stand:

- realized performance only, never backtests, in every headline figure,
- current and maximum drawdown for every forager and for the colony,
- the full slash and burn history,
- the insurance cache balance and reserve ratio, including when it is below
  target,
- the loss-absorption waterfall of section 4.2, so depositors know exactly where
  their protection ends and their exposure begins.

The product's honesty is a feature: the colony shows its losses and its decayed
trails in the same view as its winners, because a fund that hides drawdown is
lying, and the whole premise of KOLNY is that realized truth, not narrative,
directs capital.

---

## 7. Risk parameters (Anchor config)

| Parameter | On-chain field | Type | Default | Range | Meaning |
|---|---|---|---|---|---|
| Minimum bond | `min_bond` | u64 | project-set | > 0 | Floor bond, in `base_mint` base units |
| Bond ratio | `bond_ratio_bps` | u16 | 1000 | 500..5000 | Bond as fraction of allocation; 10% |
| Bond haircut | `bond_haircut_bps` | u16 | 3000 | 1000..7000 | Share of a posted bond not recognized as allocation capacity, because the bond sits in the traded sub-account and may already be partly consumed since the last settlement. Not a price haircut; no oracle is read. Tightens the requirement: 0.10 ratio at 30% becomes 14.3% of allocation |
| Probation drawdown | `dd_probation_bps` | u16 | 1500 | 500..4000 | Current drawdown to enter probation; 15% |
| Slash drawdown | `dd_slash_bps` | u16 | 3000 | 1000..6000 | Current drawdown to slash; 30% |
| Epoch loss limit | `epoch_loss_limit_bps` | u16 | 1000 | 300..3000 | Per-epoch realized loss to pause/probate; 10% |
| Probation grace | `probation_grace_epochs` | u8 | 1 | 1..8 | Epochs to recover before slash |
| Non-response timeout | `nonresponse_timeout_epochs` | u8 | 2 | 1..8 | Silence before non-response slash |
| Slash burn share | `slash_burn_bps` | u16 | 5000 | 0..10000 | Seized bond burned; rest to cache; 50% |
| Max leverage | `max_leverage_x` | u8 | 1 | 1..1 | Unlevered only; borrowing disallowed |
| Single-asset cap | `max_single_asset_bps` | u16 | 4000 | 1000..10000 | Max one asset in a sub-account; 40% |
| Cache accrual | `cache_accrual_bps` | u16 | 1000 | 0..5000 | Realized profit routed to cache; 10% |
| Cache reserve target | `cache_reserve_target_bps` | u16 | 400 | 100..1000 | Target cache/TVL; 4% |

`max_leverage_x` is fixed at 1 by policy; it exists as a field only to make the
zero-leverage rule explicit and auditable on-chain, not to be raised.
