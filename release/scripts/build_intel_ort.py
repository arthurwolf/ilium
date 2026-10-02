"""Build pinned Intel ONNX Runtime on native macOS, then the release pair.

The reviewed source register is an explicit input, not a hash invented by this
helper. It must contain schema=1,state=reviewed,version,tag,commit,source_url,
source_sha256,reviewed_by. A source archive supplied locally is verified before
use; --download-source explicitly acquires that exact URL on the native host.
An existing output root/Cargo cache is never cleaned or reused.
"""

import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import time
import urllib.error
import urllib.request

from release_tool import JsonArgumentParser, ReleaseError, digest, emit, read_json
from audit_native import atomic_write

VERSION = "1.24.2"
TAG = "v1.24.2"
COMMIT = "058787ceead760166e3c50a0a4cba8a833a6f53f"
SOURCE_URL = f"https://codeload.github.com/microsoft/onnxruntime/tar.gz/{COMMIT}"
TAG_URL = f"https://api.github.com/repos/microsoft/onnxruntime/git/ref/tags/{TAG}"


def require(condition, message):
    if not condition:
        raise ReleaseError(message)


def require_intel_host(system, machine):
    require(system == "Darwin" and machine == "x86_64", "Intel ORT build requires native macOS x86_64; no download/build is allowed on another host")


def validate_source_register(register):
    require(register.get("schema") == 1 and register.get("state") == "reviewed" and bool(register.get("reviewed_by")), "ORT source register must be reviewed")
    for field, value in (("version", VERSION), ("tag", TAG), ("commit", COMMIT), ("source_url", SOURCE_URL)):
        require(register.get(field) == value, f"ORT source {field} differs from pinned identity")
    require(isinstance(register.get("source_sha256"), str) and re.fullmatch(r"[0-9a-f]{64}", register["source_sha256"]), "ORT source archive has no reviewed SHA-256")
    return register


def verify_source_hash(archive, expected):
    require(not Path(archive).is_symlink(), "ORT source archive must not be a link")
    require(digest(Path(archive).read_bytes()) == expected, "ORT source archive differs from reviewed SHA-256")


def request_bytes(url):
    headers = {"User-Agent": "ilium-native-release-provenance/1", "Accept": "application/vnd.github+json"}
    token = os.environ.get("GITHUB_TOKEN")
    if token and url.startswith("https://api.github.com/"):
        # Shared runner addresses exhaust the anonymous API quota; the workflow token raises it.
        headers["Authorization"] = "Bearer " + token
    delay = 20
    for attempt in range(6):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=120) as response:
                require(response.url.startswith(("https://api.github.com/", "https://codeload.github.com/")), "source acquisition redirected outside pinned official hosts")
                return response.read()
        except urllib.error.HTTPError as error:
            if error.code not in (403, 429, 500, 502, 503, 504) or attempt == 5:
                raise
            time.sleep(delay)
            delay *= 2


def verify_tag_commit(reference, tag_object=None):
    value = reference.get("object", {})
    if value.get("type") == "tag":
        require(tag_object is not None and tag_object.get("sha") == value.get("sha"), "annotated tag object is missing or mismatched")
        value = tag_object.get("object", {})
    require(value.get("type") == "commit" and value.get("sha") == COMMIT, "upstream tag does not resolve to the pinned commit")
    return value["sha"]


def upstream_tag():
    reference = json.loads(request_bytes(TAG_URL))
    tag_object = None
    if reference.get("object", {}).get("type") == "tag":
        object_hash = reference["object"].get("sha", "")
        require(re.fullmatch(r"[0-9a-f]{40}", object_hash), "upstream tag object identity is invalid")
        tag_object = json.loads(request_bytes(f"https://api.github.com/repos/microsoft/onnxruntime/git/tags/{object_hash}"))
    verify_tag_commit(reference, tag_object)
    return {"reference_url": TAG_URL, "reference": reference, "tag_object": tag_object}


