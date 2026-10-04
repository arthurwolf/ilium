#!/usr/bin/env python3
"""Inspect and exercise all macOS packages before issuing a lifecycle receipt."""  # Separate build from qualification.
from __future__ import annotations  # Preserve portable annotations.
import argparse  # Type CLI inputs.
from contextlib import contextmanager  # Restore hidden inputs.
import hashlib  # Hash native containers.
import json  # Retain JSONL evidence.
import math  # Reject nonfinite embedding vectors when reopening receipts.
import os  # Manage owned objects.
from pathlib import Path  # Keep paths explicit.
import plistlib  # Parse Apple observations.
import re  # Validate native names.
import selectors  # Bound Darwin protocol.
import signal  # Signal recorded identities.
import stat  # Distinguish filesystem types.
import struct  # Inspect container headers.
import subprocess  # Launch owned commands.
import sys  # Use current Python.
import time  # Bound native lifecycles.
from typing import Any, Iterator  # Type receipt boundaries.
import uuid  # Allocate unpredictable sessions.
import xml.etree.ElementTree as xml_tree  # Inspect script-free metadata.
from xml.parsers.expat import ExpatError as expat_error  # Normalize plist failures.
import zlib  # Bound decompression.
import zipfile  # Normalize malformed ZIP containers.
import audit_native  # Reuse native validators.
import build_macos_packages as packages  # Preserve B1 contract.
import release_tool  # Reuse archive policy.
import smoke_installed_animation as animation_gate  # Exercise the installed helper under each package.
#
root = Path(__file__).resolve().parents[2]  # Locate repository root.
require = packages.require  # Reuse structured failures.
max_metadata_bytes = 33_554_432  # Bound metadata inputs.
max_payload_bytes = packages.max_payload_bytes  # Preserve archive limits.
#
def strict_json(data: bytes) -> dict[str, Any]:  # Reject duplicate keys.
    def pairs(items: list[tuple[str, Any]]) -> dict[str, Any]:  # Validate object keys.
        require(len({key for key, _value in items}) == len(items), 'duplicate JSON evidence key')  # Fail ambiguous records.
        return dict(items)  # Retain unique values.
    value = json.loads(data, object_pairs_hook=pairs)  # Parse one record.
    require(isinstance(value, dict), 'JSON evidence must be an object')  # Reject nonobject evidence.
    return value  # Return validated evidence.
#
def safe_xml(data: bytes) -> xml_tree.Element:  # Bound entity-free XML.
    require(0 < len(data) <= max_metadata_bytes and b'\x00' not in data and b'<!DOCTYPE' not in data.upper() and b'<!ENTITY' not in data.upper(), 'unsafe XML metadata')  # Forbid alternate encodings.
    try:  # Normalize parsing failures.
        document = xml_tree.fromstring(data.decode('utf-8'))  # Parse admitted UTF-8 only.
    except (UnicodeDecodeError, xml_tree.ParseError) as error:  # Reject ambiguous encodings.
        raise release_tool.ReleaseError('malformed package metadata XML') from error  # Preserve structured failure.
    pending, visited = [(document, 0)], 0  # Bound iterative traversal.
    while pending:  # Avoid recursive parser validation.
        element, depth = pending.pop()  # Inspect one metadata node.
        visited += 1  # Account every admitted node.
        require(depth <= 64 and visited <= 4096, 'excessive XML depth or nodes')  # Reject recursive resource exhaustion.
        pending.extend((child, depth + 1) for child in element)  # Retain bounded traversal work.
    return document  # Retain exact attributes.
#
def expected_tree(audit: dict[str, Any], target: dict[str, Any], observed: dict[str, Any]) -> None:  # Validate payload descriptions.
    require(isinstance(observed, dict) and set(observed) == set(audit['files']), 'payload tree inventory differs')  # Reject inventory extras.
    for name, digest in audit['files'].items():  # Check audited files.
        entry = observed[name]  # Select declared metadata.
        require(isinstance(entry, dict) and set(entry) == {'kind', 'mode', 'bytes', 'sha256'}, 'payload tree entry schema differs')  # Require exact schema.
        require(entry['kind'] == 'file' and entry['sha256'] == digest and entry['mode'] == (0o755 if name in target['executables'] else 0o644), 'payload tree content/type/mode differs')  # Preserve adjacent payload.
        require(type(entry['bytes']) is int and 0 < entry['bytes'] <= packages.max_file_bytes, 'invalid payload length')  # Bound member size.
    require(sum(entry['bytes'] for entry in observed.values()) <= max_payload_bytes, 'payload tree exceeds byte limit')  # Bound installed bytes.
#
def validate_build_receipt(receipt: dict[str, Any], target: dict[str, Any], audit: dict[str, Any], binding: dict[str, Any]) -> None:  # Validate build evidence.
    fields = {'schema', 'state', 'publication_allowed', 'native_payload_executed', 'tag', 'version', 'arch', 'target', 'package_files', 'payload_tree', 'packages', 'package_bytes', 'layout', 'distribution_sha256', 'component_sha256', 'native_identity', 'toolchain', 'commands', 'payload_signing', 'payload_notarization', 'container_signing', 'container_notarization', 'credentials_used', 'owned_images_detached', 'work_retained'} | set(binding)  # Require B1 schema.
    require(set(receipt) == fields, 'build receipt has missing or unknown fields')  # Reject schema drift.
    require(receipt['schema'] == 1 and receipt['state'] == 'built-not-qualified' and receipt['publication_allowed'] is False and receipt['native_payload_executed'] is False, 'build receipt qualification state differs')  # Preserve construction state.
    require(all(receipt.get(key) == value for key, value in binding.items()), 'build receipt native/source binding differs')  # Rejoin native evidence.
    for key in ('tag', 'version', 'target', 'arch'):  # Bind package identities.
        require(receipt[key] == audit[key], 'build receipt identity differs: ' + key)  # Reject foreign identities.
    require(receipt['package_files'] == audit['files'], 'build payload hashes differ')  # Use final audited hashes.
    expected_tree(audit, target, receipt['payload_tree'])  # Validate payload inventory.
    names = set(packages.package_names(target['arch']))  # Require three containers.
    require(set(receipt['packages']) == names and set(receipt['package_bytes']) == names, 'build container inventory differs')  # Reject container extras.
    require(all(isinstance(value, str) and re.fullmatch(r'[0-9a-f]{64}', value) for value in receipt['packages'].values()), 'invalid container hash')  # Validate digest spelling.
    require(all(type(value) is int and 0 < value <= max_payload_bytes for value in receipt['package_bytes'].values()), 'invalid container size')  # Bound container sizes.
    require(receipt['layout'] == packages.package_layout(target['arch'], audit['version']), 'package layout differs')  # Preserve installation policy.
    require(receipt['distribution_sha256'] == release_tool.digest(packages.distribution_xml(receipt['layout'])), 'generated distribution hash differs')  # Bind source Distribution.
    require(isinstance(receipt['component_sha256'], str) and re.fullmatch(r'[0-9a-f]{64}', receipt['component_sha256']), 'missing component build digest')  # Bind intermediate component.
    require(receipt['payload_signing'] == audit['signing'] and receipt['payload_notarization'] == audit['notarization'], 'payload trust claims changed')  # Preserve audit distinctions.
    require(receipt['container_signing'] == dict.fromkeys(packages.formats, 'unsigned') and receipt['container_notarization'] == dict.fromkeys(packages.formats, 'disabled') and receipt['credentials_used'] is False, 'container trust claims differ')  # Prevent trust escalation.
    identity = receipt['native_identity']  # Inspect build identity.
    require(isinstance(identity, dict) and identity.get('system') == 'Darwin' and identity.get('machine') == packages.architectures[target['arch']][0] and identity.get('runner') == target['runner'] and identity.get('translated') is False, 'build was not genuinely native')  # Require native construction.
    require(receipt['owned_images_detached'] is True and receipt['work_retained'] is True, 'build cleanup/retention state differs')  # Require completed build.
    require(isinstance(receipt['commands'], list) and 0 < len(receipt['commands']) <= packages.max_commands and isinstance(receipt['toolchain'], dict), 'missing bounded native build evidence')  # Retain build observations.
    require(all(isinstance(item, dict) and item.get('exit_code') == 0 for item in receipt['commands']), 'build contains failed command evidence')  # Reject failed builds.
    compression = [item for item in receipt['commands'] if item.get('label') == 'pkgbuild']  # Bind payload encoding.
    require(len(compression) == 1, 'build must record one pkgbuild command')  # Require construction evidence.
    command = compression[0].get('command')  # Read actual arguments.
    require(isinstance(command, list) and command.count('--compression') == 1, 'build must explicitly select legacy PKG compression')  # Reject implicit compression.
    require(command[command.index('--compression'):][:2] == ['--compression', 'legacy'], 'unsupported PKG payload compression')  # Require supported compression.
#
def validate_smoke_session(session: dict[str, Any], installed: Path) -> None:
    require(isinstance(session, dict) and session.get('state') == 'passed' and session.get('pane_stopped_by_kill_session') is True, 'installed session lifecycle did not pass')
    nonce = session.get('nonce')
    require(isinstance(nonce, str) and re.fullmatch('[0-9a-f]{32}', nonce), 'invalid installed session nonce')
    require(session.get('name') == 'macos-smoke-' + nonce, 'installed session name differs')
    pane, server = session.get('pane'), session.get('server')
    chain = session.get('ancestry')
    require(isinstance(pane, dict) and isinstance(server, dict) and isinstance(chain, list) and 2 <= len(chain) <= 16, 'missing installed session process identities')
    require(chain[0] == pane and chain[-1] == server and server.get('executable') == str(installed / 'ilium-server') and pane.get('executable') in ('/bin/sh', '/bin/bash'), 'session did not use the installed server')
    require(all(isinstance(item, dict) and type(item.get('pid')) is int and item['pid'] > 1 and type(item.get('ppid')) is int and item['ppid'] > 1 and isinstance(item.get('started'), str) and item['started'] for item in chain), 'invalid installed process custody')
    require(len({item['pid'] for item in chain}) == len(chain) and all(first['ppid'] == second['pid'] for first, second in zip(chain, chain[1:])), 'installed server ancestry differs')
    ready = nonce + ' ' + str(pane['pid']) + '\n'
    require(session.get('ready') == ready and session.get('ready_sha256') == release_tool.digest(ready.encode('ascii')), 'installed pane readiness differs')
    require(isinstance(session.get('listing'), str) and re.search(r'(?<![A-Za-z0-9_-])' + re.escape(session['name']) + r'(?![A-Za-z0-9_-])', session['listing']), 'installed session listing differs')


