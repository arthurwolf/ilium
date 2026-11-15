"""Read-only Pages qualification gates with injected GitHub and source providers."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import preview_qualification as qualification
import release_tool


class QualificationTests(unittest.TestCase):
    source_sha = 'a' * 40
    pages_sha = 'b' * 40

    def provider(self, *, failed_step=None, missing_artifact=None, release_commit=None,
                 expired_artifact=None, duplicate_artifact=None,
                 foreign_artifact=None, missing_artifact_id=None, run_status='success'):
        markers = qualification.REQUIRED_STEPS
        jobs = [{'id': index + 1, 'run_id': 123, 'head_sha': self.source_sha,
                 'status': 'completed', 'conclusion': 'success',
                 'steps': [{'name': marker, 'status': 'completed',
                            'conclusion': 'failure' if marker == failed_step else 'success'}]}
                for index, marker in enumerate(markers)]
        artifacts = [{'id': index + 11, 'name': name, 'expired': name == expired_artifact,
                      'workflow_run': {'id': 123, 'head_sha': self.source_sha}}
                     for index, name in enumerate(qualification.ARTIFACTS)
                     if name != missing_artifact]
        if duplicate_artifact:
            artifacts.append({**next(item for item in artifacts if item['name'] == duplicate_artifact),
                              'id': 99})
        for artifact in artifacts:
            if artifact['name'] == foreign_artifact:
                artifact['workflow_run'] = {'id': 999, 'head_sha': self.source_sha}
            if artifact['name'] == missing_artifact_id:
                artifact['id'] = None
        responses = {
            'actions/workflows/release.yml': {'id': 7, 'path': '.github/workflows/release.yml'},
            'actions/runs/123': {'workflow_id': 7, 'head_repository': {'full_name': 'arthurwolf/ilium'},
                                 'head_branch': qualification.BRANCH, 'event': 'workflow_dispatch',
                                 'run_attempt': 1, 'status': 'completed', 'conclusion': run_status,
                                 'head_sha': self.source_sha},
            'releases/tags/' + qualification.TAG: {'tag_name': qualification.TAG,
                                                    'prerelease': True, 'draft': False,
                                                    'immutable': True},
            'git/ref/tags/' + qualification.TAG: {'object': {'type': 'commit',
                                                             'sha': release_commit or self.source_sha}},
            'actions/runs/123/jobs?per_page=100&page=1': {'total_count': len(jobs), 'jobs': jobs},
            'actions/runs/123/artifacts?per_page=100': {'total_count': len(artifacts),
                                                        'artifacts': artifacts},
        }
        return lambda path: responses[path]

    def verify(self, provider):
        with patch.object(qualification, 'source_changes', return_value=['release/install.sh']):
            return qualification.verify_run('123', self.source_sha, self.pages_sha,
                                            github=provider)

    def test_success_requires_native_package_windows_jobs_and_retained_artifacts(self):
        result = self.verify(self.provider())
        self.assertEqual(result['qualified_source_sha'], self.source_sha)
        self.assertEqual(set(result['artifact_ids']), set(qualification.ARTIFACTS))
        for changed in (
            {'failed_step': qualification.REQUIRED_STEPS[3]},
            {'missing_artifact': 'native-x86_64-apple-darwin'},
            {'expired_artifact': 'windows-preview-powershell-installer-check'},
            {'duplicate_artifact': 'macos-packages-x86_64'},
            {'foreign_artifact': 'linux-packages-aarch64'},
            {'missing_artifact_id': 'native-aarch64-unknown-linux-gnu'},
            {'release_commit': 'c' * 40},
            {'run_status': 'failure'},
        ):
            with self.subTest(changed=changed), self.assertRaises(release_tool.ReleaseError):
                self.verify(self.provider(**changed))

    def test_postqualification_diff_is_exact_allowlist_and_rejects_deletion(self):
        def git(*args, root):
            if args[0] == 'rev-parse':
                return (self.pages_sha + '\n').encode()
            if args[0] == 'merge-base':
                return b''
            if args[0] == 'status':
                return b''
            return diff
        with patch.object(qualification, 'git', side_effect=git):
            diff = b'M\0release/install.sh\0A\0release/scripts/preview_qualification.py\0'
            self.assertEqual(qualification.source_changes(self.source_sha, self.pages_sha),
                             ['release/install.sh', 'release/scripts/preview_qualification.py'])
            postqualification_publication_files = (
                'release/RELEASING.md',
                'release/notes/v0.1.1-preview.7.md',
                'release/tests/test_readme_assets.py',
                'changelog.d/preview7-release.md',
            )
            for path in postqualification_publication_files:
                with self.subTest(allowed_path=path):
                    diff = b'A\0' + path.encode() + b'\0'
                    self.assertEqual(qualification.source_changes(self.source_sha, self.pages_sha),
                                     [path])
            for diff in (b'M\0Cargo.lock\0', b'D\0release/install.sh\0',
                         b'M\0release/install.ps1\0',
                         b'M\0release/scripts/release_tool.py\0',
                         b'M\0.github/workflows/release.yml\0',
                         b'M\0release/targets.toml\0',
                         b'A\0release/notes/v0.1.1-preview.8.md\0',
                         b'A\0release/tests/test_readme_assets_copy.py\0'):
                with self.subTest(diff=diff), self.assertRaises(release_tool.ReleaseError):
                    qualification.source_changes(self.source_sha, self.pages_sha)

    def test_checked_artifact_ids_are_the_only_download_outputs(self):
        ids = self.verify(self.provider())['artifact_ids']
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / 'github-output'
            output.touch()
            qualification.write_artifact_outputs(output, ids)
            lines = output.read_text().splitlines()
            self.assertEqual(set(lines), {
                key + '=' + str(ids[name])
                for name, key in qualification.ARTIFACT_OUTPUTS.items()
            })
            with self.assertRaises(release_tool.ReleaseError):
                qualification.write_artifact_outputs(output, {**ids,
                    'native-x86_64-apple-darwin': None})

    def test_published_package_readback_requires_exact_checksum_and_bytes(self):
        files = {'ilium-linux-aarch64.rpm': b'rpm',
                 'ilium-linux-aarch64.snap': b'snap',
                 'ilium-macos-x86_64.pkg': b'pkg'}
        hashes = {name: hashlib.sha256(content).hexdigest() for name, content in files.items()}
        checksums = ''.join(f'{digest}  {name}\n' for name, digest in hashes.items()).encode()
        base = qualification.pipeline.ORIGIN + '/download/' + qualification.TAG + '/'
        assets = [{'name': 'SHA256SUMS', 'browser_download_url': base + 'SHA256SUMS'},
                  *({'name': name, 'browser_download_url': base + name,
                     'digest': 'sha256:' + digest, 'size': len(files[name])}
                    for name, digest in hashes.items())]
        def github(path):
            self.assertEqual(path, 'releases/tags/' + qualification.TAG)
            return {'assets': assets}
        def request(url, *, limit):
            name = url.rsplit('/', 1)[1]
            return (checksums if name == 'SHA256SUMS' else files[name]), {}
        qualification.published_packages(hashes, github=github, request=request)
        files['ilium-linux-aarch64.snap'] = b'foreign snap bytes'
        with self.assertRaises(release_tool.ReleaseError):
            qualification.published_packages(hashes, github=github, request=request)
        files['ilium-linux-aarch64.snap'] = b'snap'
        files['ilium-macos-x86_64.pkg'] = b'foreign macOS package bytes'
        with self.assertRaises(release_tool.ReleaseError):
            qualification.published_packages(hashes, github=github, request=request)

    def test_native_receipt_requires_qualified_commit_and_archive_hash(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            target = {'archive': 'ilium-macos-x86_64.tar.gz',
                      'rust_target': 'x86_64-apple-darwin'}
            archive = directory / target['archive']
            archive.write_bytes(b'qualified native archive')
            digest = qualification.sha(archive)
            audit = directory / 'native-audit.json'
            audit.write_text('{}')
            bridge = {'schema': 1, 'state': 'passed', 'publication_allowed': True,
                      'tag': qualification.TAG, 'target': target['rust_target'],
                      'native_identity': {'runner': 'macos-15-intel'},
                      'archive': {'sha256': digest},
                      'native_audit': {'sha256': qualification.sha(audit)}}
            (directory / 'native-candidate-receipt.json').write_text(json.dumps(bridge))
            harness = {'schema': 1, 'tag': qualification.TAG,
                       'target': target['rust_target'], 'source_commit': self.source_sha}
            harness_path = directory / 'native-test-harness.json'
            harness_path.write_text(json.dumps(harness))
            (directory / 'SHA256SUMS').write_text(digest + '  ' + target['archive'] + '\n')
            with patch.object(qualification.release_tool, 'audit_receipt',
                              return_value={'native_identity': bridge['native_identity']}):
                self.assertEqual(qualification.verify_native(directory, target, self.source_sha), digest)
                harness['source_commit'] = self.pages_sha
                harness_path.write_text(json.dumps(harness))
                with self.assertRaises(release_tool.ReleaseError):
                    qualification.verify_native(directory, target, self.source_sha)
                harness['source_commit'] = self.source_sha
                harness_path.write_text(json.dumps(harness))
                archive.write_bytes(b'foreign native archive')
                with self.assertRaises(release_tool.ReleaseError):
                    qualification.verify_native(directory, target, self.source_sha)

    def test_linux_package_receipt_rejects_a_modified_snap(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            files = {'ilium-linux-aarch64.rpm': b'qualified rpm',
                     'ilium-linux-aarch64.snap': b'qualified snap'}
            hashes = {}
            for name, content in files.items():
                (directory / name).write_bytes(content)
                hashes[name] = qualification.sha(directory / name)
            build = {'tag': qualification.TAG, 'source_archive_sha256': 'a' * 64,
                     'packages': hashes}
            smoke = {'schema': 1, 'state': 'passed',
                     'target': 'aarch64-unknown-linux-gnu', 'tag': qualification.TAG,
                     'archive_sha256': 'a' * 64, 'package_files': hashes.copy()}
            (directory / 'linux-packages-aarch64.json').write_text(json.dumps(build))
            (directory / 'linux-animation-smoke-aarch64.json').write_text(json.dumps(smoke))
            self.assertEqual(qualification.verify_package(directory, 'linux', 'a' * 64,
                                                           self.source_sha), hashes)
            (directory / 'ilium-linux-aarch64.snap').write_bytes(b'foreign snap')
            with self.assertRaises(release_tool.ReleaseError):
                qualification.verify_package(directory, 'linux', 'a' * 64,
                                             self.source_sha)
            (directory / 'ilium-linux-aarch64.snap').write_bytes(files['ilium-linux-aarch64.snap'])
            smoke['package_files'] = {**hashes, 'ilium-linux-aarch64.snap': '0' * 64}
            (directory / 'linux-animation-smoke-aarch64.json').write_text(json.dumps(smoke))
            with self.assertRaises(release_tool.ReleaseError):
                qualification.verify_package(directory, 'linux', 'a' * 64,
                                             self.source_sha)

    def test_macos_smoke_package_hashes_must_equal_build_receipt(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            files = {'ilium-macos-x86_64.zip': b'zip',
                     'ilium-macos-x86_64.pkg': b'pkg',
                     'ilium-macos-x86_64.dmg': b'dmg'}
            hashes = {}
            for name, content in files.items():
                (directory / name).write_bytes(content)
                hashes[name] = qualification.sha(directory / name)
            build = {'source_commit': self.source_sha, 'source_archive_sha256': 'a' * 64,
                     'packages': hashes, 'package_files': {'ilium': 'b' * 64},
                     'package_bytes': {name: len(content) for name, content in files.items()}}
            build_path = directory / 'macos-packages-x86_64.json'
            build_path.write_text(json.dumps(build))
            smoke = {'state': 'passed', 'publication_allowed': True,
                     'source_commit': self.source_sha, 'source_archive_sha256': 'a' * 64,
                     'tag': qualification.TAG, 'arch': 'x86_64',
                     'build_receipt_sha256': qualification.sha(build_path),
                     'packages': hashes.copy(), 'package_files': build['package_files'],
                     'package_bytes': build['package_bytes']}
            smoke_path = directory / 'macos-smoke-x86_64.json'
            smoke_path.write_text(json.dumps(smoke))
            self.assertEqual(qualification.verify_package(directory, 'macos', 'a' * 64,
                                                           self.source_sha), hashes)
            smoke['packages'] = {**hashes, 'ilium-macos-x86_64.pkg': '0' * 64}
            smoke_path.write_text(json.dumps(smoke))
            with self.assertRaises(release_tool.ReleaseError):
                qualification.verify_package(directory, 'macos', 'a' * 64,
                                             self.source_sha)


if __name__ == '__main__':
    unittest.main()
