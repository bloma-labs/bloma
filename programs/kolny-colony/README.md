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

## Loss absorption

```
1. the forager's own sub-account
2. the forager's bond
3. the Risk Cache
4. depositor net asset value
```

Steps 2 and 3 are the cushion. Step 4 is the honest limit: the cushion is
finite, and once bonds and cache are exhausted the remaining loss reduces
depositor share value.

Bond, cache, principal and losses are all denominated in the **same base
asset**. A bond in a different token could not cover a base-asset loss without
a swap, and this program has no DEX, so the waterfall would break.

The burn share of a slash is transferred to a locked incinerator PDA that has
no withdrawal instruction. That is an economic burn. If the base mint is not
the project token, it does **not** reduce project-token supply, and no code or
copy here claims that it does.

## Fixed-point arithmetic contract

This section is normative for every other implementation of the allocation
logic. The on-chain result is the one that moves capital, so a mirror that
disagrees is wrong by definition, and the web UI would be showing a projected
allocation that the chain will not honor.

### Representation

| Quantity | Unit | Type | Notes |
|---|---|---|---|
| Rates, weights, ratios | basis points, 1e-4 | `u16` | `BPS_DENOM = 10_000` |
| Signed rates (`r`, `perf`) | basis points | `i64` | Losses are negative |
| `promote_perf_bar_bps` | basis points | `i32` | Signed, may be negative |
| Pheromone `tau`, deposit `D`, `Q` | FP6, 1e-6 | `u64` / `i64` | `tau = 1.0` is `1_000_000` |
| `tanh` argument and result | FP6 | `i64` | Result in `(-1e6, +1e6)` |
| Token amounts | base-asset atoms | `u64` | Never scaled |
| Share amounts | atoms | `u128` | Virtual offset `1e6` |
| Accumulators, intermediates | -- | `u128` | Every ratio widens before multiplying |

There is **no floating point anywhere**. `f64` on-chain is non-deterministic
across validators; a mirror that uses floats will drift from the chain.

### Division order

1. **Widen, then multiply, then divide.** Every ratio is computed in `u128`
   (`i128` when signed) and multiplies before it divides. `pool * tau / K`,
   never `(tau / K) * pool`.
2. **Truncate toward zero** (plain Rust integer division) everywhere except the
   one case below. Residues are dust and stay in the vault.
3. **Never form a shared divisor.** There is no `K` to round, in either
   direction. One rounded divisor is shared by every uncapped forager, so any
   rounding of it biases all of their targets at once: rounding down
   over-allocates the pool outright, and rounding up still disagrees with the
   exact answer by up to 12 atoms in the near-infeasible regime. Divide once per
   forager, as `pool * tau * remaining_bps / (BPS * rest_sum)`.
   `level_form_differs_from_rounding_a_shared_divisor` pins a case where the two
   part company, so a mirror that rounds a divisor is known to diverge rather
   than assumed to match.
4. **Never double-round a weight.** Capital is `pool * tau / K` clamped by
   `pool * w_max / BPS`. Do not compute a weight in bps and then apply it.
5. Deposits round shares **down**; withdrawals round assets **down**. Both
   favor the vault.

Because of (2), per-forager targets **do not sum to exactly the pool**. A
residue of up to one atom per forager stays behind as un-deployed reserve. A
mirror that distributes the remainder to make the total land exactly on the
pool will disagree with the chain. Do not "fix" the dust.

### tanh

`tanh` is **not** a library call and **not** a lookup table. It is integer
range reduction plus an odd Taylor series, which is what makes it reproducible
bit-for-bit in any language with 128-bit integers.

```text
input  x, FP6 signed          output  tanh(x), FP6 signed, |result| <= 1e6 - 1
working scale W = 1e12        (i128)

1. if x == 0 return 0; remember the sign and work on |x|.
2. clamp |x| to 8 * 1e6.       tanh(8) differs from 1 by 2.3e-7, below FP6.
3. u = |x| * 1e6               promote FP6 -> FP12
   k = 0;  while u >= W/4 { u /= 2; k += 1 }        k <= 5
4. u2  = u*u / W
   acc = (-17 * W) / 315
   acc = (2 * W) / 15  + acc*u2 / W
   acc = -(W / 3)      + acc*u2 / W
   acc = W             + acc*u2 / W
   t   = u * acc / W                    tanh(u) through the 7th order
5. repeat k times:  t = (2 * t * W * W) / (W*W + t*t)     double angle
6. r = (t + 500000) / 1000000           FP12 -> FP6, round half up
   if r > 999999 { r = 999999 }         keeps |D| < Q strictly
7. reapply the sign.
```

Every division truncates toward zero, which Rust `i128 /`, TypeScript `BigInt /`
and Python `int()` division on positives all do. The widest intermediate is
step 5's numerator at about 2e36, inside `i128`. Worst-case error against the
real `tanh` is under 1e-6 across the whole domain.

**A 33-entry lookup table with linear interpolation was implemented first and
then removed, on measurement.** No such table reproduces the deposit column of
this project's own worked example at the three decimals that document prints,
at any domain:

