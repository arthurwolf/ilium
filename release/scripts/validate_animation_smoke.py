#!/usr/bin/env python3
"""Seal native package animation smoke evidence for candidate readback."""
from __future__ import annotations

import argparse
import build_linux_packages as linux_packages
import build_windows_installers as windows_installers
import hashlib
import json
from pathlib import Path, PureWindowsPath
import platform

import release_tool
import smoke_installed_animation as animation

WINDOWS_NAME = 'windows-animation-smoke.json'
LINUX_NAME = 'linux-animation-smoke-{arch}.json'
SOURCE_FILES = (*animation.SOURCE_FILES,
                'release/scripts/validate_animation_smoke.py',
                'release/scripts/smoke_windows_installers.py',
                'release/scripts/smoke_linux_packages.py')
HOST_FORMATS = ('deb', 'snap', 'flatpak', 'appimage')


def require(condition, message):
    if not condition:
        raise release_tool.ReleaseError(message)


def sha(path):
    value = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def content_sha(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'),
                                     ensure_ascii=False).encode('utf-8')).hexdigest()


def source_hashes(root):
    return {name: sha(Path(root) / name) for name in SOURCE_FILES}


def bounded_jsonl(path, limit=8_000_000):
    data = Path(path).read_bytes()
    require(data.endswith(b'\n') and len(data) <= limit, 'native animation journal is incomplete or unbounded')
    return [json.loads(line) for line in data.splitlines()]


def windows_events(rows, audit, expected_inputs, source_files):
    require(rows and all(isinstance(row, dict) for row in rows) and
            rows[-1].get('type') == 'result' and rows[-1].get('state') == 'passed',
            'Windows native installer smoke has no passing terminal result')
    bindings = [row for row in rows if row.get('type') == 'binding']
    require(len(bindings) == 1 and bindings[0].get('sha256') == expected_inputs and
            bindings[0].get('tag') == audit['tag'] and
            bindings[0].get('version') == audit['version'] and
            bindings[0].get('package_files') == audit['files'] and
            bindings[0].get('source_files') == source_files and
            rows[-1].get('input_sha256') == expected_inputs and
            rows[-1].get('source_files') == source_files and
            rows[-1].get('installers') == {name: expected_inputs[name]
                                             for name in windows_installers.INSTALLER_NAMES} and
            rows[-1].get('version') == audit['version'] and
            rows[-1].get('native_exe_msi') is True and
            rows[-1].get('public_release_verified') is False,
            'Windows smoke run did not consume the sealed installer, archive and audit bytes')
    accounts = [row for row in rows if row.get('type') == 'account-before']
    require(len(accounts) == 1 and accounts[0].get('custody') in
            ('github-hosted', 'operator-attested-exclusive') and
            isinstance(accounts[0].get('identity'), dict) and
            isinstance(accounts[0]['identity'].get('local_app_data'), str) and
            PureWindowsPath(accounts[0]['identity']['local_app_data']).is_absolute(),
            'Windows native smoke lacks the installed account identity')
    installed = (PureWindowsPath(accounts[0]['identity']['local_app_data']) /
                 'Programs' / 'ilium')
    exits, proofs, formats = {}, [], set()
    for row in rows:
        kind = row.get('type')
        if kind == 'command-exit':
            exits[row.get('label')] = row
        elif kind == 'format-result' and row.get('state') == 'passed':
            formats.add(row.get('format'))
        elif kind == 'installed-animation':
            label = row.get('label')
            exit_row = exits.get(label + '-animation') if isinstance(label, str) else None
            require(isinstance(exit_row, dict) and exit_row.get('returncode') == 0 and
                    exit_row.get('stderr') == '' and row.get('job_empty') is True and
                    row.get('stdout_sha256') == hashlib.sha256(
                        exit_row.get('stdout', '').encode()).hexdigest(),
                    'Windows installed animation lacks its successful owned command')
            catalogue, renders = animation.parse_probe_output(exit_row['stdout'])
            command = row.get('command')
            require(isinstance(command, list) and len(command) == 2 and
                    command[1] == 'release-animation-probe' and
                    catalogue == row.get('catalogue') and renders == row.get('renders'),
                    'Windows animation rows or installed command differ')
            client = PureWindowsPath(command[0])
            require(client == installed / 'ilium.exe' and
                    catalogue.get('client_path') == str(client) and
                    catalogue.get('helper_path') == str(client.with_name('ilium-animation-helper.exe')) and
                    catalogue.get('client_sha256') == audit['files']['ilium.exe'] and
                    catalogue.get('helper_sha256') == audit['files']['ilium-animation-helper.exe'],
                    'Windows animation used another installed image')
            proofs.append({'label': label, 'command': command, 'stdout': exit_row['stdout'],
                           'stdout_sha256': row['stdout_sha256']})
    require({'msi', 'exe'} <= formats and len(proofs) >= 4 and
            any(proof['label'].startswith('msi-') for proof in proofs) and
            any(proof['label'].startswith('exe-') for proof in proofs),
            'Windows MSI and EXE animation transactions are incomplete')
    return proofs


