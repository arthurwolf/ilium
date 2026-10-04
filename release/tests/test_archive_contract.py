"""Synthetic archive fixtures exercise packaging without executing archive bytes."""

import gzip
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "release/scripts"
sys.path.insert(0, str(SCRIPTS))
import release_tool as tool


class ArchiveContractTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="ilium-archive-fixture-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.directory = self.root / "candidate"
        self.directory.mkdir()
        self.workspace = self.root / "Cargo.toml"
        self.workspace.write_text('[workspace.package]\nversion = "0.1.0"\n')
        self.target = tool.load_targets(ROOT / "release/targets.toml")[0]
        self.files = {"ilium": b"synthetic client; never execute", "ilium-server": b"synthetic server; never execute", "ilium-animation-helper": b"synthetic helper; never execute", "VERSION": b"0.1.0\n", "THIRD-PARTY.txt": b"Reviewed fixture licence text\n"}
        self.files.update({name: (ROOT / "ilium-animation-js/assets/packages" / name).read_bytes() for name in tool.APPROVED_PACKAGES})
        for name, content in self.files.items():
            (self.directory / name).write_bytes(content)
        self.receipt = self.root / "audit.json"
        self.write_receipt()

    def write_receipt(self, **changes):
        receipt = {
            "schema": 1, "state": "passed", "publication_allowed": True,
            "target": self.target["rust_target"], "tag": "v0.1.0", "version": "0.1.0",
            "os": self.target["os"], "arch": self.target["arch"],
            "files": {name: hashlib.sha256(content).hexdigest() for name, content in self.files.items()},
            "binary_versions": {"ilium": "ilium 0.1.0", "ilium-server": "ilium-server 0.1.0", "ilium-animation-helper": "ilium-animation-helper 0.1.0"},
            "native_identity": {"system": "Linux", "machine": "x86_64", "runner": "fixture"},
            "dependency_closure": {"complete": True},
            "notices": {"state": "reviewed", "sha256": hashlib.sha256(self.files["THIRD-PARTY.txt"]).hexdigest()},
        }
        receipt.update(changes)
        self.receipt.write_text(json.dumps(receipt))

    def invoke(self, command, archive, *extra):
        arguments = [sys.executable, "-B", str(SCRIPTS / "release_tool.py"), command,
                     "--manifest", str(ROOT / "release/targets.toml"), "--target", self.target["rust_target"],
                     "--workspace", str(self.workspace), "--tag", "v0.1.0", "--audit-report", str(self.receipt)]
        if command == "package":
            arguments += ["--directory", str(self.directory), "--output", str(archive)]
        else:
            arguments += ["--archive", str(archive), "--checksums", str(self.checksums)]
        result = subprocess.run(arguments + list(map(str, extra)), capture_output=True, text=True)
        records = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertTrue(records, result.stderr)
        self.assertTrue(all("type" in record for record in records))
        return result, records[-1]

    def archive(self, members=None, mutations=None):
        path = self.root / self.target["archive"]
        prefix = "ilium-linux-x86_64"
        members = members or [(prefix, None)] + [(f"{prefix}/{name}", content) for name, content in sorted(self.files.items())]
        with path.open("wb") as raw:
            with gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=0) as compressed:
                with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as archive:
                    for index, (name, content) in enumerate(members):
                        info = tarfile.TarInfo(name)
                        info.mode = 0o755 if content is None or name.rsplit("/", 1)[-1] in self.target["executables"] else 0o644
                        info.type = tarfile.DIRTYPE if content is None else tarfile.REGTYPE
                        if mutations:
                            mutations(index, info)
                        info.size = len(content) if content is not None else 0
                        archive.addfile(info, io.BytesIO(content) if content is not None else None)
        self.checksums = self.root / "SHA256SUMS"
        # All five names must appear, even when this test verifies only one archive.
        hashes = [(hashlib.sha256(path.read_bytes()).hexdigest() if row["archive"] == path.name else "a" * 64, row["archive"]) for row in tool.load_targets(ROOT / "release/targets.toml")]
        self.checksums.write_text("".join(f"{digest}  {name}\n" for digest, name in hashes))
        return path

    def test_packaging_is_reproducible_and_verification_never_executes_members(self):
        path = self.root / self.target["archive"]
        first, record = self.invoke("package", path)
        self.assertEqual(first.returncode, 0, record)
        bytes_first = path.read_bytes()
        (self.directory / "ilium").chmod(0o600)
        second, record = self.invoke("package", path)
        self.assertEqual(second.returncode, 0, record)
        self.assertEqual(path.read_bytes(), bytes_first)
        self.archive()
        verified, record = self.invoke("verify-package", path)
        self.assertEqual(verified.returncode, 0, record)
        self.assertEqual(record["state"], "passed")

    def test_hostile_member_inventory_is_rejected(self):
        base = [("ilium-linux-x86_64", None)] + [(f"ilium-linux-x86_64/{name}", content) for name, content in sorted(self.files.items())]
        cases = {
            "missing pair": base[:-1], "duplicate": base + [base[-1]],
            "traversal": base + [("ilium-linux-x86_64/../escape", b"x")],
            "absolute": base + [("/absolute", b"x")],
            "backslash": base + [("ilium-linux-x86_64\\escape", b"x")],
            "extra": base + [("ilium-linux-x86_64/unreviewed.so", b"x")],
            "wrong VERSION": [(name, b"0.2.0\n" if name.endswith("/VERSION") else content) for name, content in base],
            "wrong order": list(reversed(base)),
        }
        for reason, members in cases.items():
            with self.subTest(reason=reason):
                result, record = self.invoke("verify-package", self.archive(members))
                self.assertNotEqual(result.returncode, 0, reason)
                self.assertEqual(record["type"], "error")

    def test_missing_or_tampered_official_animation_and_helper_are_rejected(self):
        prefix = "ilium-linux-x86_64"
        for name in ("ilium-animation-helper", *tool.APPROVED_PACKAGES):
            with self.subTest(name=name):
                members = [(prefix, None)] + [(f"{prefix}/{member}", content) for member, content in sorted(self.files.items()) if member != name]
                result, _record = self.invoke("verify-package", self.archive(members))
                self.assertNotEqual(result.returncode, 0)
        for name in tool.APPROVED_PACKAGES:
            with self.subTest(tampered=name):
                original = self.files[name]
                self.files[name] = original + b"tampered"
                self.write_receipt()
                result, record = self.invoke("package", self.root / self.target["archive"])
                self.assertNotEqual(result.returncode, 0, record)
                self.files[name] = original
                self.write_receipt()

    def test_links_special_files_and_nondeterministic_metadata_are_rejected(self):
        for attribute, value in (("type", tarfile.SYMTYPE), ("type", tarfile.LNKTYPE), ("type", tarfile.FIFOTYPE), ("mtime", 10), ("uid", 42), ("mode", 0o777)):
            with self.subTest(attribute=attribute, value=value):
                def mutation(index, info):
                    if index == 1:
                        setattr(info, attribute, value)
                        if attribute == "type":
                            info.linkname = "../../escape"
                result, record = self.invoke("verify-package", self.archive(mutations=mutation))
                self.assertNotEqual(result.returncode, 0, record)

    def test_tag_workspace_native_versions_and_hashes_must_all_agree(self):
        path = self.archive()
        for changes in ({"tag": "v0.2.0"}, {"version": "0.2.0"}, {"binary_versions": {"ilium": "ilium 0.1.0", "ilium-server": "ilium-server 0.2.0"}}, {"files": {name: "a" * 64 for name in self.files}}, {"state": "blocked"}, {"publication_allowed": False}, {"dependency_closure": {"complete": False}}, {"notices": {"state": "blocked"}}):
            with self.subTest(changes=changes):
                self.write_receipt(**changes)
                result, record = self.invoke("verify-package", path)
                self.assertNotEqual(result.returncode, 0, record)
        self.write_receipt()
        self.workspace.write_text('[workspace.package]\nversion = "0.2.0"\n')
        result, record = self.invoke("verify-package", path)
        self.assertNotEqual(result.returncode, 0, record)

    def test_malformed_receipt_types_are_json_errors(self):
        path = self.archive()
        for document in ([], {"schema": 1, "state": "passed", "publication_allowed": True, "native_identity": None}):
            with self.subTest(document=document):
                self.receipt.write_text(json.dumps(document))
                result, record = self.invoke("verify-package", path)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(record["type"], "error")

    def test_checked_in_blocked_notices_cannot_be_packaged_with_a_passed_receipt(self):
        self.files["THIRD-PARTY.txt"] = (ROOT / "release/THIRD-PARTY.txt").read_bytes()
        (self.directory / "THIRD-PARTY.txt").write_bytes(self.files["THIRD-PARTY.txt"])
        self.write_receipt()
        result, record = self.invoke("package", self.root / self.target["archive"])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("blocked", record["error"])

    def test_incomplete_duplicate_and_wrong_checksums_are_rejected(self):
        path = self.archive()
        original = self.checksums.read_text()
        for content in (original.splitlines()[0] + "\n", original + original.splitlines()[0] + "\n", original.replace(hashlib.sha256(path.read_bytes()).hexdigest(), "0" * 64), original + "0" * 64 + "  unexpected.zip\n"):
            with self.subTest(content=content):
                self.checksums.write_text(content)
                result, record = self.invoke("verify-package", path)
                self.assertNotEqual(result.returncode, 0, record)

    def test_failed_package_preserves_existing_output_and_rejects_symlinks(self):
        path = self.root / self.target["archive"]
        path.write_bytes(b"existing archive")
        (self.directory / "unreviewed.dll").write_bytes(b"x")
        result, _ = self.invoke("package", path)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(path.read_bytes(), b"existing archive")
        (self.directory / "unreviewed.dll").unlink()
        (self.directory / "ilium").unlink()
        (self.directory / "ilium").symlink_to(self.directory / "ilium-server")
        result, _ = self.invoke("package", path)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(path.read_bytes(), b"existing archive")

    def test_hidden_bytes_after_tar_end_are_not_a_valid_archive(self):
        path = self.archive()
        plain = gzip.decompress(path.read_bytes())
        # A reader stopping at the first tar terminator would miss this payload.
        path.write_bytes(gzip.compress(plain + b"hidden unexpected archive payload", mtime=0))
        original = self.checksums.read_text()
        first_name = self.target["archive"]
        self.checksums.write_text("\n".join((hashlib.sha256(path.read_bytes()).hexdigest() + "  " + first_name) if line.endswith("  " + first_name) else line for line in original.splitlines()) + "\n")
        result, record = self.invoke("verify-package", path)
        self.assertNotEqual(result.returncode, 0, record)

    def test_windows_zip_has_fixed_order_modes_and_timestamp(self):
        self.target = tool.load_targets(ROOT / "release/targets.toml")[2]
        self.files = {"ilium.exe": b"client", "ilium-server.exe": b"server", "ilium-animation-helper.exe": b"helper", "VERSION": b"0.1.0\n", "THIRD-PARTY.txt": b"Reviewed fixture licence text\n"}
        self.files.update({name: (ROOT / "ilium-animation-js/assets/packages" / name).read_bytes() for name in tool.APPROVED_PACKAGES})
        for path in self.directory.iterdir():
            path.unlink()
        for name, content in self.files.items():
            (self.directory / name).write_bytes(content)
        self.write_receipt(os="windows", arch="x86_64", binary_versions={"ilium.exe": "ilium 0.1.0", "ilium-server.exe": "ilium-server 0.1.0", "ilium-animation-helper.exe": "ilium-animation-helper 0.1.0"}, native_identity={"system": "Windows", "machine": "AMD64", "runner": "fixture"}, windows_ort={"state": "passed", "source_tag": "v1.24.2", "source_commit": "058787ceead760166e3c50a0a4cba8a833a6f53f", "source_sha256": "a" * 64, "rust_crt": "static", "ort_crt": "static"})
        path = self.root / self.target["archive"]
        result, record = self.invoke("package", path)
        self.assertEqual(result.returncode, 0, record)
        with zipfile.ZipFile(path) as archive:
            self.assertEqual(archive.namelist(), ["ilium-windows-x86_64/"] + [f"ilium-windows-x86_64/{name}" for name in sorted(self.files)])
            self.assertTrue(all(member.date_time == (1980, 1, 1, 0, 0, 0) for member in archive.infolist()))


if __name__ == "__main__":
    unittest.main()
