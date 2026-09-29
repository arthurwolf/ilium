"""Fixtures test native gate decisions; no fixture is native release evidence."""

import hashlib
import copy
import importlib
import json
from pathlib import Path
import sys
import subprocess
import shutil
import platform
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "release/scripts"))


class NativeAuditTests(unittest.TestCase):
    def module(self, name):
        self.assertIsNotNone(importlib.util.find_spec(name), f"{name} native gate is missing")
        return importlib.import_module(name)

    def intel_build_receipt(self):
        commit = "058787ceead760166e3c50a0a4cba8a833a6f53f"
        toolchains = {
            "clang": (["clang", "--version"], "Apple clang version 17.0.0\nTarget: x86_64-apple-darwin24.5.0\n"),
            "xcode": (["xcodebuild", "-version"], "Xcode 16.4\nBuild version 16F6\n"),
            "developer_directory": (["xcode-select", "-p"], "/Applications/Xcode_16.4.app/Contents/Developer\n"),
            "cmake": (["cmake", "--version"], "cmake version 3.31.6\n"),
            "rustc": (["rustc", "-Vv"], "rustc 1.88.0 (synthetic fixture)\nhost: x86_64-apple-darwin\nrelease: 1.88.0\n"),
            "cargo": (["cargo", "--version"], "cargo 1.88.0 (synthetic fixture)\n"),
        }
        return {
            "schema": 1, "state": "built-not-qualified", "source_register_sha256": "c" * 64,
            "source_register": {"schema": 1, "state": "reviewed", "version": "1.24.2", "tag": "v1.24.2", "commit": commit, "source_url": f"https://codeload.github.com/microsoft/onnxruntime/tar.gz/{commit}", "reviewed_by": "fixture reviewer", "source_sha256": "a" * 64},
            "tag_resolution": {"reference_url": "https://api.github.com/repos/microsoft/onnxruntime/git/ref/tags/v1.24.2", "reference": {"ref": "refs/tags/v1.24.2", "object": {"type": "commit", "sha": commit}}},
            "runner_identity": "macos-15-intel",
            "native_identity": {"system": "Darwin", "machine": "x86_64", "release": "24.5.0", "version": "Darwin Kernel Version 24.5.0: synthetic fixture"},
            "toolchain": {**{name: {"command": command, "stdout": output, "stderr": ""} for name, (command, output) in toolchains.items()}, "python": "3.11.7 (synthetic fixture)"},
            "runtime": {"path": "/native/build/Release/Release/libonnxruntime.1.24.2.dylib", "version": "1.24.2", "sha256": "b" * 64},
            "environment": {"ORT_LIB_LOCATION": "/native/build/Release/Release", "ORT_LIB_PATH": "/native/build/Release/Release", "ORT_PREFER_DYNAMIC_LINK": "1", "CARGO_HOME": "/native/cargo-home", "CARGO_TARGET_DIR": "/native/cargo-target"},
            "ort_command": [f"/native/source/onnxruntime-{commit}/build.sh", "--config", "Release", "--build_shared_lib", "--parallel", "4", "--use_xcode", "--skip_submodule_sync", "--build_dir", "/native/build", "--cmake_extra_defines", "CMAKE_OSX_ARCHITECTURES=x86_64"],
            "cargo_command": ["cargo", "build", "--locked", "--release", "--manifest-path", "/native/workspace/Cargo.toml", "--target", "x86_64-apple-darwin", "--bin", "ilium", "--bin", "ilium-server"],
        }

    def test_loader_parsers_recover_dependencies_without_executing_code(self):
        audit = self.module("audit_native")
        self.assertEqual(audit.parse_dependencies("linux", ' 0x (NEEDED) Shared library: [libonnxruntime.so.1]\n 0x (NEEDED) Shared library: [libc.so.6]\n'), ["libonnxruntime.so.1", "libc.so.6"])
        self.assertEqual(audit.parse_dependencies("windows", 'Image has the following dependencies:\n\n    KERNEL32.dll\n    onnxruntime.dll\n\n  Summary\n'), ["KERNEL32.dll", "onnxruntime.dll"])
        self.assertEqual(audit.parse_dependencies("macos", '/candidate/ilium:\n\t@executable_path/libonnxruntime.1.24.2.dylib (compatibility version 1.0.0, current version 1.24.2)\n\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)\n'), ["@executable_path/libonnxruntime.1.24.2.dylib", "/usr/lib/libSystem.B.dylib"])

    def test_undeclared_or_unreviewed_dynamic_libraries_block_the_closure(self):
        audit = self.module("audit_native")
        inventory = {"schema": 1, "state": "reviewed", "target": "x86_64-pc-windows-msvc", "files": [], "system_libraries": [{"name": "KERNEL32.dll", "reviewed": True, "source": "https://learn.microsoft.com/windows/"}]}
        self.assertEqual(audit.validate_closure("windows", {"ilium.exe": ["KERNEL32.dll"], "ilium-server.exe": []}, inventory, {"ilium.exe", "ilium-server.exe"}), [])
        for dependencies in (["unknown.dll"], ["onnxruntime.dll"], ["kernel32.dll", "unknown.dll"]):
            with self.subTest(dependencies=dependencies):
                with self.assertRaises(ValueError):
                    audit.validate_closure("windows", {"ilium.exe": dependencies, "ilium-server.exe": []}, inventory, {"ilium.exe", "ilium-server.exe"})
        inventory["system_libraries"][0]["reviewed"] = False
        with self.assertRaises(ValueError):
            audit.validate_closure("windows", {"ilium.exe": ["KERNEL32.dll"], "ilium-server.exe": []}, inventory, {"ilium.exe", "ilium-server.exe"})

    def test_macos_homebrew_and_unversioned_onnx_are_rejected(self):
        audit = self.module("audit_native")
        inventory = {"schema": 1, "state": "reviewed", "target": "x86_64-apple-darwin", "files": [{"name": "libonnxruntime.1.24.2.dylib"}], "system_libraries": []}
        for dependency in ("/opt/homebrew/lib/libonnxruntime.dylib", "/usr/local/lib/libonnxruntime.dylib", "/usr/lib/libonnxruntime.dylib", "@rpath/libonnxruntime.1.24.2.dylib", "@executable_path/libonnxruntime.dylib"):
            with self.subTest(dependency=dependency):
                with self.assertRaises(ValueError):
                    audit.validate_closure("macos", {"ilium": [dependency], "ilium-server": [], "libonnxruntime.1.24.2.dylib": []}, inventory, {"ilium", "ilium-server", "libonnxruntime.1.24.2.dylib"})

    def test_embedding_proof_requires_real_finite_vector_and_shipped_loaded_path(self):
        audit = self.module("audit_native")
        with tempfile.TemporaryDirectory(prefix="ilium-embedding-fixture-") as directory:
            root = Path(directory)
            model = root / "model.onnx"
            model.write_bytes(b"synthetic model fixture")
            runtime = root / "libonnxruntime.1.24.2.dylib"
            runtime.write_bytes(b"synthetic runtime fixture")
            proof = {"type": "embedding-proof", "model_sha256": hashlib.sha256(model.read_bytes()).hexdigest(), "input": "release embedding acceptance", "embedding": [0.1, -0.2, 0.3], "loaded_runtime": str(runtime)}
            result = audit.validate_embedding_proof(proof, root, model, [str(runtime)])
            self.assertEqual(result["dimensions"], 3)
            for changes in ({"embedding": []}, {"embedding": [float("nan")]}, {"embedding": [0.0, 0.0]}, {"loaded_runtime": "/opt/homebrew/lib/libonnxruntime.dylib"}, {"model_sha256": "0" * 64}, {"input": ""}):
                with self.subTest(changes=changes):
                    with self.assertRaises(ValueError):
                        audit.validate_embedding_proof(dict(proof, **changes), root, model, [str(runtime)])
            with self.assertRaises(ValueError):
                audit.validate_embedding_proof(proof, root, model, [])

    def test_held_embedding_process_must_be_installed_client_with_shipped_mapping(self):
        audit = self.module("audit_native")
        self.assertTrue(hasattr(audit, "validate_process_mapping"), "hardened-runtime process inspection gate is missing")
        client = Path("/candidate with spaces/ilium")
        runtime = Path("/candidate with spaces/libonnxruntime.1.24.2.dylib")
        maps = f"__TEXT 1234-5678 r-x /candidate with spaces/ilium\n__TEXT 5678-9999 r-x {runtime}\n"
        self.assertEqual(audit.validate_process_mapping(str(client), maps, client, runtime), [str(runtime)])
        for observed_client, mapping in (("/other/ilium", maps), (str(client), maps.replace(str(runtime), "/opt/homebrew/lib/libonnxruntime.dylib")), (str(client), maps + "__TEXT 000-123 r-x /opt/homebrew/lib/libonnxruntime.dylib\n")):
            with self.subTest(observed_client=observed_client, mapping=mapping):
                with self.assertRaises(ValueError):
                    audit.validate_process_mapping(observed_client, mapping, client, runtime)

    def test_linux_rpaths_cannot_redirect_system_dependencies_to_build_host(self):
        audit = self.module("audit_native")
        self.assertTrue(hasattr(audit, "validate_linux_rpaths"), "ELF loader path gate is missing")
        audit.validate_linux_rpaths(' (RUNPATH) Library runpath: [$ORIGIN]\n')
        for path in ("/opt/vendor", "/tmp/build", "$ORIGIN/../../escape", "$ORIGIN:/usr/local/lib", "relative/path"):
            with self.subTest(path=path):
                with self.assertRaises(ValueError):
                    audit.validate_linux_rpaths(f" (RUNPATH) Library runpath: [{path}]\n")

    def test_intel_native_audit_requires_pinned_build_receipt_and_runtime_hash(self):
        audit = self.module("audit_native")
        self.assertTrue(hasattr(audit, "validate_intel_build_receipt"), "Intel source-build provenance gate is missing")
        receipt = self.intel_build_receipt()
        runtimes = [{"name": "libonnxruntime.1.24.2.dylib", "version": "1.24.2", "sha256": "b" * 64}]
        audit.validate_intel_build_receipt(receipt, runtimes)
        for changes in ({"state": "blocked"}, {"native_identity": {"system": "Linux", "machine": "x86_64"}}, {"runtime": {"version": "1.24.2", "sha256": "c" * 64}}, {"source_register": {"state": "reviewed", "version": "1.24.1"}}, {"environment": {}}):
            with self.subTest(changes=changes):
                with self.assertRaises(ValueError):
                    audit.validate_intel_build_receipt(dict(receipt, **changes), runtimes)

    def test_intel_receipt_requires_structured_toolchain_runner_and_source_evidence(self):
        audit = self.module("audit_native")
        baseline = self.intel_build_receipt()
        runtimes = [{"name": "libonnxruntime.1.24.2.dylib", "version": "1.24.2", "sha256": "b" * 64}]
        mutations = [
            ("runner_identity", None), ("runner_identity", "unreviewed-runner"),
            ("native_identity", {"system": "Darwin", "machine": "x86_64"}),
            ("toolchain", {"clang": "fixture", "xcode": "fixture", "rustc": "fixture"}),
            ("source_register_sha256", None),
            ("tag_resolution", {"reference_url": "https://unreviewed.invalid/tag", "reference": baseline["tag_resolution"]["reference"]}),
        ]
        for key, value in mutations:
            with self.subTest(key=key, value=value):
                receipt = copy.deepcopy(baseline)
                receipt[key] = value
                with self.assertRaises(ValueError):
                    audit.validate_intel_build_receipt(receipt, runtimes)
        for tool in ("clang", "xcode", "developer_directory", "cmake", "rustc", "cargo", "python"):
            with self.subTest(missing_tool=tool):
                receipt = copy.deepcopy(baseline)
                del receipt["toolchain"][tool]
                with self.assertRaises(ValueError):
                    audit.validate_intel_build_receipt(receipt, runtimes)
        for field, value in (("command", ["echo", "clang"]), ("stdout", "unverified compiler"), ("stderr", None)):
            with self.subTest(clang_field=field):
                receipt = copy.deepcopy(baseline)
                receipt["toolchain"]["clang"][field] = value
                with self.assertRaises(ValueError):
                    audit.validate_intel_build_receipt(receipt, runtimes)

    def test_intel_receipt_requires_exact_native_ort_and_cargo_boundaries(self):
        audit = self.module("audit_native")
        baseline = self.intel_build_receipt()
        runtimes = [{"name": "libonnxruntime.1.24.2.dylib", "version": "1.24.2", "sha256": "b" * 64}]
        mutations = [
            ("ort_command", None), ("cargo_command", None),
            ("ort_command", [value for value in baseline["ort_command"] if value != "--use_xcode"]),
            ("ort_command", ["CMAKE_OSX_ARCHITECTURES=arm64" if value == "CMAKE_OSX_ARCHITECTURES=x86_64" else value for value in baseline["ort_command"]]),
            ("cargo_command", ["aarch64-apple-darwin" if value == "x86_64-apple-darwin" else value for value in baseline["cargo_command"]]),
            ("cargo_command", baseline["cargo_command"][:-2]),
            ("environment", dict(baseline["environment"], ORT_LIB_LOCATION="/other/ort", ORT_LIB_PATH="/other/ort")),
            ("environment", {key: value for key, value in baseline["environment"].items() if key != "ORT_LIB_PATH"}),
        ]
        for key, value in mutations:
            with self.subTest(key=key, value=value):
                receipt = copy.deepcopy(baseline)
                receipt[key] = value
                with self.assertRaises(ValueError):
                    audit.validate_intel_build_receipt(receipt, runtimes)

    def test_macos_relocation_rewrites_only_declared_runtime_names(self):
        audit = self.module("audit_native")
        self.assertTrue(hasattr(audit, "macos_relocations"), "native runtime relocation is missing")
        changes = audit.macos_relocations({"ilium": ["@rpath/libonnxruntime.1.dylib", "/usr/lib/libSystem.B.dylib"], "libonnxruntime.1.24.2.dylib": ["@rpath/libonnxruntime.1.dylib", "/usr/lib/libSystem.B.dylib"]}, {"libonnxruntime.1.24.2.dylib"})
        self.assertEqual(changes, [("ilium", "@rpath/libonnxruntime.1.dylib", "@executable_path/libonnxruntime.1.24.2.dylib"), ("libonnxruntime.1.24.2.dylib", "@rpath/libonnxruntime.1.dylib", "@executable_path/libonnxruntime.1.24.2.dylib")])
        with self.assertRaises(ValueError):
            audit.macos_relocations({"ilium": ["/opt/homebrew/lib/unreviewed.dylib"]}, {"libonnxruntime.1.24.2.dylib"})

    def test_macos_dylib_referenced_only_by_its_own_id_is_unreachable(self):
        audit = self.module("audit_native")
        inventory = {"files": [{"name": "liborphan.1.2.3.dylib"}], "system_libraries": []}
        graph = {"ilium": [], "ilium-server": [], "liborphan.1.2.3.dylib": ["@executable_path/liborphan.1.2.3.dylib"]}
        with self.assertRaises(ValueError):
            audit.validate_closure("macos", graph, inventory, set(graph))

    def test_macos_closure_traverses_both_executable_roots_and_runtime_cycles(self):
        audit = self.module("audit_native")
        inventory = {"files": [{"name": "libfirst.1.0.0.dylib"}, {"name": "libsecond.1.0.0.dylib"}], "system_libraries": []}
        graph = {"ilium": [], "ilium-server": ["@executable_path/libfirst.1.0.0.dylib"], "libfirst.1.0.0.dylib": ["@executable_path/libsecond.1.0.0.dylib"], "libsecond.1.0.0.dylib": ["@executable_path/libfirst.1.0.0.dylib"]}
        self.assertEqual(audit.validate_closure("macos", graph, inventory, set(graph)), ["libfirst.1.0.0.dylib", "libsecond.1.0.0.dylib"])
        graph["ilium-server"] = []
        with self.assertRaises(ValueError):
            audit.validate_closure("macos", graph, inventory, set(graph))

    def test_macos_loader_inspection_excludes_lc_id_dylib_without_dropping_load_edges(self):
        audit = self.module("audit_native")
        code = {"ilium", "ilium-server", "libfirst.1.0.0.dylib"}
        dependency = "@executable_path/libfirst.1.0.0.dylib"
        def native_command(command, **options):
            name = Path(command[-1]).name
            if command[:2] == ["lipo", "-archs"]:
                return subprocess.CompletedProcess(command, 0, "x86_64\n", "")
            if command[:2] == ["otool", "-L"]:
                return subprocess.CompletedProcess(command, 0, f"{name}:\n\t{dependency} (compatibility version 1.0.0, current version 1.0.0)\n", "")
            if command[:2] == ["otool", "-l"]:
                command_kind = "LC_ID_DYLIB" if name.endswith(".dylib") else "LC_LOAD_DYLIB"
                return subprocess.CompletedProcess(command, 0, f"{name}:\nLoad command 0\n          cmd {command_kind}\n      cmdsize 64\n         name {dependency} (offset 24)\n", "")
            raise AssertionError(command)
        with patch.object(audit, "run", side_effect=native_command):
            graph, _ = audit.inspect_graph({"os": "macos", "arch": "x86_64"}, Path("/native/installed"), code, None)
        self.assertEqual(graph, {"ilium": [dependency], "ilium-server": [dependency], "libfirst.1.0.0.dylib": []})

    def test_unsigned_macos_repairs_adhoc_signatures_after_relocation(self):
        audit = self.module("audit_native")
        signed = set()
        def native_command(command, **options):
            name = Path(command[-1]).name
            if command[:2] == ["codesign", "--force"]:
                signed.add(name)
                return subprocess.CompletedProcess(command, 0, "", "")
            if command[:2] == ["codesign", "--display"]:
                return subprocess.CompletedProcess(command, 0, "", "Signature=adhoc")
            if command[:2] == ["codesign", "--verify"]:
                if name not in signed:
                    raise ValueError("fixture relocation invalidated the initial signature")
                return subprocess.CompletedProcess(command, 0, "", "valid on disk")
            raise AssertionError(command)
        with patch.object(audit, "run", side_effect=native_command):
            signing, notarization = audit.sign_macos(Path("/native/candidate"), {"ilium", "ilium-server", "libonnxruntime.1.24.2.dylib"}, None, None, Path("/native/output"))
        self.assertEqual(signing["state"], "unsigned")
        self.assertFalse(signing["credentials_present"])
        self.assertEqual(set(signing["nested_code"]), {"ilium", "ilium-server", "libonnxruntime.1.24.2.dylib"})
        self.assertEqual(notarization["state"], "disabled")

    def test_audit_output_cannot_overwrite_candidate_or_evidence_inputs(self):
        audit = self.module("audit_native")
        self.assertTrue(hasattr(audit, "validate_output_paths"), "audit output ownership guard is missing")
        with tempfile.TemporaryDirectory(prefix="ilium-audit-output-fixture-") as directory:
            root = Path(directory)
            candidate = root / "installed"
            candidate.mkdir()
            evidence = root / "runtime.json"
            evidence.write_text("fixture evidence")
            audit.validate_output_paths(root / "audit.json", candidate / "THIRD-PARTY.txt", candidate, [evidence])
            for output in (evidence, candidate / "ilium", candidate / "audit.json"):
                with self.subTest(output=output):
                    with self.assertRaises(ValueError):
                        audit.validate_output_paths(output, candidate / "THIRD-PARTY.txt", candidate, [evidence])
            self.assertEqual(evidence.read_text(), "fixture evidence")

    def test_checked_in_inventories_block_publication_until_reviewed_candidate_exists(self):
        audit = self.module("audit_native")
        document = json.loads((ROOT / "release/windows-runtime.json").read_text())
        self.assertEqual(document["state"], "blocked")
        self.assertFalse(document["publication_allowed"])
        with self.assertRaises(ValueError):
            audit.validate_runtime_inventory(document, "x86_64-pc-windows-msvc")
        with self.assertRaises(ValueError):
            audit.validate_notices((ROOT / "release/THIRD-PARTY.txt").read_bytes())

    def test_intel_build_requires_native_host_and_reviewed_pinned_source(self):
        build = self.module("build_intel_ort")
        for system, machine in (("Linux", "x86_64"), ("Darwin", "arm64"), ("Windows", "AMD64")):
            with self.subTest(system=system, machine=machine):
                with self.assertRaises(ValueError):
                    build.require_intel_host(system, machine)
        register = {"schema": 1, "state": "reviewed", "version": "1.24.2", "tag": "v1.24.2", "commit": "058787ceead760166e3c50a0a4cba8a833a6f53f", "source_url": "https://codeload.github.com/microsoft/onnxruntime/tar.gz/058787ceead760166e3c50a0a4cba8a833a6f53f", "source_sha256": "a" * 64, "reviewed_by": "fixture reviewer"}
        build.validate_source_register(register)
        for changes in ({"commit": "0" * 40}, {"tag": "v1.24.1"}, {"source_sha256": ""}, {"state": "blocked"}, {"reviewed_by": ""}, {"source_url": "https://evil.invalid/source.tar.gz"}):
            with self.subTest(changes=changes):
                with self.assertRaises(ValueError):
                    build.validate_source_register(dict(register, **changes))

    def test_notices_require_complete_lockfile_coverage_and_real_licence_bytes(self):
        audit = self.module("audit_native")
        with tempfile.TemporaryDirectory(prefix="ilium-notices-fixture-") as directory:
            root = Path(directory)
            lock = root / "Cargo.lock"
            lock.write_text('version = 4\n[[package]]\nname = "dep"\nversion = "1.2.3"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "' + 'a' * 64 + '"\n')
            licence = root / "LICENSE"
            licence.write_text("Actual synthetic fixture licence text\n")
            inventory = {"schema": 1, "state": "reviewed", "lock_sha256": hashlib.sha256(lock.read_bytes()).hexdigest(), "packages": [{"name": "dep", "version": "1.2.3", "source": "registry+https://github.com/rust-lang/crates.io-index", "reviewed": True, "license": "MIT", "license_source": "https://example.invalid/fixture/LICENSE", "license_file": str(licence), "license_sha256": hashlib.sha256(licence.read_bytes()).hexdigest()}]}
            notices = audit.generate_notices(lock, inventory, {"files": []})
            self.assertIn(b"Actual synthetic fixture licence text", notices)
            inventory["packages"] = []
            with self.assertRaises(ValueError):
                audit.generate_notices(lock, inventory, {"files": []})

    def test_intel_source_archive_hash_and_tag_resolution_must_match(self):
        build = self.module("build_intel_ort")
        with tempfile.TemporaryDirectory(prefix="ilium-ort-source-fixture-") as directory:
            archive = Path(directory) / "source.tar.gz"
            archive.write_bytes(b"synthetic archive hash fixture")
            with self.assertRaises(ValueError):
                build.verify_source_hash(archive, "0" * 64)
            build.verify_source_hash(archive, hashlib.sha256(archive.read_bytes()).hexdigest())
        with self.assertRaises(ValueError):
            build.verify_tag_commit({"object": {"type": "commit", "sha": "0" * 40}})
        self.assertEqual(build.verify_tag_commit({"object": {"type": "commit", "sha": "058787ceead760166e3c50a0a4cba8a833a6f53f"}}), "058787ceead760166e3c50a0a4cba8a833a6f53f")

    def test_intel_build_uses_shared_runtime_and_avoids_archive_git_mutation(self):
        build = self.module("build_intel_ort")
        self.assertTrue(hasattr(build, "build_command"), "pinned source archive build command is missing")
        command = build.build_command(Path("/native/source"), Path("/native/build"), 4)
        self.assertEqual(command, ["/native/source/build.sh", "--config", "Release", "--build_shared_lib", "--parallel", "4", "--use_xcode", "--skip_submodule_sync", "--build_dir", "/native/build", "--cmake_extra_defines", "CMAKE_OSX_ARCHITECTURES=x86_64"])
        environment = build.cargo_environment(Path("/native/ort"), Path("/native/cargo-home"), Path("/native/cargo-target"))
        self.assertEqual(environment["ORT_LIB_LOCATION"], "/native/ort")
        self.assertEqual(environment["ORT_LIB_PATH"], "/native/ort")
        self.assertEqual(environment["ORT_PREFER_DYNAMIC_LINK"], "1")

    def test_native_tools_help_and_invalid_flags_emit_jsonl(self):
        for name in ("audit_native", "build_intel_ort"):
            self.module(name)
            for arguments, expected_exit in ((["--help"], 0), (["--unknown"], 2)):
                with self.subTest(name=name, arguments=arguments):
                    result = subprocess.run([sys.executable, "-B", str(ROOT / f"release/scripts/{name}.py"), *arguments], capture_output=True, text=True)
                    self.assertEqual(result.returncode, expected_exit, result.stderr)
                    records = [json.loads(line) for line in result.stdout.splitlines()]
                    self.assertEqual(len(records), 1)
                    self.assertIn("type", records[0])

    @unittest.skipUnless(platform.system() != "Darwin" or platform.machine() != "x86_64", "non-Intel host gate only")
    def test_intel_cli_rejects_this_host_before_download_or_filesystem_work(self):
        self.module("build_intel_ort")
        with tempfile.TemporaryDirectory(prefix="ilium-ort-host-guard-fixture-") as directory:
            root = Path(directory)
            arguments = [sys.executable, "-B", str(ROOT / "release/scripts/build_intel_ort.py")]
            for flag in ("source-register", "source-archive", "output-root", "output", "cargo-environment-output", "cargo-workspace", "cargo-target-dir", "cargo-home"):
                arguments += ["--" + flag, str(root / flag)]
            arguments += ["--download-source", "--runner-identity", "synthetic host gate", "--parallel", "2"]
            result = subprocess.run(arguments, capture_output=True, text=True)
            self.assertEqual(result.returncode, 2, result.stderr)
            record = json.loads(result.stdout)
            self.assertEqual(record["state"], "blocked")
            self.assertIn("requires native macOS", record["error"])
            self.assertEqual(list(root.iterdir()), [])

    @unittest.skipUnless(platform.system() == "Linux" and platform.machine() == "x86_64" and shutil.which("cc") and shutil.which("readelf"), "requires native Linux x86_64 ELF tools")
    def test_real_linux_loader_audit_binds_compiled_fixture_pair_and_notices(self):
        self.module("audit_native")
        with tempfile.TemporaryDirectory(prefix="ilium-linux-native-fixture-") as directory:
            root = Path(directory)
            candidate = root / "installed"
            candidate.mkdir()
            source = root / "fixture.c"
            for executable in ("ilium", "ilium-server"):
                source.write_text('#include <stdio.h>\n#include <string.h>\nint main(int argc, char **argv) { if (argc == 2 && strcmp(argv[1], "--version") == 0) { puts("' + executable + ' 0.1.0"); return 0; } return 2; }\n')
                subprocess.run(["cc", str(source), "-o", str(candidate / executable)], check=True, capture_output=True)
            (candidate / "VERSION").write_text("0.1.0\n")
            workspace = root / "Cargo.toml"
            workspace.write_text('[workspace.package]\nversion = "0.1.0"\n')
            lock = root / "Cargo.lock"
            lock.write_text('version = 4\n[[package]]\nname = "fixture"\nversion = "0.1.0"\n')
            licence = root / "LICENSE"
            licence.write_text("Synthetic native-fixture licence bytes\n")
            dependencies = root / "dependencies.json"
            dependencies.write_text(json.dumps({"schema": 1, "state": "reviewed", "lock_sha256": hashlib.sha256(lock.read_bytes()).hexdigest(), "packages": [{"name": "fixture", "version": "0.1.0", "source": "workspace-or-vendored", "reviewed": True, "license": "MIT", "license_source": "https://example.invalid/fixture/LICENSE", "license_file": str(licence), "license_sha256": hashlib.sha256(licence.read_bytes()).hexdigest()}]}))
            runtime = root / "runtime.json"
            runtime.write_text(json.dumps({"schema": 1, "state": "reviewed", "publication_allowed": True, "target": "x86_64-unknown-linux-gnu", "files": [], "system_libraries": [{"name": "libc.so.6", "reviewed": True, "source": "native glibc fixture"}]}))
            output = root / "audit.json"
            arguments = [sys.executable, "-B", str(ROOT / "release/scripts/audit_native.py"), "--manifest", str(ROOT / "release/targets.toml"), "--target", "x86_64-unknown-linux-gnu", "--workspace", str(workspace), "--lockfile", str(lock), "--tag", "v0.1.0", "--directory", str(candidate), "--runtime-inventory", str(runtime), "--dependency-inventory", str(dependencies), "--output", str(output), "--notices-output", str(candidate / "THIRD-PARTY.txt"), "--runner-identity", "synthetic native Linux fixture"]
            result = subprocess.run(arguments, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            receipt = json.loads(output.read_text())
            self.assertEqual(receipt["state"], "passed")
            self.assertEqual(receipt["binary_versions"], {"ilium": "ilium 0.1.0", "ilium-server": "ilium-server 0.1.0"})
            self.assertEqual(receipt["files"]["ilium"], hashlib.sha256((candidate / "ilium").read_bytes()).hexdigest())
            runtime.write_text(json.dumps({"schema": 1, "state": "blocked"}))
            result = subprocess.run(arguments, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            receipt = json.loads(output.read_text())
            self.assertEqual(receipt["state"], "blocked")
            self.assertFalse(receipt["publication_allowed"])


if __name__ == "__main__":
    unittest.main()