def linux_container_labels():
    import smoke_linux_packages as smoke
    jobs = [('deb', image) for image in smoke.DEB_IMAGES]
    jobs += [('rpm', image) for image in smoke.RPM_IMAGES]
    jobs.append(('appimage', smoke.APPIMAGE_IMAGE))
    return {(kind + '-' + image.replace('/', '_').replace(':', '_')): (kind, image)
            for kind, image in jobs}


def linux_container_proof(proof, kind, image, audit, audit_path, source, arch,
                          package_files, archive_sha256, tag):
    package_name = linux_packages.package_name(arch, kind)
    require(proof.get('schema') == 1 and proof.get('state') == 'passed' and
            proof.get('publication_allowed') is False and
            proof.get('scope') == 'installed-animation-container' and
            proof.get('format') == kind and proof.get('image') == image and
            proof.get('arch') == arch and
            proof.get('tag') == tag and
            proof.get('native_audit_sha256') == sha(audit_path) and
            proof.get('source_archive_sha256') == archive_sha256 and
            proof.get('package') == package_name and
            proof.get('package_sha256') == package_files[package_name] and
            proof.get('source_files') == {name: source[name] for name in animation.SOURCE_FILES} and
            proof.get('installed_files') == {name: audit['files'][name]
                                             for name in ('ilium', 'ilium-server',
                                                          'ilium-animation-helper',
                                                          *release_tool.APPROVED_PACKAGES)},
            'Linux container animation provenance differs')
    output = proof.get('stdout')
    require(isinstance(output, str) and proof.get('stdout_sha256') ==
            hashlib.sha256(output.encode()).hexdigest(),
            'Linux container animation output digest differs')
    catalogue, renders = animation.parse_probe_output(output)
    require(proof.get('catalogue') == catalogue and proof.get('renders') == renders and
            proof.get('client_path') == catalogue.get('client_path') and
            proof.get('helper_path') == catalogue.get('helper_path') and
            catalogue.get('client_sha256') == audit['files']['ilium'] and
            catalogue.get('helper_sha256') == audit['files']['ilium-animation-helper'],
            'Linux container used another animation client or helper')
    client = Path(proof['client_path'])
    require(client.is_absolute() and client.name == 'ilium' and
            Path(proof['helper_path']) == client.with_name('ilium-animation-helper') and
            (client.parent == Path('/usr/lib/ilium') if kind in ('deb', 'rpm') else
             client.parent.parent.name == 'appimage'),
            'Linux container installed path differs')


