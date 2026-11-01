import hashlib
import io
import tempfile
import unittest
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


if __name__ == '__main__':
    unittest.main()
