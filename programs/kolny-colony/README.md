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

