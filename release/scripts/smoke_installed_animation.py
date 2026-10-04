#!/usr/bin/env python3
"""Bind an installed animation probe to audited bytes and its real launcher."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
from typing import Callable

import release_tool

SOURCE_FILES = (
    'release/scripts/smoke_installed_animation.py',
    'ilium/src/main.rs',
    'ilium-client/src/lib.rs',
    'ilium-client/src/animation_plugins.rs',
    'ilium-client/src/release_animation.rs',
    'ilium-client/src/execution.rs',
    'ilium-animation-js/src/bin/ilium-animation-helper.rs',
    'ilium-animation-js/src/helper.rs',
    'ilium-animation-js/src/runtime.rs',
    'ilium-platform/src/animation_sandbox.rs',
    'ilium-animation-js/src/release.rs',
    'Cargo.lock',
)
FORMATS = {
    'linux': {'archive', 'deb', 'rpm', 'appimage', 'snap', 'flatpak'},
    'macos': {'archive', 'zip', 'pkg', 'dmg'},
    'windows': {'archive', 'zip', 'msi', 'exe'},
}
MANAGED_LAUNCHERS = {'appimage', 'snap', 'flatpak'}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise release_tool.ReleaseError(message)


def sha(path: Path) -> str:
    value = hashlib.sha256()
    with path.open('rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def regular(root: Path, name: str, digest: str) -> Path:
    path = root / name
    metadata = path.lstat()
    require(path.is_file() and not path.is_symlink() and metadata.st_size > 0,
            'installed animation member is not a plain nonempty file: ' + name)
    require(sha(path) == digest, 'installed animation member differs from audit: ' + name)
    return path


def parse_probe_output(output: str) -> tuple[dict, list[dict]]:
    require(output.endswith('\n') and len(output.encode('utf-8')) <= 65536,
            'installed animation probe output is unbounded or incomplete')
    rows = []
    for line in output.splitlines():
        require(line.startswith('{'), 'installed animation probe emitted non-JSONL output')
        rows.append(json.loads(line))
    require(len(rows) == 4 and all(isinstance(row, dict) for row in rows),
            'installed animation probe must emit exactly four records')
    catalogue, beach, carpet, result = rows
    require(catalogue.get('type') == 'artifact' and catalogue.get('gate') == 'installed_catalogue'
            and catalogue.get('packages') == ['beach', 'carpet'],
            'installed catalogue did not discover both approved packages')
    require(result == {'type': 'result', 'gate': 'installed_animation', 'state': 'passed',
                       'publication_allowed': False, 'packages': ['beach', 'carpet']},
            'installed animation probe did not finish both packages')
    for row, name in ((beach, 'beach'), (carpet, 'carpet')):
        expected = next(digest for filename, digest in release_tool.APPROVED_PACKAGES.items()
                        if filename.startswith(name + '-'))
        require(row.get('type') == 'artifact' and row.get('gate') == 'installed_render'
                and row.get('package') == name and row.get('archive_sha256') == expected
                and row.get('helper_sha256') == catalogue.get('helper_sha256')
                and row.get('rendered_frames') == 2 and row.get('physical_retirement') is True
                and type(row.get('worker_threads_before')) is int
                and row.get('worker_threads_after') == row['worker_threads_before']
                and type(row.get('worker_bytes_before')) is int
                and row.get('worker_bytes_after') == row['worker_bytes_before'],
                'installed ' + name + ' did not render and physically retire')
    return catalogue, [beach, carpet]


def validate_receipt(receipt: dict, *, target: dict, tag: str, package_format: str,
                     audit_path: Path, installed_root: Path, executable_root: Path,
                     command: list[str], source_root: Path | None = None,
                     source_hashes: dict[str, str] | None = None) -> None:
    require(isinstance(receipt, dict) and set(receipt) == {
        'schema', 'state', 'publication_allowed', 'scope', 'format', 'target', 'tag',
        'version', 'installed_root', 'executable_root', 'launcher_command',
        'native_audit_sha256', 'source_files', 'installed_files', 'native_identity',
        'stdout', 'stdout_sha256', 'stderr_sha256', 'catalogue', 'renders'},
        'installed animation receipt schema differs')
    require(receipt['schema'] == 1 and receipt['state'] == 'passed'
            and receipt['publication_allowed'] is False
            and receipt['scope'] == 'installed-animation-contract'
            and receipt['format'] == package_format and receipt['target'] == target['rust_target']
            and receipt['tag'] == tag and receipt['version'] == tag[1:]
            and receipt['native_audit_sha256'] == sha(audit_path),
            'installed animation identity or native audit differs')
    require(receipt['installed_root'] == str(installed_root)
            and receipt['executable_root'] == str(executable_root)
            and receipt['launcher_command'] == command,
            'installed animation launcher or payload root differs')
    system = {'linux': 'Linux', 'macos': 'Darwin', 'windows': 'Windows'}[target['os']]
    machines = {'x86_64': {'x86_64', 'AMD64'}, 'aarch64': {'aarch64', 'arm64', 'ARM64'}}[target['arch']]
    identity = receipt['native_identity']
    require(isinstance(identity, dict) and identity.get('system') == system
            and identity.get('machine') in machines, 'installed animation did not run on target host')
    require((source_root is None) != (source_hashes is None),
            'exactly one animation source authority required')
    expected_source = ({name: sha(source_root / name) for name in SOURCE_FILES}
                       if source_root is not None else source_hashes)
    require(receipt['source_files'] == expected_source,
            'installed animation probe source changed or is missing')
    audit = release_tool.audit_receipt(audit_path, target, tag[1:], tag)
    members = (*target['executables'], *target['packages'])
    require(receipt['installed_files'] == {name: audit['files'][name] for name in members},
            'installed animation member map differs from audit')
    output = receipt['stdout']
    require(isinstance(output, str) and hashlib.sha256(output.encode('utf-8')).hexdigest()
            == receipt['stdout_sha256'] and receipt['stderr_sha256'] == hashlib.sha256(b'').hexdigest(),
            'installed animation output or diagnostics differ')
    catalogue, renders = parse_probe_output(output)
    client_name, helper_name = target['executables'][0], target['executables'][2]
    require(catalogue == receipt['catalogue'] and renders == receipt['renders']
            and catalogue.get('client_path') == str(executable_root / client_name)
            and catalogue.get('helper_path') == str(executable_root / helper_name)
            and catalogue.get('client_sha256') == audit['files'][client_name]
            and catalogue.get('helper_sha256') == audit['files'][helper_name],
            'installed animation process or sibling helper differs from audited payload')


def smoke(arguments: argparse.Namespace, *, command: list[str] | None = None,
          environment: dict[str, str] | None = None,
          executor: Callable[[list[str], dict[str, str], int], object] | None = None) -> dict:
    workspace = arguments.workspace.resolve(strict=True)
    source_root = workspace.parent if workspace.name == 'Cargo.toml' else workspace
    installed = arguments.root.resolve(strict=True)
    require(arguments.workspace.is_absolute() and arguments.root.is_absolute()
            and arguments.output.is_absolute() and arguments.audit.is_absolute(),
            'animation qualification paths must be absolute')
    target = next((row for row in release_tool.load_targets(arguments.manifest)
                   if (row['os'], row['arch']) == (arguments.os, arguments.arch)), None)
    require(target is not None and arguments.format in FORMATS[arguments.os],
            'animation format is outside the five-target release matrix')
    executable_root = getattr(arguments, 'executable_root', None) or installed
    require(executable_root.is_absolute(), 'observed executable root must be absolute')
    if arguments.format in MANAGED_LAUNCHERS:
        require(command is not None and getattr(arguments, 'executable_root', None) is not None,
                'managed package requires its real installed launcher and observed root')
    elif command is None:
        command = [str(installed / target['executables'][0]), 'release-animation-probe']
    require(isinstance(command, list) and all(isinstance(part, str) and part for part in command)
            and command[-1] == 'release-animation-probe', 'invalid installed animation command')
    audit = release_tool.audit_receipt(arguments.audit, target, arguments.tag[1:], arguments.tag)
    members = (*target['executables'], *target['packages'])
    for name in members:
        regular(installed, name, audit['files'][name])
    require(all((source_root / name).is_file() for name in SOURCE_FILES),
            'installed animation acceptance source is incomplete')
    source_hashes = {name: sha(source_root / name) for name in SOURCE_FILES}
    require(not arguments.output.exists() and arguments.output.parent.is_dir(),
            'animation receipt output must be a new path in an owned directory')
    child_environment = dict(os.environ if environment is None else environment)
    process = (executor(command, child_environment, 300) if executor is not None else
               subprocess.run(command, env=child_environment, text=True, capture_output=True,
                              timeout=300))
    require(process.returncode == 0 and not process.stderr,
            'installed animation probe failed: ' + (process.stdout + process.stderr)[-2000:])
    catalogue, renders = parse_probe_output(process.stdout)
    require(all(sha(installed / name) == audit['files'][name] for name in members),
            'installed animation payload changed during native execution')
    require(all(sha(source_root / name) == digest for name, digest in source_hashes.items()),
            'installed animation gate source changed during execution')
    receipt = {
        'schema': 1, 'state': 'passed', 'publication_allowed': False,
        'scope': 'installed-animation-contract', 'format': arguments.format,
        'target': target['rust_target'], 'tag': arguments.tag, 'version': arguments.tag[1:],
        'installed_root': str(installed), 'executable_root': str(executable_root),
        'launcher_command': command, 'native_audit_sha256': sha(arguments.audit),
        'source_files': source_hashes,
        'installed_files': {name: audit['files'][name] for name in members},
        'native_identity': {'system': platform.system(), 'machine': platform.machine()},
        'stdout': process.stdout,
        'stdout_sha256': hashlib.sha256(process.stdout.encode('utf-8')).hexdigest(),
        'stderr_sha256': hashlib.sha256(process.stderr.encode('utf-8')).hexdigest(),
        'catalogue': catalogue, 'renders': renders,
    }
    validate_receipt(receipt, target=target, tag=arguments.tag, package_format=arguments.format,
                     audit_path=arguments.audit, installed_root=installed,
                     executable_root=executable_root, command=command, source_root=source_root)
    with arguments.output.open('x', encoding='utf-8') as output:
        json.dump(receipt, output, indent=2, sort_keys=True)
        output.write('\n')
    release_tool.emit({'type': 'artifact', 'path': str(arguments.output),
                       'sha256': sha(arguments.output), 'bytes': arguments.output.stat().st_size})
    release_tool.emit({'type': 'result', 'state': 'passed', 'scope': receipt['scope'],
                       'format': arguments.format, 'publication_allowed': False})
    return receipt


def parser() -> argparse.Namespace:
    argument = release_tool.JsonArgumentParser(description=__doc__, allow_abbrev=False)
    for name in ('workspace', 'root', 'manifest', 'audit', 'output'):
        argument.add_argument('--' + name, type=Path, required=True)
    for name in ('os', 'arch', 'tag', 'format'):
        argument.add_argument('--' + name, required=True)
    return argument.parse_args()


if __name__ == '__main__':
    try:
        smoke(parser())
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        release_tool.emit({'type': 'error', 'state': 'blocked', 'publication_allowed': False,
                           'message': str(error)[:2000]})
        raise SystemExit(1) from None
