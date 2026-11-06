import hashlib
import io
import tarfile
import tempfile
import unittest
from unittest import mock
from contextlib import redirect_stdout
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))  # Use the production publisher module.
import publish_linux_preview as publisher  # noqa: E402

COMMIT = 'a' * 40


class PublishLinuxPreviewTests(unittest.TestCase):
    def make_release(self, root):
        native = root / 'native'
        packages = root / 'packages'
        native.mkdir()
        packages.mkdir()
        tarball = native / publisher.TARBALL
        tarball.write_bytes(b'tarball bytes')
        digest = hashlib.sha256(b'tarball bytes').hexdigest()
        (native / 'SHA256SUMS').write_text(f'{digest}  {publisher.TARBALL}\n', encoding='ascii')
        for name in publisher.PACKAGES:
            (packages / name).write_bytes(b'package ' + name.encode())
        return native, packages

    def test_inventory_lists_every_asset_with_its_real_digest(self):
        with tempfile.TemporaryDirectory() as root:
            native, packages = self.make_release(Path(root))
            files, inventory = publisher.verify_inventory(native, packages, publisher.PREVIEW_TAG)
            self.assertEqual(sorted(files), sorted(publisher.ASSETS))
            self.assertEqual(len(inventory.splitlines()), len(publisher.ASSETS))
            self.assertIn(f"{hashlib.sha256(b'tarball bytes').hexdigest()}  {publisher.TARBALL}", inventory)

    def test_refuses_any_tag_other_than_the_preview_tag(self):
        with tempfile.TemporaryDirectory() as root:
            native, packages = self.make_release(Path(root))
            for tag in ('v0.1.1', 'v0.1.1-linux.4', ''):
                with self.subTest(tag=tag), self.assertRaises(SystemExit):
                    publisher.verify_inventory(native, packages, tag)

    def test_rejects_a_native_checksum_that_does_not_match_the_tarball(self):
        with tempfile.TemporaryDirectory() as root:
            native, packages = self.make_release(Path(root))
            (native / 'SHA256SUMS').write_text(f'{"0" * 64}  {publisher.TARBALL}\n', encoding='ascii')
            with self.assertRaises(SystemExit):
                publisher.verify_inventory(native, packages, publisher.PREVIEW_TAG)

    def test_missing_or_empty_package_fails_closed(self):
        with tempfile.TemporaryDirectory() as root:
            native, packages = self.make_release(Path(root))
            (packages / publisher.PACKAGES[0]).unlink()
            with self.assertRaises(SystemExit):
                publisher.verify_inventory(native, packages, publisher.PREVIEW_TAG)
            (packages / publisher.PACKAGES[0]).write_bytes(b'')
            with self.assertRaises(SystemExit):
                publisher.verify_inventory(native, packages, publisher.PREVIEW_TAG)

    def test_dry_run_prints_plan_without_github_access(self):
        with tempfile.TemporaryDirectory() as root:
            native, packages = self.make_release(Path(root))
            output = io.StringIO()
            with redirect_stdout(output):
                code = publisher.main(['dry-run', '--tag', publisher.PREVIEW_TAG, '--commit', COMMIT,
                                       '--native-directory', str(native), '--packages-directory', str(packages),
                                       '--work', str(Path(root) / 'work')])
            self.assertEqual(code, 0)
            self.assertIn('dry-run-passed', output.getvalue())
            self.assertIn(publisher.TARBALL, output.getvalue())


def make_deb(version):
    """Minimal ar archive holding control.tar.gz with a Version field."""
    control = io.BytesIO()
    with tarfile.open(fileobj=control, mode='w:gz') as archive:
        data = f'Package: ilium\nVersion: {version}\nArchitecture: arm64\n'.encode()
        info = tarfile.TarInfo('./control')
        info.size = len(data)
        archive.addfile(info, io.BytesIO(data))
    members = [('debian-binary', b'2.0\n'), ('control.tar.gz', control.getvalue())]
    out = b'!<arch>\n'
    for name, body in members:
        out += f'{name + "/":<16}{0:<12}{0:<6}{0:<6}{100644:<8}{len(body):<10}`\n'.encode() + body + (b'\n' if len(body) % 2 else b'')
    return out


