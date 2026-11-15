#!/usr/bin/env python3
"""Read-only custody gate for a Pages-only preview.7 workflow dispatch.

The native qualification precedes the installer checksum commit. A Pages
checkout may therefore descend from the build commit only through an explicit
set of installer, Pages, test and documentation paths. Published archive bytes
must still equal the native receipts from the original successful run.
"""
from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import re
import subprocess
import sys

import preview_pages
import release_pipeline as pipeline
import release_tool

ROOT = Path(__file__).resolve().parents[2]
TAG = 'v0.1.1-preview.7'
BRANCH = 'release/v0.1.1-preview.7'
REPOSITORY = 'arthurwolf/ilium'
SHA = re.compile(r'[0-9a-f]{40}\Z')
HASH = re.compile(r'[0-9a-f]{64}\Z')
RUN_ID = re.compile(r'[1-9][0-9]{0,19}\Z')

# Changes outside these exact paths require new native qualification. No
# generated binaries, build policy, manifest, dependency, package source, or
# native qualification code may enter the Pages checkout without a new run.
DESCENDANT_PATHS = frozenset({
    '.github/workflows/preview-pages.yml',
    '.github/workflows/preview-pages-tests.yml',
    'README.md',
    'src/docs/installation.md',
    'changelog.d/preview-pages.md',
    'changelog.d/preview7-release.md',
    'release/RELEASING.md',
    'release/install.sh',
    'release/linux-preview-pins.json',
    'release/notes/v0.1.1-preview.7.md',
    'release/site/index.html',
    'release/site/404.html',
    'release/site/_headers',
    'release/scripts/preview_pages.py',
    'release/scripts/preview_qualification.py',
    'release/tests/test_posix_install.py',
    'release/tests/test_preview_pages.py',
    'release/tests/test_preview_qualification.py',
    'release/tests/test_readme_assets.py',
})
ARTIFACTS = {
    'native-x86_64-apple-darwin': 'native-macos-intel',
    'macos-packages-x86_64': 'packages-macos-intel',
    'native-aarch64-unknown-linux-gnu': 'native-linux-arm64',
    'linux-packages-aarch64': 'packages-linux-arm64',
    'windows-preview-powershell-installer-check': 'windows-installer',
}
ARTIFACT_OUTPUTS = {
    'native-x86_64-apple-darwin': 'native_macos_intel_id',
    'macos-packages-x86_64': 'packages_macos_intel_id',
    'native-aarch64-unknown-linux-gnu': 'native_linux_arm64_id',
    'linux-packages-aarch64': 'packages_linux_arm64_id',
    'windows-preview-powershell-installer-check': 'windows_installer_id',
}
# Reusable job display names vary with GitHub's presentation. These unique
# executed step names identify the five required jobs in the exact run.
REQUIRED_STEPS = (
    'macOS product build, tests, licence closure and candidate audit',
    'Qualify all three formats and retain exact native audit and installed animation bytes',
    'Linux product build, tests, licence closure and candidate audit',
    'Seal native Linux animation evidence',
    'Install and remove the published preview using the candidate source',
)


def require(condition, message):
    if not condition:
        raise release_tool.ReleaseError(message)


def git(*arguments, root=ROOT):
    result = subprocess.run(['git', '-C', str(root), *arguments], capture_output=True, check=False)
    require(result.returncode == 0, 'Git source ancestry or checkout inspection failed')
    return result.stdout


def source_changes(qualified_sha, current_sha, *, root=ROOT):
    require(git('rev-parse', 'HEAD', root=root).decode().strip() == current_sha,
            'Pages checkout differs from workflow source commit')
    require(git('merge-base', '--is-ancestor', qualified_sha, current_sha, root=root) == b'',
            'Pages source is not descended from the qualified native source')
    raw = git('diff', '--name-status', '--no-renames', '-z', qualified_sha, current_sha, root=root)
    fields = raw.split(b'\0')
    require(fields[-1:] == [b''], 'Git source diff is incomplete')
    changes = []
    for index in range(0, len(fields) - 1, 2):
        require(index + 1 < len(fields) - 1, 'Git source diff is malformed')
        status, name = fields[index:index + 2]
        require(status in (b'A', b'M'), 'post-qualification source removed or renamed a file')
        path = name.decode('utf-8')
        require(path in DESCENDANT_PATHS, 'post-qualification source changed a native-owned path: ' + path)
        changes.append(path)
    require(changes, 'Pages source has no audited installer or Pages change after qualification')
    require(not git('status', '--porcelain', '--untracked-files=no', root=root).strip(),
            'Pages checkout has uncommitted tracked source')
    return changes


