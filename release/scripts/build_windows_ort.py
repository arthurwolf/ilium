#!/usr/bin/env python3
"""Build pinned ONNX Runtime with static MSVC CRT, then the Windows pair.

The helper shares the reviewed source identity and safe archive extraction with
the Intel builder.  It emits build evidence only; dumpbin closure and real
installed inference remain independent publication gates.
"""

import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import tarfile

from audit_native import atomic_write
from build_intel_ort import (
    COMMIT,
    TAG,
    VERSION,
    command_identity,
    request_bytes,
    unpack_source,
    upstream_tag,
    validate_source_register,
    verify_source_hash,
)
from release_tool import JsonArgumentParser, ReleaseError, digest, emit, read_json


TARGET = "x86_64-pc-windows-msvc"
RUNNER = "windows-2022"
TARGET_RUSTFLAGS = "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS"
STATIC_CRT_FLAG = "-Ctarget-feature=+crt-static"
GENERATOR = "Visual Studio 17 2022"
STATIC_MSVC_RUNTIME = "MultiThreaded$<$<CONFIG:Debug>:Debug>"


def require(condition, message):
    if not condition:
        raise ReleaseError(message)


def require_windows_host(system, machine, runner):
    require(
        system == "Windows" and machine == "AMD64" and runner == RUNNER,
        "Windows ORT build requires native Windows AMD64 on windows-2022; no download/build is allowed on another host",
    )


def build_command(source, output, parallel, toolset_version):
    return [
        str(source / "build.bat"),
        "--config", "Release",
        "--build_shared_lib",
        "--enable_msvc_static_runtime",
        "--cmake_generator", GENERATOR,
        "--msvc_toolset", toolset_version,
        "--parallel", str(parallel),
        "--skip_submodule_sync",
        "--build_dir", str(output),
    ]


def cargo_environment(runtime_directory, cargo_home, cargo_target):
    return {
        # Forward slashes are accepted by MSVC/Cargo and keep the emitted
        # receipt stable across Python's Windows and POSIX path renderers.
        "ORT_LIB_LOCATION": Path(runtime_directory).as_posix(),
        "ORT_LIB_PATH": Path(runtime_directory).as_posix(),
        "ORT_PREFER_DYNAMIC_LINK": "1",
        "CARGO_HOME": str(cargo_home),
        "CARGO_TARGET_DIR": str(cargo_target),
        TARGET_RUSTFLAGS: STATIC_CRT_FLAG,
    }


def find_outputs(build_directory):
    build_directory = Path(build_directory)
    dlls = [path.resolve() for path in build_directory.rglob("onnxruntime.dll")
            if path.is_file() and not path.is_symlink()]
    libraries = [path.resolve() for path in build_directory.rglob("onnxruntime.lib")
                 if path.is_file() and not path.is_symlink()]
    require(len(dlls) == 1, "Windows ORT build must produce exactly one regular onnxruntime.dll")
    require(len(libraries) == 1, "Windows ORT build must produce exactly one regular onnxruntime.lib")
    require(dlls[0].parent == libraries[0].parent, "ONNX Runtime DLL and import library must share one reviewed output directory")
    return dlls[0], libraries[0]


def logged_command(command, cwd, environment, log_path):
    emit({"type": "progress", "command": "build-windows-ort", "operation": "native-build",
          "argv": list(map(str, command)), "cwd": str(cwd), "log_path": str(log_path)})
    with Path(log_path).open("x", encoding="utf-8") as log:
        result = subprocess.run(list(map(str, command)), cwd=cwd, env=environment,
                                stdout=log, stderr=subprocess.STDOUT, check=False)
    emit({"type": "artifact", "path": str(Path(log_path).resolve()),
          "sha256": digest(Path(log_path).read_bytes())})
    require(result.returncode == 0, f"native build failed exit={result.returncode}; complete log: {log_path}")


def parse_cmake_cache(path):
    values = {}
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        if not line or line.startswith(("#", "//")) or "=" not in line:
            continue
        key_and_type, value = line.split("=", 1)
        if ":" not in key_and_type:
            continue
        key, _value_type = key_and_type.split(":", 1)
        require(key not in values, f"duplicate CMake cache key: {key}")
        values[key] = value
    return values


def same_windows_path(left, right):
    return os.path.normpath(str(left)).casefold() == os.path.normpath(str(right)).casefold()


