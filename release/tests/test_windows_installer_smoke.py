"""Portable fault injection for the audited Windows gate; no native acceptance claims."""  # Run on every source runner.
import copy  # Keep independent readbacks from sharing mutable fixtures.
import itertools  # Advance deadlines without sleeping.
import json  # Build real schema-1 input receipts.
import os  # Verify child isolation does not change the parent environment.
from pathlib import Path  # Use each test host's real temporary filesystem.
import sys  # Import the exact proposed scripts as the existing tests do.
import tempfile  # Own every synthetic payload and diagnostic directory.
from types import SimpleNamespace  # Supply explicit portable native boundaries.
import unittest  # Join the existing five-runner source suite.
from unittest.mock import Mock, patch  # Inject failures without loading native APIs.
import yaml  # Reuse the workflow's already pinned PyYAML dependency.
root = Path(__file__).resolve().parents[2]  # Resolve the checkout containing this test.
sys.path.insert(0, str(root / 'release/scripts'))  # Use sibling production modules.
import build_windows_installers as package  # Exercise the recovered builder parser.
import release_tool  # Exercise real archive and audit validators.
import smoke_windows_installers as smoke  # Native adapters remain unloaded on import.
#
class memory_evidence:  # Retain structured observations independently of console output.
    def __init__(self, directory):  # Keep each test's evidence beneath its temporary root.
        self.root, self.rows = directory, []  # Match only the journal's public boundary.
    def record(self, kind, **values):  # Preserve actual observation values.
        self.rows.append(copy.deepcopy({'type': kind, **values}))  # Later fixture changes cannot rewrite history.
#
class held_process:  # Model one original handle, not a reopenable PID.
    def __init__(self, name, events, exits=True):  # Distinguish server and pane readbacks.
        self.name, self.events, self.exits = name, events, exits  # Keep retirement behavior explicit.
        self.identity, self.closed = {'pid': 17, 'created': 123, 'image': name}, False  # PID equality deliberately proves nothing.
    def alive(self):  # The lifecycle requires live handles before shutdown.
        return not self.closed  # This fixture stays live until the supported shutdown stage.
    def wait(self, seconds):  # Record which original handle is waited on and its bound.
        self.events.append(('wait', self.name, seconds))  # A bare PID query cannot satisfy this observation.
        return self.exits  # Inject failure to retire the original server.
    def close(self):  # Repeated handle release is harmless.
        self.closed = True  # Closing an observation does not launch or signal anything.
#
class command_job:  # Execute the real probe control flow with synthetic process readbacks.
    def __init__(self, fault=''):  # One named defect per lifecycle.
        self.fault, self.events, self.commands, self.running = fault, [], [], False  # Keep command order and liveness separate.
        self.server = held_process('ilium-server.exe', self.events, fault != 'retirement')  # Retain the first server identity.
        self.pane = held_process('cmd.exe', self.events)  # Retain the pane shell independently.
    def run(self, command, environment, cwd, timeout):  # No native executable is invoked by this fixture.
        self.commands.append((list(command), dict(environment), cwd, timeout))  # Retain exact executable resolution and bounds.
        output, error, code = '', '', 0  # Successful command defaults are adjusted by the named defect.
        if command[-1] == '--version':  # Versions are returned by both installed executable paths.
            name = Path(command[0]).name.removesuffix('.exe')  # Label the actual selected executable.
            version = '10.1.0' if self.fault == name + '-version' else '0.1.0'
            output = (release_tool.helper_version_record(version) if name == 'ilium-animation-helper'
                      else name + ' ' + version) + '\n'  # Include the helper's exact JSONL contract.
            error = 'diagnostic' if self.fault == 'version-stderr' else ''  # Unexpected diagnostics must fail.
        elif command[-1] == 'release-animation-probe':
            directory = Path(command[0]).parent
            helper_hash = package.sha(directory / 'ilium-animation-helper.exe')
            rows = [
                {'type': 'artifact', 'gate': 'installed_catalogue', 'packages': ['beach', 'carpet'],
                 'client_path': str(directory / 'ilium.exe'),
                 'client_sha256': package.sha(directory / 'ilium.exe'),
                 'helper_path': str(directory / 'ilium-animation-helper.exe'),
                 'helper_sha256': helper_hash, 'worker_threads_before': 1,
                 'worker_bytes_before': 1024},
                *({'type': 'artifact', 'gate': 'installed_render', 'package': name,
                   'archive_sha256': digest, 'helper_sha256': helper_hash,
                   'rendered_frames': 2, 'physical_retirement': True,
                   'worker_threads_before': 1, 'worker_threads_after': 1,
                   'worker_bytes_before': 1024, 'worker_bytes_after': 1024}
                  for filename, digest in release_tool.APPROVED_PACKAGES.items()
                  for name in [filename.split('-')[0]]),
                {'type': 'result', 'gate': 'installed_animation', 'state': 'passed',
                 'publication_allowed': False, 'packages': ['beach', 'carpet']},
            ]
            if self.fault == 'animation-missing-carpet':
                rows.pop(2)
            if self.fault == 'animation-helper-hash':
                rows[0]['helper_sha256'] = '0' * 64
            output = ''.join(json.dumps(row) + '\n' for row in rows)
        elif 'new-pane' in command:  # Create the marker through the captured supported shell command.
            self.running = True  # The original server and shell now belong to the private job.
            nonce = command[-1].removeprefix('echo ').split('>', 1)[0]  # Interpret only this fixture's emitted marker command.
            (cwd / 'pane-marker.txt').write_text('wrong' if self.fault == 'marker' else nonce, encoding='ascii')  # Materialize real readiness evidence.
            code = 9 if self.fault == 'new-pane' else 0  # Fail after partial startup to exercise cleanup.
        elif command[-1] == 'ls':  # A successful listing must be nonempty.
            output = '' if self.fault == 'listing' else 'default pane\n'  # Do not invent an undocumented listing schema.
        elif 'kill-session' in command:  # Model only the supported graceful retirement request.
            self.running = self.fault == 'descendants'  # Residual owned descendants block success.
            if self.fault == 'changed-payload':  # The post-lifecycle inventory must catch a late mutation.
                Path(command[0]).write_bytes(b'changed during lifecycle')  # Mutate only the test-owned installed client.
        return {'returncode': code, 'stdout': output, 'stderr': error, 'process': {'fixture': True}}  # Never label synthetic readbacks native.
    def active_count(self):  # Expose a real condition to the production bounded polling loop.
        return int(self.running)  # Nonzero remains nonzero until graceful shutdown succeeds.
    def hold_image(self, image):  # Hand back the same original observation on each request.
        return self.server if image.name == 'ilium-server.exe' else self.pane  # Native membership is tested in test_windows_job.
    def terminate_and_close(self):  # Model task-private failure cleanup only.
        self.events.append(('cleanup',))  # Prove the finalizer was reached.
        if self.fault == 'cleanup':  # Independent cleanup failure must block acceptance.
            raise OSError('job retirement failed')  # Do not convert unavailable retirement into empty membership.