def tag_commit(github):
    reference = github('git/ref/tags/' + TAG)
    entry = reference.get('object', {})
    require(entry.get('type') in ('commit', 'tag') and SHA.fullmatch(str(entry.get('sha', ''))),
            'preview.7 tag reference is invalid')
    if entry['type'] == 'tag':
        annotated = github('git/tags/' + entry['sha'])
        entry = annotated.get('object', {})
    require(entry.get('type') == 'commit' and SHA.fullmatch(str(entry.get('sha', ''))),
            'preview.7 tag does not resolve directly to a commit')
    return entry['sha']


def result_jobs(github, run_id, source_sha):
    jobs = []
    for page in range(1, 11):
        response = github(f'actions/runs/{run_id}/jobs?per_page=100&page={page}')
        batch = response.get('jobs')
        require(isinstance(batch, list), 'qualified run jobs are missing')
        jobs.extend(batch)
        require(len(jobs) <= 1000, 'qualified run has too many jobs')
        if len(jobs) >= response.get('total_count', -1):
            break
    require(len(jobs) == response.get('total_count'), 'qualified run jobs are incomplete')
    matches = {}
    for job in jobs:
        if job.get('run_id') != int(run_id) or job.get('head_sha') != source_sha:
            continue
        steps = job.get('steps') or []
        for marker in REQUIRED_STEPS:
            found = [step for step in steps if step.get('name') == marker]
            if found:
                require(marker not in matches and len(found) == 1,
                        'qualified run has duplicate native job evidence: ' + marker)
                require(job.get('status') == 'completed' and job.get('conclusion') == 'success'
                        and found[0].get('status') == 'completed' and found[0].get('conclusion') == 'success',
                        'qualified native job or required step did not succeed: ' + marker)
                matches[marker] = job.get('id')
    require(set(matches) == set(REQUIRED_STEPS) and len(set(matches.values())) == len(REQUIRED_STEPS),
            'one or more native/package/Windows qualification jobs did not run successfully')
    return matches


def artifact_metadata(github, run_id, source_sha):
    response = github(f'actions/runs/{run_id}/artifacts?per_page=100')
    artifacts = response.get('artifacts')
    require(isinstance(artifacts, list) and response.get('total_count') == len(artifacts)
            and len(artifacts) <= 100, 'qualified run artifact inventory is incomplete')
    selected = {}
    for artifact in artifacts:
        name = artifact.get('name')
        if name not in ARTIFACTS:
            continue
        # upload-artifact v4 gives each artifact an immutable ID. Reject a
        # duplicate name before passing its exact ID to download-artifact.
        artifact_id = artifact.get('id')
        require(name not in selected and isinstance(artifact_id, int) and artifact_id > 0
                and artifact.get('expired') is False
                and artifact.get('workflow_run', {}).get('id') == int(run_id)
                and artifact.get('workflow_run', {}).get('head_sha') == source_sha,
                'qualified native artifact is duplicate, expired or from another source: ' + name)
        selected[name] = artifact_id
    require(set(selected) == set(ARTIFACTS) and len(set(selected.values())) == len(ARTIFACTS),
            'qualified run lacks unique retained native/package/Windows artifacts')
    return selected


def verify_run(run_id, source_sha, current_sha, *, github=pipeline.github, root=ROOT):
    require(RUN_ID.fullmatch(run_id) and SHA.fullmatch(source_sha) and SHA.fullmatch(current_sha),
            'qualification run ID and source commits must be complete and well formed')
    require(os.environ.get('GITHUB_REPOSITORY', REPOSITORY) == REPOSITORY
            and os.environ.get('GITHUB_REF', 'refs/heads/' + BRANCH) == 'refs/heads/' + BRANCH,
            'Pages qualification must run on the owning release branch')
    workflow = github('actions/workflows/release.yml')
    run = github('actions/runs/' + run_id)
    require(workflow.get('path') == '.github/workflows/release.yml'
            and run.get('workflow_id') == workflow.get('id')
            and run.get('head_repository', {}).get('full_name') == REPOSITORY
            and run.get('head_branch') == BRANCH and run.get('event') == 'workflow_dispatch'
            and run.get('run_attempt') == 1 and run.get('status') == 'completed'
            and run.get('conclusion') == 'success' and run.get('head_sha') == source_sha,
            'native qualification run identity, source or success differs')
    release = github('releases/tags/' + TAG)
    require(release.get('tag_name') == TAG and release.get('prerelease') is True
            and release.get('draft') is False and release.get('immutable') is True
            and tag_commit(github) == source_sha,
            'published immutable preview.7 release is not at the qualified source commit')
    jobs = result_jobs(github, run_id, source_sha)
    artifacts = artifact_metadata(github, run_id, source_sha)
    changes = source_changes(source_sha, current_sha, root=root)
    return {'schema': 1, 'state': 'passed', 'run_id': int(run_id),
            'qualified_source_sha': source_sha, 'pages_source_sha': current_sha,
            'native_job_ids': jobs, 'artifact_ids': artifacts, 'audited_paths': changes}


