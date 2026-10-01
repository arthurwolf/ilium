"""Fail-closed native candidate audit. All paths are explicit; stdout is JSONL.

Runtime inventory schema 1: state=reviewed, target, files (name, sha256,
version, reviewed, license, license_source, license_file, license_sha256),
system_libraries (name, reviewed, source). Every shipped library is inspected
recursively. System entries are constrained to OS namespaces; declaring an
arbitrary library "system" cannot exempt it from packaging.

Dependency inventory schema 1: state=reviewed, lock_sha256, packages with
name/version/source and the same reviewed licence fields. It covers every
Cargo.lock package, including vendored/workspace source entries. Licence text
is read and hashed, never inferred from a package's SPDX label alone.
"""

import argparse
import json
import math
import os
from pathlib import Path, PurePosixPath, PureWindowsPath
import platform
import re
import selectors
import subprocess
import sys
import tempfile
import time
import tomllib

from release_tool import (JsonArgumentParser, ReleaseError, digest, emit,
                          read_json, safe_member_name, selected_target,
                          workspace_version)


def require(condition, message):
    if not condition:
        raise ReleaseError(message)


def atomic_write(path, content):
    path = Path(path)
    require(not path.is_symlink(), f"output must not be a link: {path}")
    with tempfile.NamedTemporaryFile(prefix=".ilium-audit-", dir=path.parent, delete=False) as temporary:
        temporary_path = Path(temporary.name)
        try:
            temporary.write(content)
            temporary.flush()
            os.fsync(temporary.fileno())
            temporary.close()
            os.replace(temporary_path, path)
        finally:
            if temporary_path.exists():
                temporary_path.unlink()


def validate_output_paths(output, notices, directory, inputs):
    output, notices, directory = Path(output), Path(notices), Path(directory).resolve()
    protected = {Path(path).resolve() for path in inputs if path is not None}
    require(not output.is_symlink() and output.resolve() not in protected and output.resolve() != directory and directory not in output.resolve().parents, "audit report would overwrite candidate/evidence inputs")
    require(not notices.is_symlink() and notices.resolve() == directory / "THIRD-PARTY.txt" and notices.resolve() not in protected, "notices output must be the candidate THIRD-PARTY.txt")


def native_identity(target, runner):
    identity = {"system": platform.system(), "machine": platform.machine(),
                "release": platform.release(), "version": platform.version(), "runner": runner}
    expected = {"linux": "Linux", "macos": "Darwin", "windows": "Windows"}[target["os"]]
    architectures = {"x86_64": {"x86_64", "AMD64"}, "aarch64": {"aarch64", "arm64", "ARM64"}}
    require(identity["system"] == expected and identity["machine"] in architectures[target["arch"]], "audit requires the matching native OS/architecture; cross-host evidence is prohibited")
    require(bool(runner.strip()), "runner identity is required")
    return identity


def run(command, environment=None, check=True, timeout=120):
    result = subprocess.run(list(map(str, command)), capture_output=True, text=True,
                            env=environment, timeout=timeout, check=False)
    if check and result.returncode != 0:
        raise ReleaseError(f"native command failed: {command[0]} exit={result.returncode}: {result.stderr[-2000:]}")
    return result


def parse_dependencies(operating_system, output):
    if operating_system == "linux":
        dependencies = re.findall(r"\(NEEDED\).*?\[([^\]]+)\]", output)
    elif operating_system == "macos":
        dependencies = re.findall(r"^\s+(.+?) \(compatibility version", output, re.MULTILINE)
    elif operating_system == "windows":
        dependencies = re.findall(r"^\s+([A-Za-z0-9_.-]+\.dll)\s*$", output, re.MULTILINE | re.IGNORECASE)
    else:
        raise ReleaseError("unknown loader format")
    require(len(dependencies) == len(set(dependencies)), "loader output has duplicate dependencies")
    return dependencies


def parse_macos_load_commands(output):
    """Distinguish a dylib's LC_ID_DYLIB identity from its actual load edges."""
    blocks = re.split(r"(?m)^\s*Load command [0-9]+\s*$", output)
    require(len(blocks) > 1, "Mach-O load-command evidence is missing")
    install_name, dependencies = None, []
    load_kinds = {"LC_LOAD_DYLIB", "LC_LOAD_WEAK_DYLIB", "LC_REEXPORT_DYLIB", "LC_LAZY_LOAD_DYLIB", "LC_LOAD_UPWARD_DYLIB"}
    for block in blocks[1:]:
        command = re.search(r"(?m)^\s+cmd (LC_[A-Z0-9_]+)\s*$", block)
        require(command is not None, "Mach-O load command has no command identity")
        kind = command[1]
        if kind != "LC_ID_DYLIB" and kind not in load_kinds:
            require(not kind.endswith("_DYLIB"), f"unsupported Mach-O dylib command: {kind}")
            continue
        name = re.search(r"(?m)^\s+name (.+?) \(offset [0-9]+\)\s*$", block)
        require(name is not None, f"Mach-O {kind} has no library name")
        if kind == "LC_ID_DYLIB":
            require(install_name is None, "Mach-O has duplicate install-name identities")
            install_name = name[1]
        else:
            dependencies.append(name[1])
    require(len(dependencies) == len(set(dependencies)), "Mach-O has duplicate load dependencies")
    return install_name, dependencies


def validate_runtime_inventory(inventory, target):
    require(inventory.get("schema") == 1 and inventory.get("state") == "reviewed" and inventory.get("publication_allowed") is True, "runtime inventory is blocked/unreviewed; publication prohibited")
    require(inventory.get("target") == target, "runtime inventory target differs")
    require(isinstance(inventory.get("files"), list) and isinstance(inventory.get("system_libraries"), list), "runtime inventory lacks files/system closure")
    names = []
    for item in inventory["files"]:
        safe_member_name(item.get("name"))
        require(item.get("reviewed") is True and bool(item.get("version")), "bundled runtime lacks reviewed version")
        require(isinstance(item.get("sha256"), str) and re.fullmatch(r"[0-9a-f]{64}", item["sha256"]), "bundled runtime lacks a SHA-256")
        names.append(item["name"].casefold())
    require(len(names) == len(set(names)), "duplicate bundled runtime entry")
    system_names = [item.get("name", "").casefold() for item in inventory["system_libraries"]]
    require(len(system_names) == len(set(system_names)), "duplicate system dependency entry")
    return inventory


