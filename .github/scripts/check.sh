#!/usr/bin/env bash
# Local gate for the KOLNY specification repository.
#
# Runs the same checks as .github/workflows/ci.yml so a contributor can see the
# result before opening a pull request. Every check prints PASS or FAIL; the
# script exits non-zero if any check fails.
#
# Usage: bash .github/scripts/check.sh

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT" || exit 1

STATUS=0

REQUIRED=(
  "README.md"
  "LICENSE"
  "docs/architecture.md"
  "docs/allocation-spec.md"
  "docs/risk-spec.md"
  "docs/security.md"
  "docs/references.md"
  ".github/workflows/ci.yml"
  ".github/scripts/gate.py"
  "Anchor.toml"
  "Cargo.toml"
  "idl/kolny_colony.json"
  "programs/kolny-colony/Cargo.toml"
  "programs/kolny-colony/src/lib.rs"
  "programs/kolny-colony/README.md"
  "scripts/set-program-id.sh"
)

echo "repository root: $REPO_ROOT"
echo

MISSING=()
for file in "${REQUIRED[@]}"; do
  [ -f "$file" ] || MISSING+=("$file")
done

if [ ${#MISSING[@]} -eq 0 ]; then
  echo "PASS structure (${#REQUIRED[@]} required files present)"
else
  echo "FAIL structure"
  for file in "${MISSING[@]}"; do
    echo "  missing: $file"
  done
  STATUS=1
fi

# Every specification document must carry real content. A file reduced to a
# placeholder would let the README advertise a document that says nothing.
THIN=()
for file in docs/*.md; do
  [ -f "$file" ] || continue
  lines=$(wc -l < "$file")
  if [ "$lines" -lt 60 ]; then
    THIN+=("$file ($lines lines)")
  fi
done

if [ ${#THIN[@]} -eq 0 ]; then
  echo "PASS document-substance (no specification document under 60 lines)"
else
  echo "FAIL document-substance"
  for entry in "${THIN[@]}"; do
    echo "  $entry"
  done
  STATUS=1
fi

# The program ID appears in three places and every PDA derives from it. If they
# drift, the mismatch does not fail loudly at runtime; it silently produces a
# second, unreachable set of accounts. So it is checked here instead.
LIB_ID=$(grep -oP 'declare_id!\("\K[^"]+' programs/kolny-colony/src/lib.rs 2>/dev/null)
TOML_ID=$(grep -oP '^kolny_colony = "\K[^"]+' Anchor.toml 2>/dev/null)
IDL_ID=$(python3 -c "import json;print(json.load(open('idl/kolny_colony.json'))['address'])" 2>/dev/null)

if [ -n "$LIB_ID" ] && [ "$LIB_ID" = "$TOML_ID" ] && [ "$LIB_ID" = "$IDL_ID" ]; then
  echo "PASS program-id-consistency (lib.rs, Anchor.toml and idl agree on $LIB_ID)"
else
  echo "FAIL program-id-consistency"
  echo "  src/lib.rs   $LIB_ID"
  echo "  Anchor.toml  $TOML_ID"
  echo "  idl address  $IDL_ID"
  STATUS=1
fi

# This repository must not be able to send a transaction to any cluster as a
# side effect of a build, a test or a workflow.
CLUSTER=$(grep -oP '^cluster = "\K[^"]+' Anchor.toml 2>/dev/null)
# Character classes rather than literal two-word strings, so this pattern also
# catches odd spacing and does not match itself.
WIRED=$(grep -rnE 'anchor[[:space:]]+deploy|solana[[:space:]]+program[[:space:]]+deploy|solana[[:space:]]+airdrop|solana-keygen[[:space:]]+new' \
  .github/workflows package.json Anchor.toml 2>/dev/null | wc -l)

if [ "$CLUSTER" = "Localnet" ] && [ "$WIRED" -eq 0 ]; then
  echo "PASS no-chain-contact (cluster is Localnet, no deploy command is wired in)"
else
  echo "FAIL no-chain-contact"
  echo "  cluster: $CLUSTER (expected Localnet)"
  echo "  deploy commands wired into workflows or scripts: $WIRED"
  STATUS=1
fi

echo
python3 .github/scripts/gate.py || STATUS=1

echo
if [ "$STATUS" -eq 0 ]; then
  echo "RESULT PASS"
else
  echo "RESULT FAIL"
fi

exit "$STATUS"
