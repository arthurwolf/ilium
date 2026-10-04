#!/usr/bin/env python3
"""Repackage audited macOS tarballs; build success does not qualify publication."""  # Preserve the audit boundary.
from __future__ import annotations  # Permit forward annotations on Python 3.11.
import argparse  # Type the parsed command arguments.
import ctypes  # Query Rosetta in this Python process.
import errno  # Distinguish an absent translation sysctl from query failure.
import hashlib  # Bind every input and output to exact bytes.
import json  # Emit and retain structured evidence.
import os  # Create exclusive files and synchronize output.
from pathlib import Path, PurePosixPath  # Separate filesystem and archive paths.
import platform  # Check native operating system and architecture.
import plistlib  # Read hdiutil's documented machine-readable output.
import re  # Validate names, versions, digests, and load commands.
import signal  # Stop only a command's newly created process group.
import stat  # Reject links and special files without following them.
import struct  # Inspect thin Mach-O headers without executing their bytes.
import subprocess  # Run explicit native command argument arrays.
import sys  # Preserve the existing JSONL command-line convention.
import tarfile  # Classify strict archive-parser failures.
import tempfile  # Allocate exclusive staging beside the output directory.
import time  # Enforce finite native command budgets.
from typing import Any  # Describe validated JSON objects at module boundaries.
import xml.etree.ElementTree as xml_tree  # Escape all generated Distribution attributes.
from xml.parsers.expat import ExpatError as expat_error  # Normalize malformed native plist XML.
import zipfile  # Classify strict ZIP-parser failures.
import audit_native  # Reuse the supplied read-only Mach-O/closure validators.
import release_tool  # Reuse the authoritative manifest, archive, and audit policy.
#
root = Path(__file__).resolve().parents[2]  # Match the existing release script layout.
formats = ('zip', 'pkg', 'dmg')  # Every build produces all three required formats.
architectures = {'x86_64': ('x86_64', 0x01000007), 'aarch64': ('arm64', 0x0100000C)}  # Map project names to Mach-O.
max_file_bytes = 1_073_741_824  # Bound any individual payload or evidence file.
max_payload_bytes = 2_000_000_000  # Preserve the authoritative archive size bound.
max_json_bytes = 33_554_432  # Bound native metadata and the embedded build receipt.
max_command_bytes = 8_388_608  # Bound the combined output of each native command.
max_commands = 1024  # Bound the complete command evidence inventory.
source_names = ('Cargo.toml', 'Cargo.lock', 'release/targets.toml', 'release/embedding-model.json', 'release/ort-source.json', 'release/ort-runtime.json', 'release/licence-sources.json', 'release/tests/embedding_acceptance.py', 'release/scripts/release_tool.py', 'release/scripts/audit_native.py', 'release/scripts/build_macos_packages.py', 'ilium-animation-js/src/release.rs', 'ilium-animation-js/src/bin/ilium-animation-helper.rs', 'ilium-client/src/animation_plugins.rs', 'ilium-client/src/background_animation/plugin_backend.rs', 'ilium-platform/src/animation_sandbox.rs', 'ilium-platform/src/lib.rs', 'ilium-animation-js/assets/packages/beach-1.0.0.iliumanim', 'ilium-animation-js/assets/packages/carpet-1.0.0.iliumanim')  # Bind compiled identities and the exact official package bytes.
native_names = ('native-audit.json', 'runtime-inventory.json', 'dependency-inventory.json', 'embedding-command.json', 'embedding-receipt.json', 'SHA256SUMS', 'native-candidate-receipt.json', 'native-test-binary', 'native-test-harness.json')  # Match the supplied native artifact contract.
#
def require(condition: object, message: str) -> None:  # Keep policy failures compatible with release_tool.
    if not condition:  # Stop at the first violated boundary.
        raise release_tool.ReleaseError(message)  # Let main emit one structured failure.
#
def emit(kind: str, **values: Any) -> None:  # Keep stdout exclusively JSONL.
    release_tool.emit({'type': kind, **values})  # Use the existing serializer.
#
def regular_file(path: Path, limit: int = max_payload_bytes, *, allow_hardlinks: bool = False) -> Path:  # Reject artifact aliases while admitting system tool stubs.
    information = path.lstat()  # Inspect the entry itself.
    require(stat.S_ISREG(information.st_mode) and (allow_hardlinks or information.st_nlink == 1), 'not an admitted regular file: ' + str(path))  # Reject symlinks/devices and hardlinked artifacts.
    require(0 <= information.st_size <= limit, 'file exceeds size bound: ' + str(path))  # Bound every file read.
    return path  # Preserve the caller's explicit pathname.
#
def sha(path: Path, *, allow_hardlinks: bool = False) -> str:  # Hash bounded regular files incrementally.
    regular_file(path, allow_hardlinks=allow_hardlinks)  # Restrict the exception to explicit system-tool hashing.
    value = hashlib.sha256()  # Start an independent digest.
    total = 0  # Bound a file that grows after initial inspection.
    with path.open('rb') as stream:  # Open only the validated regular entry.
        while block := stream.read(1_048_576):  # Bound each allocation.
            total += len(block)  # Count the bytes actually observed.
            require(total <= max_payload_bytes, 'file grew beyond hash size bound')  # Stop without reading an unbounded changing input.
            value.update(block)  # Cover every byte in order.
    return value.hexdigest()  # Return the standard lowercase digest.
#
def read_json(path: Path) -> dict[str, Any]:  # Add a size bound to the supplied duplicate-key parser.
    regular_file(path, max_json_bytes)  # Reject oversized metadata before decoding.
    return release_tool.read_json(path)  # Preserve duplicate-key rejection.
#
def write_bytes(path: Path, data: bytes, mode: int = 0o644) -> None:  # Never overwrite an existing entry.
    with path.open('xb') as stream:  # Require exclusive ownership of the new file.
        stream.write(data)  # Retain the exact supplied bytes.
        stream.flush()  # Flush Python buffering before fsync.
        os.fsync(stream.fileno())  # Synchronize the completed file.
    path.chmod(mode)  # Normalize permissions without changing content.
    os.utime(path, (0, 0))  # Normalize staging timestamps.
#
def write_json(path: Path, value: dict[str, Any]) -> None:  # Write a finite, strict JSON receipt.
    data = (json.dumps(value, sort_keys=True, indent=2, ensure_ascii=True, allow_nan=False) + '\n').encode('ascii')  # Retain LF and reject nonfinite numbers.
    require(len(data) <= max_json_bytes, 'receipt exceeds size bound')  # Avoid an unbounded evidence artifact.
    write_bytes(path, data)  # Use exclusive synchronized output.
#
def package_name(architecture: str, package_format: str) -> str:  # Supply stable names to future aggregation.
    require(architecture in architectures and package_format in formats, 'unsupported macOS package name')  # Reject unknown architecture/format pairs.
    return 'ilium-macos-' + architecture + '.' + package_format  # Preserve the requested public asset spelling.