def make_tarball(version, architecture='aarch64'):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode='w:gz') as archive:
        for name, data in ((f'ilium-linux-{architecture}', None), (f'ilium-linux-{architecture}/VERSION', (version + '\n').encode())):
            info = tarfile.TarInfo(name)
            if data is None:
                info.type = tarfile.DIRTYPE
            else:
                info.size = len(data)
            archive.addfile(info, None if data is None else io.BytesIO(data))
    return buffer.getvalue()


class Aarch64ReleaseTests(unittest.TestCase):
    def make_arm(self, root, version='0.1.1-linux.2', deb_version='0.1.1~linux.2'):
        native, packages = root / 'arm-native', root / 'arm-packages'
        native.mkdir()
        packages.mkdir()
        (native / publisher.AARCH64_TARBALL).write_bytes(make_tarball(version))
        for name in publisher.AARCH64_PACKAGES:
            (packages / name).write_bytes(make_deb(deb_version) if name.endswith('.deb') else b'arm ' + name.encode())
        return native, packages

    def run_main(self, mode, native, packages, root, commit=publisher.AARCH64_COMMIT, tag=publisher.AARCH64_TAG):
        output = io.StringIO()
        with redirect_stdout(output):
            code = publisher.main([mode, '--tag', tag, '--commit', commit, '--native-directory', str(native),
                                   '--packages-directory', str(packages), '--work', str(root / 'work')])
        return code, output.getvalue()

    def test_release_constants_split_architectures_by_tag(self):
        self.assertEqual(publisher.PREVIEW_TAG, 'v0.1.1-linux.1')
        self.assertEqual(publisher.AARCH64_TAG, 'v0.1.1-linux.2')
        self.assertEqual(publisher.AARCH64_COMMIT, 'f69e1c94cf527d9394de2970561874e4ca68bb5a')
        self.assertEqual(publisher.AARCH64_ASSETS, ('ilium-linux-aarch64.tar.gz', 'ilium-linux-aarch64.deb',
                                                    'ilium-linux-aarch64.AppImage', 'ilium-linux-aarch64.flatpak'))
        self.assertTrue(all('x86_64' not in name for name in publisher.AARCH64_ASSETS))
        self.assertTrue(all('aarch64' not in name for name in publisher.ASSETS))

    def test_inventory_is_exactly_four_assets_with_real_digests(self):
        with tempfile.TemporaryDirectory() as root:
            files, inventory = publisher.verify_inventory(*self.make_arm(Path(root)), publisher.AARCH64_TAG)
            self.assertEqual(sorted(files), sorted(publisher.AARCH64_ASSETS))
            names = [line.split('  ')[1] for line in inventory.splitlines()]
            self.assertEqual(names, list(publisher.AARCH64_ASSETS))
            self.assertIn(f"{publisher.sha256(files[publisher.AARCH64_TARBALL])}  ilium-linux-aarch64.tar.gz", inventory)

    def test_rpm_and_snap_are_refused_not_silently_uploaded(self):
        with tempfile.TemporaryDirectory() as root:
            native, packages = self.make_arm(Path(root))
            (packages / 'ilium-linux-aarch64.rpm').write_bytes(b'rpm')
            with self.assertRaises(SystemExit):
                publisher.verify_inventory(native, packages, publisher.AARCH64_TAG)

    def test_missing_empty_or_mismatched_native_checksum_fail_closed(self):
        with tempfile.TemporaryDirectory() as root:
            native, packages = self.make_arm(Path(root))
            (native / 'SHA256SUMS').write_text(f'{"0" * 64}  {publisher.AARCH64_TARBALL}\n', encoding='ascii')
            with self.assertRaises(SystemExit):
                publisher.verify_inventory(native, packages, publisher.AARCH64_TAG)
            (native / 'SHA256SUMS').unlink()
            (packages / 'ilium-linux-aarch64.flatpak').write_bytes(b'')
            with self.assertRaises(SystemExit):
                publisher.verify_inventory(native, packages, publisher.AARCH64_TAG)
            (packages / 'ilium-linux-aarch64.flatpak').unlink()
            with self.assertRaises(SystemExit):
                publisher.verify_inventory(native, packages, publisher.AARCH64_TAG)

    def test_version_consistency_accepts_linux2_and_rejects_stale_artifacts(self):
        with tempfile.TemporaryDirectory() as root:
            native, packages = self.make_arm(Path(root))
            code, output = self.run_main('check-versions', native, packages, Path(root))
            self.assertEqual(code, 0)
            self.assertIn('versions-verified', output)
        for version, deb in (('0.1.1-linux.1', '0.1.1~linux.2'), ('0.1.1-linux.2', '0.1.1~linux.1'), ('0.1.1-linux.2', '0.1.1-linux.2')):
            with self.subTest(tarball=version, deb=deb), tempfile.TemporaryDirectory() as root:
                native, packages = self.make_arm(Path(root), version, deb)
                with self.assertRaises(SystemExit):
                    self.run_main('check-versions', native, packages, Path(root))

    def test_check_versions_applies_to_the_arm_tag_only(self):
        with tempfile.TemporaryDirectory() as root:
            native, packages = self.make_arm(Path(root))
            with self.assertRaises(SystemExit):
                self.run_main('check-versions', native, packages, Path(root), tag=publisher.PREVIEW_TAG)

    def test_arm_release_must_come_from_the_recorded_commit(self):
        with tempfile.TemporaryDirectory() as root:
            native, packages = self.make_arm(Path(root))
            with self.assertRaises(SystemExit):
                self.run_main('dry-run', native, packages, Path(root), commit=COMMIT)

    def test_dry_run_has_no_network_access(self):
        with tempfile.TemporaryDirectory() as root, mock.patch.object(publisher.subprocess, 'run') as run:
            native, packages = self.make_arm(Path(root))
            code, output = self.run_main('dry-run', native, packages, Path(root))
            self.assertEqual(code, 0)
            run.assert_not_called()
            self.assertIn('dry-run-passed', output)
            self.assertIn('ilium-linux-aarch64.flatpak', output)

    def test_github_dry_run_only_reads_and_refuses_an_existing_release_or_tag(self):
        calls = []

        def api(arguments, payload=None, method=None):
            calls.append((arguments, payload, method))
            path = arguments[-1]
            if path.endswith('/releases'):
                return []
            if 'matching-refs' in path:
                return []
            return {'sha': publisher.AARCH64_COMMIT}
        with tempfile.TemporaryDirectory() as root, mock.patch.object(publisher, 'github', side_effect=api), \
                mock.patch.object(publisher.shutil, 'which', return_value='/usr/bin/gh'), mock.patch.object(publisher.subprocess, 'run') as run:
            native, packages = self.make_arm(Path(root))
            code, output = self.run_main('github-dry-run', native, packages, Path(root))
            self.assertEqual(code, 0)
            run.assert_not_called()
            self.assertTrue(calls and all(payload is None and method is None for _, payload, method in calls))
            self.assertIn('github-dry-run-passed', output)
        for taken in ('release', 'tag'):
            def taken_api(arguments, payload=None, method=None, taken=taken):
                path = arguments[-1]
                if path.endswith('/releases'):
                    return [{'tag_name': publisher.AARCH64_TAG}] if taken == 'release' else []
                if 'matching-refs' in path:
                    return [{'ref': 'refs/tags/' + publisher.AARCH64_TAG}] if taken == 'tag' else []
                return {'sha': publisher.AARCH64_COMMIT}
            with self.subTest(taken=taken), tempfile.TemporaryDirectory() as root, mock.patch.object(publisher, 'github', side_effect=taken_api), \
                    mock.patch.object(publisher.shutil, 'which', return_value='/usr/bin/gh'):
                native, packages = self.make_arm(Path(root))
                with self.assertRaises(SystemExit):
                    self.run_main('github-dry-run', native, packages, Path(root))

    def publish_fixture(self, root, drift=None, extra_asset=False):
        native, packages = self.make_arm(root)
        files, inventory = publisher.verify_inventory(native, packages, publisher.AARCH64_TAG)
        work = root / 'work'
        work.mkdir()
        sums = work / publisher.SUMS_NAME
        sums.write_text(inventory, encoding='ascii')
        assets = [{'name': n, 'digest': 'sha256:' + publisher.sha256(p)} for n, p in files.items()]
        assets.append({'name': publisher.SUMS_NAME, 'digest': 'sha256:' + publisher.sha256(sums)})
        if drift:
            assets[0]['digest'] = 'sha256:' + '9' * 64
        if extra_asset:
            assets.append({'name': 'stray.bin', 'digest': 'sha256:' + '1' * 64})
        requests = []

        def api(arguments, payload=None, method=None):
            requests.append((arguments, payload, method))
            if arguments[-1].endswith('/releases') and payload is None:
                return []
            if arguments[-1].endswith('/releases'):
                return {'id': 9}
            return {'id': 9, 'draft': method is None and len(requests) < 5, 'prerelease': True, 'html_url': 'u', 'assets': assets} | ({'draft': False} if method == 'PATCH' or len(requests) > 4 else {})
        return files, inventory, work, requests, api

    def test_publish_creates_a_draft_for_the_new_tag_uploads_four_assets_then_publishes(self):
        with tempfile.TemporaryDirectory() as root:
            files, inventory, work, requests, api = self.publish_fixture(Path(root))
            with mock.patch.object(publisher, 'github', side_effect=api), mock.patch.object(publisher.subprocess, 'run') as run, \
                    redirect_stdout(io.StringIO()) as output:
                publisher.publish(files, inventory, publisher.AARCH64_COMMIT, 'o/r', work, publisher.AARCH64_TAG)
            create = next(payload for arguments, payload, method in requests if payload and 'tag_name' in payload)
            self.assertEqual(create['tag_name'], 'v0.1.1-linux.2')
            self.assertEqual(create['target_commitish'], publisher.AARCH64_COMMIT)
            self.assertTrue(create['draft'] and create['prerelease'])
            self.assertIn('QEMU', create['body'])
            self.assertIn('v0.1.1-linux.1', create['body'])
            command = run.call_args.args[0]
            self.assertEqual(command[:4], ['gh', 'release', 'upload', 'v0.1.1-linux.2'])
            self.assertNotIn('--clobber', command)
            self.assertEqual(sorted(Path(a).name for a in command[command.index('o/r') + 1:]), sorted([*publisher.AARCH64_ASSETS, 'SHA256SUMS']))
            patch = next(payload for arguments, payload, method in requests if method == 'PATCH')
            self.assertEqual(patch, {'draft': False, 'prerelease': True, 'make_latest': 'false'})
            self.assertIn('"state": "passed"', output.getvalue())

    def test_publish_refuses_an_existing_release_before_any_write(self):
        with tempfile.TemporaryDirectory() as root:
            files, inventory, work, requests, api = self.publish_fixture(Path(root))
            with mock.patch.object(publisher, 'github', return_value=[{'tag_name': publisher.AARCH64_TAG}]), \
                    mock.patch.object(publisher.subprocess, 'run') as run:
                with self.assertRaises(SystemExit):
                    publisher.publish(files, inventory, publisher.AARCH64_COMMIT, 'o/r', work, publisher.AARCH64_TAG)
            run.assert_not_called()

    def test_publish_fails_before_going_public_on_digest_drift_or_stray_assets(self):
        for label, options in (('drift', {'drift': True}), ('stray', {'extra_asset': True})):
            with self.subTest(label=label), tempfile.TemporaryDirectory() as root:
                files, inventory, work, requests, api = self.publish_fixture(Path(root), **options)
                with mock.patch.object(publisher, 'github', side_effect=api), mock.patch.object(publisher.subprocess, 'run'):
                    with self.assertRaises(SystemExit):
                        publisher.publish(files, inventory, publisher.AARCH64_COMMIT, 'o/r', work, publisher.AARCH64_TAG)
                self.assertFalse(any(method == 'PATCH' for _, _, method in requests))

    def test_arm_release_notes_state_the_limits(self):
        text = (publisher.NOTES_DIRECTORY / 'v0.1.1-linux.2.md').read_text(encoding='utf-8')
        for phrase in ('QEMU', 'native ARM64 hardware', '4 of 6', 'coming soon', 'v0.1.1-linux.1', 'rpm', 'Snap'):
            self.assertIn(phrase, text)

    def test_failed_append_modes_no_longer_exist(self):
        for mode in ('append-aarch64', 'append-aarch64-dry-run'):
            with self.subTest(mode=mode), self.assertRaises(SystemExit):
                publisher.main([mode, '--tag', publisher.AARCH64_TAG])


