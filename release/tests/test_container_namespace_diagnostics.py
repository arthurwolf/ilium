#!/usr/bin/env python3
import importlib.util  # Import only the supplied guest controller, without smoke-harness dependencies.
import io  # Supply bounded synthetic proc streams without reading host topology.
import json  # Inspect the actual failure record published by main.
import os  # Preserve real filesystem stat calls outside mocked proc identities.
from pathlib import Path  # Read the guest source and own temporary test outputs.
import tempfile  # Keep record-publication tests in private disposable directories.
from types import SimpleNamespace  # Model only kernel fields consumed by the guest.
import unittest  # Run independent behavioral regressions without root or containers.
from unittest.mock import patch  # Replace only kernel observations and forbidden process launches.

guest_path = Path(__file__).resolve().parents[2] / 'release/packaging/linux/container_fixture_guest.py'  # Import the selected checkout's guest controller.
guest_specification = importlib.util.spec_from_file_location('namespace_diagnostic_guest', guest_path)  # Load the reviewed module without entering its CLI.
guest = importlib.util.module_from_spec(guest_specification)  # Keep test state separate from any production import.
guest_specification.loader.exec_module(guest)  # Execute only definitions and fixed constants.
real_stat = os.stat  # Keep ordinary temporary-file operations outside the namespace mock.