#
def package_names(architecture: str) -> tuple[str, ...]:  # Enumerate the complete mandatory format set.
    return tuple(package_name(architecture, item) for item in formats)  # Keep deterministic format order.
#
def receipt_name(architecture: str) -> str:  # Name the immutable build receipt separately from smoke evidence.
    require(architecture in architectures, 'unsupported macOS receipt architecture')  # Reject unknown targets.
    return 'macos-packages-' + architecture + '.json'  # Let both native outputs merge without collisions.
#
def smoke_receipt_name(architecture: str) -> str:  # Reserve the B2 lifecycle artifact's stable name.
    require(architecture in architectures, 'unsupported macOS smoke architecture')  # Reject unknown targets.
    return 'macos-smoke-' + architecture + '.json'  # Avoid changing a hashed build receipt after testing.
#
def version_from_tag(tag: str) -> str:  # Make the PKG numeric version policy explicit.
    require(isinstance(tag, str) and re.fullmatch(r'v(?:0|[1-9][0-9]{0,4})\.(?:0|[1-9][0-9]{0,4})\.(?:0|[1-9][0-9]{0,4})', tag), 'macOS package versions require plain vMAJOR.MINOR.PATCH; prereleases are not rewritten')  # Fail unsupported native ordering safely.
    return tag[1:]  # Preserve every accepted version digit.
#
def relative_name(name: str) -> str:  # Validate recursive evidence paths without normalization aliases.
    require(isinstance(name, str) and re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]*(?:/[A-Za-z0-9][A-Za-z0-9._-]*)*', name) and '..' not in name, 'unsafe relative inventory path')  # Reject traversal, hidden names, and separators from other platforms.
    return name  # Use exactly the validated spelling.
#
def tree_inventory(directory: Path) -> dict[str, dict[str, Any]]:  # Observe the whole tree, including directories.
    require(directory.is_dir() and not directory.is_symlink(), 'inventory root must be a regular directory')  # Refuse a redirected root.
    pending, entries, folded, total = [directory], {}, set(), 0  # Keep traversal and resource accounting bounded.
    while pending:  # Visit every directory without following links.
        current = pending.pop()  # Work within the explicitly selected tree.
        for path in sorted(current.iterdir()):  # Produce deterministic discovery order.
            name = relative_name(path.relative_to(directory).as_posix())  # Retain exact recursive names.
            require(name.casefold() not in folded and len(entries) < 4096, 'case collision or excessive inventory entries')  # Match case-insensitive macOS filesystems safely.
            folded.add(name.casefold())  # Reserve this spelling before descent.
            information = path.lstat()  # Inspect entry type without following it.
            mode = stat.S_IMODE(information.st_mode)  # Record executable and data permission bits.
            require(not mode & 0o7000, 'special permission bits are prohibited')  # Reject privilege-bearing metadata.
            if stat.S_ISDIR(information.st_mode):  # Retain directories as exact inventory entries.
                entries[name] = {'kind': 'directory', 'mode': mode}  # Include empty directories in comparisons.
                pending.append(path)  # Traverse only real directories.
                continue  # Keep file handling separate.
            regular_file(path, max_file_bytes)  # Reject all links and special objects.
            total += information.st_size  # Count expanded file bytes.
            require(total <= 4 * max_payload_bytes, 'recursive inventory exceeds byte bound')  # Bound retained native evidence too.
            entries[name] = {'kind': 'file', 'mode': mode, 'bytes': information.st_size, 'sha256': sha(path)}  # Bind content and type.
    return dict(sorted(entries.items()))  # Return canonical path order.
#
def verify_tree_hashes(directory: Path, expected: dict[str, str]) -> dict[str, dict[str, Any]]:  # Reject unknown directories as well as files.
    require(isinstance(expected, dict) and expected, 'missing expected file inventory')  # Never certify an empty payload.
    parents: set[str] = set()  # Derive the only permitted directory entries.
    for name, digest in expected.items():  # Validate every independently supplied expectation.
        relative_name(name)  # Reject unsafe paths before filesystem comparison.
        require(isinstance(digest, str) and re.fullmatch(r'[0-9a-f]{64}', digest), 'invalid file digest')  # Require actual SHA-256 spelling.
        parents.update(parent.as_posix() for parent in PurePosixPath(name).parents if parent.as_posix() != '.')  # Admit only necessary parent directories.
    observed = tree_inventory(directory)  # Recursively inspect actual entries.
    require(set(observed) == set(expected) | parents, 'recursive file/directory inventory differs')  # Fail closed on extras and omissions.
    require(all(observed[name]['kind'] == 'directory' for name in parents), 'expected parent is not a directory')  # Reject file/directory substitutions.
    require(all(observed[name].get('sha256') == digest for name, digest in expected.items()), 'recursive file hashes differ')  # Bind the exact bytes.
    return observed  # Allow callers to retain modes and sizes as well.
#
def macho_header(content: bytes, architecture: str, executable: bool) -> None:  # Provide an offline architecture and malformed-header gate.
    require(architecture in architectures and len(content) >= 32, 'missing thin Mach-O header')  # Reject truncated inputs early.
    magic, cpu, _subtype, kind, count, command_bytes, _flags, reserved = struct.unpack_from('<IiiIIIII', content)  # Read the complete 64-bit header.
    require(magic == 0xFEEDFACF and cpu == architectures[architecture][1], 'wrong or universal Mach-O architecture')  # Admit only the selected thin CPU.
    require(kind == (2 if executable else 6) and reserved == 0, 'unexpected Mach-O file kind or reserved field')  # Distinguish executables from dylibs.
    require(0 < count <= 65536 and count * 8 <= command_bytes <= min(len(content) - 32, 4_194_304), 'malformed Mach-O load-command region')  # Bound native parser input complexity.
    position = 32  # Start at the first load command.
    for _index in range(count):  # Validate each load-command extent.
        require(position + 8 <= 32 + command_bytes, 'truncated Mach-O load command')  # Keep reads inside the declared region.
        _command, size = struct.unpack_from('<II', content, position)  # Read command identity and byte extent.
        require(size >= 8 and size % 8 == 0 and position + size <= 32 + command_bytes, 'invalid Mach-O command length')  # Reject overlaps and escapes.
        position += size  # Advance through the actual command inventory.
    require(position == 32 + command_bytes, 'Mach-O load-command count/size mismatch')  # Reject hidden command-region bytes.
