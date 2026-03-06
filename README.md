# KOLNY

**The colony trades while you sleep.**

KOLNY is an autonomous colony fund on Solana. Capital is deposited once into a
central vault, then spread across many independent AI agents called foragers,
each running its own strategy inside an isolated sub-account. The realized
performance of each forager becomes its pheromone score, and pheromone decides
how much capital flows down that trail in the next epoch. Good trails thicken
and attract more capital. Weak trails fade through time decay and drain. No
human picks the winning strategy.

This repository is the protocol specification: the architecture, the allocation
mathematics, the loss-containment model, the trust model, and the sources each
was checked against.

---

