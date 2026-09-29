"""Pinned Windows ONNX Runtime source-build contract tests."""

import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "release/scripts"
sys.path.insert(0, str(SCRIPTS))
MODULE_PATH = SCRIPTS / "build_windows_ort.py"


def load_builder():
    specification = importlib.util.spec_from_file_location("build_windows_ort", MODULE_PATH)
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


class WindowsOrtBuilderTests(unittest.TestCase):
    def test_builder_module_exists(self):
        self.assertTrue(MODULE_PATH.is_file())

    def test_host_gate_requires_windows_amd64_and_exact_runner(self):
        builder = load_builder()
        builder.require_windows_host("Windows", "AMD64", "windows-2022")
        for system, machine, runner in (
            ("Linux", "x86_64", "windows-2022"),
            ("Windows", "ARM64", "windows-2022"),
            ("Windows", "AMD64", "windows-latest"),
        ):
            with self.subTest(system=system, machine=machine, runner=runner):
                with self.assertRaises(ValueError):
                    builder.require_windows_host(system, machine, runner)

    def test_host_gate_precedes_source_read_network_and_output_creation(self):
        builder = load_builder()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            arguments = builder.parser().parse_args([
                "--source-register", str(root / "register.json"),
                "--source-archive", str(root / "source.tar.gz"),
                "--output-root", str(root / "output-root"),
                "--output", str(root / "output-root/receipt.json"),
                "--cargo-environment-output", str(root / "output-root/environment.json"),
                "--cargo-workspace", str(root / "Cargo.toml"),
                "--cargo-target-dir", str(root / "cargo-target"),
                "--cargo-home", str(root / "cargo-home"),
                "--runner-identity", "windows-2022",
                "--parallel", "2",
            ])
            with patch.object(builder.platform, "system", return_value="Linux"), \
                 patch.object(builder.platform, "machine", return_value="x86_64"), \
                 patch.object(builder, "read_json") as read_json, \
                 patch.object(builder, "upstream_tag") as upstream_tag:
                with self.assertRaisesRegex(ValueError, "native Windows AMD64"):
                    builder.build(arguments)
            read_json.assert_not_called()
            upstream_tag.assert_not_called()
            self.assertFalse(arguments.output_root.exists())

    def test_commands_and_environment_encode_static_crt_contract(self):
        builder = load_builder()
        source = Path("C:/owned/source/onnxruntime-" + builder.COMMIT)
        output = Path("C:/owned/ort-build")
        self.assertEqual(builder.build_command(source, output, 4, "14.40.33807"), [
            str(source / "build.bat"), "--config", "Release", "--build_shared_lib",
            "--enable_msvc_static_runtime", "--cmake_generator", "Visual Studio 17 2022",
            "--msvc_toolset", "14.40.33807", "--parallel", "4",
            "--skip_submodule_sync", "--build_dir", str(output),
        ])
        environment = builder.cargo_environment(
            Path("C:/owned/ort-lib"), Path("C:/owned/cargo-home"), Path("C:/owned/cargo-target")
        )
        self.assertEqual(environment["ORT_LIB_LOCATION"], "C:/owned/ort-lib")
        self.assertEqual(environment["ORT_LIB_PATH"], "C:/owned/ort-lib")
        self.assertEqual(environment["ORT_PREFER_DYNAMIC_LINK"], "1")
        self.assertEqual(environment["CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS"], "-Ctarget-feature=+crt-static")
        self.assertNotIn("CARGO_ENCODED_RUSTFLAGS", environment)
        self.assertNotIn("RUSTFLAGS", environment)

    def test_cmake_cache_binds_selected_visual_studio_instance_compilers_and_static_runtime(self):
        builder = load_builder()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            installation = root / "Microsoft Visual Studio/2022/Enterprise"
            compiler = installation / "VC/Tools/MSVC/14.40.33807/bin/Hostx64/x64/cl.exe"
            compiler.parent.mkdir(parents=True)
            compiler.write_bytes(b"fixture")
            source = root / ("source/onnxruntime-" + builder.COMMIT)
            cache = root / "build/Windows/Release/CMakeCache.txt"
            cache.parent.mkdir(parents=True)
            cache.write_text("\n".join([
                "CMAKE_GENERATOR:INTERNAL=Visual Studio 17 2022",
                f"CMAKE_GENERATOR_INSTANCE:INTERNAL={installation}",
                "CMAKE_GENERATOR_TOOLSET:INTERNAL=host=x64,version=14.40.33807",
                f"CMAKE_C_COMPILER:FILEPATH={compiler}",
                f"CMAKE_CXX_COMPILER:FILEPATH={compiler}",
                "CMAKE_MSVC_RUNTIME_LIBRARY:STRING=MultiThreaded$<$<CONFIG:Debug>:Debug>",
                f"CMAKE_HOME_DIRECTORY:INTERNAL={source / 'cmake'}",
            ]) + "\n")
            selection = {
                "installation_path": str(installation),
                "installation_version": "17.11.35222.181",
                "toolset_version": "14.40.33807",
                "compiler_path": str(compiler),
            }
            identity = builder.cmake_build_identity(root / "build", source, selection)
            self.assertEqual(identity["path"], str(cache.resolve()))
            self.assertEqual(identity["values"]["CMAKE_GENERATOR"], "Visual Studio 17 2022")
            self.assertEqual(identity["values"]["CMAKE_C_COMPILER"], str(compiler))
            self.assertRegex(identity["sha256"], r"^[0-9a-f]{64}$")
            cache.write_text(cache.read_text().replace("MultiThreaded$<$<CONFIG:Debug>:Debug>", "MultiThreadedDLL"))
            with self.assertRaisesRegex(ValueError, "static MSVC runtime"):
                builder.cmake_build_identity(root / "build", source, selection)

    def test_parse_cmake_cache_rejects_duplicate_keys(self):
        builder = load_builder()
        with tempfile.TemporaryDirectory() as temporary:
            cache = Path(temporary) / "CMakeCache.txt"
            cache.write_text("CMAKE_GENERATOR:INTERNAL=one\nCMAKE_GENERATOR:INTERNAL=two\n")
            with self.assertRaisesRegex(ValueError, "duplicate"):
                builder.parse_cmake_cache(cache)

    def test_output_selection_requires_one_dll_and_matching_import_library(self):
        builder = load_builder()
        with tempfile.TemporaryDirectory() as temporary:
            build = Path(temporary)
            release = build / "Windows/Release/Release"
            release.mkdir(parents=True)
            dll = release / "onnxruntime.dll"
            import_library = release / "onnxruntime.lib"
            dll.write_bytes(b"dll")
            import_library.write_bytes(b"lib")
            self.assertEqual(builder.find_outputs(build), (dll.resolve(), import_library.resolve()))
            (build / "duplicate").mkdir()
            (build / "duplicate/onnxruntime.dll").write_bytes(b"other")
            with self.assertRaisesRegex(ValueError, "exactly one"):
                builder.find_outputs(build)


if __name__ == "__main__":
    unittest.main()
