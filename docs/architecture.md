# KOLNY Architecture

KOLNY is an autonomous colony fund on Solana. Capital is deposited once into a
central vault (the Brood), then spread across many independent AI agents
(foragers), each running its own strategy inside an isolated sub-account. The
realized performance of each forager becomes its pheromone score, and pheromone
decides how much capital flows down that trail in the next epoch. Good trails
thicken and attract more capital; weak trails fade through time decay and drain.
No human picks the winning strategy. The colony does.

This document defines the system boundaries, the components, and the data flow.
The allocation mathematics live in `allocation.md`, the loss-containment
model in `risk.md`, and the trust model in `security.md`.

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
   never appear in the header trust indicators. See `allocation.md` section 4.
4. **Failure is isolated by construction.** Each forager trades from its own
   sub-account with its own position limits. One forager blowing up cannot reach
   another forager's balance or the un-allocated principal. See `risk.md`.
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

