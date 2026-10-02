#!/usr/bin/env python3
"""Bridge matching native build bytes to fail-closed audit/package receipts.

No Cargo build, dependency licence inference or network acquisition occurs here.
Unknown loader edges or unavailable reviewed redistribution rights block output.
"""
from __future__ import annotations
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
from types import SimpleNamespace

from audit_native import (audit, generate_notices, licence_bytes, native_identity,
                          parse_dependencies, parse_macos_load_commands, run,
                          reject_windows_dynamic_crt, system_dependency, windows_version)
from release_tool import (JsonArgumentParser, ReleaseError, digest, emit, package,
                          read_json, safe_member_name, selected_target, workspace_version)

ROOT = Path(__file__).resolve().parents[2]


def require(condition, message):
    if not condition:
        raise ReleaseError(message)


def sha(path):
    value = hashlib.sha256()
    with Path(path).open('rb') as stream:
        while block := stream.read(65536):
            value.update(block)
    return value.hexdigest()


def write_json(path, value):
    with Path(path).open('x', encoding='utf-8') as output:
        json.dump(value, output, indent=2, sort_keys=True, allow_nan=False)
        output.write('\n')


def plain(path):
    path = Path(path)
    require(path.is_absolute() and path.is_file() and not path.is_symlink(), 'Expected absolute plain file: ' + str(path))
    return path


def extract_ort_notices(archive_path, register_path, output):
    register = read_json(register_path)
    require(register.get('state') == 'reviewed' and register.get('version') == '1.24.2' and register.get('commit') == '058787ceead760166e3c50a0a4cba8a833a6f53f', 'ORT source register differs from locked native runtime')
    require(sha(plain(archive_path)) == register.get('source_sha256'), 'ORT source archive differs from reviewed bytes')
    prefix = 'onnxruntime-' + register['commit'] + '/'
    names = [prefix + 'LICENSE', prefix + 'ThirdPartyNotices.txt']
    contents = {}
    with tarfile.open(archive_path, 'r:gz') as archive:
        for member in archive:
            if member.name not in names:
                continue
            require(member.name not in contents and member.isfile() and 0 < member.size <= 16_777_216, 'Invalid or duplicate ORT notice member')
            stream = archive.extractfile(member)
            require(stream is not None, 'ORT notice member cannot be read')
            contents[member.name] = stream.read(member.size + 1)
            require(len(contents[member.name]) == member.size, 'ORT notice member truncated')
    require(set(contents) == set(names), 'Pinned ORT source archive lacks root licence/notices')
    for name in names:
        contents[name].decode('utf-8')
        expected = register.get('notice_sha256', {}).get(Path(name).name)
        require(expected is None or digest(contents[name]) == expected, 'ORT root notice hash differs')
    path = output / 'ORT-LICENSE-AND-NOTICES.txt'
    path.write_bytes(contents[names[0]] + b'\n\nONNX Runtime Third-Party Notices\n\n' + contents[names[1]])
    return {'license': 'MIT with upstream third-party notices', 'license_source': register['source_url'], 'license_file': str(path), 'license_sha256': sha(path), 'reviewed': True}, {'archive_sha256': sha(archive_path), 'members': {Path(name).name: digest(data) for name, data in contents.items()}}


def loader_dependencies(target, path, dumpbin):
    if target['os'] == 'linux':
        result = run(['readelf', '-d', path])
        values = parse_dependencies('linux', result.stdout)
    elif target['os'] == 'macos':
        result = run(['otool', '-l', path])
        _, values = parse_macos_load_commands(result.stdout)
    else:
        require(dumpbin is not None, 'Windows requires an explicit dumpbin executable')
        result = run([dumpbin, '/DEPENDENTS', path])
        values = parse_dependencies('windows', result.stdout)
    require(values, 'Native dependency evidence has no loader edges: ' + str(path))
    return values, {'command_output': result.stdout, 'source_path': str(path), 'sha256': sha(path)}


def select_runtime(name, roots, allowed_hash=None):
    # Cargo/ORT may retain multiple identical library copies; distinct bytes
    # are ambiguous evidence and must never be selected by path ordering.
    candidates = []
    for root in roots:
        require(root.is_absolute() and root.is_dir() and not root.is_symlink(), 'Runtime search directory must be a plain absolute directory')
        for path in root.rglob('*'):
            if path.name.casefold() == name.casefold() and path.is_file() and not path.is_symlink():
                candidates.append(path.resolve())
    require(candidates, 'Shared runtime missing from explicit build/runtime directories: ' + name)
    hashes = {sha(path) for path in candidates}
    require(len(hashes) == 1, 'Distinct runtime bytes make dependency resolution ambiguous: ' + name)
    require(allowed_hash is None or hashes == {allowed_hash}, 'Runtime differs from reviewed build/licence evidence: ' + name)
    return sorted(set(candidates))[0]


