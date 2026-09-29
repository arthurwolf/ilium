"""Licence receipts must bind locked package identities to actual source bytes."""
import importlib.util
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import subprocess
import sys
import tarfile
import os

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))


class LicenceInventoryTests(unittest.TestCase):
    def test_utf8_manifests_do_not_depend_on_runner_locale(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            registry, package = self.fixture(root)
            metadata = package / "Cargo.toml"
            metadata.write_bytes(metadata.read_bytes() + 'description="Café fixture"\n'.encode("utf-8"))
            (package / "LICENSE").write_bytes("MIT License\nCopyright Café fixture\n".encode("utf-8"))
            self.checksum(package, {})
            script = Path(__file__).resolve().parents[1] / "scripts/licence_inventory.py"
            result = subprocess.run(
                [sys.executable, str(script), "--workspace", str(root), "--registry-source", str(registry),
                 "--notice-directory", str(root / "notices"), "--output", str(root / "inventory.json")],
                env=os.environ | {"LC_ALL": "C", "PYTHONUTF8": "0", "PYTHONCOERCECLOCALE": "0"},
                capture_output=True, text=True, encoding="utf-8",
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            inventory = json.loads((root / "inventory.json").read_text(encoding="utf-8"))
            notice = Path(inventory["packages"][0]["license_file"]).read_text(encoding="utf-8")
            self.assertIn("Café fixture", notice)

    def test_cli_help_and_invalid_arguments_are_jsonl(self):
        script = Path(__file__).resolve().parents[1] / "scripts/licence_inventory.py"
        for arguments, code in ((["--help"], 0), ([], 2)):
            result = subprocess.run([sys.executable, str(script), *arguments], capture_output=True, text=True)
            self.assertEqual(result.returncode, code)
            self.assertEqual(result.stderr, "")
            records = [json.loads(line) for line in result.stdout.splitlines()]
            self.assertEqual(len(records), 1)
            self.assertIn(records[0]["type"], ("result", "error"))

    def module(self):
        path = Path(__file__).resolve().parents[1] / "scripts/licence_inventory.py"
        self.assertTrue(path.is_file(), "licence inventory generator is missing")
        spec = importlib.util.spec_from_file_location("licence_inventory", path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def fixture(self, root):
        (root / "Cargo.toml").write_text('[workspace]\nmembers=[]\n[workspace.package]\nlicense="MIT"\n')
        (root / "Cargo.lock").write_text('version=4\n[[package]]\nname="fixture"\nversion="1.0.0"\nsource="registry+https://github.com/rust-lang/crates.io-index"\nchecksum="' + 'a' * 64 + '"\n')
        registry = root / "registry/src/test-index"
        package = registry / "fixture-1.0.0"
        package.mkdir(parents=True)
        (package / "Cargo.toml").write_text('[package]\nname="fixture"\nversion="1.0.0"\nlicense="MIT"\n')
        self.checksum(package, {})
        return registry, package

    def checksum(self, package, files):
        files["Cargo.toml"] = hashlib.sha256((package / "Cargo.toml").read_bytes()).hexdigest()
        registry = package.parent
        archive = registry.parent.parent / "cache" / registry.name / (package.name + ".crate")
        archive.parent.mkdir(parents=True, exist_ok=True)
        with tarfile.open(archive, "w:gz") as output:
            for path in sorted(package.iterdir()):
                if path.name != ".cargo-checksum.json":
                    output.add(path, arcname=package.name + "/" + path.name)
        hash_value = hashlib.sha256(archive.read_bytes()).hexdigest()
        lock = registry.parents[2] / "Cargo.lock"
        text = lock.read_text()
        import re
        lock.write_text(re.sub(r'checksum="[0-9a-f]{64}"', 'checksum="' + hash_value + '"', text))
        (package / ".cargo-checksum.json").write_text(json.dumps({"package": hash_value, "files": files}))

    def test_forged_unpacked_checksum_cannot_override_locked_archive(self):
        module = self.module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            registry, package = self.fixture(root)
            (package / "LICENSE").write_text("Original publisher notice")
            self.checksum(package, {"LICENSE": module.digest((package / "LICENSE").read_bytes())})
            (package / "LICENSE").write_text("Forged notice")
            checksum = json.loads((package / ".cargo-checksum.json").read_text())
            checksum["files"]["LICENSE"] = module.digest((package / "LICENSE").read_bytes())
            (package / ".cargo-checksum.json").write_text(json.dumps(checksum))
            with self.assertRaisesRegex(ValueError, "checksum"):
                module.build_inventory(root, registry, None, root / "evidence")

    def test_tampered_registry_declaration_is_rejected(self):
        module = self.module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            registry, package = self.fixture(root)
            text = b"MIT License\nCopyright fixture owner\nPermission is hereby granted.\n"
            (package / "LICENSE").write_bytes(text)
            self.checksum(package, {"LICENSE": module.digest(text)})
            metadata = package / "Cargo.toml"
            metadata.write_text(metadata.read_text().replace('license="MIT"', 'license="Unverified"'))
            with self.assertRaisesRegex(ValueError, "metadata checksum"):
                module.build_inventory(root, registry, None, root / "evidence")

    def test_missing_licence_cannot_produce_reviewed_receipt(self):
        module = self.module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            registry, _ = self.fixture(root)
            with self.assertRaisesRegex(ValueError, "licence"):
                module.build_inventory(root, registry, None, root / "evidence")

    def test_tampered_registry_licence_is_rejected(self):
        module = self.module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            registry, package = self.fixture(root)
            (package / "LICENSE").write_text("Original publisher notice")
            self.checksum(package, {"LICENSE": module.digest((package / "LICENSE").read_bytes())})
            (package / "LICENSE").write_text("changed licence bytes")
            with self.assertRaisesRegex(ValueError, "checksum"):
                module.build_inventory(root, registry, None, root / "evidence")

    def test_complete_receipt_contains_real_notice_and_lock_identity(self):
        module = self.module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            registry, package = self.fixture(root)
            text = b"MIT License\nCopyright fixture owner\nPermission is hereby granted.\n"
            (package / "LICENSE").write_bytes(text)
            self.checksum(package, {"LICENSE": module.digest(text)})
            result = module.build_inventory(root, registry, None, root / "evidence")
            self.assertEqual(result["state"], "reviewed")
            self.assertEqual(result["lock_sha256"], module.digest((root / "Cargo.lock").read_bytes()))
            record = result["packages"][0]
            self.assertIn(text, Path(record["license_file"]).read_bytes())
            self.assertEqual(record["license_sha256"], module.digest(Path(record["license_file"]).read_bytes()))

    def test_external_register_cannot_escape_licence_root(self):
        module = self.module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            registry, _ = self.fixture(root)
            register = root / "sources.json"
            register.write_text(json.dumps({"schema": 1, "packages": {"fixture@1.0.0": {"file": "../outside", "sha256": "a" * 64, "source": "https://example.invalid/LICENSE"}}}))
            with self.assertRaisesRegex(ValueError, "escape|unsafe"):
                module.build_inventory(root, registry, register, root / "evidence")

    def test_nested_vendored_path_dependency_is_included(self):
        module = self.module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text('[workspace]\nmembers=["client"]\n[workspace.package]\nversion="1.0.0"\nlicense="MIT"\n')
            client = root / "client"
            client.mkdir()
            (client / "Cargo.toml").write_text('[package]\nname="client"\nversion.workspace=true\nlicense.workspace=true\n[dependencies]\nwidget={path="vendor/widget"}\n')
            widget = client / "vendor/widget"
            widget.mkdir(parents=True)
            (widget / "Cargo.toml").write_text('[package]\nname="widget"\nversion="2.0.0"\nlicense="MIT"\n')
            self.assertIn(("widget", "2.0.0"), module.local_packages(root))


if __name__ == "__main__":
    unittest.main()
