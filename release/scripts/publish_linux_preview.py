"""Verify and publish a Linux preview release from qualified artifacts.

Each architecture has its own immutable prerelease: x86_64 in `v0.1.1-linux.1`
and ARM64 in `v0.1.1-linux.2` (GitHub makes a published release immutable, so
assets can never be added to an existing one). The qualification run that
produced the artifacts must already have passed on the exact commit; this script
re-checks the bytes, builds one checksum inventory for the tag's architecture,
and publishes a prerelease that never becomes the latest release. The tag selects
the architecture; it never replaces or deletes an existing release or asset.

Modes: `dry-run` performs every local check and prints the plan without any
network access; `github-dry-run` additionally performs GitHub reads only (the
release and tag must not exist, the commit must); `publish` creates a draft,
uploads the assets, verifies every digest by readback and only then publishes it;
`check-versions` verifies that ARM64 artifacts carry the tag's version.
"""

import argparse
import hashlib
import io
import json
import re
import shutil
import subprocess
import sys
import tarfile
from pathlib import Path

PREVIEW_TAG = 'v0.1.1-linux.1'
TARBALL = 'ilium-linux-x86_64.tar.gz'
PACKAGES = ('ilium-linux-x86_64.deb', 'ilium-linux-x86_64.rpm', 'ilium-linux-x86_64.AppImage',
            'ilium-linux-x86_64.flatpak', 'ilium-linux-x86_64.snap')
ASSETS = (TARBALL,) + PACKAGES
AARCH64_TAG = 'v0.1.1-linux.2'
# The ARM64 release is built from this exact commit (the x86_64 source plus the version bump only).
AARCH64_COMMIT = 'f69e1c94cf527d9394de2970561874e4ca68bb5a'
AARCH64_TARBALL = 'ilium-linux-aarch64.tar.gz'
# Four of six ARM64 formats are qualified; rpm and Snap are deliberately not published.
AARCH64_PACKAGES = ('ilium-linux-aarch64.deb', 'ilium-linux-aarch64.AppImage', 'ilium-linux-aarch64.flatpak')
AARCH64_ASSETS = (AARCH64_TARBALL,) + AARCH64_PACKAGES
SUMS_NAME = 'SHA256SUMS'
NOTES_DIRECTORY = Path(__file__).resolve().parents[1] / 'notes'
NOTES = """Linux x86_64 development preview `{tag}`.

Assets: `{assets}`, plus `SHA256SUMS` listing every file above.

Not yet available: Windows, macOS and Linux ARM64 downloads here (Linux ARM64 is published separately in `v0.1.1-linux.2`).
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


def deb_version(path):
    """Return the Version field of a .deb without extracting it (pure Python, dpkg-deb as a fallback)."""
    data = path.read_bytes()
    if data[:8] == b'!<arch>\n':
        offset = 8
        while offset + 60 <= len(data):
            header = data[offset:offset + 60]
            name, size = header[:16].decode('ascii').strip().rstrip('/'), int(header[48:58])
            body = data[offset + 60:offset + 60 + size]
            offset += 60 + size + (size & 1)
            if name.startswith('control.tar') and not name.endswith('.zst'):
                with tarfile.open(fileobj=io.BytesIO(body), mode='r:*') as control:
                    member = next((m for m in control.getmembers() if m.name.lstrip('./') == 'control'), None)
                    if member is not None:
                        for line in control.extractfile(member).read().decode('utf-8').splitlines():
                            if line.startswith('Version:'):
                                return line.split(':', 1)[1].strip()
    if shutil.which('dpkg-deb'):
        result = subprocess.run(['dpkg-deb', '-f', str(path), 'Version'], text=True, capture_output=True, check=False)
        if result.returncode == 0 and result.stdout.strip():
            return result.stdout.strip()
    fail(f'cannot read the Version of {path.name}')

def tarball_version(path):
    with tarfile.open(path, 'r:gz') as archive:
        members = [m for m in archive.getmembers() if m.isfile() and re.fullmatch(r'(?:[^/]+/)?VERSION', m.name)]
        if len(members) != 1:
            fail(f'{path.name} must contain exactly one VERSION file, found {len(members)}')
        return archive.extractfile(members[0]).read().decode('utf-8').strip()

def verify_versions(files, tag):
    """Every versioned ARM64 artifact must carry the tag's version (deb uses ~ for the prerelease dash)."""
    version = tag[1:]
    found = tarball_version(files[AARCH64_TARBALL])
    if found != version:
        fail(f'{AARCH64_TARBALL} VERSION is {found!r}, expected {version!r}')
    expected_deb = version.replace('-', '~')
    found = deb_version(files['ilium-linux-aarch64.deb'])
    if found != expected_deb:
        fail(f'ilium-linux-aarch64.deb Version is {found!r}, expected {expected_deb!r}')

