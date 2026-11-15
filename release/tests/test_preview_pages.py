"""Preview Pages contracts without provider writes or production access."""
import hashlib
from email.message import Message
import json
from pathlib import Path
import shutil
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import pages
import preview_pages
import release_tool


class PreviewPagesTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'release/site').mkdir(parents=True)
        for name in ('index.html', '_headers', '404.html'):
            shutil.copyfile(ROOT / 'release/site' / name, self.root / 'release/site' / name)
        shutil.copyfile(ROOT / 'release/targets.toml', self.root / 'release/targets.toml')
        self.targets = release_tool.load_targets(self.root / 'release/targets.toml')
        pins = {'ilium-linux-x86_64.tar.gz': {'version': '0.1.1-linux.3', 'sha256': 'a' * 64},
                'ilium-linux-aarch64.tar.gz': {'version': '0.1.1-linux.3', 'sha256': 'b' * 64},
                'ilium-macos-aarch64.tar.gz': {'version': '0.1.1-preview.4', 'sha256': 'c' * 64}}
        (self.root / 'release/linux-preview-pins.json').write_text(json.dumps({'archives': pins}))
        posix = release_tool.posix_table(self.targets)
        block = release_tool.preview_pins_block({name: (pin['version'], pin['sha256'])
                                                 for name, pin in sorted(pins.items())})
        (self.root / 'release/install.sh').write_text(posix + '\n' + block + '\n')
        windows = pages.windows_table(self.targets)
        ps = (windows + '\nfunction Resolve-IliumLatestResponse { return \'0.1.1-preview.6\' }\n'
              + "function Get-IliumLatestVersion {}\n$pinnedVersion = '0.1.1-preview.6'\n"
              + "$pinnedArchive = 'ilium-windows-x86_64.zip'\n$pinnedHash = '" + 'd' * 64 + "'\n")
        (self.root / 'release/install.ps1').write_text(ps)
        self.pins = {**pins, 'ilium-windows-x86_64.zip': {'version': '0.1.1-preview.6', 'sha256': 'd' * 64}}

    def test_evidence_and_artifact_creation_are_exclusive(self):
        evidence = self.root / 'receipt.json'
        preview_pages.write_json(evidence, {'first': True})
        with self.assertRaises(FileExistsError):
            preview_pages.write_json(evidence, {'first': False})
        self.assertEqual(preview_pages.read_json(evidence), {'first': True})
        output = self.root / 'site'
        output.mkdir()
        sentinel = output / 'install.sh'
        sentinel.write_bytes(b'other writer')
        arguments = SimpleNamespace(source=self.root, tag='v0.1.1-preview.7', output=output)
        with patch.object(preview_pages.pipeline, 'public_latest_tag', return_value=None), \
             patch.object(preview_pages, 'source_commit', return_value='e' * 40), \
             patch.object(preview_pages, 'published_pin_evidence', return_value={}):
            with self.assertRaises(FileExistsError):
                preview_pages.prepare(arguments)
        self.assertEqual(sentinel.read_bytes(), b'other writer')

    def test_baseline_includes_exact_security_headers_and_current_deployment(self):
        response_headers = Message()
        response_headers['Content-Type'] = 'text/plain; charset=utf-8'
        response_headers['Cache-Control'] = 'no-store, max-age=0'
        response_headers['Content-Security-Policy'] = "default-src 'none'"
        response_headers['X-Content-Type-Options'] = 'nosniff'
        def request(url, *, headers, limit):
            if url.endswith('/manifest.json') or url.endswith('/ilium-resource-that-must-not-exist'):
                raise preview_pages.pipeline.HTTPFailure(404)
            return url.encode(), response_headers
        with patch.object(preview_pages.pipeline, 'request', side_effect=request), \
             patch.object(preview_pages.pipeline, 'missing_response', return_value=(b'old 404', response_headers)), \
             patch.object(preview_pages.pipeline, 'pages_project', return_value=('project', {
                 'production_branch': 'master', 'canonical_deployment': {'id': 'old'}})), \
             patch.object(preview_pages.pipeline, 'cloudflare', return_value={
                 'id': 'old', 'environment': 'production', 'latest_stage': {'status': 'success'}}):
            preview_pages.baseline(SimpleNamespace(output=self.root / 'baseline.json'))
        record = preview_pages.read_json(self.root / 'baseline.json')
        self.assertEqual(record['canonical_deployment_id'], 'old')
        self.assertEqual(record['routes']['/manifest.json']['status'], 404)
        self.assertEqual(record['routes']['/install.sh']['content_security_policy'], "default-src 'none'")
        self.assertEqual(record['routes']['/install.ps1']['x_content_type_options'], 'nosniff')
        self.assertEqual(record['routes']['/manifest.json']['sha256'], hashlib.sha256(b'old 404').hexdigest())

    def test_upload_uncertain_exit_reconciles_one_owned_provider_record(self):
        directory = self.root / 'site'
        directory.mkdir()
        (directory / 'install.sh').write_bytes(b'installer')
        log = self.root / 'upload.log'
        def deployments(_path):
            intent = preview_pages.read_json(log.with_suffix('.intent.json'))
            return [{'id': 'owned', 'url': 'https://owned.ilium-setup.pages.dev',
                     'latest_stage': {'status': 'success'},
                     'deployment_trigger': {'metadata': {'commit_message': intent['owner_marker'],
                                                         'commit_hash': 'e' * 40,
                                                         'branch': 'preview-e'}}}]
        with patch.object(preview_pages.pipeline, 'logged', side_effect=OSError('connection lost')) as upload, \
             patch.object(preview_pages.pipeline, 'pages_project', return_value=('project', {})), \
             patch.object(preview_pages.pipeline, 'cloudflare', side_effect=deployments):
            record = preview_pages.owned_deployment(directory, 'preview-e', 'e' * 40, log)
        self.assertEqual(record['id'], 'owned')
        self.assertEqual(upload.call_count, 1)
        self.assertEqual(preview_pages.read_json(log.with_suffix('.readback.json'))['deployment_id'], 'owned')

    def test_promotion_requires_unchanged_baseline_and_verifies_canonical_bytes(self):
        directory = self.root / 'site'
        directory.mkdir()
        (directory / 'manifest.json').write_bytes(b'candidate')
        (self.root / 'preview.json').write_text(json.dumps({'deployment_id': 'verified-preview'}))
        (self.root / 'baseline.json').write_text(json.dumps({'canonical_deployment_id': 'old'}))
        arguments = SimpleNamespace(artifact=directory, source=self.root,
                                    preview_receipt=self.root / 'preview.json',
                                    baseline=self.root / 'baseline.json',
                                    log=self.root / 'production.log', output=self.root / 'production.json')
        metadata = {'source_commit': 'e' * 40}
        record = {'id': 'new', 'environment': 'production'}
        with patch.object(preview_pages, 'verify_preview_receipt', return_value=metadata), \
             patch.object(preview_pages, 'assert_source_artifact', return_value=metadata), \
             patch.object(preview_pages.pipeline, 'public_latest_tag', return_value=None), \
             patch.object(preview_pages, 'assert_baseline', side_effect=release_tool.ReleaseError('changed')) as gate, \
             patch.object(preview_pages, 'owned_deployment') as upload:
            with self.assertRaises(release_tool.ReleaseError):
                preview_pages.promote(arguments)
        gate.assert_called_once()
        upload.assert_not_called()
        with patch.object(preview_pages, 'verify_preview_receipt', return_value=metadata), \
             patch.object(preview_pages, 'assert_source_artifact', return_value=metadata), \
             patch.object(preview_pages.pipeline, 'public_latest_tag', return_value=None), \
             patch.object(preview_pages, 'assert_baseline'), \
             patch.object(preview_pages, 'owned_deployment', return_value=record), \
             patch.object(preview_pages.pipeline, 'pages_project', return_value=('project', {
                 'canonical_deployment': {'id': 'new'}})), \
             patch.object(preview_pages.pipeline, 'hosted_bytes') as hosted:
            preview_pages.promote(arguments)
        hosted.assert_called_once_with(directory, 'https://' + pages.HOST)
        self.assertEqual(preview_pages.read_json(arguments.output)['deployment_id'], 'new')

    def test_recovery_is_read_only_even_if_foreign_writer_changes_production(self):
        baseline = {'canonical_deployment_id': 'old', 'routes': {'/': {'sha256': 'a'}}}
        (self.root / 'baseline.json').write_text(json.dumps(baseline))
        log = self.root / 'production.log'
        log.with_suffix('.intent.json').write_text(json.dumps({
            'schema': 1, 'branch': 'master', 'source_commit': 'e' * 40,
            'owner_marker': 'our-upload'}))
        arguments = SimpleNamespace(baseline=self.root / 'baseline.json', log=log,
                                    output=self.root / 'recovery.json')
        with patch.object(preview_pages.pipeline, 'pages_project', return_value=('project', {
                 'canonical_deployment': {'id': 'foreign'}})), \
             patch.object(preview_pages.pipeline, 'cloudflare', return_value=[{
                 'id': 'foreign', 'deployment_trigger': {'metadata': {'commit_message': 'someone-else'}}}]) as provider, \
             patch.object(preview_pages, 'live_snapshot', return_value={'/': {'sha256': 'b'}}):
            preview_pages.recover(arguments)
        self.assertEqual(preview_pages.read_json(arguments.output)['state'], 'other-production-writer-preserved')
        self.assertEqual(provider.call_count, 1)
        self.assertTrue(all(call.kwargs.get('method', 'GET') == 'GET' for call in provider.call_args_list))

    def test_recovery_without_upload_intent_records_status_without_provider_write(self):
        (self.root / 'baseline.json').write_text(json.dumps({
            'canonical_deployment_id': 'old', 'routes': {'/': {'sha256': 'old'}}}))
        arguments = SimpleNamespace(baseline=self.root / 'baseline.json',
                                    log=self.root / 'missing-upload.log',
                                    output=self.root / 'no-intent.json')
        with patch.object(preview_pages.pipeline, 'pages_project', return_value=('project', {
                 'canonical_deployment': {'id': 'old'}})), \
             patch.object(preview_pages.pipeline, 'cloudflare') as provider, \
             patch.object(preview_pages, 'live_snapshot', return_value={'/': {'sha256': 'old'}}):
            preview_pages.recover(arguments)
        result = preview_pages.read_json(arguments.output)
        self.assertEqual(result['state'], 'no-upload-intent-baseline-live')
        self.assertFalse(result['has_upload_intent'])
        provider.assert_not_called()
        arguments.output = self.root / 'no-intent-foreign.json'
        with patch.object(preview_pages.pipeline, 'pages_project', return_value=('project', {
                 'canonical_deployment': {'id': 'foreign'}})), \
             patch.object(preview_pages.pipeline, 'cloudflare') as provider, \
             patch.object(preview_pages, 'live_snapshot', return_value={'/': {'sha256': 'new'}}):
            preview_pages.recover(arguments)
        self.assertEqual(preview_pages.read_json(arguments.output)['state'], 'no-upload-intent-other-production')
        provider.assert_not_called()

    def test_preview_artifact_is_six_files_and_describes_only_pinned_targets(self):
        targets, pins, installers = preview_pages.installer_pins(self.root, 'v0.1.1-preview.7')
        self.assertEqual(pins, self.pins)
        files = preview_pages.site_files(self.root, 'v0.1.1-preview.7', targets, pins,
                                        installers, 'e' * 40, {})
        self.assertEqual(set(files), pages.FILES)
        self.assertIn(b'x86_64-pc-windows-msvc', files['index.html'])
        self.assertNotIn(b'x86_64-apple-darwin', files['index.html'])
        self.assertIn(b'ilium-windows-x86_64.zip', files['manifest.json'])
        self.assertIn(b'not a five-platform stable release', files['index.html'])
        self.assertEqual(files['install.sh'], (self.root / 'release/install.sh').read_bytes())
        self.assertEqual(files['install.ps1'], (self.root / 'release/install.ps1').read_bytes())
        directory = self.root / 'out'
        directory.mkdir()
        for name, content in files.items():
            (directory / name).write_bytes(content)
        self.assertEqual(preview_pages.verify_artifact(directory)['source_commit'], 'e' * 40)
        (directory / 'install.ps1').write_bytes(b'different')
        with self.assertRaises(release_tool.ReleaseError):
            preview_pages.verify_artifact(directory)

    def test_windows_latest_fallback_must_match_requested_published_pin(self):
        installer = self.root / 'release/install.ps1'
        installer.write_text(installer.read_text().replace("return '0.1.1-preview.6'", "return '0.1.1-preview.5'"))
        with self.assertRaises(release_tool.ReleaseError):
            preview_pages.installer_pins(self.root, 'v0.1.1-preview.7')

    def test_macos_intel_pin_is_allowed_only_with_published_checksum_match(self):
        archive_name = 'ilium-macos-x86_64.tar.gz'
        archive_bytes = b'qualified macOS Intel preview archive fixture'
        archive_hash = hashlib.sha256(archive_bytes).hexdigest()
        pins = dict(self.pins)
        pins[archive_name] = {'version': '0.1.1-preview.7', 'sha256': archive_hash}
        (self.root / 'release/linux-preview-pins.json').write_text(
            json.dumps({'archives': {name: pin for name, pin in pins.items()
                                     if name != 'ilium-windows-x86_64.zip'}}))
        installer = self.root / 'release/install.sh'
        original = installer.read_text()
        block = release_tool.preview_pins_block({name: (pin['version'], pin['sha256'])
                                                 for name, pin in sorted(pins.items())
                                                 if name != 'ilium-windows-x86_64.zip'})
        installer.write_text(original.split(release_tool.PREVIEW_PINS_START)[0] + block + '\n')
        _targets, loaded_pins, _installers = preview_pages.installer_pins(self.root, 'v0.1.1-preview.7')
        self.assertEqual(loaded_pins[archive_name]['sha256'], archive_hash)

        def published(checksum_hash):
            sums = (checksum_hash + '  ' + archive_name + '\n').encode()
            def github(path):
                tag = path.removeprefix('releases/tags/')
                return {'tag_name': tag, 'prerelease': True, 'draft': False, 'immutable': True,
                        'id': 7, 'assets': [
                            {'name': 'SHA256SUMS', 'digest': 'sha256:' + hashlib.sha256(sums).hexdigest(),
                             'browser_download_url': preview_pages.pipeline.ORIGIN + '/download/' + tag + '/SHA256SUMS'},
                            {'name': archive_name, 'digest': None,
                             'browser_download_url': preview_pages.pipeline.ORIGIN + '/download/' + tag + '/' + archive_name}]}
            def request(url, *, limit):
                return (sums if url.endswith('/SHA256SUMS') else archive_bytes), {}
            return preview_pages.published_pin_evidence({archive_name: pins[archive_name]},
                                                        github=github, request=request)
        self.assertEqual(published(archive_hash)['0.1.1-preview.7']['archives'][archive_name]['sha256'], archive_hash)
        with self.assertRaises(release_tool.ReleaseError):
            published('0' * 64)

    def test_published_pin_readback_rejects_wrong_asset_digest(self):
        archives = {name: ('asset-' + name).encode() for name in self.pins}
        carried_name = 'ilium-windows-x86_64.exe'
        archives[carried_name] = b'carried Windows installer fixture'
        carried_hash = hashlib.sha256(archives[carried_name]).hexdigest()
        verified_pins = {name: {'version': pin['version'], 'sha256': hashlib.sha256(archives[name]).hexdigest()}
                         for name, pin in self.pins.items()}
        sums = (''.join(pin['sha256'] + '  ' + name + '\n' for name, pin in verified_pins.items())
                + carried_hash + '  ' + carried_name + '\n').encode()
        sha = hashlib.sha256(sums).hexdigest()
        def github(path):
            tag = path.removeprefix('releases/tags/')
            selected = {name: pin for name, pin in verified_pins.items()
                        if 'v' + pin['version'] == tag or tag in ('v0.1.1-preview.6', 'v0.1.1-preview.7')}
            if tag in ('v0.1.1-preview.6', 'v0.1.1-preview.7'):
                selected[carried_name] = {'sha256': carried_hash}
            assets = [{'name': 'SHA256SUMS', 'digest': 'sha256:' + sha,
                       'size': len(sums),
                       'browser_download_url': preview_pages.pipeline.ORIGIN + '/download/' + tag + '/SHA256SUMS'}]
            assets += [{'name': name, 'digest': None if name in ('ilium-windows-x86_64.zip', carried_name) else 'sha256:' + pin['sha256'],
                        'size': len(archives[name]),
                        'browser_download_url': preview_pages.pipeline.ORIGIN + '/download/' + tag + '/' + name}
                       for name, pin in selected.items()]
            return {'tag_name': tag, 'prerelease': True, 'draft': False,
                    'immutable': True, 'id': 4, 'assets': assets}
        def request(url, *, limit):
            return (sums if url.endswith('/SHA256SUMS') else archives[url.rsplit('/', 1)[1]]), {}
        evidence = preview_pages.published_pin_evidence(verified_pins,
                                                         carrier_tag='v0.1.1-preview.7',
                                                         github=github, request=request)
        self.assertEqual(len(evidence['carry_forward_preview_6']['assets']), len(verified_pins) + 1)
        changed_sums = sums.replace(carried_hash.encode(), b'f' * 64)
        def changed_github(path):
            record = github(path)
            if path.endswith('v0.1.1-preview.7'):
                record['assets'][0]['digest'] = 'sha256:' + hashlib.sha256(changed_sums).hexdigest()
                for asset in record['assets']:
                    if asset['name'] == carried_name:
                        asset['digest'] = 'sha256:' + 'f' * 64
            return record
        def changed_request(url, *, limit):
            if url.endswith('/v0.1.1-preview.7/SHA256SUMS'):
                return changed_sums, {}
            return request(url, limit=limit)
        with self.assertRaises(release_tool.ReleaseError):
            preview_pages.published_pin_evidence(verified_pins, carrier_tag='v0.1.1-preview.7',
                                                 github=changed_github, request=changed_request)
        wrong = dict(verified_pins)
        wrong['ilium-windows-x86_64.zip'] = {'version': '0.1.1-preview.6', 'sha256': 'f' * 64}
        with self.assertRaises(release_tool.ReleaseError):
            preview_pages.published_pin_evidence(wrong, carrier_tag='v0.1.1-preview.7',
                                                 github=github, request=request)


if __name__ == '__main__':
    unittest.main()
