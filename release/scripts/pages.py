#!/usr/bin/env python3
"""Build only the six-file static Pages artifact. No network or deployment."""
from __future__ import annotations

import argparse
import html
import json
from pathlib import Path
import re
import shutil
import sys
import tempfile
from html.parser import HTMLParser

# Direct CLI and import use the same checked-in manifest policy.
import release_tool

FILES = frozenset(('index.html', 'install.sh', 'install.ps1', 'manifest.json', '_headers', '404.html'))
SITE_FILES = frozenset(('index.html', '_headers', '404.html'))
SOURCE_ROOT = Path(__file__).resolve().parents[2]
HOST = 'ilium-setup.pages.dev'
REPOSITORY = 'https://github.com/arthurwolf/ilium'
TAG_PATTERN = r'v[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?'


def require(condition, message):
    if not condition:
        raise release_tool.ReleaseError(message)


windows_table = release_tool.windows_table


def checksum_inventory(path, targets):
    content = Path(path).read_bytes()
    checksums = {}
    for line in content.decode('ascii').splitlines():
        match = re.fullmatch(r'([0-9a-f]{64})  ([A-Za-z0-9._-]+)', line)
        require(match is not None, 'malformed release checksum entry')
        require(match[2] not in checksums, 'duplicate release checksum entry')
        checksums[match[2]] = match[1]
    require(set(checksums) == {row['archive'] for row in targets}, 'release checksum inventory must cover exactly five manifest archives')
    return content, checksums


def text_content(content):
    require(len(content) <= 1024 * 1024, 'static resource exceeds 1 MiB')
    text = content.decode('utf-8-sig')
    require(not any(ord(character) < 32 and character not in '\t\r\n' for character in text), 'binary/control content in static resource')
    secret_patterns = (
        r'\b(?:ghp_|github_pat_)[A-Za-z0-9_]{20,}',
        r'\bAKIA[A-Z0-9]{16}\b',
        r'-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----',
        r'(?i)\b(?:CLOUDFLARE_API_TOKEN|API_KEY|API_TOKEN|SECRET_KEY)\s*[:=]\s*[\"\x27]?[A-Za-z0-9_./+-]{12,}',
    )
    require(not any(re.search(pattern, text) for pattern in secret_patterns), 'credential-like literal in static resource')
    return text


class StaticHTML(HTMLParser):
    def handle_starttag(self, tag, attributes):
        require(tag not in {'script', 'iframe', 'object', 'embed', 'form', 'link', 'img', 'audio', 'video', 'base', 'style'}, 'active or external runtime HTML is forbidden')
        for name, value in attributes:
            require(not name.lower().startswith('on'), 'inline event handlers are forbidden')
            require(name not in {'src', 'srcset', 'style'}, 'runtime resources and inline styles are forbidden')
            if name == 'href':
                require(value is not None and (value.startswith('/') and not value.startswith('//') or value.startswith(REPOSITORY + '/')), 'unexpected HTML link origin')
        if tag == 'meta':
            require(not any(name == 'http-equiv' for name, _ in attributes), 'HTML redirects are forbidden')


def validate_html(content):
    text = text_content(content)
    parser = StaticHTML(convert_charrefs=True)
    parser.feed(text)
    parser.close()
    require('<html lang="en">' in text and '<main>' in text, 'HTML requires language and main landmark')
    return text


def parse_headers(text):
    headers = {}
    route = None
    for line in text.splitlines():
        if not line:
            continue
        if not line[0].isspace():
            require(line in {'/*', '/install.sh', '/install.ps1', '/manifest.json'} and line not in headers, 'unexpected or duplicate header route')
            route = line
            headers[route] = {}
            continue
        require(route is not None and ':' in line, 'invalid Pages header syntax')
        name, value = line.strip().split(':', 1)
        require(name not in headers[route], 'duplicate Pages header')
        headers[route][name] = value.strip()
    return headers


def validate_headers(content):
    headers = parse_headers(text_content(content))
    expected = {
        '/*': {'Cache-Control': 'no-store, max-age=0', 'X-Content-Type-Options': 'nosniff', 'Referrer-Policy': 'no-referrer', 'Content-Security-Policy': "default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'", 'X-Frame-Options': 'DENY'},
        '/install.sh': {'Content-Type': 'text/plain; charset=utf-8'},
        '/install.ps1': {'Content-Type': 'text/plain; charset=utf-8'},
        '/manifest.json': {'Content-Type': 'application/json; charset=utf-8'},
    }
    require(headers == expected, 'Pages cache/MIME/security headers differ from contract')