def verify_inventory(native_dir, packages_dir, tag):
    """Locate and byte-check the asset set of the tag's architecture; returns (files, SHA256SUMS text)."""
    if tag == PREVIEW_TAG:
        tarball_name, assets = TARBALL, ASSETS
    elif tag == AARCH64_TAG:
        tarball_name, assets = AARCH64_TARBALL, AARCH64_ASSETS
    else:
        fail(f'refusing tag {tag!r}; only {PREVIEW_TAG} and {AARCH64_TAG} may be published here')
    tarball = locate(native_dir, tarball_name)
    native_sums = list(native_dir.rglob(SUMS_NAME))
    expected_line = f'{sha256(tarball)}  {tarball_name}\n'
    if tag == PREVIEW_TAG or native_sums:  # The ARM64 native step may omit its own checksum file.
        if len(native_sums) != 1 or native_sums[0].read_text(encoding='ascii') != expected_line:
            fail('native SHA256SUMS does not match the qualified tarball bytes')
    files = {tarball_name: tarball}
    for name in assets[1:]:
        files[name] = locate(packages_dir, name)
    for name, path in files.items():
        if path.stat().st_size == 0:
            fail(f'{name} is empty')
    stray = sorted(path.name for path in packages_dir.rglob('ilium-linux-*') if path.is_file() and path.name not in assets)
    if tag == AARCH64_TAG and stray:
        fail('refusing unexpected ARM64 files ' + ', '.join(stray) + '; rpm and Snap are not published')
    if tag == AARCH64_TAG:
        verify_versions(files, tag)
    inventory = ''.join(f'{sha256(files[name])}  {name}\n' for name in assets)
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


def release_body(tag, assets):
    if tag == AARCH64_TAG:
        return (NOTES_DIRECTORY / f'{tag}.md').read_text(encoding='utf-8')
    return NOTES.format(tag=tag, assets=', '.join(assets))

def release_title(tag):
    return 'Ilium ' + tag + (' (Linux ARM64 preview)' if tag == AARCH64_TAG else ' (Linux preview)')

def existing_releases(tag, repository):
    return [row for row in github(['--paginate', f'repos/{repository}/releases']) if row.get('tag_name') == tag]

def github_dry_run_checks(tag, commit, repository):
    """GitHub reads only: the release and tag must not exist yet and the commit must."""
    if existing_releases(tag, repository):
        fail(f'a release for {tag} already exists; refusing to overwrite it')
    if github([f'repos/{repository}/git/matching-refs/tags/{tag}']):
        fail(f'tag {tag} already exists on GitHub; refusing to reuse it')
    found = github([f'repos/{repository}/commits/{commit}'])
    if found.get('sha') != commit:
        fail(f'commit {commit} is not on GitHub')

