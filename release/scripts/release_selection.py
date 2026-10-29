"""Offline target-selection intent; this is never native qualification or permission to publish."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tomllib
from typing import cast

import release_tool
import build_linux_packages
import build_macos_packages
import build_windows_installers
import smoke_macos_packages

intent_fields = frozenset(('schema', 'kind', 'state', 'publication_allowed', 'tag', 'commit', 'target_manifest_sha256', 'mode', 'selected_targets', 'omitted_targets', 'omitted_assets'))
tag_pattern = r'v[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?'

def require(condition: bool, message: str) -> None:
    if condition:
        return
    raise release_tool.ReleaseError(message)

def regular_input(path: Path) -> os.stat_result:
    attributes = path.lstat()
    reparse = getattr(attributes, 'st_file_attributes', 0) & getattr(stat, 'FILE_ATTRIBUTE_REPARSE_POINT', 0x400)
    require(stat.S_ISREG(attributes.st_mode) and not reparse, 'selection input must be a regular nonredirected file')
    require(attributes.st_size <= 1_048_576, 'selection input exceeds size bound')
    return attributes

def input_bytes(path: Path) -> bytes:
    attributes = regular_input(path)
    flags = os.O_RDONLY | getattr(os, 'O_BINARY', 0) | getattr(os, 'O_NOFOLLOW', 0) | getattr(os, 'O_NONBLOCK', 0)
    descriptor = os.open(path, flags)
    with os.fdopen(descriptor, 'rb') as source:
        opened = os.fstat(source.fileno())
        require(stat.S_ISREG(opened.st_mode) and os.path.samestat(attributes, opened), 'selection input changed before opening')
        current = path.lstat()
        reparse = getattr(current, 'st_file_attributes', 0) & getattr(stat, 'FILE_ATTRIBUTE_REPARSE_POINT', 0x400)
        require(stat.S_ISREG(current.st_mode) and not reparse and os.path.samestat(opened, current), 'selection input changed before opening')
        content = source.read(1_048_577)
    require(len(content) <= 1_048_576, 'selection input exceeds size bound')
    return content

def require_committed_policy(root: Path, path: Path, commit: str) -> bytes:
    """Return policy bytes only when they match the named checkout commit."""
    require(re.fullmatch(r'[0-9a-f]{40}', commit) is not None, 'policy commit is invalid')
    root = root.resolve()
    path = path.absolute()
    try:
        relative = path.relative_to(root)
    except ValueError as error:
        raise release_tool.ReleaseError('selection policy is outside checkout') from error
    require(bool(relative.parts) and '..' not in relative.parts, 'selection policy path is invalid')
    current = root
    for part in relative.parts[:-1]:
        current /= part
        require(current.is_dir() and not current.is_symlink(), 'selection policy parent is redirected')
    content = input_bytes(path)
    result = subprocess.run(['git', '-C', str(root), 'show', commit + ':' + relative.as_posix()],
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    require(result.returncode == 0 and result.stdout == content, 'selection policy differs from commit')
    return content

def load_catalog(manifest: Path) -> tuple[list[dict[str, object]], str]:
    original = input_bytes(manifest)
    targets = release_tool.validate_targets_document(tomllib.loads(original.decode('utf-8')))
    return targets, hashlib.sha256(original).hexdigest()

def catalog_assets(targets: list[dict[str, object]], selected: set[str]) -> dict[str, str]:
    assets = {name: 'shared' for name in ('SHA256SUMS', 'VERSION', 'install.sh', 'install.ps1', 'candidate.json', 'qualification.json')}
    for row in targets:
        owner = cast(str, row['rust_target'])
        if owner not in selected:
            continue
        architecture = cast(str, row['arch'])
        names = [cast(str, row['archive'])]
        if row['os'] == 'linux':
            names.extend((*build_linux_packages.package_names(architecture), build_linux_packages.receipt_name(architecture)))
        elif row['os'] == 'windows':
            names.extend((*build_windows_installers.INSTALLER_NAMES, build_windows_installers.RECEIPT_NAME))
        elif row['os'] == 'macos':
            names.extend((*build_macos_packages.package_names(architecture), build_macos_packages.receipt_name(architecture), build_macos_packages.smoke_receipt_name(architecture), *smoke_macos_packages.retained_evidence_names(architecture)))
        for name in names:
            require(name.casefold() not in {existing.casefold() for existing in assets}, 'planned asset names collide')
            assets[name] = owner
    return assets

def validate_intent(value: object, *, manifest: Path, expected_tag: str, expected_commit: str) -> dict[str, object]:
    require(isinstance(expected_tag, str) and re.fullmatch(tag_pattern, expected_tag) is not None, 'expected selection tag is invalid')
    require(isinstance(expected_commit, str) and re.fullmatch(r'[0-9a-f]{40}', expected_commit) is not None, 'expected selection commit is invalid')
    targets, manifest_digest = load_catalog(manifest)
    require(isinstance(value, dict) and set(value) == intent_fields, 'selection intent fields differ')
    document = cast(dict[str, object], value)
    require(type(document['schema']) is int and document['schema'] == 1, 'selection intent schema must be 1')
    require(document['kind'] == 'target-selection-intent' and document['state'] == 'planned' and document['publication_allowed'] is False, 'selection intent cannot claim qualification or publication authority')
    require(document['tag'] == expected_tag and document['commit'] == expected_commit, 'selection intent source identity differs')
    require(document['target_manifest_sha256'] == manifest_digest, 'selection intent target manifest bytes differ')
    require(document['mode'] in ('full', 'partial'), 'selection intent mode is unknown')
    selected_value = document['selected_targets']
    require(isinstance(selected_value, list) and all(isinstance(name, str) for name in selected_value), 'selected targets must be an array of exact names')
    selected = cast(list[str], selected_value)
    target_rows = {cast(str, row['rust_target']): row for row in targets}
    require(len(selected) == len(set(selected)), 'selected targets contain duplicates')
    require(set(selected) <= set(target_rows), 'selected targets contain unknown or differently cased names')
    omitted_value = document['omitted_targets']
    require(isinstance(omitted_value, dict), 'omitted targets must be an object')
    omitted = cast(dict[str, object], omitted_value)
    require(set(omitted) == set(target_rows) - set(selected), 'omitted targets must be the exact unselected complement')
    for reason in omitted.values():
        require(isinstance(reason, str) and 0 < len(reason) <= 2_000 and reason == reason.strip() and reason.isprintable(), 'omission reason must be nonblank bounded single-line text')
    omitted_assets_value = document['omitted_assets']
    require(isinstance(omitted_assets_value, dict), 'omitted assets must be an object')
    omitted_assets = cast(dict[str, object], omitted_assets_value)
    available = catalog_assets(targets, set(selected))
    mandatory = {row['archive'] for row in targets if row['rust_target'] in selected}
    require(set(omitted_assets) <= set(available) - mandatory - {name for name, owner in available.items() if owner == 'shared'}, 'asset omissions must name optional assets of selected targets')
    for reason in omitted_assets.values():
        require(isinstance(reason, str) and 0 < len(reason) <= 2_000 and reason == reason.strip() and reason.isprintable(), 'omission reason must be nonblank bounded single-line text')
    if document['mode'] == 'full':
        require(set(selected) == set(target_rows) and not omitted and not omitted_assets, 'full selection must contain every approved target and asset')
    else:
        require(bool(selected) and bool(omitted or omitted_assets), 'partial selection must omit a target or asset')
        selected_systems = {target_rows[name]['os'] for name in selected}
        require('linux' in selected_systems, 'partial intent must select a Linux target; qualification is still required')
    result = dict(document)
    result['selected_targets'] = sorted(selected)
    result['omitted_targets'] = {name: omitted[name] for name in sorted(omitted)}
    result['omitted_assets'] = {name: omitted_assets[name] for name in sorted(omitted_assets)}
    return result

def resolve_intent(*, manifest: Path, expected_tag: str, expected_commit: str, intent_path: Path | None = None, policy_path: Path | None = None, policy_content: bytes | None = None) -> dict[str, object]:
    require(sum(value is not None for value in (intent_path, policy_path, policy_content)) <= 1,
            'selection intent and policy are mutually exclusive')
    if intent_path is not None:
        value: object = release_tool.parse_json(input_bytes(intent_path).decode('utf-8'))
    elif policy_path is not None or policy_content is not None:
        raw_policy = input_bytes(policy_path) if policy_path is not None else policy_content
        require(isinstance(raw_policy, bytes) and len(raw_policy) <= 1_048_576, 'selection policy bytes are invalid')
        policy = release_tool.parse_json(raw_policy.decode('utf-8'))
        require(isinstance(policy, dict) and set(policy) == intent_fields - {'tag', 'commit', 'target_manifest_sha256'}, 'selection policy fields differ')
        require(policy['kind'] == 'target-selection-policy', 'selection policy kind differs')
        _, manifest_digest = load_catalog(manifest)
        value = {**policy, 'kind': 'target-selection-intent', 'tag': expected_tag,
                 'commit': expected_commit, 'target_manifest_sha256': manifest_digest}
    else:
        targets, manifest_digest = load_catalog(manifest)
        value = {'schema': 1, 'kind': 'target-selection-intent', 'state': 'planned', 'publication_allowed': False, 'tag': expected_tag, 'commit': expected_commit, 'target_manifest_sha256': manifest_digest, 'mode': 'full', 'selected_targets': [row['rust_target'] for row in targets], 'omitted_targets': {}, 'omitted_assets': {}}
    return validate_intent(value, manifest=manifest, expected_tag=expected_tag, expected_commit=expected_commit)

def canonical_bytes(intent: dict[str, object]) -> bytes:
    return (json.dumps(intent, sort_keys=True, separators=(',', ':'), ensure_ascii=True, allow_nan=False) + '\n').encode('ascii')

def availability_bytes(value: object, checksums: bytes, *, manifest: Path, expected_tag: str, expected_commit: str) -> bytes:
    """Bind installer availability to a validated intent and exact checksum bytes.

    This serialization is not qualification: callers must separately verify
    each archive against native evidence and bind this file into publication.
    """
    intent = validate_intent(value, manifest=manifest, expected_tag=expected_tag, expected_commit=expected_commit)
    targets, digest = load_catalog(manifest)
    require(digest == intent['target_manifest_sha256'], 'selection intent target manifest bytes differ')
    selected = set(cast(list[str], intent['selected_targets']))
    selected_archives = {cast(str, row['archive']) for row in targets if row['rust_target'] in selected}
    require(isinstance(checksums, bytes) and 0 < len(checksums) <= 16384 and checksums.endswith(b'\n'), 'malformed checksum inventory')
    names = set()
    for line in checksums.split(b'\n')[:-1]:
        match = re.fullmatch(rb'[0-9a-f]{64}  ([A-Za-z0-9._-]+)', line)
        require(match is not None, 'malformed checksum record')
        name = match.group(1).decode('ascii')
        require(name not in names, 'duplicate checksum record')
        names.add(name)
    require(names == selected_archives, 'checksum records differ from selected archives')
    lines = ['ilium-availability-v1', 'tag ' + expected_tag, 'commit ' + expected_commit,
             'selection-sha256 ' + hashlib.sha256(canonical_bytes(intent)).hexdigest(),
             'checksums-sha256 ' + hashlib.sha256(checksums).hexdigest()]
    lines.extend(('available ' if row['rust_target'] in selected else 'omitted ') + cast(str, row['archive'])
                 for row in sorted(targets, key=lambda row: cast(str, row['archive'])))
    return ('\n'.join(lines) + '\n').encode('ascii')

def planned_assets(value: object, *, manifest: Path, expected_tag: str, expected_commit: str) -> dict[str, str]:
    """Describe expected public names and owners, without asserting any asset exists."""
    intent = validate_intent(value, manifest=manifest, expected_tag=expected_tag, expected_commit=expected_commit)
    targets, digest = load_catalog(manifest)
    require(digest == intent['target_manifest_sha256'], 'selection intent target manifest bytes differ')
    assets = catalog_assets(targets, set(cast(list[str], intent['selected_targets'])))
    if intent['mode'] == 'partial':
        assets['AVAILABILITY'] = 'shared'
    omitted_assets = cast(dict[str, object], intent['omitted_assets'])
    return {name: assets[name] for name in sorted(assets) if name not in omitted_assets}

def main(argv: list[str] | None = None) -> int:
    try:
        parser = release_tool.JsonArgumentParser(description='Validate a target-selection intent; never qualifies artifacts or permits publication.')
        parser.add_argument('--manifest', type=Path, required=True)
        parser.add_argument('--tag', required=True)
        parser.add_argument('--commit', required=True)
        parser.add_argument('--intent', type=Path)
        arguments = parser.parse_args(argv)
        intent = resolve_intent(manifest=arguments.manifest, expected_tag=arguments.tag, expected_commit=arguments.commit, intent_path=arguments.intent)
        content = canonical_bytes(intent)
        assets = planned_assets(intent, manifest=arguments.manifest, expected_tag=arguments.tag, expected_commit=arguments.commit)
        release_tool.emit({'type': 'result', 'command': 'selection-intent', 'intent': intent, 'intent_sha256': hashlib.sha256(content).hexdigest(), 'planned_assets': assets})
        return 0
    except (release_tool.ReleaseError, OSError, ValueError, TypeError) as error:
        release_tool.emit({'type': 'error', 'command': 'selection-intent', 'message': str(error)})
        return 1

if __name__ == '__main__':
    sys.exit(main())
