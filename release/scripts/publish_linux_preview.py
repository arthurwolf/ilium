#!/usr/bin/env python3
"""Verify and publish the Linux x86_64 preview release from qualified artifacts.

The qualification run that produced the artifacts must already have passed on
the exact commit; this script re-checks the bytes, builds one checksum inventory,
and publishes a prerelease that never becomes the latest release. `dry-run`
performs every local check and prints the plan without any GitHub write.
"""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

PREVIEW_TAG = 'v0.1.1-linux.1'
TARBALL = 'ilium-linux-x86_64.tar.gz'
PACKAGES = ('ilium-linux-x86_64.deb', 'ilium-linux-x86_64.rpm', 'ilium-linux-x86_64.AppImage',
            'ilium-linux-x86_64.flatpak', 'ilium-linux-x86_64.snap')
ASSETS = (TARBALL,) + PACKAGES
NOTES = """Linux x86_64 development preview `{tag}`.

Assets: `{assets}`, plus `SHA256SUMS` listing every file above.

Not yet available: Windows, macOS and Linux ARM64 downloads.
Plugins are a development preview and are not supported in this release.
This is a prerelease; it is not the latest release.
"""


def fail(message):
    raise SystemExit('publish-linux-preview: ' + message)


def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            digest.update(block)
    return digest.hexdigest()


def locate(root, name):
    matches = sorted(path for path in root.rglob(name) if path.is_file())
    if len(matches) != 1:
        fail(f'expected exactly one {name} under {root}, found {len(matches)}')
    return matches[0]


def verify_inventory(native_dir, packages_dir, tag):
    if tag != PREVIEW_TAG:
        fail(f'refusing tag {tag!r}; only {PREVIEW_TAG} may be published here')
    tarball = locate(native_dir, TARBALL)
    native_sums = locate(native_dir, 'SHA256SUMS').read_text(encoding='ascii')
    expected_line = f'{sha256(tarball)}  {TARBALL}\n'
    if native_sums != expected_line:
        fail('native SHA256SUMS does not match the qualified tarball bytes')
    files = {TARBALL: tarball}
    for name in PACKAGES:
        files[name] = locate(packages_dir, name)
    for name, path in files.items():
        if path.stat().st_size == 0:
            fail(f'{name} is empty')
    inventory = ''.join(f'{sha256(files[name])}  {name}\n' for name in ASSETS)
    return files, inventory


def github(arguments, payload=None, method=None):
    command = ['gh', 'api', *arguments]
    if method is not None:
        command += ['-X', method]
    if payload is not None:
        command += ['--input', '-']
    result = subprocess.run(command, input=None if payload is None else json.dumps(payload),
                            text=True, capture_output=True, check=False)
    if result.returncode != 0:
        fail('GitHub request failed: ' + ' '.join(arguments[:2]) + ' ' + result.stderr.strip()[:300])
    return json.loads(result.stdout) if result.stdout.strip() else {}


def publish(files, inventory, commit, repository, work):
    sums_path = work / 'SHA256SUMS'
    sums_path.write_text(inventory, encoding='ascii')
    existing = [row for row in github(['--paginate', f'repos/{repository}/releases'])
                if row.get('tag_name') == PREVIEW_TAG]
    if existing:
        fail(f'a release for {PREVIEW_TAG} already exists; refusing to overwrite it')
    body = NOTES.format(tag=PREVIEW_TAG, assets=', '.join(ASSETS))
    release = github([f'repos/{repository}/releases'], {
        'tag_name': PREVIEW_TAG, 'target_commitish': commit, 'name': 'Ilium ' + PREVIEW_TAG + ' (Linux preview)',
        'body': body, 'draft': True, 'prerelease': True})
    release_id = release['id']
    upload = [str(path) for path in files.values()] + [str(sums_path)]
    subprocess.run(['gh', 'release', 'upload', PREVIEW_TAG, '--repo', repository, *upload], check=True)
    readback = github([f'repos/{repository}/releases/{release_id}'])
    remote = {row['name']: row.get('digest', '') for row in readback.get('assets', [])}
    local = {name: 'sha256:' + sha256(path) for name, path in files.items()}
    for name, digest in local.items():
        if remote.get(name) != digest:
            fail(f'uploaded {name} digest {remote.get(name)!r} differs from local {digest}')
    if remote.get('SHA256SUMS') != 'sha256:' + sha256(sums_path):
        fail('uploaded SHA256SUMS digest differs from local inventory')
    github([f'repos/{repository}/releases/{release_id}'],
           {'draft': False, 'prerelease': True, 'make_latest': 'false'}, method='PATCH')
    final = github([f'repos/{repository}/releases/{release_id}'])
    if final.get('draft') is not False or final.get('prerelease') is not True:
        fail('published release is not a prerelease, or is still a draft')
    print(json.dumps({'type': 'result', 'command': 'publish-linux-preview', 'state': 'passed',
                      'tag': PREVIEW_TAG, 'commit': commit, 'release_id': release_id,
                      'url': final.get('html_url'), 'assets': sorted(local)}))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('mode', choices=['dry-run', 'publish'])
    parser.add_argument('--tag', required=True)
    parser.add_argument('--commit', required=True, help='full 40-character commit the qualification passed on')
    parser.add_argument('--native-directory', type=Path, required=True)
    parser.add_argument('--packages-directory', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    arguments = parser.parse_args(argv)
    if not re.fullmatch(r'[0-9a-f]{40}', arguments.commit):
        fail('commit must be a full lowercase 40-character SHA')
    arguments.work.mkdir(parents=True, exist_ok=True)
    files, inventory = verify_inventory(arguments.native_directory, arguments.packages_directory, arguments.tag)
    if arguments.mode == 'dry-run':
        print(json.dumps({'type': 'result', 'command': 'publish-linux-preview', 'state': 'dry-run-passed',
                          'tag': arguments.tag, 'commit': arguments.commit, 'assets': sorted(files)}))
        print(inventory, end='')
        return 0
    repository = os.environ.get('GITHUB_REPOSITORY') or fail('GITHUB_REPOSITORY is not set')
    publish(files, inventory, arguments.commit, repository, arguments.work)
    return 0


if __name__ == '__main__':
    sys.exit(main())
