#!/usr/bin/env python3
"""Publish the preview installers to the existing Pages project, without stable-release gates.

The preview is a set of independently published, pinned prereleases. Every
pin is checked against GitHub's published SHA256SUMS and asset digest before
any Pages write. A production write needs a saved baseline and a verified
preview deployment of the identical six-file artifact.
"""
from __future__ import annotations

import argparse
import hashlib
import html
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import uuid

import pages
import release_tool
import release_pipeline as pipeline

ROOT = Path(__file__).resolve().parents[2]


class _AgentHTTPSOnly(pipeline.HTTPSOnly):
    """HTTPS-only opener handler that always names the pipeline as the User-Agent.

    Cloudflare answers urllib's default agent with HTTP 403 (error 1010), which hides
    the authoritative 404 and served bytes the Pages checks read back.
    """
    handler_order = 100

    def https_request(self, request):
        if not request.has_header('User-agent'):
            request.add_header('User-agent', 'ilium-release-pipeline')
        return request


pipeline.HTTPSOnly = _AgentHTTPSOnly
HEX = re.compile(r'[0-9a-f]{64}\Z')
COMMIT = re.compile(r'[0-9a-f]{40}\Z')
VERSION = re.compile(r'0\.1\.1-preview\.[0-9]+\Z')
WINDOWS_PIN = re.compile(r"\$pinnedVersion = '([^']+)'\s*\$pinnedArchive = '([^']+)'\s*\$pinnedHash = '([0-9a-f]{64})'")
FALLBACK = re.compile(r"return '([^']+)'", re.M)
ROUTES = ('/', '/install.sh', '/install.ps1', '/manifest.json', '/ilium-resource-that-must-not-exist')


def require(condition, message):
    if not condition:
        raise release_tool.ReleaseError(message)


def digest(content):
    return hashlib.sha256(content).hexdigest()


def read_json(path):
    return json.loads(Path(path).read_text(encoding='utf-8'))


def write_json(path, record):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('x', encoding='utf-8') as output:
        output.write(json.dumps(record, sort_keys=True, indent=2) + '\n')


def source_commit(root):
    root = Path(root).resolve()
    result = subprocess.run(['git', '-C', str(root), 'rev-parse', 'HEAD'], capture_output=True, text=True, check=True)
    commit = result.stdout.strip()
    require(COMMIT.fullmatch(commit), 'source is not a full Git commit')
    # The exact deployment inputs must be committed. An unrelated dirty file
    # cannot silently alter the six-file artifact.
    paths = ('release/install.sh', 'release/install.ps1', 'release/linux-preview-pins.json',
             'release/targets.toml', 'release/site/index.html', 'release/site/404.html',
             'release/site/_headers', 'release/scripts/preview_pages.py',
             'release/scripts/pages.py', 'release/scripts/release_tool.py',
             'release/scripts/release_pipeline.py', '.github/workflows/preview-pages.yml')
    status = subprocess.run(['git', '-C', str(root), 'status', '--porcelain', '--', *paths],
                            capture_output=True, text=True, check=True)
    require(not status.stdout.strip(), 'preview Pages source inputs must be committed')
    return commit


