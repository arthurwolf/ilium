#!/usr/bin/env python3
"""Requirement coverage of the plugin platform test suite.

Reads the requirement universe from the plugin manual and the REQ tags of the Rust tests, and
reports which ids have at least one tagged test (docs/research/plugin-platform/test-plan.md
section 3.3). Python 3 standard library only.

Usage:
    python3 tools/req_coverage.py --manual <pages dir> --root <workspace root>
        [--lane-map tools/lane_map.json] [--families all|normative] [--strict-extra] [--verbose]

Output: JSONL on stdout, one object per line, each with a `type`:
    summary    one per family (normative first)
    uncovered  one per id without a tagged test, with the page and the responsible lane
    error      tag problems (orphan-tag, unknown-id, bad-tag) and manual problems (duplicate-id)
    warning    an optional input is missing (for example tools/test_port_map.tsv)
    test       with --verbose: the tests that cite each id
    result     last line: exit code and counts

Exit codes:
    0  full coverage of the MUST and SHOULD normative ids (and, with --strict-extra, of every
       extra family) and no tag errors
    1  a MUST or SHOULD normative id has no test, a tag error was found, or (with --strict-extra)
       an extra-family id has no test
    2  usage error or a manual that cannot be parsed (for example an id defined twice)

Rules (test-plan.md section 3.3):
    * Normative ids come from rows of tables whose header row begins with `| Id` or `| [Id` (any
      letter case) in pages/reference-requirements-*.md. In reference-requirements-user-admin.md only tables under
      the heading `### Requirements` count (the "Id ranges" table holds range rows).
    * The keyword of a row is the strongest of MUST NOT, MUST, SHOULD NOT, SHOULD, MAY found in the
      statement cell. A row with none of them is treated as MUST (strictest reading) and counted in
      the summary field `keywordless`.
    * Rows whose statement starts with "withdrawn" or whose test cell says "None, withdrawn" are
      excluded, and so is every id listed under a "Retired ids" heading. Tests may still cite them.
    * A `/// REQ:` or `// REQ:` line attaches its ids to the next function within 6 lines that has a
      test attribute (`#[test]`, `#[tokio::test]`, `#[rstest]`, `#[test_case]`) or sits inside a
      `proptest!` block. Anything else is an orphan tag.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import os
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

EXIT_OK = 0
EXIT_UNCOVERED = 1
EXIT_USAGE = 2

TAG_WINDOW_LINES = 6
EXCLUDED_DIRECTORIES = {"target", "vendor", "fuzz", ".git", "node_modules"}
USER_ADMIN_PAGE = "reference-requirements-user-admin.md"
FEATURE_PAGE = "reference-feature-traceability.md"
CONFORMANCE_PAGE = "reference-conformance-suite.md"
BUDGET_PAGE = "reference-host-budgets.md"
PORT_MAP = "tools/test_port_map.tsv"
PORT_CLASS = "adapt-to-plugin-api"

KEYWORDS_BY_STRENGTH = ("MUST NOT", "MUST", "SHOULD NOT", "SHOULD", "MAY")
KEYWORD_LEVEL = {"MUST NOT": "must", "MUST": "must", "SHOULD NOT": "should", "SHOULD": "should", "MAY": "may"}
TAG_MODIFIERS = {"WARN", "MAY"}

NORMATIVE_ID = re.compile(r"^[A-Z]{3}-[0-9]{3}$")
FEATURE_ID = re.compile(r"^F-[0-9]{3}$")
BEHAVIOUR_ID = re.compile(r"^[a-z][a-z-]*-B[0-9]{2}$")
CONFORMANCE_ID = re.compile(r"^CONF-[A-Z]+-[0-9]{2}$")
BUDGET_ID = re.compile(r"^(B[0-9]{1,2}|HOST-START|CLI-COLD)$")

ROW_NORMATIVE = re.compile(r"^\| \[?([A-Z]{3}-[0-9]{3})\]? \|")
ROW_ANY_ID = re.compile(r"^\| \[?([A-Za-z][A-Za-z0-9-]*)\]? \|")
ROW_FEATURE = re.compile(r"^\| \[?(F-[0-9]{3})\]? \|")
ROW_BEHAVIOUR = re.compile(r"^\| \[?([a-z][a-z-]*-B[0-9]{2})\]? \|")
ROW_BUDGET = re.compile(r"^\| \[?(B[0-9]{1,2}|HOST-START|CLI-COLD)\]? \|")
CONFORMANCE_ANYWHERE = re.compile(r"\bCONF-[A-Z]+-[0-9]{2}\b")
HEADING = re.compile(r"^(#{1,6})\s+(.*?)\s*$")
TAG_LINE = re.compile(r"^\s*//[/!]?\s*REQ:\s*(.*?)\s*$")
FN_LINE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)")
TEST_ATTRIBUTE = re.compile(r"^\s*#\[\s*(?:tokio::test|test|rstest|test_case)\b")
RANGE = re.compile(r"^(.*?)([0-9]+)\.\.([0-9]+)$")
TRAILING_NUMBER = re.compile(r"^(.*?)([0-9]+)$")

FAMILY_ORDER = ("normative", "feature", "behaviour", "conformance", "budget", "wit", "port")


class UsageError(Exception):
    """Bad arguments or an unreadable input: exit code 2."""


@dataclass
class Requirement:
    id: str
    family: str
    page: str
    level: str = "must"
    keyword: str = ""
    note: str = ""
    lane_hint: str = ""


@dataclass
class TestRef:
    file: str
    line: int
    name: str
    modifiers: list[str] = field(default_factory=list)


@dataclass
class Universe:
    requirements: dict[str, Requirement] = field(default_factory=dict)
    excluded: set[str] = field(default_factory=set)
    errors: list[dict] = field(default_factory=list)
    warnings: list[dict] = field(default_factory=list)


def split_cells(row: str) -> list[str]:
    """Splits a Markdown table row on unescaped pipes; drops the empty edge cells."""
    cells = [cell.strip() for cell in re.split(r"(?<!\\)\|", row.strip())]
    if cells and cells[0] == "":
        cells = cells[1:]
    if cells and cells[-1] == "":
        cells = cells[:-1]
    return cells


def keyword_of(statement: str) -> str:
    for keyword in KEYWORDS_BY_STRENGTH:
        if re.search(r"\b" + keyword.replace(" ", r"\s+") + r"\b", statement):
            return keyword
    return ""


def iter_tables(text: str):
    """Yields (heading path, header cells, list of (line number, row text)) for every table."""
    headings: list[tuple[int, str]] = []
    lines = text.splitlines()
    index = 0
    while index < len(lines):
        line = lines[index]
        heading = HEADING.match(line)
        if heading:
            level = len(heading.group(1))
            headings = [item for item in headings if item[0] < level] + [(level, heading.group(2))]
            index += 1
            continue
        if line.startswith("|"):
            header = line
            rows = []
            index += 1
            while index < len(lines) and lines[index].startswith("|"):
                rows.append((index + 1, lines[index]))
                index += 1
            rows = [row for row in rows if not re.match(r"^\|\s*:?-{2,}", row[1])]
            yield [title for _, title in headings], header, rows
            continue
        index += 1


def load_normative(manual: Path, universe: Universe) -> None:
    pages = sorted(manual.glob("reference-requirements-*.md"))
    if not pages:
        raise UsageError(f"no reference-requirements-*.md pages in {manual}")
    for page in pages:
        text = page.read_text(encoding="utf-8")
        for headings, header, rows in iter_tables(text):
            retired_section = any("retired ids" in title.lower() for title in headings)
            if retired_section:
                for _, row in rows:
                    match = ROW_ANY_ID.match(row)
                    if match:
                        universe.excluded.add(match.group(1))
                continue
            # The security-policy page writes its header in lower case (`| id |`).
            if not re.match(r"^\| \[?id\b", header, re.I):
                continue
            if page.name == USER_ADMIN_PAGE and (not headings or headings[-1] != "Requirements"):
                continue
            for line_number, row in rows:
                match = ROW_NORMATIVE.match(row)
                if not match:
                    continue
                requirement_id = match.group(1)
                cells = split_cells(row)
                statement = cells[1] if len(cells) > 1 else ""
                test_cell = cells[-1] if len(cells) > 2 else ""
                withdrawn = statement.lower().startswith("withdrawn") or re.match(
                    r"^none,\s*withdrawn", test_cell.lower()
                )
                if requirement_id in universe.requirements or (
                    requirement_id in universe.excluded and not withdrawn
                ):
                    universe.errors.append(
                        {
                            "type": "error",
                            "code": "duplicate-id",
                            "id": requirement_id,
                            "page": page.name,
                            "line": line_number,
                            "message": f"{requirement_id} is defined more than once",
                        }
                    )
                    continue
                if withdrawn:
                    universe.excluded.add(requirement_id)
                    continue
                keyword = keyword_of(statement)
                universe.requirements[requirement_id] = Requirement(
                    id=requirement_id,
                    family="normative",
                    page=page.name,
                    level=KEYWORD_LEVEL.get(keyword, "must"),
                    keyword=keyword,
                )
    for requirement_id in universe.excluded:
        universe.requirements.pop(requirement_id, None)


def add_extra(universe: Universe, requirement: Requirement) -> None:
    universe.requirements.setdefault(requirement.id, requirement)


def load_features(manual: Path, universe: Universe) -> None:
    page = manual / FEATURE_PAGE
    if not page.is_file():
        universe.warnings.append({"type": "warning", "code": "missing-page", "page": FEATURE_PAGE})
        return
    for _, header, rows in iter_tables(page.read_text(encoding="utf-8")):
        columns = [cell.lower() for cell in split_cells(header)]
        for _, row in rows:
            match = ROW_FEATURE.match(row)
            if not match:
                continue
            cells = split_cells(row)
            retired = any(cell.strip("` ").lower().startswith("retired") for cell in cells)
            owner = cells[columns.index("owner")] if "owner" in columns and columns.index("owner") < len(cells) else ""
            add_extra(
                universe,
                Requirement(
                    id=match.group(1),
                    family="feature",
                    page=FEATURE_PAGE,
                    note="retired" if retired else "",
                    lane_hint=owner,
                ),
            )


def load_behaviours(manual: Path, universe: Universe) -> None:
    for page in sorted(manual.glob("core-plugin-*.md")):
        for line in page.read_text(encoding="utf-8").splitlines():
            match = ROW_BEHAVIOUR.match(line)
            if match:
                add_extra(universe, Requirement(id=match.group(1), family="behaviour", page=page.name))


def load_conformance(manual: Path, universe: Universe) -> None:
    page = manual / CONFORMANCE_PAGE
    if not page.is_file():
        universe.warnings.append({"type": "warning", "code": "missing-page", "page": CONFORMANCE_PAGE})
        return
    for case_id in sorted(set(CONFORMANCE_ANYWHERE.findall(page.read_text(encoding="utf-8")))):
        add_extra(universe, Requirement(id=case_id, family="conformance", page=CONFORMANCE_PAGE))


def load_budgets(manual: Path, universe: Universe) -> None:
    page = manual / BUDGET_PAGE
    if not page.is_file():
        universe.warnings.append({"type": "warning", "code": "missing-page", "page": BUDGET_PAGE})
        return
    for line in page.read_text(encoding="utf-8").splitlines():
        match = ROW_BUDGET.match(line)
        if match:
            add_extra(universe, Requirement(id=match.group(1), family="budget", page=BUDGET_PAGE))


def strip_wit_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return re.sub(r"//[^\n]*", "", text)


def wit_functions(text: str) -> list[str]:
    """Returns `<package>/<interface>.<function>` and `<package>/<interface>.<resource>.<method>`."""
    text = strip_wit_comments(text)
    package_match = re.search(r"^\s*package\s+([a-z0-9-]+:[a-z0-9-]+)(?:@[^;\s]+)?\s*;", text, re.M)
    if not package_match:
        return []
    package = package_match.group(1)
    results = []
    stack: list[tuple[str, str]] = []  # (kind, name) per open brace
    token = re.compile(
        r"\binterface\s+([a-z0-9-]+)\s*\{|\bworld\s+([a-z0-9-]+)\s*\{|\bresource\s+([a-z0-9-]+)\s*\{"
        r"|(?:^|[;{\s])(%?[a-z0-9-]+)\s*:\s*(?:static\s+)?(?:async\s+)?func\b|\bconstructor\s*\(|\{|\}"
    )
    for match in token.finditer(text):
        whole = match.group(0)
        if match.group(1):
            stack.append(("interface", match.group(1)))
        elif match.group(2):
            stack.append(("world", match.group(2)))
        elif match.group(3):
            stack.append(("resource", match.group(3)))
        elif match.group(4):
            name = match.group(4).lstrip("%")
            path = [entry for entry in stack if entry[0] in ("interface", "resource")]
            if stack and stack[0][0] == "interface" and path:
                results.append(f"{package}/" + ".".join(item[1] for item in path) + f".{name}")
        elif whole.startswith("constructor"):
            path = [entry for entry in stack if entry[0] in ("interface", "resource")]
            if stack and stack[0][0] == "interface" and path and path[-1][0] == "resource":
                results.append(f"{package}/" + ".".join(item[1] for item in path) + ".constructor")
        elif whole == "{":
            stack.append(("block", ""))
        elif whole == "}":
            if stack:
                stack.pop()
    return results


def load_wit(root: Path, universe: Universe) -> None:
    wit_dir = root / "wit"
    files = sorted(wit_dir.rglob("*.wit")) if wit_dir.is_dir() else []
    if not files:
        universe.warnings.append({"type": "warning", "code": "missing-wit", "path": str(wit_dir)})
        return
    for wit_file in files:
        for function in wit_functions(wit_file.read_text(encoding="utf-8")):
            add_extra(
                universe,
                Requirement(id=f"WIT:{function}", family="wit", page=str(wit_file.relative_to(root))),
            )


def load_port(root: Path, universe: Universe) -> None:
    port_map = root / PORT_MAP
    if not port_map.is_file():
        universe.warnings.append({"type": "warning", "code": "missing-port-map", "path": PORT_MAP})
        return
    lines = port_map.read_text(encoding="utf-8").splitlines()
    if not lines:
        return
    header = lines[0].split("\t")
    try:
        file_column, test_column, class_column = header.index("file"), header.index("test"), header.index("class")
    except ValueError as error:
        raise UsageError(f"{PORT_MAP}: header lacks file, test or class column") from error
    target_column = header.index("target_crate") if "target_crate" in header else None
    for line in lines[1:]:
        cells = line.split("\t")
        if len(cells) <= max(file_column, test_column, class_column) or cells[class_column] != PORT_CLASS:
            continue
        target = cells[target_column] if target_column is not None and target_column < len(cells) else ""
        add_extra(
            universe,
            Requirement(
                id=f"PORT:{cells[file_column]}::{cells[test_column]}",
                family="port",
                page=PORT_MAP,
                lane_hint=target,
            ),
        )


def family_of(identifier: str) -> str | None:
    if NORMATIVE_ID.match(identifier):
        return "normative"
    if FEATURE_ID.match(identifier):
        return "feature"
    if BEHAVIOUR_ID.match(identifier):
        return "behaviour"
    if CONFORMANCE_ID.match(identifier):
        return "conformance"
    if BUDGET_ID.match(identifier):
        return "budget"
    if identifier.startswith("WIT:"):
        return "wit"
    if identifier.startswith("PORT:"):
        return "port"
    return None


def proptest_lines(lines: list[str]) -> set[int]:
    """Indexes (0-based) of the lines that sit inside a `proptest! { ... }` invocation."""
    inside: set[int] = set()
    depth = 0
    active = False
    opened = False
    for index, line in enumerate(lines):
        code = line.split("//", 1)[0]
        if not active and "proptest!" in code:
            active, opened, depth = True, False, 0
            code = code[code.index("proptest!") :]
        if not active:
            continue
        inside.add(index)
        for char in code:
            if char == "{":
                depth += 1
                opened = True
            elif char == "}":
                depth -= 1
        if opened and depth <= 0:
            active = False
    return inside


def iter_rust_files(root: Path):
    for directory, subdirectories, files in os.walk(root):
        subdirectories[:] = sorted(name for name in subdirectories if name not in EXCLUDED_DIRECTORIES)
        for name in sorted(files):
            if name.endswith(".rs"):
                yield Path(directory) / name


def parse_tag(body: str) -> tuple[list[tuple[str, list[str]]], list[str]]:
    """Splits the text after `REQ:` into (id, modifiers) pairs and a list of problems."""
    entries, problems = [], []
    for part in body.split(","):
        words = part.split()
        if not words:
            problems.append("empty id")
            continue
        identifier, modifiers = words[0], words[1:]
        unknown = [word for word in modifiers if word not in TAG_MODIFIERS]
        if unknown:
            problems.append(f"unknown modifier {' '.join(unknown)} after {identifier}")
        entries.append((identifier, [word for word in modifiers if word in TAG_MODIFIERS]))
    return entries, problems


def scan_tests(root: Path) -> tuple[dict[str, list[TestRef]], list[dict]]:
    tests: dict[str, list[TestRef]] = {}
    errors: list[dict] = []
    for path in iter_rust_files(root):
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except UnicodeDecodeError:
            continue
        if not any("REQ:" in line for line in lines):
            continue
        relative = str(path.relative_to(root))
        in_proptest = proptest_lines(lines)
        for index, line in enumerate(lines):
            tag = TAG_LINE.match(line)
            if not tag:
                continue
            entries, problems = parse_tag(tag.group(1))
            for problem in problems:
                errors.append({"type": "error", "code": "bad-tag", "file": relative, "line": index + 1, "message": problem})
            target = None
            has_test_attribute = False
            for offset in range(1, TAG_WINDOW_LINES + 1):
                if index + offset >= len(lines):
                    break
                candidate = lines[index + offset]
                if TEST_ATTRIBUTE.match(candidate):
                    has_test_attribute = True
                function = FN_LINE.match(candidate)
                if function:
                    target = (function.group(1), index + offset)
                    break
            if target is None or not (has_test_attribute or target[1] in in_proptest):
                errors.append(
                    {
                        "type": "error",
                        "code": "orphan-tag",
                        "file": relative,
                        "line": index + 1,
                        "message": f"REQ tag not followed by a test within {TAG_WINDOW_LINES} lines",
                    }
                )
                continue
            for identifier, modifiers in entries:
                tests.setdefault(identifier, []).append(
                    TestRef(file=relative, line=target[1] + 1, name=target[0], modifiers=modifiers)
                )
    return tests, errors


@dataclass
class LaneMap:
    lanes: dict[str, dict]

    @staticmethod
    def load(path: Path | None) -> "LaneMap":
        if path is None:
            return LaneMap({})
        try:
            document = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise UsageError(f"lane map {path}: {error}") from error
        lanes = document.get("lanes") if isinstance(document, dict) else None
        if not isinstance(lanes, dict):
            raise UsageError(f"lane map {path}: missing object `lanes`")
        return LaneMap(lanes)

    def lane_for(self, requirement: Requirement) -> str | None:
        number_match = TRAILING_NUMBER.match(requirement.id)
        for lane, entry in self.lanes.items():
            for pattern in entry.get("ranges", []):
                range_match = RANGE.match(pattern)
                if range_match and number_match:
                    if number_match.group(1) == range_match.group(1) and int(range_match.group(2)) <= int(
                        number_match.group(2)
                    ) <= int(range_match.group(3)):
                        return lane
                elif pattern == requirement.id:
                    return lane
            for pattern in entry.get("patterns", []):
                if fnmatch.fnmatchcase(requirement.id, pattern):
                    return lane
            if requirement.family == "port" and requirement.lane_hint:
                plugin = requirement.lane_hint.rstrip("/").split("/")[-1]
                if plugin in entry.get("port_plugins", []):
                    return lane
        return None


def build_universe(manual: Path, root: Path, families: str) -> Universe:
    universe = Universe()
    load_normative(manual, universe)
    if families == "all":
        load_features(manual, universe)
        load_behaviours(manual, universe)
        load_conformance(manual, universe)
        load_budgets(manual, universe)
        load_wit(root, universe)
        load_port(root, universe)
    return universe


def run(arguments: argparse.Namespace, emit) -> int:
    manual = Path(arguments.manual)
    root = Path(arguments.root)
    if not manual.is_dir():
        raise UsageError(f"--manual {manual} is not a directory")
    if not root.is_dir():
        raise UsageError(f"--root {root} is not a directory")
    lane_map = LaneMap.load(Path(arguments.lane_map) if arguments.lane_map else None)
    universe = build_universe(manual, root, arguments.families)
    duplicate_errors = [error for error in universe.errors if error["code"] == "duplicate-id"]
    for record in universe.errors + universe.warnings:
        emit(record)
    if duplicate_errors:
        emit({"type": "result", "exit_code": EXIT_USAGE, "reason": "duplicate requirement ids in the manual"})
        return EXIT_USAGE

    tests, tag_errors = scan_tests(root)
    checked_families = set(FAMILY_ORDER) if arguments.families == "all" else {"normative"}
    for identifier in sorted(tests):
        family = family_of(identifier)
        if family is None:
            for ref in tests[identifier]:
                tag_errors.append({"type": "error", "code": "unknown-id", "file": ref.file, "line": ref.line,
                                   "message": f"{identifier} matches no id family"})
            continue
        if family not in checked_families:
            continue
        if identifier not in universe.requirements and identifier not in universe.excluded:
            for ref in tests[identifier]:
                tag_errors.append({"type": "error", "code": "unknown-id", "file": ref.file, "line": ref.line,
                                   "message": f"{identifier} is not defined in the manual"})
    for record in tag_errors:
        emit(record)

    exit_code = EXIT_OK
    by_family: dict[str, list[Requirement]] = {}
    for requirement in universe.requirements.values():
        by_family.setdefault(requirement.family, []).append(requirement)
    uncovered_records = []
    for family in FAMILY_ORDER:
        if family not in checked_families:
            continue
        members = sorted(by_family.get(family, []), key=lambda item: item.id)
        uncovered = [item for item in members if item.id not in tests]
        summary = {"type": "summary", "family": family, "total": len(members),
                   "covered": len(members) - len(uncovered)}
        if family == "normative":
            summary["must_uncovered"] = [item.id for item in uncovered if item.level == "must"]
            summary["should_uncovered"] = [item.id for item in uncovered if item.level == "should"]
            summary["may_uncovered"] = [item.id for item in uncovered if item.level == "may"]
            summary["keywordless"] = sum(1 for item in members if not item.keyword)
            summary["excluded"] = len(universe.excluded)
            if summary["must_uncovered"] or summary["should_uncovered"]:
                exit_code = EXIT_UNCOVERED
        else:
            summary["uncovered"] = [item.id for item in uncovered]
            if family == "feature":
                summary["retired"] = [item.id for item in members if item.note == "retired"]
            if uncovered and arguments.strict_extra:
                exit_code = EXIT_UNCOVERED
        emit(summary)
        for item in uncovered:
            record = {"type": "uncovered", "family": family, "id": item.id, "page": item.page,
                      "lane": lane_map.lane_for(item)}
            if family == "normative":
                record["keyword"] = item.keyword or None
            uncovered_records.append(record)
    for record in uncovered_records:
        emit(record)
    if arguments.verbose:
        for identifier in sorted(tests):
            if identifier in universe.requirements:
                emit({"type": "test", "id": identifier,
                      "tests": [{"file": ref.file, "line": ref.line, "name": ref.name,
                                 **({"modifiers": ref.modifiers} if ref.modifiers else {})}
                                for ref in tests[identifier]]})
    if tag_errors:
        exit_code = max(exit_code, EXIT_UNCOVERED)
    emit({"type": "result", "exit_code": exit_code, "tag_errors": len(tag_errors),
          "uncovered": len(uncovered_records), "tagged_ids": len(tests)})
    return exit_code


def parse_arguments(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Requirement coverage of the plugin platform tests.")
    parser.add_argument("--manual", required=True, help="the manual's pages directory")
    parser.add_argument("--root", required=True, help="the workspace root to scan for tests")
    parser.add_argument("--lane-map", help="tools/lane_map.json, names the lane responsible for a gap")
    parser.add_argument("--families", choices=("all", "normative"), default="normative")
    parser.add_argument("--strict-extra", action="store_true", help="exit 1 when an extra-family id has no test")
    parser.add_argument("--verbose", action="store_true", help="also print the tests that cite each id")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    def emit(record: dict) -> None:
        sys.stdout.write(json.dumps(record, sort_keys=False) + "\n")

    try:
        arguments = parse_arguments(sys.argv[1:] if argv is None else argv)
    except SystemExit as exit_request:
        return EXIT_USAGE if exit_request.code else EXIT_OK
    try:
        return run(arguments, emit)
    except UsageError as error:
        emit({"type": "error", "code": "usage", "message": str(error)})
        emit({"type": "result", "exit_code": EXIT_USAGE})
        return EXIT_USAGE


if __name__ == "__main__":
    sys.exit(main())
