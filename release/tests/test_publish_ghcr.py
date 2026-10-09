"""Offline, nonpublishing tests for the qualified GHCR asset bundle."""
import hashlib
from io import BytesIO
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import yaml

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import publish_ghcr as ghcr
import release_tool


class FakeClient:
    def __init__(self, plan, *, existing=False, public=True, uncertain_push=False):
        self.plan = plan
        self.manifest = make_manifest(plan) if existing else None
        self.public = public
        self.uncertain_push = uncertain_push
        self.token = 'synthetic-token'
        self.digest = 'sha256:' + 'a' * 64
        self.pushes = 0
        self.pulls = 0

    @property
    def immutable_reference(self):
        return ghcr.PACKAGE + '@' + self.digest

    def fetch(self, reference, *, public=False):
        assert reference in (self.plan['reference'], self.immutable_reference)
        if public and not self.public:
            raise release_tool.ReleaseError('synthetic anonymous manifest denied')
        return self.manifest

    def push(self, plan):
        assert plan is self.plan
        self.pushes += 1
        self.manifest = make_manifest(plan)
        if self.uncertain_push:
            raise release_tool.ReleaseError('synthetic lost push response')

    def resolve(self, reference, *, public=False):
        assert reference in (self.plan['reference'], self.immutable_reference)
        if public and not self.public:
            raise release_tool.ReleaseError('synthetic anonymous manifest denied')
        return self.digest

    def pull_public(self, reference, directory):
        assert reference == self.immutable_reference
        self.pulls += 1
        if not self.public:
            raise release_tool.ReleaseError('synthetic anonymous pull denied')
        for file in self.plan['files']:
            (Path(directory) / file.name).write_bytes(file.read_bytes())


def make_manifest(plan):
    return {'schemaVersion': 2, 'mediaType': ghcr.MANIFEST_TYPE,
            'config': {'mediaType': ghcr.ARTIFACT_TYPE},
            'annotations': {'org.opencontainers.image.source': ghcr.SOURCE,
                            'org.opencontainers.image.version': plan['tag']},
            'layers': [{'mediaType': ghcr.LAYER_TYPE, 'digest': 'sha256:' + plan['assets'][file.name],
                        'size': file.stat().st_size,
                        'annotations': {'org.opencontainers.image.title': file.name}}
                       for file in plan['files']]}


def public_metadata():
    return {'id': 73, 'name': 'ilium-release', 'package_type': 'container', 'visibility': 'public',
            'owner': {'login': 'arthurwolf'},
            'repository': {'full_name': 'arthurwolf/ilium'},
            'html_url': 'https://github.com/arthurwolf/ilium/packages/73'}


class PublisherTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.candidate = self.root / 'candidate'
        self.candidate.mkdir()
        self.payload = self.candidate / 'ilium-linux-x86_64.tar.gz'
        self.payload.write_bytes(b'qualified fixture payload')
        self.tag = 'v0.1.1'
        self.commit = 'a' * 40
        self.assets = {self.payload.name: hashlib.sha256(self.payload.read_bytes()).hexdigest()}
        self.plan = {'reference': ghcr.PACKAGE + ':' + self.tag, 'tag': self.tag,
                     'commit': self.commit, 'release_id': 42, 'files': [self.payload],
                     'assets': self.assets}
        self.output = self.root / 'package-receipt.json'
        self.intent = self.root / 'ghcr-intent.json'

    def inputs(self):
        qualification = {'schema': 1, 'state': 'passed', 'publication_allowed': True,
                         'tag': self.tag, 'commit': self.commit, 'archives': self.assets,
                         'public_transport_verified': False}
        quality_file = self.candidate / 'qualification.json'
        quality_file.write_text(json.dumps(qualification))
        final = self.root / 'final-qualification.json'
        final.write_bytes(quality_file.read_bytes())
        published = self.root / 'publication-receipt.json'
        published.write_text(json.dumps({'schema': 1, 'state': 'passed', 'immutable': True,
                                         'tag': self.tag, 'commit': self.commit,
                                         'release_id': 42, 'assets': self.assets}))
        completed = self.root / 'completed-public-qualification.json'
        completed.write_text(json.dumps({'schema': 1, 'state': 'passed',
                                         'publication_allowed': True,
                                         'public_transport_verified': True,
                                         'tag': self.tag, 'commit': self.commit,
                                         'archives': self.assets}))
        channel = self.root / 'final-channel-readback.json'
        channel.write_text(json.dumps({'schema': 1, 'state': 'passed',
                                       'immutable': True, 'tag': self.tag,
                                       'commit': self.commit, 'latest_release_id': 42,
                                       'production_deployment_id': 'fixture-deployment'}))
        arguments = SimpleNamespace(candidate=self.candidate, final_qualification=final,
                                    publication_receipt=published,
                                    completed_qualification=completed,
                                    channel_readback=channel,
                                    manifest=self.root / 'targets.toml',
                                    workspace=self.root / 'Cargo.toml')
        environment = {'GITHUB_EVENT_NAME': 'push', 'GITHUB_REPOSITORY': ghcr.REPOSITORY,
                       'GITHUB_REF': 'refs/tags/' + self.tag, 'GITHUB_SHA': self.commit,
                       'GHCR_TOKEN': 'synthetic-token', 'GITHUB_ACTOR': 'fixture'}
        metadata = {'tag': self.tag, 'commit': self.commit, 'archives': self.assets}
        return arguments, environment, metadata

    def test_payload_hash_mismatch_fails_before_registry_access(self):
        arguments, environment, metadata = self.inputs()
        self.payload.write_bytes(b'tampered fixture payload')
        with patch.object(ghcr.pipeline, 'candidate_data', return_value=(metadata, [{}] * 5)), \
             patch.object(ghcr.pipeline, 'publication_files', return_value=[self.payload]):
            with self.assertRaisesRegex(release_tool.ReleaseError, 'payload differs'):
                ghcr.prepare(arguments, environment)

    def test_unqualified_candidate_fails_before_registry_access(self):
        arguments, environment, metadata = self.inputs()
        quality = json.loads((self.candidate / 'qualification.json').read_text())
        quality['publication_allowed'] = False
        (self.candidate / 'qualification.json').write_text(json.dumps(quality))
        arguments.final_qualification.write_bytes((self.candidate / 'qualification.json').read_bytes())
        with patch.object(ghcr.pipeline, 'candidate_data', return_value=(metadata, [{}] * 5)):
            with self.assertRaisesRegex(release_tool.ReleaseError, 'unqualified'):
                ghcr.prepare(arguments, environment)

    def test_dispatch_never_reaches_candidate_or_registry(self):
        arguments, environment, _metadata = self.inputs()
        environment['GITHUB_EVENT_NAME'] = 'workflow_dispatch'
        with patch.object(ghcr.pipeline, 'candidate_data') as candidate:
            with self.assertRaisesRegex(release_tool.ReleaseError, 'dispatch cannot publish'):
                ghcr.prepare(arguments, environment)
            candidate.assert_not_called()

    def test_incomplete_public_channel_fails_before_registry_access(self):
        arguments, environment, metadata = self.inputs()
        completed = json.loads(arguments.completed_qualification.read_text())
        completed['public_transport_verified'] = False
        arguments.completed_qualification.write_text(json.dumps(completed))
        with patch.object(ghcr.pipeline, 'candidate_data', return_value=(metadata, [{}] * 5)), \
             patch.object(ghcr.pipeline, 'publication_files', return_value=[self.payload]):
            with self.assertRaisesRegex(release_tool.ReleaseError, 'completed public'):
                ghcr.prepare(arguments, environment)

    def test_conflicting_existing_tag_is_never_overwritten(self):
        client = FakeClient(self.plan, existing=True)
        client.manifest['layers'][0]['digest'] = 'sha256:' + 'f' * 64
        with self.assertRaisesRegex(release_tool.ReleaseError, 'bytes differ'):
            ghcr.publish(self.plan, client, self.output, public_metadata)
        self.assertEqual(client.pushes, 0)
        self.assertFalse(self.output.exists())

    def test_failed_push_retains_complete_intent_before_registry_mutation(self):
        class FailingPush(FakeClient):
            def push(inner, plan):
                self.assertTrue(self.intent.is_file())
                inner.pushes += 1
                raise release_tool.ReleaseError('synthetic push failure')

        client = FailingPush(self.plan)
        with self.assertRaisesRegex(release_tool.ReleaseError, 'synthetic push failure'):
            ghcr.publish(self.plan, client, self.output, public_metadata,
                         intent_path=self.intent)
        self.assertEqual(client.pushes, 1)
        self.assertFalse(self.output.exists())
        self.assertEqual(json.loads(self.intent.read_text()), {
            'schema': 1, 'state': 'prepared', 'operation': 'publish-ghcr',
            'repository': ghcr.REPOSITORY, 'package': ghcr.PACKAGE,
            'source': ghcr.SOURCE, 'reference': self.plan['reference'],
            'tag': self.tag, 'commit': self.commit, 'release_id': 42,
            'assets': self.assets})

    def test_windows_intent_does_not_open_unsupported_directory(self):
        with patch.object(ghcr.sys, 'platform', 'win32'), patch.object(
                ghcr.os, 'open', side_effect=PermissionError('Windows directory handle')) as directory_open:
            try:
                intent = ghcr.write_intent(self.intent, self.plan)
            except PermissionError:
                self.fail('Windows intent must not require a POSIX directory handle')
        directory_open.assert_not_called()
        self.assertEqual(json.loads(self.intent.read_text()), intent)

    def test_conflicting_existing_intent_blocks_registry_access(self):
        self.intent.write_text(json.dumps({'schema': 1, 'state': 'other'}))
        client = FakeClient(self.plan)
        with self.assertRaisesRegex(release_tool.ReleaseError, 'intent differs'):
            ghcr.publish(self.plan, client, self.output, public_metadata,
                         intent_path=self.intent)
        self.assertEqual(client.pushes, 0)
        self.assertFalse(self.output.exists())

    def test_failed_anonymous_manifest_retains_verified_identity_and_original_error(self):
        client = FakeClient(self.plan, existing=True, public=False)
        with patch.object(ghcr, 'emit') as emitted:
            with self.assertRaisesRegex(release_tool.ReleaseError, 'synthetic anonymous manifest denied'):
                ghcr.publish(self.plan, client, self.output, public_metadata,
                             intent_path=self.intent)
        warnings = [call for call in emitted.call_args_list if call.args == ('warning',)]
        self.assertEqual(len(warnings), 1)
        self.assertEqual(warnings[0].kwargs['reference'], self.plan['reference'])
        self.assertEqual(warnings[0].kwargs['manifest_digest'], client.digest)
        self.assertEqual(warnings[0].kwargs['intent_path'], str(self.intent.resolve()))
        self.assertTrue(self.intent.is_file())
        self.assertFalse(self.output.exists())
        self.assertEqual(client.pushes, 0)

    def test_anonymous_read_or_public_package_metadata_failure_blocks_receipt(self):
        client = FakeClient(self.plan, existing=True, public=False)
        with self.assertRaisesRegex(release_tool.ReleaseError, 'anonymous manifest denied'):
            ghcr.publish(self.plan, client, self.output, public_metadata)
        self.assertFalse(self.output.exists())
        client.public = True
        private = {**public_metadata(), 'visibility': 'private'}
        with self.assertRaisesRegex(release_tool.ReleaseError, 'not public'):
            ghcr.publish(self.plan, client, self.output, lambda: private)
        self.assertFalse(self.output.exists())
        unlinked = {**public_metadata(), 'repository': None}
        with self.assertRaisesRegex(release_tool.ReleaseError, 'not linked'):
            ghcr.publish(self.plan, client, self.output, lambda: unlinked)
        self.assertFalse(self.output.exists())

    def test_anonymous_pull_hash_mismatch_blocks_receipt(self):
        class CorruptPull(FakeClient):
            def pull_public(self, reference, directory):
                assert reference == self.immutable_reference
                (Path(directory) / self.plan['files'][0].name).write_bytes(b'wrong registry bytes')

        client = CorruptPull(self.plan, existing=True)
        with self.assertRaisesRegex(release_tool.ReleaseError, 'SHA-256 mismatch'):
            ghcr.publish(self.plan, client, self.output, public_metadata)
        self.assertFalse(self.output.exists())

    def test_partial_failure_reconciles_identical_tag_without_second_push(self):
        client = FakeClient(self.plan, uncertain_push=True, public=False)
        with self.assertRaises(release_tool.ReleaseError):
            ghcr.publish(self.plan, client, self.output, public_metadata)
        self.assertEqual(client.pushes, 1)
        self.assertFalse(self.output.exists())
        client.public = True
        receipt = ghcr.publish(self.plan, client, self.output, public_metadata)
        self.assertEqual(client.pushes, 1)
        self.assertEqual(receipt['retry_state'], 'existing-verified')
        self.assertEqual(receipt['assets'], self.assets)
        self.assertTrue(receipt['anonymous_pull_verified'])

    def test_successful_new_public_bundle_has_exact_readback_receipt(self):
        client = FakeClient(self.plan)
        receipt = ghcr.publish(self.plan, client, self.output, public_metadata)
        self.assertEqual(client.pushes, 1)
        self.assertEqual(client.pulls, 1)
        self.assertEqual(json.loads(self.output.read_text()), receipt)
        self.assertEqual(receipt['linked_repository'], ghcr.REPOSITORY)

    def test_metadata_reader_uses_scoped_token_without_emitting_it(self):
        class Response(BytesIO):
            status = 200

        def fake_open(request, *, timeout):
            self.assertEqual(request.full_url, ghcr.PACKAGE_API)
            self.assertEqual(request.get_header('Authorization'), 'Bearer synthetic-token')
            self.assertEqual(timeout, 30)
            return Response(json.dumps(public_metadata()).encode())

        with patch.object(ghcr.urllib.request, 'urlopen', side_effect=fake_open):
            self.assertEqual(ghcr.public_package_metadata('synthetic-token'), public_metadata())

    def test_raw_digest_manifest_must_hash_to_requested_reference(self):
        raw = json.dumps(make_manifest(self.plan), separators=(',', ':')).encode()
        digest = 'sha256:' + hashlib.sha256(raw).hexdigest()
        with tempfile.TemporaryDirectory() as scratch:
            client = ghcr.OrasClient('oras', 'fixture', 'synthetic-token', scratch)

            def fake_run(args, *, public=False):
                self.assertEqual(args[:3], ['manifest', 'fetch', '--output'])
                self.assertTrue(public)
                Path(args[3]).write_bytes(raw)
                return SimpleNamespace(returncode=0, stdout='', stderr='')

            with patch.object(client, 'run', side_effect=fake_run):
                self.assertEqual(client.fetch(ghcr.PACKAGE + '@' + digest, public=True),
                                 make_manifest(self.plan))
                with self.assertRaisesRegex(release_tool.ReleaseError, 'digest manifest bytes differ'):
                    client.fetch(ghcr.PACKAGE + '@sha256:' + 'b' * 64, public=True)

    def test_documented_owner_package_page_is_accepted(self):
        metadata = {**public_metadata(), 'html_url':
                    'https://github.com/users/arthurwolf/packages/container/package/ilium-release'}
        self.assertEqual(ghcr.validate_package_metadata(metadata), metadata['html_url'])

    def test_unrelated_github_package_page_is_rejected(self):
        for url in ('https://github.com/other/repo/packages/73',
                    'https://github.com/arthurwolf/ilium',
                    'https://github.com/arthurwolf/ilium/packages/74',
                    'https://github.com.evil.invalid/arthurwolf/ilium/packages/73'):
            with self.subTest(url=url), self.assertRaisesRegex(
                    release_tool.ReleaseError, 'unexpected package page'):
                ghcr.validate_package_metadata({**public_metadata(), 'html_url': url})

    def test_tag_resolve_drift_cannot_validate_other_manifest(self):
        class DriftClient(FakeClient):
            def fetch(self, reference, *, public=False):
                manifest = super().fetch(reference, public=public)
                if reference == self.immutable_reference:
                    return {**manifest, 'annotations': {
                        **manifest['annotations'],
                        'org.opencontainers.image.source': 'https://github.com/other/repo'}}
                return manifest

        client = DriftClient(self.plan, existing=True)
        with self.assertRaisesRegex(release_tool.ReleaseError, 'different source'):
            ghcr.publish(self.plan, client, self.output, public_metadata)
        self.assertEqual(client.pulls, 0)
        self.assertFalse(self.output.exists())

    def test_anonymous_digest_manifest_source_drift_blocks_receipt(self):
        class DriftClient(FakeClient):
            def fetch(self, reference, *, public=False):
                manifest = super().fetch(reference, public=public)
                if public:
                    return {**manifest, 'annotations': {
                        **manifest['annotations'],
                        'org.opencontainers.image.source': 'https://github.com/other/repo'}}
                return manifest

        client = DriftClient(self.plan, existing=True)
        with self.assertRaisesRegex(release_tool.ReleaseError, 'different source'):
            ghcr.publish(self.plan, client, self.output, public_metadata)
        self.assertEqual(client.pulls, 0)
        self.assertFalse(self.output.exists())

    def test_public_digest_reference_must_resolve_to_selected_digest(self):
        class DriftClient(FakeClient):
            def resolve(self, reference, *, public=False):
                if reference == self.immutable_reference and public:
                    return 'sha256:' + 'b' * 64
                return super().resolve(reference, public=public)

        client = DriftClient(self.plan, existing=True)
        with self.assertRaisesRegex(release_tool.ReleaseError, 'another manifest'):
            ghcr.publish(self.plan, client, self.output, public_metadata)
        self.assertEqual(client.pulls, 0)
        self.assertFalse(self.output.exists())

    def test_tag_move_after_public_pull_blocks_receipt(self):
        class MovingTagClient(FakeClient):
            def resolve(self, reference, *, public=False):
                if reference == self.plan['reference'] and not public and self.pulls:
                    return 'sha256:' + 'b' * 64
                return super().resolve(reference, public=public)

        client = MovingTagClient(self.plan, existing=True)
        with self.assertRaisesRegex(release_tool.ReleaseError, 'tag moved'):
            ghcr.publish(self.plan, client, self.output, public_metadata)
        self.assertEqual(client.pulls, 1)
        self.assertFalse(self.output.exists())


