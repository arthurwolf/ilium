"""Windows process custody for installed acceptance; importable without Windows."""  # No native API loads at import time.
from __future__ import annotations  # Preserve portable forward annotations.
import ctypes as ct  # Bind documented Win32 APIs without additional dependencies.
import json  # Retain independently readable process evidence.
import os  # Resolve absolute native paths and standard stream handles.
from pathlib import Path  # Keep caller-owned evidence paths explicit.
import re  # Restrict evidence names to single safe components.
import subprocess  # Use Windows argv quoting without invoking a shell.
import sys  # Surface cleanup failures without replacing a primary exception.
import time  # Bound waits with a monotonic clock.
from typing import Any  # Contain dynamically bound ctypes functions at the native boundary.
dword, word, handle, pointer = ct.c_uint32, ct.c_uint16, ct.c_void_p, ct.c_void_p  # Win32 widths must not depend on Unix C long.
class startup_info(ct.Structure):  # Match STARTUPINFOW including pointer-sized fields.
    _fields_ = [('cb', dword), ('reserved', ct.c_wchar_p), ('desktop', ct.c_wchar_p), ('title', ct.c_wchar_p), ('x', dword), ('y', dword), ('x_size', dword), ('y_size', dword), ('x_chars', dword), ('y_chars', dword), ('fill', dword), ('flags', dword), ('show', word), ('reserved_count', word), ('reserved_bytes', pointer), ('stdin', handle), ('stdout', handle), ('stderr', handle)]  # Official ABI order; snake_case field aliases do not alter layout.
class startup_info_ex(ct.Structure):  # Carry an explicit standard-handle inheritance allowlist.
    _fields_ = [('startup', startup_info), ('attributes', pointer)]  # Match STARTUPINFOEXW.
class process_information(ct.Structure):  # Retain the exact created process and primary thread.
    _fields_ = [('process', handle), ('thread', handle), ('pid', dword), ('tid', dword)]  # Match PROCESS_INFORMATION.
class basic_limits(ct.Structure):  # Match JOBOBJECT_BASIC_LIMIT_INFORMATION.
    _fields_ = [('process_time', ct.c_int64), ('job_time', ct.c_int64), ('flags', dword), ('minimum_set', ct.c_size_t), ('maximum_set', ct.c_size_t), ('active_limit', dword), ('affinity', ct.c_size_t), ('priority', dword), ('scheduling', dword)]  # Native alignment preserves both 32-bit and 64-bit layouts.
class extended_limits(ct.Structure):  # Add IO_COUNTERS and pointer-sized memory limits.
    _fields_ = [('basic', basic_limits), ('io', ct.c_uint64 * 6), ('process_memory', ct.c_size_t), ('job_memory', ct.c_size_t), ('peak_process_memory', ct.c_size_t), ('peak_job_memory', ct.c_size_t)]  # Match JOBOBJECT_EXTENDED_LIMIT_INFORMATION.
class accounting(ct.Structure):  # Obtain authoritative current job membership counts.
    _fields_ = [('times', ct.c_int64 * 4), ('faults', dword), ('total', dword), ('active', dword), ('terminated', dword)]  # Match JOBOBJECT_BASIC_ACCOUNTING_INFORMATION.