def publish(files, inventory, commit, repository, work, tag):
    sums_path = work / SUMS_NAME
    sums_path.write_text(inventory, encoding='ascii')
    if existing_releases(tag, repository):
        fail(f'a release for {tag} already exists; refusing to overwrite it')
    body = release_body(tag, list(files))
    # The release stays a draft while assets are uploaded: a published release is immutable.
    release = github([f'repos/{repository}/releases'], {
        'tag_name': tag, 'target_commitish': commit, 'name': release_title(tag),
        'body': body, 'draft': True, 'prerelease': True})
    release_id = release['id']
    upload = [str(path) for path in files.values()] + [str(sums_path)]
    subprocess.run(['gh', 'release', 'upload', tag, '--repo', repository, *upload], check=True)
    readback = github([f'repos/{repository}/releases/{release_id}'])
    remote = {row['name']: row.get('digest', '') for row in readback.get('assets', [])}
    local = {name: 'sha256:' + sha256(path) for name, path in files.items()}
    for name, digest in local.items():
        if remote.get(name) != digest:
            fail(f'uploaded {name} digest {remote.get(name)!r} differs from local {digest}')
    if remote.get(SUMS_NAME) != 'sha256:' + sha256(sums_path):
        fail('uploaded SHA256SUMS digest differs from local inventory')
    if sorted(remote) != sorted([*local, SUMS_NAME]):
        fail('the draft release holds assets other than the expected set')
    github([f'repos/{repository}/releases/{release_id}'],
           {'draft': False, 'prerelease': True, 'make_latest': 'false'}, method='PATCH')
    final = github([f'repos/{repository}/releases/{release_id}'])
    if final.get('draft') is not False or final.get('prerelease') is not True:
        fail('published release is not a prerelease, or is still a draft')
    print(json.dumps({'type': 'result', 'command': 'publish-linux-preview', 'state': 'passed',
                      'tag': tag, 'commit': commit, 'release_id': release_id,
                      'url': final.get('html_url'), 'assets': sorted(local)}))

def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('mode', choices=['dry-run', 'github-dry-run', 'publish', 'check-versions'])
    parser.add_argument('--tag', required=True)
    parser.add_argument('--commit', help='full 40-character commit the qualification passed on (not needed for check-versions)')
    parser.add_argument('--repository', default='arthurwolf/ilium', help='GitHub owner/name')
    parser.add_argument('--native-directory', type=Path, required=True)
    parser.add_argument('--packages-directory', type=Path, required=True)
    parser.add_argument('--work', type=Path, help='scratch directory for the generated SHA256SUMS')
    arguments = parser.parse_args(argv)
    if arguments.mode != 'check-versions':
        if not arguments.commit or not re.fullmatch(r'[0-9a-f]{40}', arguments.commit):
            fail('commit must be a full lowercase 40-character SHA')
        if arguments.work is None:
            fail('--work is required')
        if arguments.tag == AARCH64_TAG and arguments.commit != AARCH64_COMMIT:
            fail(f'{AARCH64_TAG} must be published from commit {AARCH64_COMMIT}')
        arguments.work.mkdir(parents=True, exist_ok=True)
    if arguments.mode == 'check-versions' and arguments.tag != AARCH64_TAG:
        fail(f'check-versions applies to {AARCH64_TAG} only')
    files, inventory = verify_inventory(arguments.native_directory, arguments.packages_directory, arguments.tag)
    if arguments.mode == 'check-versions':
        print(json.dumps({'type': 'result', 'command': 'publish-linux-preview', 'state': 'versions-verified',
                          'tag': arguments.tag, 'assets': sorted(files)}))
        return 0
    if arguments.mode == 'dry-run':
        print(json.dumps({'type': 'result', 'command': 'publish-linux-preview', 'state': 'dry-run-passed',
                          'tag': arguments.tag, 'commit': arguments.commit, 'assets': sorted(files)}))
        print(inventory, end='')
        return 0
    if shutil.which('gh') is None:
        fail('the gh command is required for GitHub access')
    if arguments.mode == 'github-dry-run':
        github_dry_run_checks(arguments.tag, arguments.commit, arguments.repository)
        print(json.dumps({'type': 'result', 'command': 'publish-linux-preview', 'state': 'github-dry-run-passed',
                          'tag': arguments.tag, 'commit': arguments.commit, 'assets': sorted(files)}))
        print(inventory, end='')
        return 0
    publish(files, inventory, arguments.commit, arguments.repository, arguments.work, arguments.tag)
    return 0


if __name__ == '__main__':
    sys.exit(main())