def select_msvc_compiler(installation, compiler_paths):
    installation = Path(installation).resolve()
    candidates = []
    for path in compiler_paths:
        candidate = Path(path)
        if not candidate.is_absolute() or not candidate.is_file():
            continue
        resolved = candidate.resolve()
        if not any(same_windows_path(parent, installation) for parent in resolved.parents):
            continue
        parts = resolved.parts
        indices = [index for index, value in enumerate(parts) if value.casefold() == "msvc"]
        if len(indices) != 1 or indices[0] + 1 >= len(parts):
            continue
        toolset_version = parts[indices[0] + 1]
        if not re.fullmatch(r"14\.[0-9.]+", toolset_version):
            continue
        candidates.append((tuple(int(part) for part in toolset_version.split(".")), resolved, toolset_version))
    require(candidates, "cannot resolve a native x64 MSVC compiler")
    newest = max(item[0] for item in candidates)
    selected = [item for item in candidates if item[0] == newest]
    require(len(selected) == 1, "cannot resolve one native x64 MSVC compiler for the newest toolset")
    _version, compiler, toolset_version = selected[0]
    return compiler, toolset_version


def generated_compiler_path(cache_path, language):
    """Compiler recorded by CMake's own compiler-detection output, or None."""
    files = sorted(Path(cache_path).parent.glob(f"CMakeFiles/*/CMake{language}Compiler.cmake"))
    files = [file for file in files if file.is_file() and not file.is_symlink()]
    if len(files) != 1:
        return None
    match = re.search(rf'^set\(CMAKE_{language}_COMPILER "([^"]+)"\)', files[0].read_text(encoding="utf-8"), re.MULTILINE)
    return match.group(1) if match else None


def cmake_build_identity(build_directory, source, selection):
    matches = []
    for path in Path(build_directory).rglob("CMakeCache.txt"):
        if path.is_file() and not path.is_symlink():
            values = parse_cmake_cache(path)
            if (values.get("CMAKE_GENERATOR") == GENERATOR and
                    same_windows_path(values.get("CMAKE_HOME_DIRECTORY", ""), Path(source) / "cmake")):
                matches.append((path.resolve(), values))
    require(len(matches) == 1, "Windows ORT build must have exactly one top-level CMake cache")
    path, values = matches[0]
    # Visual Studio generators do not cache the compiler: CMake records the
    # resolved cl.exe in the generated CMake{C,CXX}Compiler.cmake files beside
    # the cache. Those files are the authoritative identity, so fall back to
    # them only when the cache itself lacks the entry.
    for language, key in (("C", "CMAKE_C_COMPILER"), ("CXX", "CMAKE_CXX_COMPILER")):
        if not values.get(key):
            generated = generated_compiler_path(path, language)
            if generated is not None:
                values[key] = generated
    required = ("CMAKE_GENERATOR", "CMAKE_GENERATOR_INSTANCE", "CMAKE_GENERATOR_TOOLSET",
                "CMAKE_C_COMPILER", "CMAKE_CXX_COMPILER", "CMAKE_MSVC_RUNTIME_LIBRARY",
                "CMAKE_HOME_DIRECTORY")
    missing = [name for name in required if not (isinstance(values.get(name), str) and values[name])]
    require(not missing, "Windows ORT CMake cache lacks required build identity: " + ", ".join(missing))
    require(same_windows_path(values["CMAKE_GENERATOR_INSTANCE"], selection["installation_path"]),
            "CMake generator instance differs from selected Visual Studio instance")
    require(values["CMAKE_GENERATOR_TOOLSET"] == "host=x64,version=" + selection["toolset_version"],
            "CMake generator toolset differs from selected MSVC toolset")
    for key in ("CMAKE_C_COMPILER", "CMAKE_CXX_COMPILER"):
        require(Path(values[key]).is_absolute() and same_windows_path(values[key], selection["compiler_path"]),
                f"{key} differs from the selected native x64 compiler")
    require(values["CMAKE_MSVC_RUNTIME_LIBRARY"] == STATIC_MSVC_RUNTIME,
            "CMake cache does not prove the static MSVC runtime")
    return {"path": str(path), "sha256": digest(path.read_bytes()),
            "values": {name: values[name] for name in required}}