def installer_pins(root, tag):
    targets = release_tool.load_targets(root / 'release/targets.toml')
    pins_path = root / 'release/linux-preview-pins.json'
    linux_pins = release_tool.load_preview_pins(pins_path, targets)
    pages.verify_preview_pins(root, targets)
    installers = pages.installer_sources(root, targets)
    ps_text = pages.text_content(installers['install.ps1'])
    matches = WINDOWS_PIN.findall(ps_text)
    require(len(matches) == 1, 'PowerShell preview needs one explicit version/archive/hash pin')
    win_version, win_archive, win_hash = matches[0]
    require(VERSION.fullmatch(win_version) and
            int(win_version.rsplit('.', 1)[1]) <= int(tag.rsplit('.', 1)[1]) and
            win_archive == 'ilium-windows-x86_64.zip',
            'PowerShell preview pin must select an existing Windows preview release')
    # The no-stable-release path must resolve to the same pin.
    resolver = ps_text.split('function Resolve-IliumLatestResponse', 1)
    require(len(resolver) == 2, 'PowerShell latest resolver is missing')
    resolver = resolver[1].split('function Get-IliumLatestVersion', 1)[0]
    require(FALLBACK.findall(resolver) == [win_version], 'PowerShell latest fallback differs from its preview pin')
    pins = {name: {'version': version, 'sha256': sha}
            for name, (version, sha) in linux_pins.items()}
    # Preview.7 carries the preview.6 Windows archive byte for byte. The
    # release tag selects the download, while VERSION and executables retain
    # the payload version. Do not apply that exception to a different hash.
    payload_version = win_version
    if win_version == '0.1.1-preview.7':
        require(win_hash == 'c267349717786f9540ea06dd26a1976adcde4b9209340d12848e679c1239cb89',
                'preview.7 Windows payload hash differs from the carried preview.6 archive')
        payload_version = '0.1.1-preview.6'
    pins[win_archive] = {'version': win_version, 'sha256': win_hash, 'payload_version': payload_version}
    return targets, pins, installers


def parse_checksums(content):
    text = content.decode('ascii')
    require(text.endswith('\n') and len(content) <= 16384, 'published SHA256SUMS is malformed or oversized')
    records = {}
    for line in text[:-1].split('\n'):
        match = re.fullmatch(r'([0-9a-f]{64})  (ilium-[A-Za-z0-9._-]+)', line)
        require(match and match[2] not in records, 'published SHA256SUMS has malformed or duplicate entries')
        records[match[2]] = match[1]
    return records