def validate_smoke_embedding(embedding: dict[str, Any], installed: Path, audit: dict[str, Any], binding: dict[str, Any], model_register: dict[str, Any]) -> None:
    require(isinstance(embedding, dict) and embedding.get('state') == 'passed' and embedding.get('dimensions') == 384, 'installed embedding did not pass')
    proof = embedding.get('proof')
    require(isinstance(proof, dict) and proof.get('type') == 'embedding-proof' and proof.get('input') == 'release embedding acceptance', 'missing installed embedding proof')
    require(proof.get('executable_path') == str(installed / 'ilium') and proof.get('binary_sha256') == audit['files']['ilium'], 'embedding client differs from audited installation')
    require(isinstance(model_register, dict) and isinstance(model_register.get('files'), dict), 'invalid installed embedding model register')
    model_hash = model_register['files'].get('model.onnx')
    require(isinstance(model_hash, str) and re.fullmatch('[0-9a-f]{64}', model_hash) and proof.get('model_sha256') == model_hash and embedding.get('model_sha256') == model_hash, 'installed embedding model differs')
    vector = proof.get('embedding')
    require(isinstance(vector, list) and len(vector) == 384 and all(type(value) in (int, float) and -sys.float_info.max <= value <= sys.float_info.max and math.isfinite(value) for value in vector) and any(value != 0 for value in vector), 'installed embedding vector is not finite and nonzero')
    require(embedding.get('vector_sha256') == release_tool.digest(json.dumps(vector, allow_nan=False).encode()), 'installed embedding vector digest differs')
    require(isinstance(proof.get('loaded_runtime'), str), 'invalid installed embedding runtime path')
    runtime = Path(proof['loaded_runtime'])
    require(runtime.parent == installed and re.fullmatch(r'libonnxruntime\.[0-9]+\.[0-9]+\.[0-9]+\.dylib', runtime.name) and runtime.name in audit['files'], 'embedding loaded an unshipped runtime')
    require(embedding.get('loaded_runtime') == str(runtime) and embedding.get('runtime_sha256') == audit['files'][runtime.name], 'installed embedding runtime digest differs')
    process = embedding.get('process')
    require(isinstance(process, dict) and type(process.get('pid')) is int and process['pid'] > 1 and process['pid'] == proof.get('ilium_pid') and process.get('executable') == str(installed / 'ilium') and bool(process.get('started')), 'embedding process custody differs')
    mappings = embedding.get('native_mappings')
    require(isinstance(mappings, str), 'missing installed embedding mappings')
    audit_native.validate_process_mapping(process['executable'], mappings, installed / 'ilium', runtime)
    for field, source_name in (('wrapper_sha256', 'release/tests/embedding_acceptance.py'), ('register_sha256', 'release/embedding-model.json')):
        require(embedding.get(field) == binding['source_inputs'][source_name], 'installed embedding acceptance source differs')
    output = embedding.get('stdout')
    require(isinstance(output, str) and output.endswith('\n') and len(output.encode('utf-8')) <= 1_000_000 and embedding.get('stdout_sha256') == release_tool.digest(output.encode('utf-8')), 'installed embedding stdout differs')
    records = [strict_json(line.encode('utf-8')) for line in output.splitlines()]
    require([record for record in records if record.get('type') == 'embedding-proof'] == [proof], 'installed embedding stdout proof differs')


def validate_smoke_receipt(receipt: dict[str, Any], build: dict[str, Any], target: dict[str, Any], audit: dict[str, Any], binding: dict[str, Any], build_receipt_sha256: str, smoke_sources: dict[str, str], model_register: dict[str, Any]) -> None:
    """Reopen native M3 evidence on the publication host without executing payloads."""
    validate_build_receipt(build, target, audit, binding)
    fields = {'schema', 'state', 'publication_allowed', 'tag', 'version', 'arch', 'target', 'smoke_source_inputs', 'build_receipt_sha256', 'packages', 'package_bytes', 'package_files', 'payload_tree', 'native_identity', 'formats', 'cleanup', 'commands', 'container_signing', 'container_notarization', 'credentials_used', 'root'} | set(binding)
    require(isinstance(receipt, dict) and set(receipt) == fields, 'smoke receipt has missing or unknown fields')
    require(receipt['schema'] == 1 and receipt['state'] == 'passed' and receipt['publication_allowed'] is True, 'macOS smoke did not qualify publication')
    require(all(receipt.get(key) == value for key, value in binding.items()), 'smoke native/source binding differs')
    require(receipt['build_receipt_sha256'] == build_receipt_sha256 and receipt['smoke_source_inputs'] == smoke_sources, 'smoke build or acceptance source differs')
    for key in ('tag', 'version', 'arch', 'target', 'package_files', 'payload_tree', 'packages', 'package_bytes', 'container_signing', 'container_notarization', 'credentials_used'):
        require(receipt[key] == build[key], 'smoke construction binding differs: ' + key)
    identity = receipt['native_identity']
    require(isinstance(identity, dict) and identity.get('system') == 'Darwin' and identity.get('machine') == packages.architectures[target['arch']][0] and identity.get('runner') == target['runner'] and identity.get('translated') is False, 'smoke was not genuinely native')
    cleanup = {'state': 'passed', **dict.fromkeys(('sources_restored', 'owned_images_detached', 'installed_payloads_removed', 'package_receipts_removed', 'owned_processes_stopped', 'work_retained'), True)}
    require(receipt['cleanup'] == cleanup, 'installed macOS cleanup is incomplete')
    require(isinstance(receipt['root'], str), 'invalid macOS smoke root')
    root_path = Path(receipt['root'])
    require(root_path.is_absolute() and '..' not in root_path.parts, 'invalid macOS smoke root')
    results = receipt['formats']
    require(isinstance(results, dict) and set(results) == set(packages.formats), 'macOS smoke format inventory differs')
    hidden_originals = []
    for name, result in results.items():
        require(isinstance(result, dict) and result.get('state') == 'passed' and result.get('owned_processes_stopped') is True, 'macOS format lifecycle did not pass')
        require(result.get('payload_tree') == build['payload_tree'] and result.get('versions') == audit['binary_versions'], 'installed macOS payload or versions differ')
        require(isinstance(result.get('installed_directory'), str), 'invalid installed macOS directory')
        installed = Path(result['installed_directory'])
        require(installed.is_absolute() and installed.is_relative_to(root_path / name) and '..' not in installed.parts, 'installed macOS directory escaped its smoke root')
        hidden = result.get('source_hiding')
        require(isinstance(hidden, list) and len(hidden) == 3 and all(isinstance(item, dict) and set(item) == {'original', 'hidden'} and all(isinstance(value, str) for value in item.values()) for item in hidden), 'source/build/package inputs were not hidden')
        originals = [Path(item['original']) for item in hidden]
        require(len(set(originals)) == 3 and all(path.is_absolute() and '..' not in path.parts and not installed.is_relative_to(path) for path in originals), 'hidden macOS input custody differs')
        require(all(Path(item['hidden']).is_relative_to(root_path / name / 'hidden-inputs') and '..' not in Path(item['hidden']).parts for item in hidden), 'hidden macOS input escaped its owned root')
        hidden_originals.append(originals)
        validate_smoke_session(result.get('session'), installed)
        validate_smoke_embedding(result.get('embedding'), installed, audit, binding, model_register)
        animation_path = root_path / name / 'installed-animation.json'
        animation = result.get('animation')
        require(isinstance(animation, dict) and animation_path.is_file() and
                not animation_path.is_symlink() and
                result.get('animation_receipt_sha256') == packages.sha(animation_path) and
                packages.read_json(animation_path) == animation and
                animation.get('source_files') == {source_name: smoke_sources[source_name]
                                                  for source_name in animation_gate.SOURCE_FILES},
                'installed macOS animation evidence is missing or changed')
        animation_gate.validate_receipt(
            animation, target=target, tag=receipt['tag'], package_format=name,
            audit_path=root_path / 'acceptance-assets/native-audit.json',
            installed_root=installed, executable_root=installed,
            command=[str(installed / 'ilium'), 'release-animation-probe'],
            source_hashes={source_name: smoke_sources[source_name]
                           for source_name in animation_gate.SOURCE_FILES})
        installation = result.get('installation')
        require(isinstance(installation, dict) and installation.get('format') == name and installation.get('payload_removed') is True, 'macOS installed payload removal is incomplete')
        if name == 'dmg':
            require(installation.get('source_image_detached_before_execution') is True, 'DMG source image remained attached during execution')
        if name == 'pkg':
            require(all(installation.get(key) is True for key in ('receipt_removed', 'target_detached', 'target_image_removed', 'host_receipts_unchanged')), 'PKG receipt or target cleanup is incomplete')
    require(all(paths == hidden_originals[0] for paths in hidden_originals), 'formats hid different source/build inputs')
    commands = receipt['commands']
    require(isinstance(commands, list) and 0 < len(commands) <= packages.max_commands and all(isinstance(item, dict) and type(item.get('exit_code')) is int and 0 <= item['exit_code'] <= 255 for item in commands), 'missing or invalid macOS native command evidence')
    require(all(item['exit_code'] == 0 or item.get('label') in ('package-signature-diagnostic', 'process-identity', 'session-cleanup') for item in commands), 'macOS smoke retained a failed required command')


def read_build(directory: Path, target: dict[str, Any], audit: dict[str, Any], binding: dict[str, Any]) -> dict[str, Any]:  # Revalidate build outputs.
    require(directory.is_dir() and not directory.is_symlink(), 'package input must be a regular directory')  # Preserve filesystem boundary.
    names = set(packages.package_names(target['arch'])) | {packages.receipt_name(target['arch'])}  # Require four artifacts.
    require({path.name for path in directory.iterdir()} == names, 'package directory has missing or unexpected entries')  # Reject unrelated artifacts.
    receipt = packages.read_json(directory / packages.receipt_name(target['arch']))  # Bound strict JSON.
    validate_build_receipt(receipt, target, audit, binding)  # Join native evidence.
    for name, digest in receipt['packages'].items():  # Rehash every container.
        path = packages.regular_file(directory / name)  # Reject filesystem aliases.
        require(packages.sha(path) == digest and path.stat().st_size == receipt['package_bytes'][name], 'package bytes differ: ' + name)  # Bind actual bytes.
    return receipt  # Retain immutable receipt.