def resolve_msvc_toolchain(probe_directory):
    program_files = os.environ.get("ProgramFiles(x86)")
    require(bool(program_files), "ProgramFiles(x86) is missing on the Windows runner")
    vswhere = Path(program_files) / "Microsoft Visual Studio/Installer/vswhere.exe"
    require(vswhere.is_absolute() and vswhere.is_file(), "vswhere.exe is missing from the Windows runner")
    prefix = [str(vswhere), "-latest", "-version", "[17.0,18.0)", "-products", "*",
              "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64"]
    installation_result = command_identity(prefix + ["-property", "installationPath"])
    installation_paths = [Path(line.strip()) for line in installation_result["stdout"].splitlines() if line.strip()]
    require(len(installation_paths) == 1 and installation_paths[0].is_absolute() and installation_paths[0].is_dir(),
            "cannot resolve one Visual Studio 2022 installation")
    installation = installation_paths[0].resolve()
    version_result = command_identity(prefix + ["-property", "installationVersion"])
    versions = [line.strip() for line in version_result["stdout"].splitlines() if line.strip()]
    require(len(versions) == 1 and re.fullmatch(r"17\.[0-9.]+", versions[0]),
            "cannot resolve the selected Visual Studio 2022 version")
    compiler_result = command_identity(prefix + ["-find", "VC/Tools/MSVC/**/bin/Hostx64/x64/cl.exe"])
    paths = [Path(line.strip()) for line in compiler_result["stdout"].splitlines() if line.strip()]
    compiler, toolset_version = select_msvc_compiler(installation, paths)
    probe_source = Path(probe_directory) / "msvc-identity-probe.c"
    probe_object = Path(probe_directory) / "msvc-identity-probe.obj"
    probe_source.write_text("int ilium_msvc_identity_probe(void) { return 0; }\n", encoding="utf-8")
    return {
        "vswhere": installation_result,
        "vswhere_version": version_result,
        "vswhere_compiler": compiler_result,
        "selection": {"installation_path": str(installation), "installation_version": versions[0],
                      "toolset_version": toolset_version, "compiler_path": str(compiler)},
        "cl": command_identity([str(compiler), "/nologo", "/Bv", "/c", str(probe_source), "/Fo" + str(probe_object)]),
        "cmake": command_identity(["cmake", "--version"]),
        "rustc": command_identity(["rustc", "-Vv"]),
        "cargo": command_identity(["cargo", "--version"]),
        "python": sys.version,
    }


