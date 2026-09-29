"""Synthetic archives exercise extraction gates; they are not native proof."""
import copy
import contextlib
import io
import json
from pathlib import Path
import stat
import sys
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import ort_runtime as adapter


class OrtRuntimeTests(unittest.TestCase):
    def fixture(self, directory, target='aarch64-apple-darwin', mutation=None):
        policy = copy.deepcopy(json.loads((ROOT / 'release/ort-runtime.json').read_text()))
        source = json.loads((ROOT / 'release/ort-source.json').read_text())
        asset = policy['assets'][target]
        # These expressly synthetic assets use the approved shape but different
        # hashes. Native qualification always consumes the real frozen policy.
        items = {}
        for name, item in asset['selected'].items():
            output = item['output']
            data = b'synthetic fixture ' + output.encode()
            if output == 'VERSION_NUMBER': data = b'1.24.2\n'
            if output == 'GIT_COMMIT_ID': data = (policy['commit'] + '\n').encode()
            if output.startswith('lib/libonnxruntime'): data = b'synthetic identical native alias fixture'
            items[name] = ('file', data, '')
            for parent in Path(name).parents:
                if str(parent) != '.': items.setdefault(str(parent), ('directory', b'', ''))
        if mutation:
            mutation(items, asset)
        archive = directory / asset['name']
        if asset['format'] == 'tar.gz':
            with tarfile.open(archive, 'w:gz') as stream:
                for name, (kind, data, link) in items.items():
                    member = tarfile.TarInfo(name); member.mode = 0o644
                    member.type = {'file': tarfile.REGTYPE, 'directory': tarfile.DIRTYPE, 'link': tarfile.SYMTYPE, 'hardlink': tarfile.LNKTYPE, 'fifo': tarfile.FIFOTYPE}[kind]
                    member.size = len(data); member.linkname = link
                    stream.addfile(member, io.BytesIO(data))
        else:
            with zipfile.ZipFile(archive, 'w') as stream:
                for name, (kind, data, _) in items.items():
                    member = zipfile.ZipInfo(name + '/' if kind == 'directory' else name)
                    member.create_system = 3
                    member.external_attr = ({'file': stat.S_IFREG, 'directory': stat.S_IFDIR, 'link': stat.S_IFLNK, 'fifo': stat.S_IFIFO}[kind] | 0o644) << 16
                    stream.writestr(member, data)
        asset['members'] = {name: {'type': kind if kind in ('file', 'directory') else 'file', 'size': len(data)} for name, (kind, data, _) in items.items()}
        for name in asset['selected']:
            if name in items:
                asset['selected'][name]['sha256'] = adapter.digest(items[name][1])
        asset['bytes'], asset['sha256'] = archive.stat().st_size, adapter.sha(archive)
        asset['github_digest'] = 'sha256:' + asset['sha256']
        register = directory / 'register.json'; register.write_text(json.dumps(policy))
        source_path = directory / 'source.json'; source_path.write_text(json.dumps(source))
        args = SimpleNamespace(register=register, source_register=source_path, target=target, archive=archive, output_directory=directory / 'output')
        return args, policy

    def test_real_policy_records_official_digests_and_pinned_identity(self):
        register = json.loads((ROOT / 'release/ort-runtime.json').read_text())
        source = json.loads((ROOT / 'release/ort-source.json').read_text())
        for target in adapter.TARGETS:
            asset = adapter.validate_register(register, source, target)
            self.assertTrue(asset['url'].startswith('https://github.com/microsoft/onnxruntime/releases/download/v1.24.2/'))
            self.assertTrue(asset['selected'])
        self.assertEqual(register['assets']['aarch64-apple-darwin']['sha256'], '0af4fa503e8ea285245b47ee42d0a7461b8156a81270857da0c1d4ecf858abde')
        self.assertEqual(register['assets']['x86_64-pc-windows-msvc']['sha256'], '8e3e9c826375352e29cb2614fe44f3d7a4b0ff7b8028ad7a456af9d949a7e8b0')

    def test_selected_regular_assets_and_explicit_dynamic_environment(self):
        for target in adapter.TARGETS:
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temporary:
                args, _ = self.fixture(Path(temporary), target)
                with contextlib.redirect_stdout(io.StringIO()):
                    adapter.extract(args)
                receipt = json.loads((args.output_directory / 'ort-runtime-receipt.json').read_text())
                self.assertEqual(receipt['state'], 'extracted-not-qualified')
                self.assertFalse(receipt['publication_allowed'])
                self.assertEqual(receipt['environment']['ORT_PREFER_DYNAMIC_LINK'], '1')
                self.assertEqual(receipt['environment']['ORT_LIB_LOCATION'], str(args.output_directory / 'lib'))
                for name, sha in receipt['files'].items():
                    self.assertEqual(adapter.sha(args.output_directory / name), sha)
                    self.assertFalse((args.output_directory / name).is_symlink())

    def test_link_special_traversal_mixed_root_and_case_collisions_fail(self):
        def add(kind, name):
            return lambda items, asset: items.update({name: (kind, b'' if kind != 'file' else b'fixture', '../escape')})
        mutations = [add('link', 'onnxruntime-osx-arm64-1.24.2/lib/linked'), add('hardlink', 'onnxruntime-osx-arm64-1.24.2/lib/hardlinked'), add('fifo', 'onnxruntime-osx-arm64-1.24.2/special'), add('file', 'onnxruntime-osx-arm64-1.24.2/../escape'), add('file', '/absolute'), add('file', 'unrelated-root/file'), add('file', 'onnxruntime-osx-arm64-1.24.2/license')]
        for mutation in mutations:
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temporary:
                args, _ = self.fixture(Path(temporary), mutation=mutation)
                with self.assertRaises(ValueError): adapter.extract(args)
                self.assertFalse(args.output_directory.exists())

    def test_zip_symlinks_and_special_members_are_not_regularized(self):
        for kind in ('link', 'fifo'):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as temporary:
                args, _ = self.fixture(Path(temporary), 'x86_64-pc-windows-msvc', lambda items, asset: items.update({asset['root'] + '/lib/special': (kind, b'fixture', '')}))
                with self.assertRaises(ValueError): adapter.extract(args)
                self.assertFalse(args.output_directory.exists())

    def test_duplicate_archive_members_fail_even_with_matching_archive_hash(self):
        with tempfile.TemporaryDirectory() as temporary:
            args, policy = self.fixture(Path(temporary), 'x86_64-pc-windows-msvc')
            asset = policy['assets'][args.target]; name = next(iter(asset['selected']))
            with zipfile.ZipFile(args.archive, 'a') as stream:
                import warnings
                with warnings.catch_warnings():
                    warnings.simplefilter('ignore', UserWarning); stream.writestr(name, b'duplicate')
            asset['bytes'], asset['sha256'] = args.archive.stat().st_size, adapter.sha(args.archive)
            asset['github_digest'] = 'sha256:' + asset['sha256']
            args.register.write_text(json.dumps(policy))
            with self.assertRaisesRegex(ValueError, 'duplicate'): adapter.extract(args)
            self.assertFalse(args.output_directory.exists())

    def test_unknown_extra_inventory_and_file_parent_fail(self):
        with tempfile.TemporaryDirectory() as temporary:
            args, policy = self.fixture(Path(temporary)); asset = policy['assets'][args.target]
            entries = [(name, item['type'], item['size']) for name, item in asset['members'].items()]
            with self.assertRaisesRegex(ValueError, 'differs'):
                adapter.validate_inventory(entries + [(asset['root'] + '/extra', 'file', 1)], asset)
            bad = copy.deepcopy(asset); parent = asset['root'] + '/lib'; bad['members'][parent]['type'] = 'file'
            entries = [(name, item['type'], item['size']) for name, item in bad['members'].items()]
            with self.assertRaisesRegex(ValueError, 'parent'): adapter.validate_inventory(entries, bad)

    def test_asset_hash_and_selected_hash_tampering_block_before_writes(self):
        for mode in ('archive', 'member'):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                args, policy = self.fixture(Path(temporary))
                if mode == 'archive': args.archive.write_bytes(b'changed')
                else:
                    first = next(iter(policy['assets'][args.target]['selected']))
                    policy['assets'][args.target]['selected'][first]['sha256'] = '0' * 64
                    args.register.write_text(json.dumps(policy))
                with self.assertRaisesRegex(ValueError, 'bytes|hash'): adapter.extract(args)
                self.assertFalse(args.output_directory.exists())

    def test_existing_output_and_symlink_output_are_preserved(self):
        for mode in ('directory', 'symlink'):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                args, _ = self.fixture(Path(temporary))
                if mode == 'directory': args.output_directory.mkdir(); (args.output_directory / 'authored').write_bytes(b'keep')
                else: args.output_directory.symlink_to(args.output_directory.parent / 'missing', target_is_directory=True)
                with self.assertRaisesRegex(ValueError, 'new absolute'): adapter.extract(args)
                if mode == 'directory': self.assertEqual((args.output_directory / 'authored').read_bytes(), b'keep')
                else: self.assertTrue(args.output_directory.is_symlink())

    def test_source_version_commit_and_strategy_mismatch_fail(self):
        register = json.loads((ROOT / 'release/ort-runtime.json').read_text())
        source = json.loads((ROOT / 'release/ort-source.json').read_text())
        for target in ('x86_64-unknown-linux-gnu', 'x86_64-apple-darwin'):
            with self.assertRaisesRegex(ValueError, 'strategies'): adapter.validate_register(register, source, target)
        changed = dict(source, commit='0' * 40)
        with self.assertRaisesRegex(ValueError, 'source'): adapter.validate_register(register, changed, 'aarch64-apple-darwin')

    def test_embedded_commit_and_alias_disagreement_fail(self):
        for mode in ('commit', 'alias'):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                def mutate(items, asset):
                    name = asset['root'] + ('/GIT_COMMIT_ID' if mode == 'commit' else '/lib/libonnxruntime.dylib')
                    items[name] = ('file', b'wrong fixture bytes', '')
                args, _ = self.fixture(Path(temporary), mutation=mutate)
                with self.assertRaisesRegex(ValueError, 'commit|alias'): adapter.extract(args)
                self.assertFalse(args.output_directory.exists())


if __name__ == '__main__':
    unittest.main()