#
def extract_package(archive: Path, target: dict[str, Any], audit: dict[str, Any], version: str, destination: Path) -> dict[str, bytes]:  # Materialize only authoritative parsed bytes.
    regular_file(archive)  # Reject filesystem aliases before archive parsing.
    require(archive.name == target['archive'] and not os.path.lexists(destination), 'archive name differs or payload destination already exists')  # Reserve a fresh output boundary.
    names = audit['files']  # Use the final, post-relocation hash inventory.
    require(len({name.casefold() for name in names}) == len(names), 'audited filenames collide on macOS')  # Reject case aliases before any write.
    content = release_tool.read_archive(archive, target, audit)  # Preserve every original tar header, metadata, and trailer guard.
    release_tool.verify_content(content, audit, version)  # Require exact inventory, hashes, VERSION, and notices.
    for name, data in content.items():  # Check every bounded materialized file first.
        require(0 < len(data) <= max_file_bytes, 'invalid payload member size: ' + name)  # Reject empty or oversized payloads.
        if name not in ('VERSION', 'THIRD-PARTY.txt', *target['packages']):  # Only admitted executables and libraries are native code.
            macho_header(data, target['arch'], name in target['executables'])  # Check all executable roots and every runtime.
    destination.mkdir(mode=0o755)  # Create the new flat payload directory.
    destination.chmod(0o755)  # Override a restrictive ambient umask explicitly.
    for name, data in sorted(content.items()):  # Never use tar-selected filesystem extraction paths.
        write_bytes(destination / name, data, 0o755 if name in target['executables'] else 0o644)  # Preserve audit bytes with canonical modes.
    verify_tree_hashes(destination, names)  # Confirm no case folding or filesystem write changed the result.
    return content  # Supply identical bytes to the deterministic ZIP writer.
#
class command_runner:  # Own every launched command, log, and deadline.
    def __init__(self, work: Path) -> None:  # Construct a runner only inside the new work directory.
        self.work = work  # Retain the explicit ownership boundary.
        self.logs = work / 'logs'  # Keep raw command evidence out of product archives.
        self.logs.mkdir(mode=0o700)  # Refuse to reuse another invocation's logs.
        self.temporary = work / 'temporary'  # Constrain tool temporary image paths to owned storage.
        self.temporary.mkdir(mode=0o700)  # Create the only supplied TMPDIR.
        self.records: list[dict[str, Any]] = []  # Retain complete bounded command output in the receipt.
        self.deadline = time.monotonic() + 1800  # Bound the complete native build command sequence.
        self.environment = {'PATH': '/usr/bin:/bin:/usr/sbin:/sbin', 'HOME': str(work), 'TMPDIR': str(self.temporary) + '/', 'LC_ALL': 'C', 'LANG': 'C', 'TZ': 'UTC', 'COPYFILE_DISABLE': '1'}  # Exclude ambient loader overrides and credentials.
    #
    def run(self, label: str, command: list[str | Path], timeout: int = 120, cleanup: bool = False) -> bytes:  # Run without shell interpolation or unbounded waits.
        require(re.fullmatch(r'[a-z0-9-]+', label) and len(self.records) < max_commands, 'invalid command label or command budget exceeded')  # Keep log names and evidence finite.
        arguments = [str(part) for part in command]  # Preserve each argument as a separate value.
        require(arguments and Path(arguments[0]).is_absolute(), 'native tool path must be absolute')  # Avoid PATH-selected tool replacement.
        budget = min(timeout, 120 if cleanup else max(0, self.deadline - time.monotonic()))  # Give cleanup its own finite allowance.
        require(budget > 0, 'native build command deadline exceeded')  # Fail before spawning after exhaustion.
        stem = self.logs / ('%04d-' % len(self.records) + label)  # Allocate a unique retained log pair.
        stdout_path, stderr_path = stem.with_suffix('.stdout'), stem.with_suffix('.stderr')  # Keep both channels independently inspectable.
        failure = ''  # Record timeout or output-limit failure independently of exit status.
        emit('progress', operation=label, command=arguments, timeout_seconds=budget, log=str(stem))  # Expose explicit operation and evidence paths.
        with stdout_path.open('xb') as stdout, stderr_path.open('xb') as stderr:  # Prevent accidental log replacement.
            process = subprocess.Popen(arguments, cwd=self.work, env=self.environment, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr, start_new_session=True)  # Own a fresh process group.
            end = time.monotonic() + budget  # Bound this specific invocation.
            try:  # Ensure an interrupted command cannot run unattended.
                while process.poll() is None:  # Observe the owned process only.
                    if time.monotonic() >= end or stdout_path.stat().st_size + stderr_path.stat().st_size > max_command_bytes:  # Check time and output while running.
                        failure = 'native command exceeded time/output bound'  # Preserve why this operation failed.
                        break  # Enter owned-process cleanup immediately.
                    try:  # Use short bounded waits while checking log growth.
                        process.wait(timeout=min(0.25, max(0.001, end - time.monotonic())))  # Avoid a polling busy loop.
                    except subprocess.TimeoutExpired:  # Keep monitoring a still-running command.
                        continue  # Recheck both finite bounds.
            finally:  # Release only children belonging to this invocation.
                if process.poll() is None:  # Never signal an unrelated completed PID.
                    try:  # Permit a child that exits between observation and signaling.
                        os.killpg(process.pid, signal.SIGKILL)  # Kill only the fresh owned command group.
                    except ProcessLookupError:  # The owned process group has already ended.
                        pass  # Reap the original child without signaling another process.
                    process.wait(timeout=10)  # Bound reaping as well.
        require(stdout_path.stat().st_size + stderr_path.stat().st_size <= max_command_bytes, 'native output exceeded bound; raw logs retained at ' + str(stem))  # Reject oversized evidence without reading it into memory.
        stdout_data, stderr_data = stdout_path.read_bytes(), stderr_path.read_bytes()  # Read the bounded completed logs.
        record = {'label': label, 'command': arguments, 'exit_code': process.returncode, 'timeout_seconds': budget, 'stdout': stdout_data.decode('utf-8'), 'stderr': stderr_data.decode('utf-8'), 'stdout_sha256': release_tool.digest(stdout_data), 'stderr_sha256': release_tool.digest(stderr_data)}  # Bind exact UTF-8 output bytes.
        self.records.append(record)  # Preserve failed nonzero results for diagnostic reuse.
        require(not failure and process.returncode == 0, failure or 'native command failed; inspect ' + str(stem))  # Never translate failure into qualification.
        return stdout_data  # Let callers parse the actual native output.