def installer_sources(root, targets):
    installers = {}
    for name, expected in (('install.sh', release_tool.posix_table(targets)), ('install.ps1', windows_table(targets))):
        source = root / 'release' / name
        require(source.is_file() and not source.is_symlink(), 'missing or symlink canonical installer: ' + name)
        content = source.read_bytes()
        text = text_content(content).replace('\r\n', '\n')
        start, end = expected.splitlines()[0], expected.splitlines()[-1]
        require(text.count(start) == 1 and text.count(end) == 1, 'missing/duplicate generated installer table: ' + name)
        actual = start + text.split(start, 1)[1].split(end, 1)[0] + end
        require(actual == expected, 'stale installer target table: ' + name)
        installers[name] = content
    return installers


def site_sources(root, targets, release_tag):
    site = root / 'release/site'
    require(site.is_dir() and not site.is_symlink(), 'missing static site source')
    require({path.name for path in site.iterdir()} == SITE_FILES, 'static source inventory must contain exactly index.html, _headers, 404.html')
    for name in SITE_FILES:
        require((site / name).is_file() and not (site / name).is_symlink(), 'static site sources must be regular files')
    index = validate_html((site / 'index.html').read_bytes())
    require(index.count('@TARGET_ROWS@') == 1 and index.count('@RELEASE@') > 0, 'missing site generation placeholders')
    rows = '\n'.join('<tr><th scope="row">' + html.escape(row['os'] + ' ' + row['arch']) + '</th><td><code>' + html.escape(row['rust_target']) + '</code></td><td>' + html.escape(row['minimum_tested_os']) + '</td></tr>' for row in targets)
    index = index.replace('@TARGET_ROWS@', rows).replace('@RELEASE@', release_tag)
    require('@' not in index, 'unresolved site placeholder')
    not_found = (site / '404.html').read_bytes()
    validate_html(not_found)
    require('Page not found' in not_found.decode() and 'href="/"' in not_found.decode(), 'custom 404 must identify missing resources and link to root')
    headers = (site / '_headers').read_bytes()
    validate_headers(headers)
    return {'index.html': index.encode('utf-8'), '404.html': not_found, '_headers': headers}


def validate_metadata(metadata, targets):
    require(set(metadata) == {'schema_version', 'host', 'repository', 'release_tag', 'targets', 'target_manifest_sha256', 'release_sha256sums_sha256', 'release_sha256sums', 'files'}, 'unexpected deployment manifest fields')
    require(metadata['schema_version'] == 1 and metadata['host'] == HOST and metadata['repository'] == REPOSITORY, 'deployment identity mismatch')
    require(isinstance(metadata['release_tag'], str) and re.fullmatch(TAG_PATTERN, metadata['release_tag']), 'unsafe release tag')
    require(metadata['targets'] == targets, 'deployment target matrix differs from authoritative manifest')
    require(isinstance(metadata['release_sha256sums'], dict) and set(metadata['release_sha256sums']) == {row['archive'] for row in targets}, 'deployment checksum inventory mismatch')
    require(isinstance(metadata['files'], dict) and set(metadata['files']) == FILES - {'manifest.json'}, 'deployment file digest inventory mismatch')
    digests = [metadata['target_manifest_sha256'], metadata['release_sha256sums_sha256'], *metadata['release_sha256sums'].values(), *metadata['files'].values()]
    require(all(isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value) for value in digests), 'invalid deployment digest')