class owned_process:  # Keep identity attached to a held kernel handle, never merely a PID.
    def __init__(self, job: windows_job, process_handle: int, pid: int):  # Only the job creates owned process observations.
        self.job, self.handle = job, process_handle  # Keep the exact process object alive until explicitly released.
        self.identity = job._identity(process_handle, pid)  # Read image and creation time through that same handle.
    def alive(self) -> bool:  # A live original process is the inverse of a zero-duration wait.
        return not self.wait(0)  # PID reuse cannot affect a retained process handle.
    def wait(self, seconds: float) -> bool:  # Return true only when this original process has exited.
        if not self.handle or not 0 <= seconds <= 1800:  # Reject closed handles and accidental unbounded waits.
            raise ValueError('closed process handle or invalid wait bound')  # Never infer process state from an invalid query.
        result = self.job.api.WaitForSingleObject(self.handle, int(seconds * 1000))  # Wait on the original kernel object.
        if result == 0xFFFFFFFF:  # WAIT_FAILED requires the authoritative Win32 error.
            raise ct.WinError(ct.get_last_error())  # Preserve the native failure immediately.
        if result not in (0, 258):  # Process waits cannot legitimately return another wait state.
            raise OSError('unexpected process wait result: ' + str(result))  # Fail closed on unrecognized results.
        return result == 0  # WAIT_OBJECT_0 proves that the held process exited.
    def close(self) -> None:  # Repeated cleanup is harmless and never signals a process.
        if not self.handle:  # A previously closed observation needs no work.
            return  # Preserve idempotent caller cleanup.
        self.job._check(self.job.api.CloseHandle(self.handle))  # Release only this owned observation handle.
        self.handle = 0  # Prevent reuse of a stale handle value.
