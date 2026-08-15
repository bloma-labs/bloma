#!/usr/bin/env python3
"""Documentation gate for the KOLNY specification repository.

Three checks run over every Markdown file in the tree:

1. Prohibited language. The canonical prohibition list is section 6.1 of
   ``docs/risk-spec.md``. That section states that it is the single place the
   prohibited terms are written down, so this script parses the terms out of it
   rather than carrying a second copy. A duplicated list would drift from the
   specification and would itself become a place the terms appear.

2. Emoji. The house style forbids emoji in every surface, including
   documentation and commit messages.

3. Cross-references. Every relative Markdown link and every backticked
   ``*.md`` reference must resolve to a file that exists, so the specification
   cannot advertise a document the tree does not contain.

Exit status is 0 when every check passes and 1 otherwise.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
CANONICAL_LIST = REPO_ROOT / "docs" / "risk-spec.md"
CANONICAL_HEADING = "Prohibited terms"

# Codepoint ranges covering pictographic emoji, dingbats, variation selectors
# and the box-drawing characters that leak in from ASCII diagrams.
EMOJI_RANGES = (
    (0x1F000, 0x1FAFF),
    (0x2190, 0x21FF),
    (0x2300, 0x23FF),
    (0x2460, 0x24FF),
    (0x25A0, 0x27BF),
    (0x2B00, 0x2BFF),
    (0x2500, 0x257F),
    (0xFE00, 0xFE0F),
)

SKIP_DIRS = {".git", "node_modules", "target", ".next", "dist", "build"}

# These documents are maintained elsewhere and copied in. A copy step is exactly
# where a private path or an internal account name gets carried across by
# accident, so the shapes such a leak takes are matched here rather than trusting
# the copy to have been read carefully. Matching shapes rather than a list of
# known-bad names means this keeps working for names nobody has thought of yet.
PRIVATE_SHAPES = (
    (re.compile(r"/home/[A-Za-z0-9._-]+/"), "absolute home directory path"),
    (re.compile(r"/Users/[A-Za-z0-9._-]+/"), "absolute macOS home path"),
    (re.compile(r"[A-Za-z]:\\\\?Users\\"), "absolute Windows user path"),
    (re.compile(r"\bapps/(?:web|service)/"), "internal monorepo application path"),
    (re.compile(r"\.next/"), "internal build output path"),
)

# A capitalized token directly before "repository" is usually an account or
# organization name. Public ones are fine; anything unrecognized is treated as an
# internal name that escaped a copy step, which is how the private deploy account
# leaked into two of these documents once already.
REPO_OWNER = re.compile(r"\b([A-Z][A-Za-z0-9-]{2,})\s+repositor(?:y|ies)\b")
PUBLIC_OWNERS = {"github", "kolny", "the", "this", "anchor", "solana", "mit", "idl"}

MD_LINK = re.compile(r"\[[^\]]*\]\(([^)\s]+)")
MD_BACKTICK_DOC = re.compile(r"`([A-Za-z0-9._/-]+\.md)`")
QUOTED = re.compile(r'"([^"]+)"')
HEADING = re.compile(r"^(#{1,6})\s+(.*)$")


def markdown_files() -> list[Path]:
    out = []
    for path in sorted(REPO_ROOT.rglob("*.md")):
        if any(part in SKIP_DIRS for part in path.relative_to(REPO_ROOT).parts):
            continue
        out.append(path)
    return out


def canonical_region() -> tuple[int, int]:
    """Return the (start, end) line numbers, 1-based inclusive, of the
    prohibition list section inside the canonical document."""
    lines = CANONICAL_LIST.read_text(encoding="utf-8").splitlines()
    start = end = None
    level = 0
    for index, line in enumerate(lines, start=1):
        match = HEADING.match(line)
        if match is None:
            continue
        if start is None:
            if CANONICAL_HEADING.lower() in match.group(2).lower():
                start = index
                level = len(match.group(1))
            continue
        if len(match.group(1)) <= level:
            end = index - 1
            break
    if start is None:
        raise SystemExit(
            f"FAIL structure: no heading containing {CANONICAL_HEADING!r} in "
            f"{CANONICAL_LIST.relative_to(REPO_ROOT)}. The prohibited-language "
            f"check has no source of truth."
        )
    return start, end if end is not None else len(lines)


def prohibited_terms(region: tuple[int, int]) -> list[str]:
    """Terms are the double-quoted fragments of the canonical bullet list."""
    lines = CANONICAL_LIST.read_text(encoding="utf-8").splitlines()
    start, end = region
    terms: list[str] = []
    for line in lines[start - 1 : end]:
        if not line.lstrip().startswith("-"):
            continue
        terms.extend(QUOTED.findall(line))
    return sorted({t.strip().lower() for t in terms if t.strip()})


def check_language(files: list[Path], terms: list[str], region: tuple[int, int]) -> list[str]:
    if not terms:
        return [
            "no terms parsed from the canonical prohibition list; the check "
            "would pass vacuously"
        ]
    start, end = region
    failures = []
    for path in files:
        rel = path.relative_to(REPO_ROOT)
        is_canonical = path.resolve() == CANONICAL_LIST.resolve()
        for number, line in enumerate(
            path.read_text(encoding="utf-8").splitlines(), start=1
        ):
            if is_canonical and start <= number <= end:
                continue
            lowered = line.lower()
            for term in terms:
                if term in lowered:
                    failures.append(f"{rel}:{number}: prohibited term {term!r}")
    return failures


def check_emoji(files: list[Path]) -> list[str]:
    failures = []
    for path in files:
        rel = path.relative_to(REPO_ROOT)
        for number, line in enumerate(
            path.read_text(encoding="utf-8").splitlines(), start=1
        ):
            for char in line:
                point = ord(char)
                if any(low <= point <= high for low, high in EMOJI_RANGES):
                    failures.append(
                        f"{rel}:{number}: non-text character U+{point:04X}"
                    )
                    break
    return failures


def check_private_identifiers(files: list[Path]) -> list[str]:
    failures = []
    for path in files:
        rel = path.relative_to(REPO_ROOT)
        for number, line in enumerate(
            path.read_text(encoding="utf-8").splitlines(), start=1
        ):
            for pattern, label in PRIVATE_SHAPES:
                found = pattern.search(line)
                if found:
                    failures.append(f"{rel}:{number}: {label} {found.group(0)!r}")
            for owner in REPO_OWNER.findall(line):
                if owner.lower() not in PUBLIC_OWNERS:
                    failures.append(
                        f"{rel}:{number}: unrecognized account name {owner!r} "
                        f"before 'repository'"
                    )
    return failures


def check_references(files: list[Path]) -> list[str]:
    """A clickable link and a prose mention are resolved differently.

    GitHub renders ``[text](path)`` relative to the directory of the file it
    appears in, so a link is only correct if it resolves that way. A backticked
    ``path.md`` in prose is a name, not a link, and a reader reads it as the
    document at that path in the repository. So a mention is accepted if it
    resolves either from the file's own directory or from the repository root.
    """
    failures = []
    for path in files:
        rel = path.relative_to(REPO_ROOT)
        text = path.read_text(encoding="utf-8")

        links = set()
        for target in MD_LINK.findall(text):
            if target.startswith(("http://", "https://", "mailto:", "#")):
                continue
            links.add(target.split("#", 1)[0])
        for target in sorted(t for t in links if t):
            if not (path.parent / target).resolve().exists():
                failures.append(f"{rel}: unresolved link {target!r}")

        mentions = set(MD_BACKTICK_DOC.findall(text)) - links
        for target in sorted(t for t in mentions if t):
            from_file = (path.parent / target).resolve()
            from_root = (REPO_ROOT / target).resolve()
            if not from_file.exists() and not from_root.exists():
                failures.append(f"{rel}: unresolved mention {target!r}")
    return failures


def report(name: str, failures: list[str]) -> bool:
    if failures:
        print(f"FAIL {name}")
        for line in failures:
            print(f"  {line}")
        return False
    print(f"PASS {name}")
    return True


def main() -> int:
    files = markdown_files()
    if not files:
        print("FAIL structure: no Markdown files found")
        return 1
    print(f"scanning {len(files)} Markdown files")

    region = canonical_region()
    terms = prohibited_terms(region)
    print(
        f"canonical prohibition list: "
        f"{CANONICAL_LIST.relative_to(REPO_ROOT)} lines {region[0]}-{region[1]}, "
        f"{len(terms)} terms"
    )

    ok = True
    ok &= report("prohibited-language", check_language(files, terms, region))
    ok &= report("emoji", check_emoji(files))
    ok &= report("private-identifiers", check_private_identifiers(files))
    ok &= report("cross-references", check_references(files))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
