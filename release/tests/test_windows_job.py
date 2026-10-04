"""Portable process-custody regressions; all Win32 calls are simulated."""  # These tests never certify native execution.
from __future__ import annotations  # Keep fixture annotations portable.
import ctypes as ct  # Exercise the real fixed-width native structures.
import importlib.util  # Verify that importing the helper invokes no Windows API.
import json  # Inspect retained evidence rather than helper names.
import os  # Write only fixture-owned retained output streams.
from pathlib import Path  # Resolve the installed test module and temporary fixtures.
import sys  # Install explicit test-only module adapters.
import tempfile  # All files remain fixture-owned.
from types import SimpleNamespace  # Avoid changing the host os.name globally.
import unittest  # Run on every existing source-test runner.
from unittest.mock import Mock, patch  # Replace native boundaries, not the tested custody methods.
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'release/scripts'))  # Match the supplied release-test import contract.
import windows_job as custody  # Importing must remain safe on Linux and macOS.
class test_windows_job(unittest.TestCase):  # Behavioral contracts operate through the real proposed helper.
    def setUp(self) -> None:  # unittest requires this framework method spelling.
        self.temporary = tempfile.TemporaryDirectory()  # Create an isolated evidence root per test.
        self.addCleanup(self.temporary.cleanup)  # Delete only this fixture's files.
        self.root = Path(self.temporary.name)  # Keep all exercised paths absolute.
        self.image = self.root / 'installed-server.exe'  # Synthetic bytes are never executed.
        self.image.write_bytes(b'fixture image')  # hold_image requires an actual regular path.
        self.calls, self.failures, self.last_error, self.active = [], {}, 5, 0  # Record calls and inject controlled native failures.
        self.ids, self.pid_handles, self.members = [7], {7: 701}, {501, 701}  # Separate a PID from its current kernel handle.
        self.wait_results, self.closed = {}, []  # Track original-handle waits and close-only foreign cleanup.
        self.api = Mock()  # Native exports also accept the real ctypes prototype declarations.
        names = ('CreateJobObjectW SetInformationJobObject AssignProcessToJobObject TerminateJobObject QueryInformationJobObject IsProcessInJob CreateProcessW ResumeThread TerminateProcess WaitForSingleObject CloseHandle GetExitCodeProcess OpenProcess QueryFullProcessImageNameW GetProcessTimes InitializeProcThreadAttributeList UpdateProcThreadAttribute DeleteProcThreadAttributeList').split()  # Explicit exported API fixture surface.
        for name in names:  # Record actual native-boundary call order.
            getattr(self.api, name).side_effect = lambda *arguments, name=name: self.dispatch(name, arguments)  # Invoke the stateful native simulator.
        fake_os = SimpleNamespace(name='nt', path=os.path, devnull=os.devnull, set_inheritable=Mock())  # Preserve host pathlib behavior and avoid changing real descriptor inheritance.
        for adapter in (patch.object(custody, 'os', fake_os), patch.object(ct, 'WinDLL', return_value=self.api, create=True), patch.object(ct, 'get_last_error', side_effect=lambda: self.last_error, create=True), patch.object(ct, 'WinError', side_effect=lambda code: OSError(code, 'injected native failure'), create=True), patch.dict(sys.modules, {'msvcrt': SimpleNamespace(get_osfhandle=lambda descriptor: descriptor)})):  # Replace every native-only boundary before construction.
            adapter.start()  # Run real helper logic against portable fake APIs.
            self.addCleanup(adapter.stop)  # Restore process-wide test adapters after job cleanup.
        self.job = custody.windows_job(self.root, 'fixture')  # Exercise real constructor limits and evidence creation.
        self.addCleanup(self.cleanup_job)  # Retain cleanup even when an assertion fails.
    def cleanup_job(self) -> None:  # Remove deliberate fault injection before fixture disposal.
        self.failures.clear()  # A test's expected failure must not affect framework cleanup.
        self.active = 0  # No simulated process needs host-side termination.
        self.job.terminate_and_close()  # Exercise idempotence after explicit tested cleanup too.
    def dispatch(self, name: str, arguments: tuple):  # Simulate native state changes, preserving exact passed handles.
        self.calls.append((name, arguments))  # Assertions observe actual ordering and target identity.
        if name in self.failures:  # Faults are injected at the native API boundary.
            failure = self.failures[name]  # Preserve a supplied exception object for identity assertions.
            if isinstance(failure, BaseException):  # Some tests force an interrupted native operation.
                raise failure  # Do not replace the injected primary exception.
            return failure  # Zero and WAIT_FAILED retain their distinct API meanings.
        if name == 'CreateJobObjectW':  # The fixture has one private job identity.
            return 100  # It is deliberately distinct from every process handle.
        if name == 'InitializeProcThreadAttributeList':  # Fill the sizing readback before initialization.
            arguments[3]._obj.value = 64  # Match the real helper's allocated attribute storage contract.
            return int(arguments[0] is not None)  # The first sizing call intentionally fails with a size.
        if name == 'CreateProcessW':  # Populate actual ctypes PROCESS_INFORMATION storage.
            information, startup = arguments[9]._obj, arguments[8]._obj.startup  # Inspect native creation outputs and inherited streams.
            information.process, information.thread, information.pid, information.tid = 501, 502, 7, 8  # Created handles are independent of PID reuse.
            os.write(startup.stdout, b'installed readback\n')  # Prove retained file readback without invoking a subprocess.
            return 1  # Creation succeeds while the thread remains suspended.
        if name == 'AssignProcessToJobObject':  # Successful admission precedes any executable instruction.
            self.active = 1  # The owned process is now represented in job accounting.
            return 1  # Resume is permitted only after this acknowledgment.
        if name == 'QueryFullProcessImageNameW':  # Identity comes through the opened process handle.
            arguments[2].value = str(self.image)  # Return the actual fixture path for strict resolution.
            return 1  # The caller still verifies job membership first.
        if name == 'GetProcessTimes':  # Creation identity must survive a later PID reassignment.
            arguments[1]._obj.value = 9000 + arguments[0]  # Distinguish each held process object.
            return 1  # The remaining zero times are irrelevant to identity.
        if name == 'GetExitCodeProcess':  # A completed command has a retained native exit code.
            arguments[1]._obj.value = 0  # No version or lifecycle success is inferred here.
            return 1  # The real helper returns this readback to its caller.
        if name == 'WaitForSingleObject':  # Wait semantics use the original handle, never the PID map.
            result = self.wait_results.get(arguments[0], 0)  # Tests choose live, exited or failed states independently.
            if arguments[0] == 501 and result == 0:  # Successful command retirement removes the created child.
                self.active = 0  # Reflect natural exit in subsequent accounting queries.
            return result  # All waits remain bounded by the helper's passed timeout.
        if name == 'QueryInformationJobObject':  # Model both authoritative native readback shapes.
            if arguments[1] == 1:  # Basic accounting reports only active owned processes.
                arguments[2]._obj.active = self.active  # Populate the real fixed-width structure.
                return 1  # The helper must not replace this value with cached state.
            buffer = arguments[2]  # Process-list layout uses fixed DWORD counts and pointer-width IDs.
            custody.dword.from_buffer(buffer, 0).value = len(self.ids)  # Report the required list length.
            custody.dword.from_buffer(buffer, 4).value = len(self.ids)  # This fixture snapshot is complete.
            for index, pid in enumerate(self.ids):  # Write IDs without dereferencing any host process.
                ct.c_size_t.from_buffer(buffer, 8 + index * ct.sizeof(ct.c_size_t)).value = pid  # Preserve platform pointer width.
            return 1  # Membership still requires independent handle-based verification.
        if name == 'OpenProcess':  # A reused PID can now map to a foreign kernel object.
            return self.pid_handles[arguments[2]]  # Keep this mapping independent from retained handles.
        if name == 'IsProcessInJob':  # Admission cannot rely on a previously enumerated PID.
            arguments[2]._obj.value = arguments[0] in self.members  # Verify the current opened handle's membership.
            return 1  # A false membership result is a successful native query.
        if name == 'CloseHandle':  # Closing an observation must not signal its process.
            self.closed.append(arguments[0])  # Record exactly which owned/foreign handles were released.
            return 1  # The fixture never closes an actual OS process handle.
        if name in ('TerminateJobObject', 'TerminateProcess'):  # Termination affects only fixture-owned state.
            self.active = 0  # Successful simulated retirement permits an authoritative empty readback.
            self.wait_results[501] = 0  # A terminated original child becomes signaled.
            return 1  # Tests inspect the target handle before accepting cleanup.
        return 1  # Remaining setup and attribute APIs acknowledge their bounded operation.
    def run_child(self, timeout: int = 3) -> dict:  # Execute the actual suspended-launch helper against the fake kernel.
        return self.job.run([str(self.image), '--version'], {'PATH': str(self.root)}, self.root, timeout)  # No command is executed on the host.
    def records(self) -> list[dict]:  # Evidence assertions consume the real persisted JSONL.
        return [json.loads(line) for line in (self.root / 'fixture.job.jsonl').read_text().splitlines()]  # Flushed rows survive failed operations.
    def test_import_is_portable_and_fixed_width(self) -> None:  # Loading release tests cannot start native work.
        with patch.object(ct, 'WinDLL', side_effect=AssertionError('native API loaded during import'), create=True):  # A prohibited import-time load fails immediately.
            specification = importlib.util.spec_from_file_location('custody_import_probe', custody.__file__)  # Re-execute only module definitions.
            specification.loader.exec_module(importlib.util.module_from_spec(specification))  # Verify portability independently of an already cached import.
        self.assertEqual(ct.sizeof(custody.dword), 4)  # Unix c_ulong must never widen a Windows DWORD.
        self.assertEqual(ct.sizeof(custody.accounting), 48)  # Preserve the authoritative accounting ABI.
    def test_launch_assigns_before_resume_and_retains_exact_output(self) -> None:  # Enforce custody before any executable instruction.
        result = self.run_child()  # Run real launch, wait, stream and cleanup logic.
        order = [name for name, _ in self.calls]  # Observe native call order, not source text.
        self.assertLess(order.index('CreateProcessW'), order.index('AssignProcessToJobObject'))  # Creation obtains the original process handle.
        self.assertLess(order.index('AssignProcessToJobObject'), order.index('ResumeThread'))  # No descendant can precede job membership.
        self.assertTrue(self.api.CreateProcessW.call_args.args[5] & 4)  # CREATE_SUSPENDED is mandatory.
        self.assertEqual(self.api.SetInformationJobObject.call_args.args[2]._obj.basic.flags, 0x2000)  # Kill-on-close excludes both breakaway flags.
        self.assertEqual(self.api.UpdateProcThreadAttribute.call_args.args[2], 0x20002)  # Inheritance uses an explicit handle allowlist.
        streams = self.api.CreateProcessW.call_args.args[8]._obj.startup  # Read the streams actually supplied to native creation.
        self.assertEqual(set(self.api.UpdateProcThreadAttribute.call_args.args[3]), {streams.stdin, streams.stdout, streams.stderr})  # The job and unrelated handles cannot inherit.
        self.assertEqual(result['stdout'], 'installed readback\n')  # File-backed output reaches the caller intact.
        self.assertEqual(result['process']['pid'], 7)  # Identity is retained alongside the command result.
        self.assertTrue(self.job.started)  # Successful resumption acknowledges possible account mutation.
    def test_failed_admission_never_resumes_and_retires_only_created_handle(self) -> None:  # Failed ownership cannot launch an unmanaged child.
        self.failures['AssignProcessToJobObject'] = 0  # Simulate incompatible parent-job limits.
        with self.assertRaises(OSError):  # Native admission failure remains a failed gate.
            self.run_child()  # The child exists but is still suspended.
        self.api.ResumeThread.assert_not_called()  # No user code may run after rejected admission.
        self.api.TerminateProcess.assert_called_once_with(501, 1)  # The exact creation handle is the only permitted direct termination.
        self.api.WaitForSingleObject.assert_called_once_with(501, 10000)  # Failed creation retirement is bounded.
        self.assertFalse(self.job.started)  # Attempted process creation alone cannot authorize account cleanup.
        self.assertTrue({501, 502} <= set(self.closed))  # Both creation handles are independently released.
    def test_creation_resume_and_validation_failures_do_not_claim_started(self) -> None:  # Account-mutation custody depends on actual resumption.
        for name, value in (('CreateProcessW', 0), ('ResumeThread', 0xFFFFFFFF)):  # Cover failure before and after job admission.
            with self.subTest(api=name):  # Each failure retains an independent command attempt.
                self.failures[name] = value  # Inject only the selected native failure.
                with self.assertRaises(OSError):  # Neither failure may become a normal exit.
                    self.run_child()  # Execute real failure unwinding.
                self.assertFalse(self.job.started)  # Created-but-suspended is never started.
                self.failures.clear()  # Permit the next independent scenario.
        self.run_child()  # Establish a previous successful resumption.
        with self.assertRaises(ValueError):  # Validation of a later command fails before creation.
            self.job.run(['relative.exe'], {}, self.root)  # Relative images cannot prove installed provenance.
        self.assertFalse(self.job.started)  # The prior command's launch flag cannot leak into this attempt.
    def test_timeout_is_bounded_and_preserves_started_and_evidence(self) -> None:  # A timed-out installer may already have changed account state.
        self.wait_results[501] = 258  # Keep the original created process live.
        with self.assertRaises(TimeoutError):  # A timeout cannot be rewritten as an exit result.
            self.run_child(2)  # Use a short explicit native wait without real sleeping.
        self.assertTrue(self.job.started)  # The caller must fence unsettled service-side transactions.
        self.api.WaitForSingleObject.assert_called_once_with(501, 2000)  # Observe the actual bounded native wait.
        self.assertTrue(any(row['type'] == 'process-timeout' for row in self.records()))  # Preserve evidence even on failure.
        self.api.TerminateProcess.assert_not_called()  # An admitted process stays under private-job custody.
    def test_original_handle_retirement_does_not_follow_a_reused_pid(self) -> None:  # A live replacement PID cannot falsify original-server exit.
        self.wait_results[701] = 258  # The installed original server starts alive.
        original = self.job.hold_image(self.image)  # Acquire a membership-verified original handle.
        self.pid_handles[7], self.wait_results[701], self.wait_results[702] = 702, 0, 258  # Reuse its PID for a foreign live process.
        self.assertTrue(original.wait(2))  # Only the original object's exit satisfies retirement.
        self.assertFalse(original.alive())  # The replacement PID does not resurrect the original object.
        self.assertEqual(original.identity['created'], 9701)  # Retain the original creation identity.
        self.assertEqual(self.api.OpenProcess.call_count, 1)  # Retirement performs no fresh PID lookup.
        self.assertTrue(all(call.args[0] == 701 for call in self.api.WaitForSingleObject.call_args_list))  # Every liveness read uses the held handle.
    def test_reused_foreign_pid_is_closed_without_identity_or_signal(self) -> None:  # Membership must be checked after OpenProcess.
        self.pid_handles[7] = 702  # The enumerated PID now identifies a foreign process.
        self.assertEqual(self.job.query(), [])  # Foreign membership cannot supply installed-server evidence.
        self.api.OpenProcess.assert_called_once_with(0x101000, False, 7)  # The opened handle has query/synchronize rights only.
        self.api.IsProcessInJob.assert_called_once()  # Validate that exact new handle against the private job.
        self.assertEqual(self.api.IsProcessInJob.call_args.args[:2], (702, 100))  # An ambient or null job check cannot substitute for private custody.
        self.api.QueryFullProcessImageNameW.assert_not_called()  # Do not interpret foreign identity as owned evidence.
        self.api.CloseHandle.assert_called_once_with(702)  # Foreign cleanup consists solely of releasing the observation.
        self.api.TerminateProcess.assert_not_called()  # No unrelated process is signaled.
    def test_identity_failure_survives_observation_close_failure(self) -> None:  # Cleanup cannot replace the actual custody-read failure.
        primary = ValueError('identity unavailable')  # Preserve a distinct exception instance.
        self.failures.update(QueryFullProcessImageNameW=primary, CloseHandle=0)  # Independently fail identity and observation cleanup.
        with self.assertRaises(ValueError) as caught:  # The original error type must survive.
            self.job.query()  # Exercise nested native-handle finalizers.
        self.assertIs(caught.exception, primary)  # Do not replace the primary exception object.
        self.assertIn('observation close', '\n'.join(primary.__notes__))  # Surface the secondary cleanup failure.
        self.api.TerminateProcess.assert_not_called()  # A failed read still cannot authorize signaling.
    def test_child_primary_failure_retains_handle_cleanup_errors(self) -> None:  # Preserve both failure categories in persisted evidence.
        primary = ValueError('wait readback unavailable')  # Simulate a failure after successful resumption.
        self.failures.update(WaitForSingleObject=primary, CloseHandle=0)  # Fail independent creation-handle cleanup too.
        with self.assertRaises(ValueError) as caught:  # Cleanup must not replace the original native wait failure.
            self.run_child()  # The actual run finally records cleanup diagnostics.
        self.assertIs(caught.exception, primary)  # Preserve exact exception identity.
        self.assertIn('child cleanup', '\n'.join(primary.__notes__))  # Keep secondary handle errors visible.
        self.assertTrue(any(row['type'] == 'child-cleanup-failure' for row in self.records()))  # Retain errors beyond exception formatting.
    def test_constructor_preserves_primary_and_both_cleanup_failures(self) -> None:  # Native custody setup must unwind without masking its failure.
        primary, sink = ValueError('limit binding failed'), Mock()  # The evidence sink is also explicitly controlled.
        self.failures.update(SetInformationJobObject=primary, CloseHandle=0)  # Fail native setup and its job close independently.
        sink.close.side_effect = OSError('evidence close failed')  # Add a second cleanup failure.
        with patch.object(Path, 'open', return_value=sink), self.assertRaises(ValueError) as caught:  # No real log file is intercepted outside this constructor call.
            custody.windows_job(self.root, 'constructor_failure')  # Exercise the real constructor failure path.
        self.assertIs(caught.exception, primary)  # The primary type, object and cause remain intact.
        notes = '\n'.join(primary.__notes__)  # Secondary failures are attached instead of replacing it.
        self.assertIn('job close', notes)  # Report uncertain native handle cleanup.
        self.assertIn('evidence close failed', notes)  # Report the independently unavailable evidence close.
    def test_job_cleanup_is_scoped_verified_and_idempotent(self) -> None:  # Forced cleanup is visible and never confused with graceful success.
        observed = self.job.query()[0]  # Hold an owned observation through cleanup.
        self.active = 1  # Simulate a leftover owned descendant.
        self.job.terminate_and_close()  # Use the real bounded cleanup implementation.
        self.api.TerminateJobObject.assert_called_once_with(100, 1)  # Signal only this private job.
        self.api.TerminateProcess.assert_not_called()  # Never use a process-name or PID fallback.
        self.assertEqual(observed.handle, 0)  # Release held references before final emptiness accounting.
        self.assertEqual(self.closed[-1], 100)  # The private job handle is released last.
        self.assertTrue(any(row['type'] == 'job-empty' and row['active'] == 0 for row in self.records()))  # Success follows native readback.
        self.job.terminate_and_close()  # Explicit caller cleanup can safely repeat.
        self.api.TerminateJobObject.assert_called_once()  # Repeated cleanup performs no new signal.
    def test_failed_termination_and_empty_readback_remain_bounded_failures(self) -> None:  # Last-handle close cannot manufacture a verified empty result.
        self.active, self.failures['TerminateJobObject'] = 1, 0  # The owned process cannot be confirmed retired.
        clock = SimpleNamespace(monotonic=Mock(side_effect=[0, 16]), sleep=Mock())  # Advance only the fixture clock beyond the deadline.
        with patch.object(custody, 'time', clock), self.assertRaisesRegex(OSError, 'native failure.*15 seconds'):  # Preserve both cleanup and readback failures.
            self.job.terminate_and_close()  # No real sleep or native termination occurs.
        self.assertEqual(self.closed[-1], 100)  # Still attempt the kill-on-close fallback for this private job.
        self.assertTrue(self.job.events.closed)  # Independent evidence cleanup must still run.
        self.assertFalse(any(row['type'] == 'job-empty' for row in self.records()))  # Failure cannot publish fabricated empty-job evidence.
        self.assertEqual(clock.monotonic.call_count, 2)  # The bounded loop cannot become an unending wait.
    def test_closed_job_cannot_query_the_ambient_parent_job(self) -> None:  # A NULL job handle would otherwise address caller ambient custody.
        self.job.terminate_and_close()  # Close the fixture's private job first.
        count = self.api.QueryInformationJobObject.call_count  # Freeze the legitimate pre-close query count.
        for operation in (self.job.query, self.job.active_count):  # Both public readback entry points need the same guard.
            with self.assertRaisesRegex(ValueError, 'already closed'):  # Unavailable custody must not become an empty or ambient result.
                operation()  # Exercise the actual closed-job guard.
        self.assertEqual(self.api.QueryInformationJobObject.call_count, count)  # No NULL-handle native query may occur.
if __name__ == '__main__':  # Support direct invocation as well as unittest discovery.
    unittest.main()  # Report only portable regression results.
