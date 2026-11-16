#!/usr/bin/env python3
"""Publish preview.7's complete immutable asset bundle, never stable qualification.

The read-only gate binds an exact successful native run to the immutable tag,
checks its retained receipts, and downloads every public release byte. Carried
files must also match a fresh public preview.6 download. Registry reconciliation
and anonymous digest-bound pulls use the existing stable publisher unchanged.
Retries use a fresh --payload directory with the same retained --intent path.
The shared publisher detects conflicting tags but registry fetch/push is not
an atomic conditional create; serialize writers for this preview tag.
"""
from __future__ import annotations

import os
import hashlib
from io import BytesIO
from pathlib import Path
import stat
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request
import zipfile

import build_linux_packages
import build_macos_packages
import build_windows_installers
import preview_qualification as qualification
import preview_pages
import publish_ghcr as registry
import release_pipeline as pipeline
import release_tool

ROOT = Path(__file__).resolve().parents[2]
TAG = qualification.TAG
PREVIOUS_TAG = 'v0.1.1-preview.6'
require = registry.require


def verify_runtime_source():
    """The executing publisher and every local Python helper must exist at HEAD."""
    sources = list((ROOT / 'release/scripts').rglob('*.py'))
    require(all(path.is_file() and not path.is_symlink() for path in sources),
            'preview publisher runtime contains a linked or nonregular Python source')
    paths = sorted(path.relative_to(ROOT).as_posix() for path in sources)
    require('release/scripts/publish_preview_ghcr.py' in paths,
            'preview publisher source is missing')
    tracked = qualification.git('ls-files', '--error-unmatch', '--', *paths, root=ROOT).decode().splitlines()
    require(tracked == paths and not qualification.git(
        'status', '--porcelain', '--untracked-files=all', '--', 'release/scripts', root=ROOT).strip(),
        'preview publisher and runtime helpers must be tracked and unchanged at HEAD')


class NoArtifactRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        return None


def artifact_location(artifact_id, token):
    """Read GitHub's signed ZIP URL without forwarding the API credential."""
    url = f'https://api.github.com/repos/{registry.REPOSITORY}/actions/artifacts/{artifact_id}/zip'
    request = urllib.request.Request(url, headers={
        'Accept': 'application/vnd.github+json', 'Authorization': 'Bearer ' + token,
        'X-GitHub-Api-Version': '2022-11-28', 'User-Agent': 'ilium-preview-ghcr'})
    try:
        with urllib.request.build_opener(NoArtifactRedirect()).open(request, timeout=90):
            raise release_tool.ReleaseError('GitHub native artifact download did not return a signed redirect')
    except urllib.error.HTTPError as error:
        try:
            require(error.code == 302, 'GitHub native artifact ZIP redirect is unavailable')
            location = error.headers.get('Location', '')
        finally:
            error.close()
    parsed = urllib.parse.urlsplit(location)
    require(parsed.scheme == 'https' and parsed.hostname is not None
            and parsed.port in (None, 443) and not parsed.username and not parsed.password
            and not parsed.fragment
            and any(parsed.hostname.endswith(suffix) for suffix in
                    ('.blob.core.windows.net', '.actions.githubusercontent.com')),
            'GitHub native artifact ZIP redirect has an unexpected origin')
    return location


def zip_file_hashes(body):
    """Inventory an artifact without extracting paths or trusting symlink modes."""
    files, names, total = {}, set(), 0
    try:
        with zipfile.ZipFile(BytesIO(body)) as archive:
            members = archive.infolist()
            require(0 < len(members) <= 10000, 'native artifact ZIP entry count is invalid')
            for member in members:
                name = member.filename.rstrip('/') if member.is_dir() else member.filename
                require(name and name not in names and not name.startswith('/') and '\\' not in name
                        and all(part not in ('', '.', '..') for part in name.split('/')),
                        'native artifact ZIP has an unsafe or repeated path')
                names.add(name)
                mode = stat.S_IFMT(member.external_attr >> 16)
                require(mode in ((0, stat.S_IFDIR) if member.is_dir() else (0, stat.S_IFREG)),
                        'native artifact ZIP contains a link or special file')
                if member.is_dir():
                    continue
                total += member.file_size
                require(0 <= member.file_size <= pipeline.MAX_DOWNLOAD and total <= 4 * pipeline.MAX_DOWNLOAD,
                        'native artifact ZIP expanded bytes exceed the custody bound')
                digest = hashlib.sha256()
                with archive.open(member) as source:
                    while block := source.read(1024 * 1024):
                        digest.update(block)
                files[name] = digest.hexdigest()
    except (zipfile.BadZipFile, RuntimeError, NotImplementedError):
        raise release_tool.ReleaseError('native artifact ZIP is corrupt or unsupported') from None
    require(files, 'native artifact ZIP contains no evidence files')
    return files