| Table domain | Step | Worst error | Reproduces the worked example |
|---|---|---|---|
| `[0, 1.5]` | 0.0469 | 1.8e-4 | X |
| `[0, 2]` | 0.0625 | 2.4e-4 | X |
| `[0, 3]` | 0.0938 | 8.4e-4 | X |
| `[0, 4]` | 0.1250 | 1.4e-3 | X |
| `[0, 8]` | 0.2500 | 3.8e-3 | X |
| range reduction plus series | -- | < 1e-6 | O |

The error was not cosmetic. Over `[0, 4]` the table missed four of the ten
published deposit values, and compounded across three epochs it moved a
forager's capital by enough to send basis points of the pool to the wrong
forager. `tanh_reproduces_the_specification_deposit_table` pins all ten values
so the table cannot come back by accident.

A mirror that calls its language's `Math.tanh` will differ from the chain in
the low digits. Transcribe the steps above.

### Pipeline, in order

```
realized = (vault_balance - bond) - principal          i64, bond excluded
r_bps    = realized * 10_000 / principal               i64, 0 if principal == 0
perf_bps = r_bps - risk_aversion_bps * dd_bps / 10_000 i64
x_fp6    = perf_bps * 1_000_000 / perf_norm_s_bps      i64
D        = tanh_fp6(x_fp6) * Q / 1_000_000             i64, signed
tau'     = max(0, tau * (10_000 - rho_bps) / 10_000 + D)  clamped to ceiling
```

### Solving the water-filling level

The capped weight vector `w_f = min(w_max, tau_f / K)` summing to 1 is what the
specification's "trim, redistribute the excess, repeat" loop converges to. It is
solved directly, but **`K` is never formed as a quotient**: rounding a single
shared divisor biases every uncapped target at once, and the scalar form is only
an equivalent description of the answer.

The state stored is a **level**: `capped_count`, `remaining_bps` and `rest_sum`.

```text
remaining_bps = BPS - capped_count * w_max     share left for the uncapped
rest_sum      = total pheromone of the uncapped
```

A trail is capped exactly when this division-free integer test holds, walking
the pheromone ranking once from the largest:

```text
remaining_bps * tau  >=  w_max * rest_sum
```

Targets then follow with one division each:

```text
capped:    pool * w_max / BPS
uncapped:  pool * tau * remaining_bps / (BPS * rest_sum)
```

No rounding enters the cap decision, and there is no iteration whose stopping
condition two implementations could disagree about. `rest_sum == 0` means the
capped set holds the entire pheromone sum, so every live trail is at the cap and
the balance of the pool stays as un-deployed reserve; a trail with `tau == 0`
still receives nothing.

An earlier version did form `K` as a quotient. Truncating it downward inflated
every uncapped target simultaneously, and where the capped set nearly filled the
pool the targets summed to **more than the pool existed** -- 900,049 against a
900,000 pool in the regression case now pinned by
`allocation_never_exceeds_the_pool`. That regime is reachable in production,
because the cap relaxation for a three-forager colony lands in it. The level form
removes the failure rather than papering over it with a rounding direction.

### Why 21 tracked trails is always enough

At most `floor(BPS / w_max)` foragers can sit at the cap at once, since each
capped one takes `w_max` and the weights sum to 1. The board therefore needs
`floor(BPS / w_max) + 1` slots: the capped ones, plus one more to confirm the
next trail is genuinely uncapped.

The bound comes from the **smallest cap the configuration gate will ever
accept**, `MIN_W_MAX_BPS = 500`, giving `floor(10000 / 500) = 20` capped and
`TOP_TRAILS_LEN = 21`. Two things keep it true as `w_max` moves:

- `update_config` rejects any `w_max_bps` below `MIN_W_MAX_BPS`, so an
  authority cannot shrink the cap and overflow the board.
- `effective_max_weight_bps` only ever **raises** the cap (it relaxes the cap
  when a small colony cannot fill the pool under it) and never lowers it, so
  the effective cap is always at least `w_max_bps`, and the capped set is
  always at most 20.

`solve_alloc_divisor` additionally bounds its own scan at
`(BPS_DENOM - 1) / w_max`, so the denominator stays positive and the loop
cannot run past the slots that exist. A unit test pins
`TOP_TRAILS_LEN >= 10000 / MIN_W_MAX_BPS + 1`, so lowering the floor without
enlarging the board fails the build's test run rather than truncating the
capped set silently at runtime.

## Build

Toolchain this was built and verified against:

```
anchor-cli             0.31.1
solana-cli / agave     3.0.0
platform-tools         BPF rustc 1.89.0
host rustc / cargo     1.95.0
```

```bash
cd packages/anchor-program

# Pure logic tests. No validator, no network.
cargo test

# Compile the program and generate the IDL. Does not contact any cluster.
anchor build

ls -la target/deploy/kolny_colony.so
ls -la target/idl/kolny_colony.json
```

`anchor-lang` and `anchor-spl` are pinned to `0.31.1` to match the CLI exactly;
a CLI/lib version split is what produces IDL `TypeNotFound` failures.
`[profile.release] overflow-checks = true` is required by Anchor 0.31 and is
also a correctness control, since arithmetic here is financial.

