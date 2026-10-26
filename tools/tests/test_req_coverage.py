"""Unit tests of tools/req_coverage.py on synthetic manuals and workspaces.

Run with: python3 -m unittest discover tools/tests
"""

import importlib.util
import io
import json
import sys
import tempfile
import unittest
import unittest.mock
from contextlib import redirect_stdout
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "req_coverage.py"
SPEC = importlib.util.spec_from_file_location("req_coverage", SCRIPT)
req_coverage = importlib.util.module_from_spec(SPEC)
sys.modules["req_coverage"] = req_coverage  # dataclasses look the module up while decorating
SPEC.loader.exec_module(req_coverage)

REQUIREMENTS_PAGE = """# Host requirements

### Requirements

| Id | Requirement | Test |
|----|-------------|------|
| HST-001 | The host MUST start. | unit |
| HST-002 | The host SHOULD log. | unit |
| HST-003 | The host MAY cache. | unit |
| HST-004 | The host MUST NOT panic; it SHOULD recover. | unit |
| HST-005 | The host starts plugins in order. | unit |
| [HST-006] | Withdrawn: replaced by HST-001. | None, withdrawn |
| HST-007 | The host MUST stop. | None, withdrawn |

## Retired ids

| Id | Reason |
|----|--------|
| HST-008 | merged |
| DEV-189a | split |
"""

USER_ADMIN_PAGE = """# User admin

### Id ranges

| Id | Range |
|----|-------|
| USR-110 | screens |

### Requirements

| Id | Requirement | Test |
|----|-------------|------|
| USR-110 | The screen MUST list plugins. | ui |
"""


def write(root: Path, relative: str, text: str) -> None:
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


