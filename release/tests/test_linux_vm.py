"""VM cleanup must retain process ownership instead of signaling a saved PID."""
from pathlib import Path
from contextlib import redirect_stderr, redirect_stdout  # Keep synthetic command diagnostics out of the test runner's output.
import io  # Capture expected parser errors and synthetic guest output.
import json  # Read retained applicability evidence as structured data.
import shlex  # Assert actual guest argv rather than command substrings.
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/packaging/linux'))
import vm_smoke


class ProcessOwnershipTests(unittest.TestCase):
    def test_guest_shutdown_timeout_still_reaps_only_owned_child(self):
        process = Mock()
        process.poll.return_value = None
        process.wait.side_effect = [subprocess.TimeoutExpired('qemu', 10), 0]
        with patch.object(vm_smoke, 'ssh', side_effect=subprocess.TimeoutExpired('ssh', 30)), \
             patch.object(vm_smoke, 'emit'):
            vm_smoke.stop_owned_vm(process, 23456, Path('synthetic-key'))
        process.terminate.assert_called_once_with()
        process.kill.assert_not_called()
        self.assertEqual(process.wait.call_count, 2)

    def test_already_exited_vm_does_not_contact_or_signal_another_owner(self):
        process = Mock()
        process.poll.return_value = 0
        process.wait.return_value = 0
        with patch.object(vm_smoke, 'ssh') as shutdown:
            vm_smoke.stop_owned_vm(process, 23456, Path('synthetic-key'))
        shutdown.assert_not_called()
        process.terminate.assert_not_called()
        process.kill.assert_not_called()
        process.wait.assert_called_once_with(timeout=10)

    def test_successful_vm_run_never_signals_a_detached_saved_pid(self):
        commands = []
        process = Mock(pid=424242)
        process.poll.return_value = None
        process.wait.return_value = 0

        def run(command, **options):
            command = [str(part) for part in command]
            commands.append(command)
            if command[0] == 'ssh-keygen':
                Path(command[-1] + '.pub').write_text('synthetic-public-key')
            if command[0] == 'qemu-system-x86_64' and '-pidfile' in command:
                Path(command[command.index('-pidfile') + 1]).write_text('424242')
            return subprocess.CompletedProcess(command, 0, '', '')

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'packages').mkdir()
            (root / 'packages/fixture.deb').write_bytes(b'labelled synthetic package')
            def ssh_result(port, key, command, timeout=3600):
                words = shlex.split(command)
                if words[:2] == ['python3', '-c']:
                    count = len(json.loads(words[3]))
                    return subprocess.CompletedProcess([], 0, json.dumps({'type': 'result', 'command': 'vm-inputs', 'state': 'passed', 'files': count, 'mismatches': []}), '')
                return subprocess.CompletedProcess([], 0, '', '')
            with patch.object(vm_smoke, 'verified_image', return_value='fixture-hash'), \
                 patch.object(vm_smoke, 'run', side_effect=run), \
                 patch.object(vm_smoke, 'ssh', side_effect=ssh_result) as guest_commands, \
                 patch.object(vm_smoke.time, 'sleep'), \
                 patch.object(vm_smoke.subprocess, 'Popen', return_value=process) as spawn, \
                 patch.object(vm_smoke, 'emit'):
                result = vm_smoke.main(['--packages', str(root / 'packages'),
                                        '--work', str(root / 'work'), '--log', str(root / 'log')])
            self.assertEqual(result, 0)
            self.assertNotIn(['kill', '424242'], commands,
                             'the old PID may belong to another process after guest shutdown')
            spawn.assert_called_once()
            self.assertNotIn('-daemonize', spawn.call_args.args[0])
            process.wait.assert_called()
            host_commands = [call.args[2] for call in guest_commands.call_args_list
                             if 'smoke_linux_packages.py host ' in call.args[2]]
            self.assertEqual(len(host_commands), 1)
            self.assertIn('--flatpak-user-dir /home/tester/flatpak-install', host_commands[0],
                          'Flatpak lookup must survive lifecycle HOME/XDG isolation')