def linux_events(rows, *, command, formats, target, package_files,
                 archive_sha256, audit_sha256, source, tag, proofs):
    records_per_format = 3 if command == 'host' else 1
    require(isinstance(rows, list) and len(rows) == len(formats) * records_per_format + 1 and
            all(isinstance(row, dict) for row in rows) and
            rows[-1].get('type') == 'summary' and rows[-1].get('command') == command and
            rows[-1].get('state') == 'passed' and rows[-1].get('failed') == 0 and
            rows[-1].get('arch') == target['arch'] and
            rows[-1].get('tag') == tag and
            isinstance(rows[-1].get('native_identity'), dict) and
            rows[-1]['native_identity'].get('system') == 'Linux' and
            rows[-1]['native_identity'].get('machine') in
            ({'x86_64', 'AMD64'} if target['arch'] == 'x86_64' else
             {'aarch64', 'arm64', 'ARM64'}),
            'Linux package smoke lacks a complete passing native group summary')
    terminal_rows = rows[:-1]
    if command == 'host':
        terminal_rows = []
        for offset in range(0, len(rows) - 1, 3):
            artifact, nested_result, terminal = rows[offset:offset + 3]
            kind = terminal.get('format')
            require(kind in formats and isinstance(proofs.get(kind), dict),
                    'Linux host nested animation has no matching retained proof')
            encoded_proof = (json.dumps(proofs[kind], indent=2, sort_keys=True) + '\n').encode('utf-8')
            artifact_path = artifact.get('path')
            terminal_log = terminal.get('log')
            require(isinstance(artifact_path, str) and isinstance(terminal_log, str) and
                    Path(artifact_path).is_absolute() and Path(terminal_log).is_absolute() and
                    Path(terminal_log).name == kind + '-host.log' and
                    Path(artifact_path) == Path(terminal_log).with_name(
                        kind + '-host-installed-animation.json') and
                    artifact == {'type': 'artifact', 'path': artifact_path,
                                 'sha256': hashlib.sha256(encoded_proof).hexdigest(),
                                 'bytes': len(encoded_proof)} and
                    nested_result == {'type': 'result', 'state': 'passed',
                                      'scope': 'installed-animation-contract',
                                      'format': kind, 'publication_allowed': False},
                    'Linux host nested animation journal or proof bytes differ')
            terminal_rows.append(terminal)
    seen = set()
    for row in terminal_rows:
        require(isinstance(row, dict) and row.get('type') == 'result' and
                row.get('command') == command and row.get('state') == 'passed' and
                row.get('arch') == target['arch'] and
                row.get('tag') == tag and
                row.get('source_archive_sha256') == archive_sha256 and
                row.get('native_audit_sha256') == audit_sha256 and
                row.get('source_files') == {name: source[name]
                                            for name in animation.SOURCE_FILES},
                'Linux package smoke result identity or terminal state differs')
        kind = row.get('format')
        identity = (kind, row.get('environment')) if command == 'containers' else kind
        require(identity in formats and identity not in seen and
                row.get('package') == linux_packages.package_name(target['arch'], kind) and
                row.get('package_sha256') == package_files.get(row.get('package')),
                'Linux package smoke consumed other package bytes')
        seen.add(identity)
        if command == 'host':
            gates = row.get('gates')
            require(isinstance(gates, dict) and row.get('removed') is True and
                    row.get('animation_sha256') == content_sha(proofs[kind]) and
                    all(gates.get(name) == 'passed' for name in
                        ('preflight', 'install', 'verify', 'remove', 'absence', 'state_cleanup')) and
                    row.get('execution') == {'deb': 'native-host', 'snap': 'classic',
                                             'flatpak': 'sandbox', 'appimage': 'fuse'}[kind],
                    'Linux host package animation preceded a failed lifecycle or cleanup')
        else:
            label = kind + '-' + row['environment'].replace('/', '_').replace(':', '_')
            animation_result = row.get('animation')
            require(isinstance(animation_result, dict) and
                    animation_result.get('content_sha256') == content_sha(proofs[label]) and
                    row.get('execution') == ('extract-and-run' if kind == 'appimage'
                                             else 'native-container'),
                    'Linux container result lacks its completed animation proof')
    require(seen == set(formats), 'Linux package smoke format coverage differs')


def linux_host_proof(proof, kind, target, audit, source, tag):
    root = Path(proof.get('installed_root', ''))
    executable = Path(proof.get('executable_root', ''))
    command = proof.get('launcher_command')
    require(isinstance(command, list) and command[-1:] == ['release-animation-probe'],
            'Linux host animation launcher is missing')
    require(root.is_absolute() and executable.is_absolute(),
            'Linux host animation installed paths must be absolute')
    animation.validate_receipt(proof, target=target, tag=tag, package_format=kind,
                               audit_path=Path(audit), installed_root=root,
                               executable_root=executable, command=command,
                               source_hashes={name: source[name] for name in animation.SOURCE_FILES})
    require((kind == 'deb' and command == ['/usr/bin/ilium', 'release-animation-probe'] and
             root == Path('/usr/lib/ilium')) or
            (kind == 'snap' and command == ['snap', 'run', 'ilium', 'release-animation-probe'] and
             root == executable and any(str(root).startswith(prefix) for prefix in
                                        ('/snap/ilium/', '/var/lib/snapd/snap/ilium/'))) or
            (kind == 'flatpak' and Path(command[0]).is_absolute() and
             command[0].endswith('/flatpak-client.sh') and
             executable == Path('/app/lib/ilium') and root.is_absolute() and
             root.parent.name == 'lib' and root.parent.parent.name == 'files') or
            (kind == 'appimage' and Path(command[0]).is_absolute() and
             command[0].endswith('/appimage-client.sh') and
             root == executable and root.parent.name == 'appimage'),
            'Linux host proof did not use the required installed launcher')


