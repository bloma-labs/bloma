# KOLNY Architecture

KOLNY is an autonomous colony fund on Solana. Capital is deposited once into a
central vault (the Brood), then spread across many independent AI agents
(foragers), each running its own strategy inside an isolated sub-account. The
realized performance of each forager becomes its pheromone score, and pheromone
decides how much capital flows down that trail in the next epoch. Good trails
thicken and attract more capital; weak trails fade through time decay and drain.
No human picks the winning strategy. The colony does.

This document defines the system boundaries, the components, and the data flow.
The allocation mathematics live in `allocation-spec.md`, the loss-containment
model in `risk-spec.md`, and the trust model in `security.md`.

---

## 1. Design principles

1. **On-chain is the source of truth for value and consensus state.** Custody,
   share accounting, per-forager pheromone, bonds, slashing, and each epoch's
   committed inputs live on-chain. Nothing that moves capital depends on a
   trusted off-chain database.
2. **Off-chain does computation and market execution.** Strategy execution,
   heavy math, and indexing run off-chain. Results are committed back on-chain
   before they can move capital.
3. **Realized performance only.** Allocation reacts to realized, settled,
   fee-netted results. Unrealized marks and backtests never move capital and
   never appear in the header trust indicators. See `allocation-spec.md` section 4.
4. **Failure is isolated by construction.** Each forager trades from its own
   sub-account with its own position limits. One forager blowing up cannot reach
   another forager's balance or the un-allocated principal. See `risk-spec.md`.
5. **The allocation is reproducible.** Given the on-chain inputs (per-forager
   realized performance, current pheromone, config), anyone can recompute the
   next epoch's weights and get the same answer. This is what makes the Trail
   Board auditable rather than a marketing surface.

---

## 2. On-chain and off-chain boundary

| Concern | Where | Component | Rationale |
|---|---|---|---|
| Custody of deposited capital | On-chain | Brood Vault | Value must be trust-minimized |
| Depositor share accounting | On-chain | Brood Vault | Ownership is consensus state |
| Forager records, bond escrow, status | On-chain | Forager Registry | Bonds and status gate capital |
| Isolated per-forager balances | On-chain | Sub-account PDAs | Isolation must be enforced, not promised |
| Pheromone state, epoch config | On-chain | Allocation Engine | Weights must be reproducible and tamper-evident |
| Weight computation | Hybrid | Allocation Engine | Compute-heavy; see section 5 |
| Bond slashing and burn | On-chain | Slash module | Penalties must be enforceable |
| Insurance reserve | On-chain | Risk Cache | Payout ordering is consensus state |
| Strategy execution | Off-chain | forager-runtime | Market access, latency, model inference |
| Pre-trade position-limit checks | Off-chain + on-chain | forager-runtime + program guards | Enforced off-chain per trade, bounded on-chain per epoch |
| Realized-performance aggregation | Off-chain | forager-runtime | Reads venue fills, nets fees, closes epoch |
| Pheromone and weight proposal | Off-chain | pheromone-engine | Mirrors on-chain math, proposes epoch update |
| New-agent probation | Off-chain + on-chain | scout-sandbox + Registry | Small tickets, promotion evaluation |
| Read model for the web and CLI | Off-chain | indexer API (service) | Serves Trail Board, history, decay curves |

The rule of thumb: if a bug in a component could silently move or steal capital,
that component's authority lives on-chain. Everything else is off-chain and is
treated as an untrusted proposer whose output is checked before it takes effect.

---

## 3. Component map

```mermaid
%%{init: {'theme':'base','themeVariables':{'background':'#0E0F0C','mainBkg':'#5C4B3A','primaryColor':'#5C4B3A','primaryTextColor':'#E4E0D2','primaryBorderColor':'#B6E04A','secondaryColor':'#3E5A44','secondaryTextColor':'#E4E0D2','secondaryBorderColor':'#B6E04A','tertiaryColor':'#1A1712','tertiaryTextColor':'#E4E0D2','lineColor':'#B6E04A','textColor':'#E4E0D2','nodeTextColor':'#E4E0D2','clusterBkg':'#161310','clusterBorder':'#5C4B3A','edgeLabelBackground':'#0E0F0C','fontFamily':'Public Sans, sans-serif'}}}%%
flowchart TB
  subgraph OFFCHAIN["Off-chain services"]
    direction TB
    RT["forager-runtime<br/>strategy execution + limits + perf"]
    PE["pheromone-engine<br/>evaporation + deposit + weights"]
    SS["scout-sandbox<br/>probation + promotion"]
    RC["risk-cache service<br/>drawdown watch + slash proposals"]
    IX["indexer API (service)<br/>Trail Board read model"]
  end
  subgraph ONCHAIN["Anchor program (Solana mainnet)"]
    direction TB
    REG["Forager Registry<br/>records + bond escrow + status"]
    BV["Brood Vault<br/>custody + shares"]
    AE["Allocation Engine<br/>pheromone + capped weights"]
    SUB["Forager sub-accounts<br/>isolated PDAs"]
    SL["Slash module"]
    CA["Risk Cache reserve"]
  end
  USER["Depositor wallet"] -->|"deposit base asset"| BV
  BV -->|"mint shares"| USER
  AE -->|"target weights"| BV
  BV -->|"route capital"| SUB
  SUB --> RT
  RT -->|"epoch realized perf commit"| AE
  AE -->|"updated pheromone"| AE
  RT -->|"limit breach / loss"| RC
  RC -->|"slash trigger"| SL
  SL -->|"burn + cover"| CA
  CA -->|"cover shortfall"| BV
  REG --- SUB
  SS -->|"promotion"| REG
  PE -.->|"mirror + propose"| AE
  ONCHAIN --> IX
  IX -->|"weights, realized perf, drawdown, decay"| WEB["web (Trail Board)"]
  IX --> CLI["kolny-cli / Agent SDK"]

  classDef onc fill:#5C4B3A,stroke:#B6E04A,stroke-width:1px,color:#E4E0D2;
  classDef off fill:#3E5A44,stroke:#B6E04A,stroke-width:1px,color:#E4E0D2;
  classDef val fill:#0E0F0C,stroke:#D08A2C,stroke-width:1px,color:#E4E0D2;
  class REG,BV,AE,SUB,SL,CA onc;
  class RT,PE,SS,RC,IX off;
  class USER,WEB,CLI val;
```