class namespace_diagnostic_tests(unittest.TestCase):  # Exercise rejected layouts and durable failure evidence.
    def records(self, mountinfo):  # Model exactly the available pre-transaction kernel records.
        return {'/proc/1/comm': 'systemd\n', '/proc/self/mountinfo': mountinfo, '/proc/1/cgroup': '0::/init.scope\n', '/proc/self/cgroup': '0::/system.slice/ilium-container-acceptance.service\n', '/proc/1/uid_map': '0 524288 65536\n', '/proc/1/gid_map': '0 524288 65536\n', '/sys/fs/cgroup/cgroup.controllers': 'cpu memory\n', '/smoke/fixture.json': json.dumps({'nonce': 'a' * 32})}  # Missing pids stops the positive mount case at the next unchanged gate.

    def mocked_path(self, records):  # Provide both original reads and new bounded diagnostic streams.
        def select_path(value):  # Resolve only the fixed paths explicitly supplied by this test.
            name = str(value)  # Match the guest's actual kernel path spelling.
            self.assertIn(name, records)  # An unexpected read must fail rather than inspect the host.
            return SimpleNamespace(read_text=lambda: records[name], open=lambda *args, **kwargs: io.StringIO(records[name]))  # Keep each diagnostic stream independently closeable.
        return select_path  # Supply a callable patch target without touching pathlib globally.

    def kernel_stat(self, path, *args, **kwargs):  # Model only namespace identity observations.
        if str(path).startswith(('/proc/1/ns/', '/proc/self/ns/')):  # Both views refer to the same synthetic container namespaces.
            return SimpleNamespace(st_ino=123)  # Host corroboration is deliberately outside this guest-only regression.
        return real_stat(path, *args, **kwargs)  # Preserve real temporary output creation and hashing.

    def test_mount_predicates_remain_independent_and_fail_closed(self):  # Diagnose each conjunct while keeping every rejection effective.
        for mount_root, options, filesystem in (('/', 'rw,nosuid', 'cgroup2'), ('/host/subtree', 'rw', 'cgroup2'), ('/', 'ro', 'cgroup2'), ('/', 'rw', 'cgroup'), ('/host', 'ro', 'tmpfs')):  # Include a valid mount that reaches the subsequent controller gate.
            with self.subTest(root=mount_root, options=options, filesystem=filesystem):  # Name the exact predicate combination on regression.
                row = '50 40 0:28 ' + mount_root + ' /sys/fs/cgroup ' + options + ' shared:5 - ' + filesystem + ' cgroup rw\n'  # Exercise optional mountinfo fields before the separator.
                diagnostics = {}  # Inspect mutations retained even when the call raises.
                expected = 'container lacks cpu, memory or pids' if (mount_root, options, filesystem) == ('/', 'rw,nosuid', 'cgroup2') else 'cgroup namespace root is not a writable unified hierarchy'  # Preserve both original rejection messages.
                with patch.object(guest, 'Path', side_effect=self.mocked_path(self.records(row))), patch.object(guest.os, 'stat', side_effect=self.kernel_stat):  # Never use actual namespaces or control files.
                    with self.assertRaisesRegex(ValueError, expected):  # Passing a bad layout would regress the isolation gate.
                        guest.namespace_record(diagnostics)  # Run the actual predicate and diagnostic capture.
                self.assertEqual(diagnostics['cgroup_mount_rows'], [{'text': row.rstrip('\n'), 'truncated': False}])  # Retain exact raw evidence rather than a reconstructed mount line.
                self.assertEqual(diagnostics['mount_checks'], {'root_is_slash': mount_root == '/', 'mount_rw': 'rw' in options.split(','), 'filesystem_is_cgroup2': filesystem == 'cgroup2', 'superblock_rw': True})  # Verify that all predicates are measured independently.

    def test_missing_and_duplicate_mounts_still_reject(self):  # Do not select a convenient row from an ambiguous topology.
        row = '50 40 0:28 / /sys/fs/cgroup rw - cgroup2 cgroup rw\n'  # Supply an otherwise admissible exact mountpoint.
        for count in (0, 2):  # Absence and overmount ambiguity remain distinct diagnostics.
            diagnostics = {}  # Preserve rejected observations for each case independently.
            with patch.object(guest, 'Path', side_effect=self.mocked_path(self.records(row * count))), patch.object(guest.os, 'stat', side_effect=self.kernel_stat):  # Keep every kernel read synthetic.
                with self.assertRaisesRegex(ValueError, 'cgroup mount is missing or ambiguous'):  # The original uniqueness gate remains authoritative.
                    guest.namespace_record(diagnostics)  # Exercise the complete guest namespace path.
            self.assertEqual(diagnostics['cgroup_mount_count'], count)  # Report the exact observed row count.
            self.assertNotIn('mount_checks', diagnostics)  # Do not manufacture checks for an unselected mount.

    def test_diagnostic_retention_remains_bounded_and_explicit(self):  # Exercise worst-case JSON escaping under the existing host cap.
        row = '50 40 0:28 /' + '\U0001f642' * 3000 + ' /sys/fs/cgroup rw - cgroup2 cgroup rw\n'  # Force raw-row truncation without changing target selection.
        records = self.records(row * 9)  # Exceed the retained row-count ceiling independently.
        for name in ('/proc/1/cgroup', '/proc/self/cgroup', '/proc/1/uid_map', '/proc/1/gid_map'):  # Cover every supplemental text allocation.
            records[name] = '\U0001f642' * 1000  # Default JSON escaping expands each character twelvefold.
        diagnostics = {}  # Measure the actual object that result.json will contain.
        with patch.object(guest, 'Path', side_effect=self.mocked_path(records)), patch.object(guest.os, 'stat', side_effect=self.kernel_stat):  # Avoid allocating or inspecting a real kernel hierarchy.
            with self.assertRaisesRegex(ValueError, 'cgroup mount is missing or ambiguous'):  # Truncation must never select a passing row.
                guest.namespace_record(diagnostics)  # Keep the original topology failure authoritative.
        self.assertEqual(len(diagnostics['cgroup_mount_rows']), 4)  # Enforce the raw-row retention bound.
        self.assertTrue(diagnostics['cgroup_mount_rows_truncated'])  # Mark discarded rows explicitly.
        self.assertTrue(all(row['truncated'] for row in diagnostics['cgroup_mount_rows']))  # Mark every shortened retained row explicitly.
        result = {'schema': 1, 'nonce': 'a' * 32, 'state': 'failed', 'case_exit_code': None, 'user_manager_retired': False, 'errors': ['cgroup mount is missing or ambiguous'], 'namespace_diagnostics': diagnostics, 'case.stdout_sha256': guest.hashlib.sha256(b'').hexdigest(), 'case.stderr_sha256': guest.hashlib.sha256(b'').hexdigest()}  # Use the full failed-guest envelope emitted for this rejection.
        with tempfile.TemporaryDirectory() as directory:  # Measure actual UTF-8 atomic_json output, including final LF.
            path = Path(directory) / 'result.json'  # Keep all test evidence in an owned temporary scope.
            guest.atomic_json(path, result)  # Exercise the production JSON escaping and publication path.
            self.assertLess(len(path.read_bytes()), 90000)  # Leave at least 38000 bytes below the unchanged host recovery ceiling.

    def test_optional_diagnostic_read_failure_preserves_topology_error(self):  # Diagnostic access cannot replace the rejection being diagnosed.
        row = '50 40 0:28 /host/subtree /sys/fs/cgroup rw - cgroup2 cgroup rw\n'  # Keep the topology rejection independent from supplemental evidence.
        original_path = self.mocked_path(self.records(row))  # Preserve every unaffected synthetic kernel record.
        def denied_open(*args, **kwargs):  # Model an inaccessible optional membership observation.
            raise OSError('diagnostic membership denied')  # The original mount predicate must still execute.
        def select_path(value):  # Replace only the supplemental PID 1 membership stream.
            selected = original_path(value)  # Keep unknown-path assertions active.
            if str(value) == '/proc/1/cgroup':  # Avoid changing the actual mount table or namespace identities.
                selected.open = denied_open  # Fail only this optional diagnostic read.
            return selected  # Preserve all other kernel observations.
        diagnostics = {}  # Capture both the missing evidence and the actual failing predicate.
        with patch.object(guest, 'Path', side_effect=select_path), patch.object(guest.os, 'stat', side_effect=self.kernel_stat):  # Keep execution confined to the test seams.
            with self.assertRaisesRegex(ValueError, 'cgroup namespace root is not a writable unified hierarchy'):  # A diagnostic error must not mask isolation failure.
                guest.namespace_record(diagnostics)  # Run the original guard after the failed optional read.
        self.assertEqual(diagnostics['/proc/1/cgroup'], {'error': 'diagnostic membership denied'})  # Report unavailable evidence instead of fabricating membership.
        self.assertFalse(diagnostics['mount_checks']['root_is_slash'])  # Still retain the discriminating original observation.

    @unittest.skipUnless(os.name == 'posix', 'Linux guest uses O_NOFOLLOW; native acceptance remains mandatory')  # Keep Windows release test discovery portable.
    def test_main_publishes_failure_without_starting_manager_or_package(self):  # Preserve useful evidence through the real finalization path.
        row = '50 40 0:28 /host/subtree /sys/fs/cgroup rw - cgroup2 cgroup rw\n'  # Reject before manager startup and host admission.
        with tempfile.TemporaryDirectory() as directory:  # Own every emitted record and empty output stream.
            state_root = Path(directory)  # Preserve actual filesystem publication and SHA-256 behavior.
            with patch.object(guest, 'state_root', state_root), patch.object(guest, 'Path', side_effect=self.mocked_path(self.records(row))), patch.object(guest.os, 'stat', side_effect=self.kernel_stat), patch.object(guest.os, 'geteuid', return_value=0), patch.object(guest, 'run') as manager_run, patch.object(guest.subprocess, 'run') as case_run:  # Mock authority observations while prohibiting external execution.
                self.assertEqual(guest.main(), 1)  # Capability rejection must retain a failed exit status.
            result = json.loads((state_root / 'result.json').read_text())  # Inspect the actual atomically published record.
            self.assertEqual(result['state'], 'failed')  # Diagnostic availability is never package qualification.
            self.assertEqual(result['errors'], ['cgroup namespace root is not a writable unified hierarchy'])  # Keep the primary failure intact.
            self.assertIsNone(result['case_exit_code'])  # The package transaction was never attempted.
            self.assertFalse(result['user_manager_retired'])  # No manager was started and no retirement is fabricated.
            self.assertNotIn('namespace', result)  # Partial observations cannot impersonate accepted evidence.
            self.assertFalse(result['namespace_diagnostics']['mount_checks']['root_is_slash'])  # Preserve the discriminating predicate after failure.
            self.assertEqual(result['case.stdout_sha256'], guest.hashlib.sha256(b'').hexdigest())  # Preserve uniform output hashing even before execution.
            manager_run.assert_not_called()  # Do not start or stop a manager for a rejected topology.
            case_run.assert_not_called()  # Never attempt installation before namespace admission.

    @unittest.skipUnless(os.name == 'posix', 'Linux guest uses O_NOFOLLOW; native acceptance remains mandatory')  # Keep Windows release test discovery portable.
    def test_main_rejects_retained_hybrid_topology_without_starting_manager_or_package(self):  # Replay the observed failure through real finalization.
        row = '519 635 0:57 / /sys/fs/cgroup ro,nosuid,nodev,noexec shared:303 - tmpfs tmpfs ro,size=4096k,nr_inodes=1024,mode=755,uid=678297600,gid=678297600,inode64\n'  # Reproduce the actual Debian 12 hybrid mount recorded in the native packet.
        records = self.records(row)  # Preserve the real failed topology independently of launch configuration.
        records['/proc/1/cgroup'] = '1:name=systemd:/init.scope\n0::/init.scope\n'  # Both legacy and unified memberships coexist in the failed boot.
        records['/proc/self/cgroup'] = '1:name=systemd:/system.slice/ilium-container-acceptance.service\n0::/system.slice/ilium-container-acceptance.service\n'  # A unified membership line alone cannot establish a usable cgroup-v2 mount.
        with tempfile.TemporaryDirectory() as directory:  # Own every emitted record and empty output stream.
            state_root = Path(directory)  # Preserve actual filesystem publication and SHA-256 behavior.
            with patch.object(guest, 'state_root', state_root), patch.object(guest, 'Path', side_effect=self.mocked_path(records)), patch.object(guest.os, 'stat', side_effect=self.kernel_stat), patch.object(guest.os, 'geteuid', return_value=0), patch.object(guest, 'run') as manager_run, patch.object(guest.subprocess, 'run') as case_run:  # Mock authority observations while prohibiting external execution.
                self.assertEqual(guest.main(), 1)  # Capability rejection must retain a failed exit status.
            result = json.loads((state_root / 'result.json').read_text())  # Inspect the actual atomically published record.
            self.assertEqual(result['state'], 'failed')  # Diagnostic availability is never package qualification.
            self.assertEqual(result['errors'], ['cgroup namespace root is not a writable unified hierarchy'])  # Keep the primary failure intact.
            self.assertIsNone(result['case_exit_code'])  # The package transaction was never attempted.
            self.assertFalse(result['user_manager_retired'])  # No manager was started and no retirement is fabricated.
            self.assertNotIn('namespace', result)  # Partial observations cannot impersonate accepted evidence.
            self.assertEqual(result['namespace_diagnostics']['mount_checks'], {'root_is_slash': True, 'mount_rw': False, 'filesystem_is_cgroup2': False, 'superblock_rw': False})  # Retain all four actually observed predicate outcomes.
            self.assertEqual(result['namespace_diagnostics']['cgroup_mount_rows'], [{'text': row.rstrip('\n'), 'truncated': False}])  # Preserve the exact native failure pattern after finalization.
            self.assertEqual(result['namespace_diagnostics']['/proc/1/cgroup']['text'], records['/proc/1/cgroup'])  # Do not hide hybrid membership behind the available unified row.
            self.assertFalse((state_root / 'ready.json').exists())  # Rejected topology cannot ask the host to admit the package transaction.
            self.assertEqual(result['case.stdout_sha256'], guest.hashlib.sha256(b'').hexdigest())  # Preserve uniform output hashing even before execution.
            manager_run.assert_not_called()  # Do not start or stop a manager for a rejected topology.
            case_run.assert_not_called()  # Never attempt installation before namespace admission.


if __name__ == '__main__':  # Support direct isolated execution and unittest discovery.
    unittest.main()  # Return the real regression outcome.
