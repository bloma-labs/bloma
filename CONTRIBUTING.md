# Contributing to KOLNY

This repository holds the KOLNY protocol specification: the architecture, the
allocation mathematics, the loss-containment model, the trust model, and the
sources those documents were checked against. The on-chain program, the Agent
SDK, and the CLI are published here as they reach a pinned interface.

Specification work is the highest-value contribution right now. A flaw in the
allocation math or the loss waterfall is cheaper to fix in `docs/` than after a
program is deployed.

## What we are looking for

- **Errors in the allocation math.** Section 3 of `docs/allocation-spec.md`
  defines evaporation, the bounded performance deposit, and the cap-and-
  redistribute step. If a parameter range admits a degenerate weight vector, or
  the fixed-point rounding drifts against the reference, that is a bug.
- **Gaps in loss containment.** `docs/risk-spec.md` defines the sub-account
  isolation boundary, slash triggers, and the loss-absorption waterfall. An
  unhandled path from one forager's failure to another forager's balance is the
  most serious class of report.
- **Trust-model holes.** `docs/security.md` lists the guarantees the system
  claims. A claim that does not survive an adversarial reading should be
  narrowed, not defended.
- **Corrections to cited facts.** `docs/references.md` records the external
  sources and the date each was verified. Version numbers and registry facts go
  stale; a correction with a source link is welcome.

## Ground rules

### Honesty is enforced, not encouraged

Section 6 of `docs/risk-spec.md` is a prohibition list: language that promises
certainty the protocol cannot deliver is not permitted anywhere in this
repository, the website, the CLI output, or any other surface. Autonomous agents
lose money. Any contribution whose wording implies otherwise will be rejected on
that basis alone, regardless of technical merit.

The continuous-integration workflow enforces this mechanically. See
`.github/workflows/ci.yml`.

### Claims must match the tree

If a change adds a claim about what this repository contains or what the
protocol does, the corresponding artifact must exist in the same change. A
README that advertises a package the tree does not contain is treated as a
defect with the same severity as a broken build.

### Style

- All documentation and code comments are in English.
- No emoji anywhere, including commit messages. Use `O` / `X` or `PASS` / `FAIL`.
- Commit messages are plain English sentences. Do not use Conventional Commits
  prefixes (`feat:`, `fix:`, `chore:`, and so on) or any other colon prefix.
- Wrap prose at 80 columns.

## Reporting a problem

Open an issue describing:

1. Which document or file, and which section.
2. What the text claims.
3. Why that is wrong, with a concrete counterexample where the claim is about
   the mathematics or the risk model.
4. A source link, where the claim is about an external fact.

For a suspected flaw that would let capital move incorrectly, say so plainly in
the issue title. Those are triaged first.

## Proposing a change

1. Fork and branch from `main`.
2. Make the change. Keep one concern per commit.
3. Run the local gate before opening a pull request:

```bash
bash .github/scripts/check.sh
```

That script runs the same checks as the continuous-integration workflow:
repository structure, document substance, prohibited language, emoji, and
cross-reference resolution. It must print `PASS` for every check.

4. Open a pull request describing what changed and why. If the change touches
   the allocation math or the risk model, state which invariant it preserves.

## Security

Do not open a public issue for a vulnerability in a deployed program. There is
no deployed program at the time of writing; when there is, this section will
carry a disclosure address and a response window.

## License

Contributions are accepted under the MIT License. See `LICENSE`.
