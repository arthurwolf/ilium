import hashlib
import io
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
            for tag in ('v0.1.1', 'v0.1.1-linux.2', ''):
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


class AppendAarch64Tests(unittest.TestCase):
    def make_arm(self, root, optional=()):
        native, packages = root / 'arm-native', root / 'arm-packages'
        native.mkdir()
        packages.mkdir()
        (native / publisher.AARCH64_TARBALL).write_bytes(b'arm tarball')
        for name in publisher.AARCH64_REQUIRED_PACKAGES + tuple(optional):
            (packages / name).write_bytes(b'arm ' + name.encode())
        return native, packages

    def x86_sums(self):
        return ''.join(f'{"1" * 64}  {name}\n' for name in publisher.ASSETS) + ''

    def release(self, extra=()):
        names = list(publisher.ASSETS) + [publisher.SUMS_NAME] + list(extra)
        return {'id': 7, 'draft': False, 'prerelease': True, 'assets': [{'name': n, 'digest': 'sha256:' + '1' * 64} for n in names]}

    def test_set_uses_aarch64_names_and_skips_unqualified_formats(self):
        with tempfile.TemporaryDirectory() as root:
            files = publisher.verify_aarch64_inventory(*self.make_arm(Path(root)), publisher.PREVIEW_TAG)
            self.assertEqual(sorted(files), sorted(['ilium-linux-aarch64.tar.gz', 'ilium-linux-aarch64.deb', 'ilium-linux-aarch64.AppImage']))
            self.assertTrue(all('x86_64' not in name for name in files))

    def test_optional_format_is_included_when_present_and_missing_required_fails(self):
        with tempfile.TemporaryDirectory() as root:
            native, packages = self.make_arm(Path(root), optional=('ilium-linux-aarch64.rpm',))
            self.assertIn('ilium-linux-aarch64.rpm', publisher.verify_aarch64_inventory(native, packages, publisher.PREVIEW_TAG))
            (packages / 'ilium-linux-aarch64.deb').unlink()
            with self.assertRaises(SystemExit):
                publisher.verify_aarch64_inventory(native, packages, publisher.PREVIEW_TAG)

    def test_wrong_tag_and_bad_native_checksum_fail(self):
        with tempfile.TemporaryDirectory() as root:
            native, packages = self.make_arm(Path(root))
            with self.assertRaises(SystemExit):
                publisher.verify_aarch64_inventory(native, packages, 'v0.1.1')
            (native / 'SHA256SUMS').write_text(f'{"0" * 64}  {publisher.AARCH64_TARBALL}\n', encoding='ascii')
            with self.assertRaises(SystemExit):
                publisher.verify_aarch64_inventory(native, packages, publisher.PREVIEW_TAG)

    def test_merge_keeps_existing_lines_byte_for_byte_and_appends_arm(self):
        with tempfile.TemporaryDirectory() as root:
            files = publisher.verify_aarch64_inventory(*self.make_arm(Path(root)), publisher.PREVIEW_TAG)
            existing = self.x86_sums()
            merged = publisher.merge_checksums(existing, files)
            self.assertTrue(merged.startswith(existing))
            added = merged[len(existing):].splitlines()
            self.assertEqual([line.split('  ')[1] for line in added],
                             ['ilium-linux-aarch64.tar.gz', 'ilium-linux-aarch64.deb', 'ilium-linux-aarch64.AppImage'])
            self.assertIn(f"{hashlib.sha256(b'arm tarball').hexdigest()}  ilium-linux-aarch64.tar.gz", added)

    def test_merge_refuses_duplicates_malformed_and_incomplete_x86_inventory(self):
        files = {'ilium-linux-aarch64.deb': Path(__file__)}
        for bad in (self.x86_sums() + f'{"2" * 64}  ilium-linux-aarch64.deb\n', 'garbage\n', ''.join(self.x86_sums().splitlines(True)[1:])):
            with self.subTest(bad=bad[:20]), self.assertRaises(SystemExit):
                publisher.merge_checksums(bad, files)

    def test_plan_refuses_existing_remote_arm_asset(self):
        files = {'ilium-linux-aarch64.deb': Path(__file__)}
        with self.assertRaises(SystemExit):
            publisher.append_plan(files, self.release(extra=['ilium-linux-aarch64.deb']), self.x86_sums())

    def test_dry_run_writes_nothing_to_github(self):
        with tempfile.TemporaryDirectory() as root, mock.patch.dict('os.environ', {'GITHUB_REPOSITORY': 'o/r'}), \
                mock.patch.object(publisher, 'release_for_tag', return_value=self.release()), \
                mock.patch.object(publisher, 'download_sums', return_value=self.x86_sums()), \
                mock.patch.object(publisher.subprocess, 'run') as run, mock.patch.object(publisher, 'github') as api:
            native, packages = self.make_arm(Path(root))
            output = io.StringIO()
            with redirect_stdout(output):
                code = publisher.main(['append-aarch64-dry-run', '--tag', publisher.PREVIEW_TAG, '--commit', COMMIT,
                                       '--native-directory', str(native), '--packages-directory', str(packages),
                                       '--work', str(Path(root) / 'work')])
            self.assertEqual(code, 0)
            run.assert_not_called()
            api.assert_not_called()
            self.assertIn('append-dry-run-passed', output.getvalue())

    def test_upload_never_clobbers_assets_and_clobbers_only_checksums(self):
        with tempfile.TemporaryDirectory() as root, mock.patch.dict('os.environ', {'GITHUB_REPOSITORY': 'o/r'}):
            native, packages = self.make_arm(Path(root))
            files = publisher.verify_aarch64_inventory(native, packages, publisher.PREVIEW_TAG)
            work = Path(root) / 'work'
            work.mkdir()
            merged = publisher.merge_checksums(self.x86_sums(), files)
            (work / publisher.SUMS_NAME).write_text(merged, encoding='ascii')
            readback = self.release()
            readback['assets'] += [{'name': n, 'digest': 'sha256:' + publisher.sha256(p)} for n, p in files.items()]
            for row in readback['assets']:
                if row['name'] == publisher.SUMS_NAME:
                    row['digest'] = 'sha256:' + publisher.sha256(work / publisher.SUMS_NAME)
            with mock.patch.object(publisher, 'release_for_tag', return_value=self.release()), \
                    mock.patch.object(publisher, 'download_sums', return_value=self.x86_sums()), \
                    mock.patch.object(publisher.subprocess, 'run') as run, \
                    mock.patch.object(publisher, 'github', return_value=readback):
                publisher.append_aarch64(files, 'o/r', work, dry_run=False)
            uploads = [call.args[0] for call in run.call_args_list]
            self.assertNotIn('--clobber', uploads[0])
            self.assertEqual(sorted(a for a in uploads[0] if a.endswith(tuple(files))), sorted(map(str, files.values())))
            self.assertIn('--clobber', uploads[1])
            self.assertEqual(uploads[1][-1], str(work / publisher.SUMS_NAME))

    def test_changed_x86_digest_after_upload_fails(self):
        with tempfile.TemporaryDirectory() as root, mock.patch.dict('os.environ', {'GITHUB_REPOSITORY': 'o/r'}):
            files = publisher.verify_aarch64_inventory(*self.make_arm(Path(root)), publisher.PREVIEW_TAG)
            work = Path(root) / 'work'
            work.mkdir()
            readback = self.release()
            merged = publisher.merge_checksums(self.x86_sums(), files)
            (work / publisher.SUMS_NAME).write_text(merged, encoding='ascii')
            readback['assets'] += [{'name': n, 'digest': 'sha256:' + publisher.sha256(p)} for n, p in files.items()]
            readback['assets'][0]['digest'] = 'sha256:' + '9' * 64
            readback['assets'][len(publisher.ASSETS)]['digest'] = 'sha256:' + publisher.sha256(work / publisher.SUMS_NAME)
            with mock.patch.object(publisher, 'release_for_tag', return_value=self.release()), \
                    mock.patch.object(publisher, 'download_sums', return_value=self.x86_sums()), \
                    mock.patch.object(publisher.subprocess, 'run'), mock.patch.object(publisher, 'github', return_value=readback):
                with self.assertRaises(SystemExit) as caught:
                    publisher.append_aarch64(files, 'o/r', work, dry_run=False)
            self.assertIn('x86_64 asset', str(caught.exception))


if __name__ == '__main__':
    unittest.main()
