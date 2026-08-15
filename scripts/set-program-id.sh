#!/usr/bin/env bash
#
# Replace the placeholder program ID with the real one.
#
# THIS SCRIPT TOUCHES NO CLUSTER. It edits two source files and nothing else.
# It is deliberately not wired into any build, test, hook or CI job: it rewrites
# source, so running it has to be a decision someone makes rather than a side
# effect of a build.
#
# Usage:
#   scripts/set-program-id.sh <PROGRAM_ID>
#
# After running it you MUST re-run `anchor build`, because the IDL embeds the
# address, and then re-derive the PDAs and compare them against the front end
# and the SDK. Every PDA in this program derives from the program ID, so a
# mismatch does not fail loudly -- it silently produces a second, unreachable
# set of accounts.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIB="$HERE/programs/kolny-colony/src/lib.rs"
TOML="$HERE/Anchor.toml"

PLACEHOLDER="Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS"

if [ $# -ne 1 ]; then
  echo "usage: $(basename "$0") <PROGRAM_ID>" >&2
  echo "" >&2
  echo "Get the ID from the keypair the project owner supplies:" >&2
  echo "  solana-keygen pubkey /path/to/kolny-colony-keypair.json" >&2
  echo "" >&2
  echo "Do NOT generate a keypair here. The deploy and program keypairs are" >&2
  echo "the owner's to create and hold." >&2
  exit 2
fi

NEW_ID="$1"

# Base58 excludes 0, O, I and l. Solana public keys are 32 bytes, which encodes
# to 43 or 44 base58 characters.
if ! printf '%s' "$NEW_ID" | grep -Eq '^[1-9A-HJ-NP-Za-km-z]{43,44}$'; then
  echo "FAIL: '$NEW_ID' is not a base58 public key." >&2
  exit 1
fi

if [ "$NEW_ID" = "$PLACEHOLDER" ]; then
  echo "FAIL: that is the placeholder, not a real program ID." >&2
  exit 1
fi

CURRENT="$(grep -oP 'declare_id!\("\K[^"]+' "$LIB")"
echo "current: $CURRENT"
echo "new:     $NEW_ID"

# Replace whatever is there now, so this is repeatable rather than one-shot.
sed -i "s/declare_id!(\"$CURRENT\")/declare_id!(\"$NEW_ID\")/" "$LIB"
sed -i "s/^kolny_colony = \".*\"/kolny_colony = \"$NEW_ID\"/" "$TOML"

echo ""
echo "updated:"
grep -n 'declare_id!' "$LIB"
grep -n '^kolny_colony' "$TOML"

echo ""
echo "NEXT, all four are required:"
echo "  1. anchor build                 regenerate the IDL, which embeds the address"
echo "  2. check target/idl/kolny_colony.json 'address' equals $NEW_ID"
echo "  3. re-derive every PDA and compare against the front end and the SDK"
echo "  4. only then initialize any account"
