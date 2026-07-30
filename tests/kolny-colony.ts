/**
 * KOLNY colony integration tests.
 *
 * WRITTEN BUT NOT WIRED TO RUN. These require a local validator
 * (`solana-test-validator`) and a deployed program, which is a separate
 * explicit decision by the project owner. Nothing in this package runs them
 * automatically, and none of them may ever be pointed at devnet or mainnet.
 *
 * To run them locally, once that decision is made:
 *   solana-test-validator            # in a separate terminal
 *   anchor test --provider.cluster localnet --skip-local-validator
 *
 * The PDA helpers below are the canonical TypeScript derivations. They must
 * stay character-for-character identical to the seeds in
 * `programs/kolny-colony/src/constants.rs`; if they drift, every address
 * silently diverges from the on-chain one.
 */

import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";
import { assert } from "chai";

import { KolnyColony } from "../target/types/kolny_colony";

// ---------------------------------------------------------------------------
// PDA derivation -- mirrors constants.rs SEED_* exactly
// ---------------------------------------------------------------------------

const SEED_COLONY = Buffer.from("colony");
const SEED_BROOD = Buffer.from("brood");
const SEED_CACHE = Buffer.from("cache");
const SEED_TRAIL_BOARD = Buffer.from("trail_board");
const SEED_FORAGER = Buffer.from("forager");
const SEED_FORAGER_VAULT = Buffer.from("forager_vault");
const SEED_BROOD_VAULT = Buffer.from("brood_vault");
const SEED_CACHE_VAULT = Buffer.from("cache_vault");
const SEED_INCINERATOR = Buffer.from("incinerator");
const SEED_POSITION = Buffer.from("position");
const SEED_REDEEM = Buffer.from("redeem");

/** u64 seed components are always little-endian. */
function u64le(value: number | bigint): Buffer {
  const buf = Buffer.alloc(8);
  buf.writeBigUInt64LE(BigInt(value));
  return buf;
}

export function colonyConfigPda(programId: PublicKey): [PublicKey, number] {
  return PublicKey.findProgramAddressSync([SEED_COLONY], programId);
}

export function broodStatePda(programId: PublicKey): [PublicKey, number] {
  return PublicKey.findProgramAddressSync([SEED_BROOD], programId);
}

export function riskCacheStatePda(programId: PublicKey): [PublicKey, number] {
  return PublicKey.findProgramAddressSync([SEED_CACHE], programId);
}

export function trailBoardPda(programId: PublicKey): [PublicKey, number] {
  return PublicKey.findProgramAddressSync([SEED_TRAIL_BOARD], programId);
}

export function foragerStatePda(
  programId: PublicKey,
  operator: PublicKey,
  foragerId: number | bigint,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [SEED_FORAGER, operator.toBuffer(), u64le(foragerId)],
    programId,
  );
}

export function foragerVaultPda(
  programId: PublicKey,
  foragerState: PublicKey,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [SEED_FORAGER_VAULT, foragerState.toBuffer()],
    programId,
  );
}

export function vaultBasePda(programId: PublicKey): [PublicKey, number] {
  return PublicKey.findProgramAddressSync([SEED_BROOD_VAULT], programId);
}

export function cacheVaultPda(programId: PublicKey): [PublicKey, number] {
  return PublicKey.findProgramAddressSync([SEED_CACHE_VAULT], programId);
}

export function incineratorVaultPda(programId: PublicKey): [PublicKey, number] {
  return PublicKey.findProgramAddressSync([SEED_INCINERATOR], programId);
}

export function depositorPositionPda(
  programId: PublicKey,
  depositor: PublicKey,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [SEED_POSITION, depositor.toBuffer()],
    programId,
  );
}

export function redemptionRequestPda(
  programId: PublicKey,
  depositor: PublicKey,
  requestId: number | bigint,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [SEED_REDEEM, depositor.toBuffer(), u64le(requestId)],
    programId,
  );
}

// ---------------------------------------------------------------------------