#
def validate_package_metadata(distribution: bytes, information: bytes, layout: dict[str, Any], payload_tree: dict[str, Any]) -> dict[str, Any]:  # Inspect before privilege.
    wanted = packages.package_layout(layout['zip']['prefix'].removeprefix('ilium-macos-'), layout['pkg']['version'])  # Revalidate package layout.
    require(layout == wanted, 'unrecognized package metadata layout')  # Reject policy drift.
    reference, observed = safe_xml(packages.distribution_xml(layout)), safe_xml(distribution)  # Compare actual Distribution.
    for element in observed.iter():  # Validate generated bookkeeping.
        if element.tag != 'pkg-ref':  # Preserve reference semantics.
            continue  # Retain installer controls.
        for field in ('installKBytes', 'archiveKBytes'):  # Allow generated sizes.
            if field in element.attrib:  # Validate numeric metadata.
                require(re.fullmatch(r'[0-9]{1,12}', element.attrib[field]), 'invalid generated package size')  # Reject arbitrary expressions.
                del element.attrib[field]  # Compare remaining semantics.
        if 'auth' in element.attrib:  # Require administrator authorization.
            require(element.attrib.pop('auth') == 'root', 'unexpected package authorization')  # Preserve installation domain.
        for child in list(element):  # Exclude relocatable bundles.
            require(child.tag == 'bundle-version' and not child.attrib and not list(child) and not (child.text or '').strip(), 'unexpected package reference child')  # Reject active extensions.
            element.remove(child)  # Ignore empty bookkeeping.
        if (element.text or '').strip() == '#' + layout['pkg']['component']:  # Allow local URL prefix.
            element.text = layout['pkg']['component']  # Normalize component URL.
    def shape(element: xml_tree.Element) -> tuple[Any, ...]:  # Compare XML structure.
        return (element.tag, tuple(sorted(element.attrib.items())), (element.text or '').strip(), tuple(sorted((shape(child) for child in element), key=repr)))  # Preserve semantic multiplicities.
    require(shape(observed) == shape(reference), 'actual Distribution changes package behavior')  # Reject changed behavior.
    info = safe_xml(information)  # Inspect actual PackageInfo.
    require(info.tag == 'pkg-info' and set(info.attrib) == {'format-version', 'identifier', 'version', 'install-location', 'auth'}, 'PackageInfo schema differs')  # Reject unknown controls.
    expected = {'format-version': '2', 'identifier': layout['pkg']['identifier'], 'version': layout['pkg']['version'], 'install-location': layout['pkg']['install_location'], 'auth': 'root'}  # Freeze administrator destination.
    require(info.attrib == expected, 'PackageInfo identity or installation location differs')  # Prevent installation redirection.
    children = list(info)  # Require payload metadata.
    payloads = [child for child in children if child.tag == 'payload']  # Identify payload description.
    require(len(payloads) == 1, 'PackageInfo must describe one payload')  # Reject duplicate payloads.
    for child in children:  # Allow empty bookkeeping.
        if child.tag == 'payload':  # Bound payload attributes.
            require(not list(child) and set(child.attrib) == {'installKBytes', 'numberOfFiles'} and all(re.fullmatch(r'[0-9]{1,12}', value) for value in child.attrib.values()), 'invalid PackageInfo payload metadata')  # Reject unknown fields.
            require(int(child.attrib['numberOfFiles']) in (len(payload_tree), len(payload_tree) + 1) and 0 < int(child.attrib['installKBytes']) <= max_payload_bytes // 1024 + 16384, 'PackageInfo payload count/size differs')  # Allow optional root.
            continue  # Exclude installer scripts.
        require(child.tag in ('bundle-version', 'upgrade-bundle', 'update-bundle', 'atomic-update-bundle', 'strict-identifier', 'relocate') and not child.attrib and not list(child) and not (child.text or '').strip(), 'PackageInfo contains scripts/bundles/unsupported behavior')  # Reject active extensions.
    return {'distribution_sha256': release_tool.digest(distribution), 'package_info_sha256': release_tool.digest(information), 'identifier': expected['identifier'], 'version': expected['version'], 'install_location': expected['install-location'], 'scripts': False, 'container_signing': 'unsigned', 'container_notarization': 'disabled'}  # Retain inspected metadata.
#
def validate_bom(output: str, payload_tree: dict[str, Any]) -> None:  # Compare native BOM.
    found: dict[str, tuple[int, int]] = {}  # Track unique records.
    for line in output.splitlines():  # Inspect all entries.
        fields = line.split('\t')  # Parse tab-separated fields.
        require(len(fields) == 6, 'BOM record field count differs')  # Avoid guessing columns.
        name, symbolic, numeric, uid, gid, size = fields  # Match -p fMmugs exactly.
        name = name.removeprefix('./') if name != '.' else name  # Normalize dot prefix.
        require(re.fullmatch(r'[0-7]{3,7}', numeric) and uid == '0' and gid == '0', 'BOM modes or ownership differ')  # Require root:wheel ownership.
        mode = int(numeric, 8)  # Retain type bits.
        if name == '.':  # Allow explicit root.
            require(name not in found and symbolic.startswith('d') and stat.S_IMODE(mode) == 0o755 and stat.S_IFMT(mode) in (0, stat.S_IFDIR), 'invalid BOM root')  # Require canonical root.
            found[name] = (0o755, 0)  # Reject duplicate roots.
            continue  # Ignore root bookkeeping.
        require(name in payload_tree and name not in found and symbolic.startswith('-') and stat.S_IFMT(mode) in (0, stat.S_IFREG) and re.fullmatch(r'[0-9]+', size), 'BOM has unknown/link/special/duplicate entry')  # Reject non-payload entries.
        found[name] = (stat.S_IMODE(mode), int(size))  # Compare modes and sizes.
    require(set(found) - {'.'} == set(payload_tree), 'BOM file inventory differs')  # Require complete payload.
    require(all(found[name] == (entry['mode'], entry['bytes']) for name, entry in payload_tree.items()), 'BOM file metadata differs')  # Preserve audited layout.
#
def bounded_inflate(data: bytes, limit: int, framing: int) -> bytes:  # Bound single-stream expansion.
    packages.require(0 < len(data) <= packages.max_payload_bytes and 0 <= limit <= packages.max_payload_bytes, 'invalid compressed input bounds')  # Bound input/output sizes.
    inflater = zlib.decompressobj(framing)  # Select exact framing.
    try:  # Normalize decoder failures.
        result = inflater.decompress(data, limit + 1)  # Bound expanded output.
    except zlib.error as error:  # Reject malformed streams.
        raise packages.release_tool.ReleaseError('invalid compressed package stream') from error  # Preserve failure cause.
    packages.require(len(result) <= limit and inflater.eof and not inflater.unused_data and not inflater.unconsumed_tail, 'compressed package has excessive output, truncation, or trailing streams')  # Require one complete stream.
    return result  # Avoid filesystem extraction.
#
def xml_document(data: bytes, expected_root: str) -> xml_tree.Element:  # Parse bounded XML.
    packages.require(0 < len(data) <= packages.max_json_bytes and b'\x00' not in data and b'<!DOCTYPE' not in data.upper() and b'<!ENTITY' not in data.upper(), 'unsafe or oversized package XML')  # Reject alternate encodings.
    try:  # Normalize XML failures.
        document = xml_tree.fromstring(data.decode('utf-8'))  # Parse entity-free UTF-8.
    except (UnicodeDecodeError, xml_tree.ParseError) as error:  # Reject malformed metadata.
        raise packages.release_tool.ReleaseError('malformed package XML') from error  # Preserve policy failure.
    packages.require(document.tag == expected_root and sum(1 for _node in document.iter()) <= 4096, 'unexpected package XML root or node count')  # Bound XML structure.
    return document  # Defer vocabulary validation.
#
def xml_children(element: xml_tree.Element, allowed: set[str], repeated: set[str] | None = None) -> dict[str, list[xml_tree.Element]]:  # Reject unknown fields.
    result: dict[str, list[xml_tree.Element]] = {}  # Preserve repeated-entry order.
    for child in element:  # Inspect child fields.
        packages.require(child.tag in allowed and (child.tag not in result or child.tag in (repeated or set())), 'unknown or duplicated package XML field: ' + str(child.tag))  # Reject alternate semantics.
        result.setdefault(child.tag, []).append(child)  # Retain admitted elements.
    return result  # Return classified fields.
#
def xml_scalar(element: xml_tree.Element) -> str:  # Read scalar metadata.
    packages.require(not element.attrib and not list(element), 'structured XML field where scalar metadata was expected')  # Reject structured values.
    return (element.text or '').strip()  # Allow scalar indentation.
#
def decimal_field(element: xml_tree.Element) -> int:  # Decode bounded integers.
    value = xml_scalar(element)  # Require scalar fields.
    packages.require(re.fullmatch(r'0|[1-9][0-9]{0,19}', value), 'invalid package decimal field')  # Bound integer spelling.
    return int(value)  # Return decoded integer.
#
def xar_digest(element: xml_tree.Element, data: bytes) -> None:  # Verify member hashes.
    algorithm = element.attrib.get('style')  # Read checksum algorithm.
    packages.require(set(element.attrib) == {'style'} and algorithm in ('sha1', 'sha256', 'sha512') and not list(element), 'unsupported XAR member checksum')  # Reject weak checksums.
    expected = (element.text or '').strip()  # Read hexadecimal digest.
    packages.require(expected == hashlib.new(algorithm, data).hexdigest(), 'XAR member checksum differs')  # Hash complete content.
#
def parse_xar(data: bytes) -> dict[str, bytes]:  # Inspect unsigned XAR.
    packages.require(28 <= len(data) <= packages.max_payload_bytes, 'invalid XAR size')  # Bound container size.
    magic, header_size, version, compressed_size, expanded_size, algorithm_id = struct.unpack_from('>IHHQQI', data)  # Decode Apple header.
    algorithms = {1: 'sha1', 3: 'sha256', 4: 'sha512'}  # Map Apple algorithms.
    packages.require(magic == 0x78617221 and header_size == 28 and version == 1 and algorithm_id in algorithms, 'unsupported XAR header or TOC checksum')  # Reject unsupported headers.
    packages.require(0 < compressed_size <= packages.max_json_bytes and 0 < expanded_size <= packages.max_json_bytes and 28 + compressed_size < len(data), 'invalid XAR TOC extent')  # Bound TOC sizes.
    compressed_toc = data[28:28 + compressed_size]  # Slice declared TOC.
    toc_bytes = bounded_inflate(compressed_toc, expanded_size, zlib.MAX_WBITS)  # Use zlib framing.
    packages.require(len(toc_bytes) == expanded_size, 'XAR expanded TOC size differs')  # Verify expanded length.
    document = xml_document(toc_bytes, 'xar')  # Parse TOC structure.
    packages.require(not document.attrib and len(document) == 1 and document[0].tag == 'toc' and not document[0].attrib, 'unexpected XAR document structure')  # Reject extra wrappers.
    fields = xml_children(document[0], {'creation-time', 'checksum', 'file'}, {'file'})  # Reject signatures/extensions.
    packages.require('checksum' in fields and 'file' in fields, 'missing XAR checksum or file inventory')  # Require nonempty inventory.
    if 'creation-time' in fields:  # Allow creation timestamp.
        packages.require(re.fullmatch(r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z', xml_scalar(fields['creation-time'][0])), 'invalid XAR creation time')  # Validate UTC spelling.
    checksum = fields['checksum'][0]  # Bind checksum declaration.
    algorithm = algorithms[algorithm_id]  # Select admitted algorithm.
    packages.require(checksum.attrib == {'style': algorithm}, 'XAR header/TOC checksum styles differ')  # Reject contradictory algorithms.
    checksum_fields = xml_children(checksum, {'offset', 'size'})  # Read checksum extent.
    packages.require(set(checksum_fields) == {'offset', 'size'}, 'missing XAR TOC checksum extent')  # Require both bounds.
    checksum_offset, checksum_size = decimal_field(checksum_fields['offset'][0]), decimal_field(checksum_fields['size'][0])  # Decode heap offsets.
    heap = data[28 + compressed_size:]  # Locate XAR heap.
    packages.require(checksum_offset == 0 and checksum_size == hashlib.new(algorithm).digest_size and checksum_size <= len(heap), 'invalid XAR TOC checksum placement')  # Require checksum prefix.
    packages.require(heap[:checksum_size] == hashlib.new(algorithm, compressed_toc).digest(), 'XAR compressed TOC checksum differs')  # Hash compressed TOC.
    extents = [(0, checksum_size)]  # Account for checksum bytes.
    pending = [(element, '', 0) for element in reversed(fields['file'])]  # Bound directory traversal.
    files: dict[str, bytes] = {}  # Retain verified files.
    directories: set[str] = set()  # Track directory inventory.
    names: set[str] = set()  # Reject case aliases.
    identifiers: set[str] = set()  # Track unique IDs.
    expanded_total = 0  # Bound aggregate expansion.
    metadata_names = {'inode', 'deviceno', 'mode', 'uid', 'user', 'gid', 'group', 'atime', 'mtime', 'ctime'}  # Admit Apple stat fields.
    while pending:  # Inspect before extraction.
        element, parent, depth = pending.pop()  # Preserve parent relationships.
        identifier = element.attrib.get('id')  # Read member ID.
        packages.require(set(element.attrib) == {'id'} and isinstance(identifier, str) and re.fullmatch(r'[1-9][0-9]{0,9}', identifier) and identifier not in identifiers and len(identifiers) < 1024 and depth <= 8, 'invalid XAR file identifier, count, or depth')  # Bound unique structure.
        identifiers.add(identifier)  # Reserve member identity.
        member = xml_children(element, {'name', 'type', 'data', 'file'} | metadata_names, {'file'})  # Reject active metadata.
        packages.require('name' in member and 'type' in member, 'missing XAR member name/type')  # Require extraction identity.
        base_name = xml_scalar(member['name'][0])  # Reject encoded names.
        packages.require(member['name'][0].text == base_name and '/' not in base_name and packages.relative_name(base_name) == base_name, 'unsafe XAR basename')  # Use explicit hierarchy.
        name = parent + base_name  # Form validated path.
        packages.require(name.casefold() not in names, 'duplicate or case-colliding XAR member')  # Reject path aliases.
        names.add(name.casefold())  # Reserve unique spelling.
        kind = xml_scalar(member['type'][0])  # Read explicit type.
        packages.require(kind in ('file', 'directory'), 'XAR links or special files are prohibited')  # Reject unsafe types.
        for metadata_name in metadata_names & set(member):  # Validate stat fields.
            value = xml_scalar(member[metadata_name][0])  # Require scalar metadata.
            if metadata_name in ('inode', 'deviceno', 'uid', 'gid'):  # Check numeric fields.
                packages.require(re.fullmatch(r'0|[1-9][0-9]{0,19}', value), 'invalid XAR numeric metadata')  # Require bounded decimals.
                continue  # Continue scalar validation.
            if metadata_name == 'mode':  # Inspect permission bits.
                packages.require(re.fullmatch(r'[0-7]{3,6}', value) and not int(value, 8) & 0o7000 and int(value, 8) <= 0o777, 'unsafe XAR mode')  # Reject privileged modes.
                continue  # Continue metadata validation.
            if metadata_name in ('atime', 'mtime', 'ctime'):  # Inspect UTC timestamps.
                packages.require(re.fullmatch(r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z', value), 'invalid XAR timestamp')  # Validate timestamp spelling.
                continue  # Continue owner validation.
            packages.require(len(value) <= 256 and '\x00' not in value, 'invalid XAR user/group metadata')  # Bound owner labels.
        if kind == 'directory':  # Inspect directory entries.
            packages.require('data' not in member and 'file' in member, 'XAR directory contains data or is empty')  # Require directory children.
            directories.add(name)  # Retain directory topology.
            pending.extend((child, name + '/', depth + 1) for child in reversed(member['file']))  # Queue actual children.
            continue  # Exclude directory streams.
        packages.require('data' in member and 'file' not in member, 'XAR regular file has no data or contains children')  # Reject nested files.
        data_element = member['data'][0]  # Inspect stream record.
        packages.require(not data_element.attrib, 'unexpected XAR data attributes')  # Reject alternate semantics.
        stream_fields = xml_children(data_element, {'offset', 'length', 'size', 'encoding', 'archived-checksum', 'extracted-checksum'})  # Admit complete stream fields.
        packages.require(set(stream_fields) == {'offset', 'length', 'size', 'encoding', 'archived-checksum', 'extracted-checksum'}, 'missing XAR data extent/encoding/checksum')  # Require complete custody.
        offset, length, size = (decimal_field(stream_fields[field][0]) for field in ('offset', 'length', 'size'))  # Decode both lengths.
        packages.require(length <= packages.max_payload_bytes and size <= packages.max_payload_bytes and offset + length <= len(heap), 'XAR file extent escapes heap')  # Bound heap extent.
        expanded_total += size  # Count expanded bytes.
        packages.require(expanded_total <= packages.max_payload_bytes, 'XAR expanded inventory exceeds limit')  # Bound aggregate output.
        if length:  # Ignore zero-length extents.
            extents.append((offset, offset + length))  # Track stored ranges.
        archived = heap[offset:offset + length]  # Slice admitted extent.
        xar_digest(stream_fields['archived-checksum'][0], archived)  # Verify stored bytes.
        encoding = stream_fields['encoding'][0]  # Read compression style.
        packages.require(set(encoding.attrib) == {'style'} and not list(encoding) and not (encoding.text or '').strip(), 'invalid XAR encoding field')  # Reject ambiguous encodings.
        style = encoding.attrib['style']  # Select native framing.
        packages.require(style in ('application/octet-stream', 'application/x-gzip'), 'unsupported XAR stream encoding')  # Reject unsupported compression.
        content = archived if style == 'application/octet-stream' else bounded_inflate(archived, size, zlib.MAX_WBITS)  # Decode zlib explicitly.
        packages.require(len(content) == size, 'XAR extracted member size differs')  # Verify decoded length.
        xar_digest(stream_fields['extracted-checksum'][0], content)  # Hash extracted bytes.
        files[name] = content  # Retain verified content.
    position = 0  # Account for every byte.
    for start, end in sorted(extents):  # Inspect ordered extents.
        packages.require(start == position and end >= start, 'XAR heap has overlapping or unlisted bytes')  # Reject gaps/overlaps.
        position = end  # Advance owned extent.
    packages.require(position == len(heap), 'XAR heap has an unlisted trailer')  # Reject hidden suffixes.
    required_directories = {parent for name in files for parent in (name.rsplit('/', 1)[0],) if '/' in name}  # Identify parent directories.
    required_directories |= {prefix for name in tuple(required_directories) for prefix in ('/'.join(name.split('/')[:index]) for index in range(1, len(name.split('/'))))}  # Include ancestor directories.
    packages.require(directories == required_directories, 'XAR directory inventory differs from its file hierarchy')  # Reject directory extras.
    return dict(sorted(files.items()))  # Return verified inventory.
#
def product_parts(data: bytes, component: str) -> dict[str, bytes]:  # Admit one component.
    packages.require('/' not in component and packages.relative_name(component) == component, 'invalid component package name')  # Require safe basename.
    outer = parse_xar(data)  # Verify outer archive.
    names = {'Bom', 'PackageInfo', 'Payload'}  # Declare component inventory.
    expanded_names = {'Distribution'} | {component + '/' + name for name in names}  # Describe expanded topology.
    if set(outer) == expanded_names:  # Admit expanded component.
        result = {'Distribution': outer['Distribution'], **{name: outer[component + '/' + name] for name in names}}  # Flatten known entries.
    else:  # Inspect nested alternative.
        packages.require(set(outer) == {'Distribution', component}, 'PKG has unknown or missing component files, resources, or scripts')  # Reject component extras.
        inner = parse_xar(outer[component])  # Allow one nesting level.
        packages.require(set(inner) == names, 'component PKG has unknown or missing files/scripts')  # Reject scripts/nesting.
        result = {'Distribution': outer['Distribution'], **inner}  # Return uniform metadata.
    packages.require(all(result[name] for name in result) and all(len(result[name]) <= packages.max_json_bytes for name in ('Distribution', 'PackageInfo', 'Bom')), 'empty or oversized PKG metadata')  # Bound component metadata.
    packages.require(len(result['Bom']) >= 32 and result['Bom'].startswith(b'BOMStore'), 'invalid PKG bill-of-materials header')  # Validate BOM header.
    return result  # Defer semantic validation.
#
def parse_cpio(data: bytes, expected_tree: dict[str, dict[str, Any]]) -> dict[str, bytes]:  # Inspect flat CPIO.
    packages.require(isinstance(expected_tree, dict) and 0 < len(expected_tree) <= 256 and all('/' not in name and packages.relative_name(name) == name and entry.get('kind') == 'file' for name, entry in expected_tree.items()), 'invalid expected CPIO payload inventory')  # Use frozen inventory.
    packages.require(0 < len(data) <= packages.max_payload_bytes and not data.startswith(b'pbzx'), 'unexpected PKG payload compression; build with --compression legacy')  # Reject unsupported PBZX.
    archive = bounded_inflate(data, packages.max_payload_bytes, zlib.MAX_WBITS | 16) if data.startswith(b'\x1f\x8b') else data  # Decode single gzip stream.
    content: dict[str, bytes] = {}  # Retain verified payload.
    folded: set[str] = set()  # Track pathname aliases.
    position, count, expanded_total, root_seen = 0, 0, 0, False  # Bound root parsing.
    while position < len(archive):  # Read complete records.
        count += 1  # Count all entries.
        packages.require(count <= len(expected_tree) + 2 and position + 6 <= len(archive), 'excess or truncated CPIO entries')  # Reject excess records.
        magic = archive[position:position + 6]  # Read ASCII magic.
        packages.require(magic in (b'070707', b'070701', b'070702'), 'unsupported CPIO record format')  # Reject unsupported formats.
        new_format = magic != b'070707'  # Select alignment rules.
        header_size = 110 if new_format else 76  # Select header length.
        packages.require(position + header_size <= len(archive), 'truncated CPIO header')  # Bound header reads.
        fields = archive[position + 6:position + header_size]  # Slice numeric fields.
        packages.require(re.fullmatch(rb'[0-9A-Fa-f]+' if new_format else rb'[0-7]+', fields), 'invalid CPIO numeric header')  # Validate numeric spelling.
        if new_format:  # Decode newc fields.
            values = [int(fields[index:index + 8], 16) for index in range(0, 104, 8)]  # Preserve field ordering.
            _inode, mode, uid, gid, links, _mtime, size, _device_major, _device_minor, special_major, special_minor, name_size, checksum = values  # Identify device metadata.
            special = special_major | special_minor  # Reject special devices.
        else:  # Decode odc fields.
            widths, values, cursor = (6, 6, 6, 6, 6, 6, 6, 11, 6, 11), [], 0  # Use odc widths.
            for width in widths:  # Parse fixed widths.
                values.append(int(fields[cursor:cursor + width], 8))  # Decode octal field.
                cursor += width  # Advance field cursor.
            _device, _inode, mode, uid, gid, links, special, _mtime, name_size, size = values  # Preserve odc ordering.
            checksum = 0  # Use absent checksum.
        position += header_size  # Locate pathname bytes.
        packages.require(1 <= name_size <= 256 and position + name_size <= len(archive), 'invalid CPIO name extent')  # Bound pathname length.
        raw_name = archive[position:position + name_size]  # Read exact pathname.
        packages.require(raw_name[-1:] == b'\x00' and b'\x00' not in raw_name[:-1], 'CPIO name is not singly NUL-terminated')  # Require single terminator.
        try:  # Admit ASCII names.
            name = raw_name[:-1].decode('ascii')  # Decode without normalization.
        except UnicodeDecodeError as error:  # Reject foreign names.
            raise packages.release_tool.ReleaseError('non-ASCII CPIO pathname') from error  # Preserve structured failure.
        position += name_size  # Advance past pathname.
        if new_format:  # Apply newc alignment.
            padding = (-position) % 4  # Compute name padding.
            packages.require(archive[position:position + padding] == b'\x00' * padding, 'nonzero CPIO name padding')  # Require zero padding.
            position += padding  # Advance to payload.
        packages.require(size <= packages.max_file_bytes and position + size <= len(archive), 'invalid CPIO data extent')  # Bound payload extent.
        value = archive[position:position + size]  # Slice exact data.
        position += size  # Advance past payload.
        if new_format:  # Align next header.
            padding = (-position) % 4  # Compute data padding.
            packages.require(archive[position:position + padding] == b'\x00' * padding, 'nonzero CPIO data padding')  # Reject hidden bytes.
            position += padding  # Finish record extent.
        packages.require(checksum == (sum(value) & 0xffffffff if magic == b'070702' else 0), 'CPIO payload checksum differs')  # Verify CPIO checksum.
        if name == 'TRAILER!!!':  # Recognize mandatory trailer.
            packages.require(size == 0 and root_seen and set(content) == set(expected_tree) and len(archive) - position <= 65536 and not any(archive[position:]), 'CPIO trailer, padding, or exact inventory differs')  # Reject incomplete inventory.
            return dict(sorted(content.items()))  # Return verified bytes.
        packages.require(uid == 0 and gid == 0 and special == 0 and not mode & 0o7000, 'CPIO ownership or special metadata differs')  # Require safe ownership.
        if name in ('.', './'):  # Admit one root.
            packages.require(not root_seen and stat.S_ISDIR(mode) and stat.S_IMODE(mode) == 0o755 and size == 0 and links >= 1, 'invalid or duplicate CPIO root')  # Require canonical root.
            root_seen = True  # Record root presence.
            continue  # Exclude directory data.
        name = name.removeprefix('./')  # Normalize one prefix.
        packages.require('/' not in name and packages.relative_name(name) == name and name in expected_tree and name.casefold() not in folded, 'unsafe, unknown, or duplicated CPIO payload name')  # Reject pathname aliases.
        folded.add(name.casefold())  # Reserve audited basename.
        expected = expected_tree[name]  # Select final audit entry.
        expanded_total += size  # Count total output.
        packages.require(expanded_total <= packages.max_payload_bytes and stat.S_ISREG(mode) and links == 1 and stat.S_IMODE(mode) == expected['mode'] and size == expected['bytes'] and hashlib.sha256(value).hexdigest() == expected['sha256'], 'CPIO file type, links, mode, size, or audited bytes differ')  # Reject payload substitutions.
        content[name] = value  # Retain audited bytes.
    raise packages.release_tool.ReleaseError('CPIO payload has no trailer')  # Reject missing trailer.
def copy_verified(source: Path, destination: Path, digest: str, mode: int = 0o644) -> None:  # Snapshot bounded input.
    packages.regular_file(source)  # Reject source aliases.
    copied = 0  # Bound input growth.
    with source.open('rb') as incoming, destination.open('xb') as outgoing:  # Create exclusive snapshot.
        while block := incoming.read(min(1_048_576, max_payload_bytes - copied + 1)):  # Bound the total copy.
            copied += len(block)  # Count copied bytes.
            require(copied <= max_payload_bytes, 'input grew beyond snapshot limit')  # Reject excess growth.
            outgoing.write(block)  # Copy exact block.
        outgoing.flush()  # Complete Python buffering.
        os.fsync(outgoing.fileno())  # Synchronize snapshot bytes.
    destination.chmod(mode)  # Set private permissions.
    require(packages.sha(destination) == digest, 'copied input differs from bound bytes')  # Reject copying drift.
#
def run_command(runner: packages.command_runner, label: str, command: list[str | Path], timeout: int = 120, allowed: tuple[int, ...] = (0,), environment: dict[str, str] | None = None, cwd: Path | None = None, cleanup: bool = False) -> bytes:  # Reuse bounded runner.
    previous_environment, previous_work, count = runner.environment, runner.work, len(runner.records)  # Preserve runner ownership.
    runner.environment = environment if environment is not None else previous_environment  # Use explicit environment.
    runner.work = cwd if cwd is not None else previous_work  # Select private cwd.
    try:  # Restore runner settings.
        try:  # Require declared status.
            output = runner.run(label, command, timeout=timeout, cleanup=cleanup)  # Preserve command bounds.
        except release_tool.ReleaseError as error:  # Preserve timeout failures.
            if len(runner.records) != count + 1 or runner.records[-1]['exit_code'] == 0 or runner.records[-1]['exit_code'] not in allowed or not str(error).startswith('native command failed; inspect '):  # Admit explicit nonzero diagnostics only.
                raise  # Propagate bounds and uncertain failures.
            output = runner.records[-1]['stdout'].encode('utf-8')  # Recover bounded output.
        require(runner.records[-1]['exit_code'] in allowed, 'unexpected native command status: ' + label)  # Validate exit status.
        return output  # Return actual evidence.
    finally:  # Restore owned cwd.
        runner.environment, runner.work = previous_environment, previous_work  # Restore runner environment.
#
def process_identity(runner: packages.command_runner, process_id: int) -> dict[str, Any] | None:  # Observe process identity.
    require(type(process_id) is int and process_id > 1, 'invalid owned process ID')  # Exclude groups/init.
    output = run_command(runner, 'process-identity', ['/bin/ps', '-p', str(process_id), '-o', 'pid=', '-o', 'ppid=', '-o', 'lstart=', '-o', 'comm='], allowed=(0, 1), cleanup=True).decode('utf-8').strip()  # Read native identity.
    if not output:  # Handle absent PID.
        require(not runner.records[-1]['stderr'].strip(), 'process observation failed rather than proving absence')  # Reject uncertain observation.
        return None  # Record process absence.
    fields = output.split(None, 7)  # Preserve path spaces.
    require(len(fields) == 8 and fields[0] == str(process_id) and fields[1].isdigit(), 'malformed native process identity')  # Reject ambiguous rows.
    return {'pid': process_id, 'ppid': int(fields[1]), 'started': ' '.join(fields[2:7]), 'executable': fields[7]}  # Retain birth/path identity.
#
def same_process(first: dict[str, Any] | None, second: dict[str, Any]) -> bool:  # Prevent PID reuse.
    return first is not None and all(first.get(key) == second.get(key) for key in ('pid', 'started', 'executable'))  # Compare recorded identity.
#
def stop_process(runner: packages.command_runner, identity: dict[str, Any]) -> None:  # Stop owned process.
    for action in (signal.SIGTERM, signal.SIGKILL):  # Bound identity escalation.
        if not same_process(process_identity(runner, identity['pid']), identity):  # Reject reused identity.
            return  # Preserve replacement process.
        try:  # Handle exit races.
            os.kill(identity['pid'], action)  # Signal matching identity.
        except ProcessLookupError:  # Native exit won the race.
            return  # Skip unnecessary signals.
        deadline = time.monotonic() + 5  # Bound escalation stage.
        while time.monotonic() < deadline:  # Await owned identity.
            if not same_process(process_identity(runner, identity['pid']), identity):  # Observe process exit.
                return  # Release custody record.
            time.sleep(0.25)  # Bound polling interval.
    require(not same_process(process_identity(runner, identity['pid']), identity), 'owned process survived bounded cleanup')  # Preserve live installations.
#
def installed_processes(runner: packages.command_runner, installed: Path) -> list[dict[str, Any]]:  # Find installed processes.
    output = run_command(runner, 'installed-processes', ['/bin/ps', '-axww', '-o', 'pid=', '-o', 'comm='], cleanup=True).decode('utf-8')  # Observe executable paths.
    paths = {str(installed / 'ilium'), str(installed / 'ilium-server')}  # Limit private custody.
    found = []  # Exclude user processes.
    for line in output.splitlines():  # Inspect native table.
        parts = line.strip().split(None, 1)  # Preserve executable spaces.
        if len(parts) != 2 or parts[1] not in paths:  # Exclude unrelated paths.
            continue  # Reject basename custody.
        require(parts[0].isdigit() and len(found) < 16, 'excessive or invalid installed processes')  # Bound adopted identities.
        identity = process_identity(runner, int(parts[0]))  # Recheck birth/path identity.
        if identity is not None and identity['executable'] in paths:  # Handle short-lived processes.
            found.append(identity)  # Retain matching executable.
    return found  # Return owned identities.
#
def stop_installed(runner: packages.command_runner, installed: Path) -> None:  # Clean owned processes.
    for identity in installed_processes(runner, installed):  # Adopt private-path identities.
        stop_process(runner, identity)  # Recheck before signaling.
    require(not installed_processes(runner, installed), 'installed processes remain alive')  # Require process absence.
#
def private_environment(directory: Path) -> tuple[dict[str, str], Path]:  # Isolate application state.
    home = directory / 'home'  # Create private home.
    home.mkdir(mode=0o700)  # Reject preexisting state.
    project = directory / 'project'  # Create private project.
    project.mkdir(mode=0o700)  # Exclude source cwd.
    environment = {'PATH': str(Path(sys.executable).resolve().parent) + ':/usr/bin:/bin:/usr/sbin:/sbin', 'HOME': str(home), 'TMPDIR': str(directory / 'temporary') + '/', 'LANG': 'C', 'LC_ALL': 'C', 'TZ': 'UTC', 'TERM': 'xterm-256color'}  # Exclude credential overrides.
    for variable, name in (('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_STATE_HOME', 'state'), ('XDG_CACHE_HOME', 'cache')):  # Isolate standard directories.
        destination = home / name  # Keep state private.
        destination.mkdir(mode=0o700)  # Create owned directories.
        environment[variable] = str(destination)  # Pass explicit state paths.
    (directory / 'temporary').mkdir(mode=0o700)  # Own temporary files.
    return environment, project  # Reuse isolated environment.
#
def session_gate(runner: packages.command_runner, installed: Path, directory: Path, environment: dict[str, str], project: Path) -> dict[str, Any]:  # Exercise installed lifecycle.
    nonce = uuid.uuid4().hex  # Create unpredictable tokens.
    session = 'macos-smoke-' + nonce  # Exclude user sessions.
    ready, release, fixture = directory / 'pane-ready', directory / 'pane-release', directory / 'pane.sh'  # Own lifecycle controls.
    script = b'#!/bin/sh\nset -eu # Fail incomplete fixture commands.\nprintf "%s %s\\n" "$3" "$$" > "$1" # Report the exact nonce and shell PID.\ncount=0 # Bound orphaned fixture lifetime.\nwhile [ ! -e "$2" ] && [ "$count" -lt 180 ]; do # Await only the owned release file.\n  /bin/sleep 1 # Keep the pane alive without busy waiting.\n  count=$((count + 1)) # Advance the finite lifetime budget.\ndone # End on release or timeout.\ntest -e "$2" # Treat an unreleased timeout as failure.\n'  # Use documented CLI.
    packages.write_bytes(fixture, script, 0o700)  # Freeze fixture command.
    client = installed / 'ilium'  # Use installed client.
    pane, server, chain, marker = None, None, [], b''  # Track observed identities.
    try:  # Ensure failure cleanup.
        run_command(runner, 'session-create', [client, 'new-pane', '--session-name', session, '--', '/bin/sh', fixture, ready, release, nonce], environment=environment, cwd=project)  # Create real pane.
        deadline = time.monotonic() + 45  # Bound pane readiness.
        while not os.path.lexists(ready) and time.monotonic() < deadline:  # Observe ready marker.
            time.sleep(0.1)  # Bound readiness polling.
        packages.regular_file(ready, 256)  # Reject marker aliases.
        marker = ready.read_bytes()  # Retain executed response.
        match = re.fullmatch(nonce.encode() + rb' ([0-9]+)\n', marker)  # Check nonce/PID evidence.
        require(match is not None, 'pane readiness proof differs')  # Reject stale markers.
        observed_pane = process_identity(runner, int(match.group(1)))  # Observe claimed PID.
        require(observed_pane is not None and observed_pane['executable'] in ('/bin/sh', '/bin/bash'), 'pane is not the expected live shell')  # Reject arbitrary PIDs.
        arguments = run_command(runner, 'pane-arguments', ['/bin/ps', '-p', str(observed_pane['pid']), '-o', 'args=']).decode('utf-8')  # Bind shell arguments.
        require(str(fixture) in arguments and nonce in arguments, 'pane command custody differs')  # Exclude unrelated shells.
        pane = observed_pane  # Adopt verified shell.
        cursor = pane  # Trace server ancestry.
        for _index in range(16):  # Bound ancestor walk.
            chain.append(cursor)  # Retain native lineage.
            if cursor['executable'] == str(installed / 'ilium-server'):  # Identify installed server.
                server = cursor  # Establish server custody.
                break  # Finish proven ancestry.
            require(cursor['ppid'] > 1 and cursor['ppid'] not in {entry['pid'] for entry in chain}, 'pane has no installed server ancestor')  # Reject cyclic ancestry.
            cursor = process_identity(runner, cursor['ppid'])  # Observe parent identity.
            require(cursor is not None, 'pane ancestry disappeared before observation')  # Require live ancestry.
        require(server is not None, 'installed server ancestry exceeds bound')  # Require server evidence.
        listing = run_command(runner, 'session-list', [client, 'ls'], environment=environment, cwd=project).decode('utf-8')  # List installed sessions.
        require(re.search(r'(?<![A-Za-z0-9_-])' + re.escape(session) + r'(?![A-Za-z0-9_-])', listing), 'created session is not listed')  # Require owned token.
        run_command(runner, 'session-kill', [client, 'kill-session', session], environment=environment, cwd=project)  # Kill exact session.
        deadline = time.monotonic() + 15  # Bound pane shutdown.
        while same_process(process_identity(runner, pane['pid']), pane) and time.monotonic() < deadline:  # Observe recorded pane.
            time.sleep(0.25)  # Bound termination polling.
        require(not same_process(process_identity(runner, pane['pid']), pane), 'kill-session did not stop the actual pane')  # Require completed lifecycle.
        return {'state': 'passed', 'name': session, 'nonce': nonce, 'fixture_sha256': packages.sha(fixture), 'ready_sha256': release_tool.digest(marker), 'ready': marker.decode('ascii'), 'pane': pane, 'server': server, 'ancestry': chain, 'listing': listing, 'pane_stopped_by_kill_session': True}  # Retain lifecycle evidence.
    finally:  # Always perform cleanup.
        if not os.path.lexists(release):  # Release owned fixture.
            packages.write_bytes(release, b'release\n')  # Preserve external controls.
        try:  # Allow ended sessions.
            run_command(runner, 'session-cleanup', [client, 'kill-session', session], allowed=tuple(range(256)), environment=environment, cwd=project, timeout=30, cleanup=True)  # Target created session.
        finally:  # Continue native cleanup.
            stop_installed(runner, installed)  # Stop private installation.
            if pane is not None:  # Require established custody.
                stop_process(runner, pane)  # Recheck before signaling.
#
def embedding_gate(runner: packages.command_runner, installed: Path, directory: Path, environment: dict[str, str], project: Path, wrapper: Path, register: Path, model: Path, audit: dict[str, Any]) -> dict[str, Any]:  # Bound held-process protocol.
    command = [str(Path(sys.executable).resolve()), str(wrapper), '--model-lock', str(register), '--installed-directory', str(installed), '--model', str(model), '--text', 'release embedding acceptance', '--hold-for-native-audit']  # Use reviewed wrapper.
    errors_path = directory / 'embedding.stderr'  # Own inference diagnostics.
    output, pending, proof, observed, child = b'', b'', None, None, None  # Track proof custody.
    deadline = min(runner.deadline, time.monotonic() + 600)  # Bound complete inference.
    with errors_path.open('xb') as errors:  # Preserve prior evidence.
        process = subprocess.Popen(command, cwd=project, env=environment, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors, start_new_session=True)  # Own wrapper group.
        try:  # Clean all failures.
            require(process.stdout is not None and process.stdin is not None, 'embedding pipes unavailable')  # Require bidirectional handshake.
            with selectors.DefaultSelector() as selector:  # Use guarded Darwin readiness.
                selector.register(process.stdout, selectors.EVENT_READ)  # Bound stdout reads.
                ended = False  # Observe complete protocol.
                while not ended:  # Bound post-proof output.
                    require(time.monotonic() < deadline and errors_path.stat().st_size <= packages.max_command_bytes, 'embedding time/stderr bound exceeded')  # Reject unbounded inference.
                    events = selector.select(min(0.25, max(0.001, deadline - time.monotonic())))  # Respect protocol deadline.
                    if not events:  # Await actual inference.
                        continue  # Recheck output growth.
                    block = os.read(process.stdout.fileno(), 65536)  # Read bounded chunk.
                    if not block:  # Handle protocol EOF.
                        ended = True  # Defer result validation.
                        continue  # Reject EOF-only proof.
                    output += block  # Retain exact stdout.
                    pending += block  # Buffer incomplete JSONL.
                    require(len(output) <= 1_000_000, 'embedding stdout bound exceeded')  # Bound aggregate evidence.
                    while b'\n' in pending:  # Parse complete records.
                        line, pending = pending.split(b'\n', 1)  # Preserve partial suffix.
                        record = strict_json(line)  # Reject ambiguous JSON.
                        if record.get('type') != 'embedding-proof':  # Retain wrapper progress.
                            continue  # Admit one proof.
                        require(proof is None, 'duplicate embedding proof')  # Reject duplicate proofs.
                        proof = record  # Retain finite vector.
                        require(proof.get('input') == 'release embedding acceptance' and proof.get('executable_path') == str(installed / 'ilium') and proof.get('binary_sha256') == audit['files']['ilium'], 'embedding executed a different input/client')  # Bind installed executable.
                        child = process_identity(runner, proof.get('ilium_pid'))  # Observe held client.
                        require(child is not None and child['executable'] == str(installed / 'ilium'), 'embedding child is not the installed client')  # Reject foreign executables.
                        cursor = child  # Establish wrapper custody.
                        for _index in range(16):  # Bound ancestry walk.
                            if cursor['ppid'] == process.pid:  # Identify reviewed wrapper.
                                break  # Finish proven custody.
                            require(cursor['ppid'] > 1, 'embedding child is not owned by the wrapper')  # Reject unrelated processes.
                            cursor = process_identity(runner, cursor['ppid'])  # Observe native parents.
                            require(cursor is not None, 'embedding ancestry disappeared')  # Require live custody.
                        else:  # Reject unproven ancestry.
                            raise release_tool.ReleaseError('embedding ancestry exceeds bound')  # Keep inference unqualified.
                        mappings = run_command(runner, 'embedding-vmmap', ['/usr/bin/vmmap', '-w', str(child['pid'])]).decode('utf-8')  # Inspect dyld mappings.
                        loaded = audit_native.validate_process_mapping(child['executable'], mappings, installed / 'ilium', Path(proof.get('loaded_runtime', '')))  # Reject runtime fallback.
                        observed = audit_native.validate_embedding_proof(proof, installed, model, loaded)  # Validate actual inference.
                        require(observed['dimensions'] == 384 and observed['runtime_sha256'] == audit['files'].get(Path(observed['loaded_runtime']).name), 'embedding dimension or final runtime hash differs')  # Bind shape/runtime bytes.
                        observed.update(process=child, native_mappings=mappings, proof=proof)  # Retain checked runtime.
                        process.stdin.write(b'native-audit-observed\n')  # Acknowledge verified mapping.
                        process.stdin.flush()  # Release held client.
            require(not pending and proof is not None and observed is not None, 'embedding stream is incomplete or missing proof')  # Require complete proof.
            process.wait(timeout=max(0.001, min(10, deadline - time.monotonic())))  # Bound wrapper completion.
            require(process.returncode == 0 and errors_path.stat().st_size <= packages.max_command_bytes, 'embedding wrapper failed or exceeded stderr bound')  # Require native success.
            require(not same_process(process_identity(runner, child['pid']), child), 'embedding held client did not exit')  # Verify protocol cleanup.
            observed.update(command=command, wrapper_sha256=packages.sha(wrapper), register_sha256=packages.sha(register), stdout=output.decode('utf-8'), stdout_sha256=release_tool.digest(output), stderr=errors_path.read_text(encoding='utf-8'), state='passed')  # Retain native observations.
            return observed  # Require exited children.
        finally:  # Clean held processes.
            if process.stdin is not None:  # Trigger wrapper cleanup.
                try:  # Handle broken acknowledgements.
                    process.stdin.close()  # Release the held client.
                except BrokenPipeError:  # Handle closed input.
                    pass  # Continue native cleanup.
            if process.poll() is None:  # Limit Popen custody.
                for action in (signal.SIGTERM, signal.SIGKILL):  # Bound wrapper escalation.
                    try:  # Handle wrapper exits.
                        os.killpg(process.pid, action)  # Signal owned group.
                    except ProcessLookupError:  # Handle absent group.
                        pass  # Reap original child.
                    try:  # Bound cleanup waiting.
                        process.wait(timeout=5)  # Reap exact child.
                        break  # Finish group cleanup.
                    except subprocess.TimeoutExpired:  # Bound escalation stages.
                        continue  # Exclude unbounded waits.
                require(process.poll() is not None, 'embedding wrapper survived cleanup')  # Reject persistent processes.
            if process.stdout is not None:  # Always close pipes.
                process.stdout.close()  # Prevent handle leaks.
            stop_installed(runner, installed)  # Clean installed processes.
#
@contextmanager  # Always restore sources.
def hidden_sources(paths: list[Path], directory: Path) -> Iterator[list[dict[str, str]]]:  # Hide payload sources.
    moved: list[dict[str, str]] = []  # Track completed moves.
    directory.mkdir(mode=0o700)  # Own hiding area.
    require(len(paths) == len(set(paths)) and all(not first.is_relative_to(second) for first in paths for second in paths if first != second), 'source hiding roots overlap')  # Preserve restoration order.
    try:  # Restore after failures.
        for index, source in enumerate(paths):  # Hide validated roots.
            destination = directory / str(index)  # Allocate exclusive name.
            packages.commit_directory(source, destination)  # Preserve existing destinations.
            moved.append({'original': str(source), 'hidden': str(destination)})  # Record move custody.
        require(all(not os.path.lexists(path) for path in paths), 'source payload paths remain available')  # Verify source absence.
        yield moved  # Exercise isolated installation.
    finally:  # Require source restoration.
        failures = []  # Attempt every restoration.
        for item in reversed(moved):  # Reverse completed moves.
            try:  # Preserve replacement entries.
                packages.commit_directory(Path(item['hidden']), Path(item['original']))  # Restore without clobbering.
            except (ValueError, OSError, AttributeError) as error:  # Retain uncertain ownership.
                failures.append(str(error))  # Report restoration failures.
        require(not failures, 'source restoration failed: ' + '; '.join(failures))  # Block incomplete restoration.
#
def attach_image(runner: packages.command_runner, image: Path, mount: Path, readonly: bool, owners: tuple[Path, ...]) -> dict[str, str]:  # Attach owned image.
    mount.mkdir(mode=0o700)  # Own empty mountpoint.
    command: list[str | Path] = ['/usr/bin/hdiutil', 'attach', '-nobrowse', '-noautoopen', '-owners', 'on', '-plist', '-mountpoint', mount]  # Preserve mounted ownership.
    if readonly:  # Force readonly inspection.
        command.append('-readonly')  # Preserve source image.
    command.append(image)  # Select validated image.
    document = plistlib.loads(run_command(runner, 'image-attach', command, timeout=120))  # Parse actual attachment.
    require(isinstance(document, dict) and isinstance(document.get('system-entities'), list), 'image attachment evidence is malformed')  # Require device evidence.
    mounted = [item for item in document['system-entities'] if isinstance(item, dict) and item.get('mount-point') == str(mount)]  # Select requested mount.
    require(len(mounted) == 1 and mount.is_mount() and mount.stat().st_dev != mount.parent.stat().st_dev, 'requested image volume was not mounted')  # Exclude host directories.
    matches = [item for item in packages.owned_images(runner, owners) if Path(item['image-path']).resolve() == image.resolve()]  # Bind image custody.
    require(len(matches) == 1, 'attached image ownership is ambiguous')  # Reject label-based custody.
    devices = {entry.get('dev-entry') for entry in matches[0].get('system-entities', []) if isinstance(entry, dict) and re.fullmatch(r'/dev/disk[0-9]+', str(entry.get('dev-entry', '')))}  # Select whole device.
    require(len(devices) == 1, 'attached image has no unique whole disk')  # Prevent broad cleanup.
    return {'image': str(image), 'mount': str(mount), 'device': devices.pop()}  # Retain attachment identity.
#
def detach_image(runner: packages.command_runner, handle: dict[str, str], owners: tuple[Path, ...]) -> None:  # Detach owned image.
    matches = [item for item in packages.owned_images(runner, owners) if Path(item['image-path']).resolve() == Path(handle['image']).resolve()]  # Recheck attachment custody.
    require(len(matches) == 1 and any(entry.get('dev-entry') == handle['device'] for entry in matches[0].get('system-entities', []) if isinstance(entry, dict)), 'image device ownership changed')  # Reject reused devices.
    run_command(runner, 'image-detach', ['/usr/bin/hdiutil', 'detach', handle['device']], timeout=60, cleanup=True)  # Detach without force.
    require(not Path(handle['mount']).is_mount(), 'owned image mount remains active')  # Require actual unmount.
    require(not any(Path(item['image-path']).resolve() == Path(handle['image']).resolve() for item in packages.owned_images(runner, owners)), 'owned image remains attached')  # Require complete detach.
#
def materialize(content: dict[str, bytes], destination: Path, payload_tree: dict[str, Any]) -> None:  # Materialize validated payload.
    destination.mkdir(mode=0o755)  # Require fresh installation.
    destination.chmod(0o755)  # Normalize directory mode.
    for name, data in sorted(content.items()):  # Write admitted files.
        packages.write_bytes(destination / name, data, payload_tree[name]['mode'])  # Preserve audited bytes.
    require(packages.tree_inventory(destination) == payload_tree, 'portable installation differs from audited payload')  # Verify written inventory.
#
def remove_payload(runner: packages.command_runner, installed: Path, payload_tree: dict[str, Any], administrator: bool) -> None:  # Remove stopped installation.
    require(not installed_processes(runner, installed), 'refusing to remove a live installed pair')  # Clean process mappings.
    require(packages.tree_inventory(installed) == payload_tree, 'refusing removal of modified or extra installation files')  # Preserve unexpected files.
    paths = [installed / name for name in sorted(payload_tree)]  # Enumerate audited files.
    if administrator:  # Handle root-owned payload.
        run_command(runner, 'payload-remove', ['/usr/bin/sudo', '-n', '/bin/rm', '--', *paths], cleanup=True)  # Delete explicit paths.
        run_command(runner, 'payload-directory-remove', ['/usr/bin/sudo', '-n', '/bin/rmdir', '--', installed], cleanup=True)  # Require empty directory.
    else:  # Handle user-owned payload.
        for path in paths:  # Remove verified members.
            path.unlink()  # Preserve unrelated paths.
        installed.rmdir()  # Reject remaining extras.
    require(not os.path.lexists(installed), 'owned installed payload remains')  # Verify removal result.
def smoke(arguments: argparse.Namespace) -> dict[str, Any]:  # Qualify all formats.
    workspace, manifest = arguments.workspace.absolute(), arguments.manifest.absolute()  # Preserve terminal aliases.
    require(not arguments.native.is_symlink() and not arguments.packages.is_symlink() and not arguments.build_work.is_symlink(), 'smoke inputs cannot be symlink roots')  # Reject redirected roots.
    native, archive, products, build_work = arguments.native.resolve(strict=True), arguments.archive.resolve(strict=True), arguments.packages.resolve(strict=True), arguments.build_work.resolve(strict=True)  # Resolve explicit inputs.
    target, audit, runtime, binding = packages.load_native(native, archive, manifest, workspace, arguments.tag, arguments.arch, arguments.source_commit)  # Revalidate native custody.
    identity = packages.native_identity(target, arguments.runner_identity)  # Require native host.
    source = workspace.resolve().parent  # Locate source checkout.
    receipt = read_build(products, target, audit, binding)  # Validate corrected build.
    build_receipt_sha256 = packages.sha(products / packages.receipt_name(arguments.arch))  # Bind immutable receipt.
    ownership = packages.read_json(build_work / 'ownership.json')  # Verify work ownership.
    require(ownership.get('schema') == 1 and ownership.get('work') == str(build_work) and ownership.get('output') == str(products) and ownership.get('state') == 'building', 'build work ownership differs')  # Preserve B1 semantics.
    require(packages.tree_inventory(build_work / 'payload') == receipt['payload_tree'] and packages.sha(build_work / target['archive']) == binding['source_archive_sha256'], 'retained build payload/archive differs')  # Bind source paths.
    smoke_root = packages.fresh_path(arguments.root)  # Require private root.
    output = packages.fresh_path(arguments.output)  # Reject existing receipt.
    require(output.parent == products and output.name == packages.smoke_receipt_name(arguments.arch), 'smoke receipt path differs from stable output contract')  # Require explicit output.
    inputs = [native, products, build_work]  # Select hidden roots.
    require(all(not smoke_root.is_relative_to(path) and not path.is_relative_to(smoke_root) for path in (*inputs, source)), 'smoke root overlaps source or build inputs')  # Preserve source ownership.
    require(all(not first.is_relative_to(second) for first in inputs for second in inputs if first != second) and len(set(inputs)) == 3, 'native/build/package roots overlap')  # Require disjoint hiding.
    require(not any(Path(sys.executable).resolve().is_relative_to(path) for path in inputs), 'Python interpreter would be hidden with source inputs')  # Keep wrapper available.
    smoke_sources = {name: packages.sha(source / name) for name in ('release/scripts/smoke_macos_packages.py', 'release/tests/test_macos_packages.py', *animation_gate.SOURCE_FILES)}  # Bind acceptance source.
    require(packages.sha(Path(__file__)) == smoke_sources['release/scripts/smoke_macos_packages.py'], 'loaded smoke script differs from source checkout')  # Reject import substitution.
    for module_name, module_path in (('release_tool.py', Path(release_tool.__file__)), ('audit_native.py', Path(audit_native.__file__)), ('build_macos_packages.py', Path(packages.__file__))):  # Bind actual loaded helpers.
        require(packages.sha(module_path) == binding['source_inputs']['release/scripts/' + module_name], 'loaded packaging module differs from checkout')  # Reject imported policy substitutes.
    smoke_root.mkdir(mode=0o700)  # Own smoke storage.
    runner = packages.command_runner(smoke_root)  # Reuse bounded evidence.
    runner.deadline = time.monotonic() + 2400  # Bound all formats.
    owners = (smoke_root,)  # Own only this invocation's images.
    results: dict[str, Any] = {}  # Require every format.
    installations: list[Path] = []  # Track cleanup destinations.
    try:  # Preserve failure custody.
        actual_commit = run_command(runner, 'source-commit', ['/usr/bin/git', '-C', source, 'rev-parse', 'HEAD']).decode('ascii').strip()  # Observe source revision.
        dirty = run_command(runner, 'source-clean', ['/usr/bin/git', '-C', source, 'status', '--porcelain', '--untracked-files=no'])  # Require clean checkout.
        require(actual_commit == arguments.source_commit and not dirty.strip(), 'smoke checkout is dirty or from another commit')  # Require source agreement.
        containers = smoke_root / 'container-snapshots'  # Own immutable inputs.
        containers.mkdir(mode=0o700)  # Exclude outside writers.
        for name, digest in receipt['packages'].items():  # Freeze container bytes.
            copy_verified(products / name, containers / name, digest)  # Verify bounded copies.
        inputs.append(containers)  # Hide source snapshots.
        assets = smoke_root / 'acceptance-assets'  # Retain isolated test inputs.
        assets.mkdir(mode=0o700)  # Own wrapper/model area.
        wrapper, register, model_directory = assets / 'embedding_acceptance.py', assets / 'embedding-model.json', assets / 'model'  # Preserve reviewed protocol.
        copy_verified(source / 'release/tests/embedding_acceptance.py', wrapper, binding['source_inputs']['release/tests/embedding_acceptance.py'], 0o700)  # Copy source-bound wrapper.
        copy_verified(source / 'release/embedding-model.json', register, binding['source_inputs']['release/embedding-model.json'])  # Preserve model register.
        animation_audit = assets / 'native-audit.json'
        copy_verified(native / 'native-audit.json', animation_audit, binding['native_audit_sha256'])
        require(packages.sha(wrapper) == binding['source_inputs']['release/tests/embedding_acceptance.py'] and packages.sha(register) == binding['source_inputs']['release/embedding-model.json'], 'acceptance asset copy differs')  # Verify copied hashes.
        model_files = packages.read_json(register)['files']  # Use retained inventory.
        packages.verify_tree_hashes(native / 'evidence/model', model_files)  # Reject model extras.
        model_directory.mkdir(mode=0o700)  # Own model destination.
        for name in sorted(model_files):  # Require flat model names.
            release_tool.safe_member_name(name)  # Reject model aliases.
            copy_verified(native / 'evidence/model' / name, model_directory / name, model_files[name])  # Copy pinned input.
        packages.verify_tree_hashes(model_directory, model_files)  # Verify inference inputs.
        model = model_directory / 'model.onnx'  # Use declared model file.
        layout, payload_tree = receipt['layout'], receipt['payload_tree']  # Freeze installed inventory.
        for package_format in packages.formats:  # Require every format.
            directory = smoke_root / package_format  # Isolate format state.
            directory.mkdir(mode=0o700)  # Own lifecycle root.
            environment, project = private_environment(directory)  # Exclude real home.
            package = containers / packages.package_name(arguments.arch, package_format)  # Select bound container.
            installed, handle, host_before = directory / 'installed', None, None  # Choose portable destination.
            metadata: dict[str, Any] = {'format': package_format}  # Retain format evidence.
            if package_format == 'zip':  # Inspect deterministic ZIP.
                zip_target = dict(target, archive=package.name, format='zip')  # Preserve ZIP contract.
                content = release_tool.read_archive(package, zip_target, audit)  # Reject malformed archives.
                release_tool.verify_content(content, audit, audit['version'])  # Require audited bytes.
                materialize(content, installed, payload_tree)  # Write verified payload.
            elif package_format == 'dmg':  # Inspect mounted DMG.
                handle = attach_image(runner, package, directory / 'source-volume', True, owners)  # Require readonly mount.
                mounted = Path(handle['mount'])  # Retain owned filesystem.
                image_tree = {layout['dmg']['prefix']: {'kind': 'directory', 'mode': 0o755}, **{layout['dmg']['prefix'] + '/' + name: entry for name, entry in payload_tree.items()}}  # Require exact folder.
                require(packages.tree_inventory(mounted) == image_tree, 'mounted DMG has missing, changed, or extra payload entries')  # Reject image extras.
                content = {name: (mounted / layout['dmg']['prefix'] / name).read_bytes() for name in payload_tree}  # Copy regular payload.
                release_tool.verify_content(content, audit, audit['version'])  # Bind mounted bytes.
                materialize(content, installed, payload_tree)  # Install outside image.
                metadata['source_image'] = handle.copy()  # Retain device identity.
                detach_image(runner, handle, owners)  # Detach before execution.
                metadata['source_image_detached_before_execution'] = True  # Record actual detach.
                handle = None  # Clear source mount.
            else:  # Preflight before privilege.
                parts = product_parts(package.read_bytes(), layout['pkg']['component'])  # Parse XAR in memory.
                content = parse_cpio(parts['Payload'], payload_tree)  # Validate CPIO inventory.
                release_tool.verify_content(content, audit, audit['version'])  # Preserve final audit.
                metadata.update(validate_package_metadata(parts['Distribution'], parts['PackageInfo'], layout, payload_tree))  # Require script-free destination.
                bom = directory / 'Bom'  # Own parsed BOM.
                packages.write_bytes(bom, parts['Bom'])  # Avoid implicit /tmp writes.
                bom_output = run_command(runner, 'package-bom', ['/usr/bin/lsbom', '-p', 'fMmugs', bom]).decode('utf-8')  # Inspect native metadata.
                validate_bom(bom_output, payload_tree)  # Cross-check BOM inventory.
                metadata.update(bom_sha256=packages.sha(bom), bom_output=bom_output, payload_sha256=release_tool.digest(parts['Payload']))  # Retain internal hashes.
                run_command(runner, 'package-signature-diagnostic', ['/usr/sbin/pkgutil', '--check-signature', package], allowed=tuple(range(256)))  # Record unsigned diagnostics.
                image = directory / 'install-target.dmg'  # Create isolated volume.
                megabytes = max(128, (sum(item['bytes'] for item in payload_tree.values()) + 1_048_575) // 1_048_576 + 96)  # Bound volume capacity.
                run_command(runner, 'target-create', ['/usr/bin/hdiutil', 'create', '-size', str(megabytes) + 'm', '-fs', 'HFS+', '-type', 'UDIF', '-volname', 'Ilium-smoke-' + arguments.arch, image], timeout=120)  # Own writable image.
                handle = attach_image(runner, image, directory / 'target-volume', False, owners)  # Require distinct filesystem.
                mount = Path(handle['mount'])  # Limit Installer target.
                require(not run_command(runner, 'target-receipts-before', ['/usr/sbin/pkgutil', '--volume', mount, '--pkgs']).strip(), 'new target volume already has package receipts')  # Reject preexisting installation.
                host_before = sorted(run_command(runner, 'host-receipts-before', ['/usr/sbin/pkgutil', '--pkgs']).decode('utf-8').splitlines())  # Observe host receipts.
                run_command(runner, 'package-install', ['/usr/bin/sudo', '-n', '/usr/sbin/installer', '-pkg', package, '-target', mount], timeout=300)  # Install on owned volume.
                installed = mount / layout['pkg']['install_location'].lstrip('/')  # Retain exact destination.
                info = plistlib.loads(run_command(runner, 'installed-receipt', ['/usr/sbin/pkgutil', '--volume', mount, '--pkg-info-plist', layout['pkg']['identifier']]))  # Inspect isolated receipt.
                require(isinstance(info, dict) and info.get('pkgid') == layout['pkg']['identifier'] and info.get('pkg-version') == audit['version'], 'installed receipt identity differs')  # Reject stale receipt.
                require(isinstance(info.get('volume'), str) and Path(info['volume']).resolve() == mount.resolve() and str(info.get('install-location', '')).strip('/') == layout['pkg']['install_location'].strip('/'), 'installed receipt escaped the isolated destination')  # Bind target location.
                file_output = run_command(runner, 'installed-receipt-files', ['/usr/sbin/pkgutil', '--volume', mount, '--files', layout['pkg']['identifier']]).decode('utf-8')  # Read registered paths.
                files = [line.removeprefix('./') if line != '.' else line for line in file_output.splitlines()]  # Normalize root spelling.
                require(len(files) == len(set(files)) and set(files) - {'.'} == set(payload_tree), 'installed receipt file inventory differs')  # Reject registered extras.
                metadata.update(target=handle.copy(), installed_receipt=info, installed_receipt_files=file_output, host_receipts_before_sha256=release_tool.digest(json.dumps(host_before).encode('utf-8')))  # Retain Installer evidence.
            installations.append(installed)  # Track owned destination.
            require(packages.tree_inventory(installed) == payload_tree, 'installed payload differs from final audit')  # Verify actual installation.
            if package_format == 'pkg':  # Inspect mounted ownership.
                require(all((installed / name).stat().st_uid == 0 and (installed / name).stat().st_gid == 0 for name in payload_tree), 'PKG ownership is not root:wheel')  # Verify administrator installation.
            packages.inspect_code(installed, target, audit, runtime, runner)  # Recheck installed code.
            with hidden_sources(inputs, directory / 'hidden-inputs') as hidden:  # Hide payload sources.
                versions = {name: release_tool.version_identity(name, run_command(runner, 'installed-version', [installed / name, '--version'], environment=environment, cwd=project).decode('utf-8'), audit['version']) for name in target['executables']}  # Exercise all three executable identities.
                require(versions == audit['binary_versions'], 'installed pair version output differs')  # Require exact versions.
                session = session_gate(runner, installed, directory, environment, project)  # Require native session lifecycle.
                embedding = embedding_gate(runner, installed, directory, environment, project, wrapper, register, model, audit)  # Require observed native inference.
                animation = animation_gate.smoke(
                    argparse.Namespace(workspace=workspace, root=installed, manifest=manifest,
                                       audit=animation_audit, output=directory / 'installed-animation.json',
                                       os='macos', arch=arguments.arch, tag=arguments.tag,
                                       format=package_format), environment=environment,
                    executor=lambda command, child_env, timeout: argparse.Namespace(
                        returncode=0, stdout=run_command(runner, 'installed-animation-' + package_format,
                                                         command, timeout=timeout,
                                                         environment=child_env, cwd=project).decode('utf-8'),
                        stderr=''))
                hiding = [dict(item) for item in hidden]  # Record hidden roots.
            require(not installed_processes(runner, installed), 'format left an installed process alive')  # Clean before removal.
            remove_payload(runner, installed, payload_tree, package_format == 'pkg')  # Remove stopped payload.
            if package_format == 'pkg':  # Exercise receipt removal.
                mount = Path(handle['mount'])  # Use proven target.
                run_command(runner, 'installed-receipt-forget', ['/usr/bin/sudo', '-n', '/usr/sbin/pkgutil', '--volume', mount, '--forget', layout['pkg']['identifier']], cleanup=True)  # Forget isolated receipt.
                require(not run_command(runner, 'target-receipts-after', ['/usr/sbin/pkgutil', '--volume', mount, '--pkgs'], cleanup=True).strip(), 'isolated package receipt remains')  # Verify receipt absence.
                host_after = sorted(run_command(runner, 'host-receipts-after', ['/usr/sbin/pkgutil', '--pkgs'], cleanup=True).decode('utf-8').splitlines())  # Recheck host receipts.
                require(host_after == host_before, 'host package receipt set changed during isolated install')  # Reject host side effects.
                detach_image(runner, handle, owners)  # Detach owned target.
                image = Path(handle['image'])  # Identify detached image.
                packages.regular_file(image, max_payload_bytes + 268_435_456).unlink()  # Remove detached image.
                metadata.update(payload_removed=True, receipt_removed=True, target_detached=True, target_image_removed=True, host_receipts_unchanged=True)  # Retain PKG cleanup.
            else:  # Record portable removal.
                metadata.update(payload_removed=True)  # Verify installation absence.
            results[package_format] = {'state': 'passed', 'installed_directory': str(installed), 'payload_tree': payload_tree, 'versions': versions, 'source_hiding': hiding, 'session': session, 'embedding': embedding, 'animation': animation, 'animation_receipt_sha256': packages.sha(directory / 'installed-animation.json'), 'installation': metadata, 'owned_processes_stopped': True}  # Retain complete evidence.
            packages.emit('progress', operation='format-passed', format=package_format, installed_directory=str(installed), publication_allowed=False)  # Require format completion.
    finally:  # Clean failed acceptance.
        failures = []  # Track cleanup failures.
        for installed in installations:  # Inspect owned destinations.
            try:  # Clean after assertions.
                stop_installed(runner, installed)  # Signal recorded identities.
            except (ValueError, OSError, subprocess.SubprocessError) as error:  # Preserve live installations.
                failures.append(str(error))  # Reject uncertain custody.
        if not failures:  # Preserve live mappings.
            try:  # Reconcile failed attachments.
                packages.detach_owned_images(runner, owners)  # Require image-path custody.
            except (ValueError, OSError, subprocess.SubprocessError) as error:  # Retain uncertain images.
                failures.append(str(error))  # Report incomplete cleanup.
        if failures:  # Block failed cleanup.
            packages.emit('error', operation='smoke-cleanup', root=str(smoke_root), publication_allowed=False, failures=failures)  # Retain diagnostic root.
        require(not failures, 'owned smoke cleanup is incomplete')  # Preserve uncertain trees.
    require(set(results) == set(packages.formats), 'not all mandatory formats passed')  # Require architecture coverage.
    _target, _audit, _runtime, after = packages.load_native(native, archive, manifest, workspace, arguments.tag, arguments.arch, arguments.source_commit)  # Reopen restored custody.
    require(after == binding and read_build(products, target, audit, binding) == receipt, 'source/build inputs changed during smoke')  # Reject input drift.
    require(packages.sha(products / packages.receipt_name(arguments.arch)) == build_receipt_sha256 and all(packages.sha(source / name) == digest for name, digest in smoke_sources.items()), 'acceptance code or build receipt changed')  # Rebind final evidence.
    cleanup = {'state': 'passed', 'sources_restored': True, 'owned_images_detached': True, 'installed_payloads_removed': True, 'package_receipts_removed': True, 'owned_processes_stopped': True, 'work_retained': True}  # Distinguish retained diagnostics.
    result = {'schema': 1, 'state': 'passed', 'publication_allowed': True, 'tag': arguments.tag, 'version': audit['version'], 'arch': arguments.arch, 'target': target['rust_target'], **binding, 'smoke_source_inputs': smoke_sources, 'build_receipt_sha256': build_receipt_sha256, 'packages': receipt['packages'], 'package_bytes': receipt['package_bytes'], 'package_files': audit['files'], 'payload_tree': receipt['payload_tree'], 'native_identity': identity, 'formats': results, 'cleanup': cleanup, 'commands': runner.records, 'container_signing': receipt['container_signing'], 'container_notarization': receipt['container_notarization'], 'credentials_used': False, 'root': str(smoke_root)}  # Bind lifecycle results.
    validate_smoke_receipt(result, receipt, target, audit, binding, build_receipt_sha256, smoke_sources, packages.read_json(source / 'release/embedding-model.json'))
    packages.write_json(output, result)  # Commit qualified receipt.
    packages.emit('artifact', path=str(output), sha256=packages.sha(output), bytes=output.stat().st_size)  # Report actual artifact.
    packages.emit('result', command='smoke', state='passed', publication_allowed=True, receipt=str(output), root=str(smoke_root), packages=receipt['packages'])  # Qualify without publication.
    return result  # Return native evidence.
#
def parser() -> release_tool.JsonArgumentParser:  # Require complete smoke.
    result = release_tool.JsonArgumentParser(description=__doc__, allow_abbrev=False)  # Preserve portable JSONL.
    result.add_argument('--manifest', type=Path, default=root / 'release/targets.toml')  # Use five-target manifest.
    result.add_argument('--workspace', type=Path, default=root / 'Cargo.toml')  # Bind workspace version.
    result.add_argument('--tag', required=True)  # Require exact tag.
    result.add_argument('--arch', required=True, choices=sorted(packages.architectures))  # Select native architecture.
    result.add_argument('--source-commit', required=True)  # Bind full commit.
    result.add_argument('--runner-identity', required=True)  # Require manifest runner.
    result.add_argument('--native', type=Path, required=True)  # Read native artifact.
    result.add_argument('--archive', type=Path, required=True)  # Select audited tarball.
    result.add_argument('--packages', type=Path, required=True)  # Read B1 artifacts.
    result.add_argument('--build-work', type=Path, required=True)  # Identify owned work.
    result.add_argument('--root', type=Path, required=True)  # Require isolated root.
    result.add_argument('--output', type=Path, required=True)  # Name stable receipt.
    return result  # Expose no bypasses.
#
def main(argv: list[str] | None = None) -> int:  # Keep discovery portable.
    try:  # Return structured failures.
        smoke(parser().parse_args(argv))  # Require all lifecycles.
        return 0  # Require committed receipt.
    except (ValueError, OSError, KeyError, TypeError, AttributeError, EOFError, OverflowError, RecursionError, struct.error, zlib.error, zipfile.BadZipFile, xml_tree.ParseError, expat_error, subprocess.SubprocessError, plistlib.InvalidFileException) as error:  # Normalize operation failures.
        packages.emit('error', command='smoke', state='blocked', publication_allowed=False, message=str(error)[:2000])  # Reject incomplete work.
        return 2  # Preserve failure convention.
    except KeyboardInterrupt:  # Keep cancellation unqualified.
        packages.emit('error', command='smoke', state='cancelled', publication_allowed=False)  # Report cancelled state.
        return 130  # Return interruption status.
#
if __name__ == '__main__':  # Guard native execution.
    sys.exit(main())  # Return workflow status.