def published_pin_evidence(pins, *, carrier_tag=None, github=pipeline.github, request=pipeline.request):
    """Read published releases; caller can inject a read-only provider in tests."""
    evidence = {}
    release_records = {}
    versions = {pin['version'] for pin in pins.values()}
    if carrier_tag is not None:
        require(carrier_tag.startswith('v') and VERSION.fullmatch(carrier_tag[1:]),
                'unsafe preview carrier release tag')
        versions.add('0.1.1-preview.6')
        versions.add(carrier_tag[1:])
    for version in sorted(versions):
        tag = 'v' + version
        release = github('releases/tags/' + tag)
        require(release.get('tag_name') == tag and release.get('prerelease') is True
                and release.get('draft') is False and release.get('immutable') is True,
                'pin release must be published, immutable and a prerelease: ' + tag)
        assets = {item['name']: item for item in release.get('assets', [])}
        require(len(assets) == len(release.get('assets', [])) and 'SHA256SUMS' in assets,
                'published release asset inventory is incomplete: ' + tag)
        sums_asset = assets['SHA256SUMS']
        sums_digest = sums_asset.get('digest')
        require(sums_digest is None or isinstance(sums_digest, str)
                and re.fullmatch(r'sha256:[0-9a-f]{64}', sums_digest),
                'published checksum asset has malformed provider digest: ' + tag)
        sums_url = sums_asset.get('browser_download_url')
        require(sums_url == pipeline.ORIGIN + '/download/' + tag + '/SHA256SUMS',
                'published checksum URL differs from fixed release: ' + tag)
        content, _headers = request(sums_url, limit=16384)
        require(sums_asset.get('size') is None or len(content) == sums_asset['size'],
                'published checksum size differs from GitHub asset metadata: ' + tag)
        if sums_digest is not None:
            require(digest(content) == sums_digest[7:], 'published checksum bytes differ from GitHub asset digest: ' + tag)
        checksums = parse_checksums(content)
        release_records[version] = {'checksums': checksums, 'assets': assets}
        # The current preview release carries prior verified archives without
        # changing their bytes. Check both each original pin and the carried
        # copy before publishing an installer pointing at either release.
        selected = {name: pin['sha256'] for name, pin in pins.items()
                    if pin['version'] == version or carrier_tag == tag}
        archive_evidence = {}
        for name, sha in selected.items():
            asset = assets.get(name)
            require(checksums.get(name) == sha and asset is not None,
                    'installer pin differs from published SHA256SUMS: ' + tag + '/' + name)
            url = pipeline.ORIGIN + '/download/' + tag + '/' + name
            require(asset.get('browser_download_url') == url,
                    'published archive URL differs from fixed release: ' + tag + '/' + name)
            provider_digest = asset.get('digest')
            require(provider_digest is None or provider_digest == 'sha256:' + sha,
                    'published archive API digest differs from installer pin: ' + tag + '/' + name)
            archive_bytes, _headers = request(url, limit=pipeline.MAX_DOWNLOAD)
            require(asset.get('size') is None or len(archive_bytes) == asset['size'],
                    'published archive size differs from GitHub asset metadata: ' + tag + '/' + name)
            require(digest(archive_bytes) == sha,
                    'downloaded published archive bytes differ from installer pin: ' + tag + '/' + name)
            archive_evidence[name] = {'sha256': sha, 'size': len(archive_bytes),
                                      'provider_digest_available': provider_digest is not None}
        evidence[version] = {'tag': tag, 'release_id': release.get('id'),
                             'sha256sums_sha256': digest(content),
                             'sha256sums_provider_digest_available': sums_digest is not None,
                             'archives': archive_evidence}
    if carrier_tag is not None:
        previous = release_records.get('0.1.1-preview.6')
        current = release_records[carrier_tag[1:]]
        require(previous is not None and carrier_tag != 'v0.1.1-preview.6',
                'combined preview must carry the complete published preview.6 release')
        for label, release in (('preview.6', previous), ('carrier', current)):
            require(set(release['assets']) == set(release['checksums']) | {'SHA256SUMS'},
                    label + ' asset names must exactly match its published checksum inventory')
        previous_checksums = previous['checksums']
        require(set(previous_checksums) <= set(current['checksums']),
                'combined preview omits a preview.6 release asset')
        carried = {}
        for name, sha in sorted(previous_checksums.items()):
            require(current['checksums'][name] == sha,
                    'combined preview changed a preview.6 asset checksum: ' + name)
            sizes = []
            for version, release in (('0.1.1-preview.6', previous), (carrier_tag[1:], current)):
                asset = release['assets'][name]
                asset_digest = asset.get('digest')
                require(asset_digest is None or asset_digest == 'sha256:' + sha,
                        'carried asset API digest differs from published checksum: ' + version + '/' + name)
                url = pipeline.ORIGIN + '/download/v' + version + '/' + name
                require(asset.get('browser_download_url') == url,
                        'carried asset URL differs from release identity: ' + version + '/' + name)
                size = asset.get('size')
                require(isinstance(size, int) and size > 0,
                        'carried asset lacks a positive published size: ' + version + '/' + name)
                sizes.append(size)
                if asset_digest is None and name not in pins:
                    body, _headers = request(url, limit=pipeline.MAX_DOWNLOAD)
                    require(len(body) == size and digest(body) == sha,
                            'carried asset without API digest differs from published checksum: ' + version + '/' + name)
            require(sizes[0] == sizes[1], 'combined preview changed a preview.6 asset size: ' + name)
            carried[name] = {'sha256': sha, 'size': sizes[0]}
        evidence['carry_forward_preview_6'] = {'source_tag': 'v0.1.1-preview.6',
                                                'carrier_tag': carrier_tag, 'assets': carried}
    return evidence


