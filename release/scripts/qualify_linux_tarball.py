#!/usr/bin/env python3
"""Nonpublishing Linux preview acceptance for a fresh, manually extracted archive.

Run inside the caller's fresh delegated systemd unit on disposable Ubuntu 22.04
with Python 3.11. This owns filesystem state only; the caller must independently
prove retirement of the unit and its cgroup after this command exits.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import stat
import subprocess
import sys
import tempfile
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/tests'))
import release_tool
import native_install as install_gate
import smoke_installed_animation as animation_gate

TAG = 'v0.1.1-linux.1'
TARGET = 'x86_64-unknown-linux-gnu'
PROFILE = 'linux-x86-preview'
PTY_TESTS = (
    'attaching_tui_renders_the_pane_created_by_new_pane_and_responds_to_the_help_keystroke',
    'right_click_restart_reloads_only_the_client_and_preserves_the_server',
)
# Bind the executed Python helpers, their policy inputs and the PTY/animation
# contracts to Git blobs, including this entry point rather than just HEAD.
SOURCE_FILES = tuple(sorted({
    'release/scripts/qualify_linux_tarball.py', 'release/scripts/release_tool.py',
    'release/scripts/audit_native.py', 'release/scripts/smoke_installed_animation.py',
    'release/tests/native_install.py', 'release/tests/embedding_acceptance.py',
    'ilium/tests/pty_tui_smoke.rs', 'release/targets.toml',
    'release/embedding-model.json', 'release/ort-source.json', 'release/ort-runtime.json',
    'Cargo.toml', 'Cargo.lock', *animation_gate.SOURCE_FILES,
}))
ISOLATED_KEYS = {
    'HOME', 'USERPROFILE', 'LOCALAPPDATA', 'APPDATA', 'XDG_DATA_HOME',
    'XDG_CONFIG_HOME', 'XDG_RUNTIME_DIR', 'XDG_BIN_HOME', 'TERM', 'COLORTERM',
    'ILIUM_CONFIG_DIR', 'ILIUM_AGENT_SETUP_HOME', 'ILIUM_DEBUG_LOG_DIR', 'HF_HUB_OFFLINE',
}
require = install_gate.require
sha = install_gate.sha


def plain(path: Path) -> Path:
    require(path.is_file() and not path.is_symlink(), 'Input is not a plain file: ' + str(path))
    return path


def hash_map(value: object, label: str, *, nested: bool = False) -> dict[str, str]:
    require(isinstance(value, dict) and bool(value), label + ' inventory is missing')
    for name, digest in value.items():
        require(isinstance(name, str) and isinstance(digest, str)
                and re.fullmatch(r'[0-9a-f]{64}', digest), label + ' hash is invalid')
        parts = name.split('/') if nested else [name]
        for part in parts:
            release_tool.safe_member_name(part)
    return value


def source_snapshot(source: Path, commit: str) -> dict[str, str]:
    require(re.fullmatch(r'[0-9a-f]{40}', commit) is not None, 'A full frozen source commit is required')
    def git(*arguments: str) -> bytes:
        return subprocess.check_output(['git', *arguments], cwd=source, stderr=subprocess.PIPE)
    require(git('rev-parse', '--show-toplevel').decode().strip() == str(source), 'Source must be the checkout root')
    require(git('rev-parse', 'HEAD').decode().strip() == commit, 'Frozen source commit changed')
    require(not git('status', '--porcelain', '--untracked-files=no').strip(), 'Tracked frozen source is dirty')
    # Reject imported copies from another checkout even if a same-named source
    # file happens to be present in the requested checkout.
    for module, name in ((release_tool, 'release/scripts/release_tool.py'),
                         (install_gate, 'release/tests/native_install.py'),
                         (animation_gate, 'release/scripts/smoke_installed_animation.py')):
        require(Path(module.__file__).resolve() == source / name, 'Execution helper came from another checkout: ' + name)
    require(Path(__file__).resolve() == source / 'release/scripts/qualify_linux_tarball.py', 'Entry point came from another checkout')
    result = {}
    for name in SOURCE_FILES:
        expected = hashlib.sha256(git('show', commit + ':' + name)).hexdigest()
        require(sha(plain(source / name)) == expected, 'Frozen execution source differs: ' + name)
        result[name] = expected
    require(tuple(install_gate.TESTS) == PTY_TESTS, 'The two mandatory installed PTY tests changed')
    return result


def baseline_identity(unit: str) -> dict:
    require(sys.version_info[:2] == (3, 11), 'Direct tarball acceptance requires Python 3.11')
    identity = platform.freedesktop_os_release()
    require(platform.system() == 'Linux' and platform.machine() == 'x86_64'
            and identity.get('ID') == 'ubuntu' and identity.get('VERSION_ID') == '22.04',
            'Direct tarball acceptance requires disposable Ubuntu 22.04 x86_64')
    require(os.geteuid() != 0, 'Installed acceptance must run as the delegated unprivileged account')
    require(re.fullmatch(r'ilium-preview-direct-[0-9a-f]{32}\.service', unit) is not None,
            'A fresh direct-tarball unit identity is required')
    rows = [line.split() for line in Path('/proc/self/mountinfo').read_text().splitlines()]
    mounts = [fields for fields in rows if len(fields) > 6 and fields[4] == '/sys/fs/cgroup']
    require(len(mounts) == 1, 'Unified cgroup mount is missing or ambiguous')
    fields = mounts[0]
    separator = fields.index('-')
    require(fields[3] == '/' and 'rw' in fields[5].split(',')
            and len(fields) > separator + 3 and fields[separator + 1] == 'cgroup2'
            and 'rw' in fields[separator + 3].split(','), 'Writable unified cgroup root is required')
    membership = Path('/proc/self/cgroup').read_text().splitlines()
    relative = '/system.slice/' + unit
    require(membership == ['0::' + relative + '/leaf'], 'Caller is outside its exact owned delegated leaf')
    parent = Path('/sys/fs/cgroup' + relative)
    leaf = parent / 'leaf'
    controls = {'cpu', 'memory', 'pids'}
    require(parent.stat().st_uid == os.geteuid()
            and os.access(parent / 'cgroup.subtree_control', os.W_OK)
            and controls <= set((parent / 'cgroup.subtree_control').read_text().split())
            and controls <= set((leaf / 'cgroup.controllers').read_text().split())
            and str(os.getpid()) in (leaf / 'cgroup.procs').read_text().split()
            and not (parent / 'cgroup.procs').read_text().strip(), 'Owned cgroup is not delegated to this caller')
    query = subprocess.run(['/usr/bin/systemctl', 'show', unit, '--property=LoadState',
                            '--property=ActiveState', '--property=ControlGroup',
                            '--property=Delegate', '--property=Transient', '--property=KillMode'],
                           capture_output=True, text=True, timeout=30,
                           env={'PATH': os.defpath, 'LANG': 'C', 'LC_ALL': 'C'})
    values = dict(line.split('=', 1) for line in query.stdout.splitlines() if '=' in line)
    require(query.returncode == 0 and values == {
        'LoadState': 'loaded', 'ActiveState': 'active', 'ControlGroup': relative,
        'Delegate': 'yes', 'Transient': 'yes', 'KillMode': 'control-group',
    }, 'Owned systemd process boundary differs from the required delegated unit')
    require(Path('/usr/bin/bwrap').is_file() and os.access('/usr/bin/bwrap', os.X_OK),
            'Bubblewrap is required for real installed animation acceptance')
    return {'system': platform.system(), 'machine': platform.machine(),
            'os_release': identity, 'python': platform.python_version(),
            'unit': unit, 'cgroup': str(parent), 'leaf': str(leaf), 'properties': values}


def bound_reference(native: Path, reference: object, name: str) -> str:
    require(isinstance(reference, dict) and set(reference) == {'path', 'sha256'}
            and isinstance(reference['path'], str) and Path(reference['path']).is_absolute()
            and Path(reference['path']).name == name
            and isinstance(reference['sha256'], str)
            and re.fullmatch(r'[0-9a-f]{64}', reference['sha256']), 'Native bridge reference differs: ' + name)
    # Producer paths describe the original build host. Never execute or read
    # those paths; only the downloaded, explicitly selected artifact is used.
    require(sha(plain(native / name)) == reference['sha256'], 'Native input bytes differ: ' + name)
    return reference['sha256']


def validate_inputs(arguments: argparse.Namespace, source: Path, native: Path) -> SimpleNamespace:
    require(arguments.tag == TAG, 'This gate accepts only the first Linux preview tag')
    for value in (arguments.expected_bridge_sha256, arguments.expected_harness_sha256):
        require(isinstance(value, str) and re.fullmatch(r'[0-9a-f]{64}', value), 'Expected receipt SHA-256 is required')
    bridge_path, harness_path = native / 'native-candidate-receipt.json', native / 'native-test-harness.json'
    require(sha(plain(bridge_path)) == arguments.expected_bridge_sha256, 'Native bridge receipt differs from download binding')
    require(sha(plain(harness_path)) == arguments.expected_harness_sha256, 'Native harness receipt differs from download binding')
    target = release_tool.selected_target(source / 'release/targets.toml', TARGET)
    version = release_tool.workspace_version(source / 'Cargo.toml', arguments.tag)
    audit_path = plain(native / 'native-audit.json')
    audit = release_tool.audit_receipt(audit_path, target, version, arguments.tag)
    bridge, harness = release_tool.read_json(bridge_path), release_tool.read_json(harness_path)
    require(bridge.get('schema') == 1 and bridge.get('state') == 'passed'
            and bridge.get('publication_allowed') is True
            and bridge.get('qualification_profile') == PROFILE
            and bridge.get('target') == TARGET and bridge.get('tag') == arguments.tag
            and bridge.get('workspace_sha256') == sha(source / 'Cargo.toml')
            and bridge.get('lock_sha256') == sha(source / 'Cargo.lock')
            and bridge.get('embedding_model_register_sha256') == sha(source / 'release/embedding-model.json')
            and bridge.get('native_identity') == audit['native_identity']
            and bridge['native_identity'].get('runner') == target['runner']
            and bridge.get('files') == audit['files'], 'Native bridge identity or source bindings differ')
    require(bridge.get('official_packages') == {name: release_tool.APPROVED_PACKAGES[name] for name in target['packages']},
            'Native official animation inventory differs')
    outputs = bridge.get('build_outputs')
    require(isinstance(outputs, dict) and set(outputs) == set(target['executables'])
            and all(isinstance(outputs[name], dict) and outputs[name].get('sha256') == audit['files'][name]
                    for name in target['executables']), 'Native executable build bindings differ')
    references = {'archive': target['archive'], 'native_audit': 'native-audit.json',
                  'runtime_inventory': 'runtime-inventory.json', 'dependency_inventory': 'dependency-inventory.json',
                  'embedding_receipt': 'embedding-receipt.json'}
    for key, name in references.items():
        bound_reference(native, bridge.get(key), name)
    runtime = release_tool.read_json(native / 'runtime-inventory.json')
    require(runtime.get('schema') == 1 and runtime.get('state') == 'reviewed'
            and runtime.get('publication_allowed') is True and runtime.get('target') == TARGET
            and isinstance(runtime.get('files'), list) and bool(runtime['files']), 'Native runtime identity differs')
    runtime_files = {}
    for item in runtime['files']:
        require(isinstance(item, dict) and isinstance(item.get('name'), str), 'Native runtime member is malformed')
        name = item['name']
        release_tool.safe_member_name(name)
        require(name not in runtime_files and item.get('reviewed') is True
                and item.get('sha256') == audit['files'].get(name), 'Native runtime member differs from audit')
        runtime_files[name] = item['sha256']
    fixed = {*target['executables'], *target['packages'], 'VERSION', 'THIRD-PARTY.txt'}
    require(set(runtime_files) == set(audit['files']) - fixed, 'Runtime inventory omits or adds audited members')
    require(set(harness) == {'schema', 'target', 'tag', 'version', 'filename', 'path', 'sha256',
                             'runtime_files', 'source_commit', 'evidence_files'}
            and harness['schema'] == 1 and harness['target'] == TARGET
            and harness['tag'] == arguments.tag and harness['version'] == version
            and harness['source_commit'] == arguments.commit
            and harness['filename'] == 'native-test-binary'
            and harness['path'] == 'evidence/harness/native-test-binary'
            and harness['runtime_files'] == runtime_files, 'Native PTY harness identity differs')
    hash_map({'native-test-binary': harness['sha256']}, 'Native harness')
    require(sha(plain(native / 'native-test-binary')) == harness['sha256']
            and sha(plain(native / harness['path'])) == harness['sha256'], 'Retained PTY harness bytes differ')
    evidence = {'evidence/' + name: digest for name, digest in install_gate.evidence_inventory(native / 'evidence').items()}
    require(evidence == hash_map(harness['evidence_files'], 'Native evidence', nested=True), 'Exact retained native evidence inventory differs')
    model_register = source / 'release/embedding-model.json'
    model_files = release_tool.read_json(model_register)['files']
    expected_harness = {'native-test-binary': harness['sha256'], **runtime_files}
    require(install_gate.tree_inventory(native / 'evidence/harness') == expected_harness,
            'Retained harness contains extra or missing runtime members')
    require(install_gate.tree_inventory(native / 'evidence/model') == model_files,
            'Retained model contains extra or missing members')
    specification = release_tool.read_json(plain(native / 'embedding-command.json'))
    embedding_arguments = SimpleNamespace(
        embedding_wrapper=source / 'release/tests/embedding_acceptance.py',
        embedding_command=native / 'embedding-command.json',
        embedding_model=native / 'evidence/model/model.onnx', embedding_model_register=model_register,
        expected_embedding_wrapper_sha256=sha(source / 'release/tests/embedding_acceptance.py'),
        expected_embedding_command_sha256=sha(native / 'embedding-command.json'),
        expected_embedding_model_register_sha256=bridge['embedding_model_register_sha256'],
        expected_embedding_model_files=json.dumps(model_files),
        expected_embedding_runtime_files=json.dumps(runtime_files),
    )
    require(specification.get('sha256') == embedding_arguments.expected_embedding_wrapper_sha256,
            'Embedding command selects another wrapper')
    embedding_binding = install_gate.validate_embedding_inputs(embedding_arguments)
    native_embedding = release_tool.read_json(native / 'embedding-receipt.json')
    require(native_embedding.get('type') == 'embedding-proof'
            and native_embedding.get('binary_sha256') == audit['files']['ilium']
            and native_embedding.get('model_sha256') == model_files['model.onnx'], 'Native embedding model/client bindings differ')
    archive = native / target['archive']
    content = release_tool.read_archive(archive, target, audit)
    release_tool.verify_content(content, audit, version)
    checksums = plain(native / 'SHA256SUMS')
    expected_checksums = (sha(archive) + '  ' + target['archive'] + '\n').encode('ascii')
    # The native artifact has one archive. Require its exact producer format,
    # including the final newline, rather than adopting a partial inventory.
    require(checksums.stat().st_size == len(expected_checksums)
            and checksums.read_bytes() == expected_checksums, 'Native checksum inventory differs')
    return SimpleNamespace(target=target, version=version, audit=audit, audit_path=audit_path,
                           harness=harness, runtime_files=runtime_files, content=content,
                           embedding_arguments=embedding_arguments, embedding_binding=embedding_binding)


def verify_installed(pair: Path, inputs: SimpleNamespace) -> None:
    require(install_gate.tree_inventory(pair) == inputs.audit['files']
            and {path.name for path in pair.iterdir()} == set(inputs.audit['files']), 'Installed member inventory or bytes changed')
    for name in inputs.audit['files']:
        metadata = (pair / name).lstat()
        mode = 0o755 if name in inputs.target['executables'] else 0o644
        require(stat.S_IMODE(metadata.st_mode) == mode and metadata.st_nlink == 1,
                'Installed member mode or ownership links changed: ' + name)


def launcher_snapshot() -> dict:
    # These launchers are never invoked. Detect an unexpected mutation without
    # adopting, replacing or reinstalling any developer/user launcher.
    result = {}
    for name in ('claude', 'codex'):
        path = Path.home() / '.local/bin' / name
        if not path.exists() and not path.is_symlink():
            result[str(path)] = None
        else:
            result[str(path)] = {'link': os.readlink(path) if path.is_symlink() else None,
                                 'resolved': str(path.resolve()),
                                 'sha256': sha(path) if path.is_file() else None}
    return result


def isolated_environment(state: Path, pair: Path) -> dict[str, str]:
    environment = {name: value for name, value in install_gate.isolated_environment(state).items() if name in ISOLATED_KEYS}
    environment.update(PATH=os.defpath, LANG='C.UTF-8', LC_ALL='C.UTF-8', SHELL='/bin/sh',
                       TMPDIR=str(state), ILIUM_PTY_SMOKE_BINARY=str(pair / 'ilium'), PYTHONDONTWRITEBYTECODE='1')
    return environment


def execute_acceptance(source: Path, output: Path, state: Path, pair: Path,
                       inputs: SimpleNamespace, tag: str) -> dict:
    environment = isolated_environment(state, pair)
    versions = {}
    for name in inputs.target['executables']:
        process = subprocess.run([str(pair / name), '--version'], env=environment,
                                 capture_output=True, text=True, timeout=120)
        (output / (name + '-version.stdout.txt')).write_text(process.stdout, encoding='utf-8')
        (output / (name + '-version.stderr.txt')).write_text(process.stderr, encoding='utf-8')
        require(process.returncode == 0 and not process.stderr, 'Installed version probe failed: ' + name)
        versions[name] = release_tool.version_identity(name, process.stdout, inputs.version)
    animation = animation_gate.smoke(SimpleNamespace(
        workspace=source / 'Cargo.toml', root=pair, manifest=source / 'release/targets.toml',
        audit=inputs.audit_path, output=output / 'installed-animation.json',
        os='linux', arch='x86_64', tag=tag, format='archive'), environment=environment)
    embedding = install_gate.run_installed_embedding(inputs.embedding_arguments, inputs.target, pair,
                                                    inputs.embedding_binding, output, environment)
    require(embedding['executable_path'] == str(pair / 'ilium')
            and type(embedding['process_id']) is int and embedding['process_id'] > 0,
            'Installed embedding executable/process identity differs')
    require(embedding.get('loaded_runtime')
            and inputs.runtime_files.get(Path(embedding['loaded_runtime']).name) == embedding['runtime_sha256'],
            'Installed embedding did not identify the audited shipped runtime')
    harness_root = state / 'harness'
    shutil.copytree(inputs.embedding_arguments.embedding_model.parents[1] / 'harness', harness_root)
    expected_harness = {'native-test-binary': inputs.harness['sha256'], **inputs.runtime_files}
    require(install_gate.tree_inventory(harness_root) == expected_harness, 'Copied PTY harness/runtime bytes differ')
    for path in harness_root.iterdir():
        path.chmod(0o700 if path.name == 'native-test-binary' else 0o644)
    tests = []
    for name in PTY_TESTS:
        command = [str(harness_root / 'native-test-binary'), name, '--exact', '--nocapture', '--test-threads=1']
        process = subprocess.run(command, cwd=source, env=environment, capture_output=True, text=True, timeout=900)
        (output / (name + '.stdout.txt')).write_text(process.stdout, encoding='utf-8')
        (output / (name + '.stderr.txt')).write_text(process.stderr, encoding='utf-8')
        install_gate.verify_pty_result(process, name)
        tests.append({'name': name, 'command': command, 'exit_code': process.returncode})
    require(install_gate.tree_inventory(harness_root) == expected_harness, 'PTY harness/runtime changed during execution')
    return {'versions': versions, 'installed_animation': animation,
            'installed_embedding': embedding, 'pty_tests': tests}


def qualify(arguments: argparse.Namespace) -> dict:
    source, native, output = arguments.source, arguments.native_directory, arguments.output
    require(all(path.is_absolute() for path in (source, native, output)), 'Acceptance paths must be absolute')
    require(source.is_dir() and not source.is_symlink() and native.is_dir() and not native.is_symlink(),
            'Source and native inputs must be plain directories')
    source, native = source.resolve(strict=True), native.resolve(strict=True)
    require(output.parent.is_dir() and not output.exists() and not output.is_symlink(), 'Output must be a new owned path')
    output = output.parent.resolve(strict=True) / output.name
    require(not any(output.is_relative_to(root) or root.is_relative_to(output) for root in (source, native)),
            'Output must not overlap source or native inputs')
    baseline = baseline_identity(arguments.unit)
    sources = source_snapshot(source, arguments.commit)
    native_files = install_gate.evidence_inventory(native)
    inputs = validate_inputs(arguments, source, native)
    launchers = launcher_snapshot()
    output.mkdir(mode=0o700)
    # Retain preflight context even if extraction fails before a native command
    # produces stdout, so failure evidence has an independently hashable file.
    install_gate.retain_json(output, 'input-bindings.json', {
        'commit': arguments.commit, 'tag': arguments.tag, 'baseline': baseline,
        'source_files': sources, 'native_bridge_sha256': arguments.expected_bridge_sha256,
        'native_harness_sha256': arguments.expected_harness_sha256,
    })
    errors, results = [], {}
    state = pair = None
    temporary = None
    try:
        temporary = tempfile.TemporaryDirectory(prefix='state-', dir=output)
        state = Path(temporary.name)
        pair = state / 'install'
        pair.mkdir(mode=0o700)
        for name, data in inputs.content.items():
            (pair / name).write_bytes(data)
            (pair / name).chmod(0o755 if name in inputs.target['executables'] else 0o644)
        verify_installed(pair, inputs)
        results = execute_acceptance(source, output, state, pair, inputs, arguments.tag)
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        errors.append(str(error)[-2000:])
    finally:
        # Recheck even a failed native execution before removing its private
        # prefix, and retain independent failures rather than masking one.
        checks = [('installed payload', lambda: verify_installed(pair, inputs) if pair is not None else None),
                  ('frozen execution source', lambda: require(source_snapshot(source, arguments.commit) == sources, 'Execution source changed')),
                  ('native inputs', lambda: require(install_gate.evidence_inventory(native) == native_files, 'Native input inventory or bytes changed')),
                  ('user launchers', lambda: require(launcher_snapshot() == launchers, 'Real CLI launcher state changed'))]
        for label, check in checks:
            try:
                check()
            except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
                errors.append(label + ': ' + str(error)[-2000:])
        if temporary is not None:
            try:
                temporary.cleanup()
            except OSError as error:
                errors.append('filesystem cleanup: ' + str(error)[-2000:])
    cleaned = state is None or not state.exists()
    if not cleaned:
        errors.append('Owned filesystem state survived cleanup')
    receipt = {'schema': 1, 'scope': 'manual-direct-tarball-acceptance',
               'state': 'failed' if errors else 'passed', 'publication_allowed': False,
               'tag': arguments.tag, 'version': inputs.version, 'commit': arguments.commit,
               'target': TARGET, 'qualification_profile': PROFILE, 'baseline': baseline,
               'source_files': sources, 'native_input_files': native_files,
               'native_bridge_sha256': arguments.expected_bridge_sha256,
               'native_harness_sha256': arguments.expected_harness_sha256,
               'installed_files': inputs.audit['files'], **results, 'errors': errors,
               'filesystem_state_cleaned': cleaned, 'process_cleanup_verified': False,
               'outer_process_cleanup_required': True,
               'evidence_files': install_gate.evidence_inventory(output)}
    receipt_path = output / 'direct-tarball.json'
    with receipt_path.open('x', encoding='utf-8') as destination:
        json.dump(receipt, destination, indent=2, sort_keys=True, allow_nan=False)
        destination.write('\n')
    release_tool.emit({'type': 'result', 'state': receipt['state'], 'scope': receipt['scope'],
                       'publication_allowed': False, 'receipt': str(receipt_path), 'sha256': sha(receipt_path),
                       'outer_process_cleanup_required': True})
    return receipt


def parser(argv: list[str] | None = None) -> argparse.Namespace:
    argument = release_tool.JsonArgumentParser(description=__doc__, allow_abbrev=False)
    for name in ('source', 'native-directory', 'output'):
        argument.add_argument('--' + name, type=Path, required=True)
    for name in ('commit', 'tag', 'expected-bridge-sha256', 'expected-harness-sha256', 'unit'):
        argument.add_argument('--' + name, required=True)
    return argument.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    try:
        return int(qualify(parser(argv))['state'] != 'passed')
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        release_tool.emit({'type': 'error', 'state': 'failed', 'publication_allowed': False,
                           'message': str(error)[-2000:], 'outer_process_cleanup_required': True})
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
