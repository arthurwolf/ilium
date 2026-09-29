"""Synthetic gate fixtures are not native binary/licence/installation proof."""
import hashlib
import importlib.util
import io
import os
import ssl
import urllib.request
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import native_candidate as bridge
import release_tool
spec = importlib.util.spec_from_file_location('native_install', Path(__file__).with_name('native_install.py'))
install = importlib.util.module_from_spec(spec); spec.loader.exec_module(install)


class NativeCandidateGates(unittest.TestCase):
    def test_unknown_non_system_runtime_cannot_be_declared_system(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            args = SimpleNamespace(build_directory=directory, runtime_directory=None, runtime_license_inventory=None, target='fixture', dumpbin=None, intel_ort_report=None)
            with patch.object(bridge, 'loader_dependencies', return_value=(['libunknown.so.7'], {})):
                with self.assertRaisesRegex(ValueError, 'Unreviewed non-system'):
                    bridge.discovery(args, {'os': 'linux', 'executables': ['ilium', 'ilium-server']}, directory, {})

    def test_distinct_runtime_bytes_block_resolution(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name, contents in [('a', b'first'), ('b', b'second')]:
                (root / name).mkdir(); (root / name / 'onnxruntime.dll').write_bytes(contents)
            with self.assertRaisesRegex(ValueError, 'ambiguous'):
                bridge.select_runtime('onnxruntime.dll', [root])

    def test_same_runtime_bytes_have_recoverable_source_and_pin(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in ('a', 'b'):
                (root / name).mkdir(); (root / name / 'onnxruntime.dll').write_bytes(b'synthetic fixture')
            value = hashlib.sha256(b'synthetic fixture').hexdigest()
            selected = bridge.select_runtime('onnxruntime.dll', [root], value)
            self.assertEqual(bridge.sha(selected), value)
            with self.assertRaisesRegex(ValueError, 'reviewed'):
                bridge.select_runtime('onnxruntime.dll', [root], '0' * 64)

    def test_shared_ort_is_required_on_mac_and_windows(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            args = SimpleNamespace(build_directory=directory, runtime_directory=None, runtime_license_inventory=None, target='fixture', dumpbin=None, intel_ort_report=None)
            for operating_system, dependency in [('macos', '/usr/lib/libSystem.B.dylib'), ('windows', 'KERNEL32.dll')]:
                with self.subTest(os=operating_system), patch.object(bridge, 'loader_dependencies', return_value=([dependency], {})):
                    with self.assertRaisesRegex(ValueError, 'bundled shared'):
                        bridge.discovery(args, {'os': operating_system, 'executables': ['ilium', 'ilium-server']}, directory, {})

    def test_windows_dynamic_crt_is_rejected_even_if_external_inventory_supplies_it(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            build = root / 'build'; build.mkdir()
            candidate = root / 'candidate'; candidate.mkdir()
            (candidate / 'ilium.exe').write_bytes(b'client')
            (candidate / 'ilium-server.exe').write_bytes(b'server')
            runtime = root / 'runtime'; runtime.mkdir()
            (runtime / 'VCRUNTIME140.dll').write_bytes(b'forbidden')
            licence = root / 'LICENSE'; licence.write_text('fixture licence')
            inventory = root / 'inventory.json'
            inventory.write_text(json.dumps({
                'schema': 1, 'state': 'reviewed', 'target': 'x86_64-pc-windows-msvc',
                'files': [{'name': 'VCRUNTIME140.dll', 'sha256': bridge.sha(runtime / 'VCRUNTIME140.dll'),
                           'version': '14', 'reviewed': True, 'license': 'fixture',
                           'license_source': 'fixture', 'license_file': str(licence.resolve()),
                           'license_sha256': bridge.sha(licence)}],
            }))
            args = SimpleNamespace(build_directory=build, runtime_directory=runtime,
                                   runtime_license_inventory=inventory,
                                   target='x86_64-pc-windows-msvc', dumpbin=Path('C:/dumpbin.exe'),
                                   intel_ort_report=None, windows_ort_report=None)
            def graph(_target, path, _dumpbin):
                return (['VCRUNTIME140.dll'] if path.name == 'ilium.exe' else ['KERNEL32.dll'], {})
            with patch.object(bridge, 'loader_dependencies', side_effect=graph):
                with self.assertRaisesRegex(ValueError, 'dynamic CRT'):
                    bridge.discovery(args, {'os': 'windows', 'executables': ['ilium.exe', 'ilium-server.exe']}, candidate, {})

    def test_linux_soname_resolves_to_exact_versioned_build_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); build = root / 'build'; build.mkdir(); candidate = root / 'candidate'; candidate.mkdir()
            (build / 'libonnxruntime.so.1.24.2').write_bytes(b'fixture native bytes')
            args = SimpleNamespace(build_directory=build, runtime_directory=None, runtime_license_inventory=None, target='fixture', dumpbin=None, intel_ort_report=None)
            def graph(target, path, dumpbin):
                return (['libc.so.6'] if path.name.startswith('libonnx') else ['libonnxruntime.so.1'], {})
            with patch.object(bridge, 'loader_dependencies', side_effect=graph):
                inventory, _ = bridge.discovery(args, {'os': 'linux', 'executables': ['ilium', 'ilium-server']}, candidate, {})
            self.assertEqual([item['name'] for item in inventory['files']], ['libonnxruntime.so.1'])
            self.assertEqual((candidate / 'libonnxruntime.so.1').read_bytes(), b'fixture native bytes')

    def source_archive(self, directory, duplicate=False, symlink=False):
        commit = '058787ceead760166e3c50a0a4cba8a833a6f53f'
        archive = directory / 'source.tar.gz'
        with tarfile.open(archive, 'w:gz') as stream:
            names = ['LICENSE', 'ThirdPartyNotices.txt', 'unrelated.txt'] + (['LICENSE'] if duplicate else [])
            for name in names:
                member = tarfile.TarInfo('onnxruntime-' + commit + '/' + name)
                data = ('synthetic fixture ' + name).encode(); member.size = len(data)
                if symlink and name == 'LICENSE':
                    member.type = tarfile.SYMTYPE; member.linkname = '/unrelated'; member.size = 0; data = b''
                stream.addfile(member, io.BytesIO(data))
        register = directory / 'register.json'
        register.write_text(json.dumps({'state': 'reviewed', 'version': '1.24.2', 'commit': commit, 'source_sha256': bridge.sha(archive), 'source_url': 'https://example.invalid/synthetic-source'}))
        return archive, register

    def test_extracts_only_two_validated_regular_ort_notices(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); archive, register = self.source_archive(root); output = root / 'out'; output.mkdir()
            item, evidence = bridge.extract_ort_notices(archive, register, output)
            self.assertEqual(set(evidence['members']), {'LICENSE', 'ThirdPartyNotices.txt'})
            self.assertEqual([path.name for path in output.iterdir()], ['ORT-LICENSE-AND-NOTICES.txt'])
            self.assertEqual(bridge.sha(Path(item['license_file'])), item['license_sha256'])

    def test_ort_duplicate_link_and_changed_source_are_blocked(self):
        for mode in ('duplicate', 'symlink', 'changed'):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary); archive, register = self.source_archive(root, mode == 'duplicate', mode == 'symlink')
                output = root / 'out'; output.mkdir()
                if mode == 'changed': archive.write_bytes(b'changed')
                with self.assertRaises(ValueError): bridge.extract_ort_notices(archive, register, output)

    def test_model_runtime_aliases_are_os_specific(self):
        self.assertEqual(bridge.canonical_ort_name('@rpath/libonnxruntime.1.dylib', 'macos'), 'libonnxruntime.1.24.2.dylib')
        self.assertEqual(bridge.canonical_ort_name('ONNXRUNTIME.DLL', 'windows'), 'onnxruntime.dll')
        self.assertIsNone(bridge.canonical_ort_name('unreviewed.dll', 'windows'))
        self.assertIsNone(bridge.canonical_ort_name('libonnxruntime.1.23.0.dylib', 'macos'))
        self.assertIsNone(bridge.canonical_ort_name('libonnxruntime.so.1.23.0', 'linux'))

    def test_zero_tests_and_failed_tests_cannot_prove_installation(self):
        for result in [subprocess.CompletedProcess([], 0, 'test result: ok. 0 passed; 0 failed;', ''), subprocess.CompletedProcess([], 1, 'test result: FAILED. 1 failed;', '')]:
            with self.assertRaises(ValueError): install.verify_pty_result(result, 'fixture')
        install.verify_pty_result(subprocess.CompletedProcess([], 0, 'test result: ok. 1 passed; 0 failed;', ''), 'fixture')

    def test_install_assets_require_exact_five_target_checksums_and_native_hash(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); manifest = ROOT / 'release/targets.toml'; targets = release_tool.load_targets(manifest)
            selected = targets[0]
            for target in targets: (root / target['archive']).write_bytes(('synthetic ' + target['archive']).encode())
            sums = ''.join(bridge.sha(root / target['archive']) + '  ' + target['archive'] + '\n' for target in targets)
            (root / 'SHA256SUMS').write_text(sums)
            args = SimpleNamespace(archive_directory=root, manifest=manifest)
            self.assertEqual(install.check_assets(args, selected)['archive_sha256'], bridge.sha(root / selected['archive']))
            (root / 'SHA256SUMS').write_text(sums.splitlines()[0] + '\n')
            with self.assertRaisesRegex(ValueError, 'five-target'): install.check_assets(args, selected)
            (root / 'SHA256SUMS').write_text(sums); (root / selected['archive']).write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError, 'differs'): install.check_assets(args, selected)

    def test_local_https_assets_use_only_requested_tag_and_members(self):
        if not __import__('shutil').which('openssl'):
            self.skipTest('local TLS prerequisite unavailable')
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); assets = root / 'assets'; assets.mkdir()
            (assets / 'ilium-linux-x86_64.tar.gz').write_bytes(b'synthetic archive fixture')
            (assets / 'SHA256SUMS').write_bytes(b'synthetic sums fixture')
            args = SimpleNamespace(archive_directory=assets, tag='v0.1.0')
            server, origin, certificate = install.serve_assets(args, {'archive': 'ilium-linux-x86_64.tar.gz'}, root)
            try:
                context = ssl.create_default_context(cafile=str(certificate))
                with urllib.request.urlopen(origin + '/download/v0.1.0/SHA256SUMS', context=context) as response:
                    self.assertEqual(response.read(), b'synthetic sums fixture')
                with self.assertRaises(urllib.error.HTTPError):
                    urllib.request.urlopen(origin + '/download/v0.2.0/SHA256SUMS', context=context)
                self.assertEqual(len(server.requests), 1)
            finally:
                server.shutdown(); server.server_close()

    def test_preview_url_is_exact_host_path_and_cannot_override_assets_tag(self):
        accepted = 'https://abc123.ilium-setup.pages.dev/install.sh'
        self.assertEqual(install.preview_command(accepted, 'linux'), install.PUBLIC_COMMANDS['posix'].replace('https://ilium-setup.pages.dev/install.sh', accepted))
        for url in ('http://abc123.ilium-setup.pages.dev/install.sh', 'https://abc123.ilium-setup.pages.dev:443/install.sh', 'https://user@abc123.ilium-setup.pages.dev/install.sh', 'https://abc123.ilium-setup.pages.dev/install.ps1', 'https://abc123.ilium-setup.pages.dev/install.sh?version=0.1.0', 'https://evil.example/install.sh', 'https://ilium-setup.pages.dev/install.sh'):
            with self.subTest(url=url), self.assertRaises(ValueError):
                install.preview_command(url, 'linux')

    def test_literal_windows_install_is_blocked_on_a_user_workstation(self):
        with patch.dict(os.environ, {'GITHUB_ACTIONS': 'false', 'RUNNER_ENVIRONMENT': ''}):
            with self.assertRaisesRegex(ValueError, 'disposable'):
                install.windows_disposable_identity({})

    def test_isolated_posix_environment_owns_default_install_and_bin_paths(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with patch.dict(os.environ, {'XDG_BIN_HOME': '/user-owned/bin'}):
                environment = install.isolated_environment(root)
            self.assertEqual(environment['XDG_BIN_HOME'], str(Path(environment['HOME']) / '.local/bin'))
            self.assertTrue(Path(environment['XDG_CONFIG_HOME']).is_relative_to(root))

    def test_windows_local_adapter_preserves_source_functions_and_requires_footer(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); output = root / 'wrapper.ps1'
            source = ROOT / 'release/install.ps1'
            install.windows_local_script(source, output)
            original = source.read_text().rstrip().rsplit('Invoke-IliumInstall -Version $Version', 1)[0]
            self.assertTrue(output.read_text().startswith(original))
            self.assertIn('-NoModifyPath', output.read_text())
            source = root / 'broken.ps1'; source.write_text('unknown entrypoint')
            with self.assertRaisesRegex(ValueError, 'entrypoint'): install.windows_local_script(source, root / 'other.ps1')


if __name__ == '__main__':
    unittest.main()