def validate(marker, *, target, tag, audit_path, source_root, packages, archive_sha256):
    common = {'schema', 'state', 'publication_allowed', 'target', 'tag',
              'archive_sha256', 'native_audit_sha256', 'source_files',
              'package_files', 'native_identity'}
    detail = ({'journal', 'proofs'} if target['os'] == 'windows' else
              {'containers', 'host', 'container_events', 'host_events', 'appimage_fuse'})
    require(isinstance(marker, dict) and set(marker) == common | detail,
            'native package animation marker schema differs')
    require(marker.get('schema') == 1 and marker.get('state') == 'passed' and
            marker.get('publication_allowed') is False and
            marker.get('target') == target['rust_target'] and marker.get('tag') == tag and
            marker.get('archive_sha256') == archive_sha256 and
            marker.get('native_audit_sha256') == sha(audit_path) and
            marker.get('source_files') == source_hashes(source_root) and
            marker.get('native_identity') == {'system': {'linux': 'Linux', 'windows': 'Windows'}[target['os']],
                                              'machine': target['arch']},
            'native package animation marker identity differs')
    audit = release_tool.audit_receipt(audit_path, target, tag[1:], tag)
    package_files = marker.get('package_files')
    expected_names = (set(windows_installers.INSTALLER_NAMES) if target['os'] == 'windows'
                      else set(linux_packages.package_names(target['arch'])))
    require(isinstance(package_files, dict) and set(package_files) == expected_names,
            'native animation package inventory differs')
    require(all((Path(packages) / name).is_file() and
                not (Path(packages) / name).is_symlink() for name in expected_names),
            'native animation package file is missing or aliased')
    require(package_files == {name: sha(Path(packages) / name) for name in expected_names},
            'native package bytes differ from animation smoke')
    if target['os'] == 'windows':
        rows = marker.get('journal')
        require(isinstance(rows, list) and len(rows) <= 10000 and
                len(json.dumps(rows).encode()) <= 8_000_000,
                'Windows animation journal absent or unbounded')
        expected_inputs = {'archive': archive_sha256, 'audit': sha(audit_path),
                           'manifest': sha(Path(source_root) / 'release/targets.toml'),
                           'receipt': sha(Path(packages) / windows_installers.RECEIPT_NAME),
                           **package_files}
        require(marker.get('proofs') == windows_events(rows, audit, expected_inputs,
                                                       marker['source_files']),
                'Windows animation smoke journal differs')
    else:
        containers = marker.get('containers')
        host = marker.get('host')
        require(isinstance(containers, dict) and set(containers) == set(linux_container_labels()) and
                isinstance(host, dict) and set(host) == set(HOST_FORMATS),
                'Linux animation smoke format coverage differs')
        for label, (kind, image) in linux_container_labels().items():
            linux_container_proof(containers[label], kind, image, audit,
                                  audit_path,
                                  marker['source_files'], target['arch'],
                                  package_files, archive_sha256, tag)
        for kind in HOST_FORMATS:
            linux_host_proof(host[kind], kind, target, audit_path,
                             marker['source_files'], tag)
        linux_events(marker.get('container_events'), command='containers',
                     formats=tuple(linux_container_labels().values()),
                     target=target, package_files=package_files,
                     archive_sha256=archive_sha256, audit_sha256=sha(audit_path),
                     source=marker['source_files'], tag=tag, proofs=containers)
        linux_events(marker.get('host_events'), command='host', formats=HOST_FORMATS,
                     target=target, package_files=package_files,
                     archive_sha256=archive_sha256, audit_sha256=sha(audit_path),
                     source=marker['source_files'], tag=tag, proofs=host)
        mount = marker.get('appimage_fuse')
        require(isinstance(mount, dict) and Path(mount.get('fuse_mount', '')).is_absolute() and
                any(isinstance(row, list) and len(row) == 2 and row[0] == mount['fuse_mount'] and
                    row[1].startswith('fuse') for row in mount.get('mount_records', [])),
                'Linux AppImage acceptance lacks a real FUSE mount record')


