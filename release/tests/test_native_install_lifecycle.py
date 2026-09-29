"""Portable contract tests; they use synthetic bytes, never native release claims."""
import hashlib
import json
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

from release.tests import native_install


class NativeInstallLifecycleTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def embedding_fixture(self):
        wrapper = self.root / 'embedding_acceptance.py'; wrapper.write_bytes(b'wrapper')
        command = self.root / 'embedding-command.json'
        command.write_text(json.dumps({'schema': 1, 'state': 'reviewed', 'protocol': 'held-installed-process-v1', 'sha256': native_install.sha(wrapper)}))
        model = self.root / 'model'; model.mkdir()
        hashes = {}
        for name in native_install.EMBEDDING_FILES:
            path = model / name; path.write_bytes(name.encode()); hashes[name] = native_install.sha(path)
        register = self.root / 'embedding-model.json'
        register.write_text(json.dumps({'schema': 1, 'reviewed': True, 'dimension': 384, 'files': hashes}))
        return SimpleNamespace(embedding_wrapper=wrapper, embedding_command=command,
                               embedding_model=model / 'model.onnx', embedding_model_register=register,
                               expected_embedding_wrapper_sha256=native_install.sha(wrapper),
                               expected_embedding_command_sha256=native_install.sha(command),
                               expected_embedding_model_register_sha256=native_install.sha(register),
                               expected_embedding_model_files=json.dumps(hashes, sort_keys=True, separators=(',', ':')),
                               expected_embedding_runtime_files='{}')

    def test_embedding_inputs_are_exactly_bound_and_plain(self):
        arguments = self.embedding_fixture()
        binding = native_install.validate_embedding_inputs(arguments)
        self.assertEqual(binding['model_files'], json.loads(arguments.expected_embedding_model_files))
        self.assertEqual(binding['wrapper_sha256'], arguments.expected_embedding_wrapper_sha256)
        arguments.embedding_model.write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'model'):
            native_install.validate_embedding_inputs(arguments)

    def test_embedding_proof_requires_installed_binary_and_real_vector(self):
        binding = {'wrapper_sha256': 'a' * 64, 'command_sha256': 'b' * 64,
                   'model_register_sha256': 'c' * 64, 'model_files': {'model.onnx': 'd' * 64}}
        proof = {'type': 'embedding-proof', 'binary_sha256': 'e' * 64,
                 'model_sha256': 'd' * 64, 'embedding': [0.25] * 384, 'loaded_runtime': ''}
        receipt = native_install.validate_installed_embedding(proof, binding, 'e' * 64, 'linux')
        self.assertTrue(receipt['finite_nonzero'])
        self.assertEqual(receipt['dimension'], 384)
        for changed in ({'embedding': [0.0] * 384}, {'embedding': [0.1] * 383}, {'binary_sha256': 'f' * 64}):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                native_install.validate_installed_embedding(dict(proof, **changed), binding, 'e' * 64, 'linux')

    def test_evidence_inventory_rehashes_every_retained_file(self):
        evidence = self.root / 'receipt.evidence'; (evidence / 'nested').mkdir(parents=True)
        (evidence / 'a.txt').write_text('a'); (evidence / 'nested/b.txt').write_text('b')
        values = native_install.evidence_inventory(evidence)
        self.assertEqual(set(values), {'a.txt', 'nested/b.txt'})
        self.assertEqual(values['a.txt'], hashlib.sha256(b'a').hexdigest())

    def test_prior_fixture_relabels_only_owned_version_and_pointer(self):
        install = self.root / 'install'
        version = install / 'versions/0.1.0'; version.mkdir(parents=True)
        (version / 'ilium').write_bytes(b'audited candidate bytes')
        state = install / 'installer-state'; state.mkdir()
        (state / 'version-0.1.0').write_text('hash  ilium\n')
        (install / 'current').write_text('0.1.0\n')
        prior = native_install.relabel_prior_fixture(install, {'os': 'linux'}, '0.1.0')
        self.assertEqual(prior, '0.0.0-task7b-prior')
        self.assertEqual((install / 'current').read_text(), prior + '\n')
        self.assertEqual((install / ('versions/' + prior) / 'ilium').read_bytes(), b'audited candidate bytes')
        self.assertTrue((state / ('version-' + prior)).is_file())

    def test_installed_pair_always_uses_canonical_version_bin_directory(self):
        install = self.root / 'install'
        for operating_system in ('linux', 'macos', 'windows'):
            with self.subTest(operating_system=operating_system):
                self.assertEqual(native_install.installed_pair(install, '0.1.0'),
                                 install / 'versions/0.1.0/bin')

    def test_windows_outer_cleanup_restores_path_even_when_uninstall_fails(self):
        environment = {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted'}
        events = []
        identities = iter(({'user_path': 'mutated'}, {'user_path': 'before'}))
        def cleanup():
            events.append('cleanup')
            raise native_install.ReleaseError('cleanup failed with exit 7')
        with patch.object(native_install, 'windows_disposable_identity', side_effect=lambda _env: next(identities)), \
             patch.object(native_install, 'restore_windows_path', side_effect=lambda _env, value: events.append(('restore', value))):
            with self.assertRaisesRegex(ValueError, 'cleanup failed with exit 7'):
                native_install.finalize_windows_cleanup(environment, 'before', cleanup)
        self.assertEqual(events, ['cleanup', ('restore', 'before')])

    def test_windows_outer_cleanup_preserves_null_and_nonnull_original_path(self):
        environment = {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted'}
        for original in (None, 'C:\\Authored'):
            restored = []
            identities = iter(({'user_path': 'mutated'}, {'user_path': original}))
            with self.subTest(original=original), \
                 patch.object(native_install, 'windows_disposable_identity', side_effect=lambda _env: next(identities)), \
                 patch.object(native_install, 'restore_windows_path', side_effect=lambda _env, value: restored.append(value)):
                native_install.finalize_windows_cleanup(environment, original, lambda: None)
            self.assertEqual(restored, [original])

    def test_windows_outer_cleanup_reports_failed_restore_readback(self):
        environment = {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted'}
        identities = iter(({'user_path': 'mutated-before'}, {'user_path': 'mutated-after'}))
        with patch.object(native_install, 'windows_disposable_identity', side_effect=lambda _env: next(identities)), \
             patch.object(native_install, 'restore_windows_path'):
            with self.assertRaisesRegex(ValueError, 'readback failed'):
                native_install.finalize_windows_cleanup(environment, 'authored-original', lambda: None)

    def test_windows_outer_cleanup_reports_both_uninstall_and_restore_failures(self):
        environment = {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted'}
        def cleanup():
            raise native_install.ReleaseError('cleanup exit 9')
        with patch.object(native_install, 'windows_disposable_identity', side_effect=ValueError('identity read failed')):
            with self.assertRaisesRegex(ValueError, 'cleanup exit 9.*identity read failed'):
                native_install.finalize_windows_cleanup(environment, None, cleanup)

    def test_windows_registry_permission_uses_supplied_hosted_environment(self):
        environment = {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted'}
        with patch.object(native_install.shutil, 'which', return_value='powershell.exe'), patch.object(native_install, 'run') as run:
            run.return_value.stdout = json.dumps({'local_app_data': 'C:\\Users\\runner\\AppData\\Local', 'user_profile': 'C:\\Users\\runner', 'user_path': 'before', 'user': 'runner'})
            self.assertEqual(native_install.windows_disposable_identity(environment)['user_path'], 'before')
        with self.assertRaisesRegex(ValueError, 'GitHub-hosted'):
            native_install.windows_disposable_identity({})


if __name__ == '__main__':
    unittest.main()