class vm_coverage_tests(unittest.TestCase):  # Exercise real orchestration with safe mocked transports and an owned synthetic child.
    def run_vm(self, formats=None, statuses=None, upload_failure=None, diagnostics_failure=False, prepare_failure=False, host_timeout=False, readiness_timeouts=0, destination_exists=False, hash_mismatch=False):  # Keep all fixture state in temporary data.
        statuses = statuses or {}  # Unspecified required stages complete successfully.
        commands, guest_commands, events = [], [], []  # Retain observable adapter calls and JSONL emissions.
        process = Mock(pid=424242)  # Model only the child returned by this test's Popen adapter.
        process.poll.return_value = None  # Keep the synthetic VM alive until owned teardown.
        process.wait.return_value = 0  # Complete bounded reap without touching a real process.
        def run(command, **options):  # Replace all local executable launches with fixture behavior.
            command = [str(part) for part in command]  # Match the production adapter's argv normalization.
            commands.append(command)  # Keep the actual transport arguments for assertions.
            if command[0] == 'ssh-keygen':  # Supply the public key consumed by cloud-init setup.
                Path(command[-1] + '.pub').write_text('synthetic-public-key', encoding='utf-8')  # Create only owned fixture data.
            if command[0] == 'scp':  # Force upload and diagnostic collection failures independently.
                if command[-2] == 'tester@127.0.0.1:log-host' and diagnostics_failure:  # Identify the actual download direction.
                    return subprocess.CompletedProcess(command, 1, '', 'synthetic diagnostics transfer failed')  # Preserve a collection-specific diagnosis.
                if upload_failure and command[-1].endswith(':' + upload_failure):  # Target the exact mandatory upload.
                    return subprocess.CompletedProcess(command, 1, '', 'synthetic upload failed: ' + upload_failure)  # Simulate a transport failure before qualification.
            return subprocess.CompletedProcess(command, 0, '', '')  # Other mocked setup and transfer commands complete.
        def ssh(port, key, command, timeout=3600):  # Model guest commands without opening a network connection.
            nonlocal readiness_timeouts
            argv = shlex.split(command)  # Parse the quoted command sent to the guest shell.
            if command == 'test -e /var/lib/cloud/instance/ilium-ready' and readiness_timeouts:
                readiness_timeouts -= 1
                raise subprocess.TimeoutExpired(argv, timeout, output=b'boot pending\n', stderr=b'SSH handshake pending\n')
            if argv[:2] == ['mkdir', '-p'] and prepare_failure:  # Fail directory preparation before any package upload.
                return subprocess.CompletedProcess(argv, 1, '', 'synthetic preparation failed')  # Keep this failure distinct from SCP.
            if command.startswith('test ! -e ') and destination_exists:
                return subprocess.CompletedProcess(argv, 1, '', 'synthetic old guest destination')
            if argv[:2] == ['python3', '-c']:
                proof = {'type': 'result', 'command': 'vm-inputs', 'state': 'failed' if hash_mismatch else 'passed',
                         'files': len(json.loads(argv[3])), 'mismatches': ['old harness'] if hash_mismatch else []}
                return subprocess.CompletedProcess(argv, int(hash_mismatch), json.dumps(proof), '')
            if len(argv) > 2 and argv[1].endswith('smoke_linux_packages.py'):  # Record only real smoke invocations.
                guest_commands.append(argv)  # Preserve the requested format order and options.
                if argv[2] == 'host' and host_timeout:  # Test the actual timeout path through the command wrapper.
                    raise subprocess.TimeoutExpired(argv, timeout, output=b'partial host output\n', stderr=b'partial host error\n')  # Supply recoverable diagnostics.
                return subprocess.CompletedProcess(argv, statuses.get(argv[2], 0), '', '')  # Control each required gate independently.
            return subprocess.CompletedProcess(argv, 0, '', '')  # Readiness and graceful shutdown are safe fixture successes.
        with tempfile.TemporaryDirectory(prefix='ilium-vm-fixture-') as temporary:  # Avoid all repository and developer-machine mutations.
            root = Path(temporary)  # Own the fixture's keys, logs and cloud-init input.
            (root / 'packages with spaces').mkdir()
            (root / 'packages with spaces/fixture.deb').write_bytes(b'labelled synthetic package')
            arguments = ['--packages', str(root / 'packages with spaces'), '--work', str(root / 'work'), '--log', str(root / 'log')]  # Include whitespace in a local source argument.
            if formats is not None:  # Exercise default formatting when the option is absent.
                arguments += ['--formats', formats]  # Pass caller spelling without fixture normalization.
            with patch.object(vm_smoke, 'verified_image', return_value='fixture-hash'), patch.object(vm_smoke, 'run', side_effect=run), patch.object(vm_smoke, 'ssh', side_effect=ssh), patch.object(vm_smoke.time, 'sleep'), patch.object(vm_smoke.subprocess, 'Popen', return_value=process), patch.object(vm_smoke, 'emit', side_effect=lambda kind, **values: events.append({'type': kind, **values})), redirect_stdout(io.StringIO()):  # Mock every native or network boundary.
                result = vm_smoke.main(arguments)  # Execute the production main path and real owned-child teardown.
            logs = {path.name: path.read_text(encoding='utf-8') for path in (root / 'log').iterdir() if path.is_file()}  # Retain evidence before deleting fixture data.
        process.wait.assert_called()  # Every completed fixture run must reap its retained child.
        process.terminate.assert_not_called()  # Graceful synthetic shutdown must not trigger signals.
        process.kill.assert_not_called()  # Never substitute a process-name or saved-PID kill.
        return result, guest_commands, commands, logs, events  # Return only observations from the real orchestration path.

    def test_readiness_timeout_retries_without_qualifying_or_losing_diagnostics(self):
        result, guest, _commands, logs, _events = self.run_vm('flatpak', readiness_timeouts=1)
        self.assertEqual(result, 0)
        self.assertEqual([command[2] for command in guest], ['host'])
        self.assertIn('boot pending', logs['vm-ready-0.log'])
        self.assertIn('SSH handshake pending', logs['vm-ready-0.log'])
        self.assertIn('command timed out', logs['vm-ready-0.log'])

    def test_all_readiness_timeouts_fail_without_upload_or_smoke(self):
        result, guest, commands, logs, events = self.run_vm('flatpak', readiness_timeouts=180)
        self.assertEqual(result, 1)
        self.assertEqual(guest, [])
        self.assertFalse(any(command[0] == 'scp' for command in commands))
        self.assertEqual(sum(name.startswith('vm-ready-') for name in logs), 180)
        self.assertTrue(any(event.get('message') == 'virtual machine never finished cloud-init' for event in events))

    def test_invalid_formats_fail_before_creating_or_booting_anything(self):  # Unknown coverage cannot consume or mutate VM resources.
        for formats in ('', 'deb,', ',deb', 'deb,,snap', 'deb,deb', 'flatpak,flatpak', 'unknown', 'rpm', 'deb,unknown'):  # Include empty, duplicate and host-unsupported requests.
            with self.subTest(formats=formats), tempfile.TemporaryDirectory() as temporary:  # Isolate each preflight rejection.
                root = Path(temporary)  # Own only the test's empty outer directory.
                with patch.object(vm_smoke, 'verified_image') as verified, patch.object(vm_smoke, 'run') as run, patch.object(vm_smoke, 'ssh') as ssh, patch.object(vm_smoke.subprocess, 'Popen') as spawn, redirect_stderr(io.StringIO()):  # Observe every possible side-effect boundary.
                    with self.assertRaises(SystemExit) as raised:  # Preserve normal argparse rejection semantics.
                        vm_smoke.main(['--packages', str(root / 'packages'), '--work', str(root / 'work'), '--log', str(root / 'log'), '--formats', formats])  # Run the actual entry point.
                self.assertEqual(raised.exception.code, 2)  # Invalid coverage is a usage error.
                for boundary in (verified, run, ssh, spawn):  # No image, command, connection or child may exist.
                    boundary.assert_not_called()  # Confirm rejection occurred before VM setup.
                self.assertFalse((root / 'work').exists())  # Do not create work for rejected requests.
                self.assertFalse((root / 'log').exists())  # Do not create logs for rejected requests.

    def test_default_inspection_subset_keeps_all_host_formats(self):  # The default must preserve every native host obligation.
        result, guest, commands, _logs, _events = self.run_vm()  # Exercise the unchanged CLI default.
        self.assertEqual(result, 0)  # All applicable required stages passed in the fixture.
        self.assertEqual([argv[2] for argv in guest], ['inspect', 'host'])  # Both lanes run exactly once.
        self.assertEqual(guest[0][guest[0].index('--formats') + 1], 'deb,appimage,snap')  # Never ask B1's inspector to accept Flatpak.
        self.assertEqual(guest[1][guest[1].index('--formats') + 1], 'deb,appimage,snap,flatpak')  # Never silently drop Flatpak from host coverage.
        self.assertEqual(guest[1][guest[1].index('--flatpak-user-dir') + 1], '/home/tester/flatpak-install')  # Preserve primary's lifecycle-safe caller API.
        package_upload = next(command for command in commands if command[0] == 'scp' and command[-1].endswith(':packages'))  # Inspect the real local argv.
        self.assertTrue(package_upload[-2].endswith('packages with spaces'))  # Whitespace stays within one SCP source argument.

    def test_reordered_subset_preserves_host_request_order(self):  # Inspection filtering must not rewrite mandatory host coverage.
        result, guest, _commands, _logs, _events = self.run_vm('flatpak,deb')  # Exercise a nondefault mixed request.
        self.assertEqual(result, 0)  # Both requested formats qualified in the synthetic lane.
        self.assertEqual(guest[0][guest[0].index('--formats') + 1], 'deb')  # Only the supported subset reaches inspect.
        self.assertEqual(guest[1][guest[1].index('--formats') + 1], 'flatpak,deb')  # Preserve the full explicit host request.

    def test_flatpak_only_records_inapplicable_inspection_and_requires_host(self):  # No empty or defaulted inspector invocation can qualify Flatpak.
        result, guest, _commands, logs, events = self.run_vm('flatpak')  # Exercise the formerly invalid empty inspection case.
        self.assertEqual(result, 0)  # Mandatory host success is sufficient when no offline lane applies.
        self.assertEqual([argv[2] for argv in guest], ['host'])  # Never execute an empty inspector request.
        self.assertEqual(guest[0][guest[0].index('--formats') + 1], 'flatpak')  # The requested native gate still executes.
        evidence = json.loads(logs['vm-inspect.jsonl'])  # Validate retained JSONL evidence rather than console prose.
        self.assertEqual(evidence['state'], 'not-applicable')  # Inapplicable inspection must not claim a pass.
        self.assertEqual(evidence['required_host_formats'], ['flatpak'])  # Record the remaining mandatory obligation.
        self.assertIn(evidence, events)  # Emit the same applicability evidence visible to the caller.

    def test_required_stage_failures_are_sticky(self):  # Later successful stages must never erase required failure.
        for formats, statuses in (('deb', {'inspect': 1}), ('deb', {'host': 1}), ('flatpak', {'host': 1})):  # Cover inspection, host and host-only failure paths.
            with self.subTest(formats=formats, statuses=statuses):  # Keep each native-lane simulation independent.
                result, guest, _commands, _logs, events = self.run_vm(formats, statuses=statuses)  # Run the actual aggregation logic.
                self.assertEqual(result, 1)  # Any required failed stage rejects VM qualification.
                self.assertEqual(guest[-1][2], 'host')  # Host coverage still runs after an inspection failure.
                self.assertEqual(events[-1]['state'], 'failed')  # The final emitted VM result agrees with the exit status.

    def test_every_required_upload_failure_stops_before_smoke(self):  # Metadata uploads are as mandatory as package and source copies.
        for target in ('repo/release/scripts', 'repo/release/packaging', 'packages', 'repo/LICENSE', 'repo/Cargo.toml'):  # Exercise all five actual upload destinations.
            with self.subTest(target=target):  # A previous copy success must not hide the next failure.
                result, guest, _commands, logs, _events = self.run_vm(upload_failure=target)  # Force one precise SCP failure.
                self.assertEqual(result, 1)  # Missing inputs cannot qualify the VM.
                self.assertEqual(guest, [])  # Do not run native gates with incomplete transferred source.
                self.assertTrue(any('synthetic upload failed: ' + target in value for value in logs.values()))  # Retain the original transfer diagnosis.

    def test_guest_directory_preparation_failure_stops_before_smoke(self):  # SSH preparation is a checked prerequisite too.
        result, guest, _commands, logs, _events = self.run_vm(prepare_failure=True)  # Force the first remote directory operation to fail.
        self.assertEqual(result, 1)  # Refuse unverified upload destinations.
        self.assertEqual(guest, [])  # No required smoke runs after failed preparation.
        self.assertIn('synthetic preparation failed', logs['vm-copy-0-prepare.log'])  # Preserve the guest's explanation.

    def test_existing_guest_destination_and_mismatched_inputs_never_reach_smoke(self):
        for options in ({'destination_exists': True}, {'hash_mismatch': True}):
            with self.subTest(options=options):
                result, guest, commands, logs, events = self.run_vm(**options)
                self.assertEqual(result, 1)
                self.assertEqual(guest, [])
                if options.get('destination_exists'):
                    self.assertFalse(any(command[0] == 'scp' for command in commands))
                    self.assertIn('synthetic old guest destination', logs['vm-copy-0-fresh.log'])
                else:
                    self.assertIn('old harness', logs['vm-inputs-verification.jsonl'])

    def test_exact_guest_verifier_executes_and_detects_changed_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / 'source'; source.mkdir()
            file = source / 'space name.py'; file.write_bytes(b'original exact bytes')
            mapped = vm_smoke.upload_manifest(source, 'repo/release/scripts')
            self.assertEqual(set(mapped), {'/home/tester/repo/release/scripts/space name.py'})
            local_manifest = {str(file): next(iter(mapped.values()))}
            command = shlex.split(vm_smoke.guest_manifest_command(local_manifest))
            verified = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(verified.returncode, 0)
            self.assertEqual(json.loads(verified.stdout)['mismatches'], [])
            file.write_bytes(b'changed exact bytes')
            changed = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(changed.returncode, 1)
            self.assertEqual(json.loads(changed.stdout)['mismatches'], [str(file)])

    def test_diagnostics_copy_failure_disqualifies_successful_smoke(self):  # Missing evidence must not be silently ignored.
        result, guest, _commands, logs, events = self.run_vm(diagnostics_failure=True)  # Make native gates pass but their diagnostic transfer fail.
        self.assertEqual(result, 1)  # The overall VM result remains unqualified.
        self.assertEqual(guest[-1][2], 'host')  # Confirm this failure followed actual requested host execution.
        self.assertIn('synthetic diagnostics transfer failed', logs['vm-host-log-copy.log'])  # Retain the collection failure locally.
        self.assertEqual(events[-1]['state'], 'failed')  # Report failure rather than a misleading pass.

    def test_host_timeout_retains_partial_output_and_collects_diagnostics(self):  # An SSH timeout must not bypass evidence collection or custody.
        result, _guest, commands, logs, _events = self.run_vm('flatpak', host_timeout=True)  # Force a timeout on the mandatory host stage.
        self.assertEqual(result, 1)  # Timeout is a failed gate.
        self.assertIn('partial host output', logs['vm-host.jsonl'])  # Preserve output captured before timeout.
        self.assertIn('partial host error', logs['vm-host.jsonl'])  # Preserve error output captured before timeout.
        self.assertIn('command timed out', logs['vm-host.jsonl'])  # Explain why the gate failed.
        self.assertTrue(any(command[0] == 'scp' and command[-2] == 'tester@127.0.0.1:log-host' for command in commands))  # Collect available native logs before owned VM shutdown.


if __name__ == '__main__':
    unittest.main()