def write_artifact_outputs(path, artifacts):
    require(set(artifacts) == set(ARTIFACT_OUTPUTS)
            and all(isinstance(value, int) and value > 0 for value in artifacts.values()),
            'qualified artifact IDs are missing or invalid')
    with Path(path).open('a', encoding='ascii') as output:
        for name, key in ARTIFACT_OUTPUTS.items():
            output.write(key + '=' + str(artifacts[name]) + '\n')


def sha(path):
    value = hashlib.sha256()
    path = Path(path)
    require(path.is_file() and not path.is_symlink() and path.stat().st_size <= pipeline.MAX_DOWNLOAD,
            'qualified artifact file is missing, linked or oversized: ' + str(path))
    with path.open('rb') as stream:
        while block := stream.read(1024 * 1024):
            value.update(block)
    return value.hexdigest()


def verify_native(directory, target, source_sha):
    archive = directory / target['archive']
    bridge = release_tool.read_json(directory / 'native-candidate-receipt.json')
    harness = release_tool.read_json(directory / 'native-test-harness.json')
    audit = release_tool.audit_receipt(directory / 'native-audit.json', target, TAG[1:], TAG)
    actual = sha(archive)
    require(bridge.get('schema') == 1 and bridge.get('state') == 'passed'
            and bridge.get('publication_allowed') is True and bridge.get('tag') == TAG
            and bridge.get('target') == target['rust_target']
            and bridge.get('archive', {}).get('sha256') == actual
            and bridge.get('native_audit', {}).get('sha256') == sha(directory / 'native-audit.json')
            and harness.get('schema') == 1 and harness.get('tag') == TAG
            and harness.get('target') == target['rust_target']
            and harness.get('source_commit') == source_sha
            and audit.get('native_identity') == bridge.get('native_identity'),
            'native artifact receipt, archive or qualified source differs: ' + target['archive'])
    sums = (directory / 'SHA256SUMS').read_text('ascii')
    require(sums == actual + '  ' + target['archive'] + '\n',
            'native artifact checksum file differs: ' + target['archive'])
    return actual


def verify_package(directory, kind, archive_sha, source_sha):
    if kind == 'macos':
        build = release_tool.read_json(directory / 'macos-packages-x86_64.json')
        smoke = release_tool.read_json(directory / 'macos-smoke-x86_64.json')
        require(build.get('source_commit') == source_sha
                and build.get('source_archive_sha256') == archive_sha
                and smoke.get('state') == 'passed' and smoke.get('publication_allowed') is True
                and smoke.get('source_commit') == source_sha
                and smoke.get('source_archive_sha256') == archive_sha
                and smoke.get('tag') == TAG and smoke.get('arch') == 'x86_64'
                and smoke.get('build_receipt_sha256') == sha(directory / 'macos-packages-x86_64.json')
                and smoke.get('packages') == build.get('packages')
                and smoke.get('package_files') == build.get('package_files')
                and smoke.get('package_bytes') == build.get('package_bytes'),
                'macOS Intel package or installed-format smoke is not bound to qualified archive')
        names = ('ilium-macos-x86_64.zip', 'ilium-macos-x86_64.pkg', 'ilium-macos-x86_64.dmg')
    else:
        build = release_tool.read_json(directory / 'linux-packages-aarch64.json')
        smoke = release_tool.read_json(directory / 'linux-animation-smoke-aarch64.json')
        require(build.get('tag') == TAG and build.get('source_archive_sha256') == archive_sha
                and smoke.get('schema') == 1 and smoke.get('state') == 'passed'
                and smoke.get('target') == 'aarch64-unknown-linux-gnu'
                and smoke.get('tag') == TAG and smoke.get('archive_sha256') == archive_sha
                and smoke.get('package_files') == build.get('packages'),
                'Linux ARM64 package or installed-format smoke is not bound to qualified archive')
        names = ('ilium-linux-aarch64.rpm', 'ilium-linux-aarch64.snap')
    packages = build.get('packages')
    require(isinstance(packages, dict) and all(HASH.fullmatch(str(packages.get(name, '')))
                                                   and sha(directory / name) == packages[name]
                                                   for name in names),
            'qualified package files differ from their native build receipt')
    return {name: packages[name] for name in names}


