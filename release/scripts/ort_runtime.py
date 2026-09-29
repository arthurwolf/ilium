#!/usr/bin/env python3
"""Extract SHA-pinned official shared ONNX assets without executing their code.

The explicit ORT_LIB_LOCATION is essential: ort-sys's prebuilt fallback selects
static libraries even when ORT_PREFER_DYNAMIC_LINK alone is present. Extraction
never certifies native linkage, dependency closure, inference or publication.
"""
from __future__ import annotations
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import stat
import sys
import tarfile
import zipfile
import zlib

from release_tool import JsonArgumentParser, ReleaseError, digest, emit, read_json

ROOT = Path(__file__).resolve().parents[2]
MAX_ARCHIVE = 100_000_000
MAX_UNCOMPRESSED = 1_073_741_824
MAX_MEMBERS = 256
MAX_SELECTED = 134_217_728
TARGETS = {
    'aarch64-apple-darwin',
    'x86_64-pc-windows-msvc',
    'x86_64-unknown-linux-gnu',
    'aarch64-unknown-linux-gnu',
}


def require(condition, message):
    if not condition:
        raise ReleaseError(message)


def sha(path):
    result = hashlib.sha256()
    with path.open('rb') as stream:
        while data := stream.read(65536):
            result.update(data)
    return result.hexdigest()


def member_name(name, directory=False):
    require(isinstance(name, str) and name and len(name) <= 512 and name.isascii(), 'Archive member name is missing/non-ASCII/oversized')
    require('\\' not in name and ':' not in name and not name.startswith('/'), 'Archive member is absolute or platform-ambiguous')
    if directory and name.endswith('/'):
        name = name[:-1]
    parts = name.split('/')
    require(all(part not in ('', '.', '..') and re.fullmatch(r'[A-Za-z0-9_.-]+', part) for part in parts), 'Archive member has unsafe path components')
    return name


def validate_register(register, source, target):
    require(register.get('schema') == 1 and register.get('state') == 'reviewed', 'Shared runtime policy is blocked/unreviewed')
    require(source.get('schema') == 1 and source.get('state') == 'reviewed', 'ORT source register is blocked/unreviewed')
    for field in ('version', 'tag', 'commit'):
        require(register.get(field) == source.get(field), 'Shared asset policy differs from pinned source: ' + field)
    require(register.get('version') == '1.24.2' and register.get('tag') == 'v1.24.2' and register.get('commit') == '058787ceead760166e3c50a0a4cba8a833a6f53f', 'Shared adapter supports only locked ONNX Runtime 1.24.2')
    require(target in TARGETS and target in register.get('assets', {}), 'Shared adapter target is not in the reviewed runtime policy')
    asset = register['assets'][target]
    require(asset.get('github_digest') == 'sha256:' + asset.get('sha256', ''), 'Shared asset hash differs from recorded GitHub digest')
    require(type(asset.get('asset_id')) is int and asset['asset_id'] > 0, 'Official shared asset ID missing')
    require(asset.get('format') in ('zip', 'tar.gz') and re.fullmatch('[0-9a-f]{64}', asset.get('sha256', '')), 'Shared archive format/hash policy invalid')
    require(asset.get('url') == 'https://github.com/microsoft/onnxruntime/releases/download/v1.24.2/' + asset.get('name', ''), 'Shared asset is not from the official pinned release')
    require(type(asset.get('bytes')) is int and 0 < asset['bytes'] <= MAX_ARCHIVE, 'Shared archive exceeds bounded size')
    members, selected = asset.get('members'), asset.get('selected')
    require(isinstance(members, dict) and 0 < len(members) <= MAX_MEMBERS and isinstance(selected, dict) and selected and set(selected) <= set(members), 'Shared member policy incomplete')
    outputs = set()
    root = asset.get('root')
    for name, item in members.items():
        require(member_name(name) == name and name.split('/')[0] == root, 'Shared member policy root/path differs')
        require(item.get('type') in ('file', 'directory', 'symlink') and type(item.get('size')) is int and item['size'] >= 0, 'Shared member type/size policy invalid')
        if item['type'] == 'symlink':
            link = item.get('link')
            require(isinstance(link, str) and link and not link.startswith('/') and '\\' not in link, 'Shared symlink target is unsafe')
    for name, item in selected.items():
        output = member_name(item.get('output'))
        require(output.casefold() not in outputs and members[name]['type'] in ('file', 'symlink') and re.fullmatch('[0-9a-f]{64}', item.get('sha256', '')), 'Selected member output collision/hash/type invalid')
        outputs.add(output.casefold())
    required = {'LICENSE', 'ThirdPartyNotices.txt', 'VERSION_NUMBER', 'GIT_COMMIT_ID'}
    if target == 'aarch64-apple-darwin':
        required |= {'lib/libonnxruntime.1.24.2.dylib', 'lib/libonnxruntime.dylib'}
    elif target in {'x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu'}:
        required |= {'lib/libonnxruntime.so', 'lib/libonnxruntime.so.1', 'lib/libonnxruntime.so.1.24.2', 'lib/libonnxruntime_providers_shared.so'}
    else:
        required |= {'lib/onnxruntime.dll', 'lib/onnxruntime.lib', 'lib/onnxruntime_providers_shared.dll', 'lib/onnxruntime_providers_shared.lib'}
    require(outputs == {name.casefold() for name in required}, 'Selected shared linker/runtime/notice outputs differ from reviewed set')
    return asset


