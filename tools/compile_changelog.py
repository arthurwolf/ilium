#!/usr/bin/env python3
"""Fold changelog.d/*.md fragments into the [Unreleased] section of CHANGELOG.md."""
import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FRAGMENT_DIR = ROOT / "changelog.d"
CHANGELOG = ROOT / "CHANGELOG.md"
CATEGORIES = ["Added", "Changed", "Deprecated", "Removed", "Fixed", "Security"]
UNRELEASED = "## [Unreleased]"


def parse_fragment(path: Path) -> dict[str, list[str]]:
    entries: dict[str, list[str]] = {}
    category = None
    for line in path.read_text().splitlines():
        heading = re.match(r"###\s+(\w+)\s*$", line)
        if heading:
            category = heading.group(1)
            if category not in CATEGORIES:
                raise ValueError(f"{path.name}: unknown heading '### {category}'")
            continue
        if not line.strip():
            continue
        if category is None:
            raise ValueError(f"{path.name}: text before the first '### <Category>' heading")
        if line.startswith("- "):
            entries.setdefault(category, []).append(line)
        else:  # continuation of the previous bullet
            if not entries.get(category):
                raise ValueError(f"{path.name}: continuation line without a bullet")
            entries[category][-1] += "\n" + line
    return entries


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="validate only; write nothing")
    args = parser.parse_args()

    fragments = sorted(p for p in FRAGMENT_DIR.glob("*.md") if p.name != "README.md")
    merged: dict[str, list[str]] = {}
    for fragment in fragments:
        try:
            parsed = parse_fragment(fragment)
        except ValueError as error:
            print(f"error: {error}", file=sys.stderr)
            return 1
        for category, bullets in parsed.items():
            merged.setdefault(category, []).extend(bullets)
    if args.check or not fragments:
        print(f"{len(fragments)} fragment(s) valid" if fragments else "no fragments")
        return 0

    text = CHANGELOG.read_text()
    if UNRELEASED not in text:
        print(f"error: CHANGELOG.md has no '{UNRELEASED}' heading", file=sys.stderr)
        return 1
    block = "".join(
        f"\n### {category}\n" + "\n".join(merged[category]) + "\n"
        for category in CATEGORIES
        if category in merged
    )
    CHANGELOG.write_text(text.replace(UNRELEASED, UNRELEASED + "\n" + block, 1))
    for fragment in fragments:
        fragment.unlink()
    print(f"folded {len(fragments)} fragment(s) into {UNRELEASED}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
