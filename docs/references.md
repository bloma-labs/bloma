# KOLNY References

Sources behind KOLNY's design, with a note on how each one maps into the system.
URLs marked `[verified]` were fetched during research on 2026-08-15 and returned
real content. DOIs are given as canonical identifiers; where a publisher's page
is subscription-gated, an accessible alternative (an institutional repository, an
author-hosted PDF, arXiv, or an accessible overview) is provided and marked
`[verified]`. Items that could not be verified as freely accessible are stated as
such rather than linked to an unchecked URL.

---

## A. Stigmergy and Ant Colony Optimization

These are the source of the pheromone mechanism in `allocation-spec.md`.

**Grasse, P.-P. (1959).** "La reconstruction du nid et les coordinations
interindividuelles chez Bellicositermes natalensis et Cubitermes sp. La theorie
de la stigmergie: Essai d'interpretation du comportement des termites
constructeurs." *Insectes Sociaux*, 6(1), 41-80. doi:10.1007/BF02223791.
Publisher page is subscription-gated (verified: it redirects to an authentication
wall). Accessible overview: https://en.wikipedia.org/wiki/Stigmergy `[verified]`.
- Maps to: the whole colony metaphor. Stigmergy is coordination through marks
  left in a shared environment. KOLNY's shared environment is on-chain realized
  performance, and the mark is the pheromone score that steers the next epoch's
  capital. No agent coordinates with another directly; they coordinate through
  the trail.

**Dorigo, M. (1992).** "Optimization, Learning and Natural Algorithms" (in
Italian). PhD thesis, Dipartimento di Elettronica, Politecnico di Milano.
Could not verify a freely accessible copy (print, Italian-language); the
peer-reviewed English formulation is the 1996 paper below, which is the one
KOLNY's formula is drawn from.
- Maps to: the origin of Ant Colony Optimization and the idea that reinforcement
  plus evaporation on a shared medium produces good global allocation without a
  central planner.

**Dorigo, M., Maniezzo, V., and Colorni, A. (1996).** "Ant System: Optimization
by a Colony of Cooperating Agents." *IEEE Transactions on Systems, Man, and
Cybernetics, Part B*, 26(1), 29-41. doi:10.1109/3477.484436. Author-community
PDF: https://jmvidal.cse.sc.edu/library/dorigo96a.pdf `[verified, PDF]`.
- Maps to: the exact update `tau <- (1 - rho) tau + sum delta_tau` that
  `allocation-spec.md` adapts, and the evaporation coefficient `rho`. Note this
  original paper writes the update with `rho` as trail *persistence* (evaporation
  is `1 - rho`); KOLNY follows the later convention where `rho` is the evaporation
  rate, and says so explicitly in `allocation-spec.md`.

**Dorigo, M., and Gambardella, L. M. (1997).** "Ant Colony System: A Cooperative
Learning Approach to the Traveling Salesman Problem." *IEEE Transactions on
Evolutionary Computation*, 1(1), 53-66. doi:10.1109/4235.585892. Author-hosted
PDF:
https://iridia.ulb.ac.be/~mdorigo/Published_papers/All_Dorigo_papers/DorGam1997tec.pdf
`[verified, PDF]`.
- Maps to: the local and global pheromone updates of ACS, which use the modern
  `(1 - rho)` evaporation with an explicit decay parameter. This is the
  convention `allocation-spec.md` uses, and the two-tier update loosely parallels
  KOLNY's Scout-versus-main-pool split.

**Ant colony optimization algorithms (accessible overview).**
https://en.wikipedia.org/wiki/Ant_colony_optimization_algorithms `[verified]`.
- Maps to: a plain statement of the modern-convention formula
  `tau_xy <- (1 - rho) tau_xy + sum_k delta_tau_xy^k` with `rho` named the
  pheromone evaporation coefficient and `delta_tau_xy^k = Q / L_k`. Used to
  confirm the exact form cited in `allocation-spec.md`.

---

## B. Multi-armed bandits: exploration, exploitation, non-stationarity

These justify the exploration budget and the time decay in `allocation-spec.md`
sections 5, 6, and 9.

**Auer, P., Cesa-Bianchi, N., and Fischer, P. (2002).** "Finite-time Analysis of
the Multiarmed Bandit Problem." *Machine Learning*, 47, 235-256.
doi:10.1023/A:1013689704352. Accessible institutional record:
https://pure.unileoben.ac.at/en/publications/finite-time-analysis-of-the-multiarmed-bandit-problem/
`[verified]`.
- Maps to: UCB1, the canonical exploration-exploitation policy and the baseline
  KOLNY reasons against. UCB1 assumes each arm's reward distribution is
  stationary, so it will keep exploiting an arm that was good long ago. That
  failure is exactly why KOLNY adds evaporation.

