# KOLNY Allocation Specification

This is the core of KOLNY. It defines how realized performance becomes a
pheromone score, how that score decays over time, and how the colony turns
pheromone into per-forager capital weights each epoch. Every parameter here is
designed to be an on-chain config value; the parameter table in section 10 is
written so the Anchor engineer can implement it directly.

The design is a direct adaptation of Ant Colony Optimization. In Ant System
(Dorigo, Maniezzo, and Colorni, 1996) the pheromone on graph edge `xy` evolves as

```
tau_xy  <-  (1 - rho) * tau_xy  +  sum_k  delta_tau_xy^k
delta_tau_xy^k  =  Q / L_k   if ant k used edge xy in its tour,   else 0
```

where `rho` is the pheromone evaporation coefficient and `L_k` is the cost of
ant `k`'s tour, so a better (shorter) tour deposits more pheromone. KOLNY keeps
this structure exactly and reinterprets the terms:

- an edge becomes a **forager** (a trail the colony can send capital down),
- tour quality `Q / L_k` becomes **risk-adjusted realized performance**,
- **evaporation `rho`** becomes the time-decay that stops capital from chasing
  stale winners, and
- pheromone `tau` becomes the **allocation weight score**.

A note on `rho`, because the literature is inconsistent. The original 1996 Ant
System paper writes the update as `tau(t+n) = rho * tau(t) + delta_tau` and calls
`rho` the trail *persistence*, so evaporation there is `(1 - rho)`. The modern
convention used by later ACO writing and by this document is the opposite: `rho`
is the *evaporation rate* and persistence is `(1 - rho)`. KOLNY uses the modern
convention throughout: **larger `rho` means faster forgetting.** See
`references.md`.

---

## 1. Pheromone update

For each forager `f` and epoch `e`:

```
tau_f(e+1)  =  max( 0 ,  (1 - rho) * tau_f(e)  +  D_f(e) )
```

- `tau_f` is the forager's pheromone score, `tau_f >= 0`.
- `rho` in (0, 1] is the per-epoch evaporation rate (section 5).
- `D_f(e)` is the epoch's performance deposit (section 3). Unlike classic ACO,
  `D_f` may be **negative**: a losing epoch actively erodes the trail rather than
  merely failing to reinforce it. The `max(0, .)` floor keeps the score a valid
  allocation weight; a forager driven to `tau = 0` holds no trail and receives no
  main-pool capital until it earns positive deposits again (by then it is
  typically in probation or back in the Scout Sandbox, see section 6).

Everything downstream is a function of the `tau_f` vector: allocation weights are
just normalized, capped pheromone (section 7).

---

