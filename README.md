# KOLNY

**The colony trades while you sleep.**

<p align="center">
  <a href="https://kolny.fi"><img src="https://img.shields.io/badge/site-kolny.fi-B6E04A?style=flat-square" alt="Site"></a>
  <a href="https://api.kolny.fi/docs"><img src="https://img.shields.io/badge/api-kolny.fi-3E5A44?style=flat-square" alt="API"></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/license-MIT-E4E0D2?style=flat-square" alt="License MIT"></a>
  <a href="./docs"><img src="https://img.shields.io/badge/specification-5%20documents-B6E04A?style=flat-square" alt="Specification"></a>
  <a href="#status"><img src="https://img.shields.io/badge/stage-pre--deployment-D08A2C?style=flat-square" alt="Stage"></a>
  <a href="#status"><img src="https://img.shields.io/badge/program-not%20deployed-B4372E?style=flat-square" alt="Program status"></a>
  <a href="https://solana.com"><img src="https://img.shields.io/badge/chain-solana-B6E04A?style=flat-square&logo=solana&logoColor=0E0F0C" alt="Solana"></a>
  <a href="https://www.anchor-lang.com/"><img src="https://img.shields.io/badge/runtime-anchor-5C4B3A?style=flat-square" alt="Anchor"></a>
  <a href="https://github.com/kolny"><img src="https://img.shields.io/badge/org-kolny-3E5A44?style=flat-square&logo=github" alt="GitHub organization"></a>
  <a href="./.github/workflows/ci.yml"><img src="https://img.shields.io/badge/ci-specification%20gate-B6E04A?style=flat-square" alt="CI"></a>
  <a href="./docs/references.md"><img src="https://img.shields.io/badge/citations-13%20verified-6E6A62?style=flat-square" alt="Citations"></a>
</p>

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

## Status

Read this section before anything else.

- **There is no deployed program.** No mainnet address, no devnet address, no
  audit. Nothing here moves capital today, and `Anchor.toml` pins
  `cluster = "Localnet"` so that stays true by construction.
- **The program builds and its tests pass.** `anchor build` succeeds,
  `cargo test` reports 70 passing, and the generated interface in
  [`idl/`](./idl/kolny_colony.json) covers 28 instructions, 7 account types,
  26 events and 33 error variants. That is a statement about a build, not about
  a deployment and not about an audit.
- **[CRITICAL] The program ID is still a placeholder.** `declare_id!` in
  `programs/kolny-colony/src/lib.rs`, `Anchor.toml` and the `address` field of
  the published IDL all read
  `Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS`, which is the Anchor template
  default and not a real deployment. At deploy time it is replaced by the real
  program keypair. **Every PDA in this program derives from the program ID, so
  addresses you derive against the published IDL today will not match the
  deployed program.** Do not hard-code them. See
  [`scripts/set-program-id.sh`](./scripts/set-program-id.sh) for the swap and
  the four things that must be redone after it.
- **The Agent SDK and the CLI are not in this tree.** They are described in
  `docs/architecture.md` and are published here once their interface settles,
  not before. If you came looking for them, their absence is the current state
  rather than an oversight.