def validate_intel_toolchain(toolchain):
    require(isinstance(toolchain, dict), "Intel ORT toolchain evidence must be structured")
    commands = {
        "clang": ["clang", "--version"], "xcode": ["xcodebuild", "-version"],
        "developer_directory": ["xcode-select", "-p"], "cmake": ["cmake", "--version"],
        "rustc": ["rustc", "-Vv"], "cargo": ["cargo", "--version"],
    }
    for name, command in commands.items():
        evidence = toolchain.get(name)
        require(isinstance(evidence, dict) and evidence.get("command") == command, f"Intel {name} identity command differs")
        require(isinstance(evidence.get("stdout"), str) and isinstance(evidence.get("stderr"), str), f"Intel {name} identity output is malformed")
    require(re.search(r"\b(?:Apple )?clang version [0-9]+\.[0-9]+", toolchain["clang"]["stdout"]), "Intel clang version evidence is missing")
    require(re.search(r"(?m)^Xcode [0-9]+(?:\.[0-9]+)*$", toolchain["xcode"]["stdout"]) and re.search(r"(?m)^Build version [A-Za-z0-9]+$", toolchain["xcode"]["stdout"]), "Intel Xcode version/build evidence is missing")
    require(PurePosixPath(toolchain["developer_directory"]["stdout"].strip()).is_absolute(), "Intel Xcode developer directory is not absolute")
    require(re.search(r"(?m)^cmake version [0-9]+\.[0-9]+", toolchain["cmake"]["stdout"]), "Intel CMake version evidence is missing")
    require(re.search(r"(?m)^rustc [0-9]+\.[0-9]+\.[0-9]+", toolchain["rustc"]["stdout"]) and re.search(r"(?m)^host: x86_64-apple-darwin$", toolchain["rustc"]["stdout"]), "Intel Rust compiler/version/host evidence is missing")
    require(re.search(r"(?m)^cargo [0-9]+\.[0-9]+\.[0-9]+", toolchain["cargo"]["stdout"]), "Intel Cargo version evidence is missing")
    require(isinstance(toolchain.get("python"), str) and re.match(r"3\.[0-9]+\.[0-9]+(?:\s|$)", toolchain["python"]), "Intel Python version evidence is missing")


def validate_intel_commands(receipt, workspace=None):
    ort = receipt.get("ort_command")
    require(isinstance(ort, list) and len(ort) == 13 and all(isinstance(value, str) for value in ort), "Intel ORT build command evidence is missing/malformed")
    source, build_directory = PurePosixPath(ort[0]), PurePosixPath(ort[10])
    require(source.is_absolute() and source.name == "build.sh" and source.parent.name == "onnxruntime-058787ceead760166e3c50a0a4cba8a833a6f53f" and ".." not in source.parts, "Intel ORT command is not from the pinned source tree")
    require(build_directory.is_absolute() and ".." not in build_directory.parts and re.fullmatch(r"[1-9][0-9]?", ort[5]) and 1 <= int(ort[5]) <= 64, "Intel ORT build directory/parallel evidence differs")
    expected_ort = [ort[0], "--config", "Release", "--build_shared_lib", "--parallel", ort[5], "--use_xcode", "--skip_submodule_sync", "--compile_no_warning_as_error", "--build_dir", ort[10], "--cmake_extra_defines", "CMAKE_OSX_ARCHITECTURES=x86_64"]
    require(ort == expected_ort, "Intel ORT command differs from the reviewed Xcode/x86_64 recipe")
    cargo = receipt.get("cargo_command")
    require(isinstance(cargo, list) and len(cargo) == 12 and all(isinstance(value, str) for value in cargo), "Intel Cargo command evidence is missing/malformed")
    manifest = PurePosixPath(cargo[5])
    require(manifest.is_absolute() and manifest.name == "Cargo.toml" and ".." not in manifest.parts, "Intel Cargo manifest identity is invalid")
    require(workspace is None or manifest == PurePosixPath(str(Path(workspace).resolve())), "Intel Cargo command used a different workspace path")
    expected_cargo = ["cargo", "build", "--locked", "--release", "--manifest-path", cargo[5], "--target", "x86_64-apple-darwin", "--bin", "ilium", "--bin", "ilium-server"]
    require(cargo == expected_cargo, "Intel Cargo command differs from the locked native release-pair build")
    environment = receipt.get("environment")
    require(isinstance(environment, dict), "Intel Cargo environment evidence is missing")
    for variable in ("ORT_LIB_LOCATION", "ORT_LIB_PATH", "CARGO_HOME", "CARGO_TARGET_DIR"):
        value = environment.get(variable)
        require(isinstance(value, str) and PurePosixPath(value).is_absolute() and ".." not in PurePosixPath(value).parts, f"Intel Cargo boundary has no absolute {variable}")
    runtime_path = receipt.get("runtime", {}).get("path")
    require(isinstance(runtime_path, str) and PurePosixPath(runtime_path).is_absolute() and PurePosixPath(runtime_path).name == "libonnxruntime.1.24.2.dylib", "Intel source-built runtime path is missing")
    runtime = PurePosixPath(runtime_path)
    require(build_directory in runtime.parents and PurePosixPath(environment["ORT_LIB_LOCATION"]) == runtime.parent and PurePosixPath(environment["ORT_LIB_PATH"]) == runtime.parent and environment.get("ORT_PREFER_DYNAMIC_LINK") == "1", "Intel Cargo ORT_LIB_LOCATION/ORT_LIB_PATH do not use the verified build output")
    outputs = [build_directory, PurePosixPath(environment["CARGO_HOME"]), PurePosixPath(environment["CARGO_TARGET_DIR"])]
    require(all(left != right and left not in right.parents and right not in left.parents for index, left in enumerate(outputs) for right in outputs[index + 1:]), "Intel ORT/Cargo output directories overlap")


def validate_intel_build_receipt(receipt, runtimes, runner="macos-15-intel", workspace=None):
    # Import at call time: the Intel builder reuses atomic_write from this module.
    from build_intel_ort import validate_source_register, verify_tag_commit
    require(receipt.get("schema") == 1 and receipt.get("state") == "built-not-qualified", "Intel ORT build receipt is missing or blocked")
    register = validate_source_register(receipt.get("source_register", {}))
    require(isinstance(receipt.get("source_register_sha256"), str) and re.fullmatch(r"[0-9a-f]{64}", receipt["source_register_sha256"]), "Intel reviewed source-register byte identity is missing")
    resolution = receipt.get("tag_resolution", {})
    require(isinstance(resolution, dict) and resolution.get("reference_url") == "https://api.github.com/repos/microsoft/onnxruntime/git/ref/tags/v1.24.2" and resolution.get("reference", {}).get("ref") == "refs/tags/v1.24.2", "Intel source tag-resolution identity is missing or differs")
    verify_tag_commit(resolution.get("reference", {}), resolution.get("tag_object"))
    identity = receipt.get("native_identity")
    require(isinstance(identity, dict) and identity.get("system") == "Darwin" and identity.get("machine") == "x86_64" and isinstance(identity.get("release"), str) and re.fullmatch(r"[0-9]+\.[0-9]+(?:\.[0-9]+)?", identity["release"]) and isinstance(identity.get("version"), str) and "Darwin Kernel Version" in identity["version"], "Intel ORT receipt has no structured native macOS Intel identity")
    require(receipt.get("runner_identity") == runner, "Intel ORT runner identity differs from the approved native runner")
    validate_intel_toolchain(receipt.get("toolchain"))
    validate_intel_commands(receipt, workspace)
    matches = [item for item in runtimes if item["name"] == "libonnxruntime.1.24.2.dylib"]
    require(len(matches) == 1 and matches[0].get("version") == "1.24.2" and receipt.get("runtime", {}).get("version") == "1.24.2" and matches[0].get("sha256") == receipt.get("runtime", {}).get("sha256"), "Intel candidate runtime differs from the pinned native build")
    return {"state": "passed", "source_tag": register["tag"], "source_commit": register["commit"], "source_sha256": register["source_sha256"], "built_runtime_sha256": matches[0]["sha256"]}