class windows_job:  # A private, unnamed, non-inheritable Windows process container.
    def __init__(self, log: Path, label: str):  # Create a fresh native job and exclusive evidence file.
        if os.name != 'nt':  # Import remains portable; executing native custody never becomes a skip.
            raise OSError('Windows job execution requires native Windows')  # The caller must report a blocked native gate.
        if not re.fullmatch(r'[A-Za-z0-9_-]+', label) or not log.is_dir() or log.is_symlink():  # Refuse unsafe evidence destinations.
            raise ValueError('job evidence requires a plain directory and safe label')  # Never overwrite unrelated paths.
        self.log, self.label, self.index, self.handle = log, label, 0, 0  # Keep all state local to this custody instance.
        self.started = False  # Distinguish resumed execution from an attempted or suspended-only launch.
        self.held: list[owned_process] = []  # Retain every observation until explicit release or final cleanup.
        self.api: Any = ct.WinDLL('kernel32', use_last_error=True)  # Load Windows only after the native precondition.
        signatures = {  # Explicit prototypes prevent 64-bit handle truncation.
            'CreateJobObjectW': (handle, [pointer, ct.c_wchar_p]), 'SetInformationJobObject': (ct.c_int32, [handle, ct.c_int32, pointer, dword]),  # Job creation and limit binding.
            'AssignProcessToJobObject': (ct.c_int32, [handle, handle]), 'TerminateJobObject': (ct.c_int32, [handle, dword]),  # Membership and private-job termination.
            'QueryInformationJobObject': (ct.c_int32, [handle, ct.c_int32, pointer, dword, pointer]), 'IsProcessInJob': (ct.c_int32, [handle, handle, pointer]),  # Read membership through held handles.
            'CreateProcessW': (ct.c_int32, [ct.c_wchar_p, ct.c_wchar_p, pointer, pointer, ct.c_int32, dword, pointer, ct.c_wchar_p, pointer, pointer]),  # Preserve process/thread custody from creation.
            'ResumeThread': (dword, [handle]), 'TerminateProcess': (ct.c_int32, [handle, dword]), 'WaitForSingleObject': (dword, [handle, dword]),  # Only created or membership-verified handles are used.
            'CloseHandle': (ct.c_int32, [handle]), 'GetExitCodeProcess': (ct.c_int32, [handle, pointer]), 'OpenProcess': (handle, [dword, ct.c_int32, dword]),  # Exact handle lifetime and readback.
            'QueryFullProcessImageNameW': (ct.c_int32, [handle, dword, ct.c_wchar_p, pointer]), 'GetProcessTimes': (ct.c_int32, [handle, pointer, pointer, pointer, pointer]),  # Native image and creation identity.
            'InitializeProcThreadAttributeList': (ct.c_int32, [pointer, dword, dword, pointer]), 'UpdateProcThreadAttribute': (ct.c_int32, [pointer, dword, ct.c_size_t, pointer, ct.c_size_t, pointer, pointer]),  # Restrict inherited handles to retained streams.
            'DeleteProcThreadAttributeList': (None, [pointer]),  # Release the initialized attribute list without freeing its Python buffer.
        }  # Every pointer return and fixed-width argument is declared above.
        for name, (result_type, argument_types) in signatures.items():  # Configure all APIs before any native object is created.
            function = getattr(self.api, name)  # Win32 export names are required API spellings.
            function.restype, function.argtypes = result_type, argument_types  # Avoid ctypes defaults at the native boundary.
        self.events = (log / (label + '.job.jsonl')).open('x', encoding='utf-8')  # Exclusive creation preserves earlier evidence.
        try:  # Constructor failures must not leak an untracked job or evidence handle.
            self.handle = self.api.CreateJobObjectW(None, None)  # Unnamed and non-inheritable prevents unrelated attachment.
            self._check(self.handle)  # Refuse an unavailable native custody primitive.
            limits = extended_limits()  # Zero initialization explicitly denies both breakaway permissions.
            limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE preserves custody if the harness exits.
            self._check(self.api.SetInformationJobObject(self.handle, 9, ct.byref(limits), ct.sizeof(limits)))  # Set JobObjectExtendedLimitInformation before creating children.
            self._emit('job-created', breakaway=False, kill_on_close=True)  # Record the selected custody policy.
        except BaseException as primary:  # Retain cleanup without replacing an interrupted or failed construction.
            errors: list[str] = []  # Constructor cleanup failures remain secondary to the original error.
            if self.handle:  # Only a successfully created private handle can be closed.
                try:  # A failed job close must not suppress the constructor failure.
                    self._check(self.api.CloseHandle(self.handle))  # No process has yet been admitted to this job.
                except Exception as error:  # Still attempt the independent evidence close.
                    errors.append('job close: ' + str(error))  # Retain native cleanup uncertainty as a secondary diagnostic.
            try:  # Evidence cleanup is independent of the private job handle.
                self.events.close()  # Preserve the partial diagnostic file on disk.
            except Exception as error:  # Keep evidence errors subordinate to the constructor failure.
                errors.append('evidence close: ' + str(error))  # Preserve all independently observed cleanup failures.
            if errors:  # Attach failures only when cleanup actually failed.
                message = 'constructor cleanup: ' + '; '.join(errors)  # Identify the secondary failure stage clearly.
                if hasattr(primary, 'add_note'):  # Modern Python preserves notes on the original exception object.
                    primary.add_note(message)  # Keep the constructor's original type, traceback and cause.
                else:  # Older Python still needs a best-effort secondary diagnostic.
                    try:  # Even a broken diagnostic stream must not replace the original exception.
                        sys.stderr.write(message + '\n')  # Surface the cleanup failure when exception notes are unavailable.
                    except Exception:  # Preserve the primary error if diagnostics themselves are unavailable.
                        pass  # Re-raising below retains the original constructor failure.
            raise  # Keep the original constructor failure.
    def _check(self, result: Any) -> None:  # Convert a failed BOOL/HANDLE result without clobbering GetLastError.
        if not result:  # Successful nonzero native results need no branch nesting.
            raise ct.WinError(ct.get_last_error())  # Capture the native cause at its boundary.
    def _emit(self, kind: str, **values: Any) -> None:  # Keep truthful records even if later acceptance fails.
        self.events.write(json.dumps({'type': kind, 'job': self.label, **values}, sort_keys=True) + '\n')  # One complete JSON record per line.
        self.events.flush()  # Flush before a subsequent native operation can fail.
    def _identity(self, process_handle: int, pid: int) -> dict[str, Any]:  # Associate every readback with the same process object.
        image, length = ct.create_unicode_buffer(32768), dword(32768)  # Accommodate the documented Windows path bound.
        self._check(self.api.QueryFullProcessImageNameW(process_handle, 0, image, ct.byref(length)))  # Request the full Win32 image path.
        created, ended, kernel, user = (ct.c_uint64() for _ in range(4))  # FILETIME is an eight-byte little-endian tick count.
        self._check(self.api.GetProcessTimes(process_handle, ct.byref(created), ct.byref(ended), ct.byref(kernel), ct.byref(user)))  # Retain creation identity independently of PID.
        return {'pid': pid, 'image': image.value, 'created': created.value}  # Creation uses 100-nanosecond ticks since the Windows epoch.
    def active_count(self) -> int:  # Obtain active membership without reopening or signaling any process.
        if not self.handle:  # Closed custody cannot provide an authoritative readback.
            raise ValueError('job already closed')  # Never report a synthetic zero for an unavailable job.
        value = accounting()  # The kernel fills the exact documented accounting layout.
        self._check(self.api.QueryInformationJobObject(self.handle, 1, ct.byref(value), ct.sizeof(value), None))  # JobObjectBasicAccountingInformation.
        return value.active  # Caller success requires this count to reach zero naturally.
    def query(self) -> list[owned_process]:  # Return newly held observations limited to this private job.
        if not self.handle:  # A NULL job handle would query the caller's ambient job instead.
            raise ValueError('job already closed')  # Never admit processes from a parent or unrelated job.
        capacity = 64  # Bound process enumeration and grow only on an authoritative incomplete readback.
        while capacity <= 4096:  # Unbounded process creation must fail the acceptance gate.
            buffer = ct.create_string_buffer(8 + ct.sizeof(ct.c_size_t) * capacity)  # Two DWORD counts precede ULONG_PTR process IDs.
            result = self.api.QueryInformationJobObject(self.handle, 3, buffer, ct.sizeof(buffer), None)  # JobObjectBasicProcessIdList.
            error = ct.get_last_error() if not result else 0  # Save the native failure before interpreting the buffer.
            assigned, count = dword.from_buffer(buffer, 0).value, dword.from_buffer(buffer, 4).value  # Distinguish requested and returned membership.
            if result and assigned == count and count <= capacity:  # Only a complete kernel snapshot is admissible.
                break  # Use this bounded snapshot for handle acquisition.
            if error not in (0, 234):  # ERROR_MORE_DATA is the only documented grow-and-retry condition.
                raise ct.WinError(error)  # Preserve other native failures as gate failures.
            capacity *= 2  # Retry without trusting a truncated PID list.
        else:  # More than the bounded process limit is an acceptance failure.
            raise OSError('private job process inventory exceeded 4096 entries')  # Never broaden inspection to a host-wide process list.
        observations: list[owned_process] = []  # Partial acquisition must be unwound on any failure.
        try:  # Each raw PID is untrusted until its newly opened handle proves membership.
            for pid in (ct.c_size_t * count).from_buffer(buffer, 8):  # Iterate only the kernel-reported job list.
                process_handle = self.api.OpenProcess(0x101000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE grants no signal right.
                if not process_handle:  # A process may have exited between enumeration and handle acquisition.
                    error = ct.get_last_error()  # Save the reason before another native call.
                    if error == 87:  # ERROR_INVALID_PARAMETER means the enumerated process no longer exists.
                        continue  # No native process is signaled or accepted from this race.
                    raise ct.WinError(error)  # Access or other failures remain unverified rather than skipped.
                try:  # Release the handle even when identity extraction fails.
                    member = ct.c_int32()  # Win32 BOOL has a fixed four-byte representation.
                    self._check(self.api.IsProcessInJob(process_handle, self.handle, ct.byref(member)))  # Fence PID reuse before reading identity.
                    if not member.value:  # A reused PID can identify an unrelated process but cannot enter custody.
                        continue  # Close the observation without signaling the foreign process.
                    observed = owned_process(self, process_handle, pid)  # Preserve image, creation time and the original handle together.
                    observations.append(observed)  # Transfer the open handle into an owned observation.
                    process_handle = 0  # The local finally no longer owns this transferred handle.
                finally:  # Every non-transferred handle must be closed.
                    if process_handle:  # A transferred handle remains held for the caller.
                        primary = sys.exc_info()[1]  # Preserve an identity-read failure during handle cleanup.
                        try:  # Closing a read-only handle never signals the process.
                            self._check(self.api.CloseHandle(process_handle))  # Release this observation only.
                        except Exception as close_error:  # Surface secondary close failures too.
                            if primary is None:  # A standalone close failure is the primary error.
                                raise  # Preserve the native close traceback.
                            primary.add_note('observation close: ' + str(close_error))  # Keep the original identity failure.
            self.held.extend(observations)  # Final cleanup also owns observations the caller forgets to close.
            self._emit('job-processes', processes=[item.identity for item in observations])  # Retain handle-bound process identities.
            return observations  # The caller may hold an original server across subsequent CLI calls.
        except BaseException as primary:  # Failed inventories retain their original error through cleanup.
            for observed in observations:  # Release only handles already proven to belong to the private job.
                try:  # Continue through all acquired observations.
                    observed.close()  # No process is signaled while unwinding discovery.
                except Exception as close_error:  # Cleanup must not replace the inventory error.
                    primary.add_note('inventory close: ' + str(close_error))  # Retain each unavailable close readback.
            raise  # Preserve the discovery failure as the primary cause.
    def hold_image(self, expected: Path) -> owned_process:  # Require one live owned process at the exact expected image path.
        if not expected.is_absolute():  # Relative paths cannot establish installed executable provenance.
            raise ValueError('expected image must be absolute')  # Refuse implicit current-directory lookup.
        wanted = os.path.normcase(str(expected.resolve(strict=True)))  # Native filesystem resolution normalizes installed absolute paths.
        observations = self.query()  # Every candidate carries a membership-verified retained handle.
        matches = [item for item in observations if item.alive() and os.path.normcase(str(Path(item.identity['image']).resolve(strict=True))) == wanted]  # Readiness cannot be satisfied by a source binary or a terminated process.
        for item in observations:  # Keep only a unique accepted image observation.
            if len(matches) != 1 or item is not matches[0]:  # Ambiguity must not choose an arbitrary server.
                item.close()  # Release unmatched or ambiguous handles without signaling them.
        if len(matches) != 1:  # A missing, exited or duplicate server fails custody establishment.
            raise OSError('expected exactly one live job-owned image: ' + str(expected))  # Never fall back to an account-wide PID/name search.
        return matches[0]  # This same original handle must later prove server retirement.
    def run(self, command: list[str], environment: dict[str, str], cwd: Path, timeout: int = 60) -> dict[str, Any]:  # Launch a bounded child only after its job custody is established.
        self.started = False  # Validation, creation and suspended-assignment failures cannot authorize account cleanup.
        if not self.handle or not command or not Path(command[0]).is_absolute() or not cwd.is_absolute() or not 1 <= timeout <= 1800:  # Enforce live custody, explicit installed/tool paths and finite waits.
            raise ValueError('job command requires absolute image/cwd and a bounded timeout')  # PATH lookup cannot satisfy installed-byte acceptance.
        if any('\0' in value for value in command) or any(not key or '=' in key or '\0' in key + value for key, value in environment.items()):  # Reject malformed command/environment block boundaries.
            raise ValueError('invalid native command or environment field')  # Prevent truncation or duplicate native environment interpretation.
        if len({key.upper() for key in environment}) != len(environment):  # Windows variable names are case insensitive.
            raise ValueError('duplicate native environment keys')  # Refuse ambiguous PATH or application configuration selection.
        self.index += 1  # Give every attempted child exclusive retained stream paths.
        stem = self.label + '-' + str(self.index)  # Stable sequential labels let JSONL reference its exact native logs.
        stdout_path, stderr_path = self.log / (stem + '.stdout.txt'), self.log / (stem + '.stderr.txt')  # Files avoid detached-descendant pipe lifetime hangs.
        information, attributes, initialized, admitted = process_information(), None, False, False  # Track exact acquisition stages for safe unwinding.
        import msvcrt  # Convert Python-owned Windows file descriptors only during native execution.
        try:  # Every acquired native object has a matching finalizer below.
            with stdout_path.open('xb') as stdout, stderr_path.open('xb') as stderr, open(os.devnull, 'rb') as stdin:  # Exclusive retained output and a noninteractive null input.
                native_handles = (handle * 3)(*(msvcrt.get_osfhandle(stream.fileno()) for stream in (stdin, stdout, stderr)))  # Limit inheritance to these owned standard streams.
                for stream in (stdin, stdout, stderr):  # The explicit handle list still requires inheritable handle attributes.
                    os.set_inheritable(stream.fileno(), True)  # These streams exist only during this serialized child launch.
                size = ct.c_size_t()  # Query the documented attribute-list allocation size.
                self.api.InitializeProcThreadAttributeList(None, 1, 0, ct.byref(size))  # The sizing call intentionally returns insufficient-buffer.
                if not size.value:  # A failed sizing query cannot become an uninitialized native attribute list.
                    raise ct.WinError(ct.get_last_error())  # Retain the authoritative sizing failure.
                attributes = ct.create_string_buffer(size.value)  # Keep backing storage alive through CreateProcessW.
                self._check(self.api.InitializeProcThreadAttributeList(attributes, 1, 0, ct.byref(size)))  # Initialize one explicit inheritance attribute.
                initialized = True  # Only initialized native attribute lists may be deleted.
                self._check(self.api.UpdateProcThreadAttribute(attributes, 0, 0x20002, native_handles, ct.sizeof(native_handles), None, None))  # PROC_THREAD_ATTRIBUTE_HANDLE_LIST excludes every other inheritable handle.
                startup = startup_info_ex()  # Zero reserved fields before filling official STARTUPINFOEXW.
                startup.startup.cb, startup.startup.flags = ct.sizeof(startup), 0x100  # STARTF_USESTDHANDLES selects the retained files.
                startup.startup.stdin, startup.startup.stdout, startup.startup.stderr = native_handles  # Supply valid inherited stream handles.
                startup.attributes = ct.cast(attributes, pointer)  # Bind the live attribute-list storage.
                argv = ct.create_unicode_buffer(subprocess.list2cmdline(command))  # CreateProcessW may mutate this command-line buffer.
                block = ct.create_unicode_buffer('\0'.join(key + '=' + value for key, value in sorted(environment.items(), key=lambda item: item[0].upper())) + '\0')  # The buffer's implicit terminator makes the required double NUL.
                self._check(self.api.CreateProcessW(command[0], argv, None, None, True, 0x80404, block, str(cwd), ct.byref(startup), ct.byref(information)))  # CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT.
                self._check(self.api.AssignProcessToJobObject(self.handle, information.process))  # No user code or descendant can run before assignment.
                admitted = True  # From this point failure cleanup belongs exclusively to the private job.
                identity = self._identity(information.process, information.pid)  # Record the actual created image before resuming its thread.
                self._emit('process-created', command=command, identity=identity, stdout=stdout_path.name, stderr=stderr_path.name)  # Persist custody and evidence references before execution.
                if self.api.ResumeThread(information.thread) == 0xFFFFFFFF:  # Resume only the exact newly created primary thread.
                    raise ct.WinError(ct.get_last_error())  # A failed resume remains a native gate failure.
                self.started = True  # The caller may now treat installer account mutation as potentially underway.
                result = self.api.WaitForSingleObject(information.process, timeout * 1000)  # Wait on the original child handle, never a cached PID.
                if result == 258:  # WAIT_TIMEOUT must never become a successful command result.
                    timeout_error = TimeoutError('owned child exceeded ' + str(timeout) + ' seconds: ' + command[0])  # Preserve the actual native timeout.
                    try:  # Logging cannot replace the timeout that already occurred.
                        self._emit('process-timeout', identity=identity, timeout=timeout)  # Leave the child under private-job custody.
                    except Exception as logging_error:  # Retain unavailable timeout evidence too.
                        timeout_error.add_note('timeout evidence: ' + str(logging_error))  # Surface the secondary logging failure.
                    raise timeout_error  # Preserve bounded command failure.
                if result != 0:  # WAIT_FAILED or another unexpected result is not an exit observation.
                    raise ct.WinError(ct.get_last_error()) if result == 0xFFFFFFFF else OSError('unexpected child wait result')  # Keep the authoritative cause.
                exit_code = dword()  # Read the original child's real exit status.
                self._check(self.api.GetExitCodeProcess(information.process, ct.byref(exit_code)))  # A signaled handle is required before interpreting STILL_ACTIVE values.
            with stdout_path.open('rb') as stdout, stderr_path.open('rb') as stderr:  # Detached descendants may continue writing after the command exits.
                output_bytes, error_bytes = stdout.read(8_000_001), stderr.read(8_000_001)  # Bound reads themselves rather than relying on racing file sizes.
            if len(output_bytes) > 8_000_000 or len(error_bytes) > 8_000_000:  # Retain oversized logs but bound Python memory consumption.
                raise OSError('native output exceeds the retained-readback bound')  # Oversized output cannot silently omit version/error evidence.
            readback = {'returncode': exit_code.value, 'stdout': output_bytes.decode('utf-8', errors='replace'), 'stderr': error_bytes.decode('utf-8', errors='replace'), 'process': identity, 'stdout_file': stdout_path.name, 'stderr_file': stderr_path.name}  # Preserve raw streams and actual process identity.
            self._emit('process-exit', identity=identity, returncode=exit_code.value, stdout=stdout_path.name, stderr=stderr_path.name)  # Zero/nonzero interpretation remains the caller's contract.
            return readback  # Caller verifies exact versions, lifecycle, and allowed installer exit codes.
        finally:  # Retire only acquired native resources and never broaden process ownership.
            primary = sys.exc_info()[1]  # Attach secondary cleanup failures without replacing this original exception.
            errors: list[str] = []  # Attempt every independent cleanup despite earlier failures.
            if information.process and not admitted:  # Assignment failure leaves a suspended process created exclusively by this harness.
                try:  # This direct termination uses the creation handle, not a PID lookup.
                    self._check(self.api.TerminateProcess(information.process, 1))  # Safely retire the never-resumed, unadmitted child.
                    if self.api.WaitForSingleObject(information.process, 10000) != 0:  # Bound retirement of the exact failed creation.
                        raise OSError('suspended child retirement could not be verified')  # Preserve uncertainty instead of implying cleanup success.
                except OSError as error:  # Other handles still need release after a termination failure.
                    errors.append(str(error))  # Surface every unverified cleanup operation.
            for native_handle in (information.thread, information.process):  # Both PROCESS_INFORMATION handles belong to this launch.
                if native_handle:  # Failed CreateProcessW leaves these fields null.
                    if not self.api.CloseHandle(native_handle):  # Closing never targets or signals a different process.
                        errors.append(str(ct.WinError(ct.get_last_error())))  # Preserve independent handle-release failures.
            if initialized:  # Delete only a successfully initialized attribute list.
                self.api.DeleteProcThreadAttributeList(attributes)  # Python later releases the backing buffer.
            if errors:  # A successful primary operation still fails when resource custody is uncertain.
                message = 'child cleanup: ' + '; '.join(errors)  # Keep one clear secondary diagnostic.
                try:  # Preserve secondary cleanup evidence independently of exception formatting.
                    self._emit('child-cleanup-failure', errors=errors)  # The caller's JSONL inventory will retain these failures.
                except Exception as evidence_error:  # An evidence failure cannot erase earlier cleanup details.
                    message += '; cleanup evidence: ' + str(evidence_error)  # Preserve both failures in the raised or attached diagnostic.
                if primary is None:  # There is no primary error to preserve.
                    raise OSError(message)  # Fail the caller's gate on cleanup failure.
                primary.add_note(message) if hasattr(primary, 'add_note') else sys.stderr.write(message + '\n')  # Keep the original exception object and expose cleanup failures.
    def terminate_and_close(self) -> None:  # Finish only this private job, with a bounded authoritative empty readback.
        if not self.handle:  # Explicit cleanup and context-manager cleanup may both run.
            return  # Idempotence never fabricates a fresh native observation.
        errors: list[str] = []  # Cleanup proceeds independently through all owned resources.
        try:  # Termination stays confined to the private non-inheritable job.
            count = self.active_count()  # Read back whether forced cleanup is actually necessary.
            self._emit('job-cleanup', active_before=count, forced=count != 0)  # Forced retirement is visible and must not certify graceful lifecycle.
            if count:  # An empty job needs no signal.
                self._check(self.api.TerminateJobObject(self.handle, 1))  # No global PID, process name, account, or service is signaled.
        except Exception as error:  # A failed membership read still leaves kill-on-close as the scoped fallback.
            errors.append(str(error))  # Keep the native cleanup uncertainty.
        for observed in self.held:  # Release observations before waiting for all native references to retire.
            try:  # A single failed CloseHandle must not block the remaining releases.
                observed.close()  # Caller-held process objects become explicitly closed.
            except Exception as error:  # Retain independent handle-close errors.
                errors.append(str(error))  # Continue cleaning the rest of this private job.
        try:  # Query actual active membership; ordinary job emptiness is not a waitable job signal.
            deadline = time.monotonic() + 15  # Keep final native cleanup finite.
            while self.active_count():  # Poll only this job's authoritative accounting state.
                if time.monotonic() >= deadline:  # No timeout can become a clean readback.
                    raise TimeoutError('private job did not become empty within 15 seconds')  # Final close still attempts scoped kill-on-close below.
                time.sleep(0.05)  # Avoid a busy loop during native process retirement.
            self._emit('job-empty', active=0)  # This record is emitted only after authoritative zero membership.
        except Exception as error:  # Preserve emptiness uncertainty while continuing handle release.
            errors.append(str(error))  # A later close cannot rewrite this failure as success.
        if not self.api.CloseHandle(self.handle):  # Last-handle close is a job-scoped kill-on-close fallback.
            errors.append(str(ct.WinError(ct.get_last_error())))  # Expose failure to release the final custody handle.
        self.handle = 0  # Prevent accidental reuse of a stale native job handle.
        try:  # Retain the full cleanup outcome even after primary acceptance fails.
            self._emit('job-closed', cleanup_errors=errors)  # This record does not assert emptiness when earlier readback failed.
        except Exception as error:  # Evidence failure cannot suppress independent native cleanup errors.
            errors.append('cleanup evidence: ' + str(error))  # Preserve the failure alongside native readback uncertainty.
        try:  # Flush and close the exclusive evidence stream on every path.
            self.events.close()  # The caller retains the complete JSONL file.
        except Exception as error:  # Closing evidence is independently necessary.
            errors.append('evidence close: ' + str(error))  # Do not replace the earlier native cleanup failures.
        if errors:  # Successful termination does not erase an earlier cleanup/readback defect.
            raise OSError('private job cleanup failed: ' + '; '.join(errors))  # Caller combines this with any primary acceptance error.