def verify_artifact_custody(directory, native, source_sha, *, github, request=pipeline.request,
                            locate=artifact_location, token=None):
    """Bind supplied extracted evidence to the exact authenticated artifact ZIPs."""
    token = os.environ.get('GH_TOKEN') if token is None else token
    require(token, 'native artifact custody requires a genuine GitHub API credential')
    ids = native.get('artifact_ids')
    require(isinstance(ids, dict) and set(ids) == set(qualification.ARTIFACTS)
            and all(type(value) is int and value > 0 for value in ids.values())
            and len(set(ids.values())) == len(ids), 'qualified native artifact IDs are missing or repeated')
    directory = Path(directory)
    require(directory.is_dir() and not directory.is_symlink()
            and {path.name for path in directory.iterdir()} == set(qualification.ARTIFACTS.values()),
            'supplied native artifact directory inventory differs from the exact run')
    custody = {}
    for name, artifact_id in ids.items():
        metadata = github('actions/artifacts/' + str(artifact_id))
        require(isinstance(metadata, dict) and isinstance(metadata.get('workflow_run'), dict),
                'native artifact API metadata is incomplete: ' + name)
        digest = metadata.get('digest')
        size = metadata.get('size_in_bytes')
        require(metadata.get('id') == artifact_id and metadata.get('name') == name
                and metadata.get('expired') is False
                and metadata.get('workflow_run', {}).get('id') == native['run_id']
                and metadata.get('workflow_run', {}).get('head_sha') == source_sha
                and isinstance(digest, str) and qualification.HASH.fullmatch(digest.removeprefix('sha256:'))
                and digest.startswith('sha256:')
                and type(size) is int and 0 < size <= pipeline.MAX_DOWNLOAD,
                'native artifact API identity, archive digest or size differs: ' + name)
        # This request has no Authorization header. Only the API-origin request
        # obtains the signed URL, and it explicitly refuses to follow redirects.
        body, _headers = request(locate(artifact_id, token), limit=size)
        require(len(body) == size and 'sha256:' + preview_pages.digest(body) == digest,
                'native artifact ZIP differs from its immutable API digest: ' + name)
        expected = zip_file_hashes(body)
        extracted = directory / qualification.ARTIFACTS[name]
        require(extracted.is_dir() and not extracted.is_symlink(),
                'supplied native artifact folder is missing or linked: ' + name)
        paths = list(extracted.rglob('*'))
        require(all(not path.is_symlink() and (path.is_file() or path.is_dir()) for path in paths),
                'supplied native artifact contains a link or special entry: ' + name)
        actual = {path.relative_to(extracted).as_posix(): qualification.sha(path)
                  for path in paths if path.is_file()}
        require(actual == expected, 'supplied native evidence differs from exact artifact ID: ' + name)
        custody[name] = {'artifact_id': artifact_id, 'archive_sha256': digest[7:], 'files': expected}
    return custody


def inventory():
    """Derive the entire user-download set from the canonical package contracts."""
    targets = release_tool.load_targets(ROOT / 'release/targets.toml')
    files = {target['archive'] for target in targets}
    for target in targets:
        if target['os'] == 'linux':
            files.update(build_linux_packages.package_names(target['arch']))
        elif target['os'] == 'macos':
            files.update(build_macos_packages.package_names(target['arch']))
        elif target['os'] == 'windows':
            files.update(build_windows_installers.INSTALLER_NAMES)
    intel = next(target for target in targets if target['rust_target'] == 'x86_64-apple-darwin')
    new = {intel['archive'], *build_macos_packages.package_names(intel['arch']),
           build_linux_packages.package_name('aarch64', 'rpm'),
           build_linux_packages.package_name('aarch64', 'snap')}
    require(new < files, 'preview new-file inventory is not part of complete downloads')
    return files, new