def validate_windows_build_receipt(receipt, runtimes, runner="windows-2025", workspace=None):
    from build_intel_ort import validate_source_register, verify_tag_commit
    def receipt_path(value):
        windows_path = PureWindowsPath(value)
        return windows_path if windows_path.is_absolute() else Path(value)

    def same_receipt_path(left, right):
        return receipt_path(left) == receipt_path(right)

    require(receipt.get("schema") == 1 and receipt.get("state") == "built-not-qualified" and receipt.get("publication_allowed") is False, "Windows ORT build receipt is missing, qualified prematurely, or blocked")
    register = validate_source_register(receipt.get("source_register", {}))
    require(isinstance(receipt.get("source_register_sha256"), str) and re.fullmatch(r"[0-9a-f]{64}", receipt["source_register_sha256"]), "Windows reviewed source-register byte identity is missing")
    resolution = receipt.get("tag_resolution", {})
    require(isinstance(resolution, dict) and resolution.get("reference_url") == "https://api.github.com/repos/microsoft/onnxruntime/git/ref/tags/v1.24.2" and resolution.get("reference", {}).get("ref") == "refs/tags/v1.24.2", "Windows source tag-resolution identity is missing or differs")
    verify_tag_commit(resolution.get("reference", {}), resolution.get("tag_object"))
    identity = receipt.get("native_identity")
    require(isinstance(identity, dict) and identity.get("system") == "Windows" and identity.get("machine") == "AMD64" and all(isinstance(identity.get(field), str) and identity[field] for field in ("release", "version")), "Windows ORT receipt has no structured native AMD64 identity")
    require(receipt.get("runner_identity") == runner, "Windows ORT runner identity differs from the approved native runner")
    toolchain = receipt.get("toolchain")
    require(isinstance(toolchain, dict), "Windows ORT toolchain evidence must be structured")
    for name in ("vswhere", "vswhere_version", "vswhere_compiler", "cl", "cmake", "rustc", "cargo"):
        evidence = toolchain.get(name)
        require(isinstance(evidence, dict) and isinstance(evidence.get("command"), list) and evidence["command"] and all(isinstance(item, str) for item in evidence["command"]) and isinstance(evidence.get("stdout"), str) and isinstance(evidence.get("stderr"), str), f"Windows {name} identity evidence is malformed")
    vswhere = toolchain["vswhere"]["command"][0]
    prefix = [vswhere, "-latest", "-version", "[17.0,18.0)", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64"]
    require(receipt_path(vswhere).is_absolute() and toolchain["vswhere"]["command"] == prefix + ["-property", "installationPath"] and toolchain["vswhere_version"]["command"] == prefix + ["-property", "installationVersion"] and toolchain["vswhere_compiler"]["command"] == prefix + ["-find", "VC/Tools/MSVC/**/bin/Hostx64/x64/cl.exe"], "Windows vswhere identity commands differ")
    selection = toolchain.get("selection")
    require(isinstance(selection, dict) and all(isinstance(selection.get(name), str) and selection[name] for name in ("installation_path", "installation_version", "toolset_version", "compiler_path")), "Windows selected Visual Studio identity is missing")
    require(toolchain["vswhere"]["stdout"].strip() == selection["installation_path"] and toolchain["vswhere_version"]["stdout"].strip() == selection["installation_version"] and toolchain["vswhere_compiler"]["stdout"].strip() == selection["compiler_path"] and re.fullmatch(r"17\.[0-9.]+", selection["installation_version"]) and re.fullmatch(r"14\.[0-9.]+", selection["toolset_version"]), "Windows selected Visual Studio identity differs from vswhere")
    compiler = receipt_path(toolchain["cl"]["command"][0])
    cl_arguments = toolchain["cl"]["command"][1:]
    compiler_output = toolchain["cl"]["stdout"] + "\n" + toolchain["cl"]["stderr"]
    require(compiler.is_absolute() and compiler.name.casefold() == "cl.exe" and len(cl_arguments) == 5 and cl_arguments[:3] == ["/nologo", "/Bv", "/c"] and receipt_path(cl_arguments[3]).is_absolute() and cl_arguments[4].startswith("/Fo") and receipt_path(cl_arguments[4][3:]).is_absolute() and re.search(r"Compiler Version [0-9.]+ for x64", compiler_output), "Windows native x64 MSVC identity differs")
    installation_path = receipt_path(selection["installation_path"])
    require(same_receipt_path(str(compiler), selection["compiler_path"]) and installation_path in compiler.parents, "Windows compiler probe is not from the selected Visual Studio instance")
    require(toolchain["cmake"]["command"] == ["cmake", "--version"] and re.search(r"(?m)^cmake version [0-9]+\.[0-9]+", toolchain["cmake"]["stdout"]), "Windows CMake identity differs")
    require(toolchain["rustc"]["command"] == ["rustc", "-Vv"] and re.search(r"(?m)^host: x86_64-pc-windows-msvc$", toolchain["rustc"]["stdout"]), "Windows Rust host identity differs")
    require(toolchain["cargo"]["command"] == ["cargo", "--version"] and re.search(r"(?m)^cargo [0-9]+\.[0-9]+\.[0-9]+", toolchain["cargo"]["stdout"]), "Windows Cargo identity differs")
    require(isinstance(toolchain.get("python"), str) and re.match(r"3\.[0-9]+\.[0-9]+(?:\s|$)", toolchain["python"]), "Windows Python identity differs")
    ort = receipt.get("ort_command")
    require(isinstance(ort, list) and len(ort) == 14 and all(isinstance(value, str) for value in ort), "Windows ORT build command evidence is missing/malformed")
    source, build_directory = receipt_path(ort[0]), receipt_path(ort[13])
    require(source.is_absolute() and source.name.casefold() == "build.bat" and source.parent.name == "onnxruntime-058787ceead760166e3c50a0a4cba8a833a6f53f" and ".." not in source.parts, "Windows ORT command is not from the pinned source tree")
    require(build_directory.is_absolute() and ".." not in build_directory.parts and re.fullmatch(r"[1-9][0-9]?", ort[10]) and 1 <= int(ort[10]) <= 64, "Windows ORT build directory/parallel evidence differs")
    expected_ort = [ort[0], "--config", "Release", "--build_shared_lib", "--enable_msvc_static_runtime", "--cmake_generator", "Visual Studio 17 2022", "--msvc_toolset", selection["toolset_version"], "--parallel", ort[10], "--skip_submodule_sync", "--build_dir", ort[13]]
    require(ort == expected_ort, "Windows ORT command differs from the reviewed static-CRT shared-runtime recipe")
    require(receipt.get("ort_environment") == {"CMAKE_GENERATOR_INSTANCE": selection["installation_path"]}, "Windows ORT generator-instance environment differs")
    cache = receipt.get("cmake_cache")
    require(isinstance(cache, dict) and receipt_path(cache.get("path", "")).is_absolute() and build_directory in receipt_path(cache["path"]).parents and re.fullmatch(r"[0-9a-f]{64}", cache.get("sha256", "")), "Windows CMake cache identity is malformed")
    values = cache.get("values")
    require(isinstance(values, dict) and values.get("CMAKE_GENERATOR") == "Visual Studio 17 2022" and same_receipt_path(values.get("CMAKE_GENERATOR_INSTANCE", ""), selection["installation_path"]) and values.get("CMAKE_GENERATOR_TOOLSET") == "host=x64,version=" + selection["toolset_version"] and same_receipt_path(values.get("CMAKE_C_COMPILER", ""), selection["compiler_path"]) and same_receipt_path(values.get("CMAKE_CXX_COMPILER", ""), selection["compiler_path"]) and values.get("CMAKE_MSVC_RUNTIME_LIBRARY") == "MultiThreaded$<$<CONFIG:Debug>:Debug>" and same_receipt_path(values.get("CMAKE_HOME_DIRECTORY", ""), str(source.parent / "cmake")), "Windows CMake cache does not bind the selected Visual Studio static-runtime build")
    invocation = receipt.get("ort_invocation")
    require(isinstance(invocation, list) and len(invocation) == len(ort) + 3 and [value.casefold() for value in invocation[:3]] == ["cmd.exe", "/d", "/c"] and invocation[3:] == ort, "Windows ORT invocation evidence is malformed")
    cargo = receipt.get("cargo_command")
    require(isinstance(cargo, list) and len(cargo) == 12 and all(isinstance(value, str) for value in cargo), "Windows Cargo command evidence is missing/malformed")
    manifest = receipt_path(cargo[5])
    require(manifest.is_absolute() and manifest.name == "Cargo.toml" and ".." not in manifest.parts and (workspace is None or manifest == receipt_path(str(Path(workspace).resolve()))), "Windows Cargo manifest identity is invalid")
    require(cargo == ["cargo", "build", "--locked", "--release", "--manifest-path", cargo[5], "--target", "x86_64-pc-windows-msvc", "--bin", "ilium", "--bin", "ilium-server"], "Windows Cargo command differs from the locked native release-pair build")
    environment = receipt.get("environment")
    required_paths = ("ORT_LIB_LOCATION", "ORT_LIB_PATH", "CARGO_HOME", "CARGO_TARGET_DIR")
    require(isinstance(environment, dict) and all(isinstance(environment.get(name), str) and receipt_path(environment[name]).is_absolute() and ".." not in receipt_path(environment[name]).parts for name in required_paths), "Windows Cargo boundary lacks absolute owned paths")
    require(environment.get("ORT_PREFER_DYNAMIC_LINK") == "1" and environment.get("CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS") == "-Ctarget-feature=+crt-static" and "CARGO_ENCODED_RUSTFLAGS" not in environment, "Windows Rust/ORT linkage flags differ from the static-CRT shared-runtime contract")
    runtime_path = receipt_path(receipt.get("runtime", {}).get("path", ""))
    import_path = receipt_path(receipt.get("import_library", {}).get("path", ""))
    require(runtime_path.is_absolute() and runtime_path.name.casefold() == "onnxruntime.dll" and import_path.is_absolute() and import_path.name.casefold() == "onnxruntime.lib" and runtime_path.parent == import_path.parent and build_directory in runtime_path.parents and re.fullmatch(r"[0-9a-f]{64}", receipt.get("import_library", {}).get("sha256", "")), "Windows source-built DLL/import-library location is invalid")
    require(same_receipt_path(environment["ORT_LIB_LOCATION"], str(runtime_path.parent)) and same_receipt_path(environment["ORT_LIB_PATH"], str(runtime_path.parent)), "Windows Cargo ORT path does not use the verified source-build output")
    outputs = [build_directory, receipt_path(environment["CARGO_HOME"]), receipt_path(environment["CARGO_TARGET_DIR"])]
    require(all(left != right and left not in right.parents and right not in left.parents for index, left in enumerate(outputs) for right in outputs[index + 1:]), "Windows ORT/Cargo output directories overlap")
    matches = [item for item in runtimes if item["name"].casefold() == "onnxruntime.dll"]
    require(len(matches) == 1 and re.match(r"^1\.24\.2(?:\D|$)", matches[0].get("version", "")) and receipt.get("runtime", {}).get("version") == "1.24.2" and matches[0].get("sha256") == receipt.get("runtime", {}).get("sha256"), "Windows candidate runtime differs from the pinned source build")
    return {"state": "passed", "source_tag": register["tag"], "source_commit": register["commit"], "source_sha256": register["source_sha256"], "built_runtime_sha256": matches[0]["sha256"], "rust_crt": "static", "ort_crt": "static"}


def reject_windows_dynamic_crt(name):
    normalized = Path(name).name.casefold()
    require(not (re.fullmatch(r"(?:msvcp|vcruntime|concrt)[a-z0-9_.-]*\.dll", normalized) or normalized in {"ucrtbase.dll", "ucrtbased.dll"}), f"Windows dynamic CRT dependency is prohibited: {name}")


def system_dependency(operating_system, name):
    if operating_system == "macos":
        return (name.startswith("/System/Library/Frameworks/") or name.startswith("/usr/lib/")) and "onnx" not in name.casefold() and ".." not in name
    if operating_system == "windows":
        # Vendor CRT/UCRT redistributables are intentionally not exempted.
        return name.casefold() in {"kernel32.dll", "user32.dll", "advapi32.dll", "shell32.dll", "ole32.dll", "oleaut32.dll", "ws2_32.dll", "ntdll.dll", "bcrypt.dll", "crypt32.dll", "secur32.dll", "rpcrt4.dll", "gdi32.dll", "comdlg32.dll", "comctl32.dll", "shlwapi.dll", "winmm.dll", "imm32.dll", "version.dll", "setupapi.dll", "cfgmgr32.dll", "propsys.dll", "dwmapi.dll", "powrprof.dll", "iphlpapi.dll", "dnsapi.dll", "msvcrt.dll", "dbghelp.dll", "dxgi.dll"} or bool(re.fullmatch(r"(?:api|ext)-ms-win-[a-z0-9-]+\.dll", name.casefold()))
    return bool(re.fullmatch(r"(?:lib(?:c|m|pthread|dl|rt|resolv|util|ssl|crypto)\.so\.[0-9]+|ld-linux[^/]*\.so\.[0-9]+|lib(?:asound|udev|gcc_s)\.so\.[0-9]+|libstdc\+\+\.so\.6)", name))


def validate_closure(operating_system, graph, inventory, shipped, roots=None):
    normalize = str.casefold if operating_system == "windows" else str
    bundled = {normalize(item["name"]): item["name"] for item in inventory["files"]}
    systems = {normalize(item["name"]): item for item in inventory["system_libraries"]}
    roots = roots or (["ilium.exe", "ilium-server.exe"] if operating_system == "windows" else ["ilium", "ilium-server"])
    require(set(roots) <= set(graph), "dependency graph is missing an executable root")
    require(set(graph) == set(roots) | set(bundled.values()), "dependency graph differs from executable/runtime inventory")
    used, visited, pending = set(), set(), list(roots)
    while pending:
        parent = pending.pop()
        if parent in visited:
            continue
        visited.add(parent)
        for dependency in graph[parent]:
            if operating_system == "windows":
                reject_windows_dynamic_crt(dependency)
            if operating_system == "macos" and dependency.startswith("@executable_path/"):
                name = dependency.removeprefix("@executable_path/")
                safe_member_name(name)
                require("onnxruntime" not in name.casefold() or bool(re.fullmatch(r"libonnxruntime\.[0-9]+\.[0-9]+\.[0-9]+\.dylib", name)), "ONNX Runtime install name must be explicitly versioned")
            else:
                name = dependency
                if operating_system == "macos":
                    require(system_dependency(operating_system, dependency), f"non-relative macOS loader dependency: {dependency}")
            key = normalize(name)
            if key in bundled:
                require(bundled[key] in shipped, f"missing bundled dependency: {name}")
                used.add(bundled[key])
                pending.append(bundled[key])
            else:
                item = systems.get(key)
                require(item is not None and item.get("reviewed") is True and bool(item.get("source")) and system_dependency(operating_system, name), f"undeclared/unreviewed native dependency: {parent} -> {dependency}")
    require(set(bundled.values()) == used, "runtime inventory contains a library unreachable from both executable roots")
    return sorted(used)


def licence_bytes(item):
    require(item.get("reviewed") is True and bool(item.get("license")) and bool(item.get("license_source")), "dependency licence has not been reviewed")
    require(isinstance(item.get("license_file"), str) and Path(item["license_file"]).is_absolute(), "licence byte source must be an explicit absolute file")
    content = Path(item["license_file"]).read_bytes()
    require(digest(content) == item.get("license_sha256") and bool(content.strip()), "licence bytes are missing or differ from reviewed hash")
    content.decode("utf-8")
    return content


def validate_notices(content):
    text = content.decode("utf-8")
    require(bool(text.strip()) and not any(marker in text.casefold() for marker in ("state: blocked", '"state": "blocked"', "audit later", "placeholder", "todo licence", "todo license")), "third-party provenance is blocked or is a template")
    return content


def generate_notices(lock, dependency_inventory, runtime_inventory):
    require(dependency_inventory.get("schema") == 1 and dependency_inventory.get("state") == "reviewed", "dependency licence inventory is blocked/unreviewed")
    lock_content = Path(lock).read_bytes()
    require(digest(lock_content) == dependency_inventory.get("lock_sha256"), "dependency inventory does not bind this Cargo.lock")
    document = tomllib.loads(lock_content.decode("utf-8"))
    expected = {(item["name"], item["version"], item.get("source", "workspace-or-vendored")) for item in document.get("package", [])}
    packages = dependency_inventory.get("packages", [])
    actual = [(item.get("name"), item.get("version"), item.get("source")) for item in packages]
    require(expected and len(actual) == len(set(actual)) and set(actual) == expected, "licence inventory must cover exactly every Cargo.lock package")
    sections = [b"ILIUM THIRD-PARTY NOTICES\n", f"Cargo.lock SHA-256: {digest(lock_content)}\n".encode()]
    for item in sorted(packages, key=lambda package: (package["name"], package["version"], package["source"])) + sorted(runtime_inventory["files"], key=lambda runtime: runtime["name"]):
        content = licence_bytes(item)
        heading = f"\n--- {item['name']} {item['version']} ---\nLicence: {item['license']}\nSource: {item['license_source']}\nLicence SHA-256: {digest(content)}\n\n"
        sections.extend([heading.encode("utf-8"), content, b"\n"])
    return validate_notices(b"".join(sections))


def inspect_graph(target, directory, code, dumpbin):
    graph, evidence = {}, {}
    for name in sorted(code):
        path = directory / name
        if target["os"] == "linux":
            result = run(["readelf", "-d", path])
            header = run(["readelf", "-h", path]).stdout
            expected = "AArch64" if target["arch"] == "aarch64" else "Advanced Micro Devices X86-64"
            require(expected in header, f"ELF architecture differs: {name}")
            validate_linux_rpaths(result.stdout)
        elif target["os"] == "macos":
            result = run(["otool", "-L", path])
            require(run(["lipo", "-archs", path]).stdout.strip() == ("x86_64" if target["arch"] == "x86_64" else "arm64"), f"Mach-O architecture differs: {name}")
            load_commands = run(["otool", "-l", path]).stdout
            install_name, dependencies = parse_macos_load_commands(load_commands)
            require(not name.endswith(".dylib") or install_name == "@executable_path/" + name, f"dylib install-name identity differs: {name}")
            for loader_path in re.findall(r"^\s+path (.+?) \(offset", load_commands, re.MULTILINE):
                require(loader_path in ("@executable_path", "@loader_path"), f"unsafe Mach-O rpath: {loader_path}")
        else:
            require(dumpbin is not None and dumpbin.is_absolute(), "Windows requires explicit absolute --dumpbin path")
            result = run([dumpbin, "/DEPENDENTS", path])
            require("8664 machine (x64)" in run([dumpbin, "/HEADERS", path]).stdout, f"PE architecture differs: {name}")
        if target["os"] != "macos":
            dependencies = parse_dependencies(target["os"], result.stdout)
            require(bool(dependencies), f"no dynamic loader evidence for {name}")
        graph[name] = dependencies
        evidence[name] = {"otool_L": result.stdout, "otool_l": load_commands, "install_name": install_name} if target["os"] == "macos" else result.stdout
    return graph, evidence


def macos_relocations(graph, runtimes):
    changes = []
    onnx = [name for name in runtimes if re.fullmatch(r"libonnxruntime\.[0-9]+\.[0-9]+\.[0-9]+\.dylib", name)]
    for parent, dependencies in sorted(graph.items()):
        for dependency in dependencies:
            if system_dependency("macos", dependency):
                continue
            name = Path(dependency).name
            if name in ("libonnxruntime.dylib", "libonnxruntime.1.dylib") and len(onnx) == 1:
                name = onnx[0]
            require(name in runtimes, f"macOS relocation encountered an undeclared library: {dependency}")
            new_name = "@executable_path/" + name
            if dependency != new_name:
                changes.append((parent, dependency, new_name))
    return changes


def relocate_macos(directory, code, runtimes):
    originals = {name: run(["otool", "-L", directory / name]).stdout for name in sorted(code)}
    load_commands = {name: run(["otool", "-l", directory / name]).stdout for name in sorted(code)}
    graph = {name: parse_macos_load_commands(output)[1] for name, output in load_commands.items()}
    changes = macos_relocations(graph, runtimes)
    for name in sorted(runtimes):
        run(["install_name_tool", "-id", "@executable_path/" + name, directory / name])
    for parent, previous, replacement in changes:
        run(["install_name_tool", "-change", previous, replacement, directory / parent])
    # Any existing build-host rpath is removed, then loader inspection proves
    # all actual references resolve beside the installed executable.
    removed = []
    for name in sorted(code):
        commands = run(["otool", "-l", directory / name]).stdout
        for path in re.findall(r"^\s+path (.+?) \(offset", commands, re.MULTILINE):
            if path not in ("@executable_path", "@loader_path"):
                run(["install_name_tool", "-delete_rpath", path, directory / name])
                removed.append({"file": name, "path": path})
    return {"original_loader_output": originals, "original_load_commands": load_commands, "changes": changes, "removed_rpaths": removed}


def validate_linux_rpaths(output):
    for value in re.findall(r"\((?:RPATH|RUNPATH)\).*?\[([^\]]*)\]", output):
        require(all(path in ("$ORIGIN", "${ORIGIN}") for path in value.split(":")), "ELF loader path can escape the installed candidate")


def validate_process_mapping(observed_client, mappings, client, runtime):
    require(observed_client.strip() == str(client), "embedding process is not the installed client")
    require(re.search(re.escape(str(client)) + r"(?=\s|$)", mappings), "native mapping has no installed client executable")
    require(re.search(re.escape(str(runtime)) + r"(?=\s|$)", mappings), "installed client did not map the shipped runtime")
    # A vmmap row reads "... r-x/r-x SM=COW  /path/lib.dylib": anchor the path at a
    # whitespace boundary so the "/" inside the protection column is not its start.
    onnx_paths = re.findall(r"(?<!\S)(/[^\n]*?libonnxruntime[^\s]*\.dylib)(?=\s|$)", mappings)
    unexpected = sorted({path for path in onnx_paths if path != str(runtime)})
    require(bool(onnx_paths) and not unexpected, "installed client mapped an unshipped ONNX Runtime: " + ", ".join(unexpected[:3]))
    return [str(runtime)]


def validate_embedding_proof(proof, directory, model, loaded_paths):
    require(proof.get("type") == "embedding-proof" and bool(proof.get("input")), "embedding proof has no real input")
    require(proof.get("model_sha256") == digest(Path(model).read_bytes()), "embedding model differs from tested bytes")
    vector = proof.get("embedding")
    require(isinstance(vector, list) and 0 < len(vector) <= 65536 and all(type(value) in (int, float) and math.isfinite(value) for value in vector) and any(value != 0 for value in vector), "embedding inference did not produce a finite nonzero vector")
    runtime = Path(proof.get("loaded_runtime", ""))
    require(runtime.is_absolute() and runtime.is_file() and not runtime.is_symlink() and runtime.parent.resolve() == Path(directory).resolve(), "embedding resolved an unshipped runtime")
    require(re.fullmatch(r"libonnxruntime\.[0-9]+\.[0-9]+\.[0-9]+\.dylib", runtime.name) is not None, "embedding runtime must be versioned")
    require(str(runtime) in loaded_paths, "dyld did not confirm the shipped runtime in the installed Ilium process")
    return {"state": "passed", "dimensions": len(vector), "model_sha256": proof["model_sha256"], "input": proof["input"], "loaded_runtime": str(runtime), "runtime_sha256": digest(runtime.read_bytes()), "vector_sha256": digest(json.dumps(vector, allow_nan=False).encode())}


def embedding_gate(command_file, directory, model):
    require(command_file is not None and model is not None, "macOS requires a reviewed real post-install embedding acceptance command and explicit model")
    specification = read_json(command_file)
    require(specification.get("state") == "reviewed" and specification.get("reviewed_by"), "embedding harness is unreviewed")
    command = specification.get("command")
    require(isinstance(command, list) and command and all(isinstance(item, str) for item in command), "embedding command must be an argument array")
    executable = Path(command[0])
    require(executable.is_absolute() and digest(executable.read_bytes()) == specification.get("sha256"), "embedding harness differs from reviewed bytes")
    require(specification.get("protocol") == "held-installed-process-v1", "embedding harness must hold the installed client until the native mapping acknowledgement")
    environment = dict(os.environ)
    for variable in ("DYLD_LIBRARY_PATH", "DYLD_FALLBACK_LIBRARY_PATH", "LD_LIBRARY_PATH", "LD_PRELOAD"):
        environment.pop(variable, None)
    invocation = command + ["--installed-directory", str(directory), "--model", str(model), "--text", "release embedding acceptance", "--hold-for-native-audit"]
    # A signed hardened process may ignore DYLD_PRINT_LIBRARIES. The harness
    # emits its real inference vector and keeps its own installed child alive;
    # the auditor independently checks that child's executable and native map.
    # The reviewed harness owns child cleanup when stdin closes or is acknowledged.
    with tempfile.TemporaryFile(mode="w+b") as errors:
        process = subprocess.Popen(invocation, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors, env=environment)
        output, pending, proof = b"", b"", None
        deadline = time.monotonic() + 600
        try:
            with selectors.DefaultSelector() as selector:
                selector.register(process.stdout, selectors.EVENT_READ)
                while proof is None:
                    remaining = deadline - time.monotonic()
                    require(remaining > 0, "embedding harness timed out before inference proof")
                    require(selector.select(remaining), "embedding harness timed out before inference proof")
                    chunk = os.read(process.stdout.fileno(), 65536)
                    require(bool(chunk), "embedding harness exited without a held-process inference proof")
                    output += chunk
                    require(len(output) <= 1_000_000, "embedding harness output exceeds bounded receipt size")
                    pending += chunk
                    while b"\n" in pending:
                        line, pending = pending.split(b"\n", 1)
                        record = json.loads(line)
                        if record.get("type") == "embedding-proof":
                            require(proof is None, "embedding harness emitted duplicate inference proof")
                            proof = record
            require(proof.get("input") == "release embedding acceptance", "embedding proof input differs from invoked acceptance text")
            client = directory / "ilium"
            require(proof.get("executable_path") == str(client) and proof.get("binary_sha256") == digest(client.read_bytes()), "embedding harness did not exercise this installed Ilium")
            process_id = proof.get("ilium_pid")
            require(type(process_id) is int and process_id > 0, "embedding proof has no installed client process identity")
            observed_client = run(["/bin/ps", "-p", str(process_id), "-o", "comm="]).stdout
            mappings = run(["/usr/bin/vmmap", "-w", str(process_id)]).stdout
            loaded = validate_process_mapping(observed_client, mappings, client, Path(proof.get("loaded_runtime", "")))
            receipt = validate_embedding_proof(proof, directory, model, loaded)
            remainder, _ = process.communicate(b"native-audit-observed\n", timeout=max(1, deadline - time.monotonic()))
            output += remainder
            require(process.returncode == 0, "embedding acceptance harness failed after native observation")
            records = [json.loads(line) for line in output.splitlines()]
            require(sum(record.get("type") == "embedding-proof" for record in records) == 1, "embedding harness emitted duplicate inference proof")
            errors.seek(0)
            return receipt, {"command": invocation, "harness_sha256": specification["sha256"], "stdout": output.decode("utf-8"), "stderr": errors.read().decode("utf-8"), "observed_process": observed_client, "native_mappings": mappings}
        finally:
            if process.poll() is None:
                if process.stdin:
                    process.stdin.close()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    # Only this task-created harness is stopped. Its reviewed
                    # cleanup contract must release its own child on closed stdin.
                    process.terminate()
                    process.wait(timeout=10)
            if process.stdout:
                process.stdout.close()


def sign_macos(directory, code, identity, notarization_profile, output_parent):
    signatures = {}
    # Flat archive graph: runtime leaves first, then both executable roots.
    order = sorted(code, key=lambda name: (not name.endswith(".dylib"), name))
    require(identity != "-", "ad-hoc signing is not a credential-backed signing identity")
    for name in order:
        if identity:
            run(["codesign", "--force", "--timestamp", "--options", "runtime", "--sign", identity, directory / name])
        else:
            # install_name_tool invalidates the linker's ad-hoc signature. An
            # Apple Silicon executable still needs a valid ad-hoc seal to run;
            # this does not represent a Developer ID distribution signature.
            run(["codesign", "--force", "--sign", "-", directory / name])
    for name in order:
        result = run(["codesign", "--display", "--verbose=4", directory / name], check=False)
        if result.returncode == 0:
            run(["codesign", "--verify", "--strict", "--verbose=2", directory / name])
        elif identity:
            raise ReleaseError(f"signed code has no verifiable signature: {name}")
        signatures[name] = {"exit_code": result.returncode, "details": result.stderr, "distribution_signed": bool(identity), "verified": result.returncode == 0}
    signing = {"state": "verified" if identity else "unsigned", "credentials_present": bool(identity), "identity": identity, "nested_code": signatures}
    notarization = {"state": "disabled", "credentials_present": bool(notarization_profile)}
    if notarization_profile:
        require(bool(identity), "notarization requires a complete signed code graph")
        with tempfile.TemporaryDirectory(prefix=".ilium-notary-", dir=output_parent) as stage:
            archive = Path(stage) / "candidate.zip"
            run(["ditto", "-c", "-k", "--keepParent", directory, archive])
            result = run(["xcrun", "notarytool", "submit", archive, "--keychain-profile", notarization_profile, "--wait", "--output-format", "json"], timeout=1800)
            response = json.loads(result.stdout)
            require(response.get("status") == "Accepted" and response.get("id"), "notarization did not accept exact candidate")
            info = json.loads(run(["xcrun", "notarytool", "info", response["id"], "--keychain-profile", notarization_profile, "--output-format", "json"]).stdout)
            require(info.get("status") == "Accepted", "notarization readback failed")
            notarization = {"state": "verified", "submission": response, "readback": info, "submitted_zip_sha256": digest(archive.read_bytes())}
    return signing, notarization


def windows_version(path):
    # The path travels as an environment value, never interpolated PowerShell.
    environment = dict(os.environ, ILIUM_AUDIT_DLL_PATH=str(path))
    result = run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", "(Get-Item -LiteralPath $env:ILIUM_AUDIT_DLL_PATH).VersionInfo.FileVersion"], environment)
    return result.stdout.strip()


def audit(arguments):
    inputs = [arguments.manifest, arguments.workspace, arguments.lockfile, arguments.runtime_inventory, arguments.dependency_inventory, arguments.embedding_command, arguments.embedding_model, arguments.intel_ort_report, getattr(arguments, "windows_ort_report", None), arguments.dumpbin]
    validate_output_paths(arguments.output, arguments.notices_output, arguments.directory, inputs)
    arguments.can_record_failure = True
    target = selected_target(arguments.manifest, arguments.target)
    version = workspace_version(arguments.workspace, arguments.tag)
    identity = native_identity(target, arguments.runner_identity)
    directory = arguments.directory.resolve()
    require(not arguments.directory.is_symlink() and directory.is_dir(), "installed candidate directory is missing or a link")
    inventory = validate_runtime_inventory(read_json(arguments.runtime_inventory), arguments.target)
    dependencies = read_json(arguments.dependency_inventory)
    expected = {*target["executables"], "VERSION", "THIRD-PARTY.txt", *(item["name"] for item in inventory["files"])}
    actual = {path.name for path in directory.iterdir()}
    # Notices are generated only after complete licence review. They may be absent
    # on the first audit, but every other missing/extra byte blocks the candidate.
    require(actual - {"THIRD-PARTY.txt"} == expected - {"THIRD-PARTY.txt"}, "candidate has missing or undeclared members")
    for path in directory.iterdir():
        require(path.is_file() and not path.is_symlink(), "candidate contains a link/directory/special file")
    require((directory / "VERSION").read_bytes() == (version + "\n").encode(), "installed VERSION differs from workspace/tag")
    for item in inventory["files"]:
        path = directory / item["name"]
        require(digest(path.read_bytes()) == item["sha256"], f"runtime hash differs: {path.name}")
        licence_bytes(item)
        if target["os"] == "windows":
            require(path.suffix.casefold() == ".dll" and windows_version(path) == item["version"], f"runtime DLL version differs: {path.name}")
        elif target["os"] == "macos":
            require(path.suffix == ".dylib" and item["version"] in path.name, "macOS runtime filename is unversioned")
        else:
            require(".so" in path.name, "Linux runtime is not a shared library")
    intel_provenance = None
    windows_provenance = None
    if (target["os"], target["arch"]) == ("macos", "x86_64"):
        require(arguments.intel_ort_report is not None, "Intel candidate requires its pinned native ORT build receipt")
        build_receipt = read_json(arguments.intel_ort_report)
        intel_provenance = validate_intel_build_receipt(build_receipt, inventory["files"], target["runner"], arguments.workspace)
        intel_provenance["build_receipt_sha256"] = digest(arguments.intel_ort_report.read_bytes())
        require(build_receipt.get("workspace_sha256") == digest(arguments.workspace.read_bytes()) and build_receipt.get("lock_sha256") == digest(arguments.lockfile.read_bytes()), "Intel ORT Cargo build used different workspace/lock inputs")
        require(build_receipt.get("binaries") == {name: digest((directory / name).read_bytes()) for name in target["executables"]}, "Intel candidate binaries differ from the source-runtime Cargo build")
    if target["os"] == "windows":
        windows_report = getattr(arguments, "windows_ort_report", None)
        require(windows_report is not None, "Windows candidate requires its pinned static-CRT native ORT build receipt")
        build_receipt = read_json(windows_report)
        windows_provenance = validate_windows_build_receipt(build_receipt, inventory["files"], target["runner"], arguments.workspace)
        windows_provenance["build_receipt_sha256"] = digest(windows_report.read_bytes())
        require(build_receipt.get("workspace_sha256") == digest(arguments.workspace.read_bytes()) and build_receipt.get("lock_sha256") == digest(arguments.lockfile.read_bytes()), "Windows ORT Cargo build used different workspace/lock inputs")
        require(build_receipt.get("binaries") == {name: digest((directory / name).read_bytes()) for name in target["executables"]}, "Windows candidate binaries differ from the source-runtime Cargo build")
    notices = generate_notices(arguments.lockfile, dependencies, inventory)
    require(arguments.notices_output.resolve() == directory / "THIRD-PARTY.txt", "notices output must be the candidate THIRD-PARTY.txt")
    atomic_write(arguments.notices_output, notices)
    code = set(target["executables"]) | {item["name"] for item in inventory["files"]}
    relocation = None
    if target["os"] == "macos":
        relocation = relocate_macos(directory, code, {item["name"] for item in inventory["files"]})
    before_inspection = {name: digest((directory / name).read_bytes()) for name in code}
    graph, loader_evidence = inspect_graph(target, directory, code, arguments.dumpbin)
    require(before_inspection == {name: digest((directory / name).read_bytes()) for name in code}, "candidate code changed during native loader inspection")
    bundled = validate_closure(target["os"], graph, inventory, expected, target["executables"])
    receipt = {"schema": 1, "state": "passed", "publication_allowed": True, "target": arguments.target, "os": target["os"], "arch": target["arch"], "tag": arguments.tag, "version": version, "native_identity": identity,
               "dependency_closure": {"complete": True, "graph": graph, "bundled": bundled}, "loader_evidence": loader_evidence,
               "runtime_inventory_sha256": digest(arguments.runtime_inventory.read_bytes()), "dependency_inventory_sha256": digest(arguments.dependency_inventory.read_bytes()),
               "notices": {"state": "reviewed", "sha256": digest(notices), "lock_sha256": digest(arguments.lockfile.read_bytes())}}
    if target["os"] == "macos":
        receipt["signing"], receipt["notarization"] = sign_macos(directory, code, arguments.signing_identity, arguments.notarization_profile, arguments.output.parent)
        receipt["loader_paths"] = {"state": "passed", "relocation": relocation}
        if intel_provenance:
            receipt["intel_ort"] = intel_provenance
        qualified_hashes = {name: digest((directory / name).read_bytes()) for name in code}
        receipt["embedding"], receipt["embedding_evidence"] = embedding_gate(arguments.embedding_command, directory, arguments.embedding_model)
    else:
        require(not arguments.signing_identity and not arguments.notarization_profile, "macOS signing flags used on another OS")
        qualified_hashes = before_inspection
        if windows_provenance:
            receipt["windows_ort"] = windows_provenance
    versions = {}
    for executable in target["executables"]:
        environment = dict(os.environ)
        for variable in ("LD_LIBRARY_PATH", "LD_PRELOAD", "DYLD_LIBRARY_PATH", "DYLD_FALLBACK_LIBRARY_PATH"):
            environment.pop(variable, None)
        result = run([directory / executable, "--version"], environment)
        require(result.stdout.strip() == f"{executable.removesuffix('.exe')} {version}" and not result.stderr, f"native version output differs: {executable}")
        versions[executable] = result.stdout.strip()
    receipt["binary_versions"] = versions
    require(qualified_hashes == {name: digest((directory / name).read_bytes()) for name in code}, "candidate code changed during native runtime qualification")
    receipt["files"] = {name: digest((directory / name).read_bytes()) for name in sorted(expected)}
    atomic_write(arguments.output, (json.dumps(receipt, indent=2, sort_keys=True, allow_nan=False) + "\n").encode())
    emit({"type": "artifact", "path": str(arguments.output.resolve()), "sha256": digest(arguments.output.read_bytes())})
    emit({"type": "artifact", "path": str(arguments.notices_output.resolve()), "sha256": digest(notices)})
    emit({"type": "result", "command": "audit-native", "state": "passed", "publication_allowed": True, "target": arguments.target})


def parser():
    result = JsonArgumentParser(description=__doc__, allow_abbrev=False)
    for flag in ("manifest", "workspace", "lockfile", "directory", "runtime-inventory", "dependency-inventory", "output", "notices-output"):
        result.add_argument("--" + flag, type=Path, required=True)
    for flag in ("target", "tag", "runner-identity"):
        result.add_argument("--" + flag, required=True)
    for flag in ("dumpbin", "embedding-command", "embedding-model", "intel-ort-report", "windows-ort-report"):
        result.add_argument("--" + flag, type=Path)
    result.add_argument("--signing-identity")
    result.add_argument("--notarization-profile")
    return result


def main(argv=None):
    arguments = None
    try:
        arguments = parser().parse_args(argv)
        audit(arguments)
        return 0
    except (ValueError, OSError, UnicodeError, subprocess.SubprocessError, KeyError, TypeError, AttributeError) as error:
        blocked = {"schema": 1, "type": "error", "command": "audit-native", "state": "blocked", "publication_allowed": False, "error": str(error)}
        if arguments:
            blocked["target"] = arguments.target
        if arguments and getattr(arguments, "can_record_failure", False):
            try:
                atomic_write(arguments.output, (json.dumps(blocked, indent=2) + "\n").encode())
            except (OSError, ReleaseError) as recording_error:
                blocked["recording_error"] = str(recording_error)
        emit(blocked)
        return 2


if __name__ == "__main__":
    sys.exit(main())
