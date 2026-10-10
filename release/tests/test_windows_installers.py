"""Offline contracts for the Windows MSI and setup EXE packaging."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ElementTree
import zipfile

import yaml

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import build_windows_installers as installers
import release_pipeline as pipeline
import release_tool

WIX_NAMESPACE = '{http://wixtoolset.org/schemas/v4/wxs}'


def make_archive(directory, version='0.1.0', members=None):
    members = members or {'ilium.exe': b'client', 'ilium-server.exe': b'server',
                          'ilium-animation-helper.exe': b'helper', 'onnxruntime.dll': b'ort',
                          **{name: (ROOT / 'ilium-animation-js/assets/packages' / name).read_bytes()
                             for name in release_tool.APPROVED_PACKAGES},
                          'VERSION': (version + '\n').encode(), 'THIRD-PARTY.txt': b'notices'}
    archive = Path(directory) / 'ilium-windows-x86_64.zip'
    with zipfile.ZipFile(archive, 'w') as package:
        package.writestr(installers.ZIP_PREFIX + '/', b'')
        for name, content in members.items():
            package.writestr(installers.ZIP_PREFIX + '/' + name, content)
    return archive


class PackagingTests(unittest.TestCase):
    def test_extract_flattens_and_hashes_the_exact_members(self):
        with tempfile.TemporaryDirectory() as temporary:
            archive = make_archive(temporary)
            files = installers.extract_package(archive, '0.1.0', Path(temporary) / 'package')
            self.assertEqual(files['ilium.exe'], hashlib.sha256(b'client').hexdigest())
            self.assertEqual(set(files), {'ilium.exe', 'ilium-server.exe', 'ilium-animation-helper.exe',
                                          'onnxruntime.dll', *release_tool.APPROVED_PACKAGES,
                                          'VERSION', 'THIRD-PARTY.txt'})
            for name, digest in release_tool.APPROVED_PACKAGES.items():
                self.assertEqual(files[name], digest)

    def test_extract_rejects_tampered_official_animation(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = make_archive(temporary)
            with zipfile.ZipFile(source) as archive:
                members = {entry.filename.rsplit('/', 1)[-1]: archive.read(entry)
                           for entry in archive.infolist() if not entry.is_dir()}
            members['carpet-1.0.0.iliumanim'] = b'tampered archive'
            archive = make_archive(temporary, members=members)
            with self.assertRaises(release_tool.ReleaseError):
                installers.extract_package(archive, '0.1.0', Path(temporary) / 'tampered')

    def test_extract_rejects_unexpected_traversal_version_and_missing_members(self):
        cases = {
            'traversal': {'../evil.dll': b'x', 'ilium.exe': b'c', 'ilium-server.exe': b's', 'VERSION': b'0.1.0\n', 'THIRD-PARTY.txt': b'n'},
            'unexpected name': {'setup.cmd': b'x', 'ilium.exe': b'c', 'ilium-server.exe': b's', 'VERSION': b'0.1.0\n', 'THIRD-PARTY.txt': b'n'},
            'wrong version': {'ilium.exe': b'c', 'ilium-server.exe': b's', 'VERSION': b'9.9.9\n', 'THIRD-PARTY.txt': b'n'},
            'missing server': {'ilium.exe': b'c', 'VERSION': b'0.1.0\n', 'THIRD-PARTY.txt': b'n'},
        }
        for label, members in cases.items():
            with self.subTest(label), tempfile.TemporaryDirectory() as temporary:
                archive = make_archive(temporary, members=members)
                with self.assertRaises(release_tool.ReleaseError):
                    installers.extract_package(archive, '0.1.0', Path(temporary) / 'package')

    def test_only_plain_release_versions_are_accepted(self):
        self.assertEqual(installers.version_from_tag('v0.1.0'), '0.1.0')
        for tag in ('0.1.0', 'v0.1', 'v0.1.0-rc1', 'v1.2.3.4', 'v01.2.3', 'v1.2.3+build', 'v256.0.0', 'v1.256.0', 'v1.0.65536'):
            with self.subTest(tag), self.assertRaises(release_tool.ReleaseError):
                installers.version_from_tag(tag)

    def test_wix_source_is_per_user_with_user_path_and_clean_removal(self):
        files = {'ilium.exe': 'a', 'ilium-server.exe': 'b', 'ilium-animation-helper.exe': 'c',
                 'onnxruntime.dll': 'd', **{name: release_tool.APPROVED_PACKAGES[name]
                                           for name in release_tool.APPROVED_PACKAGES}}
        source = installers.render_wix(Path('C:/pkg'), files, '0.1.0')
        root = ElementTree.fromstring(source)
        package = root.find(WIX_NAMESPACE + 'Package')
        self.assertEqual(package.get('Scope'), 'perUser')
        self.assertEqual(package.get('Version'), '0.1.0')
        self.assertEqual(package.get('UpgradeCode'), installers.MSI_UPGRADE_CODE)
        self.assertIsNotNone(package.find(WIX_NAMESPACE + 'MajorUpgrade'))
        environment = next(package.iter(WIX_NAMESPACE + 'Environment'))
        self.assertEqual((environment.get('Name'), environment.get('System'), environment.get('Action'), environment.get('Part')), ('PATH', 'no', 'set', 'last'))
        registry = next(package.iter(WIX_NAMESPACE + 'RegistryValue'))
        self.assertEqual((registry.get('Root'), registry.get('KeyPath')), ('HKCU', 'yes'))
        self.assertEqual(len(list(package.iter(WIX_NAMESPACE + 'File'))), 6)
        self.assertEqual({item.get('Directory') for item in package.iter(WIX_NAMESPACE + 'RemoveFolder')}, {'INSTALLFOLDER', 'IliumPrograms'})

    def test_wix_escapes_source_paths(self):
        source = installers.render_wix(Path('C:/a&b'), {'ilium.exe': 'a'}, '0.1.0')
        ElementTree.fromstring(source)
        self.assertIn('a&amp;b', source)

    def test_inno_source_is_per_user_and_resolves_every_placeholder(self):
        source = installers.render_inno(Path('C:/pkg'), Path('C:/out'), '0.1.0')
        for expected in ('PrivilegesRequired=lowest', 'ChangesEnvironment=yes', 'AppVersion=0.1.0', 'DefaultDirName={localappdata}\\Programs\\ilium',
                         'OutputBaseFilename=ilium-windows-x86_64-setup', 'AppId=' + installers.EXE_APP_ID, 'RemoveFromUserPath', 'AddToUserPath'):
            self.assertIn(expected, source)
        self.assertNotIn('@APP_ID@', source)
        self.assertNotIn('@VERSION@', source)

    def test_msi_and_exe_identities_differ(self):
        self.assertNotIn(installers.MSI_UPGRADE_CODE.lower(), installers.EXE_APP_ID.lower())

    def test_cli_render_writes_both_sources(self):
        with tempfile.TemporaryDirectory() as temporary:
            archive = make_archive(temporary)
            self.assertEqual(installers.main(['render', '--tag', 'v0.1.0', '--archive', str(archive), '--work', str(Path(temporary) / 'work')]), 0)
            self.assertTrue((Path(temporary) / 'work/ilium.wxs').is_file())
            self.assertTrue((Path(temporary) / 'work/ilium.iss').is_file())


class PipelineBindingTests(unittest.TestCase):
    def receipt(self, directory, files, archive_sha='a' * 64, tag='v0.1.0'):
        directory = Path(directory)
        hashes = {}
        for name in installers.INSTALLER_NAMES:
            (directory / name).write_bytes(name.encode())
            hashes[name] = hashlib.sha256(name.encode()).hexdigest()
        (directory / installers.RECEIPT_NAME).write_text(json.dumps({'schema': 1, 'tag': tag, 'source_archive_sha256': archive_sha, 'package_files': files, 'installers': hashes}))
        return hashes

    def test_inventory_returns_installers_and_receipt_hashes(self):
        with tempfile.TemporaryDirectory() as temporary:
            files = {'ilium.exe': '1' * 64}
            hashes = self.receipt(temporary, files)
            inventory = pipeline.windows_installer_inventory(temporary, 'v0.1.0', 'a' * 64, files)
            self.assertEqual(set(inventory), set(installers.INSTALLER_NAMES) | {installers.RECEIPT_NAME})
            self.assertEqual({name: inventory[name] for name in hashes}, hashes)

    def test_inventory_rejects_foreign_zip_files_tag_and_tampered_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            files = {'ilium.exe': '1' * 64}
            self.receipt(temporary, files)
            for label, arguments in (('zip', ('v0.1.0', 'b' * 64, files)), ('files', ('v0.1.0', 'a' * 64, {'ilium.exe': '2' * 64})), ('tag', ('v0.2.0', 'a' * 64, files))):
                with self.subTest(label), self.assertRaises(release_tool.ReleaseError):
                    pipeline.windows_installer_inventory(temporary, *arguments)
            (Path(temporary) / installers.MSI_NAME).write_bytes(b'tampered')
            with self.assertRaises(release_tool.ReleaseError):
                pipeline.windows_installer_inventory(temporary, 'v0.1.0', 'a' * 64, files)


class WorkflowTests(unittest.TestCase):
    def setUp(self):
        self.workflow = yaml.load((ROOT / '.github/workflows/release.yml').read_text(), Loader=yaml.BaseLoader)

    def test_installer_job_builds_on_windows_from_the_native_artifact_and_smoke_tests(self):
        job = self.workflow['jobs']['windows-installers']
        self.assertEqual(job['runs-on'], 'windows-2025')
        self.assertEqual(set(job['needs']), {'source', 'native'})
        text = json.dumps(job['steps'])
        for expected in ('native-x86_64-pc-windows-msvc', 'build_windows_installers.py build', 'build_windows_installers.py smoke', 'wix --version', 'innosetup --version'):
            self.assertIn(expected, text)
        self.assertIn('windows-installers', [step.get('with', {}).get('name') for step in job['steps']])

    def test_native_windows_toolchain_install_retries_bounded_transient_feed_failures(self):
        job = self.workflow['jobs']['native']
        install = next(step for step in job['steps'] if step.get('name', '').startswith('Windows Visual Studio 2022 Build Tools'))
        run = install['run']
        for expected in ('$maximumAttempts = 4', '$attempt -le $maximumAttempts', 'HTTP\\s*(429|5\\d\\d)', 'Service Unavailable', '$attempt -eq $maximumAttempts', 'Start-Sleep -Seconds $delaySeconds', 'No Visual Studio 2022'):
            self.assertIn(expected, run)

    def test_aggregate_consumes_installers_and_attestation_covers_them(self):
        aggregate = self.workflow['jobs']['aggregate']
        self.assertIn('windows-installers', aggregate['needs'])
        runs = ' '.join(step.get('run', '') for step in aggregate['steps'])
        self.assertIn('--windows-installers windows-installers', runs)
        attest = next(step for step in self.workflow['jobs']['attest']['steps'] if 'subject-path' in step.get('with', {}))
        for pattern in ('candidate/*.msi', 'candidate/*.exe', 'candidate/windows-installers.json'):
            self.assertIn(pattern, attest['with']['subject-path'])


if __name__ == '__main__':
    unittest.main()