def download_release(tag, expected, *, github, request, output=None):
    """Download anonymously; require exact API, checksum and actual byte agreement."""
    pipeline.emit('progress', command='publish-preview-ghcr', phase='public-download',
                  tag=tag, files=len(expected) + 1,
                  message='Checking every public preview download against immutable metadata and checksums')
    release = github('releases/tags/' + tag)
    require(release.get('tag_name') == tag and release.get('prerelease') is True
            and release.get('draft') is False and release.get('immutable') is True
            and type(release.get('id')) is int and release['id'] > 0,
            'preview release is not published, immutable and a prerelease: ' + tag)
    rows = release.get('assets')
    require(isinstance(rows, list) and all(isinstance(row, dict) for row in rows),
            'preview release asset metadata is malformed: ' + tag)
    assets = {row.get('name'): row for row in rows}
    require(len(assets) == len(rows) and set(assets) == expected | {'SHA256SUMS'},
            'preview release asset inventory differs: ' + tag)
    base = pipeline.ORIGIN + '/download/' + tag + '/'

    def download(name, expected_hash=None):
        asset = assets[name]
        require(asset.get('browser_download_url') == base + name
                and type(asset.get('size')) is int and asset['size'] > 0,
                'preview asset URL or size differs: ' + tag + '/' + name)
        body, _headers = request(base + name, limit=16384 if name == 'SHA256SUMS' else pipeline.MAX_DOWNLOAD)
        digest = preview_pages.digest(body)
        require(len(body) == asset['size'] and asset.get('digest') in (None, 'sha256:' + digest)
                and (expected_hash is None or digest == expected_hash),
                'preview public asset hash or size differs: ' + tag + '/' + name)
        if output is not None:
            path = output / name
            # A failed attempt cannot leave stale or unqualified inputs for a retry.
            with path.open('xb') as destination:
                destination.write(body)
        return body, digest

    sums, sums_sha = download('SHA256SUMS')
    hashes = preview_pages.parse_checksums(sums)
    require(set(hashes) == expected, 'preview checksums do not match complete downloads: ' + tag)
    for name in sorted(expected):
        download(name, hashes[name])
    pipeline.emit('progress', command='publish-preview-ghcr', phase='public-download-verified',
                  tag=tag, files=len(expected) + 1)
    return {'release_id': release['id'], 'sha256sums_sha256': sums_sha,
            'assets': hashes, 'sizes': {name: assets[name]['size'] for name in expected}}


def prepare(arguments, environment, *, github=pipeline.github, request=pipeline.request):
    """Finish all source/native/public-download gates before registry login."""
    if arguments.operator:
        # A new workflow cannot be dispatched before it reaches the default
        # branch. A real local operator can publish before merge, under the
        # same native/tag/ancestry/bytes gates, without inventing Actions context.
        require(not environment.get('GITHUB_ACTIONS') and not environment.get('GITHUB_EVENT_NAME'),
                'local preview operator mode cannot impersonate an Actions run')
        origin = qualification.git('remote', 'get-url', 'origin', root=ROOT).decode().strip()
        branch = qualification.git('symbolic-ref', '--short', 'HEAD', root=ROOT).decode().strip()
        require(origin in ('https://github.com/arthurwolf/ilium.git',
                           'https://github.com/arthurwolf/ilium',
                           'git@github.com:arthurwolf/ilium.git')
                and branch == qualification.BRANCH,
                'local preview operator requires the canonical Ilium release checkout')
    else:
        require(environment.get('GITHUB_EVENT_NAME') == 'workflow_dispatch'
                and environment.get('GITHUB_REPOSITORY') == registry.REPOSITORY
                and environment.get('GITHUB_REF') == 'refs/heads/' + qualification.BRANCH
                and environment.get('GITHUB_SHA') == arguments.current_sha,
                'preview GHCR publication requires the exact owning-branch dispatch source')
    native = qualification.verify_run(arguments.run_id, arguments.source_sha,
                                      arguments.current_sha, github=github, root=ROOT)
    verify_runtime_source()
    native['artifact_custody'] = verify_artifact_custody(arguments.artifacts, native,
                                                       arguments.source_sha, github=github)
    expected, new = inventory()
    require(not arguments.payload.exists(),
            'preview payload already exists; retry with a fresh --payload directory and retain the same --intent path')
    arguments.payload.mkdir(parents=True, exist_ok=False)
    published = download_release(TAG, expected, github=github, request=request,
                                 output=arguments.payload)
    previous = download_release(PREVIOUS_TAG, expected - new, github=github, request=request)
    require(all(published['assets'][name] == digest
                and published['sizes'][name] == previous['sizes'][name]
                for name, digest in previous['assets'].items()),
            'preview.7 changed carried preview.6 asset bytes')
    # The existing gate checks the native macOS tarball, five new packages,
    # animation/package smoke and the unchanged Windows installer source.
    evidence = {TAG[1:]: {'archives': {
        name: {'sha256': digest} for name, digest in published['assets'].items()}}}

    def public_request(url, *, limit):
        base = pipeline.ORIGIN + '/download/' + TAG + '/'
        if url.startswith(base):
            name = url[len(base):]
            require(name in expected | {'SHA256SUMS'}, 'native gate requested an unexpected public asset')
            body = (arguments.payload / name).read_bytes()
            require(len(body) <= limit, 'public preview file exceeds native gate download bound')
            return body, {}
        return request(url, limit=limit)

    native.update(qualification.verify_artifacts(arguments.artifacts, arguments.source_sha,
                  published=evidence, github=github, request=public_request))
    files = sorted(arguments.payload.iterdir())
    assets = {**published['assets'], 'SHA256SUMS': published['sha256sums_sha256']}
    require(pipeline.asset_hashes(files) == assets, 'downloaded preview payload changed during qualification')
    return {'reference': registry.PACKAGE + ':' + TAG, 'tag': TAG,
            'commit': arguments.source_sha, 'publisher_commit': arguments.current_sha,
            'release_id': published['release_id'], 'files': files, 'assets': assets,
            'preview_qualification': native, 'carried_release': previous}


