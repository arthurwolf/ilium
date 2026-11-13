#!/usr/bin/env python3
"""Build tools/test_port_map.tsv from the existing-tests ledger (test-plan 5.3).

Adds target_crate, feature_ids and status columns to the ledger columns.
Prints JSONL totals per class and per owner; exits 1 on an unknown owner.
Standard library only, deterministic.
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_LEDGER = Path("/home/arthur/dev/ai/ilium/docs/research/plugin-platform/existing-tests-ledger.tsv")
DEFAULT_TRACE = ROOT / "docs/plugins-manual/pages/reference-feature-traceability.md"
DEFAULT_PLUGINS = ROOT / "plugins/core"
DEFAULT_OUT = ROOT / "tools/test_port_map.tsv"
LEDGER_COLUMNS = ["file", "test", "attr", "ignored", "owner", "class"]
EXTRA_COLUMNS = ["target_crate", "feature_ids", "status"]
PLUGIN_CLASSES = ("moves-to-plugin", "adapt-to-plugin-api")
KERNEL = "kernel"
ROW_ID = re.compile(r"^F-\d{3}$")


def plugin_names(plugins_dir: Path) -> list[str]:
    return sorted(p.name for p in plugins_dir.iterdir() if p.is_dir())


def owner_tokens(owner_cell: str, names: list[str]) -> set[str]:
    """Owners named in a traceability Owner cell; text before the first ';' of a kernel-first cell is the kernel part."""
    cell = owner_cell.strip()
    found: set[str] = set()
    if cell.startswith(KERNEL):
        found.add(KERNEL)
        cell = cell.split(";", 1)[1] if ";" in cell else ""
    for name in names:
        if re.search(r"(?<![\w-])" + re.escape(name) + r"(?![\w-])", cell):
            found.add(name)
    return found


def load_feature_map(trace_path: Path, names: list[str]) -> dict[str, list[str]]:
    """owner name -> sorted F ids."""
    mapping: dict[str, set[str]] = {}
    for line in trace_path.read_text(encoding="utf-8").splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 5 or not ROW_ID.match(cells[0]):
            continue
        for owner in owner_tokens(cells[4], names):
            mapping.setdefault(owner, set()).add(cells[0])
    return {owner: sorted(ids) for owner, ids in mapping.items()}


def classify(ledger_lines: list[str], names: list[str], features: dict[str, list[str]]):
    """Returns (output rows, class counts, owner counts, unknown owners)."""
    header = ledger_lines[0].split("\t")
    index = {column: header.index(column) for column in LEDGER_COLUMNS}
    valid_owners = set(names) | {KERNEL}
    rows, by_class, by_owner, unknown = [], {}, {}, set()
    for line in ledger_lines[1:]:
        if not line.strip():
            continue
        cells = line.split("\t")
        cells += [""] * (len(header) - len(cells))
        owner, klass = cells[index["owner"]], cells[index["class"]]
        if owner not in valid_owners:
            unknown.add(owner)
        by_class[klass] = by_class.get(klass, 0) + 1
        by_owner[owner] = by_owner.get(owner, 0) + 1
        target = f"plugins/core/{owner}" if klass in PLUGIN_CLASSES and owner in names else ""
        rows.append(cells[: len(header)] + [target, ",".join(features.get(owner, [])), "pending"])
    return header + EXTRA_COLUMNS, rows, by_class, by_owner, sorted(unknown)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", type=Path, default=DEFAULT_LEDGER)
    parser.add_argument("--traceability", type=Path, default=DEFAULT_TRACE)
    parser.add_argument("--plugins-dir", type=Path, default=DEFAULT_PLUGINS)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    args = parser.parse_args(argv)
    names = plugin_names(args.plugins_dir)
    features = load_feature_map(args.traceability, names)
    lines = args.ledger.read_text(encoding="utf-8").splitlines()
    header, rows, by_class, by_owner, unknown = classify(lines, names, features)
    args.out.write_text(
        "\n".join("\t".join(r) for r in [header] + rows) + "\n", encoding="utf-8"
    )
    for klass in sorted(by_class):
        print(json.dumps({"type": "summary", "class": klass, "tests": by_class[klass]}))
    for owner in sorted(by_owner):
        print(json.dumps({"type": "summary", "owner": owner, "tests": by_owner[owner]}))
    for owner in unknown:
        print(json.dumps({"type": "error", "code": "unknown-owner", "owner": owner}))
    print(json.dumps({"type": "result", "rows": len(rows), "out": str(args.out), "unknown_owners": len(unknown)}))
    return 1 if unknown else 0


if __name__ == "__main__":
    sys.exit(main())