#
class smoke_tests(unittest.TestCase):  # Test source contracts on every native runner.
    def setUp(self):  # Isolate files and clocks for every scenario.
        temporary = tempfile.TemporaryDirectory()  # Never share package state between cases.
        self.addCleanup(temporary.cleanup)  # Remove only this test's private fixture root.
        self.directory = Path(temporary.name).resolve()  # macOS /var may be a symlink; resolve the fixture's parent first.
        self.installed, self.system = self.directory / 'installed', self.directory / 'system'  # Distinct payload and shell roots.
        self.installed.mkdir()  # Inventory checks inspect real ordinary files.
        self.system.mkdir()  # A fake job will inspect command paths without executing them.
        self.content = {'ilium.exe': b'client', 'ilium-server.exe': b'server',
                        'ilium-animation-helper.exe': b'helper', 'onnxruntime.dll': b'runtime',
                        'VERSION': b'0.1.0\n', 'THIRD-PARTY.txt': b'reviewed notices'}  # Synthetic executables are explicitly non-native.
        for filename in release_tool.APPROVED_PACKAGES:
            self.content[filename] = (root / 'ilium-animation-js/assets/packages' / filename).read_bytes()
        self.files = {name: release_tool.digest(data) for name, data in self.content.items()}  # Compute real expected byte hashes.
        for name, data in self.content.items():  # Create a complete test-owned installed payload.
            (self.installed / name).write_bytes(data)  # No source executable can satisfy the test inventory.
        self.evidence = memory_evidence(self.directory)  # Retain every phase observation.
        self.binding = {'version': '0.1.0', 'files': self.files}  # Probe only consumes the audited payload subset.
        ticks = itertools.count(0, 61)  # Any false condition crosses the longest polling bound on its next read.
        self.clock = SimpleNamespace(monotonic=lambda: next(ticks), sleep=Mock())  # Avoid real waits without replacing the condition.
        self.clock_patch = patch.object(smoke, 'time', self.clock)  # Do not alter the interpreter's shared time module.
        self.clock_patch.start()  # All production wait loops still execute their actual branches.
        self.addCleanup(self.clock_patch.stop)  # Restore the module after the case.
    def inputs(self):  # Build a normalized archive and fully bound synthetic receipts.
        installers = self.directory / 'installers'  # Keep evidence outside the three-entry installer inventory.
        installers.mkdir()  # Each input fixture is fresh.
        manifest = root / 'release/targets.toml'  # Use the unchanged exact five-target manifest.
        target = release_tool.selected_target(manifest, 'x86_64-pc-windows-msvc')  # Exercise real target policy.
        archive = self.directory / target['archive']  # Preserve the required published basename.
        release_tool.write_archive(archive, target, self.content)  # Use real deterministic ZIP serialization.
        audit = {'schema': 1, 'state': 'passed', 'publication_allowed': True, 'target': target['rust_target'], 'tag': 'v0.1.0', 'version': '0.1.0', 'os': 'windows', 'arch': 'x86_64', 'files': self.files, 'native_identity': {'system': 'Windows', 'machine': 'AMD64', 'runner': 'synthetic-only'}, 'dependency_closure': {'complete': True, 'bundled': ['onnxruntime.dll']}, 'binary_versions': {'ilium.exe': 'ilium 0.1.0', 'ilium-server.exe': 'ilium-server 0.1.0', 'ilium-animation-helper.exe': 'ilium-animation-helper 0.1.0'}, 'notices': {'state': 'reviewed', 'sha256': self.files['THIRD-PARTY.txt']}, 'windows_ort': {'state': 'passed', 'source_tag': 'v1.24.2', 'source_commit': '058787ceead760166e3c50a0a4cba8a833a6f53f', 'source_sha256': 'a' * 64, 'rust_crt': 'static', 'ort_crt': 'static', 'built_runtime_sha256': self.files['onnxruntime.dll']}}  # This is validator input, never an actual audit result.
        audit_path = self.directory / 'native-audit.json'  # Bind the complete receipt as an explicit input.
        audit_path.write_text(json.dumps(audit), encoding='utf-8')  # Preserve every required schema field.
        for name in package.INSTALLER_NAMES:  # Build harmless synthetic installer files.
            (installers / name).write_bytes(name.encode('ascii'))  # Never execute these fixtures.
        receipt = {'schema': 1, 'tag': 'v0.1.0', 'version': '0.1.0', 'source_archive': archive.name, 'source_archive_sha256': package.sha(archive), 'package_files': self.files, 'installers': {name: package.sha(installers / name) for name in package.INSTALLER_NAMES}}  # Use the unchanged producer contract.
        (installers / package.RECEIPT_NAME).write_text(json.dumps(receipt), encoding='utf-8')  # Supply all three required installer entries.
        return SimpleNamespace(tag='v0.1.0', archive=archive, audit_report=audit_path, manifest=manifest, installers=installers)  # Match the real smoke parser.
    def test_binding_checks_real_archive_and_all_authorities(self):  # A matched synthetic chain passes source validation only.
        arguments = self.inputs()  # Exercise the real validators without mocks.
        binding = smoke.bind_inputs(arguments, self.evidence)  # No account is inspected or changed.
        self.assertEqual(binding['files'], self.files)  # The audit inventory remains the installed-byte authority.
        self.assertEqual(set(binding['paths']), {'archive', 'audit', 'manifest', 'receipt', *package.INSTALLER_NAMES})  # Every authority is retained.
        self.assertEqual(self.evidence.rows[-1]['sha256']['archive'], package.sha(arguments.archive))  # Evidence binds actual input bytes.
        for name in package.INSTALLER_NAMES:  # Reusing an old receipt after mutation must fail.
            path = arguments.installers / name  # Select the exact original installer.
            original = path.read_bytes()  # Preserve the clean fixture between subcases.
            path.write_bytes(b'tampered')  # Change one installer without updating its receipt.
            with self.subTest(name=name), self.assertRaisesRegex(release_tool.ReleaseError, 'installer bytes'):  # Require byte binding, not filenames alone.
                smoke.bind_inputs(arguments, self.evidence)  # Real digest comparison must reject it.
            path.write_bytes(original)  # Restore only this synthetic input.
        archive_bytes = arguments.archive.read_bytes()  # Preserve the normalized positive archive fixture.
        arguments.archive.write_bytes(archive_bytes + b'trailer')  # Introduce actual unaccounted archive bytes.
        with self.assertRaisesRegex(release_tool.ReleaseError, 'prefix/trailer'):  # A matching filename is insufficient archive custody.
            smoke.bind_inputs(arguments, self.evidence)  # Exercise the strict archive reader before receipt validation.
        arguments.archive.write_bytes(archive_bytes)  # Restore only the test-owned archive for independent checks.
        receipt_path = arguments.installers / package.RECEIPT_NAME  # Select the real installer binding authority.
        producer_receipt = json.loads(receipt_path.read_text(encoding='utf-8'))  # Preserve its complete valid fields.
        for field, value in (('source_archive_sha256', '0' * 64), ('package_files', dict(self.files, **{'ilium-server.exe': '0' * 64}))):  # Reject a receipt bound to another archive or payload.
            receipt_path.write_text(json.dumps(dict(producer_receipt, **{field: value})), encoding='utf-8')  # Change one custody link at a time.
            with self.subTest(field=field), self.assertRaisesRegex(release_tool.ReleaseError, 'receipt differs'):  # Require the exact audited byte chain.
                smoke.bind_inputs(arguments, self.evidence)  # Use real receipt validation without replacing its helpers.
        receipt_path.write_text(json.dumps(producer_receipt), encoding='utf-8')  # Restore a valid installer receipt for audit mutations.
        receipt = json.loads(arguments.audit_report.read_text(encoding='utf-8'))  # Retain a complete valid audit for each mutation.
        for field, value in (('state', 'blocked'), ('files', dict(self.files, **{'ilium.exe': '0' * 64})), ('windows_ort', dict(receipt['windows_ort'], built_runtime_sha256='0' * 64)), ('dependency_closure', {'complete': True, 'bundled': []})):  # Distinguish audit, archive and runtime-closure failures.
            arguments.audit_report.write_text(json.dumps(dict(receipt, **{field: value})), encoding='utf-8')  # Mutate only one authority at a time.
            with self.subTest(field=field), self.assertRaises(release_tool.ReleaseError):  # No incomplete or inconsistent authority may bind.
                smoke.bind_inputs(arguments, self.evidence)  # Use the production validators through the gate.
    def test_bad_binding_precedes_account_access_or_installer_execution(self):  # Prove fail-before-mutation through the public entry point.
        arguments = self.inputs()  # Start with a complete synthetic custody chain.
        arguments.log, arguments.disposable_account = self.directory / 'not-created', False  # The journal and native environment are explicit doubles.
        (arguments.installers / package.MSI_NAME).write_bytes(b'changed')  # Fail the real binding check.
        native_os = SimpleNamespace(name='nt', environ={'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted'})  # Avoid changing global os.name or real account state.
        with patch.object(smoke, 'journal', return_value=self.evidence), patch.object(smoke, 'os', native_os), patch.object(smoke.platform, 'machine', return_value='AMD64'), patch.object(smoke, 'windows_account') as account, patch.object(smoke, 'run_format') as execute:  # Fence every account and installer boundary.
            with self.assertRaisesRegex(release_tool.ReleaseError, 'installer bytes'):  # Invalid inputs must fail before native readback or mutation.
                smoke.smoke(arguments)  # Exercise the actual ordered preflight.
        account.assert_not_called()  # Artifact defects cannot even inspect an unrelated native account.
        execute.assert_not_called()  # No installer operation follows a failed authority chain.
        self.assertEqual(self.evidence.rows[-1]['state'], 'failed')  # Retain an explicit terminal failure.
    def test_inventory_rejects_missing_tampered_and_unexpected_payload(self):  # File existence alone cannot pass.
        self.assertEqual(smoke.inventory(self.installed, self.files, 'msi'), self.files)  # First establish the complete positive fixture.
        for name in ('ilium.exe', 'ilium-server.exe', 'ilium-animation-helper.exe',
                     *release_tool.APPROVED_PACKAGES, 'onnxruntime.dll'):  # All installed animation members have independent obligations.
            path = self.installed / name  # Address only task-owned synthetic files.
            path.write_bytes(b'tampered')  # Preserve the name but change its bytes.
            with self.subTest(name=name, defect='hash'), self.assertRaisesRegex(release_tool.ReleaseError, 'hash'):  # Require exact SHA256 acceptance.
                smoke.inventory(self.installed, self.files, 'msi')  # Detect installed-byte tampering.
            path.unlink()  # Missing server/runtime must fail too.
            with self.subTest(name=name, defect='missing'), self.assertRaisesRegex(release_tool.ReleaseError, 'inventory'):  # Missing files cannot become environmental skips.
                smoke.inventory(self.installed, self.files, 'msi')  # Verify the complete installed set.
            path.write_bytes(self.content[name])  # Restore the synthetic member for the next defect.
        extra = self.installed / 'foreign.dll'  # A similarly named unexpected runtime is not admitted.
        extra.write_bytes(b'foreign')  # Create a real extra inventory entry.
        with self.assertRaisesRegex(release_tool.ReleaseError, 'inventory'):  # Exact set equality is required.
            smoke.inventory(self.installed, self.files, 'msi')  # Reject the extra file.
        self.assertEqual(extra.read_bytes(), b'foreign')  # Validation never deletes an unrecognized entry.
    def exercise_probe(self, fault=''):  # Drive a full lifecycle through the real production probe.
        job, transaction = command_job(fault), {'uncertain': False}  # Isolate custody outcome per probe.
        with patch.object(smoke, 'windows_job', return_value=job):  # Replace only the native execution boundary.
            smoke.probe(self.installed, self.binding, 'msi', 'probe-' + (fault or 'ok'), self.evidence, self.system, transaction)  # Run real versions, marker, lifecycle and inventory checks.
        return job, transaction  # Expose captured commands for behavioral assertions.
    def test_probe_requires_both_versions_absolute_images_and_original_retirement(self):  # A full synthetic lifecycle exercises the success path.
        before = dict(os.environ)  # Preserve the parent's exact environment observation.
        job, transaction = self.exercise_probe()  # Run all production probe phases.
        self.assertEqual([Path(item[0][0]).name for item in job.commands[:3]], ['ilium.exe', 'ilium-server.exe', 'ilium-animation-helper.exe'])  # All installed images are executed independently.
        self.assertEqual([item[0][-1] for item in job.commands[:3]], ['--version'] * 3)  # Require each executable's version command.
        self.assertTrue(all(Path(item[0][0]).parent == self.installed for item in job.commands))  # Never resolve through source PATH.
        self.assertTrue(all(item[1]['PATH'] == str(self.installed) + ';' + str(self.system) for item in job.commands))  # Only installed and system directories are visible through child PATH.
        self.assertEqual(job.commands[3][0], [str(self.installed / 'ilium.exe'), 'release-animation-probe'])
        self.assertEqual(job.commands[3][3], 300)
        for command, environment, cwd, timeout in job.commands[4:]:  # Bind each lifecycle command to the supported client and one project endpoint.
            self.assertEqual(command[:3], [str(self.installed / 'ilium.exe'), '--cwd', str(cwd)])  # A server executable or different CLI project cannot pass.
        self.assertEqual({item[2] for item in job.commands}, {job.commands[0][2]})  # Every invocation uses this lifecycle's fresh project.
        self.assertEqual({item[3] for item in job.commands}, {60, 300})  # Bound the longer installed render gate separately.
        self.assertIn(str(self.system / 'cmd.exe'), job.commands[4][0])  # Use the native Windows shell contract.
        self.assertEqual(job.commands[-1][0][-2:], ['kill-session', 'default'])  # Stop only the established project session.
        self.assertEqual(job.events, [('wait', 'ilium-server.exe', 30), ('wait', 'cmd.exe', 30), ('cleanup',)])  # Wait on both original observations with explicit bounds.
        self.assertFalse(transaction['uncertain'])  # Clean retirement leaves no custody uncertainty.
        self.assertEqual(os.environ, before)  # Child isolation never changes persistent or parent environment state.
        self.assertEqual([row['state'] for row in self.evidence.rows if row['type'] == 'lifecycle'], ['passed'])  # Success follows all readbacks.
    def test_probe_faults_never_become_passes_after_cleanup(self):  # Reject output, lifecycle and retirement failures.
        for fault in ('ilium-version', 'ilium-server-version', 'version-stderr', 'animation-missing-carpet', 'animation-helper-hash', 'new-pane', 'marker', 'listing', 'retirement', 'descendants', 'changed-payload'):  # Cover the specified failure boundaries.
            job, transaction = command_job(fault), {'uncertain': False}  # Retain each failed lifecycle's private observations.
            self.evidence.rows.clear()  # Inspect only this subcase's journal.
            with self.subTest(fault=fault), patch.object(smoke, 'windows_job', return_value=job), self.assertRaises(release_tool.ReleaseError):  # Native cleanup cannot erase the failed contract.
                smoke.probe(self.installed, self.binding, 'msi', fault, self.evidence, self.system, transaction)  # Execute the full real control flow.
            self.assertIn(('cleanup',), job.events)  # Partial startup and retirement failure both reach the finalizer.
            self.assertFalse(any(row['type'] == 'lifecycle' and row['state'] == 'passed' for row in self.evidence.rows))  # Never publish a false lifecycle pass.
    def test_primary_and_job_cleanup_failures_are_both_retained(self):  # A failed inventory remains primary when retirement also fails.
        job, primary, transaction = command_job('cleanup'), ValueError('first payload failure'), {'uncertain': False}  # Track the exact exception object.
        with patch.object(smoke, 'windows_job', return_value=job), patch.object(smoke, 'inventory', side_effect=primary):  # Inject independent primary and cleanup failures.
            with self.assertRaises(ValueError) as caught:  # Preserve the primary type and identity.
                smoke.probe(self.installed, self.binding, 'msi', 'double-failure', self.evidence, self.system, transaction)  # Exercise the real finalizer.
        self.assertIs(caught.exception, primary)  # Cleanup must not replace the original exception.
        self.assertIn('job retirement failed', ' '.join(primary.__notes__))  # Retain the independent secondary cause.
        self.assertTrue(transaction['uncertain'])  # Later installation/removal writes must be fenced.
    def test_installer_exit_and_retirement_control_transaction_custody(self):  # Only settled resumed operations authorize later recovery.
        arguments = self.inputs()  # Supply exact hashed installer inputs.
        binding = smoke.bind_inputs(arguments, self.evidence)  # Exercise input binding once for all command scenarios.
        for kind in ('msi', 'exe'):  # MSI service uncertainty and private EXE custody differ.
            for fault in ('', 'exit', 'timeout', 'cleanup', 'before-launch'):  # Cover every recovery-authority boundary.
                job, transaction = Mock(), {'launched': False, 'uncertain': False}  # No native job is created.
                job.started, job.active_count.return_value = fault != 'before-launch', 0  # Model an actual resumed launch separately from intent.
                job.run.return_value = {'returncode': 3010 if fault == 'exit' else 0, 'stdout': '', 'stderr': ''}  # A restart code is never acceptance.
                if fault in ('timeout', 'before-launch'):  # Preserve original timeout or admission failure.
                    job.run.side_effect = TimeoutError(fault)  # Raise at the native process boundary.
                if fault == 'cleanup':  # Independent retirement uncertainty fences both formats.
                    job.terminate_and_close.side_effect = OSError('retirement unavailable')  # Force the cleanup error branch.
                with self.subTest(kind=kind, fault=fault), patch.object(smoke, 'windows_job', return_value=job):  # Replace only native execution.
                    if fault:  # Any observed failure blocks the installer operation.
                        with self.assertRaises((release_tool.ReleaseError, TimeoutError)):  # Preserve production error classes.
                            smoke.run_installer(kind, 'reinstall', binding, self.installed, SimpleNamespace(system_directory=self.system), self.evidence, kind + fault, transaction)  # Exercise real transaction settlement.
                    else:  # A settled zero exit and retirement permit the operation.
                        smoke.run_installer(kind, 'reinstall', binding, self.installed, SimpleNamespace(system_directory=self.system), self.evidence, kind, transaction)  # Exercise successful settlement too.
                self.assertEqual(transaction['launched'], fault != 'before-launch')  # Intent alone never authorizes recovery.
                self.assertEqual(transaction['uncertain'], fault == 'cleanup' or kind == 'msi' and fault == 'timeout')  # Do not infer service settlement after timeout.
                job.terminate_and_close.assert_called_once()  # Always retire only this private job.
                command = job.run.call_args.args[0]  # Inspect actual installer invocation arguments.
                self.assertEqual(job.run.call_args.args[-1], 300)  # Installer waits stay bounded.
                self.assertIn('MSIRESTARTMANAGERCONTROL=Disable' if kind == 'msi' else '/NOCLOSEAPPLICATIONS', command)  # Do not delegate foreign-process shutdown to installers.
                if kind == 'msi':  # Same-package repair must actually reinstall files.
                    self.assertTrue({'REINSTALL=ALL', 'REINSTALLMODE=amus'} <= set(command))  # Verify exact native repair flags.
    def format_fixture(self, kind, defect=''):  # Supply native account readbacks and actual private files.
        original_path = {'exists': True, 'type': 1, 'value': '  C:\\Authored;;%TOOLS%;'}  # Preserve whitespace, duplicates and the final empty token.
        original = {'user_path': original_path, 'machine_path': {'exists': True, 'type': 2, 'value': '%SystemRoot%\\system32'}, 'registrations': {'msi_products': []}}  # Distinguish user and machine values.
        state, actions, restores = copy.deepcopy(original), [], []  # Keep intent, actual state and compensation separate.
        account = SimpleNamespace(directory=self.installed, system_directory=self.system)  # Never instantiate the native account adapter.
        account.snapshot = lambda: copy.deepcopy(state)  # Each readback is independent.
        account.read_path = lambda: copy.deepcopy(state['user_path'])  # Acknowledgement observes actual current text and type.
        account.inno_path_receipt = lambda: {'IliumPathReceipt': 1, 'IliumPathExisted': 1, 'IliumPathOwned': 1,
                                           'IliumPathBefore': original_path['value'],
                                           'IliumPathAfter': original_path['value'] + str(self.installed),
                                           'IliumPathEntry': str(self.installed)}
        account.product_state = Mock(return_value=-1)  # Exact captured product identity is queried after removal.
        account.validate_installed = lambda *args: {'kind': kind, 'id': 1}  # Real registration validation has separate portable tests.
        def restore(value, acknowledged):  # Model the adapter's authorization boundary, not an unconditional reset.
            restores.append(copy.deepcopy(acknowledged))  # Inspect the exact states the core authorizes.
            smoke.require(state['user_path'] == value or state['user_path'] in acknowledged, 'unacknowledged PATH preserved')  # Refuse intent-only ownership.
            state['user_path'] = copy.deepcopy(value)  # Compensate only a recognized actual observation.
        account.restore_path = restore  # Never touch this source runner's registry.
        def install(_kind, action, _binding, _directory, _account, _evidence, _label, transaction):  # Replace only the native installer operation.
            actions.append(action)  # Retain exact install/reinstall/removal order.
            if defect == 'prelaunch':  # Failed admission cannot authorize any cleanup write.
                raise ValueError('not launched')  # Leave transaction.launched false.
            transaction['launched'] = True  # All remaining scenarios model actual execution.
            if action == 'uninstall':  # Removal behavior comes from this synthetic installer, never the gate.
                if defect in ('cleanup', 'unacknowledged', 'foreign-path'):  # Preserve residual bytes and expose independent cleanup failure.
                    raise OSError('uninstall failed')  # Do not fake a successful native return.
                if defect != 'removal':  # A zero exit can still leave payload behind.
                    for member in self.installed.iterdir():  # Delete only explicitly enumerated synthetic fixture files.
                        member.unlink()  # The production gate never performs this deletion.
                    self.installed.rmdir()  # Actual absence is now independently observable.
                state.clear()  # Simulate the native uninstaller's registry transition.
                state.update(copy.deepcopy(original))  # Real removal must restore exact original typed state.
                if defect == 'lossy-path':  # Model the supplied Inno removal's known type/delimiter loss.
                    state['user_path'] = {'exists': True, 'type': 2, 'value': original_path['value'][:-1]}  # A known transformation is still failed removal.
                if defect == 'machine-path':  # Do not let the fixture erase the injected foreign machine mutation.
                    state['machine_path']['value'] += ';foreign-machine-entry'  # The gate has no authority to repair this state.
            else:  # Install and reinstall expose their actual account mutations.
                state['user_path'] = {'exists': True, 'type': 1 if kind == 'exe' else 2, 'value': original_path['value'] + str(self.installed)}  # Model the EXE's type-preserving writer separately from MSI.
                state['registrations']['msi_products'] = [{'product_code': '{11111111-1111-1111-1111-111111111111}'}] if kind == 'msi' else []  # Capture the real format's product identities.
                if kind == 'exe':  # Generated metadata is separate from audited payload.
                    for name in smoke.uninstaller_names:  # Only the expected two files establish custody.
                        (self.installed / name).write_bytes((name + action).encode())  # Successful reinstall may legitimately update generated metadata.
                if defect in ('uncertain', 'unacknowledged'):  # A failed operation cannot acknowledge its possible PATH mutation.
                    transaction['uncertain'] = defect == 'uncertain'  # An unsettled MSI additionally blocks all later mutations.
                    raise TimeoutError('install incomplete')  # Preserve the failed native operation as primary.
                if action == 'reinstall' and defect == 'reinstall':  # A second install must not duplicate a PATH entry.
                    state['user_path']['value'] += ';' + str(self.installed)  # Inject a concrete idempotence defect.
                if defect == 'machine-path':  # A per-user installer must preserve machine PATH exactly.
                    state['machine_path']['value'] += ';foreign-machine-entry'  # Inject a forbidden native effect before the first probe.
                if defect == 'foreign-path':  # A successful operation can still expose an unrecognized PATH mutation.
                    state['user_path']['value'] += ';foreign'  # Observation alone does not authorize this external-looking value.
        return account, original, state, actions, restores, install  # Expose independent effect and observation records.
    def test_both_formats_reinstall_reprobe_and_remove_before_restoration(self):  # Require the complete format sequence.
        for kind in ('msi', 'exe'):  # Recreate private payload after successful removal.
            if not self.installed.exists():  # The previous format's synthetic uninstaller removed its files.
                self.installed.mkdir()  # Begin another clean fixture installation.
            for name, data in self.content.items():  # Keep both format fixtures complete even though probes are mocked in this sequence test.
                (self.installed / name).write_bytes(data)  # Recreate only the test-owned payload removed by the prior format.
            account, original, state, actions, restores, install = self.format_fixture(kind)  # Select the native-boundary fixture.
            with self.subTest(kind=kind), patch.object(smoke, 'run_installer', side_effect=install), patch.object(smoke, 'probe') as probe:  # Probe behavior is exercised independently above.
                smoke.run_format(kind, self.binding, account, original, self.evidence)  # Run the real reinstall/removal/finalizer logic.
            self.assertEqual(actions, ['install', 'reinstall', 'uninstall'])  # No maintenance-only shortcut satisfies this order.
            self.assertEqual([call.args[3] for call in probe.call_args_list], [kind + '-initial', kind + '-repeated'])  # Both installations receive a fresh full probe.
            self.assertEqual(state, original)  # Exact account preservation is required.
            self.assertFalse(self.installed.exists())  # Acceptance follows actual uninstaller removal.
            self.assertEqual(len(restores), 1)  # Recovery executes after independent ordinary-removal checks.
    def test_exe_suffix_is_preserved_and_compensation_cannot_excuse_loss(self):
        suffix = ';C:\\ExplicitOtherTool;;'
        for preserve in (True, False):
            with self.subTest(preserve=preserve):
                self.installed.mkdir(exist_ok=True)
                for name, data in self.content.items():
                    (self.installed / name).write_bytes(data)
                account, original, state, actions, restores, install = self.format_fixture('exe')
                account.path_has = Mock(return_value=False)

                def remove(*args, **kwargs):
                    install(*args, **kwargs)
                    if args[1] == 'uninstall' and preserve:
                        state['user_path']['value'] += suffix

                with patch.object(smoke, 'run_installer', side_effect=remove), patch.object(smoke, 'probe'):
                    if preserve:
                        smoke.run_format('exe', self.binding, account, original, self.evidence, suffix_fixture=suffix)
                    else:
                        with self.assertRaisesRegex(release_tool.ReleaseError, 'exact account'):
                            smoke.run_format('exe', self.binding, account, original, self.evidence, suffix_fixture=suffix)
                self.assertEqual(state['user_path'], {'exists': True, 'type': 1, 'value': original['user_path']['value'] + suffix})
                self.assertEqual(actions, ['install', 'reinstall', 'uninstall'])
                self.assertFalse(self.installed.exists())
                self.assertEqual(len(restores), 2)
                self.assertIn({'exists': True, 'type': 1, 'value': original['user_path']['value'] + str(self.installed) + suffix}, restores[-1])

    def test_suffix_fixture_rejects_invalid_or_borrowed_admission_before_install(self):
        account, original, state, actions, restores, install = self.format_fixture('exe')
        for options in ({'suffix_fixture': 'missing separator'}, {'suffix_fixture': ';other', 'borrowed_path': True}):
            with self.subTest(options=options), patch.object(smoke, 'run_installer') as execute:
                with self.assertRaises(release_tool.ReleaseError):
                    smoke.run_format('exe', self.binding, account, original, self.evidence, **options)
                execute.assert_not_called()
    def test_reinstall_and_removal_faults_fail_without_manufactured_cleanup_success(self):  # Successful installer exits cannot hide wrong state.
        for defect in ('reinstall', 'removal'):  # Isolate idempotence and residual-file failures.
            if not self.installed.exists():  # Prior compensating removal may have removed the fixture.
                self.installed.mkdir()  # Create a new task-owned installation directory.
            (self.installed / 'sentinel').write_bytes(b'owned payload')  # Residual contents must remain observable.
            account, original, state, actions, restores, install = self.format_fixture('msi', defect)  # Inject the concrete native effect.
            with self.subTest(defect=defect), patch.object(smoke, 'run_installer', side_effect=install), patch.object(smoke, 'probe') as probe, self.assertRaises(release_tool.ReleaseError):  # No compensation can turn this into pass.
                smoke.run_format('msi', self.binding, account, original, self.evidence)  # Exercise real readbacks and cleanup.
            self.assertEqual(actions.count('uninstall'), 1)  # Never relaunch successful removal after its readback fails.
            self.assertEqual(probe.call_count, 1 if defect == 'reinstall' else 2)  # Reject changed reinstall state before its second probe.
            if defect == 'removal':  # Zero exit with residual files must remain a failed removal.
                self.assertEqual((self.installed / 'sentinel').read_bytes(), b'owned payload')  # The gate did not erase evidence to make absence true.
                self.assertTrue(any(row['type'] == 'removal-readback' and not row['directory_absent'] for row in self.evidence.rows))  # Retain actual failed removal readback.
    def test_lossy_uninstall_path_is_restored_but_the_format_still_fails(self):  # Recovery must preserve both the account and the failed acceptance result.
        account, original, state, actions, restores, install = self.format_fixture('exe', 'lossy-path')  # Model a settled native uninstall with lossy PATH removal.
        with patch.object(smoke, 'run_installer', side_effect=install), patch.object(smoke, 'probe'), self.assertRaisesRegex(release_tool.ReleaseError, 'exact account'):  # Ordinary removal fails before compensation.
            smoke.run_format('exe', self.binding, account, original, self.evidence)  # Run real acknowledgement and restoration ordering.
        lossy = {'exists': True, 'type': 2, 'value': original['user_path']['value'][:-1]}  # Define the exact observed installer transition.
        self.assertIn(lossy, restores[0])  # Only the observed post-success state authorizes this compensation.
        self.assertEqual(state, original)  # Compensation restores original raw text and type.
        self.assertEqual(actions, ['install', 'reinstall', 'uninstall'])  # Readback failure does not relaunch a completed uninstaller.
        self.assertTrue(any(row['type'] == 'removal-readback' and row['snapshot']['user_path'] == lossy for row in self.evidence.rows))  # Retain the failed native removal state before repair.
        self.assertEqual([row['state'] for row in self.evidence.rows if row['type'] == 'format-result'], ['failed'])  # Successful compensation never qualifies the package.
    def test_machine_path_mutation_fails_before_probe_and_is_never_rewritten(self):  # Preserve state outside the user-PATH recovery scope.
        account, original, state, actions, restores, install = self.format_fixture('msi', 'machine-path')  # Inject a forbidden per-machine mutation.
        with patch.object(smoke, 'run_installer', side_effect=install), patch.object(smoke, 'probe') as probe, self.assertRaisesRegex(release_tool.ReleaseError, 'machine PATH'):  # Reject this before any installed lifecycle runs.
            smoke.run_format('msi', self.binding, account, original, self.evidence)  # Attempt only scoped removal and user-PATH compensation.
        probe.assert_not_called()  # Invalid account mutation prevents further product execution.
        self.assertEqual(actions, ['install', 'uninstall'])  # Attempt only the format's known compensating removal.
        self.assertEqual(state['machine_path']['value'], original['machine_path']['value'] + ';foreign-machine-entry')  # Never overwrite machine state to manufacture restoration.
        self.assertEqual([row['state'] for row in self.evidence.rows if row['type'] == 'format-result'], ['failed'])  # Report the unresolved account defect.
    def test_failed_launch_and_unsettled_msi_fence_recovery_writes(self):  # Neither intent nor possible template output proves ownership.
        for defect in ('prelaunch', 'uncertain', 'unacknowledged', 'foreign-path'):  # Distinguish admission failure, uncertain service and unowned effects.
            account, original, state, actions, restores, install = self.format_fixture('msi', defect)  # Keep actual readbacks after the injected failure.
            with self.subTest(defect=defect), patch.object(smoke, 'run_installer', side_effect=install), patch.object(smoke, 'probe'), self.assertRaises((ValueError, TimeoutError)):  # Preserve the native-operation error.
                smoke.run_format('msi', self.binding, account, original, self.evidence)  # Execute the real mutation fences.
            self.assertEqual(actions, ['install', 'uninstall'] if defect in ('unacknowledged', 'foreign-path') else ['install'])  # Never retry uninstall while MSI settlement is unknown.
            self.assertEqual(restores, [[original['user_path']]] if defect in ('unacknowledged', 'foreign-path') else [])  # Unobserved or unrecognized PATH transitions remain unauthorized.
            if defect != 'prelaunch':  # Unacknowledged or uncertain state must be preserved and reported.
                self.assertNotEqual(state['user_path'], original['user_path'])  # The gate did not overwrite a value it could not own.
            if defect == 'foreign-path':  # Even a completed operation does not authorize arbitrary observed text.
                self.assertTrue(any(row['type'] == 'path-acknowledgement' and row['state'] == state['user_path'] and not row['owned_transition'] for row in self.evidence.rows))  # Retain the observed but unowned transition.
    def test_lifecycle_primary_survives_uninstall_and_readback_failures(self):  # Continue independent cleanup without masking the primary exception.
        account, original, state, actions, restores, install = self.format_fixture('msi', 'cleanup')  # Ordinary install succeeds; cleanup fails.
        primary = ValueError('pane lifecycle failed')  # Retain the exact original failure object.
        with patch.object(smoke, 'run_installer', side_effect=install), patch.object(smoke, 'probe', side_effect=primary), self.assertRaises(ValueError) as caught:  # Exercise production exception aggregation.
            smoke.run_format('msi', self.binding, account, original, self.evidence)  # Failed lifecycle triggers bounded final removal readback.
        self.assertIs(caught.exception, primary)  # Cleanup never replaces the first failure.
        self.assertIn('uninstall failed', ' '.join(primary.__notes__))  # Surface the independent removal command error.
        self.assertIn('uninstall left install directory', ' '.join(primary.__notes__))  # Surface the independent filesystem readback error.
        self.assertEqual(state['user_path'], original['user_path'])  # Acknowledged exact PATH compensation still runs after uninstall failure.
        self.assertTrue(self.installed.exists())  # Compensation did not manufacture package removal.
    def test_changed_exe_uninstaller_is_never_executed(self):  # Name-only uninstaller discovery is insufficient custody.
        account, original, state, actions, restores, install = self.format_fixture('exe')  # Native installation supplies both generated files.
        def probe(*arguments):  # Corrupt metadata only after the second successful lifecycle.
            if arguments[3] == 'exe-repeated':  # Reinstall metadata has already been legitimately recaptured.
                (self.installed / 'unins000.exe').write_bytes(b'foreign replacement')  # Simulate an unowned replacement at the same path.
        with patch.object(smoke, 'run_installer', side_effect=install), patch.object(smoke, 'probe', side_effect=probe), self.assertRaisesRegex(release_tool.ReleaseError, 'uninstaller changed'):  # Require prior byte custody before executing removal.
            smoke.run_format('exe', self.binding, account, original, self.evidence)  # Both normal and compensating removal must refuse changed bytes.
        self.assertEqual(actions, ['install', 'reinstall'])  # Neither path executes the changed uninstaller.
        self.assertEqual((self.installed / 'unins000.exe').read_bytes(), b'foreign replacement')  # Preserve the ambiguous file for reconciliation.
    def test_exe_rejects_changed_path_type_and_replaced_reinstall_receipt(self):
        for defect in ('registry-type', 'reinstall-receipt'):
            with self.subTest(defect=defect):
                # The previous iteration may legitimately remove its fixture files.
                self.installed.mkdir(exist_ok=True)
                for name, data in self.content.items():
                    (self.installed / name).write_bytes(data)
                account, original, state, actions, restores, install = self.format_fixture('exe')
                receipt = account.inno_path_receipt

                def mutate(*args, **kwargs):
                    install(*args, **kwargs)
                    if defect == 'registry-type' and args[1] == 'install':
                        state['user_path']['type'] = 2
                    if defect == 'reinstall-receipt' and args[1] == 'reinstall':
                        account.inno_path_receipt = lambda: dict(receipt(), IliumPathBefore='replaced baseline')

                expected = 'registry type' if defect == 'registry-type' else 'original PATH ownership'
                with patch.object(smoke, 'run_installer', side_effect=mutate), patch.object(smoke, 'probe'), self.assertRaisesRegex(release_tool.ReleaseError, expected):
                    smoke.run_format('exe', self.binding, account, original, self.evidence)
                self.assertEqual(state, original)

    def test_removal_preserves_directory_error_and_registry_read_error(self):  # Independent readbacks must not mask each other.
        account = SimpleNamespace(snapshot=Mock(side_effect=OSError('registry unreadable')))  # Directory remains present and registry read fails independently.
        with self.assertRaisesRegex(release_tool.ReleaseError, 'uninstall left install directory') as caught:  # Keep the first authoritative failure.
            smoke.removed(self.installed, {}, account, [], self.evidence, 'failed-readback')  # Run real bounded absence check and follow-up read.
        self.assertIn('registry unreadable', ' '.join(caught.exception.__notes__))  # Preserve unavailable registry evidence as a secondary cause.
        account.snapshot.assert_called_once()  # Still attempt the independent read after the first failure.
    def test_workflow_passes_every_required_flag_and_keeps_always_diagnostics(self):  # Protect the only required caller integration.
        workflow = yaml.load((root / '.github/workflows/release.yml').read_text(encoding='utf-8'), Loader=yaml.BaseLoader)  # Parse the supplied full workflow.
        job = workflow['jobs']['windows-installers']  # Scope assertions to the existing Windows job.
        step = next(value for value in job['steps'] if 'build_windows_installers.py smoke ' in value.get('run', ''))  # Select the actual smoke command, independent of step label.
        import shlex  # Parse Bash comments and quotes using the declared workflow shell.
        command = shlex.split(step['run'], comments=True)  # Ignore the new explanatory command comment.
        arguments = package.parser().parse_args(command[command.index('smoke'):])  # Required arguments must parse with the recovered B1 parser.
        self.assertEqual((arguments.archive.as_posix(), arguments.audit_report.as_posix(), arguments.manifest.as_posix()), ('native-windows/ilium-windows-x86_64.zip', 'native-windows/native-audit.json', 'release/targets.toml'))  # Bind exactly the downloaded native artifacts.
        retained = next(value for value in job['steps'] if value.get('with', {}).get('name') == 'diagnostics-windows-installers')  # Use the existing diagnostic artifact authority.
        self.assertEqual((retained['if'], retained['with']['path'], retained['with']['retention-days']), ('always()', '${{ runner.temp }}/installer-smoke/', '30'))  # Failures retain all nested process and installer evidence.
        self.assertEqual(set(job['needs']), {'source', 'native'})  # Preserve prerequisite authority.