### 3.1 On-chain: the Anchor program

- **Forager Registry** holds one record per forager: operator authority, bond
  amount and escrow, lifecycle status (Scout, Active, Probation, Slashed,
  Exited), the address of its isolated sub-account, and its position-limit
  profile. Registration requires a `$KOLNY` bond. See `risk-spec.md` section 1.
- **Brood Vault** is the single entry point for depositors. A deposit of the
  base asset mints vault shares proportional to net asset value. The vault holds
  un-deployed base asset and is the accounting root; forager sub-accounts are
  children of it. Withdrawal burns shares against the vault's realized value.
- **Forager sub-accounts** are per-forager PDAs that hold that forager's
  allocated slice. A forager operator can direct trades only from its own
  sub-account and only within its position-limit profile. This PDA boundary is
  the isolation guarantee.
- **Allocation Engine** stores each forager's pheromone value, the epoch config,
  and the last committed realized-performance inputs. At each epoch boundary it
  applies evaporation and the performance deposit, then produces capped target
  weights. See section 5 for where the arithmetic runs.
- **Slash module** executes bond slashing when a trigger fires (loss threshold,
  rule violation, or non-response) and routes the slashed bond between burn and
  the Risk Cache. See `risk-spec.md` section 1.
- **Risk Cache** is the insurance reserve. It accrues from a cut of realized
  colony profit and from slashed bonds, and it is drawn first when a covered
  shortfall occurs. See `risk-spec.md` section 4.
- The program **IDL is published** to the public repository so any
  client, the CLI, and the Agent SDK build against a pinned interface.

### 3.2 Off-chain: the services

- **forager-runtime** hosts each forager's strategy against its isolated
  sub-account. It enforces position limits before every trade, records fills,
  nets fees and slippage and funding, and at each epoch close it submits a
  signed realized-performance commit to the Allocation Engine. It is the
  boundary between "an agent wants to trade" and "the sub-account is allowed to".
- **pheromone-engine** is the reference implementation of the allocation math.
  It reads on-chain committed inputs and config, recomputes pheromone and
  weights in the same fixed-point arithmetic the program uses, and proposes the
  epoch update. It is an untrusted proposer: the program verifies its output.
- **scout-sandbox** manages probationary foragers. New foragers receive small,
  fixed scout tickets funded from the exploration budget and are evaluated for
  promotion once they clear the minimum-epochs, minimum-trades, and
  performance bars. See `allocation-spec.md` section 6.
- **risk-cache service** watches per-forager drawdown and rule compliance,
  raises slash proposals, and manages cache accrual and reserve-ratio targets.
- **indexer API** (the `service` app) reads chain state and
  serves the read model consumed by the web Trail Board, the
  `kolny-cli`, and the Agent SDK: current weights, per-forager realized
  performance, drawdown, pheromone decay curves, slash history, and epoch
  history.

---

## 4. Data flow: one epoch

