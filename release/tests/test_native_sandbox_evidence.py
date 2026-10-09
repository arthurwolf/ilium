"""Synthetic receipt fixtures exercise release admission, not native execution."""
import copy
import hashlib
import json
import tempfile
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import native_sandbox_runner as runner
import release_tool
import validate_animation_smoke as smoke


class NativeSandboxEvidenceTests(unittest.TestCase):
    def fixture(self):
        target = 'x86_64-unknown-linux-gnu'
        artifact = {'schema': 1, 'state': 'compiled-not-qualified', 'executed': False,
                    'filename': 'native-sandbox-test-binary', 'target': target,
                    'tag': 'v0.1.1', 'sha256': 'a' * 64,
                    'helper_sha256': 'b' * 64, 'source_sha256': 'c' * 64}
        receipt = {'schema': 1, 'state': 'passed', 'scope': 'native-linux-sandbox-tests',
                   'publication_allowed': False, 'inputs_stable': True,
                   'uid': 1000, 'system': 'Linux', 'machine': 'x86_64',
                   'target': target, 'tag': 'v0.1.1', 'artifact_receipt_sha256': 'd' * 64,
                   'binary_sha256': artifact['sha256'],
                   'helper_sha256': artifact['helper_sha256'],
                   'source_sha256': artifact['source_sha256'], 'cases': {}}
        for index, case in enumerate(runner.CASES):
            stdout = ('running 1 test\ntest ' + case + ' ... ok\n'
                      'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.1s\n')
            unit = 'ilium-native-test-synthetic-' + str(index)
            admission = {'path': '/user.slice/' + unit + '.service', 'device': 1, 'inode': index + 1}
            stdout = runner.CGROUP_REPORT_PREFIX + json.dumps(admission) + '\n' + stdout
            command = runner.test_command(Path('/native/evidence/sandbox/native-sandbox-test-binary'), case, unit)
            command[1:1] = ['--setenv=ILIUM_NATIVE_HELPER_TEST_BINARY=/native/helper',
                            '--setenv=ILIUM_NATIVE_HELPER_TEST_SHA256=' + artifact['helper_sha256']]
            receipt['cases'][case] = {'state': 'passed', 'exit_code': 0,
                                      'unit': unit + '.service', 'delegate': False,
                                      'unit_retired': True, 'unit_fresh': True,
                                      'kernel_retirement': dict(admission, absent=True, manager_control_group=''), 'cleanup_error': None, 'stdout': stdout, 'stderr': '',
                                      'log_sha256': hashlib.sha256(stdout.encode()).hexdigest(),
                                      'command': command}
        return receipt, artifact, target

    def test_retained_artifact_bytes_are_required_for_seal_readback(self):
        _, artifact, _ = self.fixture()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'native-sandbox-artifact.json'
            path.write_text(json.dumps(artifact) + '\n')
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            smoke.validate_retained_sandbox_artifact(path, artifact, digest)
            foreign = copy.deepcopy(artifact)
            foreign['sha256'] = 'f' * 64
            with self.assertRaises(release_tool.ReleaseError):
                smoke.validate_retained_sandbox_artifact(path, foreign, digest)
            with self.assertRaises(release_tool.ReleaseError):
                smoke.validate_retained_sandbox_artifact(path, artifact, 'f' * 64)
            alias = Path(directory) / 'alias.json'
            alias.symlink_to(path)
            with self.assertRaises(release_tool.ReleaseError):
                smoke.validate_retained_sandbox_artifact(alias, artifact, digest)

    def test_manager_only_receipt_without_kernel_admission_is_rejected(self):
        receipt, artifact, target = self.fixture()
        row = receipt['cases'][runner.CASES[0]]
        row['stdout'] = '\n'.join(line for line in row['stdout'].splitlines()
                                   if not line.startswith(runner.CGROUP_REPORT_PREFIX)) + '\n'
        row['log_sha256'] = hashlib.sha256((row['stdout'] + row['stderr']).encode()).hexdigest()
        with self.assertRaises(release_tool.ReleaseError):
            smoke.validate_native_sandbox_receipt(receipt, artifact, 'd' * 64, target, 'v0.1.1')

    def test_four_bound_executed_cases_are_admitted(self):
        receipt, artifact, target = self.fixture()
        smoke.validate_native_sandbox_receipt(receipt, artifact, 'd' * 64, target, 'v0.1.1')

    def test_changed_commands_duplicate_units_and_foreign_artifacts_are_rejected(self):
        receipt, artifact, target = self.fixture()
        for key, value in [('target', 'aarch64-unknown-linux-gnu'), ('tag', 'v0.1.0'),
                           ('sha256', 'e' * 64), ('sha256', 'A' * 64)]:
            foreign = copy.deepcopy(artifact)
            foreign[key] = value
            with self.subTest(artifact_field=key, value=value), self.assertRaises(release_tool.ReleaseError):
                smoke.validate_native_sandbox_receipt(receipt, foreign, 'd' * 64, target, 'v0.1.1')
        for mutation in ['delegate', 'helper', 'case', 'duplicate unit']:
            changed = copy.deepcopy(receipt)
            row = changed['cases'][runner.CASES[0]]
            command = row['command']
            if mutation == 'delegate':
                command[command.index('--property=Delegate=no')] = '--property=Delegate=yes'
            elif mutation == 'helper':
                command[1] = '--setenv=ILIUM_NATIVE_HELPER_TEST_BINARY=relative/helper'
            elif mutation == 'case':
                command[-5] = runner.CASES[1]
            else:
                changed['cases'][runner.CASES[1]]['unit'] = row['unit']
            with self.subTest(mutation=mutation), self.assertRaises(release_tool.ReleaseError):
                smoke.validate_native_sandbox_receipt(changed, artifact, 'd' * 64, target, 'v0.1.1')

    def test_incomplete_stale_skipped_or_unretired_receipts_are_rejected(self):
        receipt, artifact, target = self.fixture()
        variants = []
        for key, value in [('state', 'failed'), ('inputs_stable', False), ('uid', 0),
                           ('target', 'aarch64-unknown-linux-gnu'), ('tag', 'v0.1.0'),
                           ('binary_sha256', 'e' * 64), ('helper_sha256', 'e' * 64),
                           ('source_sha256', 'e' * 64), ('artifact_receipt_sha256', 'e' * 64)]:
            value_receipt = copy.deepcopy(receipt)
            value_receipt[key] = value
            variants.append((key, value_receipt))
        missing = copy.deepcopy(receipt)
        missing['cases'].pop(runner.CASES[0])
        variants.append(('missing case', missing))
        for key, value in [('kernel_retirement', None), ('kernel_retirement', {'absent': True}),
                           ('exit_code', 1), ('unit_retired', False), ('delegate', True), ('unit_fresh', False), ('cleanup_error', 'stop failed'),
                           ('stdout', 'test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured;\n'),
                           ('log_sha256', 'e' * 64)]:
            value_receipt = copy.deepcopy(receipt)
            value_receipt['cases'][runner.CASES[0]][key] = value
            variants.append((key, value_receipt))
        for name, value_receipt in variants:
            with self.subTest(name=name), self.assertRaises(release_tool.ReleaseError):
                smoke.validate_native_sandbox_receipt(value_receipt, artifact, 'd' * 64, target, 'v0.1.1')


if __name__ == '__main__':
    unittest.main()