def site_files(root, tag, targets, pins, installers, commit, published):
    # Keep the canonical site's static-only and exact-source-inventory policy,
    # while replacing its five-archive stable marketing for this partial preview.
    pages.site_sources(root, targets, tag)
    available = [row for row in targets if row['archive'] in pins]
    rows = '\n'.join('<tr><th scope="row">' + html.escape(row['os'] + ' ' + row['arch'])
                     + '</th><td><code>' + html.escape(row['rust_target'])
                     + '</code></td><td><a href="' + pages.REPOSITORY + '/releases/tag/v'
                     + html.escape(pins[row['archive']]['version']) + '">'
                     + html.escape(pins[row['archive']]['version']) + '</a></td></tr>' for row in available)
    index = ('<!doctype html>\n<html lang="en"><head><meta charset="utf-8">'
             '<meta name="viewport" content="width=device-width,initial-scale=1">'
             '<title>Install Ilium preview</title></head><body><main>'
             '<h1>Install Ilium preview</h1><p>Published preview installers. Each supported platform uses '
             'its own pinned prerelease; unavailable platforms refuse installation.</p>'
             '<h2>Linux and macOS</h2><p><code>curl --proto \'=https\' --tlsv1.2 -LsSf '
             'https://ilium-setup.pages.dev/install.sh | sh</code></p>'
             '<p><a href="/install.sh">Inspect the POSIX installer</a></p>'
             '<h2>Windows PowerShell</h2><p><code>irm https://ilium-setup.pages.dev/install.ps1 | iex</code></p>'
             '<p><a href="/install.ps1">Inspect the PowerShell installer</a></p>'
             '<h2>Published installer pins</h2><table><thead><tr><th>Platform</th><th>Target</th>'
             '<th>Prerelease</th></tr></thead><tbody>' + rows + '</tbody></table>'
             '<p><a href="/manifest.json">Source and checksum manifest</a>. '
             'These are previews, not a five-platform stable release.</p></main></body></html>\n').encode()
    pages.validate_html(index)
    site = root / 'release/site'
    headers = (site / '_headers').read_bytes()
    not_found = (site / '404.html').read_bytes()
    pages.validate_headers(headers)
    pages.validate_html(not_found)
    files = {'index.html': index, 'install.sh': installers['install.sh'],
             'install.ps1': installers['install.ps1'], '_headers': headers, '404.html': not_found}
    metadata = {'schema_version': 2, 'channel': 'preview', 'host': pages.HOST,
                'repository': pages.REPOSITORY, 'preview_tag': tag, 'source_commit': commit,
                'target_manifest_sha256': digest((root / 'release/targets.toml').read_bytes()),
                'pin_source_sha256': digest((root / 'release/linux-preview-pins.json').read_bytes()),
                'pins': pins, 'published_releases': published,
                'files': {name: digest(content) for name, content in sorted(files.items())}}
    files['manifest.json'] = (json.dumps(metadata, sort_keys=True, indent=2) + '\n').encode()
    return files


def prepare(arguments):
    root = arguments.source.resolve()
    require(VERSION.fullmatch(arguments.tag[1:]) and arguments.tag.startswith('v'), 'unsafe or unsupported preview tag')
    require(pipeline.public_latest_tag() is None, 'preview default installers require no published stable latest release')
    commit = source_commit(root)
    targets, pins, installers = installer_pins(root, arguments.tag)
    published = published_pin_evidence(pins, carrier_tag=arguments.tag)
    files = site_files(root, arguments.tag, targets, pins, installers, commit, published)
    output = arguments.output
    output.mkdir(parents=True)
    for name, content in files.items():
        with (output / name).open('xb') as destination:
            destination.write(content)
    verify_artifact(output)
    pipeline.emit('result', command='prepare-preview-pages', tag=arguments.tag, source_commit=commit,
                  output=str(output.resolve()))


def verify_artifact(directory):
    directory = Path(directory)
    require(directory.is_dir() and not directory.is_symlink()
            and {p.name for p in directory.iterdir()} == pages.FILES,
            'preview Pages artifact must contain exactly six files')
    for path in directory.iterdir():
        require(path.is_file() and not path.is_symlink(), 'non-regular Pages artifact file')
        pages.text_content(path.read_bytes())
    metadata = read_json(directory / 'manifest.json')
    require(metadata.get('schema_version') == 2 and metadata.get('channel') == 'preview'
            and metadata.get('host') == pages.HOST and metadata.get('repository') == pages.REPOSITORY
            and COMMIT.fullmatch(metadata.get('source_commit', '')),
            'preview manifest identity differs')
    require(metadata.get('files') == {name: digest((directory / name).read_bytes())
                                     for name in pages.FILES - {'manifest.json'}},
            'preview manifest file hashes differ')
    pages.validate_html((directory / 'index.html').read_bytes())
    pages.validate_headers((directory / '_headers').read_bytes())
    return metadata