```mermaid
%%{init: {'theme':'base','themeVariables':{'background':'#0E0F0C','primaryColor':'#5C4B3A','primaryTextColor':'#E4E0D2','primaryBorderColor':'#B6E04A','lineColor':'#B6E04A','textColor':'#E4E0D2','actorBkg':'#5C4B3A','actorBorder':'#B6E04A','actorTextColor':'#E4E0D2','signalColor':'#B6E04A','signalTextColor':'#E4E0D2','labelBoxBkgColor':'#3E5A44','labelTextColor':'#E4E0D2','noteBkgColor':'#161310','noteTextColor':'#E4E0D2','activationBkgColor':'#3E5A44','sequenceNumberColor':'#0E0F0C','fontFamily':'Public Sans, sans-serif'}}}%%
sequenceDiagram
  autonumber
  participant D as Depositor
  participant BV as Brood Vault
  participant AE as Allocation Engine
  participant SUB as Forager sub-accounts
  participant RT as forager-runtime
  participant IX as indexer API
  D->>BV: Deposit base asset
  BV-->>D: Mint shares
  Note over AE: Epoch boundary
  AE->>AE: Evaporate pheromone, apply deposit, cap weights
  AE->>BV: Commit target weights
  BV->>SUB: Route capital (net deltas, no-trade band)
  loop During epoch
    SUB->>RT: Forager executes within position limits
    RT->>RT: Record fills, net fees and slippage
  end
  RT->>AE: Signed realized-performance commit (closed trades only)
  AE->>AE: Verify inputs and invariants
  Note over AE: Next epoch boundary uses this deposit
  AE-->>IX: State readable
  IX-->>D: Trail Board: weights, realized perf, drawdown, decay
```

The loop that gives the system its name: realized performance from epoch `e`
becomes the pheromone deposit that shapes weights for epoch `e+1`. Capital flows
toward the trails that have been paying off recently, and away from trails that
have gone quiet or turned negative, because the evaporation term erodes every
trail continuously while only realized results replenish it.

---

## 5. Where the allocation arithmetic runs

This is the central architecture trade-off. The weight computation involves a
bounded non-linear deposit and an iterative cap-and-redistribute step (see
`allocation-spec.md` sections 3 and 5). Solana programs run under a compute-unit
budget and have no floating point, so the options are:

- **Option A, fully on-chain.** The program stores pheromone in fixed-point and
  does evaporation, deposit, and capping on-chain. Fully trust-minimized, but
  the bounded deposit transform and the water-filling cap loop must be done in
  integer fixed-point, and cost grows with the forager count.
- **Option B, off-chain compute with on-chain commit.** The pheromone-engine
  computes weights and the program stores them. Cheapest, but the program would
  be trusting an off-chain number to move capital, which violates principle 1.
- **Option C, off-chain proposer with on-chain verifier (recommended).** The
  realized-performance inputs are committed on-chain by forager-runtime. The
  program itself performs the pheromone update (evaporation plus a fixed-point,
  bounds-checked deposit) and the capping, because those are cheap per forager
  and the forager count is in the tens, not thousands. The pheromone-engine runs
  the identical math off-chain only as a mirror and to drive the Trail Board;
  its numbers never move capital on their own.

Recommendation: **Option C.** Keep the state transition on-chain where it is
cheap and consensus-critical, compute the expensive non-linear deposit factor in
a way the program can cheaply re-check (submit the deposit alongside the realized
input; the program re-derives it with a fixed-point approximation and rejects it
if it is out of the `[-Q, +Q]` bound or the wrong sign), and treat the off-chain
engine as a proposer and mirror, never as an authority. The capping loop is
bounded by the active-forager count, which the config keeps small enough to fit
the compute budget; if the colony grows past that, the epoch update is chunked
across instructions. This is an implementation detail the anchor-program
engineer owns; the invariant they must preserve is that **no off-chain value
moves capital without an on-chain check of the same value.**

---

## 6. Forager lifecycle

```mermaid
%%{init: {'theme':'base','themeVariables':{'background':'#0E0F0C','primaryColor':'#5C4B3A','primaryTextColor':'#E4E0D2','primaryBorderColor':'#B6E04A','lineColor':'#B6E04A','textColor':'#E4E0D2','nodeTextColor':'#E4E0D2','fontFamily':'Public Sans, sans-serif'}}}%%
stateDiagram-v2
  [*] --> Scout: register + post bond
  Scout --> Active: promotion bar cleared
  Scout --> Exited: probation failed
  Active --> Probation: drawdown or rule breach
  Probation --> Active: recovered within grace
  Probation --> Slashed: threshold breached
  Active --> Slashed: hard breach
  Slashed --> Exited: bond slashed, capital withdrawn
  Active --> Exited: voluntary unwind
  Exited --> [*]
```

- **Scout** foragers trade only scout tickets from the exploration budget. Their
  pheromone is seeded from scout-phase realized performance at promotion.
- **Active** foragers receive pheromone-weighted allocation from the main pool.
- **Probation** freezes new allocation and starts a grace window during which
  the forager must recover or be slashed.
- **Slashed / Exited** withdraws capital back to the vault and settles the bond.

Full trigger thresholds and bond mechanics are in `risk-spec.md`.

---

## 8. Implementation note: Anchor version

The concept material references "Anchor 0.31". As of 2026-08 that is two major
versions behind: the latest published stable `anchor-lang` is **1.1.2**, and
**2.0.0-rc.1** is in release-candidate (verified against the crates.io registry
and the GitHub releases page; see `references.md`). The program should pin an
explicit, current version in `Anchor.toml` rather than 0.31, and the header
trust indicator string should be updated to match whatever is pinned. Pinning is
mandatory regardless of which version is chosen, so that the published IDL, the
SDK, and the CLI all build against one interface.
