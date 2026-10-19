import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(TOOLS))
import classify_existing_tests as cet  # noqa: E402

TRACE = """| Id | Feature | User doc | Kind | Owner | Side |
|---|---|---|---|---|---|
| F-001 | a | `x` | native-kernel | kernel tree model; tree plugin | both |
| F-002 | b | `x` | plugin-wasm | cost plugin | both |
| F-003 | c | `x` | plugin-wasm | keys-and-help and settings | both |
"""
LEDGER = "file\ttest\tattr\tignored\towner\tclass\n" + "".join(
    f"{f}\t{t}\tplain\t\t{o}\t{c}\n"
    for f, t, o, c in [
        ("a.rs", "t1", "tree", "adapt-to-plugin-api"),
        ("a.rs", "t2", "tree", "moves-to-plugin"),
        ("b.rs", "t3", "kernel", "kernel-stays"),
        ("c.rs", "t4", "cost", "obsolete"),
    ]
)


class ClassifyTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        base = Path(self.dir.name)
        (base / "plugins" / "tree").mkdir(parents=True)
        (base / "plugins" / "cost").mkdir()
        (base / "plugins" / "keys-and-help").mkdir()
        (base / "plugins" / "settings").mkdir()
        (base / "trace.md").write_text(TRACE)
        self.base = base

    def tearDown(self):
        self.dir.cleanup()

    def run_tool(self, ledger_text):
        (self.base / "ledger.tsv").write_text(ledger_text)
        out = self.base / "out.tsv"
        proc = subprocess.run(
            [sys.executable, str(TOOLS / "classify_existing_tests.py"),
             "--ledger", str(self.base / "ledger.tsv"), "--traceability", str(self.base / "trace.md"),
             "--plugins-dir", str(self.base / "plugins"), "--out", str(out)],
            capture_output=True, text=True)
        return proc, out

    def test_owner_tokens_skip_kernel_part(self):
        names = ["tree", "cost"]
        self.assertEqual(cet.owner_tokens("kernel tree model; tree plugin", names), {"kernel", "tree"})
        self.assertEqual(cet.owner_tokens("kernel", names), {"kernel"})

    def test_owner_tokens_hyphenated_names(self):
        names = ["keys-and-help", "settings", "help"]
        self.assertEqual(cet.owner_tokens("keys-and-help and settings", names), {"keys-and-help", "settings"})

    def test_columns_targets_and_status(self):
        proc, out = self.run_tool(LEDGER)
        self.assertEqual(proc.returncode, 0, proc.stdout)
        rows = [l.split("\t") for l in out.read_text().splitlines()]
        self.assertEqual(rows[0][-3:], ["target_crate", "feature_ids", "status"])
        by_test = {r[1]: r for r in rows[1:]}
        self.assertEqual(by_test["t1"][6], "plugins/core/tree")
        self.assertEqual(by_test["t2"][6], "plugins/core/tree")
        self.assertEqual(by_test["t3"][6], "")
        self.assertEqual(by_test["t4"][6], "")
        self.assertEqual(by_test["t1"][7], "F-001")
        self.assertTrue(all(r[8] == "pending" for r in rows[1:]))

    def test_totals_are_jsonl(self):
        proc, _ = self.run_tool(LEDGER)
        records = [json.loads(l) for l in proc.stdout.splitlines()]
        classes = {r["class"]: r["tests"] for r in records if "class" in r}
        owners = {r["owner"]: r["tests"] for r in records if "owner" in r}
        self.assertEqual(classes["adapt-to-plugin-api"], 1)
        self.assertEqual(owners["tree"], 2)

    def test_unknown_owner_exits_one(self):
        proc, _ = self.run_tool(LEDGER + "d.rs\tt5\tplain\t\tbogus\tkernel-stays\n")
        self.assertEqual(proc.returncode, 1)
        self.assertIn("unknown-owner", proc.stdout)

    def test_deterministic(self):
        _, out = self.run_tool(LEDGER)
        first = out.read_text()
        self.run_tool(LEDGER)
        self.assertEqual(first, out.read_text())


if __name__ == "__main__":
    unittest.main()