def assert_source_artifact(directory, root):
    """Rebuild from the committed source before each remote write."""
    directory = Path(directory)
    root = Path(root).resolve()
    metadata = verify_artifact(directory)
    commit = source_commit(root)
    require(commit == metadata['source_commit'], 'source commit changed after artifact preparation')
    targets, pins, installers = installer_pins(root, metadata['preview_tag'])
    require(pins == metadata['pins'], 'installer pins changed after artifact preparation')
    published = published_pin_evidence(pins, carrier_tag=metadata['preview_tag'])
    require(published == metadata['published_releases'], 'published preview pin evidence changed')
    expected = site_files(root, metadata['preview_tag'], targets, pins, installers, commit, published)
    require(all((directory / name).read_bytes() == content for name, content in expected.items()),
            'preview Pages artifact differs from committed canonical source')
    return metadata


def live_snapshot(origin):
    result = {}
    for route in ROUTES:
        url = origin.rstrip('/') + route
        try:
            body, headers = pipeline.request(url, headers={'Cache-Control': 'no-cache'}, limit=2_000_000)
            status = 200
        except pipeline.HTTPFailure as error:
            require(error.status == 404, 'unexpected live Pages status for ' + route)
            body, headers = pipeline.missing_response(url)
            status = 404
        result[route] = {'status': status, 'sha256': digest(body),
                         'content_type': headers.get_content_type(),
                         'cache_control': headers.get('Cache-Control', ''),
                         'content_security_policy': headers.get('Content-Security-Policy', ''),
                         'x_content_type_options': headers.get('X-Content-Type-Options', '')}
    return result


def baseline(arguments):
    prefix, project = pipeline.pages_project()
    canonical = project.get('canonical_deployment') or {}
    require(canonical.get('id'), 'Pages production has no rollback deployment identity')
    previous = pipeline.cloudflare(prefix + '/deployments/' + canonical['id'])
    require(previous.get('id') == canonical['id'] and previous.get('environment') == 'production'
            and previous.get('latest_stage', {}).get('status') == 'success',
            'current production is not a successful rollback target')
    write_json(arguments.output, {'schema': 1, 'project': 'ilium-setup',
                                  'production_branch': project['production_branch'],
                                  'canonical_deployment_id': canonical['id'],
                                  'routes': live_snapshot('https://' + pages.HOST)})
    pipeline.emit('result', command='preview-pages-baseline', output=str(arguments.output.resolve()))


def assert_baseline(record):
    _prefix, project = pipeline.pages_project()
    require(record.get('schema') == 1 and record.get('project') == 'ilium-setup'
            and record.get('production_branch') == project.get('production_branch') == 'master'
            and record.get('canonical_deployment_id') == (project.get('canonical_deployment') or {}).get('id'),
            'Pages production changed after baseline')
    require(record.get('routes') == live_snapshot('https://' + pages.HOST),
            'Pages production bytes/status/headers changed after baseline')