def canonical_ort_name(dependency, operating_system):
    name = Path(dependency).name
    if operating_system == 'macos' and name in ('libonnxruntime.dylib', 'libonnxruntime.1.dylib', 'libonnxruntime.1.24.2.dylib'):
        return 'libonnxruntime.1.24.2.dylib'
    if operating_system == 'linux' and name in ('libonnxruntime.so', 'libonnxruntime.so.1', 'libonnxruntime.so.1.24.2'):
        return name
    if operating_system == 'windows' and name.casefold() == 'onnxruntime.dll':
        return 'onnxruntime.dll'
    return None


def discovery(arguments, target, candidate, notices):
    roots = [arguments.build_directory]
    if arguments.runtime_directory:
        roots.append(arguments.runtime_directory)
    external = {}
    if arguments.runtime_license_inventory:
        inventory = read_json(arguments.runtime_license_inventory)
        require(inventory.get('schema') == 1 and inventory.get('state') == 'reviewed' and inventory.get('target') == arguments.target, 'Additional runtime licence inventory is unreviewed or target differs')
        for item in inventory.get('files', []):
            safe_member_name(item.get('name'))
            key = item['name'].casefold()
            require(key not in external, 'Duplicate additional runtime licence')
            licence_bytes(item)
            external[key] = item
    pending = list(target['executables'])
    graph, evidence, runtimes, systems = {}, {}, {}, {}
    unreviewed_by_binary = {}
    while pending:
        name = pending.pop()
        if name in graph:
            continue
        values, identity = loader_dependencies(target, candidate / name, arguments.dumpbin)
        graph[name], evidence[name] = values, identity
        # Name every unreviewed dependency at once: a CI audit cycle is far too
        # slow to discover them one failure at a time.
        unreviewed = sorted(
            dependency for dependency in values
            if not system_dependency(target['os'], dependency)
            and not canonical_ort_name(dependency, target['os'])
            and external.get(Path(dependency).name.casefold()) is None)
        if unreviewed:
            # Keep walking the rest of the graph so one failure names them all.
            unreviewed_by_binary[name] = unreviewed
        for dependency in values:
            if dependency in unreviewed:
                continue
            if target['os'] == 'windows':
                reject_windows_dynamic_crt(dependency)
            if system_dependency(target['os'], dependency):
                systems[dependency.casefold() if target['os'] == 'windows' else dependency] = {'name': dependency, 'reviewed': True, 'source': {'macos': 'https://developer.apple.com/documentation', 'windows': 'https://learn.microsoft.com/windows/win32/api/', 'linux': 'https://packages.ubuntu.com/jammy/'}[target['os']]}
                continue
            runtime_name = canonical_ort_name(dependency, target['os'])
            if runtime_name:
                item = dict(notices, name=runtime_name, version='1.24.2')
                expected = None
                build_report = arguments.intel_ort_report or getattr(arguments, 'windows_ort_report', None)
                if build_report:
                    receipt = read_json(build_report)
                    expected = receipt.get('runtime', {}).get('sha256')
                    source = plain(Path(receipt.get('runtime', {}).get('path', '')))
                    require(source.name.casefold() == runtime_name.casefold() and sha(source) == expected, 'Source-built runtime path differs from build receipt')
                else:
                    source_name = 'libonnxruntime.so.1.24.2' if target['os'] == 'linux' else runtime_name
                    source = select_runtime(source_name, roots)
                item['sha256'] = sha(source)
                if target['os'] == 'windows':
                    item['version'] = windows_version(source)
                    require(re.match(r'^1\.24\.2(?:\D|$)', item['version']), 'Windows ONNX Runtime version differs from pinned source')
            else:
                runtime_name = Path(dependency).name
                # Unknown absolute/local libraries are never declared system.
                supplied = external.get(runtime_name.casefold())
                require(supplied is not None, 'Unreviewed non-system native dependency: ' + dependency)
                item = dict(supplied)
                require(isinstance(item.get('sha256'), str) and re.fullmatch('[0-9a-f]{64}', item['sha256']) and bool(item.get('version')), 'Additional runtime lacks reviewed byte/version identity')
                require(target['os'] == 'windows' or item['name'] == runtime_name, 'Additional runtime filename case differs from loader dependency')
                runtime_name = item['name']
                source = select_runtime(runtime_name, roots, item['sha256'])
            safe_member_name(runtime_name)
            if runtime_name not in runtimes:
                require(runtime_name not in target['executables'], 'Runtime dependency collides with executable')
                destination = candidate / runtime_name
                require(not destination.exists(), 'Runtime output collides')
                shutil.copy2(source, destination)
                item['source_path'] = str(source)
                runtimes[runtime_name] = item
                pending.append(runtime_name)
    require(not unreviewed_by_binary, 'Unreviewed non-system native dependency: ' + '; '.join(
        binary + ' imports ' + ', '.join(names) for binary, names in sorted(unreviewed_by_binary.items())))
    if target['os'] in ('linux', 'macos', 'windows'):
        require(any('onnxruntime' in name.casefold() for name in runtimes), 'Native client must link a bundled shared ONNX Runtime')
    inventory = {'schema': 1, 'state': 'reviewed', 'publication_allowed': True, 'target': arguments.target, 'files': list(runtimes.values()), 'system_libraries': list(systems.values())}
    return inventory, {'graph': graph, 'evidence': evidence}


