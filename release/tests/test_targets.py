"""Run the manifest consumer against the release policy and hostile inputs."""

import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest


PROJECT_ROOT = Path(__file__).resolve().parents[2]
MANIFEST = PROJECT_ROOT / "release" / "targets.toml"
TOOL = PROJECT_ROOT / "release" / "scripts" / "release_tool.py"
sys.path.insert(0, str(TOOL.parent))
import release_tool as tool


def manifest_text(document):
    """Serialize only test fixture values; the production reader uses tomllib."""
    lines = []
    for key, value in document.items():
        if key != "target":
            lines.append(f"{key} = {json.dumps(value)}")
    for target in document.get("target", []):
        lines.append("[[target]]")
        lines.extend(f"{key} = {json.dumps(value)}" for key, value in target.items())
    return "\n".join(lines) + "\n"


class TargetManifestTests(unittest.TestCase):
    def invoke(self, *arguments, cwd=None):
        return subprocess.run(
            [sys.executable, "-B", str(TOOL), *map(str, arguments)],
            cwd=cwd,
            text=True,
            capture_output=True,
            check=False,
        )

    def output_record(self, result):
        self.assertEqual(len(result.stdout.splitlines()), 1, result)
        record = json.loads(result.stdout)
        self.assertIn("type", record)
        return record

    def document(self):
        self.assertTrue(MANIFEST.is_file(), "release target manifest is missing")
        with MANIFEST.open("rb") as manifest_file:
            return tomllib.load(manifest_file)

    def rejects_document(self, document, reason):
        with tempfile.TemporaryDirectory(prefix="ilium-targets-test-") as directory:
            manifest = Path(directory) / "targets.toml"
            manifest.write_text(manifest_text(document), encoding="utf-8")
            before = manifest.read_bytes()
            result = self.invoke("targets", "--manifest", manifest, cwd=directory)
            self.assertNotEqual(result.returncode, 0, reason)
            record = self.output_record(result)
            self.assertEqual(record["type"], "error")
            self.assertTrue(record["error"], reason)
            self.assertEqual(manifest.read_bytes(), before)
            self.assertEqual(list(Path(directory).iterdir()), [manifest])

    def test_actual_manifest_emits_exactly_five_approved_records(self):
        result = self.invoke("targets", "--manifest", MANIFEST)
        self.assertEqual(result.returncode, 0, result.stderr)
        record = self.output_record(result)
        self.assertEqual(record["type"], "result")
        self.assertEqual(record["command"], "targets")
        self.assertEqual(record["manifest"], str(MANIFEST))
        self.assertEqual(len(record["targets"]), 5)
        expected = [
            ("linux", "x86_64", "x86_64-unknown-linux-gnu", "ubuntu-22.04", "ilium-linux-x86_64.tar.gz", "tar.gz", ["ilium", "ilium-server"], "upstream-prebuilt"),
            ("linux", "aarch64", "aarch64-unknown-linux-gnu", "ubuntu-22.04-arm", "ilium-linux-aarch64.tar.gz", "tar.gz", ["ilium", "ilium-server"], "upstream-prebuilt"),
            ("windows", "x86_64", "x86_64-pc-windows-msvc", "windows-2022", "ilium-windows-x86_64.zip", "zip", ["ilium.exe", "ilium-server.exe"], "pinned-source-build"),
            ("macos", "aarch64", "aarch64-apple-darwin", "macos-15", "ilium-macos-aarch64.tar.gz", "tar.gz", ["ilium", "ilium-server"], "upstream-prebuilt"),
            ("macos", "x86_64", "x86_64-apple-darwin", "macos-15-intel", "ilium-macos-x86_64.tar.gz", "tar.gz", ["ilium", "ilium-server"], "pinned-source-build"),
        ]
        for target, row in zip(record["targets"], expected):
            with self.subTest(target=target["rust_target"]):
                fields = ("os", "arch", "rust_target", "runner", "archive", "format", "executables", "ort_strategy")
                self.assertEqual(tuple(target[field] for field in fields), row)
                self.assertEqual(target["minimum_tested_os"], row[3])
                self.assertEqual(set(target), {*fields, "minimum_tested_os"})

    def test_removed_intel_row_is_rejected(self):
        document = self.document()
        document["target"] = document["target"][:-1]
        self.rejects_document(document, "Intel macOS must remain in the release")

    def test_added_windows_arm64_row_is_rejected(self):
        document = self.document()
        row = copy.deepcopy(document["target"][2])
        row.update(arch="aarch64", rust_target="aarch64-pc-windows-msvc", archive="ilium-windows-aarch64.zip", runner="windows-11-arm")
        document["target"].append(row)
        self.rejects_document(document, "Windows ARM64 is not approved")

    def test_windows_package_receipt_requires_static_crt_source_provenance(self):
        target = next(row for row in tool.load_targets(MANIFEST) if row["os"] == "windows")
        receipt = {
            "schema": 1, "state": "passed", "publication_allowed": True,
            "target": target["rust_target"], "tag": "v0.1.0", "version": "0.1.0",
            "os": "windows", "arch": "x86_64",
            "native_identity": {"system": "Windows", "machine": "AMD64", "runner": "windows-2022"},
            "dependency_closure": {"complete": True},
            "binary_versions": {"ilium.exe": "ilium 0.1.0", "ilium-server.exe": "ilium-server 0.1.0"},
            "files": {"ilium.exe": "a" * 64, "ilium-server.exe": "b" * 64,
                      "VERSION": "c" * 64, "THIRD-PARTY.txt": "d" * 64,
                      "onnxruntime.dll": "e" * 64},
            "notices": {"state": "reviewed", "sha256": "d" * 64},
        }
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "audit.json"
            path.write_text(json.dumps(receipt))
            with self.assertRaisesRegex(ValueError, "Windows package lacks"):
                tool.audit_receipt(path, target, "0.1.0", "v0.1.0")
            receipt["windows_ort"] = {"state": "passed", "source_tag": "v1.24.2",
                                      "source_commit": "058787ceead760166e3c50a0a4cba8a833a6f53f",
                                      "source_sha256": "f" * 64, "rust_crt": "static", "ort_crt": "static"}
            path.write_text(json.dumps(receipt))
            self.assertEqual(tool.audit_receipt(path, target, "0.1.0", "v0.1.0"), receipt)

    def test_replaced_target_pair_is_rejected(self):
        document = self.document()
        document["target"][-1].update(os="windows", arch="aarch64")
        self.rejects_document(document, "five rows cannot substitute an unapproved target")

    def test_duplicate_identifiers_are_rejected(self):
        for field in ("rust_target", "runner", "archive"):
            with self.subTest(field=field):
                document = self.document()
                document["target"][1][field] = document["target"][0][field]
                self.rejects_document(document, f"duplicate {field}")
        document = self.document()
        document["target"][1].update(os="linux", arch="x86_64")
        self.rejects_document(document, "duplicate OS and architecture")

    def test_changed_ort_strategy_is_rejected_for_each_target(self):
        for index in range(5):
            document = self.document()
            document["target"][index]["ort_strategy"] = "unverified-system-library"
            self.rejects_document(document, "unapproved ONNX Runtime strategy")

    def test_changed_policy_fields_are_rejected(self):
        mutations = {
            "rust_target": "x86_64-unknown-linux-musl",
            "runner": "ubuntu-latest",
            "archive": "ilium-linux-amd64.tar.gz",
            "format": "zip",
            "executables": ["ilium"],
            "minimum_tested_os": "ubuntu-20.04",
        }
        for field, value in mutations.items():
            with self.subTest(field=field):
                document = self.document()
                document["target"][0][field] = value
                self.rejects_document(document, f"changed {field}")

    def test_unsafe_archive_names_are_rejected(self):
        for archive in ("../ilium.tar.gz", "/ilium.tar.gz", "dir/ilium.tar.gz", "dir\\ilium.zip", ".hidden.tar.gz", "ilium name.tar.gz", "ilium\nname.tar.gz", "ilium;touch-pwned.tar.gz"):
            with self.subTest(archive=archive):
                document = self.document()
                document["target"][0]["archive"] = archive
                self.rejects_document(document, "unsafe archive name")

    def test_unknown_and_missing_fields_are_rejected(self):
        document = self.document()
        document["unexpected"] = "value"
        self.rejects_document(document, "unknown top-level field")
        document = self.document()
        document["target"][0]["unexpected"] = "value"
        self.rejects_document(document, "unknown target field")
        for field in self.document()["target"][0]:
            with self.subTest(field=field):
                document = self.document()
                del document["target"][0][field]
                self.rejects_document(document, f"missing {field}")

    def test_wrong_field_types_are_rejected(self):
        for field in self.document()["target"][0]:
            with self.subTest(field=field):
                document = self.document()
                document["target"][0][field] = 42
                self.rejects_document(document, f"wrong type for {field}")

    def test_malformed_toml_and_missing_paths_emit_json_errors(self):
        with tempfile.TemporaryDirectory(prefix="ilium-targets-test-") as directory:
            manifest = Path(directory) / "targets.toml"
            for contents in (None, "[[target]]\nos = 'linux'\nos = 'macos'\n", "target = 'not a list'\n"):
                with self.subTest(contents=contents):
                    if contents is not None:
                        manifest.write_text(contents, encoding="utf-8")
                    result = self.invoke("targets", "--manifest", manifest)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(self.output_record(result)["type"], "error")

    def test_invalid_arguments_emit_json_errors(self):
        for arguments in ([], ["unknown"], ["targets"], ["targets", "--manifest", str(MANIFEST), "--unknown"]):
            with self.subTest(arguments=arguments):
                result = self.invoke(*arguments)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.output_record(result)["type"], "error")

    def test_invalid_utf8_emits_json_error(self):
        with tempfile.TemporaryDirectory(prefix="ilium-targets-test-") as directory:
            manifest = Path(directory) / "targets.toml"
            manifest.write_bytes(b"\xff\xfe")
            result = self.invoke("targets", "--manifest", manifest)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(self.output_record(result)["type"], "error")

    def test_help_is_jsonl(self):
        for arguments in (["--help"], ["targets", "--help"]):
            result = self.invoke(*arguments)
            self.assertEqual(result.returncode, 0)
            self.assertEqual(self.output_record(result)["type"], "result")

    def test_explicit_manifest_works_outside_repo_without_mutation(self):
        with tempfile.TemporaryDirectory(prefix="ilium-targets-test-") as directory:
            manifest = Path(directory) / "explicit.toml"
            manifest.write_bytes(MANIFEST.read_bytes())
            before = manifest.read_bytes()
            result = self.invoke("targets", "--manifest", manifest, cwd=directory)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(self.output_record(result)["type"], "result")
            self.assertEqual(manifest.read_bytes(), before)
            self.assertEqual(list(Path(directory).iterdir()), [manifest])


if __name__ == "__main__":
    unittest.main()
