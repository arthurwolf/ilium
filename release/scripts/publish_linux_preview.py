#!/usr/bin/env python3
"""Verify and publish the Linux preview release from qualified artifacts.

The qualification run that produced the artifacts must already have passed on
the exact commit; this script re-checks the bytes, builds one checksum inventory,
and publishes a prerelease that never becomes the latest release. `dry-run`
performs every local check and prints the plan without any GitHub write.

`publish` creates the x86_64 release and refuses to overwrite an existing one.
`append-aarch64` (and its writeless `append-aarch64-dry-run`) adds the verified
ARM64 assets to that already published release: it never replaces or deletes an
asset, refuses when any ARM64 asset already exists, and rewrites only
`SHA256SUMS`, keeping every existing line byte for byte and appending the new ones.
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
AARCH64_TARBALL = 'ilium-linux-aarch64.tar.gz'
# Verified ARM64 formats must all be present; the rest are uploaded only when qualified.
AARCH64_REQUIRED_PACKAGES = ('ilium-linux-aarch64.deb', 'ilium-linux-aarch64.AppImage')
AARCH64_OPTIONAL_PACKAGES = ('ilium-linux-aarch64.rpm', 'ilium-linux-aarch64.flatpak', 'ilium-linux-aarch64.snap')
AARCH64_ASSETS = (AARCH64_TARBALL,) + AARCH64_REQUIRED_PACKAGES + AARCH64_OPTIONAL_PACKAGES
SUMS_NAME = 'SHA256SUMS'
SUMS_LINE = re.compile(r'([0-9a-f]{64})  ([A-Za-z0-9._-]+)')
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


def verify_aarch64_inventory(native_dir, packages_dir, tag):
    """Locate and byte-check the ARM64 set; returns {asset name: path} for the assets present."""
    if tag != PREVIEW_TAG:
        fail(f'refusing tag {tag!r}; only {PREVIEW_TAG} may be extended here')
    files = {AARCH64_TARBALL: locate(native_dir, AARCH64_TARBALL)}
    native_sums = list(native_dir.rglob(SUMS_NAME))
    if native_sums:  # A native checksum file, when present, must describe exactly the tarball.
        if len(native_sums) != 1 or native_sums[0].read_text(encoding='ascii') != f'{sha256(files[AARCH64_TARBALL])}  {AARCH64_TARBALL}\n':
            fail('native SHA256SUMS does not match the qualified ARM64 tarball bytes')
    for name in AARCH64_REQUIRED_PACKAGES:
        files[name] = locate(packages_dir, name)
    for name in AARCH64_OPTIONAL_PACKAGES:
        if any(path.is_file() for path in packages_dir.rglob(name)):
            files[name] = locate(packages_dir, name)
    for name, path in files.items():
        if path.stat().st_size == 0:
            fail(f'{name} is empty')
    return files


def merge_checksums(existing_text, files):
    """Append ARM64 lines to the published SHA256SUMS, leaving every existing line untouched."""
    lines = existing_text.splitlines()
    names = []
    for line in lines:
        match = SUMS_LINE.fullmatch(line)
        if match is None:
            fail('published SHA256SUMS contains a malformed line')
        names.append(match[2])
    if len(set(names)) != len(names):
        fail('published SHA256SUMS contains a duplicate entry')
    clash = sorted(set(names) & set(files))
    if clash:
        fail('published SHA256SUMS already lists ' + ', '.join(clash) + '; refusing to overwrite')
    missing = sorted(set(ASSETS) - set(names))
    if missing:
        fail('published SHA256SUMS lacks x86_64 entries ' + ', '.join(missing))
    return ''.join(line + '\n' for line in lines) + ''.join(f'{sha256(files[name])}  {name}\n' for name in AARCH64_ASSETS if name in files)


def release_for_tag(repository):
    rows = [row for row in github(['--paginate', f'repos/{repository}/releases']) if row.get('tag_name') == PREVIEW_TAG]
    if len(rows) != 1:
        fail(f'expected exactly one release for {PREVIEW_TAG}, found {len(rows)}')
    release = rows[0]
    if release.get('draft') or not release.get('prerelease'):
        fail('the preview release must be a published prerelease before ARM64 assets are added')
    return release


def append_plan(files, release, existing_sums_text):
    """Pure decision step shared by dry-run and upload: validates, returns the merged checksum text."""
    remote_names = {row['name'] for row in release.get('assets', [])}
    clash = sorted(remote_names & set(files))
    if clash:
        fail('release already holds ' + ', '.join(clash) + '; refusing to overwrite')
    return merge_checksums(existing_sums_text, files)


def download_sums(release, work):
    row = next((row for row in release.get('assets', []) if row['name'] == SUMS_NAME), None)
    if row is None:
        fail('the published release has no SHA256SUMS')
    target = work / 'published'
    target.mkdir(parents=True, exist_ok=True)
    subprocess.run(['gh', 'release', 'download', PREVIEW_TAG, '--repo', os.environ['GITHUB_REPOSITORY'],
                    '--pattern', SUMS_NAME, '--dir', str(target), '--clobber'], check=True)
    return (target / SUMS_NAME).read_text(encoding='ascii')


def append_aarch64(files, repository, work, dry_run):
    release = release_for_tag(repository)
    existing = download_sums(release, work)
    merged = append_plan(files, release, existing)
    x86_before = {row['name']: row.get('digest', '') for row in release['assets'] if row['name'] in ASSETS}
    if dry_run:
        return merged, sorted(files)
    sums_path = work / SUMS_NAME
    sums_path.write_text(merged, encoding='ascii')
    # New assets are uploaded without --clobber so an existing asset can never be replaced;
    # only SHA256SUMS is clobbered, and only after the new assets are in place.
    subprocess.run(['gh', 'release', 'upload', PREVIEW_TAG, '--repo', repository, *map(str, files.values())], check=True)
    subprocess.run(['gh', 'release', 'upload', PREVIEW_TAG, '--repo', repository, '--clobber', str(sums_path)], check=True)
    readback = github([f'repos/{repository}/releases/{release["id"]}'])
    remote = {row['name']: row.get('digest', '') for row in readback.get('assets', [])}
    for name, path in files.items():
        if remote.get(name) != 'sha256:' + sha256(path):
            fail(f'uploaded {name} digest {remote.get(name)!r} differs from local')
    if remote.get(SUMS_NAME) != 'sha256:' + sha256(sums_path):
        fail('uploaded SHA256SUMS digest differs from the merged inventory')
    for name, digest in x86_before.items():
        if remote.get(name) != digest:
            fail(f'x86_64 asset {name} changed during the ARM64 upload')
    return merged, sorted(files)


def run_append(arguments):
    dry_run = arguments.mode.endswith('dry-run')
    files = verify_aarch64_inventory(arguments.native_directory, arguments.packages_directory, arguments.tag)
    repository = os.environ.get('GITHUB_REPOSITORY') or fail('GITHUB_REPOSITORY is not set')
    merged, assets = append_aarch64(files, repository, arguments.work, dry_run)
    print(json.dumps({'type': 'result', 'command': 'publish-linux-preview', 'state': 'append-dry-run-passed' if dry_run else 'append-passed',
                      'tag': arguments.tag, 'commit': arguments.commit, 'assets': assets}))
    print(merged, end='')
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('mode', choices=['dry-run', 'publish', 'append-aarch64-dry-run', 'append-aarch64'])
    parser.add_argument('--tag', required=True)
    parser.add_argument('--commit', required=True, help='full 40-character commit the qualification passed on')
    parser.add_argument('--native-directory', type=Path, required=True)
    parser.add_argument('--packages-directory', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    arguments = parser.parse_args(argv)
    if not re.fullmatch(r'[0-9a-f]{40}', arguments.commit):
        fail('commit must be a full lowercase 40-character SHA')
    arguments.work.mkdir(parents=True, exist_ok=True)
    if arguments.mode.startswith('append-aarch64'):
        return run_append(arguments)
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
