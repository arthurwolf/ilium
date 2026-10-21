#!/usr/bin/env python3
"""Single source for the plugin limit tables.

`docs/plugins-manual/limits.json` holds every keyed limit row (default and tier
ceilings). `--build` regenerates it from the canonical page's tables; `--check`
(the default) fails when a manual page has a limit-key table row whose
default or ceiling cells differ from the JSON.
"""
import argparse
import json
import re
import sys
from pathlib import Path

PAGES = Path("docs/plugins-manual/pages")
TABLE = PAGES / "reference-errors-and-limits.md"
DATA = Path("docs/plugins-manual/limits.json")
CELL = re.compile(r"`([a-z_]+)`")
CEILING_T2 = ("ceiling t2/t3", "ceiling t2 and t3")

SECTION_HEADINGS = {
    "manifest": "Resource limits in the manifest",
    "requestable": "Requestable limits",
    "session": "Host rules for limits and restarts",
}


def numbers(cell: str):
    """Return the integer in a plain numeric cell, or None for prose cells."""
    digits = cell.replace(",", "").strip()
    return int(digits) if digits.isdigit() else None


def parse_rows(text: str):
    """Yield (table_header_lower, cells) for each body row of each pipe table."""
    header = None
    for line in text.splitlines():
        if not line.startswith("|"):
            header = None
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if header is None:
            header = [c.lower() for c in cells]
            continue
        if set("".join(cells)) <= set("-: "):
            continue
        yield header, cells


def limit_row(header, cells):
    """Return (key, default, t1, t2) when the row is a limit row, else None."""
    if not header or header[0] != "key" or "default" not in header:
        return None
    key_match = CELL.fullmatch(cells[0])
    if not key_match:
        return None
    default_index = header.index("default")
    t1_index = next((i for i, h in enumerate(header) if h.startswith("ceiling t1")), None)
    t2_index = next((i for i, h in enumerate(header) if h in CEILING_T2), None)
    if t1_index is None or t2_index is None:
        return None
    values = (numbers(cells[default_index]), numbers(cells[t1_index]), numbers(cells[t2_index]))
    if None in values:
        return None
    return (key_match.group(1), *values)


def build() -> dict:
    text = TABLE.read_text(encoding="utf-8")
    rows = {}
    for header, cells in parse_rows(text):
        parsed = limit_row(header, cells)
        if parsed:
            key, default, t1, t2 = parsed
            rows[key] = {"default": default, "ceiling_t1": t1, "ceiling_t2_t3": t2}
    return {"source": str(TABLE), "limits": dict(sorted(rows.items()))}


def check() -> list[str]:
    data = json.loads(DATA.read_text(encoding="utf-8"))["limits"]
    problems = []
    for page in sorted(PAGES.glob("*.md")):
        for header, cells in parse_rows(page.read_text(encoding="utf-8")):
            parsed = limit_row(header, cells)
            if not parsed:
                continue
            key, default, t1, t2 = parsed
            if key not in data:
                problems.append(f"{page.name}: limit key `{key}` is not in limits.json")
                continue
            expected = data[key]
            actual = {"default": default, "ceiling_t1": t1, "ceiling_t2_t3": t2}
            if actual != expected:
                problems.append(f"{page.name}: `{key}` row {actual} != limits.json {expected}")
    return problems


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--build", action="store_true", help="regenerate limits.json")
    args = parser.parse_args()
    if args.build:
        DATA.write_text(json.dumps(build(), indent=2) + "\n", encoding="utf-8")
        print(f"wrote {DATA}")
        return 0
    problems = check()
    for problem in problems:
        print(problem)
    print(f"limit rows checked against {DATA}: {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