class WorkflowTests(unittest.TestCase):
    def test_package_job_is_push_only_and_blocks_downstream_publication(self):
        workflow = yaml.safe_load((ROOT / '.github/workflows/release.yml').read_text())
        jobs = workflow['jobs']
        package = jobs['ghcr-package']
        self.assertEqual(package['if'], "github.event_name == 'push'")
        self.assertEqual(package['permissions'], {'contents': 'read', 'packages': 'write'})
        self.assertEqual(package['needs'], ['complete'])
        self.assertNotIn('ghcr-package', jobs['github-installs']['needs'])
        self.assertTrue(any(step.get('uses', '').startswith('oras-project/setup-oras@')
                            for step in package['steps']))
        command = '\n'.join(step.get('run', '') for step in package['steps'])
        self.assertIn('publish_ghcr.py', command)
        self.assertIn('--final-qualification', command)
        self.assertIn('--publication-receipt', command)
        self.assertIn('--completed-qualification', command)
        self.assertIn('--channel-readback', command)
        self.assertIn('--intent ghcr-evidence/ghcr-intent.json', command)
        self.assertFalse(any('publish_ghcr.py' in step.get('run', '')
                             for name, job in jobs.items() if name != 'ghcr-package'
                             for step in job['steps']))


if __name__ == '__main__':
    unittest.main()