**Garivier, A., and Moulines, E. (2011).** "On Upper-Confidence Bound Policies
for Non-Stationary Bandit Problems." *Proceedings of Algorithmic Learning Theory
(ALT 2011)*; preprint arXiv:0805.3415 (2008).
https://arxiv.org/abs/0805.3415 `[verified]`. Implementation reference
(Discounted-UCB and Sliding-Window-UCB):
https://smpybandits.github.io/NonStationaryBandits.html `[verified]`.
- Maps to: the formal backbone of time decay. Discounted-UCB weights a reward
  seen `k` steps ago by `gamma^k`. KOLNY's evaporation makes an old deposit worth
  `(1 - rho)^k` after `k` epochs, so `(1 - rho)` is the discount factor `gamma`.
  Time decay is the standard remedy for non-stationarity, not a cosmetic choice.

**Thompson, W. R. (1933).** "On the Likelihood That One Unknown Probability
Exceeds Another in View of the Evidence of Two Samples." *Biometrika*, 25(3-4),
285-294. doi:10.1093/biomet/25.3-4.285. Publisher record (abstract accessible,
full text gated):
https://academic.oup.com/biomet/article-abstract/25/3-4/285/200862 `[verified]`.
- Maps to: Thompson Sampling, the alternative of allocating in proportion to each
  forager's probability of being the best, drawn from a posterior over returns.
  Recorded in `allocation-spec.md` section 9 as a candidate future allocation
  mode.

**Chapelle, O., and Li, L. (2011).** "An Empirical Evaluation of Thompson
Sampling." *Advances in Neural Information Processing Systems 24 (NeurIPS 2011)*.
Author PDF:
https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/thompson.pdf
`[verified, PDF]`.
- Maps to: the empirical case that Thompson Sampling is competitive with, and
  often better than, UCB in practice. This is why Thompson Sampling is worth
  keeping as a future option rather than dismissing.

---

## C. Solana AI agents and on-chain tooling (2026 landscape)

These support the gap thesis in the concept and fix the Anchor version.

**SendAI, Solana Agent Kit.** https://github.com/sendaifun/solana-agent-kit
`[verified]`.
- Finding: 60-plus on-chain actions across five plugins (token, NFT, DeFi, misc,
  blinks) for building agents that execute Solana operations. The README covers
  execution primitives; it contains no standard for allocating capital across
  many agents or for verifying and comparing agents' realized trading performance
  on-chain.
- Maps to: direct evidence for KOLNY's premise. Tooling to let an agent *act*
  exists and is mature; a protocol standard for performance-weighted capital
  *allocation* across a colony of agents, with bonding and slashing, is what is
  missing and what KOLNY provides.

**Alchemy (2026).** "How to Build a Solana AI Agent in 2026."
https://www.alchemy.com/blog/how-to-build-solana-ai-agents-in-2026 `[verified]`.
- Finding: a current survey of the agent-building stack (Solana Agent Kit,
  ElizaOS, GOAT, Rig). All are oriented at building and executing individual
  agents. The article does not describe a standard for cross-agent capital
  allocation or for on-chain verification of realized performance.
- Maps to: confirms the 2026 landscape is execution-first, leaving the
  allocation-and-verification layer that KOLNY targets unoccupied. Prior "agentic
  fund" narratives (for example ai16z on ElizaOS) exist but are single funds, not
  an open, performance-weighted allocation protocol.

**Anchor releases (Solana Foundation).**
https://github.com/solana-foundation/anchor/releases `[verified]`.
- Finding: the latest published stable Anchor release is 1.1.2 (June 2026).
- Maps to: the version note in `architecture.md` section 8 and `security.md`
  section 4. The concept material's "Anchor 0.31" is two major versions behind;
  the program should pin a current version.

**crates.io registry, `anchor-lang`.**
https://crates.io/api/v1/crates/anchor-lang `[verified]`.
- Finding: `max_stable_version` is 1.1.2 and the newest published version is
  2.0.0-rc.1 (release candidate, 2026-08). This is the authoritative registry
  confirmation of the release-page finding above.
- Maps to: same version note. Pin an explicit, current version in `Anchor.toml`;
  decide between the latest stable and the release candidate deliberately.

---

