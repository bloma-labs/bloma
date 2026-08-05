# KOLNY Colony Program

The on-chain core of KOLNY: an Anchor program that allocates depositor capital
across **foragers** -- operator-run agents that trade from isolated sub-accounts
-- in proportion to a decaying trail score built from realized, on-chain
performance.

## What this program does and does not do

**Does:** accounting, allocation, epoch settlement, and loss containment.

**Does not:** trade, or price open positions. There is no price oracle in the
allocation core and no venue integration. Trading happens outside this program,
in each forager's own sub-account.

That boundary is the design, not a limitation. Performance is measured only as
base-asset value that actually returned to a forager's sub-account, so every
figure the colony publishes is realized rather than marked. Foragers never
submit their own performance numbers: an operator-signed PnL would be a trusted
oracle, and gaming it would be trivial. Every input to a trail is read from
chain state.

The honest limit of that choice is stated plainly: an operator can delay
settling a losing position to postpone recognizing the loss. No mechanism here
pretends otherwise. Three things bound it -- time decay evaporates the trail of
anyone who stops settling, the bond is slashable, and a forager can be retired
and its sub-account swept.

Agent trading loses money sometimes. Deposits are not protected against loss,
and the loss-absorption waterfall below states exactly where the cushion ends.

## Accounts

| Account | Seeds | LEN | space (`8 + LEN`) |
|---|---|---|---|
| `ColonyConfig` | `[b"colony"]` | 304 | 312 |
| `BroodVaultState` | `[b"brood"]` | 128 | 136 |
| `RiskCacheState` | `[b"cache"]` | 104 | 112 |
| `TrailBoard` | `[b"trail_board"]` | 176 | 184 |
| `ForagerState` | `[b"forager", operator, forager_id]` | 224 | 232 |
| `DepositorPosition` | `[b"position", depositor]` | 56 | 64 |
| `RedemptionRequest` | `[b"redeem", depositor, request_id]` | 80 | 88 |

Every `LEN` is asserted against the real Borsh-serialized byte count by a unit
test, because an undersized account fails at runtime rather than at build time.

## PDA seeds

Numeric seed components are **little-endian**. On-chain, the indexer and the
front end must derive identically or every address silently diverges.

| PDA | Seeds |
|---|---|
| `colony_config` | `[b"colony"]` |
| `brood_vault_state` | `[b"brood"]` |
| `risk_cache_state` | `[b"cache"]` |
| `trail_board` | `[b"trail_board"]` |
| `forager_state` | `[b"forager", operator.key(), forager_id.to_le_bytes()]` |
| `forager_vault` (token) | `[b"forager_vault", forager_state.key()]` |
| `vault_base` (token) | `[b"brood_vault"]` |
| `cache_vault` (token) | `[b"cache_vault"]` |
| `incinerator_vault` (token) | `[b"incinerator"]` |
| `depositor_position` | `[b"position", depositor.key()]` |
| `redemption_request` | `[b"redeem", depositor.key(), request_id.to_le_bytes()]` |

TypeScript:

```ts
const idBuf = Buffer.alloc(8);
idBuf.writeBigUInt64LE(BigInt(foragerId));
const [foragerState] = PublicKey.findProgramAddressSync(
  [Buffer.from("forager"), operator.toBuffer(), idBuf],
  PROGRAM_ID,
);
```

Python:

```python
struct.pack("<Q", forager_id)
```

The four token accounts (`forager_vault`, `vault_base`, `cache_vault`,
`incinerator_vault`) are SPL Token or Token-2022 accounts owned by their parent
state PDA. `incinerator_vault` has no withdrawal instruction anywhere in the
program, which is what makes a transfer into it economically irreversible.

## Instructions

### Colony configuration

| Instruction | Access | Notes |
|---|---|---|
| `initialize_colony(params)` | signer becomes authority | Every parameter range-checked |
| `update_config(patch)` | authority | Per-field `Option`; each checked against the same bounds |
| `propose_authority(new)` | authority | Two-step transfer, step 1 |
| `accept_authority()` | pending authority | Two-step transfer, step 2 |
| `set_paused(bool)` | authority | Blocks deposits, rebalancing and scout funding |

### One-time account creation

Split into small instructions on purpose: a context carrying several `init`
constraints generates enough stack frame to exceed the 4096-byte limit. A front
end bundles them into a single transaction, so the user experience is unchanged.

`initialize_brood`, `open_vault_base`, `initialize_risk_cache`,
`open_cache_vault`, `open_incinerator_vault`, `initialize_trail_board`.

### Forager lifecycle