def validate_inventory(entries, asset):
    seen, folded = {}, set()
    total = 0
    for entry in entries:
        name, kind, size = entry[:3]
        link = entry[3] if len(entry) > 3 else None
        name = member_name(name, kind == 'directory')
        require(name not in seen and name.casefold() not in folded, 'Archive has duplicate or case-colliding members')
        require(kind in ('file', 'directory', 'symlink') and type(size) is int and size >= 0, 'Archive contains a link/special or invalid member')
        require(name.split('/')[0] == asset['root'], 'Archive has mixed or unexpected roots')
        require(kind != 'directory' or size == 0, 'Directory member carries data')
        if kind == 'symlink':
            require(isinstance(link, str) and link and not link.startswith('/') and '\\' not in link, 'Archive symlink target is unsafe')
        total += size
        require(total <= MAX_UNCOMPRESSED and len(seen) < MAX_MEMBERS, 'Archive exceeds bounded expanded inventory')
        seen[name] = {'type': kind, 'size': size, **({'link': link} if kind == 'symlink' else {})}
        folded.add(name.casefold())
    require(seen == asset['members'], 'Archive member inventory differs from reviewed upstream asset')
    for name in seen:
        for parent in PurePosixPath(name).parents:
            if str(parent) == '.':
                continue
            require(str(parent) in seen and seen[str(parent)]['type'] == 'directory', 'Archive has absent/file parent directory')
    return seen


def read_selected(archive, asset):
    selected = {}
    if asset['format'] == 'tar.gz':
        with tarfile.open(archive, 'r:gz') as stream:
            infos = stream.getmembers()
            entries = [(info.name, 'directory' if info.isdir() else 'file' if info.isfile() else 'symlink' if info.issym() else 'special', info.size, info.linkname) for info in infos]
            validate_inventory(entries, asset)
            by_name = {info.name: info for info in infos}
            for info in infos:
                if info.name not in asset['selected']:
                    continue
                selected_info = info
                seen_links = set()
                while selected_info.issym():
                    require(selected_info.name not in seen_links, 'Selected symlink chain is cyclic')
                    seen_links.add(selected_info.name)
                    target = str(PurePosixPath(selected_info.name).parent / selected_info.linkname)
                    selected_info = by_name.get(target)
                    require(selected_info is not None, 'Selected symlink target is missing')
                require(selected_info.isfile(), 'Selected symlink target is not a regular member')
                require(0 < selected_info.size <= MAX_SELECTED, 'Selected upstream member exceeds bound')
                source = stream.extractfile(selected_info)
                require(source is not None, 'Selected tar member unreadable')
                data = source.read(selected_info.size + 1)
                require(len(data) == selected_info.size, 'Selected tar member truncated')
                selected[info.name] = data
    else:
        with zipfile.ZipFile(archive) as stream:
            infos = stream.infolist()
            entries = []
            for info in infos:
                mode = stat.S_IFMT(info.external_attr >> 16)
                require(mode in (0, stat.S_IFREG, stat.S_IFDIR) and not (info.external_attr & 0x400) and not (info.flag_bits & 1), 'ZIP member is a link/reparse/special/encrypted entry')
                kind = 'directory' if info.is_dir() else 'file'
                require(mode != stat.S_IFDIR or kind == 'directory', 'ZIP directory type/name differs')
                require(mode != stat.S_IFREG or kind == 'file', 'ZIP file type/name differs')
                entries.append((info.filename, kind, info.file_size))
            validate_inventory(entries, asset)
            for info in infos:
                name = info.filename.rstrip('/')
                if name not in asset['selected']:
                    continue
                require(0 < info.file_size <= MAX_SELECTED, 'Selected upstream member exceeds bound')
                with stream.open(info) as source:
                    data = source.read(info.file_size + 1)
                require(len(data) == info.file_size, 'Selected ZIP member truncated')
                selected[name] = data
    require(set(selected) == set(asset['selected']), 'Selected upstream members are incomplete')
    for name, data in selected.items():
        require(digest(data) == asset['selected'][name]['sha256'], 'Selected member hash differs from upstream policy: ' + name)
    return selected