if __name__ == '__main__':
    unittest.main()


class CombinedReleaseTests(unittest.TestCase):
    """v0.1.1-linux.3 carries both architectures from one source commit and one version."""
    def make(self, root, x86_version='0.1.1-linux.3', arm_version='0.1.1-linux.3', arm_deb='0.1.1~linux.3'):
        native, packages, arm = root / 'native', root / 'packages', root / 'arm'
        for directory in (native, packages, arm):
            directory.mkdir()
        tarball = make_tarball(x86_version, 'x86_64')
        (native / publisher.TARBALL).write_bytes(tarball)
        (native / 'SHA256SUMS').write_text(f'{publisher.sha256(native / publisher.TARBALL)}  {publisher.TARBALL}\n', encoding='ascii')
        for name in publisher.PACKAGES:
            (packages / name).write_bytes(make_deb('0.1.1~linux.3') if name.endswith('.deb') else b'x86 ' + name.encode())
        (arm / publisher.AARCH64_TARBALL).write_bytes(make_tarball(arm_version))
        for name in publisher.AARCH64_PACKAGES:
            (arm / name).write_bytes(make_deb(arm_deb) if name.endswith('.deb') else b'arm ' + name.encode())
        return native, packages, arm

    def test_constants(self):
        self.assertEqual(publisher.COMBINED_TAG, 'v0.1.1-linux.3')
        self.assertEqual(len(publisher.COMBINED_COMMIT), 40)
        self.assertEqual(len(publisher.COMBINED_ASSETS), len(publisher.ASSETS) + len(publisher.AARCH64_ASSETS))

    def test_inventory_covers_both_architectures_with_real_digests(self):
        with tempfile.TemporaryDirectory() as root:
            native, packages, arm = self.make(Path(root))
            files, inventory = publisher.verify_inventory(native, packages, publisher.COMBINED_TAG, arm)
            self.assertEqual(sorted(files), sorted(publisher.COMBINED_ASSETS))
            self.assertEqual([line.split('  ')[1] for line in inventory.splitlines()], list(publisher.COMBINED_ASSETS))
            for name, path in files.items():
                self.assertIn(f'{publisher.sha256(path)}  {name}', inventory)

    def test_needs_the_arm_directory(self):
        with tempfile.TemporaryDirectory() as root:
            native, packages, _ = self.make(Path(root))
            with self.assertRaises(SystemExit):
                publisher.verify_inventory(native, packages, publisher.COMBINED_TAG)

    def test_any_mixed_embedded_version_is_refused(self):
        for kwargs in ({'x86_version': '0.1.1-linux.1'}, {'arm_version': '0.1.1-linux.2'}, {'arm_deb': '0.1.1~linux.2'}):
            with self.subTest(**kwargs), tempfile.TemporaryDirectory() as root:
                native, packages, arm = self.make(Path(root), **kwargs)
                with self.assertRaises(SystemExit):
                    publisher.verify_inventory(native, packages, publisher.COMBINED_TAG, arm)

    def test_arm_rpm_and_snap_remain_refused(self):
        with tempfile.TemporaryDirectory() as root:
            native, packages, arm = self.make(Path(root))
            (arm / 'ilium-linux-aarch64.snap').write_bytes(b'snap')
            with self.assertRaises(SystemExit):
                publisher.verify_inventory(native, packages, publisher.COMBINED_TAG, arm)

    def test_commit_is_pinned(self):
        with tempfile.TemporaryDirectory() as root:
            native, packages, arm = self.make(Path(root))
            output = io.StringIO()
            args = ['dry-run', '--tag', publisher.COMBINED_TAG, '--native-directory', str(native),
                    '--packages-directory', str(packages), '--aarch64-directory', str(arm), '--work', str(Path(root) / 'w')]
            with redirect_stdout(output):
                self.assertEqual(publisher.main(args + ['--commit', publisher.COMBINED_COMMIT]), 0)
            with self.assertRaises(SystemExit), redirect_stdout(output):
                publisher.main(args + ['--commit', 'b' * 40])