#
def native_identity(target: dict[str, Any], runner_identity: str) -> dict[str, Any]:  # Require genuine native execution for package construction.
    require(platform.system() == 'Darwin' and platform.machine() == architectures[target['arch']][0], 'macOS packaging requires the selected native architecture')  # Exclude cross-host builds.
    require(runner_identity == target['runner'], 'packaging runner differs from targets.toml')  # Bind the manifest's runner identity.
    library = ctypes.CDLL('/usr/lib/libSystem.B.dylib', use_errno=True)  # Call the process-local Apple system interface.
    query = library.sysctlbyname  # Resolve the documented Rosetta query API.
    query.argtypes = [ctypes.c_char_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t), ctypes.c_void_p, ctypes.c_size_t]  # Match the native ABI precisely.
    query.restype = ctypes.c_int  # Interpret the system return code correctly.
    translated, size = ctypes.c_int(0), ctypes.c_size_t(ctypes.sizeof(ctypes.c_int))  # Allocate the documented int output.
    result = query(b'sysctl.proc_translated', ctypes.byref(translated), ctypes.byref(size), None, 0)  # Inspect this Python process, not a child command.
    error = ctypes.get_errno() if result != 0 else 0  # Capture errno before another system call.
    require((result == 0 and size.value == ctypes.sizeof(translated) and translated.value == 0) or (result == -1 and error == errno.ENOENT), 'Rosetta translation or indeterminate native execution is prohibited')  # Treat only Apple's documented ENOENT case as native.
    return {'system': 'Darwin', 'machine': platform.machine(), 'release': platform.release(), 'version': platform.version(), 'runner': runner_identity, 'translated': False}  # Retain observed platform identity.
#
def load_native(native: Path, archive: Path, manifest: Path, workspace: Path, tag: str, architecture: str, source_commit: str) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any]]:  # Bind all packaging inputs without executing payload code.
    require(architecture in architectures and re.fullmatch(r'[0-9a-f]{40}', source_commit), 'invalid target/source identity')  # Reject arbitrary target or shortened commit IDs.
    target = release_tool.selected_target(manifest, architecture + '-apple-darwin')  # Consume the unchanged five-target manifest.
    version = release_tool.workspace_version(workspace, tag)  # Require exact workspace/tag agreement.
    require(version == version_from_tag(tag), 'native package version differs')  # Apply the explicit numeric PKG policy.
    require(native.is_dir() and not native.is_symlink(), 'native input must be a regular directory')  # Preserve the input boundary.
    require(archive.parent.resolve() == native.resolve() and archive.name == target['archive'], 'archive must be the selected native artifact')  # Do not accept an unrelated tarball.
    require({path.name for path in native.iterdir()} == set(native_names) | {'candidate', 'evidence', target['archive']}, 'native input has missing or unexpected entries')  # Match the complete supplied output contract.
    native_hashes = {name: sha(native / name) for name in (*native_names, target['archive'])}  # Bind every top-level regular native artifact.
    bridge = read_json(native / 'native-candidate-receipt.json')  # Read the original native audit/package bridge.
    audit = release_tool.audit_receipt(regular_file(native / 'native-audit.json', max_json_bytes), target, version, tag)  # Reuse all original native qualification guards.
    require(bridge.get('schema') == 1 and bridge.get('state') == 'passed' and bridge.get('publication_allowed') is True and bridge.get('target') == target['rust_target'] and bridge.get('tag') == tag, 'native bridge is unqualified or mismatched')  # Reject a build-only or foreign receipt.
    for field, name in (('archive', target['archive']), ('native_audit', 'native-audit.json'), ('runtime_inventory', 'runtime-inventory.json'), ('dependency_inventory', 'dependency-inventory.json'), ('embedding_receipt', 'embedding-receipt.json')):  # Follow each supplied immutable receipt edge.
        require(bridge.get(field, {}).get('sha256') == native_hashes[name], 'native bridge hash differs: ' + field)  # Ignore obsolete absolute runner path labels.
    source_root = workspace.resolve().parent  # Resolve the selected source checkout once.
    source_hashes = {name: sha(source_root / name) for name in source_names}  # Bind policy, source manifests, and actual acceptance wrapper bytes.
    require(source_hashes['release/targets.toml'] == sha(manifest), 'selected manifest differs from checkout')  # Prevent a second manifest source.
    require(bridge.get('workspace_sha256') == source_hashes['Cargo.toml'] and bridge.get('lock_sha256') == source_hashes['Cargo.lock'] and bridge.get('embedding_model_register_sha256') == source_hashes['release/embedding-model.json'], 'native source inputs differ')  # Preserve native source/model custody.
    require(bridge.get('files') == audit['files'], 'native bridge payload differs from audit')  # Require the final post-signing hash map.
    require(audit['native_identity'].get('runner') == target['runner'] and bridge.get('native_identity', {}).get('runner') == target['runner'], 'native receipt runner differs')  # Require the same manifest runner throughout.
    require((native / 'SHA256SUMS').read_bytes() == (native_hashes[target['archive']] + '  ' + target['archive'] + '\n').encode('ascii'), 'native single-archive checksum file differs')  # Do not confuse this with the aggregate five-entry file.
    require(audit.get('runtime_inventory_sha256') == native_hashes['runtime-inventory.json'] and audit.get('dependency_inventory_sha256') == native_hashes['dependency-inventory.json'], 'audit dependency evidence hashes differ')  # Bind the original dependency evidence objects.
    runtime = audit_native.validate_runtime_inventory(read_json(native / 'runtime-inventory.json'), target['rust_target'])  # Validate reviewed runtime names and flags.
    runtime_names = {item['name'] for item in runtime['files']}  # Use names, not pre-relocation runtime hashes, for final payload closure.
    require(runtime_names and all(name.endswith('.dylib') for name in runtime_names), 'macOS runtime inventory is missing dylibs')  # Reject non-Mach-O payload classifications.
    require(set(audit['files']) == set(target['executables']) | set(target['packages']) | {'VERSION', 'THIRD-PARTY.txt'} | runtime_names, 'audit carries undeclared payload files')  # Close the exact code/data inventory.
    bundled = audit_native.validate_closure('macos', audit['dependency_closure'].get('graph', {}), runtime, set(audit['files']), target['executables'])  # Reject unknown, unreachable, or unsafe imports.
    require(bundled == audit['dependency_closure'].get('bundled'), 'audited runtime closure differs')  # Retain the complete native closure claim.
    verify_tree_hashes(native / 'candidate', audit['files'])  # Reject unknown candidate extras before packaging.
    harness = read_json(native / 'native-test-harness.json')  # Source commit belongs to this supplied receipt.
    require(harness.get('schema') == 1 and harness.get('source_commit') == source_commit and harness.get('target') == target['rust_target'] and harness.get('tag') == tag and harness.get('version') == version, 'native harness source/target/version differs')  # Bind the actual source identity.
    require(harness.get('filename') == 'native-test-binary' and harness.get('path') == 'evidence/harness/native-test-binary' and harness.get('sha256') == native_hashes['native-test-binary'], 'native harness identity differs')  # Use only the supplied portable harness path.
    retained = harness.get('evidence_files')  # Load the complete native evidence map.
    require(isinstance(retained, dict) and retained and all(name.startswith('evidence/') for name in retained), 'native evidence map is malformed')  # Prevent evidence paths escaping their root.
    evidence_hashes = {name.removeprefix('evidence/'): digest for name, digest in retained.items()}  # Convert to root-relative verification paths.
    verify_tree_hashes(native / 'evidence', evidence_hashes)  # Reject modified or unlisted recursive evidence.
    require(retained.get(harness['path']) == harness['sha256'], 'retained harness copy differs')  # Bind both portable harness copies.
    harness_runtime = harness.get('runtime_files')  # Preserve separately sealed harness runtime identities.
    require(isinstance(harness_runtime, dict) and set(harness_runtime) == runtime_names, 'native harness runtime names differ')  # Require the complete matching code closure.
    require(all(retained.get('evidence/harness/' + name) == digest for name, digest in harness_runtime.items()), 'retained harness runtime hashes differ')  # Do not equate these hashes to packaged dylibs.
    model = read_json(source_root / 'release/embedding-model.json')  # Read the pinned test-only model register.
    model_files = model.get('files')  # Keep model data out of the product payload.
    require(model.get('reviewed') is True and isinstance(model_files, dict) and model_files and all(retained.get('evidence/model/' + name) == digest for name, digest in model_files.items()), 'retained embedding model differs')  # Bind native acceptance inputs.
    specification = read_json(native / 'embedding-command.json')  # Verify only the supplied wrapper protocol.
    require(specification.get('schema') == 1 and specification.get('state') == 'reviewed' and specification.get('protocol') == 'held-installed-process-v1' and specification.get('sha256') == source_hashes['release/tests/embedding_acceptance.py'], 'embedding command is not source-bound')  # Do not invoke stale runner command paths.
    signing = audit['signing']  # Preserve the native audit's distribution-signing distinction.
    code_names = set(target['executables']) | runtime_names  # Include every executable and library seal.
    require(set(signing.get('nested_code', {})) == code_names and all(signing['nested_code'][name].get('verified') is True for name in code_names), 'native code seals are incomplete')  # Preserve Apple Silicon's valid ad-hoc seals.
    binding = {'source_commit': source_commit, 'source_inputs': source_hashes, 'source_archive': target['archive'], 'source_archive_sha256': native_hashes[target['archive']], 'native_audit_sha256': native_hashes['native-audit.json'], 'native_candidate_receipt_sha256': native_hashes['native-candidate-receipt.json'], 'native_test_harness_sha256': native_hashes['native-test-harness.json'], 'native_files': native_hashes, 'native_evidence_files_sha256': release_tool.digest(json.dumps(retained, sort_keys=True, separators=(',', ':')).encode('utf-8'))}  # Expose exact future aggregation bindings.
    return target, audit, runtime, binding  # Keep validation reusable by M3 and offline M4 fixtures.