#
class PathMatrixTests(unittest.TestCase):
    def matrix_account(self):
        original = {'user_path': {'exists': True, 'type': 2, 'value': 'original'},
                    'machine_path': {'value': 'machine'}, 'registrations': {'inno': {}}}
        state = copy.deepcopy(original)
        writes = []

        def restore(value, allowed):
            if state['user_path'] == value:
                return
            smoke.require(state['user_path'] in allowed, 'foreign PATH preserved')
            writes.append(copy.deepcopy(value))
            state['user_path'] = copy.deepcopy(value)

        account = SimpleNamespace(directory=Path('C:/Local/Programs/ilium'), system_directory=Path('C:/Windows/System32'),
                                  snapshot=lambda: copy.deepcopy(state), restore_path=restore)
        return account, original, state, writes

    def test_matrix_runs_ten_real_gate_calls_and_restores_each_typed_fixture(self):
        account, original, state, writes = self.matrix_account()
        with tempfile.TemporaryDirectory() as temporary:
            evidence = memory_evidence(Path(temporary))
            children = []

            def journal(path):
                child = memory_evidence(path)
                children.append(child)
                return child

            def native_readback(kind, binding, account, baseline, child, **options):
                if options['suffix_fixture'] is not None:
                    before = baseline['user_path']['value'] or ''
                    suffix = options['suffix_fixture']
                    state['user_path'] = {'exists': True, 'type': baseline['user_path']['type'] or 1,
                                          'value': before + suffix if before else suffix[1:]}

            with patch.object(smoke, 'journal', side_effect=journal), patch.object(smoke, 'run_format', side_effect=native_readback) as execute:
                smoke.exe_path_matrix({}, account, original, evidence)
            self.assertEqual(execute.call_count, 10)
            fixtures = [call.args[3]['user_path'] for call in execute.call_args_list]
            self.assertEqual(fixtures[0], {'exists': False, 'type': None, 'value': None})
            self.assertEqual([(item['type'], item['value']) for item in fixtures[1:3]], [(1, ''), (2, '')])
            self.assertEqual([call.kwargs['borrowed_path'] for call in execute.call_args_list], [False] * 5 + [True] * 2 + [False] * 3)
            self.assertEqual([call.kwargs['suffix_fixture'] is not None for call in execute.call_args_list], [False] * 7 + [True] * 3)
            self.assertEqual(len({child.root for child in children}), 10)
            self.assertEqual(state, original)
            self.assertEqual(len(writes), 20)

    def test_matrix_preserves_foreign_path_and_primary_installer_failure(self):
        account, original, state, writes = self.matrix_account()
        primary = ValueError('native installer failed')

        def failed(*args, **kwargs):
            state['user_path'] = {'exists': True, 'type': 1, 'value': 'foreign edit'}
            raise primary

        with tempfile.TemporaryDirectory() as temporary:
            evidence = memory_evidence(Path(temporary))
            with patch.object(smoke, 'journal', side_effect=lambda path: memory_evidence(path)), patch.object(smoke, 'run_format', side_effect=failed) as execute:
                with self.assertRaises(ValueError) as caught:
                    smoke.exe_path_matrix({}, account, original, evidence)
            self.assertIs(caught.exception, primary)
            self.assertEqual(execute.call_count, 1)
            self.assertEqual(state['user_path']['value'], 'foreign edit')
            self.assertEqual(writes, [{'exists': False, 'type': None, 'value': None}])
            self.assertIn('foreign PATH preserved', ' '.join(primary.__notes__))


if __name__ == '__main__':  # Support focused primary verification.
    unittest.main()  # This suite reports source-contract outcomes only.