| Instruction | Access | Notes |
|---|---|---|
| `register_forager(id, strategy_meta)` | operator | Record only; starts as Scout with zero bond |
| `open_forager_vault(id)` | operator | Sub-account; derives from the record, so it comes second |
| `top_up_bond(id, amount)` | operator | Posts the bond, in base asset |
| `promote_forager(id)` | permissionless | Criteria are read from chain state, so anyone can crank it |
| `retire_forager(id)` | operator or authority | Returns bond, sweeps residual base to the vault |
| `heartbeat(id)` | operator | Liveness, for the non-response slash condition |

### Depositor flows

| Instruction | Access | Notes |
|---|---|---|
| `deposit(assets)` | anyone | Shares round down, favoring existing holders |
| `withdraw(shares)` | holder | Only within idle liquidity; otherwise fails |
| `request_redemption(shares)` | holder | Queue used when idle liquidity is short |
| `fulfill_redemption(id)` | permissionless | Partial payouts allowed |
| `fund_cache(amount)` | anyone | Donate to the insurance cache |

### Epoch settlement

| Instruction | Access | Notes |
|---|---|---|
| `begin_settlement()` | permissionless | Only after `epoch_end_ts` |
| `settle_forager(id)` | permissionless | Idempotent per epoch; takes no performance arguments |
| `finalize_settlement()` | permissionless | Requires every forager settled |
| `rebalance_forager(id)` | permissionless | Moves capital toward target |

### Exploration and risk

| Instruction | Access | Notes |
|---|---|---|
| `fund_scout(id)` | permissionless | One fixed ticket per scout per epoch |
| `slash_forager(id, reason)` | authority | Graduated by cause |

## How allocation works

### 1. Pheromone

Each epoch, for every forager:

```
tau' = max(0, (1 - rho) * tau + D)
D    = Q * tanh(perf / s)
perf = r - lambda * DD
```

`r` is the realized net return over the epoch and `DD` the realized drawdown,
both computed on-chain. The deposit is **bounded** by `Q`, so no single epoch
-- lucky tail or manipulation attempt -- can dominate a trail, and
**sign-preserving**, so a loss erodes a trail faster than passive evaporation
rather than merely failing to reinforce it. `tanh` is evaluated by integer range
reduction plus an odd Taylor series, accurate to under 1e-6 and reproducible
bit-for-bit off-chain; see the fixed-point contract below for the exact steps.

The floor is 0 by design: a dead trail has to be able to die. Propping every
forager up at a minimum weight would fight the entire mechanism.

### 2. The measurement that must not be skipped

A forager's sub-account holds **bond + principal**. Settlement therefore
computes:

```
realized = (vault_balance - bond) - principal
```

Without the bond subtraction, an operator could top up its own bond and have it
read as trading profit, inflating its trail and pulling colony capital toward
it. There are dedicated unit tests asserting that a pure bond top-up produces a
realized result of exactly zero.

### 3. Weights: bounded water-filling

Weights are normalize, then cap, then drop, then renormalize. The capped vector

```
w_f = min(w_max, tau_f / K)      with   sum(w_f) = 1
```

has exactly one solution `K`, and that single scalar reproduces the iterative
"cap, redistribute the excess, repeat" procedure exactly. Solving for `K` is
what makes each forager's target computable in O(1), which is what makes the
settlement crank possible at all.

At most `floor(1 / w_max)` foragers can be capped simultaneously, so tracking
the largest 21 trails during the crank is enough to solve `K` exactly rather
than approximately. That is what `TrailBoard` is for.

A forager whose weight falls below `w_drop` is demoted to Scout, and its
pheromone is excluded from the next epoch's normalization sum, which
redistributes the freed weight among the survivors automatically.

### 4. Settlement is a three-phase crank

N foragers cannot be looped in one transaction, so:

```
Open  --begin_settlement-->  Settling  --settle_forager (once per forager)-->
      --finalize_settlement (requires settled_count == settleable_forager_count)-->  Open
```

Registration and retirement are frozen during `Settling`, which keeps the
population stable and removes a race class. Ordering cannot be manipulated:
each forager settles independently and the accumulator is a sum. Early
finalization is blocked by the count guard.

Two populations are tracked separately and must not be conflated:
`settleable_forager_count` (everyone who must be settled) and
`active_forager_count` (only those normalized into weights). Demoting a forager
mid-crank changes the second, never the first, so the completion test stays
sound.

## NAV, and what it does not include

```
nav = idle_base + outstanding_principal
```

Both are **accounting counters**. A live token balance is never read into NAV.
That is the root defense against the donation inflation attack: tokens sent
directly to a vault account are simply uncredited and cannot move the share
price. A virtual share offset and a minimum deposit close the first-depositor
rounding attack on top of that. Rounding always favors the vault.

Unrealized losses are not in NAV, so share price can be overstated while a
forager holds an unclosed losing position. This is disclosed rather than
papered over, and it is why withdrawals are limited to idle liquidity and why
the redemption queue exists. The program cannot force-liquidate deployed
capital, so it does not promise immediate liquidity.