class CoverageCase(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.base = Path(self.directory.name)
        self.manual = self.base / "pages"
        self.root = self.base / "ws"
        self.manual.mkdir()
        self.root.mkdir()
        write(self.manual, "reference-requirements-host.md", REQUIREMENTS_PAGE)
        write(self.manual, "reference-requirements-user-admin.md", USER_ADMIN_PAGE)

    def tearDown(self) -> None:
        self.directory.cleanup()

    def run_script(self, *extra: str) -> tuple[int, list[dict]]:
        buffer = io.StringIO()
        with redirect_stdout(buffer):
            code = req_coverage.main(["--manual", str(self.manual), "--root", str(self.root), *extra])
        records = [json.loads(line) for line in buffer.getvalue().splitlines()]
        return code, records

    @staticmethod
    def of_type(records: list[dict], kind: str) -> list[dict]:
        return [record for record in records if record["type"] == kind]

    @staticmethod
    def summary(records: list[dict], family: str = "normative") -> dict:
        return next(r for r in records if r["type"] == "summary" and r["family"] == family)


class NormativeTests(CoverageCase):
    def test_universe_keywords_withdrawn_and_retired(self) -> None:
        code, records = self.run_script()
        summary = self.summary(records)
        self.assertEqual(code, req_coverage.EXIT_UNCOVERED)
        # HST-001..005 and USR-110; HST-006/007 withdrawn, HST-008 retired.
        self.assertEqual(summary["total"], 6)
        self.assertEqual(summary["must_uncovered"], ["HST-001", "HST-004", "HST-005", "USR-110"])
        self.assertEqual(summary["should_uncovered"], ["HST-002"])
        self.assertEqual(summary["may_uncovered"], ["HST-003"])
        self.assertEqual(summary["keywordless"], 1)
        uncovered = {record["id"]: record for record in self.of_type(records, "uncovered")}
        self.assertNotIn("HST-006", uncovered)
        self.assertNotIn("HST-008", uncovered)
        self.assertEqual(uncovered["HST-004"]["keyword"], "MUST NOT")
        self.assertIsNone(uncovered["HST-005"]["keyword"])
        self.assertEqual(uncovered["USR-110"]["page"], "reference-requirements-user-admin.md")

    def test_user_admin_range_table_is_not_a_duplicate(self) -> None:
        _, records = self.run_script()
        self.assertEqual([r for r in self.of_type(records, "error") if r["code"] == "duplicate-id"], [])

    def test_lower_case_header_is_a_requirement_table(self) -> None:
        write(self.manual, "reference-requirements-security.md",
              "| id | statement | test |\n|---|---|---|\n| SEC-001 | The host MUST check. | unit |\n")
        _, records = self.run_script()
        self.assertIn("SEC-001", self.summary(records)["must_uncovered"])

    def test_other_tables_are_ignored(self) -> None:
        write(self.manual, "reference-requirements-security.md",
              "| Label | Rows |\n|---|---|\n| SEC-001 | x MUST |\n")
        _, records = self.run_script()
        self.assertEqual(self.summary(records)["total"], 6)

    def test_duplicate_id_is_a_parse_error(self) -> None:
        write(self.manual, "reference-requirements-other.md",
              "| Id | Requirement | Test |\n|---|---|---|\n| HST-001 | Again MUST. | unit |\n")
        code, records = self.run_script()
        self.assertEqual(code, req_coverage.EXIT_USAGE)
        errors = [r for r in self.of_type(records, "error") if r["code"] == "duplicate-id"]
        self.assertEqual([error["id"] for error in errors], ["HST-001"])

    def test_full_coverage_exits_zero_and_may_gaps_do_not_fail(self) -> None:
        write(self.root, "crate/tests/all.rs", """
/// REQ: HST-001, HST-002
#[test]
fn starts_and_logs() {}

// REQ: HST-004
#[tokio::test]
async fn never_panics() {}

/// REQ: HST-005
/// More words about the test.
#[test]
fn ordered() {}

/// REQ: USR-110 WARN
#[test]
fn lists() {}
""")
        code, records = self.run_script("--verbose")
        self.assertEqual(code, req_coverage.EXIT_OK, records)
        self.assertEqual(self.summary(records)["may_uncovered"], ["HST-003"])
        tests = {record["id"]: record["tests"] for record in self.of_type(records, "test")}
        self.assertEqual(tests["HST-002"][0]["name"], "starts_and_logs")
        self.assertEqual(tests["USR-110"][0]["modifiers"], ["WARN"])

    def test_withdrawn_and_retired_ids_may_still_be_cited(self) -> None:
        write(self.root, "crate/tests/old.rs", "/// REQ: HST-006, HST-008\n#[test]\nfn old() {}\n")
        _, records = self.run_script()
        self.assertEqual(self.of_type(records, "error"), [])


class TagTests(CoverageCase):
    def test_orphan_tag(self) -> None:
        write(self.root, "crate/src/lib.rs", "/// REQ: HST-001\npub fn not_a_test() {}\n")
        write(self.root, "crate/tests/far.rs", "/// REQ: HST-002\n\n\n\n\n\n\n#[test]\nfn too_far() {}\n")
        code, records = self.run_script()
        orphans = [r for r in self.of_type(records, "error") if r["code"] == "orphan-tag"]
        self.assertEqual(sorted(r["file"] for r in orphans), ["crate/src/lib.rs", "crate/tests/far.rs"])
        self.assertEqual(code, req_coverage.EXIT_UNCOVERED)
        self.assertIn("HST-001", self.summary(records)["must_uncovered"])

    def test_tag_inside_proptest_block(self) -> None:
        write(self.root, "crate/tests/props.rs", """
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    // REQ: HST-001
    fn roundtrip(value in 0u32..10) {
        prop_assert!(value < 10);
    }
}

// REQ: HST-002
fn after_the_block(value: u32) {}
""")
        _, records = self.run_script()
        orphans = [r for r in self.of_type(records, "error") if r["code"] == "orphan-tag"]
        self.assertEqual([r["line"] for r in orphans], [12])
        self.assertNotIn("HST-001", self.summary(records)["must_uncovered"])

    def test_unknown_ids(self) -> None:
        write(self.root, "crate/tests/unknown.rs",
              "/// REQ: HST-099\n#[test]\nfn a() {}\n/// REQ: nonsense\n#[test]\nfn b() {}\n")
        code, records = self.run_script()
        unknown = [r for r in self.of_type(records, "error") if r["code"] == "unknown-id"]
        self.assertEqual(len(unknown), 2)
        self.assertEqual(code, req_coverage.EXIT_UNCOVERED)

    def test_bad_modifier(self) -> None:
        write(self.root, "crate/tests/bad.rs", "/// REQ: HST-001 LATER\n#[test]\nfn a() {}\n")
        _, records = self.run_script()
        self.assertEqual([r["code"] for r in self.of_type(records, "error")], ["bad-tag"])

    def test_excluded_directories_are_not_scanned(self) -> None:
        for directory in ("target", "vendor", "fuzz"):
            write(self.root, f"{directory}/x.rs", "/// REQ: HST-999\n#[test]\nfn a() {}\n")
        _, records = self.run_script()
        self.assertEqual(self.of_type(records, "error"), [])


class ExtraFamilyTests(CoverageCase):
    def setUp(self) -> None:
        super().setUp()
        write(self.manual, "reference-feature-traceability.md", """
| Id | Feature | Owner | Status |
|----|---------|-------|--------|
| F-001 | Tree | core-tree-term | planned |
| F-102 | Old | none | retired; replaced |
""")
        write(self.manual, "core-plugin-chatroom.md", """
| Id | Behaviour |
|----|-----------|
| chatroom-B01 | Sends |
| chatroom-B02 | Reads |
""")
        write(self.manual, "reference-conformance-suite.md",
              "| Case | What |\n|---|---|\n| CONF-MAN-01 | manifest |\nSee also CONF-MAN-01 and CONF-SEC-02.\n")
        write(self.manual, "reference-host-budgets.md",
              "| Id | Budget |\n|---|---|\n| B1 | x |\n| B24 | y |\n| HOST-START | z |\n| CLI-COLD | w |\n")
        write(self.root, "wit/host/log.wit", """
package ilium:host@0.1.0;

/// Logging.
interface log {
    // a comment: func
    record entry { level: u8 }
    write: func(entry: entry);
    resource sink {
        constructor(name: string);
        flush: func() -> result;
    }
}

world logger {
    import log;
    export run: func();
}
""")
        write(self.root, "tools/test_port_map.tsv",
              "file\ttest\tclass\ttarget_crate\n"
              "ilium-server/tests/a.rs\tworks\tadapt-to-plugin-api\tplugins/core/chatroom\n"
              "ilium-server/tests/a.rs\tkept\tkeep\t\n")
        write(self.base, "lane_map.json", json.dumps({"lanes": {
            "core-automation": {"ranges": ["chatroom-B01..17"], "patterns": [], "port_plugins": ["chatroom"]},
            "imports-core": {"ranges": [], "patterns": ["WIT:ilium:host/log.*"]},
            "conformance": {"patterns": ["CONF-*"]},
            "perf-budgets": {"ranges": ["B1..24"], "patterns": ["HOST-START", "CLI-COLD"]},
            "port-compat": {"ranges": ["F-001..004", "F-102"]},
            "host-runtime": {"ranges": ["HST-001..045"]},
        }}))

    def test_each_family_is_parsed(self) -> None:
        code, records = self.run_script("--families", "all", "--lane-map", str(self.base / "lane_map.json"))
        self.assertEqual(code, req_coverage.EXIT_UNCOVERED)
        totals = {r["family"]: r["total"] for r in self.of_type(records, "summary")}
        self.assertEqual(totals, {"normative": 6, "feature": 2, "behaviour": 2, "conformance": 2, "budget": 4,
                                  "wit": 3, "port": 1})
        self.assertEqual(self.summary(records, "feature")["retired"], ["F-102"])
        self.assertEqual(sorted(self.summary(records, "wit")["uncovered"]),
                         ["WIT:ilium:host/log.sink.constructor", "WIT:ilium:host/log.sink.flush",
                          "WIT:ilium:host/log.write"])
        lanes = {r["id"]: r["lane"] for r in self.of_type(records, "uncovered")}
        self.assertEqual(lanes["chatroom-B02"], "core-automation")
        self.assertEqual(lanes["WIT:ilium:host/log.write"], "imports-core")
        self.assertEqual(lanes["CONF-SEC-02"], "conformance")
        self.assertEqual(lanes["B24"], "perf-budgets")
        self.assertEqual(lanes["CLI-COLD"], "perf-budgets")
        self.assertEqual(lanes["F-102"], "port-compat")
        self.assertEqual(lanes["HST-002"], "host-runtime")
        self.assertEqual(lanes["PORT:ilium-server/tests/a.rs::works"], "core-automation")

    def test_extra_gaps_fail_only_with_strict_extra(self) -> None:
        write(self.root, "crate/tests/all.rs", """
/// REQ: HST-001, HST-002, HST-004, HST-005, USR-110
#[test]
fn normative() {}
""")
        code, _ = self.run_script("--families", "all")
        self.assertEqual(code, req_coverage.EXIT_OK)
        code, _ = self.run_script("--families", "all", "--strict-extra")
        self.assertEqual(code, req_coverage.EXIT_UNCOVERED)

    def test_extra_ids_are_known_when_tagged(self) -> None:
        write(self.root, "crate/tests/extra.rs", """
/// REQ: F-001, chatroom-B01, CONF-MAN-01, B1, HOST-START, WIT:ilium:host/log.write
/// REQ: PORT:ilium-server/tests/a.rs::works
#[test]
fn extras() {}
""")
        _, records = self.run_script("--families", "all")
        self.assertEqual(self.of_type(records, "error"), [])
        self.assertEqual(self.summary(records, "behaviour")["uncovered"], ["chatroom-B02"])

    def test_missing_port_map_is_a_warning(self) -> None:
        (self.root / "tools/test_port_map.tsv").unlink()
        _, records = self.run_script("--families", "all")
        self.assertIn("missing-port-map", [r["code"] for r in self.of_type(records, "warning")])


class UsageTests(CoverageCase):
    def test_missing_manual_is_usage_error(self) -> None:
        buffer = io.StringIO()
        with redirect_stdout(buffer):
            code = req_coverage.main(["--manual", str(self.base / "nope"), "--root", str(self.root)])
        self.assertEqual(code, req_coverage.EXIT_USAGE)

    def test_bad_lane_map_is_usage_error(self) -> None:
        write(self.base, "bad.json", "[]")
        code, _ = self.run_script("--lane-map", str(self.base / "bad.json"))
        self.assertEqual(code, req_coverage.EXIT_USAGE)

    def test_bad_arguments(self) -> None:
        with redirect_stdout(io.StringIO()), unittest.mock.patch("sys.stderr", io.StringIO()):
            self.assertEqual(req_coverage.main(["--families", "some"]), req_coverage.EXIT_USAGE)


if __name__ == "__main__":
    unittest.main()
