"""Native qualification must execute exact sandbox cases, never accept skips."""
from pathlib import Path
from contextlib import redirect_stdout
import hashlib
import io
import json
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import native_sandbox_runner as runner


class NativeSandboxRunnerTests(unittest.TestCase):
    def test_required_runner_cases_exist_in_the_native_rust_test_source(self):
        import re
        source = (ROOT / 'ilium-platform/tests/native_animation_sandbox.rs').read_text()
        names = set(re.findall(r'^fn ([a-z_][a-z0-9_]*)\(', source, re.MULTILINE))
        self.assertEqual(set(runner.CASES) - names, set(),
                         'native runner requires cases absent from its checked Rust source')

    def test_checksum_inputs_reject_symlinks_and_missing_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            regular = root / 'synthetic-input'
            regular.write_bytes(b'synthetic checksum fixture')
            self.assertEqual(runner.sha(regular), hashlib.sha256(regular.read_bytes()).hexdigest())
            link = root / 'link'
            link.symlink_to(regular)
            for path in (link, root / 'missing', root, Path('relative-input')):
                with self.subTest(path=str(path)), self.assertRaises(ValueError):
                    runner.sha(path)

    def test_inactive_service_without_kernel_retirement_proof_must_fail(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifact = root / 'evidence'
            (artifact / 'sandbox').mkdir(parents=True)
            binary = artifact / 'sandbox/native-sandbox-test-binary'
            helper = root / 'helper'
            source = root / 'ilium-platform/tests/native_animation_sandbox.rs'
            source.parent.mkdir(parents=True)
            workspace = root / 'Cargo.toml'
            for path in (binary, helper, source, workspace):
                path.write_bytes(b'synthetic native runner custody fixture')
            binary.chmod(0o700)
            helper.chmod(0o700)
            digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
            (artifact / 'native-sandbox-artifact.json').write_text(json.dumps({
                'schema': 1, 'state': 'compiled-not-qualified', 'executed': False,
                'filename': binary.name, 'target': 'x86_64-unknown-linux-gnu',
                'tag': 'v0.1.1', 'sha256': digest(binary),
                'helper_sha256': digest(helper), 'source_sha256': digest(source)}))
            commands = []
            def run(command, **options):
                commands.append(command)
                if command[0] == 'systemd-run':
                    case = command[-5]
                    text = 'test ' + case + ' ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured;\n'
                    return subprocess.CompletedProcess(command, 0, text, '')
                value = 'not-found\n' if '--property=LoadState' in command else 'inactive\n'
                return subprocess.CompletedProcess(command, 0, value, '')
            with patch.object(runner.os, 'getuid', return_value=1000), \
                    patch.object(runner.platform, 'system', return_value='Linux'), \
                    patch.object(runner.platform, 'machine', return_value='x86_64'), \
                    patch.object(runner.subprocess, 'run', side_effect=run), redirect_stdout(io.StringIO()):
                exit_code = runner.main(['--artifact-directory', str(artifact),
                                         '--helper', str(helper), '--workspace', str(workspace),
                                         '--output', str(root / 'result')])
            self.assertEqual(exit_code, 1)
            launched = next(command for command in commands if command[0] == 'systemd-run')
            unit = next(arg.split('=', 1)[1] for arg in launched if arg.startswith('--unit='))
            self.assertIn(['systemctl', '--user', 'stop', unit + '.service'], commands)
            self.assertEqual(json.loads((root / 'result/native-sandbox-tests.json').read_text())['state'], 'failed')

    def test_timeout_stops_only_the_service_created_by_the_runner(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifact = root / 'evidence'
            (artifact / 'sandbox').mkdir(parents=True)
            binary = artifact / 'sandbox/native-sandbox-test-binary'
            helper = root / 'helper'
            source = root / 'ilium-platform/tests/native_animation_sandbox.rs'
            source.parent.mkdir(parents=True)
            workspace = root / 'Cargo.toml'
            for path in (binary, helper, source, workspace):
                path.write_bytes(b'synthetic native runner custody fixture')
            binary.chmod(0o700)
            helper.chmod(0o700)
            digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
            (artifact / 'native-sandbox-artifact.json').write_text(json.dumps({
                'schema': 1, 'state': 'compiled-not-qualified', 'executed': False,
                'filename': binary.name, 'target': 'x86_64-unknown-linux-gnu',
                'tag': 'v0.1.1', 'sha256': digest(binary),
                'helper_sha256': digest(helper), 'source_sha256': digest(source)}))
            commands = []
            def run(command, **options):
                commands.append(command)
                if command[0] == 'systemd-run':
                    raise subprocess.TimeoutExpired(command, 180)
                return subprocess.CompletedProcess(command, 0, 'not-found\n', '')
            with patch.object(runner.os, 'getuid', return_value=1000), \
                    patch.object(runner.platform, 'system', return_value='Linux'), \
                    patch.object(runner.platform, 'machine', return_value='x86_64'), \
                    patch.object(runner.subprocess, 'run', side_effect=run), redirect_stdout(io.StringIO()):
                exit_code = runner.main(['--artifact-directory', str(artifact),
                                         '--helper', str(helper), '--workspace', str(workspace),
                                         '--output', str(root / 'result')])
            self.assertEqual(exit_code, 1)
            launched = next(command for command in commands if command[0] == 'systemd-run')
            unit = next(arg.split('=', 1)[1] for arg in launched if arg.startswith('--unit='))
            self.assertIn(['systemctl', '--user', 'stop', unit + '.service'], commands)
            self.assertEqual(json.loads((root / 'result/native-sandbox-tests.json').read_text())['state'], 'failed')

    def test_each_command_uses_exact_case_in_an_ordinary_owned_service(self):
        command = runner.test_command(Path('/absolute/test-binary'),
                                      runner.CASES[0], 'ilium-native-test-synthetic')
        self.assertIn('--user', command)
        self.assertIn('--property=Delegate=no', command)
        self.assertIn('--property=RuntimeMaxSec=120', command)
        self.assertEqual(command[-6:], ['/absolute/test-binary', runner.CASES[0],
                                       '--exact', '--ignored', '--test-threads=1', '--nocapture'])
        with self.assertRaises(ValueError):
            runner.test_command(Path('/absolute/test-binary'), 'unreviewed_case', 'ilium-native-test-synthetic')

    def test_kernel_retirement_requires_admission_and_actual_path_absence(self):
        unit = 'ilium-native-test-synthetic'
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            group = '/user.slice/' + unit + '.service'
            report = runner.CGROUP_REPORT_PREFIX + json.dumps(
                {'path': group, 'device': 1, 'inode': 2})
            remaining = root / group.lstrip('/')
            remaining.mkdir(parents=True)
            with self.assertRaisesRegex(ValueError, 'remains'):
                runner.kernel_retirement(report, unit, group, root)
            remaining.rmdir()
            self.assertTrue(runner.kernel_retirement(report, unit, '', root)['absent'])
            for output, manager in [('', ''), (report + '\n' + report, ''),
                                    (report, '/other.service')]:
                with self.subTest(output=output, manager=manager), self.assertRaises(ValueError):
                    runner.kernel_retirement(output, unit, manager, root)

    def test_malformed_cgroup_reports_fail_with_a_controlled_error(self):
        for payload in ('null', '[]', 'true', '"text"', '{}',
                        '{"path": "/ilium-native-test-synthetic.service", "device": 1, "inode": 2, "extra": true}'):
            with self.subTest(payload=payload), self.assertRaises(ValueError):
                runner.cgroup_identity(runner.CGROUP_REPORT_PREFIX + payload,
                                       'ilium-native-test-synthetic')

    def test_only_one_executed_passing_case_is_accepted(self):
        case = runner.CASES[0]
        output = ('running 1 test\ntest ' + case + ' ... ok\n'
                  'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.1s\n')
        self.assertTrue(runner.case_passed(case, 0, output))
        for code, text in [(1, output), (0, output.replace('1 passed', '0 passed')),
                           (0, output.replace('0 ignored', '1 ignored')),
                           (0, output.replace(case, 'another_case'))]:
            with self.subTest(code=code, text=text):
                self.assertFalse(runner.case_passed(case, code, text))


if __name__ == '__main__':
    unittest.main()