def owned_deployment(directory, branch, commit, log):
    """Retain intent before upload; reconcile an uncertain CLI exit by unique marker."""
    marker = 'ilium-preview:' + commit + ':' + uuid.uuid4().hex
    intent = {'schema': 1, 'operation': 'preview-pages-deploy', 'branch': branch,
              'source_commit': commit, 'owner_marker': marker,
              'files': {p.name: digest(p.read_bytes()) for p in directory.iterdir()}}
    write_json(log.with_suffix('.intent.json'), intent)
    failure = None
    try:
        pipeline.logged(['npx', '--yes', pipeline.WRANGLER, 'pages', 'deploy', str(directory.resolve()),
                         '--project-name', 'ilium-setup', '--branch', branch, '--commit-hash', commit,
                         '--commit-message', marker], ROOT, log)
    except (OSError, subprocess.SubprocessError, ValueError) as error:
        failure = str(error)[:500]
    prefix, _project = pipeline.pages_project()
    records = pipeline.cloudflare(prefix + '/deployments?per_page=25')
    matches = [record for record in records
               if record.get('deployment_trigger', {}).get('metadata', {}).get('commit_message') == marker
               and record.get('deployment_trigger', {}).get('metadata', {}).get('commit_hash') == commit
               and record.get('deployment_trigger', {}).get('metadata', {}).get('branch') == branch]
    require(len(matches) == 1, 'Pages upload outcome uncertain; retain intent and inspect marker before retrying: ' + str(failure))
    record = matches[0]
    require(record.get('latest_stage', {}).get('status') == 'success', 'owned Pages deployment did not succeed')
    write_json(log.with_suffix('.readback.json'), {'schema': 1, 'deployment_id': record['id'],
                                                    'url': record['url'], 'owner_marker': marker})
    return record


def verify_preview_receipt(directory, receipt):
    metadata = verify_artifact(directory)
    require(receipt.get('schema') == 1 and receipt.get('state') == 'passed'
            and receipt.get('source_commit') == metadata['source_commit']
            and receipt.get('manifest_sha256') == digest((directory / 'manifest.json').read_bytes())
            and receipt.get('environment') == 'preview', 'preview receipt does not bind this artifact')
    prefix, _project = pipeline.pages_project()
    record = pipeline.cloudflare(prefix + '/deployments/' + receipt['deployment_id'])
    require(record.get('id') == receipt['deployment_id'] and record.get('environment') == 'preview'
            and record.get('url') == receipt['url']
            and record.get('deployment_trigger', {}).get('metadata', {}).get('commit_hash') == metadata['source_commit'],
            'preview deployment provider identity differs')
    pipeline.hosted_bytes(directory, receipt['url'].rstrip('/'))
    return metadata


def deploy_preview(arguments):
    directory = arguments.artifact.resolve()
    metadata = assert_source_artifact(directory, arguments.source)
    require(pipeline.public_latest_tag() is None, 'stable latest appeared before preview deployment')
    pipeline.pages_project()
    branch = 'preview-' + metadata['preview_tag'].replace('.', '-') + '-' + metadata['source_commit'][:12]
    record = owned_deployment(directory, branch, metadata['source_commit'], arguments.log)
    require(record.get('environment') == 'preview', 'preview upload changed production')
    pipeline.hosted_bytes(directory, record['url'].rstrip('/'))
    receipt = {'schema': 1, 'state': 'passed', 'environment': 'preview',
               'source_commit': metadata['source_commit'], 'manifest_sha256': digest((directory / 'manifest.json').read_bytes()),
               'deployment_id': record['id'], 'url': record['url']}
    write_json(arguments.output, receipt)
    pipeline.emit('result', command='deploy-preview-pages', output=str(arguments.output.resolve()), url=record['url'])


def promote(arguments):
    directory = arguments.artifact.resolve()
    preview = read_json(arguments.preview_receipt)
    metadata = verify_preview_receipt(directory, preview)
    require(assert_source_artifact(directory, arguments.source) == metadata,
            'preview artifact changed after preview deployment')
    require(pipeline.public_latest_tag() is None, 'stable latest appeared before preview promotion')
    previous = read_json(arguments.baseline)
    assert_baseline(previous)
    record = owned_deployment(directory, 'master', metadata['source_commit'], arguments.log)
    try:
        require(record.get('environment') == 'production', 'Pages did not produce a production deployment')
        _prefix, project = pipeline.pages_project()
        require((project.get('canonical_deployment') or {}).get('id') == record['id'],
                'Pages canonical production deployment differs')
        pipeline.hosted_bytes(directory, 'https://' + pages.HOST)
    except (ValueError, OSError) as error:
        write_json(arguments.output.with_suffix('.failure.json'),
                   {'schema': 1, 'state': 'production-readback-failed', 'deployment_id': record['id'],
                    'previous_deployment_id': previous['canonical_deployment_id'], 'error': str(error)[:1000]})
        raise release_tool.ReleaseError('production readback failed; use recover with the saved baseline and deployment journal') from error
    write_json(arguments.output, {'schema': 1, 'state': 'passed', 'environment': 'production',
                                  'source_commit': metadata['source_commit'], 'deployment_id': record['id'],
                                  'preview_deployment_id': preview['deployment_id'],
                                  'manifest_sha256': digest((directory / 'manifest.json').read_bytes())})
    pipeline.emit('result', command='promote-preview-pages', output=str(arguments.output.resolve()), deployment_id=record['id'])