#
def package_layout(architecture: str, version: str) -> dict[str, Any]:  # Define one complete CLI payload layout per format.
    require(architecture in architectures and version_from_tag('v' + version) == version, 'invalid package layout identity')  # Refuse unsafe path/version inputs.
    prefix = 'ilium-macos-' + architecture  # Preserve the tarball's parent directory.
    return {'zip': {'prefix': prefix}, 'dmg': {'prefix': prefix, 'volume_name': 'Ilium-' + version + '-' + architecture, 'filesystem': 'HFS+', 'format': 'UDZO'}, 'pkg': {'identifier': 'io.github.arthurwolf.ilium.' + architecture + '.v' + version, 'version': version, 'install_location': '/usr/local/lib/ilium/' + version + '/' + architecture, 'component': 'ilium-component.pkg', 'host_architecture': architectures[architecture][0], 'installation_domain': 'system-or-nonsystem-volume', 'administrator_required': True, 'scripts': False, 'path_modified': False}}  # Keep versions separate and require explicit PATH selection.
#
def distribution_xml(layout: dict[str, Any]) -> bytes:  # Generate escaped, script-free Installer metadata.
    package = layout['pkg']  # Read only the validated layout contract.
    document = xml_tree.Element('installer-gui-script', {'minSpecVersion': '2'})  # Use Apple's documented Distribution root.
    xml_tree.SubElement(document, 'title').text = 'Ilium ' + package['version']  # Identify the terminal software's exact version.
    xml_tree.SubElement(document, 'options', {'customize': 'never', 'require-scripts': 'false', 'allow-external-scripts': 'false', 'hostArchitectures': package['host_architecture']})  # Exclude installer JavaScript and wrong architecture declarations.
    xml_tree.SubElement(document, 'domains', {'enable_anywhere': 'true', 'enable_currentUserHome': 'false', 'enable_localSystem': 'true'})  # Permit a real isolated nonsystem-volume smoke target.
    outline = xml_tree.SubElement(document, 'choices-outline')  # Declare exactly one mandatory installation choice.
    xml_tree.SubElement(outline, 'line', {'choice': 'ilium'})  # Link the only choice into the outline.
    choice = xml_tree.SubElement(document, 'choice', {'id': 'ilium', 'title': 'Ilium command-line tools', 'description': 'Installs Ilium, its server, animation helper, official animations and runtime libraries in ' + package['install_location'] + '. Add that directory to PATH. This package is unsigned and not notarized.', 'visible': 'false', 'selected': 'true', 'enabled': 'false'})  # State the actual installation behavior.
    xml_tree.SubElement(choice, 'pkg-ref', {'id': package['identifier']})  # Bind the choice to the unique component.
    xml_tree.SubElement(document, 'pkg-ref', {'id': package['identifier'], 'version': package['version'], 'onConclusion': 'None'}).text = package['component']  # Refer only to the locally generated component.
    xml_tree.indent(document, space='  ')  # Keep the exact generated metadata readable.
    return xml_tree.tostring(document, encoding='utf-8', xml_declaration=True) + b'\n'  # Retain deterministic XML bytes.
#
def inspect_code(payload: Path, target: dict[str, Any], audit: dict[str, Any], runtime: dict[str, Any], runner: command_runner) -> None:  # Repeat only read-only native inspection.
    names = sorted(set(target['executables']) | {item['name'] for item in runtime['files']})  # Inspect the complete admitted code graph.
    graph: dict[str, list[str]] = {}  # Reconstruct actual load edges from the packaged bytes.
    for index, name in enumerate(names):  # Keep each native inspection separately evidenced.
        path = payload / name  # Inspect this owned extracted file only.
        architecture = runner.run('lipo-' + str(index), ['/usr/bin/lipo', '-archs', path]).decode('utf-8').strip()  # Observe the native tool's architecture result.
        require(architecture == architectures[target['arch']][0], 'native Mach-O architecture differs: ' + name)  # Reject universal and foreign architecture files.
        output = runner.run('otool-' + str(index), ['/usr/bin/otool', '-l', path]).decode('utf-8')  # Read complete actual load commands.
        install_name, dependencies = audit_native.parse_macos_load_commands(output)  # Preserve weak/reexport/lazy/upward edge handling.
        require(name in target['executables'] or install_name == '@executable_path/' + name, 'dylib install name changed')  # Preserve the audited sibling-runtime contract.
        require(all(value in ('@executable_path', '@loader_path') for value in re.findall(r'^\s+path (.+?) \(offset', output, re.MULTILINE)), 'unsafe Mach-O rpath')  # Reject build-host loader fallback paths.
        graph[name] = dependencies  # Retain the observed order of actual dependencies.
        runner.run('codesign-' + str(index), ['/usr/bin/codesign', '--verify', '--strict', '--verbose=2', path])  # Verify existing seals without changing bytes.
    require(graph == audit['dependency_closure']['graph'], 'packaged loader graph differs from native audit')  # Require exact audit/native agreement.
    audit_native.validate_closure('macos', graph, runtime, set(audit['files']), target['executables'])  # Repeat fail-closed recursive import admission.
    verify_tree_hashes(payload, audit['files'])  # Prove native inspection did not modify code.