def published_packages(expected, *, github=pipeline.github, request=pipeline.request):
    release = github('releases/tags/' + TAG)
    assets = {asset['name']: asset for asset in release.get('assets', [])}
    require(len(assets) == len(release.get('assets', [])) and 'SHA256SUMS' in assets,
            'published preview.7 asset inventory is ambiguous')
    sums_url = pipeline.ORIGIN + '/download/' + TAG + '/SHA256SUMS'
    require(assets['SHA256SUMS'].get('browser_download_url') == sums_url,
            'published preview.7 checksum URL differs')
    sums_bytes, _headers = request(sums_url, limit=16384)
    checksums = preview_pages.parse_checksums(sums_bytes)
    for name, expected_sha in expected.items():
        asset = assets.get(name)
        require(asset is not None and checksums.get(name) == expected_sha,
                'published package checksum differs from qualified receipt: ' + name)
        url = pipeline.ORIGIN + '/download/' + TAG + '/' + name
        require(asset.get('browser_download_url') == url
                and asset.get('digest') in (None, 'sha256:' + expected_sha),
                'published package URL or provider digest differs: ' + name)
        body, _headers = request(url, limit=pipeline.MAX_DOWNLOAD)
        require(len(body) == asset.get('size') and hashlib.sha256(body).hexdigest() == expected_sha,
                'published package bytes differ from qualified receipt: ' + name)


def verify_artifacts(root, source_sha, *, published, github=pipeline.github,
                     request=pipeline.request):
    targets = release_tool.load_targets(ROOT / 'release/targets.toml')
    names = {target['rust_target']: target for target in targets}
    intel = verify_native(root / ARTIFACTS['native-x86_64-apple-darwin'],
                          names['x86_64-apple-darwin'], source_sha)
    arm = verify_native(root / ARTIFACTS['native-aarch64-unknown-linux-gnu'],
                        names['aarch64-unknown-linux-gnu'], source_sha)
    macos_packages = verify_package(root / ARTIFACTS['macos-packages-x86_64'],
                                    'macos', intel, source_sha)
    linux_packages = verify_package(root / ARTIFACTS['linux-packages-aarch64'],
                                    'linux', arm, source_sha)
    windows = release_tool.read_json(root / ARTIFACTS['windows-preview-powershell-installer-check'] / 'receipt.json')
    require(windows.get('schema') == 1 and windows.get('state') == 'passed'
            and windows.get('version') == '0.1.1-preview.6'
            and windows.get('client_version') == 'ilium 0.1.1-preview.6'
            and windows.get('server_version') == 'ilium-server 0.1.1-preview.6'
            and windows.get('cleanup') == 'owned-installation-removed'
            and windows.get('path_restored') is True
            and windows.get('installer_sha256') == sha(ROOT / 'release/install.ps1'),
            'Windows native installer check did not pass for this unchanged source')
    carrier = published[TAG[1:]]['archives']
    require(carrier['ilium-macos-x86_64.tar.gz']['sha256'] == intel,
            'published preview.7 macOS Intel archive differs from qualified native bytes')
    # The ARM64 tarball is carried from preview.6; only its new native RPM and
    # Snap are published from this run. The Pages preparer verifies the entire
    # preview.6 carry-forward inventory byte for byte.
    published_packages({**macos_packages, **linux_packages}, github=github, request=request)
    return {'macos_intel_archive_sha256': intel, 'linux_arm64_archive_sha256': arm,
            'windows_installer_sha256': windows['installer_sha256'],
            'published_package_sha256': {**macos_packages, **linux_packages}}


def main():
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument('phase', choices=('run', 'artifacts'))
    parser.add_argument('--run-id', required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--current-sha', required=True)
    parser.add_argument('--artifacts', type=Path)
    parser.add_argument('--github-output', type=Path)
    arguments = parser.parse_args()
    try:
        record = verify_run(arguments.run_id, arguments.source_sha, arguments.current_sha)
        if arguments.phase == 'run':
            require(arguments.github_output is not None, 'qualified artifact ID output is required')
            write_artifact_outputs(arguments.github_output, record['artifact_ids'])
        if arguments.phase == 'artifacts':
            require(arguments.artifacts is not None and arguments.artifacts.is_dir(),
                    'downloaded native artifacts are missing')
            _targets, pins, _installers = preview_pages.installer_pins(ROOT, TAG)
            published = preview_pages.published_pin_evidence(pins, carrier_tag=TAG)
            record.update(verify_artifacts(arguments.artifacts, arguments.source_sha,
                                           published=published))
        pipeline.emit('result', command='preview-pages-qualification', phase=arguments.phase, **record)
    except (release_tool.ReleaseError, OSError, KeyError, TypeError, ValueError) as error:
        pipeline.emit('error', command='preview-pages-qualification', phase=arguments.phase,
                      message=str(error))
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
