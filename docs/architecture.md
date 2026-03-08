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

