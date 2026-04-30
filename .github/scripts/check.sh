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

echo
python3 .github/scripts/gate.py || STATUS=1

echo
if [ "$STATUS" -eq 0 ]; then
  echo "RESULT PASS"
else
  echo "RESULT FAIL"
fi

exit "$STATUS"
