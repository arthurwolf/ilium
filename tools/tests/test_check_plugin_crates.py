"""Unit tests of tools/check_plugin_crates.py on synthetic workspaces.

Run with: python3 -m unittest discover tools/tests
"""

import importlib.util
import io
import json
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "check_plugin_crates.py"
SPEC = importlib.util.spec_from_file_location("check_plugin_crates", SCRIPT)
check = importlib.util.module_from_spec(SPEC)
sys.modules["check_plugin_crates"] = check
SPEC.loader.exec_module(check)

LOCK = """
[[package]]
name = "regex"
version = "1.13.0"

[[package]]
name = "walkdir"
version = "2.5.0"

[[package]]
name = "chrono"
version = "0.4.45"

[[package]]
name = "image"
version = "0.25.10"

[[package]]
name = "proptest"
version = "1.12.0"
"""

ALLOWLIST = {
    "regex": {"versions": ["1.13.0"], "wasm_safe": True},
    "walkdir": {"versions": ["2.5.0"], "wasm_safe": False, "note": "use the `fs` import walk"},
    "chrono": {
        "versions": ["0.4.45"],
        "wasm_safe": True,
        "default_features": False,
        "required_features": ["alloc"],
        "forbidden_features": ["clock"],
    },
    "image": {
        "versions": ["0.25.10"],
        "wasm_safe": True,
        "default_features": False,
        "required_features": ["png"],
    },
}


def run_tool(dependencies, dev_dependencies=""):
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        (root / "plugins/core/demo").mkdir(parents=True)
        (root / "plugins/core/demo/Cargo.toml").write_text(
            '[package]\nname = "demo"\n\n[dependencies]\n'
            + dependencies
            + "\n[dev-dependencies]\n"
            + dev_dependencies
        )
        (root / "Cargo.lock").write_text(LOCK)
        (root / "allow.json").write_text(json.dumps(ALLOWLIST))
        buffer = io.StringIO()
        with redirect_stdout(buffer):
            code = check.run(
                root, root / "allow.json", root / "Cargo.lock", "plugins/core/*/Cargo.toml"
            )
        rows = [json.loads(line) for line in buffer.getvalue().splitlines()]
        return code, rows


def rules(rows):
    return sorted(row["rule"] for row in rows if row["type"] == "violation")


class VersionMatching(unittest.TestCase):
    def test_caret_and_tilde(self):
        self.assertTrue(check.satisfies("1.13.0", "1.12.3"))
        self.assertFalse(check.satisfies("2.0.0", "1.12.3"))
        self.assertTrue(check.satisfies("0.4.45", "0.4"))
        self.assertFalse(check.satisfies("0.5.0", "0.4.1"))
        self.assertTrue(check.satisfies("0.25.10", "0.25"))
        self.assertTrue(check.satisfies("1.2.9", "~1.2.3"))
        self.assertFalse(check.satisfies("1.3.0", "~1.2.3"))

    def test_exact_and_ranges(self):
        self.assertTrue(check.satisfies("0.1.1", "=0.1.1"))
        self.assertFalse(check.satisfies("0.1.2", "=0.1.1"))
        self.assertTrue(check.satisfies("1.5.0", ">=1.2, <2"))
        self.assertFalse(check.satisfies("2.0.0", ">=1.2, <2"))


class Checks(unittest.TestCase):
    def test_allowed_dependency_passes(self):
        code, rows = run_tool('regex = "1.13.0"\n')
        self.assertEqual(code, 0)
        self.assertEqual(rules(rows), [])
        self.assertEqual(rows[-1], {"type": "result", "exit_code": 0})

    def test_unlisted_crate_is_refused(self):
        code, rows = run_tool('proptest = "1"\n')
        self.assertEqual(code, 1)
        self.assertEqual(rules(rows), ["not-allowed"])

    def test_crate_missing_from_lock_is_refused(self):
        code, rows = run_tool('regex = "9.0.0"\n')
        self.assertEqual(code, 1)
        self.assertIn("not-in-lock", rules(rows))

    def test_walkdir_is_not_wasm_safe(self):
        code, rows = run_tool('walkdir = "2.5.0"\n')
        self.assertEqual(code, 1)
        self.assertEqual(rules(rows), ["not-wasm-safe"])
        self.assertIn("fs", rows[0]["detail"])

    def test_features_and_default_features(self):
        good = 'chrono = { version = "0.4.45", default-features = false, features = ["alloc"] }\n'
        self.assertEqual(run_tool(good)[0], 0)
        wrong = 'chrono = { version = "0.4.45", features = ["alloc", "clock"] }\n'
        code, rows = run_tool(wrong)
        self.assertEqual(code, 1)
        self.assertEqual(rules(rows), ["default-features", "forbidden-features"])

    def test_required_feature_missing(self):
        code, rows = run_tool('image = { version = "0.25", default-features = false }\n')
        self.assertEqual(code, 1)
        self.assertEqual(rules(rows), ["required-features"])

    def test_dev_dependency_only_needs_the_lock(self):
        code, rows = run_tool('regex = "1.13.0"\n', 'proptest = "1"\n')
        self.assertEqual(code, 0, rows)
        code, rows = run_tool("", 'missing-crate = "1"\n')
        self.assertEqual(rules(rows), ["not-in-lock"])

    def test_no_manifest_is_a_usage_error(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.lock").write_text(LOCK)
            (root / "allow.json").write_text("{}")
            with redirect_stdout(io.StringIO()):
                code = check.run(root, root / "allow.json", root / "Cargo.lock", "plugins/*/Cargo.toml")
        self.assertEqual(code, 2)


if __name__ == "__main__":
    unittest.main()
