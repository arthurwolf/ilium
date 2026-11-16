"""Nonpublishing contracts for exact native and carried preview GHCR evidence."""
import copy
import hashlib
from io import BytesIO
import json
from pathlib import Path
import re
import sys
import tempfile
import urllib.error
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import zipfile

import yaml

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import publish_preview_ghcr as preview
import release_tool
from release.tests.test_publish_ghcr import FakeClient, public_metadata


class PreviewPublisherTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.expected, self.new = preview.inventory()
        self.source = 'a' * 40
        self.current = 'b' * 40
        self.arguments = SimpleNamespace(operator=False, run_id='123', source_sha=self.source,
            current_sha=self.current, artifacts=self.root / 'native', payload=self.root / 'payload')
        self.environment = {'GITHUB_EVENT_NAME': 'workflow_dispatch',
            'GITHUB_REPOSITORY': preview.registry.REPOSITORY,
            'GITHUB_REF': 'refs/heads/' + preview.qualification.BRANCH,
            'GITHUB_SHA': self.current}
        self.files = {name: ('synthetic fixture: ' + name).encode() for name in self.expected}
        self.releases = {}
        self.bodies = {}
        for tag, names in ((preview.TAG, self.expected),
                           (preview.PREVIOUS_TAG, self.expected - self.new)):
            base = preview.pipeline.ORIGIN + '/download/' + tag + '/'
            bodies = {name: self.files[name] for name in names}
            bodies['SHA256SUMS'] = ''.join(hashlib.sha256(bodies[name]).hexdigest() +
                '  ' + name + '\n' for name in sorted(names)).encode()
            self.bodies.update({base + name: body for name, body in bodies.items()})
            self.releases['releases/tags/' + tag] = {
                'id': 42 if tag == preview.TAG else 41, 'tag_name': tag,
                'draft': False, 'prerelease': True, 'immutable': True,
                'assets': [{'name': name, 'size': len(body),
                    'digest': 'sha256:' + hashlib.sha256(body).hexdigest(),
                    'browser_download_url': base + name} for name, body in bodies.items()]}
        self.reads = []
        source = patch.object(preview, 'verify_runtime_source')
        custody = patch.object(preview, 'verify_artifact_custody', return_value={'synthetic': 'verified'})
        source.start()
        self.custody = custody.start()
        self.addCleanup(source.stop)
        self.addCleanup(custody.stop)

    def github(self, path):
        return self.releases[path]

    def request(self, url, *, limit):
        self.reads.append(url)
        body = self.bodies[url]
        self.assertLessEqual(len(body), limit)
        return body, {}

    def prepare(self):
        with (patch.object(preview.qualification, 'verify_run', return_value={
                  'run_id': 123, 'qualified_source_sha': self.source}) as run,
              patch.object(preview.qualification, 'verify_artifacts', return_value={
                  'published_package_sha256': {name: hashlib.sha256(self.files[name]).hexdigest()
                      for name in self.new if not name.endswith('.tar.gz')}}) as artifacts):
            plan = preview.prepare(self.arguments, self.environment,
                                   github=self.github, request=self.request)
            run.assert_called_once_with('123', self.source, self.current,
                                         github=self.github, root=preview.ROOT)
            artifacts.assert_called_once()
            call = artifacts.call_args
            self.assertEqual(call.args, (self.arguments.artifacts, self.source))
            self.assertEqual(call.kwargs['published'][preview.TAG[1:]]['archives']
                ['ilium-macos-x86_64.tar.gz']['sha256'],
                hashlib.sha256(self.files['ilium-macos-x86_64.tar.gz']).hexdigest())
            body, _headers = call.kwargs['request'](preview.pipeline.ORIGIN +
                '/download/' + preview.TAG + '/ilium-linux-aarch64.rpm', limit=1000)
            self.assertEqual(body, self.files['ilium-linux-aarch64.rpm'])
        return plan

    def test_full_manifest_inventory_and_every_public_byte_are_verified(self):
        self.assertEqual(len(self.expected), 23)
        self.assertEqual(len(self.new), 6)
        self.assertEqual(len(self.expected - self.new), 17)
        plan = self.prepare()
        self.assertEqual(len(plan['files']), 24)
        self.assertEqual(plan['commit'], self.source)
        self.assertEqual(plan['publisher_commit'], self.current)
        self.assertEqual(plan['reference'], preview.registry.PACKAGE + ':' + preview.TAG)
        self.assertEqual(set(self.reads), set(self.bodies))
        self.assertEqual(preview.pipeline.asset_hashes(plan['files']), plan['assets'])

    def test_wrong_workflow_source_stops_before_native_or_network_access(self):
        for key, value in (('GITHUB_EVENT_NAME', 'push'), ('GITHUB_REPOSITORY', 'other/repo'),
                           ('GITHUB_REF', 'refs/heads/master'), ('GITHUB_SHA', self.source)):
            with self.subTest(key=key), patch.object(preview.qualification, 'verify_run') as run:
                with self.assertRaises(release_tool.ReleaseError):
                    preview.prepare(self.arguments, {**self.environment, key: value},
                                    github=self.github, request=self.request)
                run.assert_not_called()
                self.assertEqual(self.reads, [])

    def test_failed_native_gate_stops_before_public_download_or_registry(self):
        with (patch.object(preview.qualification, 'verify_run',
                           side_effect=release_tool.ReleaseError('native run failed')),
              self.assertRaises(release_tool.ReleaseError)):
            preview.prepare(self.arguments, self.environment, github=self.github, request=self.request)
        self.assertEqual(self.reads, [])
        self.assertFalse(self.arguments.payload.exists())
        self.custody.assert_not_called()

    def test_local_operator_uses_real_checkout_without_fabricating_actions_context(self):
        self.arguments.operator = True
        self.environment = {}
        with patch.object(preview.qualification, 'git', side_effect=[
                b'https://github.com/arthurwolf/ilium.git\n',
                (preview.qualification.BRANCH + '\n').encode()]) as git:
            plan = self.prepare()
        self.assertEqual(plan['commit'], self.source)
        self.assertEqual(git.call_count, 2)

    def test_local_operator_rejects_actions_and_noncanonical_checkout(self):
        self.arguments.operator = True
        for environment in ({'GITHUB_ACTIONS': 'true'}, {'GITHUB_EVENT_NAME': 'workflow_dispatch'}):
            with self.subTest(environment=environment), self.assertRaises(release_tool.ReleaseError):
                preview.prepare(self.arguments, environment, github=self.github, request=self.request)
        for origin, branch in (('https://github.com/other/ilium.git', preview.qualification.BRANCH),
                               ('https://github.com/arthurwolf/ilium.git', 'master')):
            with (self.subTest(origin=origin, branch=branch),
                  patch.object(preview.qualification, 'git', side_effect=[
                      origin.encode(), branch.encode()]),
                  self.assertRaises(release_tool.ReleaseError)):
                preview.prepare(self.arguments, {}, github=self.github, request=self.request)
        self.assertEqual(self.reads, [])

    def test_published_inventory_metadata_and_download_tampering_fail_closed(self):
        original = copy.deepcopy(self.releases)
        key = 'releases/tags/' + preview.TAG
        for mutation in ('mutable', 'draft', 'missing', 'extra', 'duplicate', 'url', 'size', 'digest'):
            self.releases = copy.deepcopy(original)
            record = self.releases[key]
            asset = record['assets'][0]
            if mutation == 'mutable': record['immutable'] = False
            elif mutation == 'draft': record['draft'] = True
            elif mutation == 'missing': record['assets'].pop()
            elif mutation == 'extra': record['assets'].append({**asset, 'name': 'foreign.exe'})
            elif mutation == 'duplicate': record['assets'].append(asset.copy())
            elif mutation == 'url': asset['browser_download_url'] = 'https://example.invalid/asset'
            elif mutation == 'size': asset['size'] += 1
            elif mutation == 'digest': asset['digest'] = 'sha256:' + 'f' * 64
            with self.subTest(mutation=mutation), self.assertRaises(release_tool.ReleaseError):
                preview.download_release(preview.TAG, self.expected,
                                         github=self.github, request=self.request)
        self.releases = original
        url = preview.pipeline.ORIGIN + '/download/' + preview.TAG + '/ilium-linux-aarch64.snap'
        self.bodies[url] = b'foreign bytes'
        with self.assertRaises(release_tool.ReleaseError):
            preview.download_release(preview.TAG, self.expected, github=self.github, request=self.request)

    def test_carried_asset_changed_with_consistent_new_release_checksums_is_rejected(self):
        name = 'ilium-windows-x86_64.zip'
        base = preview.pipeline.ORIGIN + '/download/' + preview.PREVIOUS_TAG + '/'
        old_digest = hashlib.sha256(self.files[name]).hexdigest()
        different = b'synthetic different previous payload'
        new_digest = hashlib.sha256(different).hexdigest()
        self.bodies[base + name] = different
        self.bodies[base + 'SHA256SUMS'] = self.bodies[base + 'SHA256SUMS'].replace(
            old_digest.encode(), new_digest.encode())
        for asset in self.releases['releases/tags/' + preview.PREVIOUS_TAG]['assets']:
            body = self.bodies[base + asset['name']]
            asset.update(size=len(body), digest='sha256:' + hashlib.sha256(body).hexdigest())
        with patch.object(preview.qualification, 'verify_run', return_value={}):
            with self.assertRaisesRegex(release_tool.ReleaseError, 'changed carried'):
                preview.prepare(self.arguments, self.environment, github=self.github, request=self.request)

    def test_native_artifact_failure_prevents_qualified_plan(self):
        with (patch.object(preview.qualification, 'verify_run', return_value={}),
              patch.object(preview.qualification, 'verify_artifacts',
                           side_effect=release_tool.ReleaseError('native receipt differs')),
              self.assertRaisesRegex(release_tool.ReleaseError, 'native receipt differs')):
            preview.prepare(self.arguments, self.environment, github=self.github, request=self.request)

    def test_forged_local_artifact_stops_before_public_download_or_registry(self):
        self.custody.side_effect = release_tool.ReleaseError('local native evidence differs from exact artifact ID')
        with (patch.object(preview.qualification, 'verify_run', return_value={}),
              self.assertRaisesRegex(release_tool.ReleaseError, 'exact artifact ID')):
            preview.prepare(self.arguments, self.environment, github=self.github, request=self.request)
        self.assertEqual(self.reads, [])
        self.assertFalse(self.arguments.payload.exists())

    def test_preview_publication_reuses_immutable_and_anonymous_registry_checks(self):
        plan = self.prepare()
        output, intent = self.root / 'result.json', self.root / 'intent.json'
        client = FakeClient(plan)
        with patch.object(preview.registry, 'public_package_metadata', return_value=public_metadata()):
            result = preview.publish(plan, client, output, intent_path=intent)
        self.assertTrue(result['anonymous_pull_verified'])
        self.assertEqual(client.pushes, 1)
        self.assertEqual(client.pulls, 1)
        evidence = json.loads((self.root / 'preview-ghcr-qualification.json').read_text())
        self.assertEqual(evidence['preview_qualification']['run_id'], 123)
        self.assertEqual(len(evidence['carried_release']['assets']), 17)
        self.assertNotIn('publication_allowed', evidence)
        client.manifest['layers'][0]['digest'] = 'sha256:' + 'f' * 64
        with self.assertRaises(release_tool.ReleaseError):
            preview.publish(plan, client, output, intent_path=intent)
        self.assertEqual(client.pushes, 1)

    def test_anonymous_pull_failure_never_writes_a_success_receipt(self):
        plan = self.prepare()
        client = FakeClient(plan, public=False)
        output, intent = self.root / 'result.json', self.root / 'intent.json'
        with self.assertRaises(release_tool.ReleaseError):
            preview.publish(plan, client, output, intent_path=intent)
        self.assertFalse(output.exists())
        self.assertTrue(intent.is_file())
        self.assertEqual(client.pushes, 1)

    def test_retry_uses_fresh_payload_with_same_retained_intent_and_evidence(self):
        plan = self.prepare()
        client = FakeClient(plan, public=False)
        output, intent = self.root / 'result.json', self.root / 'intent.json'
        with self.assertRaises(release_tool.ReleaseError):
            preview.publish(plan, client, output, intent_path=intent)
        retained = intent.read_bytes()
        with self.assertRaisesRegex(release_tool.ReleaseError, 'fresh --payload'):
            self.prepare()
        self.arguments.payload = self.root / 'fresh-payload'
        retry = self.prepare()
        client.plan = retry
        client.public = True
        with patch.object(preview.registry, 'public_package_metadata', return_value=public_metadata()):
            result = preview.publish(retry, client, output, intent_path=intent)
        self.assertEqual(intent.read_bytes(), retained)
        self.assertEqual(result['retry_state'], 'existing-verified')
        self.assertEqual(client.pushes, 1)
        self.assertTrue(result['anonymous_pull_verified'])

    def test_changed_remote_tag_stops_before_registry_credentials_are_used(self):
        args = SimpleNamespace(mode='publish')
        with (patch.object(preview, 'parser') as parser,
              patch.object(preview, 'prepare', return_value={'commit': self.source}),
              patch.dict(preview.os.environ, {'GHCR_TOKEN': 'synthetic-token', 'GITHUB_ACTOR': 'synthetic-actor'}),
              patch.object(preview.pipeline, 'require_remote_tag',
                           side_effect=release_tool.ReleaseError('remote tag differs')),
              patch.object(preview.registry, 'OrasClient') as client):
            parser.return_value.parse_args.return_value = args
            self.assertEqual(preview.main([]), 1)
            client.assert_not_called()

    def test_default_verify_mode_cannot_construct_a_registry_client(self):
        plan = self.prepare()
        args = SimpleNamespace(mode='verify', output=self.root / 'verified.json')
        with (patch.object(preview, 'parser') as parser,
              patch.object(preview, 'prepare', return_value=plan),
              patch.object(preview.registry, 'OrasClient') as client):
            parser.return_value.parse_args.return_value = args
            self.assertEqual(preview.main([]), 0)
            client.assert_not_called()

    def test_workflow_uses_exact_verified_artifact_ids_and_safe_dispatch_inputs(self):
        source = (ROOT / '.github/workflows/preview-ghcr.yml').read_text()
        workflow = yaml.load(source, Loader=yaml.BaseLoader)
        self.assertEqual(set(workflow['on']), {'workflow_dispatch'})
        inputs = workflow['on']['workflow_dispatch']['inputs']
        self.assertEqual(inputs['publish']['default'], 'false')
        self.assertEqual(workflow['concurrency']['cancel-in-progress'], 'false')
        job = workflow['jobs']['preview-bundle']
        self.assertIn('refs/heads/' + preview.qualification.BRANCH, job['if'])
        steps = job['steps']
        downloads = [step for step in steps if step.get('uses', '').startswith('actions/download-artifact@')]
        self.assertEqual(len(downloads), 5)
        self.assertEqual({step['with']['artifact-ids'] for step in downloads},
            {'${{ steps.native.outputs.' + key + ' }}'
             for key in preview.qualification.ARTIFACT_OUTPUTS.values()})
        for step in downloads:
            self.assertEqual(step['with']['merge-multiple'], 'true')
            self.assertEqual(step['with']['run-id'], '${{ inputs.native_run_id }}')
        for step in steps:
            if 'uses' in step:
                self.assertRegex(step['uses'], r'@[0-9a-f]{40}$')
            self.assertNotRegex(step.get('run', ''), r'\$\{\{\s*inputs\.')
        self.assertIn('fetch-depth: 0', source)
        self.assertNotIn('release_pipeline.py publish', source)


class NativeArtifactCustodyTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.source = 'a' * 40
        self.ids = {name: index + 11 for index, name in enumerate(preview.qualification.ARTIFACTS)}
        self.native = {'run_id': 123, 'artifact_ids': self.ids}
        self.metadata = {}
        self.archives = {}
        self.requests = []
        for name, artifact_id in self.ids.items():
            folder = self.root / preview.qualification.ARTIFACTS[name]
            folder.mkdir()
            contents = b'synthetic exact native artifact ' + name.encode()
            (folder / 'receipt.json').write_bytes(contents)
            buffer = BytesIO()
            with zipfile.ZipFile(buffer, 'w') as archive:
                archive.writestr('receipt.json', contents)
            body = buffer.getvalue()
            self.archives[artifact_id] = body
            self.metadata[artifact_id] = {
                'id': artifact_id, 'name': name, 'expired': False,
                'workflow_run': {'id': 123, 'head_sha': self.source},
                'digest': 'sha256:' + hashlib.sha256(body).hexdigest(), 'size_in_bytes': len(body)}

    def github(self, path):
        return self.metadata[int(path.rsplit('/', 1)[1])]

    def locate(self, artifact_id, token):
        self.assertEqual(token, 'synthetic-api-token')
        return f'https://example.blob.core.windows.net/{artifact_id}.zip'

    def request(self, url, **arguments):
        self.requests.append((url, arguments))
        # This download must never carry an API Authorization header.
        self.assertEqual(set(arguments), {'limit'})
        return self.archives[int(url.rsplit('/', 1)[1].split('.')[0])], {}

    def verify(self):
        return preview.verify_artifact_custody(self.root, self.native, self.source,
            github=self.github, request=self.request, locate=self.locate, token='synthetic-api-token')

    def test_exact_artifact_ids_archives_and_every_extracted_file_are_bound(self):
        custody = self.verify()
        self.assertEqual(set(custody), set(self.ids))
        self.assertEqual(len(self.requests), 5)
        for name, artifact_id in self.ids.items():
            self.assertEqual(custody[name]['artifact_id'], artifact_id)
            self.assertEqual(custody[name]['archive_sha256'], self.metadata[artifact_id]['digest'][7:])
            self.assertEqual(set(custody[name]['files']), {'receipt.json'})

    def test_foreign_expired_missing_digest_and_wrong_api_identity_are_rejected(self):
        artifact_id = next(iter(self.ids.values()))
        original = copy.deepcopy(self.metadata[artifact_id])
        mutations = ({'id': 999}, {'name': 'foreign'}, {'expired': True},
            {'workflow_run': {'id': 124, 'head_sha': self.source}},
            {'workflow_run': {'id': 123, 'head_sha': 'b' * 40}},
            {'digest': None}, {'digest': 'sha256:bad'}, {'size_in_bytes': 0})
        for mutation in mutations:
            self.metadata[artifact_id] = {**original, **mutation}
            with self.subTest(mutation=mutation), self.assertRaises(release_tool.ReleaseError):
                self.verify()
        self.assertEqual(self.requests, [])

    def test_archive_transport_or_extracted_file_tampering_is_rejected(self):
        artifact_id = next(iter(self.ids.values()))
        body = self.archives[artifact_id]
        self.archives[artifact_id] = body[:-1] + bytes([body[-1] ^ 1])
        with self.assertRaisesRegex(release_tool.ReleaseError, 'immutable API digest'):
            self.verify()
        self.archives[artifact_id] = body
        folder = self.root / next(iter(preview.qualification.ARTIFACTS.values()))
        path = folder / 'receipt.json'
        original = path.read_bytes()
        for mutation in ('changed', 'missing', 'extra'):
            if mutation == 'changed': path.write_bytes(b'synthetic forged receipt')
            elif mutation == 'missing': path.unlink()
            else: (folder / 'extra.json').write_text('synthetic extra evidence')
            with self.subTest(mutation=mutation), self.assertRaisesRegex(release_tool.ReleaseError,
                    'differs from exact artifact ID'):
                self.verify()
            path.write_bytes(original)

    def test_linked_local_evidence_and_unsafe_zip_entries_are_rejected(self):
        folder = self.root / next(iter(preview.qualification.ARTIFACTS.values()))
        path = folder / 'receipt.json'
        path.unlink()
        path.symlink_to(self.root / 'outside.json')
        with self.assertRaisesRegex(release_tool.ReleaseError, 'link or special entry'):
            self.verify()
        for name in ('../receipt.json', '/absolute.json', 'nested/../receipt.json', 'back\\slash'):
            buffer = BytesIO()
            with zipfile.ZipFile(buffer, 'w') as archive:
                archive.writestr(name, b'synthetic unsafe artifact')
            with self.subTest(name=name), self.assertRaises(release_tool.ReleaseError):
                preview.zip_file_hashes(buffer.getvalue())
        buffer = BytesIO()
        with zipfile.ZipFile(buffer, 'w') as archive:
            linked = zipfile.ZipInfo('symlink')
            linked.external_attr = (0o120777 << 16)
            archive.writestr(linked, b'synthetic target')
        with self.assertRaises(release_tool.ReleaseError):
            preview.zip_file_hashes(buffer.getvalue())

    def test_authenticated_redirect_is_not_followed_and_signed_url_is_checked(self):
        url = 'https://example.blob.core.windows.net/signed-archive.zip?synthetic=1'
        error = urllib.error.HTTPError('https://api.github.com', 302, 'Found', {'Location': url}, None)
        with patch.object(preview.urllib.request, 'build_opener') as opener:
            opener.return_value.open.side_effect = error
            self.assertEqual(preview.artifact_location(11, 'synthetic-api-token'), url)
            request = opener.return_value.open.call_args.args[0]
            self.assertEqual(request.get_header('Authorization'), 'Bearer synthetic-api-token')
        self.assertIsNone(preview.NoArtifactRedirect().redirect_request(None, None, 302, '', {}, url))
        for location in ('http://example.blob.core.windows.net/file', 'https://example.invalid/file',
                         'https://example.blob.core.windows.net.evil.invalid/file',
                         'https://user:password@example.blob.core.windows.net/file'):
            with patch.object(preview.urllib.request, 'build_opener') as opener:
                opener.return_value.open.side_effect = urllib.error.HTTPError(
                    'https://api.github.com', 302, 'Found', {'Location': location}, None)
                with self.subTest(location=location), self.assertRaises(release_tool.ReleaseError):
                    preview.artifact_location(11, 'synthetic-api-token')

    def test_untracked_or_modified_publisher_helpers_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            scripts = root / 'release/scripts'
            scripts.mkdir(parents=True)
            (scripts / 'publish_preview_ghcr.py').write_text('# synthetic tracked publisher')
            (scripts / 'helper.py').write_text('# synthetic helper')
            files = b'release/scripts/helper.py\nrelease/scripts/publish_preview_ghcr.py\n'
            with patch.object(preview, 'ROOT', root):
                with patch.object(preview.qualification, 'git', side_effect=[files, b'']):
                    preview.verify_runtime_source()
                for listing, status in ((files.splitlines()[0] + b'\n', b''),
                                        (files, b'M release/scripts/helper.py\n')):
                    with patch.object(preview.qualification, 'git', side_effect=[listing, status]):
                        with self.assertRaises(release_tool.ReleaseError):
                            preview.verify_runtime_source()


if __name__ == '__main__':
    unittest.main()