def build(arguments):
    target = selected_target(arguments.manifest, arguments.target)
    identity = native_identity(target, arguments.runner_identity)
    version = workspace_version(arguments.workspace, arguments.tag)
    require(arguments.output_directory.is_absolute() and not arguments.output_directory.exists(), 'Output must be a new absolute owned directory')
    require(arguments.build_directory.is_absolute() and arguments.build_directory.is_dir() and not arguments.build_directory.is_symlink(), 'Explicit native build directory required')
    require(arguments.output_directory.resolve() not in [arguments.workspace.parent.resolve(), arguments.build_directory.resolve()], 'Output collides with build/workspace')
    arguments.output_directory.mkdir(parents=False)
    output = arguments.output_directory.resolve()
    candidate = output / 'candidate'; candidate.mkdir()
    evidence_directory = output / 'evidence'; evidence_directory.mkdir()
    windows_ort_binding = None
    windows_cmake_binding = None
    if arguments.windows_ort_report is not None:
        source_receipt = plain(arguments.windows_ort_report)
        source_receipt_value = read_json(source_receipt)
        source_cache = plain(Path(source_receipt_value.get('cmake_cache', {}).get('path', '')))
        require(sha(source_cache) == source_receipt_value.get('cmake_cache', {}).get('sha256'),
                'Windows retained CMake cache differs from the build receipt')
        retained_cache = evidence_directory / 'windows-ort-CMakeCache.txt'
        shutil.copyfile(source_cache, retained_cache)
        windows_cmake_binding = {'path': 'evidence/windows-ort-CMakeCache.txt',
                                 'sha256': sha(retained_cache)}
        retained_receipt = evidence_directory / 'windows-ort-build-receipt.json'
        shutil.copyfile(source_receipt, retained_receipt)
        windows_ort_binding = {'path': 'evidence/windows-ort-build-receipt.json',
                               'sha256': sha(retained_receipt)}
        # Every later discovery/audit read uses the retained bytes, so the
        # published bridge cannot bind one receipt while qualifying another.
        arguments.windows_ort_report = retained_receipt
    binaries = {}
    for name in target['executables']:
        source = plain(arguments.build_directory / name)
        shutil.copy2(source, candidate / name)
        binaries[name] = {'source_path': str(source), 'sha256': sha(source)}
    (candidate / 'VERSION').write_text(version + '\n', encoding='utf-8')
    licence, ort_evidence = extract_ort_notices(arguments.ort_source_archive, ROOT / 'release/ort-source.json', evidence_directory)
    inventory, loader = discovery(arguments, target, candidate, licence)
    runtime_path = output / 'runtime-inventory.json'; write_json(runtime_path, inventory)
    dependencies_path = output / 'dependency-inventory.json'
    # Validate complete Cargo.lock + licence closure before attempting binaries.
    dependencies = read_json(arguments.dependency_inventory)
    generate_notices(arguments.workspace.parent / 'Cargo.lock', dependencies, inventory)
    write_json(dependencies_path, dependencies)
    wrapper = plain(ROOT / 'release/tests/embedding_acceptance.py')
    sys.path.insert(0, str(ROOT / 'release/tests'))
    from embedding_acceptance import FILES, validate_model
    validate_model(plain(arguments.model_directory / 'model.onnx'), ROOT / 'release/embedding-model.json')
    model_directory = evidence_directory / 'model'; model_directory.mkdir()
    for name in FILES:
        shutil.copy2(plain(arguments.model_directory / name), model_directory / name)
    model = model_directory / 'model.onnx'
    validate_model(model, ROOT / 'release/embedding-model.json')
    spec_path = output / 'embedding-command.json'
    specification = {'schema': 1, 'state': 'reviewed', 'reviewed_by': 'native candidate held-process contract; independent audit required', 'protocol': 'held-installed-process-v1', 'command': [str(wrapper), '--model-lock', str(ROOT / 'release/embedding-model.json')], 'sha256': sha(wrapper)}
    write_json(spec_path, specification)
    audit_path = output / 'native-audit.json'
    audit_args = SimpleNamespace(manifest=arguments.manifest, workspace=arguments.workspace, lockfile=arguments.workspace.parent / 'Cargo.lock', target=arguments.target, tag=arguments.tag, runner_identity=arguments.runner_identity, directory=candidate, runtime_inventory=runtime_path, dependency_inventory=dependencies_path, output=audit_path, notices_output=candidate / 'THIRD-PARTY.txt', embedding_command=spec_path, embedding_model=model, intel_ort_report=arguments.intel_ort_report, windows_ort_report=arguments.windows_ort_report, dumpbin=arguments.dumpbin, signing_identity=None, notarization_profile=None)
    audit(audit_args)
    # All native OSes execute actual local inference; macOS additionally proves
    # dyld mapping while this exact installed client is held by frozen auditor.
    inference = run([sys.executable, wrapper, '--installed-directory', candidate, '--model', model, '--text', 'release embedding acceptance', '--model-lock', ROOT / 'release/embedding-model.json'], timeout=600)
    records = [json.loads(line) for line in inference.stdout.splitlines()]
    require(len(records) == 1 and records[0].get('type') == 'embedding-proof', 'Native real embedding receipt missing')
    embedding_path = output / 'embedding-receipt.json'; write_json(embedding_path, records[0])
    archive = output / target['archive']
    package(SimpleNamespace(manifest=arguments.manifest, workspace=arguments.workspace, target=arguments.target, tag=arguments.tag, audit_report=audit_path, directory=candidate, output=archive))
    (output / 'SHA256SUMS').write_text(sha(archive) + '  ' + archive.name + '\n', encoding='utf-8')
    audit_receipt = read_json(audit_path)
    receipt = {'schema': 1, 'state': 'passed', 'publication_allowed': True, 'target': arguments.target, 'tag': arguments.tag, 'native_identity': identity, 'archive': {'path': str(archive), 'sha256': sha(archive)}, 'native_audit': {'path': str(audit_path), 'sha256': sha(audit_path)}, 'runtime_inventory': {'path': str(runtime_path), 'sha256': sha(runtime_path)}, 'dependency_inventory': {'path': str(dependencies_path), 'sha256': sha(dependencies_path)}, 'embedding_receipt': {'path': str(embedding_path), 'sha256': sha(embedding_path)}, 'build_outputs': binaries, 'workspace_sha256': sha(arguments.workspace), 'lock_sha256': sha(arguments.workspace.parent / 'Cargo.lock'), 'embedding_model_register_sha256': sha(ROOT / 'release/embedding-model.json'), 'ort_source_notices': ort_evidence, 'pre_relocation_loader': loader, 'files': audit_receipt['files']}
    if windows_ort_binding is not None:
        receipt['windows_ort_build_receipt'] = windows_ort_binding
        receipt['windows_ort_cmake_cache'] = windows_cmake_binding
    receipt_path = output / 'native-candidate-receipt.json'; write_json(receipt_path, receipt)
    for path in [archive, audit_path, runtime_path, dependencies_path, embedding_path, receipt_path, output / 'SHA256SUMS']:
        emit({'type': 'artifact', 'path': str(path), 'sha256': sha(path)})
    emit({'type': 'result', 'command': 'native-candidate', 'state': 'passed', 'publication_allowed': True, 'target': arguments.target})


def parser():
    result = JsonArgumentParser(description=__doc__, allow_abbrev=False)
    for name in ('manifest', 'workspace', 'build-directory', 'output-directory', 'dependency-inventory', 'ort-source-archive', 'model-directory'):
        result.add_argument('--' + name, type=Path, required=True)
    for name in ('tag', 'target', 'runner-identity'):
        result.add_argument('--' + name, required=True)
    for name in ('intel-ort-report', 'windows-ort-report', 'dumpbin', 'runtime-directory', 'runtime-license-inventory'):
        result.add_argument('--' + name, type=Path)
    return result


def main(argv=None):
    try:
        build(parser().parse_args(argv))
        return 0
    except (ValueError, OSError, UnicodeError, subprocess.SubprocessError, KeyError, TypeError, AttributeError, tarfile.TarError) as error:
        emit({'type': 'error', 'command': 'native-candidate', 'state': 'blocked', 'publication_allowed': False, 'error': str(error)[:2000]})
        return 2


if __name__ == '__main__':
    sys.exit(main())
