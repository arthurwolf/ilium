"""Synthetic unit fixtures for direct tarball custody; never release acceptance."""
import argparse
from contextlib import ExitStack
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import qualify_linux_tarball as gate
import release_tool


class LinuxTarballTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='synthetic-linux-tarball-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.source, self.native = self.root / 'source', self.root / 'native'
        self.source.mkdir()
        self.native.mkdir()
        (self.source / 'release/tests').mkdir(parents=True)
        (self.source / 'release/targets.toml').write_bytes((ROOT / 'release/targets.toml').read_bytes())
        (self.source / 'Cargo.toml').write_text('[workspace.package]\nversion = "0.1.1-linux.1"\n')
        (self.source / 'Cargo.lock').write_bytes(b'synthetic locked source')
        (self.source / 'release/tests/embedding_acceptance.py').write_bytes(b'synthetic wrapper; never executed')
        self.target = release_tool.selected_target(self.source / 'release/targets.toml', gate.TARGET)
        self.unit = 'ilium-preview-direct-' + 'a' * 32 + '.service'
        self.arguments = argparse.Namespace(
            source=self.source, native_directory=self.native, output=self.root / 'output',
            commit='b' * 40, tag=gate.TAG, unit=self.unit,
            expected_bridge_sha256='', expected_harness_sha256='')
        self.content = {'ilium': b'synthetic client', 'ilium-server': b'synthetic server',
                        'ilium-animation-helper': b'synthetic helper',
                        'VERSION': b'0.1.1-linux.1\n', 'THIRD-PARTY.txt': b'reviewed synthetic licence text',
                        'libonnxruntime.so.1': b'synthetic runtime; never loaded'}
        for name in self.target['packages']:
            self.content[name] = (ROOT / 'ilium-animation-js/assets/packages' / name).read_bytes()
        files = {name: release_tool.digest(data) for name, data in self.content.items()}
        self.audit = {'schema': 1, 'state': 'passed', 'publication_allowed': True,
                      'target': gate.TARGET, 'tag': gate.TAG, 'version': gate.TAG[1:],
                      'os': 'linux', 'arch': 'x86_64', 'files': files,
                      'native_identity': {'system': 'Linux', 'machine': 'x86_64', 'runner': self.target['runner']},
                      'dependency_closure': {'complete': True},
                      'binary_versions': {name: name + ' ' + gate.TAG[1:] for name in self.target['executables']},
                      'notices': {'state': 'reviewed', 'sha256': files['THIRD-PARTY.txt']}}
        self.write_json(self.native / 'native-audit.json', self.audit)
        release_tool.write_archive(self.native / self.target['archive'], self.target, self.content)
        self.runtime_files = {'libonnxruntime.so.1': files['libonnxruntime.so.1']}
        self.runtime = {'schema': 1, 'state': 'reviewed', 'publication_allowed': True,
                        'target': gate.TARGET, 'files': [{'name': name, 'sha256': digest, 'reviewed': True}
                                                       for name, digest in self.runtime_files.items()]}
        self.write_json(self.native / 'runtime-inventory.json', self.runtime)
        self.write_json(self.native / 'dependency-inventory.json', {'synthetic': True})
        model_directory = self.native / 'evidence/model'
        model_directory.mkdir(parents=True)
        for name in gate.install_gate.EMBEDDING_FILES:
            (model_directory / name).write_bytes(('synthetic model ' + name).encode())
        self.model_files = gate.install_gate.tree_inventory(model_directory)
        self.write_json(self.source / 'release/embedding-model.json', {
            'schema': 1, 'reviewed': True, 'dimension': 384, 'files': self.model_files})
        self.write_json(self.native / 'embedding-command.json', {
            'schema': 1, 'state': 'reviewed', 'protocol': 'held-installed-process-v1',
            'sha256': gate.sha(self.source / 'release/tests/embedding_acceptance.py'),
            'command': ['synthetic wrapper; never executed']})
        self.write_json(self.native / 'embedding-receipt.json', {
            'type': 'embedding-proof', 'binary_sha256': files['ilium'],
            'model_sha256': self.model_files['model.onnx']})
        harness_directory = self.native / 'evidence/harness'
        harness_directory.mkdir()
        for name, data in {'native-test-binary': b'synthetic PTY harness; never executed',
                           **{name: self.content[name] for name in self.runtime_files}}.items():
            (harness_directory / name).write_bytes(data)
        (self.native / 'native-test-binary').write_bytes((harness_directory / 'native-test-binary').read_bytes())
        self.harness = {'schema': 1, 'target': gate.TARGET, 'tag': gate.TAG, 'version': gate.TAG[1:],
                        'filename': 'native-test-binary', 'path': 'evidence/harness/native-test-binary',
                        'sha256': gate.sha(self.native / 'native-test-binary'), 'runtime_files': self.runtime_files,
                        'source_commit': self.arguments.commit, 'evidence_files': self.evidence()}
        self.bridge = {'schema': 1, 'state': 'passed', 'publication_allowed': True,
                       'qualification_profile': gate.PROFILE, 'target': gate.TARGET, 'tag': gate.TAG,
                       'native_identity': self.audit['native_identity'], 'files': files,
                       'workspace_sha256': gate.sha(self.source / 'Cargo.toml'),
                       'lock_sha256': gate.sha(self.source / 'Cargo.lock'),
                       'embedding_model_register_sha256': gate.sha(self.source / 'release/embedding-model.json'),
                       'official_packages': {name: release_tool.APPROVED_PACKAGES[name] for name in self.target['packages']},
                       'build_outputs': {name: {'sha256': files[name]} for name in self.target['executables']}}
        for key, name in {'archive': self.target['archive'], 'native_audit': 'native-audit.json',
                          'runtime_inventory': 'runtime-inventory.json', 'dependency_inventory': 'dependency-inventory.json',
                          'embedding_receipt': 'embedding-receipt.json'}.items():
            self.bridge[key] = {'path': '/synthetic-original-build/' + name, 'sha256': gate.sha(self.native / name)}
        self.checksum_record = (gate.sha(self.native / self.target['archive']) + '  '
                                + self.target['archive'] + '\n').encode('ascii')
        (self.native / 'SHA256SUMS').write_bytes(self.checksum_record)
        self.freeze_receipts()
        self.calls = []
        self.mode = 'pass'

    def write_json(self, path, value):
        path.write_text(json.dumps(value, sort_keys=True) + '\n', encoding='utf-8')

    def evidence(self):
        return {'evidence/' + name: digest for name, digest in gate.install_gate.evidence_inventory(self.native / 'evidence').items()}

    def freeze_receipts(self):
        self.write_json(self.native / 'native-candidate-receipt.json', self.bridge)
        self.write_json(self.native / 'native-test-harness.json', self.harness)
        self.arguments.expected_bridge_sha256 = gate.sha(self.native / 'native-candidate-receipt.json')
        self.arguments.expected_harness_sha256 = gate.sha(self.native / 'native-test-harness.json')

    def validate(self):
        return gate.validate_inputs(self.arguments, self.source, self.native)

    def test_complete_input_fixture_binds_every_retained_member(self):
        inputs = self.validate()
        self.assertEqual(inputs.content, self.content)
        self.assertEqual(inputs.runtime_files, self.runtime_files)
        self.assertEqual(inputs.embedding_binding['model_files'], self.model_files)

    def test_bridge_identity_and_source_mismatches_reject(self):
        changes = {'schema': 2, 'state': 'failed', 'publication_allowed': False,
                   'qualification_profile': 'full', 'target': 'aarch64-unknown-linux-gnu',
                   'tag': 'v0.1.1', 'workspace_sha256': '0' * 64, 'lock_sha256': '0' * 64,
                   'embedding_model_register_sha256': '0' * 64,
                   'native_identity': {'system': 'Darwin'}, 'files': {}, 'official_packages': {}, 'build_outputs': {}}
        original = copy.deepcopy(self.bridge)
        for key, value in changes.items():
            with self.subTest(field=key):
                self.bridge = {**copy.deepcopy(original), key: value}
                self.freeze_receipts()
                with self.assertRaises(release_tool.ReleaseError):
                    self.validate()

    def test_harness_identity_runtime_and_evidence_mismatches_reject(self):
        changes = {'schema': 2, 'target': 'aarch64-unknown-linux-gnu', 'tag': 'v0.1.1',
                   'version': '0.1.0', 'source_commit': 'c' * 40, 'filename': 'foreign',
                   'path': '/foreign/native-test-binary', 'sha256': '0' * 64,
                   'runtime_files': {}, 'evidence_files': {}, 'unexpected': True}
        original = copy.deepcopy(self.harness)
        for key, value in changes.items():
            with self.subTest(field=key):
                self.harness = {**copy.deepcopy(original), key: value}
                self.freeze_receipts()
                with self.assertRaises(release_tool.ReleaseError):
                    self.validate()

    def test_download_receipt_hashes_are_required_before_trusting_receipts(self):
        for name in ('expected_bridge_sha256', 'expected_harness_sha256'):
            original = getattr(self.arguments, name)
            with self.subTest(receipt=name):
                setattr(self.arguments, name, '0' * 64)
                with self.assertRaisesRegex(release_tool.ReleaseError, 'download binding'):
                    self.validate()
            setattr(self.arguments, name, original)

    def test_payload_mutation_cannot_be_adopted_by_rebinding_archive_hash(self):
        changed = {**self.content, 'ilium': b'changed synthetic client'}
        release_tool.write_archive(self.native / self.target['archive'], self.target, changed)
        self.bridge['archive']['sha256'] = gate.sha(self.native / self.target['archive'])
        self.freeze_receipts()
        with self.assertRaisesRegex(release_tool.ReleaseError, 'member hash differs'):
            self.validate()

    def test_native_checksum_record_binds_digest_and_filename(self):
        for field, replacement in (('digest', b'0' * 64), ('digest', b'g' * 64),
                                   ('digest', b'0' * 63), ('filename', b'foreign.tar.gz'),
                                   ('filename', b'../' + self.target['archive'].encode('ascii'))):
            with self.subTest(field=field, replacement=replacement):
                digest, filename = self.checksum_record.rstrip(b'\n').split(b'  ')
                record = (replacement if field == 'digest' else digest) + b'  '
                record += (replacement if field == 'filename' else filename) + b'\n'
                (self.native / 'SHA256SUMS').write_bytes(record)
                with self.assertRaisesRegex(release_tool.ReleaseError, 'Native checksum inventory differs'):
                    self.validate()

    def test_native_checksum_record_rejects_noncanonical_or_extra_lines(self):
        records = (b'', self.checksum_record.rstrip(b'\n'),
                   self.checksum_record.replace(b'\n', b'\r\n'),
                   self.checksum_record.replace(b'  ', b' ', 1),
                   self.checksum_record.replace(b'  ', b' *', 1),
                   self.checksum_record + self.checksum_record,
                   self.checksum_record + b'\n')
        for record in records:
            with self.subTest(record=record):
                (self.native / 'SHA256SUMS').write_bytes(record)
                with self.assertRaisesRegex(release_tool.ReleaseError, 'Native checksum inventory differs'):
                    self.validate()

    def test_native_checksum_record_must_be_a_plain_file(self):
        path = self.native / 'SHA256SUMS'
        path.unlink()
        with self.assertRaises(release_tool.ReleaseError):
            self.validate()
        path.symlink_to(self.native / self.target['archive'])
        with self.assertRaises(release_tool.ReleaseError):
            self.validate()

    def test_extra_missing_changed_or_linked_retained_evidence_rejects(self):
        path = self.native / 'evidence/model/config.json'
        original = path.read_bytes()
        for mode in ('extra', 'missing', 'changed', 'link'):
            with self.subTest(mode=mode):
                extra = self.native / 'evidence/undeclared.txt'
                if mode == 'extra':
                    extra.write_bytes(b'undeclared')
                elif mode == 'missing':
                    path.unlink()
                elif mode == 'changed':
                    path.write_bytes(b'changed')
                else:
                    path.unlink()
                    path.symlink_to(self.native / 'native-test-binary')
                with self.assertRaises(release_tool.ReleaseError):
                    self.validate()
                extra.unlink(missing_ok=True)
                path.unlink(missing_ok=True)
                path.write_bytes(original)

    def test_runtime_inventory_cannot_omit_audited_library(self):
        self.runtime['files'] = []
        self.write_json(self.native / 'runtime-inventory.json', self.runtime)
        self.bridge['runtime_inventory']['sha256'] = gate.sha(self.native / 'runtime-inventory.json')
        self.freeze_receipts()
        with self.assertRaisesRegex(release_tool.ReleaseError, 'runtime identity'):
            self.validate()

    def test_exact_modes_and_members_are_required_after_extraction(self):
        inputs = self.validate()
        pair = self.root / 'pair'
        pair.mkdir()
        for name, data in self.content.items():
            (pair / name).write_bytes(data)
            (pair / name).chmod(0o755 if name in self.target['executables'] else 0o644)
        gate.verify_installed(pair, inputs)
        (pair / 'ilium').chmod(0o777)
        with self.assertRaisesRegex(release_tool.ReleaseError, 'mode'):
            gate.verify_installed(pair, inputs)
        (pair / 'ilium').chmod(0o755)
        (pair / 'unexpected-empty-directory').mkdir()
        with self.assertRaisesRegex(release_tool.ReleaseError, 'inventory'):
            gate.verify_installed(pair, inputs)

    def native_adapter(self, command, **options):
        """Return labelled synthetic child outcomes; execute no native binary."""
        self.calls.append((command, options))
        executable = Path(command[0])
        output = ''
        if command[1:] == ['--version']:
            output = (release_tool.helper_version_record(gate.TAG[1:]) + '\n'
                      if executable.name == 'ilium-animation-helper'
                      else executable.name + ' ' + gate.TAG[1:] + '\n')
            code = 7 if self.mode == 'version-fails' else 0
        elif command[1:] == ['release-animation-probe']:
            if self.mode == 'animation-fails':
                return subprocess.CompletedProcess(command, 7, '', 'synthetic animation failure')
            catalogue = {'type': 'artifact', 'gate': 'installed_catalogue',
                         'client_path': str(executable), 'helper_path': str(executable.parent / 'ilium-animation-helper'),
                         'client_sha256': self.audit['files']['ilium'],
                         'helper_sha256': self.audit['files']['ilium-animation-helper'], 'packages': ['beach', 'carpet']}
            renders = [{'type': 'artifact', 'gate': 'installed_render', 'package': filename.split('-')[0],
                        'archive_sha256': digest, 'helper_sha256': self.audit['files']['ilium-animation-helper'],
                        'rendered_frames': 2, 'physical_retirement': True,
                        'worker_threads_before': 1, 'worker_threads_after': 1,
                        'worker_bytes_before': 1, 'worker_bytes_after': 1}
                       for filename, digest in release_tool.APPROVED_PACKAGES.items()]
            output = ''.join(json.dumps(row) + '\n' for row in [catalogue, *renders, {
                'type': 'result', 'gate': 'installed_animation', 'state': 'passed',
                'publication_allowed': False, 'packages': ['beach', 'carpet']}])
            code = 0
        elif '--installed-directory' in command:
            pair = Path(command[command.index('--installed-directory') + 1])
            proof = {'type': 'embedding-proof', 'binary_sha256': self.audit['files']['ilium'],
                     'model_sha256': self.model_files['model.onnx'], 'embedding': [0.5] * 384,
                     'loaded_runtime': str(pair / 'libonnxruntime.so.1'),
                     'executable_path': str(pair / 'ilium'), 'ilium_pid': 321}
            if self.mode == 'embedding-invalid':
                proof['embedding'][0] = float('nan')
            if self.mode == 'runtime-invalid':
                proof['loaded_runtime'] = str(self.native / 'evidence/harness/libonnxruntime.so.1')
            output, code = json.dumps(proof) + '\n', 0
        elif executable.name == 'native-test-binary' and command[1] in gate.PTY_TESTS:
            self.assertEqual(command[2:], ['--exact', '--nocapture', '--test-threads=1'])
            output = 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n'
            code = 0
            if self.mode == 'pty-zero':
                output = 'test result: ok. 0 passed; 0 failed;\n'
            if command[1] == gate.PTY_TESTS[-1]:
                if self.mode == 'pty-second-fails':
                    code = 7
                if self.mode == 'installed-changes':
                    pair = Path(options['env']['ILIUM_PTY_SMOKE_BINARY']).parent
                    (pair / 'libonnxruntime.so.1').write_bytes(b'changed installed runtime')
                if self.mode == 'native-changes':
                    (self.native / 'evidence/model/config.json').write_bytes(b'changed retained model')
        else:
            self.fail('Unexpected synthetic native invocation: ' + repr(command))
        return subprocess.CompletedProcess(command, code, output, '')

    def acceptance_patches(self):
        stack = ExitStack()
        stack.enter_context(patch.object(gate, 'baseline_identity', return_value={'synthetic_fixture_only': True}))
        stack.enter_context(patch.object(gate, 'source_snapshot', return_value={'synthetic_source_only': 'a' * 64}))
        stack.enter_context(patch.object(gate, 'launcher_snapshot', return_value={'synthetic_launcher_only': None}))
        stack.enter_context(patch.object(gate.subprocess, 'run', side_effect=self.native_adapter))
        stack.enter_context(patch.object(gate.animation_gate.platform, 'system', return_value='Linux'))
        stack.enter_context(patch.object(gate.animation_gate.platform, 'machine', return_value='x86_64'))
        stack.enter_context(patch.object(gate.release_tool, 'emit'))
        # The production animation validator still hashes each required source.
        for name in gate.animation_gate.SOURCE_FILES:
            path = self.source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            if not path.exists():
                path.write_bytes(('synthetic animation source ' + name).encode())
        return stack

    def test_synthetic_transaction_uses_all_official_gates_and_removes_prefix(self):
        with self.acceptance_patches():
            receipt = gate.qualify(self.arguments)
        self.assertEqual(receipt['state'], 'passed', receipt['errors'])
        self.assertFalse(receipt['publication_allowed'])
        self.assertTrue(receipt['filesystem_state_cleaned'])
        self.assertFalse(receipt['process_cleanup_verified'])
        self.assertTrue(receipt['outer_process_cleanup_required'])
        self.assertEqual([row['name'] for row in receipt['pty_tests']], list(gate.PTY_TESTS))
        self.assertFalse(list(self.arguments.output.glob('state-*')))
        self.assertEqual(json.loads((self.arguments.output / 'direct-tarball.json').read_text()), receipt)
        for _, options in self.calls:
            self.assertNotIn('LD_PRELOAD', options['env'])
            self.assertNotIn('PYTHONPATH', options['env'])
            self.assertEqual(options['env']['PATH'], os.defpath)
        self.assertEqual(gate.install_gate.evidence_inventory(self.arguments.output), {
            **receipt['evidence_files'], 'direct-tarball.json': gate.sha(self.arguments.output / 'direct-tarball.json')})

    def test_native_failures_tampering_and_zero_selected_tests_cleanup_without_acceptance(self):
        for mode in ('version-fails', 'animation-fails', 'embedding-invalid', 'runtime-invalid',
                     'pty-zero', 'pty-second-fails', 'installed-changes', 'native-changes'):
            with self.subTest(mode=mode):
                self.mode = mode
                self.arguments.output = self.root / ('output-' + mode)
                with self.acceptance_patches():
                    receipt = gate.qualify(self.arguments)
                self.assertEqual(receipt['state'], 'failed')
                self.assertTrue(receipt['errors'])
                self.assertTrue(receipt['filesystem_state_cleaned'])
                self.assertFalse(list(self.arguments.output.glob('state-*')))
                self.assertTrue((self.arguments.output / 'direct-tarball.json').is_file())

    def test_failed_extraction_still_cleans_and_publishes_nonpassing_evidence(self):
        with self.acceptance_patches(), patch.object(gate, 'verify_installed', side_effect=release_tool.ReleaseError('synthetic extraction failure')):
            receipt = gate.qualify(self.arguments)
        self.assertEqual(receipt['state'], 'failed')
        self.assertTrue(receipt['filesystem_state_cleaned'])
        self.assertIn('input-bindings.json', receipt['evidence_files'])

    def test_post_execution_source_and_launcher_changes_fail_the_receipt(self):
        for seam in ('source_snapshot', 'launcher_snapshot'):
            self.arguments.output = self.root / ('output-' + seam)
            with self.subTest(seam=seam), self.acceptance_patches(), patch.object(gate, seam, side_effect=[{'original': True}, {'changed': True}]):
                receipt = gate.qualify(self.arguments)
            self.assertEqual(receipt['state'], 'failed')
            self.assertTrue(receipt['filesystem_state_cleaned'])

    def test_cleanup_failure_is_not_recorded_as_retirement(self):
        with self.acceptance_patches(), patch.object(gate.shutil, 'rmtree', side_effect=PermissionError('synthetic cleanup denied')):
            receipt = gate.qualify(self.arguments)
        self.assertEqual(receipt['state'], 'failed')
        self.assertFalse(receipt['filesystem_state_cleaned'])
        self.assertFalse(receipt['process_cleanup_verified'])
        self.assertTrue(list(self.arguments.output.glob('state-*')))

    def test_existing_output_and_input_overlap_preserve_foreign_state(self):
        self.arguments.output.mkdir()
        sentinel = self.arguments.output / 'foreign-sentinel'
        sentinel.write_bytes(b'foreign state')
        with self.assertRaisesRegex(release_tool.ReleaseError, 'new owned path'):
            gate.qualify(self.arguments)
        self.assertEqual(sentinel.read_bytes(), b'foreign state')
        self.arguments.output = self.native / 'new-output'
        with self.assertRaisesRegex(release_tool.ReleaseError, 'overlap'):
            gate.qualify(self.arguments)