def build(arguments):
    # This must precede register reads, network access, and output creation.
    require_windows_host(platform.system(), platform.machine(), arguments.runner_identity)
    register = validate_source_register(read_json(arguments.source_register))
    for path in (arguments.output_root, arguments.cargo_target_dir, arguments.cargo_home):
        require(path.is_absolute() and not path.exists() and not path.is_symlink(),
                f"build path must be explicit, new and disjoint: {path}")
    paths = [path.resolve() for path in (arguments.output_root, arguments.cargo_target_dir, arguments.cargo_home)]
    require(all(left != right and left not in right.parents and right not in left.parents
                for index, left in enumerate(paths) for right in paths[index + 1:]),
            "ORT/Cargo output paths overlap")
    require(arguments.cargo_workspace.is_absolute() and arguments.cargo_workspace.is_file(),
            "Cargo workspace manifest must be explicit and absolute")
    for output in (arguments.output, arguments.cargo_environment_output):
        require(output.is_absolute() and output.parent.resolve() == arguments.output_root.resolve(),
                "ORT receipt outputs must be distinct files directly in the owned output root")
    require(arguments.output != arguments.cargo_environment_output,
            "ORT receipt and Cargo environment outputs must differ")
    require(arguments.source_archive.is_absolute(), "source archive path must be absolute")
    if arguments.download_source:
        require(not arguments.source_archive.exists(), "download refuses to replace an existing source archive")
        content = request_bytes(register["source_url"])
        require(digest(content) == register["source_sha256"],
                "downloaded ORT source archive differs from reviewed hash")
        with arguments.source_archive.open("xb") as source_file:
            source_file.write(content)
    verify_source_hash(arguments.source_archive, register["source_sha256"])
    tag_evidence = upstream_tag()
    arguments.output_root.mkdir(mode=0o700)
    arguments.cargo_target_dir.mkdir(mode=0o700)
    arguments.cargo_home.mkdir(mode=0o700)
    toolchain = resolve_msvc_toolchain(arguments.output_root)
    source = unpack_source(arguments.source_archive, arguments.output_root / "source")
    require((source / "build.bat").is_file(), "ORT source archive has no upstream Windows build entrypoint")
    native_build = arguments.output_root / "build"
    selection = toolchain["selection"]
    ort_command = build_command(source, native_build, arguments.parallel, selection["toolset_version"])
    environment = dict(os.environ)
    for variable in ("ORT_LIB_LOCATION", "ORT_LIB_PATH", "ORT_PREFER_DYNAMIC_LINK",
                     "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", TARGET_RUSTFLAGS):
        environment.pop(variable, None)
    # CMake applies CMAKE_GENERATOR_INSTANCE from the environment only when the
    # generator is also chosen through the environment; ORT's build.py passes
    # -G on the command line, which would otherwise ignore the instance.
    ort_environment = {"CMAKE_GENERATOR": GENERATOR,
                       "CMAKE_GENERATOR_INSTANCE": selection["installation_path"]}
    environment.update(ort_environment)
    # Keep the build command as individual argv entries.  Passing one
    # pre-quoted command-line string after /c makes cmd.exe reinterpret the
    # escaped quotes and splits the multi-word generator name at the space.
    invocation = ["cmd.exe", "/d", "/c", *map(str, ort_command)]
    logged_command(invocation, source, environment, arguments.output_root / "ort-build.log")
    cmake_cache = cmake_build_identity(native_build, source, selection)
    runtime, import_library = find_outputs(native_build)
    cargo_values = cargo_environment(runtime.parent, arguments.cargo_home, arguments.cargo_target_dir)
    environment.update(cargo_values)
    cargo_command = ["cargo", "build", "--locked", "--release", "--manifest-path",
                     str(arguments.cargo_workspace), "--target", TARGET,
                     "--bin", "ilium", "--bin", "ilium-server"]
    logged_command(cargo_command, arguments.cargo_workspace.parent, environment,
                   arguments.output_root / "cargo-build.log")
    binary_directory = arguments.cargo_target_dir / TARGET / "release"
    binaries = {name: digest((binary_directory / name).read_bytes())
                for name in ("ilium.exe", "ilium-server.exe")}
    receipt = {
        "schema": 1,
        "state": "built-not-qualified",
        "publication_allowed": False,
        "source_register": register,
        "source_register_sha256": digest(arguments.source_register.read_bytes()),
        "tag_resolution": tag_evidence,
        "toolchain": toolchain,
        "runner_identity": arguments.runner_identity,
        "native_identity": {"system": platform.system(), "machine": platform.machine(),
                            "release": platform.release(), "version": platform.version()},
        "ort_command": ort_command,
        "ort_invocation": invocation,
        "ort_environment": ort_environment,
        "cmake_cache": cmake_cache,
        "cargo_command": cargo_command,
        "environment": cargo_values,
        "runtime": {"path": str(runtime), "sha256": digest(runtime.read_bytes()), "version": VERSION},
        "import_library": {"path": str(import_library), "sha256": digest(import_library.read_bytes())},
        "binaries": binaries,
        "workspace_sha256": digest(arguments.cargo_workspace.read_bytes()),
        "lock_sha256": digest((arguments.cargo_workspace.parent / "Cargo.lock").read_bytes()),
        "remaining_gate": "recursive native dumpbin closure and real post-install embedding inference",
    }
    atomic_write(arguments.output, (json.dumps(receipt, indent=2, sort_keys=True) + "\n").encode())
    atomic_write(arguments.cargo_environment_output,
                 (json.dumps(receipt["environment"], indent=2, sort_keys=True) + "\n").encode())
    emit({"type": "artifact", "path": str(arguments.output.resolve()),
          "sha256": digest(arguments.output.read_bytes())})
    emit({"type": "artifact", "path": str(arguments.cargo_environment_output.resolve()),
          "sha256": digest(arguments.cargo_environment_output.read_bytes())})
    emit({"type": "result", "command": "build-windows-ort", "state": "built-not-qualified",
          "publication_allowed": False, "runtime": str(runtime), "import_library": str(import_library)})


def parser():
    result = JsonArgumentParser(description=__doc__, allow_abbrev=False)
    for flag in ("source-register", "source-archive", "output-root", "output",
                 "cargo-environment-output", "cargo-workspace", "cargo-target-dir", "cargo-home"):
        result.add_argument("--" + flag, type=Path, required=True)
    result.add_argument("--download-source", action="store_true")
    result.add_argument("--runner-identity", required=True)
    result.add_argument("--parallel", type=int, required=True)
    return result


def main(argv=None):
    arguments = None
    try:
        arguments = parser().parse_args(argv)
        require(1 <= arguments.parallel <= 64, "parallel worker count must be 1 through 64")
        build(arguments)
        return 0
    except (ValueError, OSError, UnicodeError, subprocess.SubprocessError,
            tarfile.TarError, KeyError, TypeError, AttributeError) as error:
        emit({"type": "error", "command": "build-windows-ort", "state": "blocked",
              "publication_allowed": False, "error": str(error)[:2000]})
        return 2


if __name__ == "__main__":
    sys.exit(main())
