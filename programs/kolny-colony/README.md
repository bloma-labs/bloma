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