def publish(plan, client, output, *, intent_path):
    """Retain preview provenance separately; never manufacture a stable receipt."""
    evidence = {key: plan[key] for key in ('tag', 'commit', 'publisher_commit', 'release_id',
                                          'assets', 'preview_qualification', 'carried_release')}
    evidence_path = Path(intent_path).with_name('preview-ghcr-qualification.json')
    require(evidence_path.resolve() not in (Path(output).resolve(), Path(intent_path).resolve()),
            'preview evidence, intent and result paths must differ')
    if evidence_path.exists():
        require(release_tool.read_json(evidence_path) == evidence,
                'existing preview GHCR evidence differs from qualified payload')
    else:
        preview_pages.write_json(evidence_path, evidence)
    return registry.publish(plan, client, output, intent_path=intent_path)


def parser():
    result = release_tool.JsonArgumentParser(description=__doc__, allow_abbrev=False)
    result.add_argument('mode', choices=('verify', 'publish'))
    result.add_argument('--operator', action='store_true',
                        help='explicit local authenticated operation before the workflow reaches the default branch')
    result.add_argument('--run-id', required=True)
    result.add_argument('--source-sha', required=True)
    result.add_argument('--current-sha', required=True)
    result.add_argument('--artifacts', type=Path, required=True)
    result.add_argument('--payload', type=Path, required=True,
                        help='new, empty task-owned directory for exact public release files')
    result.add_argument('--output', type=Path, required=True)
    result.add_argument('--intent', type=Path, required=True)
    result.add_argument('--oras', default='oras')
    return result


def main(argv=None):
    try:
        arguments = parser().parse_args(argv)
        plan = prepare(arguments, os.environ)
        if arguments.mode == 'verify':
            pipeline.write_json(arguments.output, {key: value for key, value in plan.items() if key != 'files'})
            pipeline.emit('result', command='publish-preview-ghcr', state='verified-no-publication',
                          reference=plan['reference'], assets=len(plan['assets']), output=str(arguments.output))
            return 0
        require(os.environ.get('GHCR_TOKEN') and os.environ.get('GITHUB_ACTOR'),
                'preview GHCR publication lacks its scoped credential or actor')
        # Revalidate the immutable tag immediately before registry authentication.
        pipeline.require_remote_tag(TAG, plan['commit'])
        with tempfile.TemporaryDirectory(prefix='preview-ghcr-auth-', dir=arguments.payload.parent) as scratch:
            client = registry.OrasClient(arguments.oras, os.environ['GITHUB_ACTOR'], os.environ['GHCR_TOKEN'], scratch)
            client.login()
            publish(plan, client, arguments.output, intent_path=arguments.intent)
        return 0
    except (ValueError, OSError, KeyError, TypeError) as error:
        pipeline.emit('error', command='publish-preview-ghcr', message=str(error)[:1200])
        return 1


if __name__ == '__main__':
    sys.exit(main())
