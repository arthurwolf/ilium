"""The shared CLI must preserve JSONL errors across imported Pages policy."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class ReleaseCliTests(unittest.TestCase):
    def test_pages_policy_errors_are_jsonl_without_traceback(self):
        root = Path(__file__).resolve().parents[2]
        script = root / "release/scripts/release_tool.py"
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "new-output"
            commands = [
                ["build-pages", "--manifest", str(root / "release/targets.toml"), "--output", str(output), "--release-tag", "invalid", "--release-sha256sums", str(Path(temporary) / "missing")],
                ["verify-pages", "--manifest", str(root / "release/targets.toml"), "--directory", temporary],
            ]
            for arguments in commands:
                with self.subTest(command=arguments[0]):
                    result = subprocess.run([sys.executable, str(script), *arguments], capture_output=True, text=True)
                    self.assertEqual(result.returncode, 2)
                    self.assertEqual(result.stderr, "")
                    self.assertEqual(json.loads(result.stdout)["type"], "error")
                    self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