class LinuxTarballSourceTests(unittest.TestCase):
    def test_clean_git_status_cannot_hide_changed_execution_bytes(self):
        name = 'release/scripts/qualify_linux_tarball.py'
        commit = 'a' * 40
        def git(command, **options):
            arguments = command[1:]
            if arguments == ['rev-parse', '--show-toplevel']:
                return (str(ROOT) + '\n').encode()
            if arguments == ['rev-parse', 'HEAD']:
                return (commit + '\n').encode()
            if arguments == ['status', '--porcelain', '--untracked-files=no']:
                return b''
            if arguments == ['show', commit + ':' + name]:
                return b'synthetic different committed bytes'
            self.fail('Unexpected Git invocation')
        with patch.object(gate, 'SOURCE_FILES', (name,)), patch.object(gate.subprocess, 'check_output', side_effect=git):
            with self.assertRaisesRegex(release_tool.ReleaseError, 'Frozen execution source differs'):
                gate.source_snapshot(ROOT, commit)


class LinuxTarballBaselineTests(unittest.TestCase):
    def test_exact_delegated_leaf_is_required_without_mutating_kernel_state(self):
        unit = 'ilium-preview-direct-' + 'a' * 32 + '.service'
        relative = '/system.slice/' + unit
        parent = '/sys/fs/cgroup' + relative
        records = {
            '/proc/self/mountinfo': '30 20 0:28 / /sys/fs/cgroup rw,nosuid - cgroup2 cgroup rw\n',
            '/proc/self/cgroup': '0::' + relative + '/leaf\n',
            parent + '/cgroup.subtree_control': 'cpu memory pids\n',
            parent + '/cgroup.procs': '', parent + '/leaf/cgroup.controllers': 'cpu memory pids\n',
            parent + '/leaf/cgroup.procs': '123\n',
        }
        class KernelPath:
            def __init__(self, value):
                self.value = str(value)
            def __truediv__(self, value):
                return KernelPath(self.value + '/' + value)
            def __str__(self):
                return self.value
            def read_text(self):
                return records[self.value]
            def stat(self):
                return SimpleNamespace(st_uid=1000)
            def is_file(self):
                return self.value == '/usr/bin/bwrap'
        properties = {'LoadState': 'loaded', 'ActiveState': 'active', 'ControlGroup': relative,
                      'Delegate': 'yes', 'Transient': 'yes', 'KillMode': 'control-group'}
        outcome = subprocess.CompletedProcess([], 0, ''.join(key + '=' + value + '\n' for key, value in properties.items()), '')
        mutations = (None, ('/proc/self/cgroup', '0::/foreign/leaf\n'),
                     (parent + '/cgroup.subtree_control', 'cpu memory\n'),
                     (parent + '/leaf/cgroup.controllers', 'cpu pids\n'),
                     (parent + '/cgroup.procs', '999\n'),
                     ('/proc/self/mountinfo', '30 20 0:28 /foreign /sys/fs/cgroup rw - cgroup2 cgroup rw\n'))
        for mutation in mutations:
            original = records.copy()
            if mutation is not None:
                records[mutation[0]] = mutation[1]
            with self.subTest(mutation=mutation), patch.object(gate, 'Path', KernelPath), \
                 patch.object(gate.sys, 'version_info', (3, 11)), \
                 patch.object(gate.platform, 'python_version', return_value='3.11.0'), \
                 patch.object(gate.platform, 'system', return_value='Linux'), \
                 patch.object(gate.platform, 'machine', return_value='x86_64'), \
                 patch.object(gate.platform, 'freedesktop_os_release', return_value={'ID': 'ubuntu', 'VERSION_ID': '22.04'}), \
                 patch.object(gate.os, 'geteuid', return_value=1000), patch.object(gate.os, 'getpid', return_value=123), \
                 patch.object(gate.os, 'access', return_value=True), \
                 patch.object(gate.subprocess, 'run', return_value=outcome) as manager_query:
                if mutation is None:
                    identity = gate.baseline_identity(unit)
                    self.assertEqual(identity['cgroup'], parent)
                    self.assertEqual(manager_query.call_args.args[0][:3], ['/usr/bin/systemctl', 'show', unit])
                else:
                    with self.assertRaises(release_tool.ReleaseError):
                        gate.baseline_identity(unit)
                    manager_query.assert_not_called()
            records.clear()
            records.update(original)

    def test_baseline_rejects_wrong_python_host_or_unowned_unit_before_execution(self):
        unit = 'ilium-preview-direct-' + 'a' * 32 + '.service'
        for version, system, machine, release, candidate in (
            ((3, 10), 'Linux', 'x86_64', {'ID': 'ubuntu', 'VERSION_ID': '22.04'}, unit),
            ((3, 11), 'Linux', 'aarch64', {'ID': 'ubuntu', 'VERSION_ID': '22.04'}, unit),
            ((3, 11), 'Linux', 'x86_64', {'ID': 'ubuntu', 'VERSION_ID': '24.04'}, unit),
            ((3, 11), 'Linux', 'x86_64', {'ID': 'ubuntu', 'VERSION_ID': '22.04'}, 'foreign.service'),
        ):
            with self.subTest(version=version, machine=machine, release=release, unit=candidate), \
                 patch.object(gate.sys, 'version_info', version), \
                 patch.object(gate.platform, 'system', return_value=system), \
                 patch.object(gate.platform, 'machine', return_value=machine), \
                 patch.object(gate.platform, 'freedesktop_os_release', return_value=release), \
                 patch.object(gate.os, 'geteuid', return_value=1000), \
                 patch.object(gate.subprocess, 'run') as execute:
                with self.assertRaises(release_tool.ReleaseError):
                    gate.baseline_identity(candidate)
                execute.assert_not_called()


if __name__ == '__main__':
    unittest.main()