#
def owned_images(runner: command_runner, owned_roots: tuple[Path, ...]) -> list[dict[str, Any]]:  # Reconcile only images beneath new task-owned roots.
    output = runner.run('hdiutil-info', ['/usr/bin/hdiutil', 'info', '-plist'], timeout=30, cleanup=True)  # Obtain authoritative attachment state with a finite cleanup budget.
    try:  # Keep native parsing failures inside the JSONL error contract.
        document = plistlib.loads(output)  # Parse structured device/image relationships.
    except (ValueError, OverflowError, plistlib.InvalidFileException, expat_error) as error:  # Handle binary and XML plist failures.
        raise release_tool.ReleaseError('malformed hdiutil image inventory') from error  # Never guess cleanup ownership from broken output.
    require(isinstance(document, dict) and isinstance(document.get('images'), list), 'hdiutil image inventory is malformed')  # Do not infer cleanup ownership from free text.
    result = []  # Exclude all other users' or tools' images.
    for image in document['images']:  # Examine the returned image records.
        require(isinstance(image, dict), 'hdiutil image record is malformed')  # Refuse an unparseable cleanup inventory.
        name = image.get('image-path')  # Use the native image-to-device binding.
        if not isinstance(name, str) or not Path(name).is_absolute():  # Ignore entries that cannot name an owned absolute input.
            continue  # Never derive ownership from a volume label alone.
        path = Path(name).resolve()  # Normalize macOS /var path aliases.
        if not any(path.is_relative_to(owner) for owner in owned_roots):  # Keep cleanup within this invocation's exclusive storage.
            continue  # Preserve every unrelated attachment.
        result.append(image)  # Retain only an explicitly owned image record.
    require(len(result) <= 8, 'unexpected owned disk-image count')  # Bound reconciliation operations.
    return result  # Let cleanup select whole disks from proven image records.
#
def detach_owned_images(runner: command_runner, owned_roots: tuple[Path, ...]) -> None:  # Handle hdiutil interruption without blanket detachment.
    for image in owned_images(runner, owned_roots):  # Reconcile even a creation command that failed before returning metadata.
        entities = image.get('system-entities')  # Inspect devices belonging to this exact owned image.
        require(isinstance(entities, list), 'owned image has no device inventory; retain work')  # Preserve uncertainty rather than guess a device.
        devices = {entry.get('dev-entry') for entry in entities if isinstance(entry, dict) and isinstance(entry.get('dev-entry'), str) and re.fullmatch(r'/dev/disk[0-9]+', entry['dev-entry'])}  # Select a whole owned disk only.
        require(len(devices) == 1, 'owned image has ambiguous device identity; retain work')  # Refuse broad or partial cleanup.
        runner.run('hdiutil-detach', ['/usr/bin/hdiutil', 'detach', devices.pop()], timeout=60, cleanup=True)  # Detach normally without force.
    require(not owned_images(runner, owned_roots), 'owned disk images remain attached; retain work')  # Treat incomplete cleanup as a failure.
#
def fresh_path(path: Path) -> Path:  # Check supplied destinations before resolving aliases.
    require(not os.path.lexists(path), 'destination must be new: ' + str(path))  # Include dangling symlinks in the rejection.
    parent = path.parent.resolve(strict=True)  # Allow existing macOS /var aliases only at the parent boundary.
    require(parent.is_dir() and path.name not in ('', '.', '..'), 'destination parent is invalid')  # Require an existing explicit parent.
    return parent / path.name  # Return a canonical, still-uncreated destination.
#
def commit_directory(source: Path, destination: Path) -> None:  # Commit on macOS without replacing even an empty concurrent directory.
    library = ctypes.CDLL('/usr/lib/libSystem.B.dylib', use_errno=True)  # Load the native exclusive-rename interface.
    rename = library.renamex_np  # Require Apple's documented extended rename API.
    rename.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]  # Match the source, destination, and flags ABI.
    rename.restype = ctypes.c_int  # Preserve the native success/failure result.
    result = rename(os.fsencode(source), os.fsencode(destination), 0x00000004)  # RENAME_EXCL atomically rejects every existing destination.
    error = ctypes.get_errno() if result != 0 else 0  # Capture the native failure before any other call.
    require(result == 0, 'exclusive output commit failed with errno ' + str(error) + '; staged products retained')  # Never fall back to a replacing rename.
