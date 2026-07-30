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

## 2. Authority model

Three roles, each with a strictly bounded set of actions. The design goal is that
no single role can both value capital and redirect it to itself.

| Action | Admin (multisig) | Forager operator | Depositor |
|---|---|---|---|
| Set config within ranges | Yes | No | No |
| Manage asset/venue whitelist | Yes | No | No |
| Emergency pause | Yes | No | No |
| Register forager, post bond | No | Yes (self) | No |
| Trade from own sub-account within limits | No | Yes (self) | No |
| Submit realized-performance commit | No | Yes (self) | No |
| Request unwind of own sub-account | No | Yes (self) | No |
| Deposit and mint shares | No | No | Yes (self) |
| Withdraw and burn own shares | No | No | Yes (self) |
| Move another account's capital | No | No | No |
| Mint shares without deposit | No | No | No |
| Send capital to an arbitrary address | No | No | No |

Key boundaries:

- **Admin** tunes the system and can pause in an emergency, but cannot touch
  individual depositor balances or sub-account capital, cannot mint shares, and
  cannot redirect capital anywhere. Admin actions that change economics are
  bounded to the config ranges and should sit behind a timelock (section 4).
- **Forager operators** act only on their own sub-account and only within their
  position-limit profile. They cannot change their own bond requirement,
  self-promote out of the Scout Sandbox, or reach depositor funds.
- **Depositors** move only their own shares. They have no say in allocation,
  which is the point: the colony allocates, not the depositor.

---

## 3. Price and performance trust

Two numbers can be attacked: the prices used to value assets and bonds, and the
realized performance used to update pheromone.

### 3.1 Oracle discipline

- Prices for asset marking, base-asset valuation, and `$KOLNY` bond valuation
  come from a robust oracle (for example Pyth or Switchboard), never a single-DEX
  spot price, which is cheap to manipulate.
- Every oracle read enforces a **staleness check** (reject prices older than a
  small slot bound) and a **confidence check** (reject when the oracle's
  confidence interval is too wide), plus sanity bounds. A trade or valuation that
  depends on a stale or low-confidence price is refused rather than guessed.

### 3.2 Realized performance is reconciled, not self-reported

The pheromone deposit uses realized, closed-trade performance
(`allocation-spec.md` sections 2 and 4). The attack is to fake that number. The
core defense is that performance is **derived from the sub-account's on-chain
balance, not taken on the operator's word**:

- The committed realized return `r_f` for an epoch must reconcile with the net
  base-asset change of that forager's sub-account over the epoch, adjusted for
  authorized deposits and withdrawals. If the sub-account did not actually gain
  the value, the claim is rejected.
- Only fills on whitelisted venues with real liquidity count as trades. Internal
  transfers between accounts an operator controls are not trades and produce no
  realized performance.
- The bounded `tanh` deposit caps the pheromone any single epoch can add, so even
  a partially successful manipulation yields limited allocation gain, while the
  bond remains at risk if the manufactured position later loses.

---

## 4. Upgrade authority and config safety

- **Upgrade authority** for the program is held by a multisig and should sit
  behind a timelock, so an upgrade is visible before it takes effect. The holder
  and the policy are published. Upgradability is itself a centralization risk and
  is treated as one: the honest position is that users trust the multisig and
  timelock during the program's early life, with a stated path toward tightening
  or handing off that authority. The IDL is published and versioned in the
  public repository so clients build against a known interface.
- **Config safety.** Every config parameter is range-checked on-chain against the
  bounds in `allocation-spec.md` section 11 and `risk-spec.md` section 7. This
  matters because a malicious or fat-fingered admin change is a real threat: an
  evaporation rate of zero would freeze trails forever, a max weight of 100
  percent would let one forager take everything. Because the program enforces the
  ranges, **even the admin cannot store an out-of-range value**, which bounds the
  blast radius of a compromised admin key. Economic parameter changes should also
  pass through the timelock so depositors can react.

---

## 5. Known attack surface

### 5.1 Pheromone grinding (manufacturing performance)

An operator trades with itself or a colluding counterparty to manufacture
realized profit and attract more allocation.

- **Wash trading loses money.** Round-tripping between accounts the operator
  controls pays trading fees and spread each cycle, so it cannot manufacture net
  realized profit; it destroys value. Manufacturing a real gain requires an
  external counterparty to actually pay the operator, which is expensive and
  self-limiting.
- **Reconciliation** (section 3.2) means only genuine base-asset inflow to the
  sub-account counts, so fabricated fills that do not change the balance are
  ignored.
- **Whitelist** confines trading to venues with real order books and pools;
  internal transfers are not counted.
- **Off-chain detection** flags round-trip and counterparty patterns for admin
  review and possible slash.
- **Bounded deposit and caps** limit the allocation any grinding can earn, and
  the **bond** is slashed if the manufactured exposure later loses.

### 5.2 Sandwich and MEV on rebalancing

Epoch rebalancing trades are somewhat predictable and can be front-run.

- Rebalances carry **slippage limits** and are **split and jittered** across a
  window rather than fired as one large order at a known slot.
- Execution prefers **aggregators, RFQ, or TWAP** over a single market sweep, and
  **private relays or Jito bundles** where available to reduce MEV exposure.
- The **no-trade band** and **turnover cap** (`allocation-spec.md` section 8)
  shrink the size and frequency of moves, which is itself the strongest MEV
  reducer.

### 5.3 Epoch-boundary timing

Actors may time actions around the epoch close to flatter performance or to jump
a known reallocation.

- **Settlement cutoff.** Only trades settled strictly before the close slot
  count, with a small buffer that excludes last-slot ambiguous fills, so an
  operator cannot stuff a favorable print into the boundary.
- **Anti-sniping on deposits.** Reallocation targets are computed from pheromone
  committed before the close, so a late depositor cannot front-run a reallocation
  they can already see; a short redemption cooldown (or a fee on an immediate
  deposit-withdraw round trip) removes the costless timing play.
- **Close-slot unpredictability.** The exact close slot floats within a window,
  and performance inputs can be committed under commit-reveal, so the precise
  boundary is not a fixed, gameable target.

### 5.4 Bond token price manipulation

Manipulating the `$KOLNY` oracle to over-value a bond (to under-collateralize) or
under-value it (to force a top-up) is mitigated by the robust oracle discipline
of section 3.1, the 30 percent bond haircut, and the top-up windows in
`risk-spec.md` section 1.2.

---