def recover(arguments):
    """Read back an uncertain promotion; never roll back through a read/write race.

    Pages rollback has no provider-side canonical-deployment precondition. A
    foreign promotion can occur between a read and POST, so this command only
    records which deployment is live for a separately controlled recovery.
    """
    previous = read_json(arguments.baseline)
    intent_path = arguments.log.with_suffix('.intent.json')
    intent = read_json(intent_path) if intent_path.exists() else None
    if intent is not None:
        require(intent.get('schema') == 1 and intent.get('branch') == 'master'
                and COMMIT.fullmatch(intent.get('source_commit', '')),
                'invalid production deployment intent')
    prefix, project = pipeline.pages_project()
    current_id = (project.get('canonical_deployment') or {}).get('id')
    old_id = previous['canonical_deployment_id']
    live = live_snapshot('https://' + pages.HOST)
    if intent is None:
        state = ('no-upload-intent-baseline-live' if current_id == old_id and live == previous['routes']
                 else 'no-upload-intent-other-production')
    elif current_id == old_id and live == previous['routes']:
        state = 'baseline-still-live-at-readback'
    else:
        records = pipeline.cloudflare(prefix + '/deployments?per_page=25')
        owned = [record for record in records if record.get('id') == current_id
                 and record.get('deployment_trigger', {}).get('metadata', {}).get('commit_message') == intent['owner_marker']]
        state = ('owned-promotion-live-needs-exclusive-recovery' if len(owned) == 1
                 else 'other-production-writer-preserved')
    write_json(arguments.output, {'schema': 1, 'state': state,
                                  'has_upload_intent': intent is not None,
                                  'baseline_deployment_id': old_id,
                                  'current_deployment_id': current_id,
                                  'current_routes': live})
    pipeline.emit('warning', command='recover-preview-pages', state=state,
                  output=str(arguments.output.resolve()))


def parser():
    result = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    commands = result.add_subparsers(dest='command', required=True)
    for name in ('prepare', 'baseline', 'deploy-preview', 'promote', 'recover'):
        command = commands.add_parser(name, allow_abbrev=False)
        command.add_argument('--output', type=Path, required=True)
        if name in ('prepare', 'deploy-preview', 'promote'):
            command.add_argument('--source', type=Path, default=ROOT)
        if name == 'prepare':
            command.add_argument('--tag', required=True)
        if name in ('deploy-preview', 'promote'):
            command.add_argument('--artifact', type=Path, required=True)
            command.add_argument('--log', type=Path, required=True)
        if name in ('promote', 'recover'):
            command.add_argument('--baseline', type=Path, required=True)
        if name == 'promote':
            command.add_argument('--preview-receipt', type=Path, required=True)
        if name == 'recover':
            command.add_argument('--log', type=Path, required=True)
    return result


def main(argv=None):
    try:
        arguments = parser().parse_args(argv)
        {'prepare': prepare, 'baseline': baseline, 'deploy-preview': deploy_preview,
         'promote': promote, 'recover': recover}[arguments.command](arguments)
        return 0
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
        pipeline.emit('error', command='preview-pages', message=str(error)[:1200])
        return 1


if __name__ == '__main__':
    sys.exit(main())