#
def build(arguments: argparse.Namespace) -> dict[str, Any]:  # Build all formats from one frozen native tarball.
    workspace, manifest = arguments.workspace.absolute(), arguments.manifest.absolute()  # Keep original terminal paths visible to regular-file checks.
    regular_file(workspace, max_json_bytes)  # Reject a symlinked workspace file.
    regular_file(manifest, max_json_bytes)  # Reject a symlinked manifest file.
    require(not arguments.native.is_symlink() and not arguments.archive.is_symlink(), 'native inputs cannot be symlinks')  # Check before resolve removes the terminal alias.
    native, archive = arguments.native.resolve(strict=True), arguments.archive.resolve(strict=True)  # Normalize source aliases consistently.
    target, audit, runtime, binding = load_native(native, archive, manifest, workspace, arguments.tag, arguments.arch, arguments.source_commit)  # Validate the full original custody chain first.
    identity = native_identity(target, arguments.runner_identity)  # Do not run native packaging tools on another host.
    work, output = fresh_path(arguments.work), fresh_path(arguments.output)  # Reserve independent new ownership boundaries.
    require(work != output and not work.is_relative_to(output) and not output.is_relative_to(work), 'work and output overlap')  # Keep publication separate from retained build evidence.
    protected = [native, workspace.resolve(), manifest.resolve(), workspace.resolve().parent]  # Protect source and downloaded evidence from output replacement.
    require(all(not path.is_relative_to(destination) for path in protected for destination in (work, output)), 'destination contains protected input/source')  # Reject destructive ancestor destinations.
    require(not work.is_relative_to(native) and not output.is_relative_to(native), 'destination is inside native input')  # Preserve exact native artifact inventories.
    work.mkdir(mode=0o700)  # Claim the new work directory exclusively.
    runner = command_runner(work)  # Allocate bounded logs and owned native temporary storage.
    products = Path(tempfile.mkdtemp(prefix='.ilium-macos-products-', dir=output.parent)).resolve()  # Stage on the destination filesystem for atomic directory commit.
    write_json(work / 'ownership.json', {'schema': 1, 'work': str(work), 'staged_products': str(products), 'output': str(output), 'state': 'building'})  # Retain exact cleanup locations before native tools run.
    owners = (work, products)  # These directories did not exist before this invocation.
    completed = False  # Never expose a final output before all required build checks.
    try:  # Preserve product staging and diagnostic evidence on any failure.
        source_root = workspace.resolve().parent  # Use the supplied checkout only for source identity.
        commit = runner.run('git-commit', ['/usr/bin/git', '-C', source_root, 'rev-parse', 'HEAD']).decode('ascii').strip()  # Observe the actual checkout commit.
        require(commit == arguments.source_commit, 'checkout differs from expected source commit')  # Refuse a self-asserted foreign source.
        dirty = runner.run('git-status', ['/usr/bin/git', '-C', source_root, 'status', '--porcelain', '--untracked-files=no'])  # Match the existing tracked-source cleanliness policy.
        require(not dirty.strip(), 'tracked source is dirty')  # Do not produce a same-source receipt from modified tracked code.
        for module_name, module_path in (('release_tool.py', Path(release_tool.__file__)), ('audit_native.py', Path(audit_native.__file__)), ('build_macos_packages.py', Path(__file__))):  # Bind the actual loaded policy modules.
            require(sha(module_path) == binding['source_inputs']['release/scripts/' + module_name], 'loaded packaging module differs from checkout')  # Prevent import-path substitution.
        snapshot = work / target['archive']  # Keep an owned immutable payload input for every format.
        copied = 0  # Bound total snapshot bytes even if the source grows concurrently.
        with archive.open('rb') as source, snapshot.open('xb') as destination:  # Copy without metadata or link preservation.
            while block := source.read(min(1_048_576, max_payload_bytes - copied + 1)):  # Read at most one byte beyond the allowed total.
                copied += len(block)  # Count the bytes actually read.
                require(copied <= max_payload_bytes, 'archive grew beyond snapshot size bound')  # Fail before writing excess source bytes.
                destination.write(block)  # Preserve each bounded source block exactly.
            destination.flush()  # Complete the snapshot before hashing it.
            os.fsync(destination.fileno())  # Synchronize the exact copied input.
        require(sha(snapshot) == binding['source_archive_sha256'], 'archive changed during snapshot')  # Bind parsed bytes to the original native receipt.
        payload = work / 'payload'  # Keep the audited sibling layout independent of wrappers.
        version = version_from_tag(arguments.tag)  # Reuse the already checked package version.
        content = extract_package(snapshot, target, audit, version, payload)  # Never execute or rebuild the extracted code.
        payload_tree = tree_inventory(payload)  # Record exact file modes, lengths, and digests.
        inspect_code(payload, target, audit, runtime, runner)  # Repeat read-only architecture, loader, and seal verification.
        tools = {name: {'path': name, 'resolved_path': str(Path(name).resolve(strict=True)), 'sha256': sha(Path(name).resolve(strict=True), allow_hardlinks=True)} for name in ('/usr/bin/pkgbuild', '/usr/bin/productbuild', '/usr/bin/hdiutil', '/usr/bin/lipo', '/usr/bin/otool', '/usr/bin/codesign')}  # Record system tools while admitting Apple's shared tool stubs.
        toolchain = {'python': sys.version, 'executables': tools, 'macos': runner.run('sw-vers', ['/usr/bin/sw_vers']).decode('utf-8'), 'xcode': runner.run('xcode-version', ['/usr/bin/xcodebuild', '-version']).decode('utf-8'), 'developer_directory': runner.run('developer-directory', ['/usr/bin/xcode-select', '-p']).decode('utf-8').strip()}  # Retain actual OS and selected toolchain versions.
        layout = package_layout(arguments.arch, version)  # Freeze every format's declared layout.
        zip_target = dict(target, archive=package_name(arguments.arch, 'zip'), format='zip')  # Derive a ZIP view without changing the canonical manifest.
        zip_path = products / zip_target['archive']  # Name the first required derivative artifact.
        release_tool.write_archive(zip_path, zip_target, content)  # Use the existing deterministic ZIP format and modes.
        release_tool.verify_content(release_tool.read_archive(zip_path, zip_target, audit), audit, version)  # Independently parse and hash the finished ZIP.
        components = work / 'components'  # Separate the intermediate component PKG from published assets.
        components.mkdir(mode=0o700)  # Create a new private component directory.
        component = components / layout['pkg']['component']  # Use one fixed, locally resolved component name.
        runner.run('pkgbuild', ['/usr/bin/pkgbuild', '--root', payload, '--identifier', layout['pkg']['identifier'], '--version', version, '--install-location', layout['pkg']['install_location'], '--ownership', 'recommended', '--compression', 'legacy', '--quiet', component], timeout=600)  # Build without scripts, signing, or binary rewriting.
        distribution = work / 'Distribution.xml'  # Keep native metadata as retained source evidence.
        write_bytes(distribution, distribution_xml(layout))  # Generate exact escaped distribution bytes.
        pkg_path = products / package_name(arguments.arch, 'pkg')  # Name the mandatory system-installable container.
        runner.run('productbuild', ['/usr/bin/productbuild', '--distribution', distribution, '--package-path', components, '--quiet', pkg_path], timeout=600)  # Wrap the sole local component without credentials.
        image_source = work / 'image-source'  # Supply a controlled root containing only the portable folder.
        image_source.mkdir(mode=0o755)  # Create the DMG source root explicitly.
        image_source.chmod(0o755)  # Remove ambient umask influence.
        image_payload = image_source / layout['dmg']['prefix']  # Preserve the tar/ZIP parent directory inside the volume.
        image_payload.mkdir(mode=0o755)  # Create the only product directory in the image.
        image_payload.chmod(0o755)  # Normalize its root permissions.
        for name, data in sorted(content.items()):  # Copy every audited payload member into the volume source.
            write_bytes(image_payload / name, data, 0o755 if name in target['executables'] else 0o644)  # Preserve bytes and sibling permissions exactly.
        image_tree = {layout['dmg']['prefix']: {'kind': 'directory', 'mode': 0o755}, **{layout['dmg']['prefix'] + '/' + name: entry for name, entry in payload_tree.items()}}  # Admit the portable folder and its exact payload only.
        require(tree_inventory(image_source) == image_tree, 'DMG source inventory differs')  # Reject unknown siblings as well as payload changes before imaging.
        dmg_path = products / package_name(arguments.arch, 'dmg')  # Name the final required derivative artifact.
        runner.run('hdiutil-create', ['/usr/bin/hdiutil', 'create', '-srcfolder', image_source, '-fs', 'HFS+', '-format', 'UDZO', '-volname', layout['dmg']['volume_name'], dmg_path], timeout=600)  # Create the native read-only disk image without attaching it for use.
        runner.run('hdiutil-verify', ['/usr/bin/hdiutil', 'verify', dmg_path], timeout=300)  # Verify container checksum without claiming payload smoke or notarization.
        detach_owned_images(runner, owners)  # Reconcile possible native-tool attachments before committing outputs.
        require(tree_inventory(payload) == payload_tree and tree_inventory(image_source) == image_tree, 'payload changed during native packaging')  # Reject tool-induced rewrites and extra DMG source entries.
        require(sha(snapshot) == binding['source_archive_sha256'], 'owned archive snapshot changed')  # Preserve the single payload source through the entire build.
        _target, _audit, _runtime, after = load_native(native, archive, manifest, workspace, arguments.tag, arguments.arch, arguments.source_commit)  # Reopen the original custody chain after packaging.
        require(after == binding, 'source/native input changed during packaging')  # Reject time-of-use changes to any bound input.
        require({path.name for path in products.iterdir()} == set(package_names(arguments.arch)), 'native tools produced missing or unexpected artifacts')  # Admit exactly three completed containers.
        hashes = {name: sha(products / name) for name in package_names(arguments.arch)}  # Hash final container bytes only.
        sizes = {name: (products / name).stat().st_size for name in hashes}  # Retain exact container lengths.
        require(all(size > 0 for size in sizes.values()), 'native package output is empty')  # Never publish empty tool output.
        receipt = {'schema': 1, 'state': 'built-not-qualified', 'publication_allowed': False, 'native_payload_executed': False, 'tag': arguments.tag, 'version': version, 'arch': arguments.arch, 'target': target['rust_target'], **binding, 'package_files': audit['files'], 'payload_tree': payload_tree, 'packages': hashes, 'package_bytes': sizes, 'layout': layout, 'distribution_sha256': sha(distribution), 'component_sha256': sha(component), 'native_identity': identity, 'toolchain': toolchain, 'commands': runner.records, 'payload_signing': audit['signing'], 'payload_notarization': audit['notarization'], 'container_signing': {item: 'unsigned' for item in formats}, 'container_notarization': {item: 'disabled' for item in formats}, 'credentials_used': False, 'owned_images_detached': True, 'work_retained': True}  # Distinguish construction evidence from the mandatory later lifecycle gate.
        write_json(products / receipt_name(arguments.arch), receipt)  # Freeze the build receipt after all native build checks.
        for path in products.iterdir():  # Synchronize every final product and its immutable receipt.
            with path.open('r+b') as stream:  # macOS supports flushing these owned regular outputs.
                os.fsync(stream.fileno())  # Complete writes before directory commit.
        require(not os.path.lexists(output), 'output appeared during build; replacement is prohibited')  # Preserve any concurrent user's destination.
        commit_directory(products, output)  # Atomically expose the complete set without replacing a concurrent entry.
        completed = True  # The final directory now owns the committed products.
        for path in sorted(output.iterdir()):  # Report every actual final artifact explicitly.
            emit('artifact', path=str(path), sha256=sha(path), bytes=path.stat().st_size)  # Include the immutable build receipt too.
        emit('result', command='build', state='built-not-qualified', publication_allowed=False, output=str(output), receipt=str(output / receipt_name(arguments.arch)), work=str(work), packages=hashes)  # Leave native lifecycle qualification visibly pending.
        return receipt  # Let offline tests inspect the full build contract.
    finally:  # Preserve exact ownership evidence on cancellation or failure.
        if not completed:  # A partial build must never look like final output.
            try:  # Attempt only bounded, image-path-bound native cleanup.
                detach_owned_images(runner, owners)  # Do not remove mounted trees or stop unrelated processes.
            except (ValueError, OSError, subprocess.SubprocessError, plistlib.InvalidFileException) as cleanup_error:  # Retain ambiguous ownership for the primary's inspection.
                emit('error', operation='cleanup', publication_allowed=False, message=str(cleanup_error), work=str(work), staged_products=str(products))  # Never conceal an incomplete detach.
            emit('progress', operation='retained-failed-build', work=str(work), staged_products=str(products), publication_allowed=False)  # Keep logs and partial files for bounded diagnosis.