def unpack_source(archive_path, destination):
    destination.mkdir(mode=0o700)
    seen, root = set(), None
    with tarfile.open(archive_path, "r:gz") as archive:
        for member in archive:
            path = PurePosixPath(member.name)
            require(not path.is_absolute() and ".." not in path.parts and "\\" not in member.name and path.parts and member.name not in seen, "ORT source archive has unsafe/duplicate members")
            seen.add(member.name)
            require(member.isdir() or member.isreg(), "ORT source archive links/special files are prohibited")
            root = root or path.parts[0]
            require(path.parts[0] == root, "ORT source archive must have one root")
            target = destination.joinpath(*path.parts)
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                source = archive.extractfile(member)
                require(source is not None, "unreadable ORT source member")
                with target.open("xb") as output:
                    shutil.copyfileobj(source, output)
                target.chmod(0o755 if member.mode & 0o111 else 0o644)
    require(root is not None and root == f"onnxruntime-{COMMIT}", "ORT source archive root differs from pinned commit")
    source = destination / root
    require((source / "build.sh").is_file(), "ORT source archive has no upstream build entrypoint")
    return source


def command_identity(command):
    result = subprocess.run(command, capture_output=True, text=True, check=False, timeout=120)
    require(result.returncode == 0, f"toolchain identity failed: {command[0]}")
    return {"command": command, "stdout": result.stdout, "stderr": result.stderr}


def logged_command(command, cwd, environment, log_path):
    emit({"type": "progress", "command": "build-intel-ort", "operation": "native-build", "argv": list(map(str, command)), "cwd": str(cwd), "log_path": str(log_path)})
    with log_path.open("x", encoding="utf-8") as log:
        process = subprocess.Popen(list(map(str, command)), cwd=cwd, env=environment, stdout=log, stderr=subprocess.STDOUT)
        emit({"type": "progress", "operation": "process-started", "process_id": process.pid, "log_path": str(log_path)})
        exit_code = process.wait()
    emit({"type": "artifact", "path": str(log_path), "sha256": digest(log_path.read_bytes())})
    require(exit_code == 0, f"native build failed exit={exit_code}; complete log: {log_path}")


def build_command(source, output, parallel):
    # Upstream performs git submodule sync by default. A reviewed source archive
    # has no .git metadata, so use the supported skip flag; CMake retains its
    # own pinned dependency acquisition. No implicit Git mutation is authorized.
    return [str(source / "build.sh"), "--config", "Release", "--build_shared_lib", "--parallel", str(parallel), "--use_xcode", "--skip_submodule_sync", "--compile_no_warning_as_error", "--build_dir", str(output), "--cmake_extra_defines", "CMAKE_OSX_ARCHITECTURES=x86_64"]


def cargo_environment(runtime_directory, cargo_home, cargo_target):
    # ort-sys defaults to static linkage even for a user-provided location.
    # This source lane deliberately builds a shared runtime, so request dynamic
    # linkage explicitly and prove its installed loading in audit_native.py.
    return {"ORT_LIB_LOCATION": str(runtime_directory), "ORT_LIB_PATH": str(runtime_directory), "ORT_PREFER_DYNAMIC_LINK": "1", "CARGO_HOME": str(cargo_home), "CARGO_TARGET_DIR": str(cargo_target)}


