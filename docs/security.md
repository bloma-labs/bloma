# KOLNY Security Model

KOLNY custodies pooled capital and lets external operators run agents against
slices of it. That makes two things attackable: the program that holds and moves
value, and the performance signal that decides where value flows. This document
covers account validation, the authority model, price and performance trust, the
upgrade policy, the known attack surface, and RPC hygiene. It builds on the
isolation and disclosure rules in `risk-spec.md`.

---

## 1. Anchor account validation

Every instruction validates every account it touches. The program never trusts a
passed-in account's contents without proving what it is.

- **PDA derivation.** The Forager Registry record, each forager sub-account, the
  Brood Vault, the Allocation Engine state, and the Risk Cache are all
  program-derived addresses from stable seeds with a stored `bump`. Each
  instruction re-derives the expected address from seeds and rejects any account
  whose key does not match. This prevents account substitution.
- **Ownership and linkage with `has_one`.** A sub-account is linked to its
  forager, a forager to its operator, and vault-owned token accounts to the vault
  authority, using `has_one` and seed constraints so a caller cannot pair a real
  account with someone else's child. For example, a trade instruction requires
  `sub_account.forager == forager.key()` and `forager.operator == signer.key()`.
- **Signer checks.** Every state change checks that the acting authority signed:
  the operator for forager actions, the depositor for share actions, the admin
  multisig for config. `Signer` is required, never inferred.
- **Token-account constraints.** Token accounts are validated for `mint`,
  `owner`/`authority`, and expected PDA derivation, so a forager cannot supply a
  token account it controls in place of the vault's.
- **Init safety.** Account creation uses explicit discriminators and guards
  against re-initialization; the program rejects double-init and unexpected
  account sizes. If `init_if_needed` is used, the existing-state branch is
  validated, not assumed.
- **Numeric safety.** All arithmetic is checked or saturating per the fixed-point
  rules in `allocation-spec.md`; config writes are range-checked (section 4) so
  out-of-range values are impossible to store, and integer overflow cannot move
  capital.

---

