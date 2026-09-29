"""Isolated POSIX installer contracts. Synthetic binaries never enter user data.

Run: python3 -B release/tests/test_posix_install.py --installer release/install.sh
     --manifest release/targets.toml [--filter SUBSTRING]
Every stdout record is JSONL; unittest details are retained on stderr.
"""

import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import shlex
import subprocess
import sys
import tarfile
import tempfile
import time
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "release/scripts"))
import release_tool

INSTALLER = ROOT / "release/install.sh"
MANIFEST = ROOT / "release/targets.toml"


class PosixInstallTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="ilium-posix-fixture-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.home = self.root / "home space é"
        self.home.mkdir()
        self.downloads = self.root / "releases"
        self.downloads.mkdir()
        self.commands = self.root / "commands"
        self.commands.mkdir()
        for name in ("awk", "basename", "cat", "chmod", "cmp", "cp", "cut", "dd", "df", "dirname", "find", "getconf", "grep", "gzip", "head", "id", "mkdir", "mktemp", "mv", "od", "rm", "rmdir", "sed", "sort", "tar", "tr", "wc", "sha256sum", "shasum", "openssl"):
            command = shutil.which(name)
            if command:
                (self.commands / name).symlink_to(command)
        self.script("uname", 'case "$1" in -s) printf "%s\\n" "${FIXTURE_KERNEL:-Linux}";; -m) printf "%s\\n" "${FIXTURE_ARCH:-x86_64}";; esac\n')
        # Target/ABI selection fixtures are independent of the harness host.
        self.script("getconf", 'printf "glibc 2.35\\n"\n')
        self.script("curl", f'exec {sys.executable} {ROOT / "release/tests/fixtures/posix_download.py"} "$@"\n')
        self.environment = {
            "HOME": str(self.home), "XDG_DATA_HOME": str(self.home / "data"),
            "XDG_BIN_HOME": str(self.home / "bin"), "TMPDIR": str(self.root),
            "PATH": str(self.commands), "SHELL": "/bin/sh", "LANG": "C.UTF-8",
            "ILIUM_INSTALL_TEST_ORIGIN": "https://127.0.0.1:18443/releases",
            "FIXTURE_RELEASE_ROOT": str(self.downloads),
        }
        self.install_root = self.home / "data/ilium"
        self.bin = self.home / "bin"
        with MANIFEST.open("rb") as source:
            self.targets = tomllib.load(source)["target"]
        self.release("0.1.0")

    def script(self, name, text):
        path = self.commands / name
        if path.is_symlink():
            path.unlink()
        path.write_text("#!/bin/sh\n" + text)
        path.chmod(0o755)

    def release(self, version, mutation=None, native_client=False):
        directory = self.downloads / ("v" + version)
        directory.mkdir(exist_ok=True)
        sums = []
        for target in self.targets:
            path = directory / target["archive"]
            if target["format"] == "zip":
                path.write_bytes(b"unused Windows fixture")
            else:
                prefix = target["archive"].removesuffix(".tar.gz")
                client = f'#!/bin/sh\nif [ "${{1:-}}" = --wait ]; then read answer; fi\nprintf "ilium {version}\\n"\n/bin/sh "$(dirname "$0")/ilium-server" --version\n'
                files = {"THIRD-PARTY.txt": b"Synthetic test licence\n", "VERSION": (version + "\n").encode(), "ilium": client.encode(), "ilium-server": f'#!/bin/sh\nprintf "ilium-server {version}\\n"\n'.encode()}
                if native_client:
                    files["ilium"] = Path("/bin/sh").read_bytes()
                entries = [(prefix, None)] + [(prefix + "/" + name, data) for name, data in sorted(files.items())]
                if mutation:
                    entries = mutation(prefix, entries)
                with tarfile.open(path, "w:gz", format=tarfile.USTAR_FORMAT) as archive:
                    for name, data in entries:
                        info = tarfile.TarInfo(name)
                        info.type = tarfile.DIRTYPE if data is None else tarfile.REGTYPE
                        info.mode = 0o755 if data is None or name.rsplit("/", 1)[-1] in ("ilium", "ilium-server") else 0o644
                        if isinstance(data, tuple):
                            info.type, info.linkname = data
                            data = b""
                        info.size = len(data) if data is not None else 0
                        archive.addfile(info, io.BytesIO(data) if data is not None else None)
            sums.append(hashlib.sha256(path.read_bytes()).hexdigest() + "  " + path.name + "\n")
        (directory / "SHA256SUMS").write_text("".join(sums))
        return directory

    def invoke(self, *arguments, env=None):
        return subprocess.run(["/bin/sh", str(INSTALLER), *arguments], env=self.environment | (env or {}), capture_output=True, text=True)

    def test_recovery_command_preserves_custom_paths_without_local_script(self):
        install = self.home / "custom ' install é"
        bin_directory = self.home / "custom ' bin é"
        result = self.invoke("--version", "0.1.0", "--install-dir", str(install), "--bin-dir", str(bin_directory), env={"FIXTURE_DOWNLOAD_FAILURE": "network"})
        self.assertNotEqual(result.returncode, 0)
        line = next(line for line in result.stderr.splitlines() if line.startswith("ilium-install: recovery="))
        tokens = shlex.split(line.split("recovery=", 1)[1])
        self.assertIn("https://ilium-setup.pages.dev/install.sh", tokens)
        self.assertEqual(tokens[tokens.index("--install-dir") + 1], str(install))
        self.assertEqual(tokens[tokens.index("--bin-dir") + 1], str(bin_directory))
        self.assertEqual(tokens[tokens.index("--version") + 1], "0.1.0")
        self.assertNotIn("./install.sh", tokens)

    def success(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("stage=complete", result.stdout)
        self.assertEqual(self.launch(), "ilium 0.1.0\nilium-server 0.1.0\n")

    def launch(self):
        result = subprocess.run([str(self.bin / "ilium"), "--version"], env=self.environment, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout

    def failure(self, result, stage=None, previous=False):
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("stage=complete", result.stdout + result.stderr)
        self.assertIn("stage=", result.stderr)
        self.assertIn("recovery=", result.stderr)
        self.assertIn("prior_active=" + ("yes" if previous else "no"), result.stderr)
        if stage:
            self.assertIn("stage=" + stage, result.stderr)

    def test_four_posix_targets_and_architecture_aliases(self):
        for kernel, arch, archive in (("Linux", "x86_64", "ilium-linux-x86_64.tar.gz"), ("Linux", "aarch64", "ilium-linux-aarch64.tar.gz"), ("Darwin", "arm64", "ilium-macos-aarch64.tar.gz"), ("Darwin", "x86_64", "ilium-macos-x86_64.tar.gz"), ("Linux", "arm64", "ilium-linux-aarch64.tar.gz")):
            with self.subTest(kernel=kernel, arch=arch):
                self.success(self.invoke("--version", "0.1.0", env={"FIXTURE_KERNEL": kernel, "FIXTURE_ARCH": arch}))
                requests = [json.loads(line)["url"] for line in (self.downloads / "requests.jsonl").read_text().splitlines()]
                self.assertTrue(requests[-2].endswith("/v0.1.0/" + archive))

    def test_latest_resolves_once_then_downloads_same_pinned_release(self):
        self.success(self.invoke())
        requests = [json.loads(line) for line in (self.downloads / "requests.jsonl").read_text().splitlines()]
        self.assertEqual(len(requests), 3)
        self.assertTrue(all("/download/v0.1.0/" in row["url"] for row in requests[1:]))
        for row in requests:
            self.assertIn("--proto", row["arguments"])
            self.assertIn("--tlsv1.2", row["arguments"])

    def test_unsupported_targets_do_not_download_or_install(self):
        for kernel, arch in (("FreeBSD", "x86_64"), ("Linux", "i686"), ("Darwin", "ppc"), ("Windows", "ARM64")):
            self.failure(self.invoke(env={"FIXTURE_KERNEL": kernel, "FIXTURE_ARCH": arch}), "target")
        self.assertFalse(self.install_root.exists())
        self.assertFalse((self.downloads / "requests.jsonl").exists())

    def test_musl_or_unverified_libc_is_rejected_before_mutation(self):
        for body in ('printf "musl 1.2.5\\n"\n', 'exit 1\n'):
            self.script("getconf", body)
            self.failure(self.invoke("--version", "0.1.0"), "target")
            self.assertFalse(self.install_root.exists())
            self.assertFalse((self.downloads / "requests.jsonl").exists())

    def test_invalid_versions_and_arguments_fail_before_mutation(self):
        for arguments in (("--version", "../../x"), ("--version", "1"), ("--version", "0.1.0\nx"), ("--version",), ("--wat",), ("--install-dir", "/")):
            self.failure(self.invoke(*arguments), "arguments")
        self.assertFalse(self.install_root.exists())

    def test_three_hash_tool_fallbacks_and_missing_hash(self):
        self.success(self.invoke("--version", "0.1.0"))
        (self.commands / "sha256sum").unlink()
        self.success(self.invoke("--version", "0.1.0"))
        (self.commands / "shasum").unlink()
        self.success(self.invoke("--version", "0.1.0"))
        (self.commands / "openssl").unlink()
        self.failure(self.invoke("--version", "0.1.0"), "prerequisites", previous=True)

    def test_download_failures_never_fall_back_from_requested_version(self):
        self.success(self.invoke("--version", "0.1.0"))
        for failure in ("network", "rate-limit", "asset"):
            self.failure(self.invoke("--version", "0.2.0", env={"FIXTURE_DOWNLOAD_FAILURE": failure}), "download", previous=True)
            self.assertEqual(self.launch(), "ilium 0.1.0\nilium-server 0.1.0\n")

    def test_bad_checksums_do_not_extract_or_execute(self):
        self.success(self.invoke("--version", "0.1.0"))
        release = self.release("0.2.0")
        sums = release / "SHA256SUMS"
        original = sums.read_text()
        for text in ("broken\n", original + original.splitlines()[0] + "\n", original.replace(original[:64], "0" * 64), original.replace("  ", " "), "", "\n".join(original.splitlines()[:-1]) + "\n", original.replace("ilium-windows-x86_64.zip", "unreviewed-extra.zip")):
            sums.write_text(text)
            self.failure(self.invoke("--version", "0.2.0"), "checksum", previous=True)
            self.assertFalse((self.install_root / "versions/0.2.0").exists())

    def test_hostile_archive_inventory_is_rejected_before_extraction(self):
        mutations = {
            "traversal": lambda p, e: e + [(p + "/../escaped", b"bad")],
            "absolute": lambda p, e: e + [("/tmp/ilium-escaped", b"bad")],
            "duplicate": lambda p, e: e + [e[-1]],
            "symlink": lambda p, e: e[:-1] + [(p + "/ilium-server", (tarfile.SYMTYPE, "/bin/sh"))],
            "hardlink": lambda p, e: e[:-1] + [(p + "/ilium-server", (tarfile.LNKTYPE, p + "/ilium"))],
            "unexpected": lambda p, e: e + [(p + "/other-program", b"bad")],
            "partial": lambda p, e: e[:-1],
            "version-mismatch": lambda p, e: [(n, b"9.9.9\n" if n.endswith("/VERSION") else d) for n, d in e],
            "special": lambda p, e: e + [(p + "/fifo", (tarfile.FIFOTYPE, ""))],
            "extended": lambda p, e: e + [(p + "/header", (tarfile.XHDTYPE, ""))],
        }
        self.success(self.invoke("--version", "0.1.0"))
        for name, mutation in mutations.items():
            with self.subTest(name=name):
                self.release("0.2.0", mutation)
                self.failure(self.invoke("--version", "0.2.0"), "archive", previous=True)
                self.assertEqual(self.launch(), "ilium 0.1.0\nilium-server 0.1.0\n")

    def test_invalid_pointer_and_foreign_launchers_preserved(self):
        self.success(self.invoke("--version", "0.1.0"))
        pointer = self.install_root / "current"
        for value in ("../x\n", "0.1.0\n0.2.0\n", "0.9.0\n", "0.1.0\0\n"):
            pointer.write_text(value)
            self.failure(self.invoke("--version", "0.1.0"), "ownership")
            result = subprocess.run([str(self.bin / "ilium")], env=self.environment, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
        pointer.write_text("0.1.0\n")
        launcher = self.bin / "ilium"
        launcher.write_text("user-authored executable\n")
        self.failure(self.invoke("--version", "0.1.0"), "ownership", previous=True)
        self.assertEqual(launcher.read_text(), "user-authored executable\n")

    def test_repeat_upgrade_retention_and_unknown_version_preservation(self):
        self.success(self.invoke("--version", "0.1.0"))
        before = {p: p.read_bytes() for p in (self.install_root / "current", self.bin / "ilium", self.home / ".profile")}
        self.success(self.invoke("--version", "0.1.0"))
        self.assertEqual({p: p.read_bytes() for p in before}, before)
        unknown = self.install_root / "versions/user-content"
        unknown.mkdir()
        (unknown / "notes").write_text("retain")
        for version in ("0.2.0", "0.3.0"):
            self.release(version)
            result = self.invoke("--version", version)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(self.launch(), f"ilium {version}\nilium-server {version}\n")
        self.assertEqual(sorted(p.name for p in (self.install_root / "versions").iterdir()), ["0.2.0", "0.3.0", "user-content"])

    def test_fix_round_utf8_locale_uses_byte_order(self):
        locales = subprocess.run(["locale", "-a"], capture_output=True, text=True, check=True).stdout.splitlines()
        locale = next((name for name in locales if name.lower().startswith("en_us.") and "utf" in name.lower()), None)
        self.assertIsNotNone(locale, "A non-C UTF-8 locale is required for this regression")
        localized = subprocess.run([str(self.commands / "sort")], input="THIRD-PARTY.txt\nVERSION\nilium\nilium-server\n", env=self.environment | {"LC_ALL": locale}, capture_output=True, text=True, check=True)
        self.assertNotEqual(localized.stdout, "THIRD-PARTY.txt\nVERSION\nilium\nilium-server\n")
        self.success(self.invoke("--version", "0.1.0", env={"LC_ALL": locale}))

    def test_fix_round_fresh_missing_hash_failure_leaves_retry_possible(self):
        for name in ("sha256sum", "shasum", "openssl"):
            (self.commands / name).unlink(missing_ok=True)
        self.failure(self.invoke("--version", "0.1.0"), "prerequisites")
        self.assertFalse(self.install_root.exists())
        (self.commands / "sha256sum").symlink_to(shutil.which("sha256sum"))
        self.success(self.invoke("--version", "0.1.0"))

    def assert_profile_publication_fault_preserves_original(self, fault):
        self.success(self.invoke("--version", "0.1.0", "--no-modify-path"))
        self.release("0.2.0")
        profile = self.home / ".profile"
        # The original need not end with a newline, and its mode is authored.
        original = "# authored\r\nUSER_VALUE='é'".encode()
        profile.write_bytes(original)
        profile.chmod(0o640)
        self.script("cat", 'case "$1" in */profile-block) case "${FIXTURE_PROFILE_FAULT:-}" in partial) printf "partial"; exit 1;; write) exit 1;; esac;; esac\nexec /bin/cat "$@"\n')
        self.script("mv", 'case "$2" in */.profile) case "${FIXTURE_PROFILE_FAULT:-}" in publish) exit 1;; interrupt) if [ ! -f "$FIXTURE_RELEASE_ROOT/profile-interrupted" ]; then /bin/mv "$@" || exit 1; : > "$FIXTURE_RELEASE_ROOT/profile-interrupted"; kill -HUP "$PPID"; exit 0; fi;; esac;; esac\nexec /bin/mv "$@"\n')
        self.script("cp", 'if [ "${FIXTURE_PROFILE_FAULT:-}" = metadata ]; then case "$2" in */installer-state/profile-path) printf "partial" > "$2"; exit 1;; esac; fi\nexec /bin/cp "$@"\n')
        self.failure(self.invoke("--version", "0.2.0", env={"FIXTURE_PROFILE_FAULT": fault}), "profile", previous=True)
        self.assertEqual(profile.read_bytes(), original)
        self.assertEqual(profile.stat().st_mode & 0o777, 0o640)
        self.assertFalse((self.install_root / "installer-state/profile-path").exists())
        self.assertEqual(self.launch(), "ilium 0.1.0\nilium-server 0.1.0\n")

    def test_fix_round_profile_partial_write_preserves_exact_original(self):
        self.assert_profile_publication_fault_preserves_original("partial")

    def test_fix_round_profile_write_failure_preserves_exact_original(self):
        self.assert_profile_publication_fault_preserves_original("write")

    def test_fix_round_profile_publish_failure_preserves_exact_original(self):
        self.assert_profile_publication_fault_preserves_original("publish")

    def test_fix_round_profile_metadata_failure_preserves_exact_original(self):
        self.assert_profile_publication_fault_preserves_original("metadata")

    def test_fix_round_profile_publish_interruption_preserves_exact_original(self):
        self.assert_profile_publication_fault_preserves_original("interrupt")

    def test_fix_round_concurrent_profile_edit_is_not_overwritten(self):
        self.success(self.invoke("--version", "0.1.0", "--no-modify-path"))
        self.release("0.2.0")
        profile = self.home / ".profile"
        original = b"# authored without final newline"
        profile.write_bytes(original)
        self.script("cat", 'case "$1" in */profile-block) printf "\\n# concurrent authored edit\\n" >> "$HOME/.profile";; esac\nexec /bin/cat "$@"\n')
        self.failure(self.invoke("--version", "0.2.0"), "profile", previous=True)
        self.assertEqual(profile.read_bytes(), original + b"\n# concurrent authored edit\n")
        self.assertEqual(self.launch(), "ilium 0.1.0\nilium-server 0.1.0\n")

    def test_fix_round_failed_version_receipt_allows_same_version_retry(self):
        self.success(self.invoke("--version", "0.1.0", "--no-modify-path"))
        self.release("0.2.0")
        self.script("cp", 'case "$1" in */version-receipt) printf "partial" > "$2"; exit 1;; esac\nexec /bin/cp "$@"\n')
        self.failure(self.invoke("--version", "0.2.0", "--no-modify-path"), "ownership", previous=True)
        self.assertFalse((self.install_root / "versions/0.2.0").exists())
        self.assertFalse((self.install_root / "installer-state/version-0.2.0").exists())
        self.script("cp", 'exec /bin/cp "$@"\n')
        result = self.invoke("--version", "0.2.0", "--no-modify-path")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.launch(), "ilium 0.2.0\nilium-server 0.2.0\n")

    def test_fix_round_pointer_is_read_after_lock_acquisition(self):
        self.success(self.invoke("--version", "0.1.0", "--no-modify-path"))
        self.release("0.2.0")
        self.release("0.3.0")
        gate = self.downloads / "A-gate"
        os.mkfifo(gate)
        self.script("uname", 'case "$1" in -s) if [ "${FIXTURE_ROLE:-}" = A ]; then printf "A-ready\\n" >&2; read -r release < "$FIXTURE_RELEASE_ROOT/A-gate"; fi; printf "Linux\\n";; -m) printf "x86_64\\n";; esac\n')
        process = subprocess.Popen(["/bin/sh", str(INSTALLER), "--version", "0.3.0", "--no-modify-path"], env=self.environment | {"FIXTURE_ROLE": "A"}, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            self.assertEqual(process.stderr.readline(), "A-ready\n")
            result = self.invoke("--version", "0.2.0", "--no-modify-path")
            self.assertEqual(result.returncode, 0, result.stderr)
            with gate.open("w") as stream:
                stream.write("continue\n")
            stdout, stderr = process.communicate(timeout=10)
            self.assertEqual(process.returncode, 0, stdout + stderr)
            self.assertEqual(self.launch(), "ilium 0.3.0\nilium-server 0.3.0\n")
            self.assertEqual((self.install_root / "installer-state/previous").read_text(), "0.2.0\n")
            self.assertEqual(sorted(path.name for path in (self.install_root / "versions").iterdir()), ["0.2.0", "0.3.0"])
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()

    def test_fix_round_version_publication_failures_remove_only_pending_receipt(self):
        self.success(self.invoke("--version", "0.1.0", "--no-modify-path"))
        self.release("0.2.0")
        for fault in ("receipt", "directory"):
            with self.subTest(fault=fault):
                self.script("mv", 'case "${FIXTURE_VERSION_FAULT:-}:$2" in receipt:*/version-0.2.0|directory:*/versions/0.2.0) exit 1;; esac\nexec /bin/mv "$@"\n')
                self.failure(self.invoke("--version", "0.2.0", "--no-modify-path", env={"FIXTURE_VERSION_FAULT": fault}), "ownership", previous=True)
                self.assertFalse((self.install_root / "versions/0.2.0").exists())
                self.assertFalse((self.install_root / "installer-state/version-0.2.0").exists())
                self.assertEqual(self.launch(), "ilium 0.1.0\nilium-server 0.1.0\n")
        self.script("mv", 'exec /bin/mv "$@"\n')
        result = self.invoke("--version", "0.2.0", "--no-modify-path")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.launch(), "ilium 0.2.0\nilium-server 0.2.0\n")

    def test_fix_round_read_only_history_file_is_preserved(self):
        self.success(self.invoke("--version", "0.1.0", "--no-modify-path"))
        self.release("0.2.0")
        self.assertEqual(self.invoke("--version", "0.2.0", "--no-modify-path").returncode, 0)
        self.release("0.3.0")
        previous = self.install_root / "installer-state/previous"
        previous.chmod(0o400)
        try:
            result = self.invoke("--version", "0.3.0", "--no-modify-path")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("old versions preserved", result.stderr)
            self.assertEqual(previous.read_bytes(), b"0.1.0\n")
            self.assertEqual(previous.stat().st_mode & 0o777, 0o400)
            self.assertEqual(sorted(path.name for path in (self.install_root / "versions").iterdir()), ["0.1.0", "0.2.0", "0.3.0"])
        finally:
            previous.chmod(0o600)

    def test_fix_round_read_only_previous_state_skips_pruning(self):
        self.success(self.invoke("--version", "0.1.0", "--no-modify-path"))
        self.release("0.2.0")
        self.assertEqual(self.invoke("--version", "0.2.0", "--no-modify-path").returncode, 0)
        self.release("0.3.0")
        state = self.install_root / "installer-state"
        self.script("mv", 'case "$2" in */current) /bin/mv "$@" || exit 1; /bin/chmod 400 "${2%/*}/installer-state/previous"; /bin/chmod 500 "${2%/*}/installer-state"; exit 0;; esac\nexec /bin/mv "$@"\n')
        try:
            result = self.invoke("--version", "0.3.0", "--no-modify-path")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("old versions preserved", result.stderr)
            self.assertEqual(self.launch(), "ilium 0.3.0\nilium-server 0.3.0\n")
            self.assertEqual(sorted(path.name for path in (self.install_root / "versions").iterdir()), ["0.1.0", "0.2.0", "0.3.0"])
            self.assertEqual((state / "previous").read_text(), "0.1.0\n")
        finally:
            state.chmod(0o700)
            (state / "previous").chmod(0o600)
        self.script("mv", 'exec /bin/mv "$@"\n')
        repeated = self.invoke("--version", "0.3.0", "--no-modify-path")
        self.assertEqual(repeated.returncode, 0, repeated.stderr)
        self.assertEqual(sorted(path.name for path in (self.install_root / "versions").iterdir()), ["0.1.0", "0.2.0", "0.3.0"])

    def test_failed_extraction_or_pointer_switch_rolls_back(self):
        self.success(self.invoke("--version", "0.1.0"))
        self.release("0.2.0")
        self.script("tar", 'case "$1" in -xf) exit 1;; esac\nexec /usr/bin/tar "$@"\n')
        self.failure(self.invoke("--version", "0.2.0"), "archive", previous=True)
        self.script("tar", 'exec /usr/bin/tar "$@"\n')
        self.script("mv", 'case "$2" in */current) exit 1;; esac\nexec /bin/mv "$@"\n')
        self.failure(self.invoke("--version", "0.2.0"), "switch", previous=True)
        self.assertEqual(self.launch(), "ilium 0.1.0\nilium-server 0.1.0\n")

    def test_unwritable_low_space_and_profile_rejection_preserve_prior(self):
        self.success(self.invoke("--version", "0.1.0", "--no-modify-path"))
        self.release("0.2.0")
        self.script("df", 'printf "Filesystem 1024-blocks Used Available Capacity Mounted on\\nfixture 100 99 1 99%% /\\n"\n')
        self.failure(self.invoke("--version", "0.2.0"), "space", previous=True)
        self.script("df", 'exec /bin/df "$@"\n')
        (self.home / ".profile").mkdir()
        self.failure(self.invoke("--version", "0.2.0"), "profile", previous=True)
        collision = self.root / "not-directory"
        collision.write_text("keep")
        self.failure(self.invoke("--version", "0.2.0", "--install-dir", str(collision)), "ownership")
        self.assertEqual(collision.read_text(), "keep")

    def test_profiles_preserve_bytes_and_remove_only_exact_owned_append(self):
        for shell, profile_name in (("/bin/sh", ".profile"), ("/bin/bash", ".bashrc"), ("/bin/zsh", ".zshrc")):
            profile = self.home / profile_name
            original = b"# authored\r\nexport USER_VALUE='keep'"  # no final newline
            profile.write_bytes(original)
            self.success(self.invoke("--version", "0.1.0", env={"SHELL": shell}))
            self.assertTrue(profile.read_bytes().startswith(original))
            appended = b"\n# later user addition\n"
            with profile.open("ab") as stream:
                stream.write(appended)
            result = self.invoke("--uninstall", env={"SHELL": shell})
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(profile.read_bytes(), original + appended)

    def test_no_modify_path_and_existing_user_path_do_not_claim_profile(self):
        original = f'# User PATH\nexport PATH="{self.bin}:$PATH"\n'.encode()
        profile = self.home / ".profile"
        profile.write_bytes(original)
        self.success(self.invoke("--version", "0.1.0", "--no-modify-path"))
        self.assertEqual(profile.read_bytes(), original)
        self.success(self.invoke("--version", "0.1.0"))
        self.assertEqual(profile.read_bytes(), original)
        result = self.invoke("--uninstall")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(profile.read_bytes(), original)

    def test_uninstall_retains_modified_and_unknown_content(self):
        self.success(self.invoke("--version", "0.1.0"))
        binary = self.install_root / "versions/0.1.0/bin/ilium-server"
        binary.write_text("user changed binary")
        foreign = self.install_root / "private-notes"
        foreign.write_text("keep")
        launcher = self.bin / "ilium-server"
        launcher.write_text("user launcher")
        result = self.invoke("--uninstall")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(binary.read_text(), "user changed binary")
        self.assertEqual(foreign.read_text(), "keep")
        self.assertEqual(launcher.read_text(), "user launcher")
        self.assertFalse((self.bin / "ilium").exists())

    def test_concurrent_invocation_rejected_without_disturbing_owner(self):
        process = subprocess.Popen(["/bin/sh", str(INSTALLER), "--version", "0.1.0"], env=self.environment | {"FIXTURE_DELAY": "0.4"}, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            deadline = time.monotonic() + 4
            while not (self.install_root / ".install-lock").exists() and time.monotonic() < deadline:
                time.sleep(0.01)
            second = self.invoke("--version", "0.1.0")
            self.failure(second, "lock")
            stdout, stderr = process.communicate(timeout=10)
            self.assertEqual(process.returncode, 0, stdout + stderr)
            self.assertEqual(self.launch(), "ilium 0.1.0\nilium-server 0.1.0\n")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()

    def test_old_running_process_survives_upgrade_and_retention(self):
        self.release("0.1.0", native_client=True)
        result = self.invoke("--version", "0.1.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        process = subprocess.Popen([str(self.bin / "ilium"), "-c", 'printf "ready\\n"; read answer; printf "old-native-process-alive\\n"'], env=self.environment, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            self.assertEqual(process.stdout.readline(), "ready\n")
            original_process_id = process.pid
            for version in ("0.2.0", "0.3.0"):
                self.release(version)
                result = self.invoke("--version", version)
                self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(process.pid, original_process_id)
            self.assertIsNone(process.poll())
            self.assertFalse((self.install_root / "versions/0.1.0").exists())
            if Path(f"/proc/{process.pid}/exe").exists():
                self.assertTrue(os.readlink(f"/proc/{process.pid}/exe").endswith(" (deleted)"))
            stdout, stderr = process.communicate("done\n", timeout=5)
            self.assertEqual(process.returncode, 0, stderr)
            self.assertEqual(stdout, "old-native-process-alive\n")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()

    def test_launcher_reads_pointer_only_once_during_upgrade(self):
        self.success(self.invoke("--version", "0.1.0"))
        self.release("0.2.0")
        self.assertEqual(self.invoke("--version", "0.2.0").returncode, 0)
        pointer = self.install_root / "current"
        pointer.write_text("0.1.0\n")
        self.script("cat", 'case "$1" in */current) /bin/cat "$1"; printf "0.2.0\\n" > "$1"; exit 0;; esac\nexec /bin/cat "$@"\n')
        self.script("wc", 'case "$2" in *current*) exit 90;; esac\nif [ "${FIXTURE_LAUNCH_CHECK:-}" = yes ]; then exit 90; fi\nexec /usr/bin/wc "$@"\n')
        result = subprocess.run([str(self.bin / "ilium"), "--version"], env=self.environment | {"FIXTURE_LAUNCH_CHECK": "yes"}, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "ilium 0.1.0\nilium-server 0.1.0\n")

    def test_custom_paths_with_apostrophes_unicode_and_spaces(self):
        self.install_root = self.root / "custom ' données/install"
        self.bin = self.root / "custom ' données/bin"
        self.success(self.invoke("--version", "0.1.0", "--install-dir", str(self.install_root), "--bin-dir", str(self.bin)))
        profile = self.home / ".profile"
        result = subprocess.run(["/bin/sh", "-c", '. "$HOME/.profile"; . "$HOME/.profile"; printf "%s" "$PATH"'], env=self.environment, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.count(str(self.bin)), 1)
        self.assertTrue(profile.is_file())

    def test_profile_edit_rolls_back_after_failed_switch(self):
        self.success(self.invoke("--version", "0.1.0", "--no-modify-path"))
        original = b"# user bytes without final newline"
        profile = self.home / ".profile"
        profile.write_bytes(original)
        self.release("0.2.0")
        self.script("mv", 'case "$2" in */current) exit 1;; esac\nexec /bin/mv "$@"\n')
        self.failure(self.invoke("--version", "0.2.0"), "switch", previous=True)
        self.assertEqual(profile.read_bytes(), original)
        self.assertFalse((self.install_root / "installer-state/profile-path").exists())

    def test_permission_and_full_disk_faults_are_stage_errors(self):
        self.success(self.invoke("--version", "0.1.0"))
        self.release("0.2.0")
        self.script("mktemp", 'exit 1\n')
        self.failure(self.invoke("--version", "0.2.0"), "staging", previous=True)
        self.assertFalse((self.install_root / ".install-lock").exists())
        self.script("mktemp", 'exec /usr/bin/mktemp "$@"\n')
        self.script("gzip", 'printf "disk full\\n" >&2; exit 1\n')
        self.failure(self.invoke("--version", "0.2.0"), "archive", previous=True)
        self.assertEqual(self.launch(), "ilium 0.1.0\nilium-server 0.1.0\n")

    def test_unexpected_profile_io_failure_has_structured_diagnostics(self):
        self.success(self.invoke("--version", "0.1.0", "--no-modify-path"))
        self.release("0.2.0")
        state = self.install_root / "installer-state"
        (state / "profile-path").mkdir()
        result = self.invoke("--version", "0.2.0")
        self.failure(result, "profile", previous=True)
        self.assertEqual(self.launch(), "ilium 0.1.0\nilium-server 0.1.0\n")

    def test_missing_home_reports_arguments_stage(self):
        self.failure(self.invoke("--version", "0.1.0", env={"HOME": ""}), "arguments")

    def test_installer_does_not_execute_verified_release_bytes(self):
        self.release("0.1.0", lambda p, e: [(name, b'#!/bin/sh\nprintf "executed\\n" > "$FIXTURE_RELEASE_ROOT/payload-executed"\n' if name.endswith("/ilium") else data) for name, data in e])
        result = self.invoke("--version", "0.1.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.downloads / "payload-executed").exists())

    def test_missing_hash_and_checksum_failure_do_not_decompress(self):
        self.script("gzip", 'printf "decompressed\\n" > "$FIXTURE_RELEASE_ROOT/decompressed"; exit 88\n')
        sums = self.downloads / "v0.1.0/SHA256SUMS"
        sums.write_text(sums.read_text().replace(sums.read_text()[:64], "0" * 64))
        self.failure(self.invoke("--version", "0.1.0"), "checksum")
        self.assertFalse((self.downloads / "decompressed").exists())

    def test_changed_owned_binary_bytes_are_preserved_on_repair(self):
        self.success(self.invoke("--version", "0.1.0"))
        client = self.install_root / "versions/0.1.0/bin/ilium"
        client.write_text("user changed client")
        self.failure(self.invoke("--version", "0.1.0"), "ownership", previous=True)
        self.assertEqual(client.read_text(), "user changed client")

    def test_runtime_libraries_follow_archive_target_and_modes(self):
        def with_library(prefix, entries):
            return [entries[0]] + sorted(entries[1:] + [(prefix + "/libonnxruntime.so.1.24.2", b"synthetic runtime bytes")])
        self.release("0.1.0", with_library)
        self.success(self.invoke("--version", "0.1.0"))
        self.assertTrue((self.install_root / "versions/0.1.0/bin/libonnxruntime.so.1.24.2").is_file())
        self.release("0.2.0", with_library)
        self.failure(self.invoke("--version", "0.2.0", env={"FIXTURE_KERNEL": "Darwin"}), "archive", previous=True)

    def test_same_version_repairs_only_owned_missing_binary(self):
        self.success(self.invoke("--version", "0.1.0"))
        (self.install_root / "versions/0.1.0/bin/ilium-server").unlink()
        self.success(self.invoke("--version", "0.1.0"))

    def test_repair_never_exposes_partial_executable_to_launcher(self):
        self.success(self.invoke("--version", "0.1.0"))
        (self.install_root / "versions/0.1.0/bin/ilium-server").unlink()
        self.script("cp", 'case "$3" in */versions/*/bin/*) printf "#!/bin/sh\\nprintf partial-server\\\\n\\n" > "$3"; chmod 755 "$3"; "$XDG_BIN_HOME/ilium" --version > "$FIXTURE_RELEASE_ROOT/repair-output" 2>&1; printf "%s\\n" "$?" > "$FIXTURE_RELEASE_ROOT/repair-status";; esac\nexec /bin/cp "$@"\n')
        self.success(self.invoke("--version", "0.1.0"))
        self.assertNotIn("partial-server", (self.downloads / "repair-output").read_text())
        self.assertNotEqual((self.downloads / "repair-status").read_text().strip(), "0")

    def test_launcher_rejects_symlinked_version_parent(self):
        self.success(self.invoke("--version", "0.1.0"))
        versions = self.install_root / "versions"
        moved = self.root / "foreign-versions"
        versions.rename(moved)
        versions.symlink_to(moved)
        result = subprocess.run([str(self.bin / "ilium"), "--version"], env=self.environment, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0, result.stdout)

    def test_literal_glob_and_backslash_paths_are_not_find_patterns(self):
        self.install_root = self.root / "literal * [é] ? \\b/install"
        self.bin = self.root / "literal * [é] ? \\b/bin"
        self.success(self.invoke("--version", "0.1.0", "--install-dir", str(self.install_root), "--bin-dir", str(self.bin)))
        self.success(self.invoke("--version", "0.1.0", "--install-dir", str(self.install_root), "--bin-dir", str(self.bin)))

    def test_read_only_owned_directory_fails_without_mutation(self):
        self.success(self.invoke("--version", "0.1.0"))
        before = (self.install_root / "current").read_bytes()
        self.install_root.chmod(0o500)
        try:
            # Prior activity cannot be confirmed before the lock is acquired.
            self.failure(self.invoke("--version", "0.1.0"), "ownership")
            self.assertEqual((self.install_root / "current").read_bytes(), before)
            self.assertEqual(self.launch(), "ilium 0.1.0\nilium-server 0.1.0\n")
        finally:
            self.install_root.chmod(0o700)

    def test_extra_data_and_malformed_tar_never_reach_extraction(self):
        self.success(self.invoke("--version", "0.1.0"))
        for kind in ("garbage", "truncated", "invalid-header", "extra-padding-data"):
            release = self.release("0.2.0")
            path = release / "ilium-linux-x86_64.tar.gz"
            import gzip
            if kind == "garbage":
                path.write_bytes(b"not gzip")
            else:
                raw = gzip.decompress(path.read_bytes())
                if kind == "truncated":
                    raw = raw[:1024]
                elif kind == "invalid-header":
                    raw = b"x" + raw[1:]
                else:
                    raw += b"x" + b"\0" * 511
                path.write_bytes(gzip.compress(raw))
            sums = release / "SHA256SUMS"
            lines = sums.read_text().splitlines()
            lines[0] = hashlib.sha256(path.read_bytes()).hexdigest() + "  " + path.name
            sums.write_text("\n".join(lines) + "\n")
            self.script("tar", 'printf "extracted\\n" > "$FIXTURE_RELEASE_ROOT/extracted"; exit 88\n')
            result = self.invoke("--version", "0.2.0")
            self.failure(result, "archive", previous=True)
            self.assertFalse((self.downloads / "extracted").exists())

    def test_unsafe_test_origin_and_symlinked_install_paths_rejected(self):
        for origin in ("http://127.0.0.1:18443/releases", "https://github.example/releases", "https://127.0.0.1:1@evil/releases"):
            self.failure(self.invoke("--version", "0.1.0", env={"ILIUM_INSTALL_TEST_ORIGIN": origin}), "download")
        target = self.root / "foreign"
        target.mkdir()
        symlink = self.root / "symlink-root"
        symlink.symlink_to(target)
        self.failure(self.invoke("--version", "0.1.0", "--install-dir", str(symlink)), "ownership")
        self.assertFalse(list(target.iterdir()))

    def test_changed_profile_block_and_version_content_survive_uninstall(self):
        self.success(self.invoke("--version", "0.1.0"))
        profile = self.home / ".profile"
        modified = profile.read_bytes().replace(b"ilium installer PATH", b"user edited ilium PATH")
        profile.write_bytes(modified)
        unknown = self.install_root / "versions/0.1.0/bin/user-note"
        unknown.write_text("keep")
        result = self.invoke("--uninstall")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(profile.read_bytes(), modified)
        self.assertEqual(unknown.read_text(), "keep")
        result = self.invoke("--uninstall")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_profile_uninstall_io_failure_preserves_profile_and_active_pair(self):
        profile = self.home / ".profile"
        profile.write_bytes(b"# retained user profile\n")
        self.success(self.invoke("--version", "0.1.0"))
        original = profile.read_bytes()
        self.script("cat", 'case "$1" in */profile-clean) printf "partial write\\n"; exit 1;; esac\nexec /bin/cat "$@"\n')
        self.failure(self.invoke("--uninstall"), "uninstall", previous=True)
        self.assertEqual(profile.read_bytes(), original)
        self.assertEqual(self.launch(), "ilium 0.1.0\nilium-server 0.1.0\n")

    def test_generated_target_table_is_byte_exact_and_check_is_read_only(self):
        result = subprocess.run([sys.executable, "-B", str(ROOT / "release/scripts/release_tool.py"), "generate-posix-table", "--manifest", str(MANIFEST), "--installer", str(INSTALLER), "--check"], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(json.loads(result.stdout)["state"], "passed")
        mutated = self.root / "mutated.sh"
        mutated.write_bytes(INSTALLER.read_bytes().replace(b"ilium-linux-x86_64.tar.gz", b"wrong-linux-x86_64.tar.gz", 1))
        result = subprocess.run([sys.executable, "-B", str(ROOT / "release/scripts/release_tool.py"), "generate-posix-table", "--manifest", str(MANIFEST), "--installer", str(mutated), "--check"], capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"wrong-linux", mutated.read_bytes())

    def test_fixture_runner_help_and_invalid_flags_emit_jsonl(self):
        driver = Path(__file__).resolve()
        for flags, expected_code in ((["--help"], 0), (["--unknown"], 2), (["--installer", str(INSTALLER), "--manifest", str(MANIFEST), "--filter", "no-such-test"], 2)):
            result = subprocess.run([sys.executable, "-B", str(driver), *flags], capture_output=True, text=True)
            self.assertEqual(result.returncode, expected_code, result.stderr)
            self.assertTrue(result.stdout.startswith("{"), result.stdout)
            record = json.loads(result.stdout)
            self.assertIn("type", record)


def main():
    global INSTALLER, MANIFEST
    parser = release_tool.JsonArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument("--installer", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--filter", default="")
    try:
        arguments = parser.parse_args()
    except release_tool.ReleaseError as error:
        release_tool.emit({"type": "error", "command": "test-posix-install", "error": str(error)})
        return 2
    INSTALLER, MANIFEST = arguments.installer.resolve(), arguments.manifest.resolve()
    names = unittest.defaultTestLoader.getTestCaseNames(PosixInstallTests)
    selected_names = [name for name in names if arguments.filter in name]
    if not selected_names:
        release_tool.emit({"type": "error", "command": "test-posix-install", "error": "Filter did not select any test"})
        return 2
    suite = unittest.TestSuite(PosixInstallTests(name) for name in selected_names)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    print(json.dumps({"type": "result", "command": "test-posix-install", "tests": result.testsRun, "failures": len(result.failures), "errors": len(result.errors), "state": "passed" if result.wasSuccessful() else "failed"}))
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    sys.exit(main())