def verify_pages(output, release_sha256sums, *, source_root=None, manifest=None):
    """Recheck inventory, hashes, generated site and current canonical bytes."""
    root = Path(source_root) if source_root is not None else SOURCE_ROOT
    output = Path(output)
    require(output.is_dir() and not output.is_symlink(), 'Pages output must be a regular directory')
    require({path.name for path in output.iterdir()} == FILES, 'Pages output must contain exactly six files')
    for path in output.iterdir():
        require(path.is_file() and not path.is_symlink(), 'Pages output contains non-regular file')
        text_content(path.read_bytes())
    metadata = release_tool.read_json(output / 'manifest.json')
    manifest = Path(manifest) if manifest is not None else root / 'release/targets.toml'
    targets = release_tool.load_targets(manifest)
    sums_content, sums = checksum_inventory(release_sha256sums, targets)
    validate_metadata(metadata, targets)
    require(metadata['target_manifest_sha256'] == release_tool.digest(manifest.read_bytes()), 'deployment manifest source identity differs')
    require(metadata['release_sha256sums_sha256'] == release_tool.digest(sums_content), 'deployment checksum source identity differs')
    require(metadata['release_sha256sums'] == sums, 'deployment archive checksums differ from candidate SHA256SUMS')
    expected = site_sources(root, targets, metadata['release_tag']) | installer_sources(root, targets)
    for name, content in expected.items():
        require((output / name).read_bytes() == content, 'Pages resource differs from canonical source: ' + name)
        require(metadata['files'][name] == release_tool.digest(content), 'Pages manifest resource digest differs: ' + name)
    return metadata


def build_pages(manifest, output, release_tag, release_sha256sums, *, source_root=None):
    """Fail closed, stage fresh output, compare canonical bytes, then publish locally."""
    root = Path(source_root) if source_root is not None else SOURCE_ROOT
    manifest, output = Path(manifest), Path(output)
    require(isinstance(release_tag, str) and re.fullmatch(TAG_PATTERN, release_tag), 'unsafe release tag')
    require(not output.exists() and not output.is_symlink(), 'refusing to replace existing Pages output')
    targets = release_tool.load_targets(manifest)
    sums_content, sums = checksum_inventory(release_sha256sums, targets)
    files = site_sources(root, targets, release_tag) | installer_sources(root, targets)
    metadata = {'schema_version': 1, 'host': HOST, 'repository': REPOSITORY, 'release_tag': release_tag, 'targets': targets, 'target_manifest_sha256': release_tool.digest(manifest.read_bytes()), 'release_sha256sums_sha256': release_tool.digest(sums_content), 'release_sha256sums': sums, 'files': {name: release_tool.digest(content) for name, content in sorted(files.items())}}
    validate_metadata(metadata, targets)
    files['manifest.json'] = (json.dumps(metadata, sort_keys=True, indent=2) + '\n').encode('utf-8')
    output.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix='.ilium-pages-', dir=output.parent))
    try:
        for name, content in files.items():
            (staging / name).write_bytes(content)
        verify_pages(staging, release_sha256sums, source_root=root, manifest=manifest)
        # mkdir prevents replacing a path created concurrently after validation.
        output.mkdir()
        try:
            for name in sorted(FILES):
                # Exclusive creation also protects a file inserted by another
                # writer after mkdir; never overwrite that writer's bytes.
                with (output / name).open('xb') as destination:
                    destination.write((staging / name).read_bytes())
        except OSError:
            # Preserve incomplete output rather than deleting a concurrently
            # modified destination. Its exact inventory must fail verification.
            raise
    finally:
        shutil.rmtree(staging)
    verify_pages(output, release_sha256sums, source_root=root, manifest=manifest)
    return metadata


def main(argv=None):
    parser = release_tool.JsonArgumentParser(description=__doc__, allow_abbrev=False)
    commands = parser.add_subparsers(dest='command', required=True)
    build = commands.add_parser('build-pages', allow_abbrev=False)
    for name in ('manifest', 'output', 'release-sha256sums'):
        build.add_argument('--' + name, required=True, type=Path)
    build.add_argument('--release-tag', required=True)
    build.add_argument('--source-root', type=Path, default=SOURCE_ROOT)
    try:
        arguments = parser.parse_args(argv)
        metadata = build_pages(arguments.manifest, arguments.output, arguments.release_tag, arguments.release_sha256sums, source_root=arguments.source_root)
        release_tool.emit({'type': 'artifact', 'output': str(arguments.output.resolve()), 'files': sorted(FILES), 'release_tag': metadata['release_tag']})
        release_tool.emit({'type': 'result', 'command': 'build-pages', 'state': 'passed', 'output': str(arguments.output.resolve()), 'manifest': str((arguments.output / 'manifest.json').resolve())})
        return 0
    except (ValueError, OSError, UnicodeError, KeyError, TypeError) as error:
        release_tool.emit({'type': 'error', 'command': 'build-pages', 'message': str(error)[:1000]})
        return 1


if __name__ == '__main__':
    sys.exit(main())