def seal(arguments):
    source_root = arguments.workspace.resolve(strict=True).parent
    target = release_tool.selected_target(arguments.manifest, arguments.target)
    require(target['os'] in ('linux', 'windows'), 'only Linux/Windows package smoke is sealed here')
    audit_path = arguments.audit.resolve(strict=True)
    audit = release_tool.audit_receipt(audit_path, target, arguments.tag[1:], arguments.tag)
    require(platform.system() == {'linux': 'Linux', 'windows': 'Windows'}[target['os']] and
            platform.machine() in {'x86_64': {'x86_64', 'AMD64'},
                                   'aarch64': {'aarch64', 'arm64', 'ARM64'}}[target['arch']],
            'native package smoke seal ran on another platform')
    require(arguments.archive.name == target['archive'], 'native archive name differs')
    package_names = (windows_installers.INSTALLER_NAMES if target['os'] == 'windows'
                     else linux_packages.package_names(target['arch']))
    marker = {'schema': 1, 'state': 'passed', 'publication_allowed': False,
              'target': target['rust_target'], 'tag': arguments.tag,
              'archive_sha256': sha(arguments.archive),
              'native_audit_sha256': sha(audit_path),
              'source_files': source_hashes(source_root),
              'package_files': {name: sha(arguments.packages / name) for name in package_names},
              'native_identity': {'system': platform.system(), 'machine': target['arch']}}
    if target['os'] == 'windows':
        rows = bounded_jsonl(arguments.windows_journal)
        marker['journal'] = rows
        expected_inputs = {'archive': marker['archive_sha256'],
                           'audit': marker['native_audit_sha256'],
                           'manifest': sha(arguments.manifest),
                           'receipt': sha(arguments.packages / windows_installers.RECEIPT_NAME),
                           **marker['package_files']}
        marker['proofs'] = windows_events(rows, audit, expected_inputs,
                                          marker['source_files'])
        name = WINDOWS_NAME
    else:
        labels = linux_container_labels()
        marker['containers'] = {label: release_tool.read_json(
            arguments.container_log / (label + '-installed-animation.json'))
            for label in labels}
        marker['host'] = {kind: release_tool.read_json(
            arguments.host_log / (kind + '-host-installed-animation.json'))
            for kind in HOST_FORMATS}
        marker['container_events'] = bounded_jsonl(arguments.container_log / 'containers-results.jsonl')
        marker['host_events'] = bounded_jsonl(arguments.host_log / 'host-results.jsonl')
        fuse = [json.loads(line) for line in
                (arguments.host_log / 'appimage-host.log').read_text().splitlines()
                if line.startswith('{') and '"fuse_mount"' in line]
        require(len(fuse) == 1, 'AppImage native FUSE mount record missing')
        marker['appimage_fuse'] = fuse[0]
        name = LINUX_NAME.format(arch=target['arch'])
    validate(marker, target=target, tag=arguments.tag, audit_path=audit_path,
             source_root=source_root, packages=arguments.packages,
             archive_sha256=sha(arguments.archive))
    if target['os'] == 'linux':
        host_log_root = arguments.host_log.resolve(strict=True)
        for offset in range(0, len(marker['host_events']) - 1, 3):
            artifact, _, terminal = marker['host_events'][offset:offset + 3]
            require(Path(artifact['path']).parent == host_log_root and
                    Path(terminal['log']).parent == host_log_root,
                    'Linux host animation journal came from another smoke log')
    output = arguments.packages / name
    require(not output.exists(), 'native animation smoke marker already exists')
    with output.open('x', encoding='utf-8') as stream:
        json.dump(marker, stream, indent=2, sort_keys=True)
        stream.write('\n')
    release_tool.emit({'type': 'artifact', 'path': str(output), 'sha256': sha(output)})
    release_tool.emit({'type': 'result', 'state': 'passed', 'publication_allowed': False})


def main():
    parser = release_tool.JsonArgumentParser(description=__doc__, allow_abbrev=False)
    for name in ('workspace', 'manifest', 'audit', 'archive', 'packages',
                 'windows-journal', 'container-log', 'host-log'):
        parser.add_argument('--' + name, type=Path)
    parser.add_argument('--target', required=True)
    parser.add_argument('--tag', required=True)
    args = parser.parse_args()
    require(all(getattr(args, name) is not None for name in
                ('workspace', 'manifest', 'audit', 'archive', 'packages')),
            'native animation smoke seal inputs are incomplete')
    seal(args)


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, release_tool.ReleaseError) as error:
        release_tool.emit({'type': 'error', 'state': 'blocked', 'message': str(error)[:2000]})
        raise SystemExit(1) from None
