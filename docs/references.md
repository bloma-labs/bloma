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

These are the source of the pheromone mechanism in `allocation.md`.

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
  `allocation.md` adapts, and the evaporation coefficient `rho`. Note this
  original paper writes the update with `rho` as trail *persistence* (evaporation
  is `1 - rho`); KOLNY follows the later convention where `rho` is the evaporation
  rate, and says so explicitly in `allocation.md`.

**Dorigo, M., and Gambardella, L. M. (1997).** "Ant Colony System: A Cooperative
Learning Approach to the Traveling Salesman Problem." *IEEE Transactions on
Evolutionary Computation*, 1(1), 53-66. doi:10.1109/4235.585892. Author-hosted
PDF:
https://iridia.ulb.ac.be/~mdorigo/Published_papers/All_Dorigo_papers/DorGam1997tec.pdf
`[verified, PDF]`.
- Maps to: the local and global pheromone updates of ACS, which use the modern
  `(1 - rho)` evaporation with an explicit decay parameter. This is the
  convention `allocation.md` uses, and the two-tier update loosely parallels
  KOLNY's Scout-versus-main-pool split.

**Ant colony optimization algorithms (accessible overview).**
https://en.wikipedia.org/wiki/Ant_colony_optimization_algorithms `[verified]`.
- Maps to: a plain statement of the modern-convention formula
  `tau_xy <- (1 - rho) tau_xy + sum_k delta_tau_xy^k` with `rho` named the
  pheromone evaporation coefficient and `delta_tau_xy^k = Q / L_k`. Used to
  confirm the exact form cited in `allocation.md`.

---