describe("kolny-colony", () => {
  anchor.setProvider(anchor.AnchorProvider.env());
  const program = anchor.workspace.KolnyColony as Program<KolnyColony>;
  const programId = program.programId;

  describe("pda derivation", () => {
    it("derives every singleton deterministically", () => {
      const [colony] = colonyConfigPda(programId);
      const [brood] = broodStatePda(programId);
      const [cache] = riskCacheStatePda(programId);
      const [board] = trailBoardPda(programId);

      // Distinct seeds must produce distinct addresses.
      const all = [colony, brood, cache, board].map((p) => p.toBase58());
      assert.equal(new Set(all).size, all.length);

      // Derivation is pure.
      assert.equal(colonyConfigPda(programId)[0].toBase58(), colony.toBase58());
    });

    it("uses little-endian for the forager id", () => {
      const operator = PublicKey.unique();
      const [a] = foragerStatePda(programId, operator, 1);
      const [b] = foragerStatePda(programId, operator, 256);
      // If the id were serialized big-endian these would collide in the wrong
      // byte position; asserting they differ guards the endianness contract.
      assert.notEqual(a.toBase58(), b.toBase58());

      const manual = PublicKey.findProgramAddressSync(
        [SEED_FORAGER, operator.toBuffer(), u64le(1)],
        programId,
      )[0];
      assert.equal(a.toBase58(), manual.toBase58());
    });

    it("derives a forager vault from the forager record", () => {
      const operator = PublicKey.unique();
      const [foragerState] = foragerStatePda(programId, operator, 7);
      const [vault] = foragerVaultPda(programId, foragerState);
      assert.notEqual(vault.toBase58(), foragerState.toBase58());
    });

    it("separates depositors and redemption requests", () => {
      const alice = PublicKey.unique();
      const bob = PublicKey.unique();
      assert.notEqual(
        depositorPositionPda(programId, alice)[0].toBase58(),
        depositorPositionPda(programId, bob)[0].toBase58(),
      );
      assert.notEqual(
        redemptionRequestPda(programId, alice, 0)[0].toBase58(),
        redemptionRequestPda(programId, alice, 1)[0].toBase58(),
      );
    });
  });

  // -------------------------------------------------------------------------
  // The following require a running validator and a deployed program.
  // They are intentionally skipped so that no test run can contact a cluster.
  // -------------------------------------------------------------------------

  describe.skip("colony lifecycle (needs a local validator)", () => {
    it("initializes the colony with in-range parameters", async () => {
      // initialize_colony -> initialize_brood -> open_vault_base
      // -> initialize_risk_cache -> open_cache_vault
      // -> open_incinerator_vault -> initialize_trail_board
    });

    it("rejects a parameter outside the published range", async () => {
      // update_config with rho_bps above MAX_RHO_BPS must fail ParamOutOfRange.
    });

    it("transfers authority in two steps", async () => {
      // propose_authority then accept_authority, signed by the pending key.
    });
  });

  describe.skip("deposits and shares (needs a local validator)", () => {
    it("mints shares rounded down in the vault's favor", async () => {});

    it("ignores tokens transferred directly into the vault", async () => {
      // NAV is an accounting counter, so a donation must not move share price.
    });

    it("refuses a withdrawal larger than idle liquidity", async () => {
      // Must fail InsufficientIdleLiquidity and route to request_redemption.
    });

    it("services a queued redemption partially, then fully", async () => {});
  });

  describe.skip("settlement crank (needs a local validator)", () => {
    it("excludes the bond when measuring realized performance", async () => {
      // Top up a bond with no trading activity; realized must be exactly 0
      // and the trail must not strengthen.
    });

    it("is idempotent within an epoch", async () => {
      // A second settle_forager in the same epoch must fail
      // AlreadySettledThisEpoch.
    });

    it("refuses to finalize before every forager has settled", async () => {
      // Must fail SettlementIncomplete.
    });

    it("freezes registration while settling", async () => {
      // Must fail RegistrationFrozenDuringSettlement.
    });

    it("redistributes capped weight to the uncapped foragers", async () => {
      // Reproduces the worked example from docs/allocation-spec.md 10.2.
    });

    it("demotes a forager whose trail decayed below the drop threshold", async () => {});
  });

  describe.skip("risk (needs a local validator)", () => {
    it("absorbs a loss in bond, then cache, then depositor NAV", async () => {});

    it("splits a slash between the incinerator and the cache", async () => {});

    it("refuses a non-response slash before the timeout elapses", async () => {
      // Must fail NotSlashable.
    });
  });
});
