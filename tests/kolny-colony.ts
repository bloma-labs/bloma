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