def extract(arguments):
    for path in (arguments.register, arguments.source_register):
        require(path.is_absolute() and path.is_file() and not path.is_symlink(), 'Explicit absolute plain runtime/source registers required')
    register, source = read_json(arguments.register), read_json(arguments.source_register)
    asset = validate_register(register, source, arguments.target)
    archive = arguments.archive
    require(archive.is_absolute() and archive.is_file() and not archive.is_symlink() and archive.name == asset['name'], 'Archive must be the explicit plain approved asset')
    require(archive.stat().st_size == asset['bytes'] and sha(archive) == asset['sha256'], 'Upstream shared archive differs from approved exact bytes')
    output = arguments.output_directory
    require(output.is_absolute() and not output.exists() and not output.is_symlink(), 'Extraction output must be a new absolute owned directory')
    selected = read_selected(archive, asset)
    by_output = {asset['selected'][name]['output']: data for name, data in selected.items()}
    require(by_output['VERSION_NUMBER'].decode('ascii').strip() == register['version'] and by_output['GIT_COMMIT_ID'].decode('ascii').strip() == register['commit'], 'Shared archive embedded version/source commit differs from approved release')
    for name in ('LICENSE', 'ThirdPartyNotices.txt'):
        require(by_output[name].decode('utf-8').strip(), 'Upstream licence/notices empty or unreadable')
    if arguments.target == 'aarch64-apple-darwin':
        require(by_output['lib/libonnxruntime.dylib'] == by_output['lib/libonnxruntime.1.24.2.dylib'], 'Upstream linker alias does not match versioned runtime')
    output.mkdir(parents=False)
    (output / 'lib').mkdir()
    for name, data in by_output.items():
        destination = output / name
        with destination.open('xb') as target:
            target.write(data)
        destination.chmod(0o644)
    library_directory = str((output / 'lib').resolve())
    environment = {'ORT_LIB_LOCATION': library_directory, 'ORT_LIB_PATH': library_directory, 'ORT_PREFER_DYNAMIC_LINK': '1'}
    receipt = {'schema': 1, 'state': 'extracted-not-qualified', 'publication_allowed': False, 'target': arguments.target, 'tag': register['tag'], 'version': register['version'], 'source_commit': register['commit'], 'asset_url': asset['url'], 'asset_id': asset['asset_id'], 'archive_sha256': asset['sha256'], 'register_sha256': sha(arguments.register), 'source_register_sha256': sha(arguments.source_register), 'files': {name: digest(data) for name, data in by_output.items()}, 'environment': environment, 'runtime_directory': library_directory, 'required_gates': ['native Cargo explicit dynamic linkage', 'native dependency/licence closure', 'real installed inference', 'paired release/package/install acceptance']}
    receipt_path = output / 'ort-runtime-receipt.json'
    with receipt_path.open('x', encoding='utf-8') as stream:
        json.dump(receipt, stream, indent=2, sort_keys=True); stream.write('\n')
    emit({'type': 'artifact', 'path': str(receipt_path.resolve()), 'sha256': sha(receipt_path)})
    emit({'type': 'result', 'command': 'ort-runtime', 'state': receipt['state'], 'publication_allowed': False, 'environment': environment, 'runtime_directory': library_directory, 'target': arguments.target})


def parser():
    result = JsonArgumentParser(description=__doc__, allow_abbrev=False)
    for name in ('register', 'archive', 'output-directory'):
        result.add_argument('--' + name, type=Path, required=True)
    result.add_argument('--source-register', type=Path, default=ROOT / 'release/ort-source.json')
    result.add_argument('--target', required=True)
    return result


def main(argv=None):
    try:
        extract(parser().parse_args(argv))
        return 0
    except (ValueError, OSError, UnicodeError, KeyError, TypeError, AttributeError, tarfile.TarError, zipfile.BadZipFile, RuntimeError, EOFError, zlib.error) as error:
        emit({'type': 'error', 'command': 'ort-runtime', 'state': 'blocked', 'publication_allowed': False, 'error': str(error)[:2000]})
        return 2


if __name__ == '__main__':
    sys.exit(main())