#
def parser() -> release_tool.JsonArgumentParser:  # Expose only explicit, nonpublishing construction.
    result = release_tool.JsonArgumentParser(description=__doc__, allow_abbrev=False)  # Keep help/errors JSONL-compatible.
    commands = result.add_subparsers(dest='command', required=True)  # Require the explicit build operation.
    command = commands.add_parser('build', allow_abbrev=False)  # Do not add a skip-format or skip-audit route.
    command.add_argument('--manifest', type=Path, default=root / 'release/targets.toml')  # Read the authoritative target source.
    command.add_argument('--workspace', type=Path, default=root / 'Cargo.toml')  # Bind the source workspace version.
    command.add_argument('--tag', required=True)  # Require the exact release version.
    command.add_argument('--arch', required=True, choices=sorted(architectures))  # Choose exactly one native macOS architecture.
    command.add_argument('--source-commit', required=True)  # Bind the source job's full commit identity.
    command.add_argument('--runner-identity', required=True)  # Bind the manifest's native runner.
    command.add_argument('--native', type=Path, required=True)  # Supply the complete downloaded native evidence artifact.
    command.add_argument('--archive', type=Path, required=True)  # Supply the selected audited tarball explicitly.
    command.add_argument('--work', type=Path, required=True)  # Require a new owned work directory.
    command.add_argument('--output', type=Path, required=True)  # Require a new atomic final artifact directory.
    return result  # Let M5 tests parse the exact workflow invocation.
#
def main(argv: list[str] | None = None) -> int:  # Preserve a directly testable JSONL CLI.
    try:  # Convert bounded build failures to nonzero structured results.
        arguments = parser().parse_args(argv)  # Reject unknown options and missing explicit inputs.
        build(arguments)  # Produce all three formats or fail the build.
        return 0  # This indicates construction only, never publication qualification.
    except (ValueError, OSError, KeyError, TypeError, AttributeError, EOFError, struct.error, subprocess.SubprocessError, tarfile.TarError, zipfile.BadZipFile, plistlib.InvalidFileException) as error:  # Retain supplied-parser and native-tool failures.
        emit('error', command='build', state='blocked', publication_allowed=False, message=str(error)[:2000])  # Do not print traceback data or credentials.
        return 2  # Match the fail-closed release CLI convention.
    except KeyboardInterrupt:  # Treat cancellation as incomplete work.
        emit('error', command='build', state='cancelled', publication_allowed=False)  # Keep cancellation distinct from a passed result.
        return 130  # Preserve the conventional interruption status.
#
if __name__ == '__main__':  # Run only when explicitly invoked as the CLI.
    sys.exit(main())  # Return the structured operation's status to Actions.