- **The web application and the read API are running.**
  [kolny.fi](https://kolny.fi) and [api.kolny.fi](https://api.kolny.fi/docs)
  respond. Because no program is deployed, nothing they show is derived from
  on-chain colony state yet.
- **The numbers in `docs/allocation-spec.md` section 10 are a worked example**
  chosen to illustrate the mechanics, not a backtest and not a projection. Its
  parameters are deliberately not the production defaults.

If a section of this README ever describes something the tree does not contain,
that is a defect. Open an issue.

---

## How allocation works

The mechanism is an adaptation of the Ant System pheromone update
([Dorigo, Maniezzo and Colorni, 1996](https://jmvidal.cse.sc.edu/library/dorigo96a.pdf)),
with one change that matters: in ant colony optimization a deposit is never
negative, because a path cannot be worse than not walking it. In a fund it can.
A losing epoch actively erodes a trail rather than merely failing to reinforce
it.

```mermaid
%%{init: {'theme':'base','themeVariables':{'background':'#0E0F0C','mainBkg':'#5C4B3A','primaryColor':'#5C4B3A','primaryTextColor':'#E4E0D2','primaryBorderColor':'#B6E04A','secondaryColor':'#3E5A44','secondaryTextColor':'#E4E0D2','secondaryBorderColor':'#B6E04A','tertiaryColor':'#1A1712','tertiaryTextColor':'#E4E0D2','lineColor':'#B6E04A','textColor':'#E4E0D2','nodeTextColor':'#E4E0D2','clusterBkg':'#161310','clusterBorder':'#5C4B3A','edgeLabelBackground':'#0E0F0C','fontFamily':'monospace'}}}%%
flowchart LR
  DEP["Depositor"] -->|"deposit, mint shares"| BV["Brood Vault<br/>custody and shares"]
  BV -->|"routed by weight"| SUB["Forager sub-accounts<br/>isolated PDAs"]
  SUB --> RUN["Strategy execution<br/>inside position limits"]
  RUN -->|"closed and settled trades only"| PERF["Realized performance<br/>return minus drawdown penalty"]
  PERF -->|"bounded deposit"| PH["Pheromone<br/>evaporate, then deposit"]
  PH -->|"normalize, cap, drop"| W["Target weights"]
  W --> BV
  RUN -->|"limit breach or loss"| SL["Slash<br/>burn and cover"]
  SL --> RC["Risk Cache<br/>insurance reserve"]
  RC -->|"absorb shortfall"| BV

  classDef onchain fill:#5C4B3A,stroke:#B6E04A,stroke-width:1px,color:#E4E0D2;
  classDef signal fill:#3E5A44,stroke:#B6E04A,stroke-width:1px,color:#E4E0D2;
  classDef loss fill:#0E0F0C,stroke:#B4372E,stroke-width:1px,color:#E4E0D2;
  class BV,SUB,W onchain;
  class RUN,PERF,PH signal;
  class SL,RC loss;
```

Each epoch, every forager's trail evaporates and is then topped up by a bounded
function of what it actually earned:

```
tau_f(e+1)  =  max( 0 ,  (1 - rho) * tau_f(e)  +  D_f(e) )

D_f(e)      =  Q * tanh( perf_f(e) / s )
perf_f(e)   =  r_f(e)  -  lambda * DD_f(e)
```

`r_f` is the realized net return over the epoch, after fees, slippage and
funding. `DD_f` is the realized maximum drawdown inside that epoch, and
`lambda` prices it. `rho` is the evaporation rate; at the default `rho = 0.16`
on a weekly epoch, a trail that stops earning is half forgotten in about four
weeks and effectively gone in three months.

`tanh` is what keeps a single lucky epoch from taking over the colony: the
deposit is bounded to the open interval `(-Q, +Q)` no matter how large the
return, so influence accrues over many epochs or not at all.

Pheromone is then turned into capital:

```
1.  raw_f  = tau_f / sum_g tau_g            over Active foragers
2.  cap:     any w_f above w_max is clipped to w_max and the excess is
             redistributed among the un-capped foragers, repeated until
             no un-capped forager exceeds w_max
3.  drop:    any post-cap w_f below w_drop is set to 0; that forager is
             demoted to Scout and its capital returns to the vault
4.  final w_f are the target weights, summing to 1
```

`w_drop` is a demotion threshold, not a floor. A trail that fades is removed
from the main pool rather than propped up at a minimum weight.

A fixed `scout_budget` of deployable value, 10 percent by default, never enters
this competition. It funds small, fixed-size scout tickets for new and
probationary foragers, which is how a new agent gets a first allocation despite
starting at zero pheromone. Promotion requires a minimum number of epochs, a
minimum number of realized closed trades, and a performance bar.

Full derivation, the parameter table with on-chain field names and ranges, the
rejected alternatives, and a three-epoch worked example are in
[`docs/allocation-spec.md`](./docs/allocation-spec.md).

---

## Loss containment

Each forager trades from its own PDA sub-account and can withdraw only back to
the Brood Vault, never to an arbitrary address. Leverage is fixed at zero by
an on-chain parameter, assets are restricted to a whitelist with a liquidity
floor, and a single forager is capped at `w_max` of the main pool.

When a forager loses money, the loss is absorbed in this order:

```
1. the forager's own sub-account   (its allocated capital falls first)
2. the forager's bond              (slashed portion reimburses the vault)
3. the Risk Cache                  (covers residual up to the cache balance)
4. depositor net asset value       (any remainder is borne by depositors)
```

Steps 2 and 3 are the depositor cushion. Step 4 is where it ends. The cushion
is finite, and correlated losses across many foragers at once are the failure
mode that reaches step 4 fastest. The reserve ratio is published so the size of
the remaining cushion is always visible.

Bonds are posted in `$KOLNY` and carry a 30 percent haircut because the
collateral is volatile. The honest weakness, stated in the specification rather
than hidden: if the token falls hard, bonds are worth least at exactly the
moment they are needed most.

Details, slash triggers and the full parameter table are in
[`docs/risk-spec.md`](./docs/risk-spec.md).

---

## What this repository contains

```
programs/kolny-colony/
  src/lib.rs           program entrypoint and instruction surface
  src/state.rs         ColonyConfig, BroodVaultState, RiskCacheState, TrailBoard,
                       ForagerState
  src/math.rs          fixed-point evaporation, deposit transform, weight capping
  src/instructions/    admin, vault, forager, settlement, risk
  src/events.rs        26 events; src/errors.rs 33 error variants
  README.md            account table, PDA seeds, instruction reference
idl/
  kolny_colony.json    generated interface, 28 instructions
scripts/
  set-program-id.sh    placeholder-to-real program ID swap, wired into nothing
tests/
  kolny-colony.ts      integration tests, require a local validator
docs/
  architecture.md      system boundaries, components, epoch data flow
  allocation-spec.md   pheromone update, deposit transform, weights, parameters
  risk-spec.md         bonds, slashing, isolation, insurance cache, disclosure
  security.md          account validation, authority model, attack surface
  references.md        every external source, with verification dates
.github/
  workflows/ci.yml     the gate
  scripts/check.sh     the same gate, runnable locally
  scripts/gate.py      prohibited language, non-text characters, cross-references
```

The repository root is an Anchor workspace, so `anchor build` works on a clone
without further setup.

---

## Honesty

Autonomous agent trading loses money. A protocol that allocates capital to
agents inherits that fact, and KOLNY's credibility depends on saying so rather
than working around it.

Section 6 of [`docs/risk-spec.md`](./docs/risk-spec.md) is a prohibition list.
It names the specific phrases that may not appear on any KOLNY surface, and it
is deliberately the only place in the project where those phrases are written
down. The continuous-integration gate parses that section and fails the build if
any of those terms appears anywhere else in the tree, so the rule is enforced by
the repository rather than by good intentions.

What KOLNY publishes instead of safety claims:

- Realized performance only, in every headline figure. Never backtests, never
  marks on open positions, never projections.
- Current and maximum drawdown for every forager and for the colony, at the
  same size and in the same place as the returns.
- The full slash and burn history. A slashed forager stays in the list with its
  status, it is not deleted.
- Decayed trails, kept visible. A map showing only the routes that worked is a
  lie about the terrain.
- The insurance cache balance and reserve ratio, including when it is below
  target.
- Capital that could not be deployed, reported as an un-deployed reserve rather
  than quietly over-concentrated.

Allocation is not a forecast. It is a description of where value has already
been realized, decayed by how long ago it happened.

---

## Reading the specification

```bash
git clone https://github.com/kolny/kolny.git
cd kolny

# start here
less docs/architecture.md

# run the same gate CI runs
bash .github/scripts/check.sh
```

Suggested order: `docs/architecture.md` for the boundaries and the epoch loop,
then `docs/allocation-spec.md` for the mathematics, then `docs/risk-spec.md`
for what happens when it goes wrong, then `docs/security.md` for the trust
model, and `docs/references.md` when you want to check a claim against its
source.

---

## Verification

The gate that runs on every push and pull request checks that:

- every required document and source file is present and none has been reduced
  to a stub,
- the program ID agrees across `src/lib.rs`, `Anchor.toml` and the published
  IDL, because every PDA derives from it and a drift there fails silently
  rather than loudly,
- `Anchor.toml` still pins `cluster = "Localnet"` and no deploy command is
  wired into any workflow or npm script,
- no prohibited term appears outside the canonical prohibition list,
- no emoji or box-drawing character appears in any document,
- every relative cross-reference resolves to a file that exists,
- every external URL in the tree still resolves.

Run it locally with `bash .github/scripts/check.sh`. There is no deployment
workflow in this repository, and nothing here can send a transaction to a
cluster as a side effect of a build, a test or a workflow.

---

## References

The specification cites its sources rather than asserting from memory, and
[`docs/references.md`](./docs/references.md) records what each source
establishes and the date it was checked. The load-bearing ones:

- The pheromone update comes from the Ant System paper,
  [Dorigo, Maniezzo and Colorni, 1996](https://jmvidal.cse.sc.edu/library/dorigo96a.pdf).
- Time decay is the standard remedy for non-stationary reward, not a cosmetic
  choice. See [Garivier and Moulines on discounted upper-confidence bounds](https://arxiv.org/abs/0805.3415),
  where a reward `k` steps old is weighted `gamma^k`, exactly the role
  `(1 - rho)^k` plays here.
- The gap this protocol targets is visible in the current Solana agent
  tooling: [Solana Agent Kit](https://github.com/sendaifun/solana-agent-kit)
  gives agents a rich set of on-chain actions, but nothing standardizes
  allocating capital across many agents by verified realized performance.
- The Anchor version question is live. `docs/architecture.md` section 8 records
  what the [published releases](https://github.com/solana-foundation/anchor/releases)
  showed at the time of writing and why the program must pin an explicit
  version rather than inherit one.

---

## Contributing

Specification review is the most useful contribution while there is no deployed
program: an error in the allocation math or the loss waterfall is far cheaper to
fix here than after capital is at stake. See
[`CONTRIBUTING.md`](./CONTRIBUTING.md) for what to look for and the ground
rules.

---

## License

MIT. See [`LICENSE`](./LICENSE).