def build(arguments):
    # Host gate precedes network, output-directory creation and source acquisition.
    require_intel_host(platform.system(), platform.machine())
    register = validate_source_register(read_json(arguments.source_register))
    for path in (arguments.output_root, arguments.cargo_target_dir, arguments.cargo_home):
        require(path.is_absolute() and not path.exists() and not path.is_symlink(), f"build path must be explicit, new and disjoint: {path}")
    paths = [path.resolve() for path in (arguments.output_root, arguments.cargo_target_dir, arguments.cargo_home)]
    require(all(left != right and left not in right.parents and right not in left.parents for index, left in enumerate(paths) for right in paths[index + 1:]), "ORT/Cargo output paths overlap")
    require(arguments.cargo_workspace.is_absolute() and arguments.cargo_workspace.is_file(), "Cargo workspace manifest must be explicit and absolute")
    for output in (arguments.output, arguments.cargo_environment_output):
        require(output.is_absolute() and output.parent.resolve() == arguments.output_root.resolve(), "ORT receipt outputs must be distinct files directly in the owned output root")
    require(arguments.output != arguments.cargo_environment_output, "ORT receipt and Cargo environment outputs must differ")
    require(arguments.source_archive.is_absolute(), "source archive path must be absolute")
    if arguments.download_source:
        require(not arguments.source_archive.exists(), "download refuses to replace an existing source archive")
        source_bytes = request_bytes(register["source_url"])
        require(digest(source_bytes) == register["source_sha256"], "downloaded ORT source archive differs from reviewed hash")
        with arguments.source_archive.open("xb") as source:
            source.write(source_bytes)
    verify_source_hash(arguments.source_archive, register["source_sha256"])
    tag_evidence = upstream_tag()
    # Lower the priority of this process and all subsequently inherited workers.
    os.nice(15)
    arguments.output_root.mkdir(mode=0o700)
    arguments.cargo_target_dir.mkdir(mode=0o700)
    arguments.cargo_home.mkdir(mode=0o700)
    toolchain = {"clang": command_identity(["clang", "--version"]), "xcode": command_identity(["xcodebuild", "-version"]), "developer_directory": command_identity(["xcode-select", "-p"]), "cmake": command_identity(["cmake", "--version"]), "rustc": command_identity(["rustc", "-Vv"]), "cargo": command_identity(["cargo", "--version"]), "python": sys.version}
    source = unpack_source(arguments.source_archive, arguments.output_root / "source")
    native_build = arguments.output_root / "build"
    command = build_command(source, native_build, arguments.parallel)
    environment = dict(os.environ)
    for variable in ("ORT_LIB_LOCATION", "ORT_LIB_PATH", "DYLD_LIBRARY_PATH", "DYLD_FALLBACK_LIBRARY_PATH"):
        environment.pop(variable, None)
    logged_command(command, source, environment, arguments.output_root / "ort-build.log")
    libraries = [path for path in native_build.rglob(f"libonnxruntime.{VERSION}.dylib") if path.is_file() and not path.is_symlink()]
    require(len(libraries) == 1, "build did not produce exactly one regular versioned ONNX Runtime dylib")
    runtime = libraries[0].resolve()
    require(command_identity(["lipo", "-archs", str(runtime)])["stdout"].strip() == "x86_64", "built ONNX Runtime is not Intel-only")
    cargo_values = cargo_environment(runtime.parent, arguments.cargo_home, arguments.cargo_target_dir)
    environment.update(cargo_values)
    cargo_command = ["cargo", "build", "--locked", "--release", "--manifest-path", arguments.cargo_workspace, "--target", "x86_64-apple-darwin", "--bin", "ilium", "--bin", "ilium-server"]
    logged_command(cargo_command, arguments.cargo_workspace.parent, environment, arguments.output_root / "cargo-build.log")
    binaries = {name: digest((arguments.cargo_target_dir / "x86_64-apple-darwin/release" / name).read_bytes()) for name in ("ilium", "ilium-server")}
    receipt = {"schema": 1, "state": "built-not-qualified", "publication_allowed": False, "source_register": register, "source_register_sha256": digest(arguments.source_register.read_bytes()), "tag_resolution": tag_evidence, "toolchain": toolchain, "runner_identity": arguments.runner_identity, "native_identity": {"system": platform.system(), "machine": platform.machine(), "release": platform.release(), "version": platform.version()}, "ort_command": list(map(str, command)), "cargo_command": list(map(str, cargo_command)), "environment": cargo_values, "runtime": {"path": str(runtime), "sha256": digest(runtime.read_bytes()), "version": VERSION}, "remaining_gate": "native audit and real post-install embedding inference using the shipped runtime"}
    receipt.update(binaries=binaries, workspace_sha256=digest(arguments.cargo_workspace.read_bytes()), lock_sha256=digest((arguments.cargo_workspace.parent / "Cargo.lock").read_bytes()))
    atomic_write(arguments.output, (json.dumps(receipt, indent=2, sort_keys=True) + "\n").encode())
    atomic_write(arguments.cargo_environment_output, (json.dumps(receipt["environment"], indent=2) + "\n").encode())
    emit({"type": "artifact", "path": str(arguments.output.resolve()), "sha256": digest(arguments.output.read_bytes())})
    emit({"type": "artifact", "path": str(arguments.cargo_environment_output.resolve())})
    emit({"type": "result", "command": "build-intel-ort", "state": "built-not-qualified", "publication_allowed": False, "runtime": str(runtime)})


def parser():
    result = JsonArgumentParser(description=__doc__, allow_abbrev=False)
    for flag in ("source-register", "source-archive", "output-root", "output", "cargo-environment-output", "cargo-workspace", "cargo-target-dir", "cargo-home"):
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
        require(bool(arguments.runner_identity.strip()), "runner identity is required")
        build(arguments)
        return 0
    except (ValueError, OSError, UnicodeError, subprocess.SubprocessError, tarfile.TarError, KeyError, TypeError, AttributeError) as error:
        record = {"type": "error", "command": "build-intel-ort", "state": "blocked", "publication_allowed": False, "error": str(error)}
        emit(record)
        return 2


if __name__ == "__main__":
    sys.exit(main())
