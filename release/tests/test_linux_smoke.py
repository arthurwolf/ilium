"""Portable acceptance regressions; native commands are replaced by fixture adapters."""  # These tests never install a package.
import builtins  # Simulate a platform without the POSIX account module.
import hashlib  # Build independent expected member digests.
import json  # Build exact synthetic installed JSONL output.
import importlib.util  # Import the actual complete smoke source afresh.
import os  # Supply isolated shell environments.
from pathlib import Path, PureWindowsPath  # Exercise Windows package-path spelling too.
import shlex  # Quote only owned fixture paths.
import shutil  # Locate the tools needed by shell-only fixtures.
import subprocess  # Run harmless generated shell checks against temporary data.
import sys  # Locate the supplied release scripts.
import tempfile  # Own every fixture file and directory.
import unittest  # Retain portable source-suite discovery.
from unittest.mock import patch  # Replace native adapters without installing or killing anything.

root = Path(__file__).resolve().parents[2]  # Resolve this checkout's source rather than a developer installation.
sys.path.insert(0, str(root / 'release/scripts'))  # Use the actual smoke and builder modules.
import smoke_linux_packages as smoke  # This import must itself remain portable.
import release_tool  # Bind official package fixture bytes.


def audited_tree(directory, package_format='snap'):  # Create independent nonexecutable audited fixture members.
    tree = Path(directory).resolve()  # Avoid ambient temporary-parent symlinks.
    contents = {'ilium': b'fixture client', 'ilium-server': b'fixture server', 'ilium-animation-helper': b'fixture helper',
                **{name: (root / 'ilium-animation-js/assets/packages' / name).read_bytes() for name in release_tool.APPROVED_PACKAGES},
                'libonnxruntime.so.1': b'fixture runtime', 'VERSION': b'0.1.0\n', 'THIRD-PARTY.txt': b'fixture notices'}  # Include every required category.
    prefix = 'lib/ilium' if package_format in ('snap', 'flatpak') else 'usr/lib/ilium'  # Encode the supplied packaging contract independently.
    for name, data in contents.items():  # Create actual installed-path readback inputs.
        relative = ('share/doc/ilium/' if package_format in ('snap', 'flatpak') else 'usr/share/doc/ilium/') + name if name == 'THIRD-PARTY.txt' else prefix + '/' + name  # Keep notices outside the runtime root.
        path = tree / relative  # Write only beneath the owned fixture tree.
        path.parent.mkdir(parents=True, exist_ok=True)  # Supply real directory ancestors.
        path.write_bytes(data)  # These bytes are never executed.
    receipt = {'version': '0.1.0', 'package_files': {name: hashlib.sha256(data).hexdigest() for name, data in contents.items()}}  # Hash the independent fixture bytes.
    return tree, receipt  # Reuse the same real readback inputs across rejection cases.


class linux_smoke_tests(unittest.TestCase):  # All tests in this class run without POSIX-only modules or tools.
    def test_container_animation_requires_both_installed_render_receipts(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            _tree, fixture = audited_tree(temporary, 'deb')
            package = directory / smoke.packages.package_name('x86_64', 'rpm')
            package.write_bytes(b'synthetic rpm fixture; not executable')
            fixture.update(tag='v0.1.0', source_archive_sha256='a' * 64,
                           packages={package.name: smoke.packages.sha(package)})
            audit = directory / 'native-audit.json'
            audit.write_text('{}\n')
            command = type('Arguments', (), {'audit_report': audit,
                                             'workspace': root / 'Cargo.toml',
                                             'packages': directory, 'arch': 'x86_64'})()
            files = fixture['package_files']
            catalogue = {'type': 'artifact', 'gate': 'installed_catalogue',
                         'packages': ['beach', 'carpet'],
                         'client_path': '/usr/lib/ilium/ilium',
                         'client_sha256': files['ilium'],
                         'helper_path': '/usr/lib/ilium/ilium-animation-helper',
                         'helper_sha256': files['ilium-animation-helper']}
            renders = [{'type': 'artifact', 'gate': 'installed_render',
                        'package': filename.split('-')[0], 'archive_sha256': digest,
                        'helper_sha256': files['ilium-animation-helper'],
                        'rendered_frames': 2, 'physical_retirement': True,
                        'worker_threads_before': 1, 'worker_threads_after': 1,
                        'worker_bytes_before': 1024, 'worker_bytes_after': 1024}
                       for filename, digest in release_tool.APPROVED_PACKAGES.items()]
            rows = [catalogue, *renders, {'type': 'result', 'gate': 'installed_animation',
                                          'state': 'passed', 'publication_allowed': False,
                                          'packages': ['beach', 'carpet']}]
            output = ''.join(json.dumps(row) + '\n' for row in rows)
            result = subprocess.CompletedProcess([], 0, 'ILIUM_ANIMATION_BEGIN\n' + output +
                                                 'ILIUM_ANIMATION_END\n', '')
            accepted = smoke.container_animation(command, fixture, result, 'rpm-test', 'rpm',
                                                 'fedora:41', directory)
            self.assertEqual(json.loads(Path(accepted['path']).read_text())['renders'], renders)
            for changed in (result.stdout.replace(files['ilium-animation-helper'], '0' * 64),
                            result.stdout.replace('ILIUM_ANIMATION_END\n', '')):
                with self.assertRaises(release_tool.ReleaseError):
                    smoke.container_animation(command, fixture,
                                              subprocess.CompletedProcess([], 0, changed, ''),
                                              'rpm-rejected', 'rpm', 'fedora:41', directory)
    def test_complete_module_import_and_inspect_parser_without_pwd(self):  # Reproduce the Windows import boundary even on Linux.
        original_import = builtins.__import__  # Delegate every unrelated import normally.
        attempts = []  # Detect any attempted POSIX acquisition at module load.
        def without_pwd(name, *arguments, **options):  # Simulate the actual absence of the platform module.
            if name == 'pwd':  # Existing sys.modules entries must not mask the regression.
                attempts.append(name)  # Retain the forbidden import attempt.
                raise ModuleNotFoundError("No module named 'pwd'")  # Match Windows' module availability.
            return original_import(name, *arguments, **options)  # Keep the rest of Python import semantics intact.
        specification = importlib.util.spec_from_file_location('linux_smoke_without_pwd', smoke.__file__)  # Read the actual entire production source.
        module = importlib.util.module_from_spec(specification)  # Avoid reusing the already imported smoke object.
        with patch.object(builtins, '__import__', side_effect=without_pwd), patch.object(sys, 'path', list(sys.path)):  # Bound import interception and path mutations.
            specification.loader.exec_module(module)  # An unconditional import fails this real load.
            arguments = module.parser().parse_args(['inspect', '--arch', 'aarch64', '--packages', 'fixture'])  # Portable inspection setup must also remain available.
            self.assertEqual(module.selected_formats(arguments), ['deb', 'rpm', 'appimage', 'snap'])  # Preserve declared nonexecuting coverage.
        self.assertEqual(attempts, [])  # A passing import must not silently fall back after trying pwd.

    def test_linux_snap_refuses_missing_account_inventory(self):  # Lazy acquisition must still fail closed on the native boundary.
        original_import = builtins.__import__  # Preserve all unrelated imports.
        def without_pwd(name, *arguments, **options):  # Refuse exactly the required account module.
            if name == 'pwd':  # Simulate a broken native account prerequisite.
                raise ModuleNotFoundError("No module named 'pwd'")  # The smoke must report an explicit gate failure.
            return original_import(name, *arguments, **options)  # Do not stub the smoke itself.
        def configure_only(arguments, receipt, log, package_format, configure):  # Reach the native boundary without creating a host workspace.
            return configure(None, None, None, None, None)  # Acquisition must precede any package or filesystem access.
        with patch.object(smoke, 'host_case', side_effect=configure_only), patch.object(smoke.platform, 'system', return_value='Linux'), patch.object(builtins, '__import__', side_effect=without_pwd), patch.object(smoke, 'package_reference') as unpacker:  # Replace only native setup adapters.
            with self.assertRaisesRegex(smoke.release_tool.ReleaseError, 'POSIX pwd'):  # An empty account list is never an acceptable fallback.
                smoke.host_snap(None, None, None)  # Exercise the public adapter's real configuration function.
        unpacker.assert_not_called()  # Failure must happen before package admission or installation.

    def test_exact_members_and_each_wrong_installed_hash(self):  # Verify all format layouts and every audited member category.
        for package_format in ('deb', 'rpm', 'appimage', 'flatpak', 'snap'):  # Cover both FHS and sandbox prefixes.
            with self.subTest(format=package_format), tempfile.TemporaryDirectory() as temporary:  # Isolate every format's filesystem.
                tree, receipt = audited_tree(temporary, package_format)  # Create independent installed-path fixture bytes.
                self.assertEqual(smoke.verify_payload(package_format, tree, receipt), receipt['package_files'])  # Establish the real positive readback path.
                for name in receipt['package_files']:  # Corrupt client, server, runtime, VERSION and notices separately.
                    path = smoke.notices_path(package_format, tree) if name == 'THIRD-PARTY.txt' else smoke.payload_root(package_format, tree) / name  # Locate the actual installed member.
                    original = path.read_bytes()  # Preserve the fixture's positive baseline.
                    path.write_bytes(original + b' changed')  # Change real bytes without changing the expected digest.
                    with self.subTest(member=name), self.assertRaises(smoke.release_tool.ReleaseError):  # No single audited category may escape verification.
                        smoke.verify_payload(package_format, tree, receipt)  # Invoke the same readback used by native gates.
                    path.write_bytes(original)  # Restore only this owned fixture member.

    def test_missing_extra_and_nonregular_runtime_members_fail(self):  # Exact names alone cannot qualify a payload.
        for mutation in ('missing', 'extra', 'misplaced-notice', 'directory'):  # Include the original misplaced-notice false acceptance.
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temporary:  # Start from an independent clean payload.
                tree, receipt = audited_tree(temporary)  # Use the Snap layout without symlink requirements.
                payload = tree / 'lib/ilium'  # Select only the owned runtime root.
                if mutation in ('missing', 'directory'):  # Replace or remove the installed server member.
                    (payload / 'ilium-server').unlink()  # Delete only the synthetic server bytes.
                    if mutation == 'directory':  # A matching directory name must still fail.
                        (payload / 'ilium-server').mkdir()  # Substitute a nonregular runtime member.
                else:  # Add a member outside the exact audited runtime inventory.
                    (payload / ('THIRD-PARTY.txt' if mutation == 'misplaced-notice' else 'extra.so')).write_bytes(b'extra')  # Keep expected hashes unchanged.
                with self.assertRaises(smoke.release_tool.ReleaseError):  # Each mutation must disqualify installed-byte evidence.
                    smoke.verify_payload('snap', tree, receipt)  # Exercise exact inventory and file-type checks.

    def test_complete_layout_uses_posix_member_names_on_windows(self):  # Reproduce Windows path spelling independently of the host OS.
        with tempfile.TemporaryDirectory() as temporary:  # All filesystem reads stay inside this fixture.
            tree, receipt = audited_tree(temporary)  # Build the independently declared Snap runtime and notices.
            (tree / 'share/doc/ilium/LICENSE').write_bytes(b'fixture license')  # Supply the known builder license path.
            (tree / 'meta').mkdir()  # Supply the only allowed Snap metadata directory.
            (tree / 'meta/snap.yaml').write_bytes(b'fixture metadata')  # No metadata is executed by layout inspection.
            def windows_member_path(value):  # Use Windows semantics only for relative package member calculations.
                return PureWindowsPath(value) if isinstance(value, str) else Path(value)  # Retain real temporary filesystem operations.
            with patch.object(smoke, 'Path', side_effect=windows_member_path):  # Exercise the actual namespace comparison with backslash-native parents.
                reference = smoke.layout_reference('snap', tree, receipt)  # This failed when structural parents used str(Path).
                verifier = smoke.reference_script(reference)  # Generated checksum input must retain POSIX member separators too.
            self.assertIn('lib/ilium/ilium', reference)  # The full admitted leaf set remains available.
            self.assertIn('  lib/ilium/ilium\n', verifier)  # Verify the checksum record's actual member spelling.
            (tree / 'meta/hooks').mkdir()  # An unrequested hook directory is outside the builder layout.
            with self.assertRaises(smoke.release_tool.ReleaseError):  # Extra directories cannot qualify as harmless parents.
                smoke.layout_reference('snap', tree, receipt)  # Test the complete payload allowlist.

    def test_invalid_explicit_format_sets_fail_before_adapters(self):  # Required-format omissions must never become success.
        cases = [('inspect', ''), ('inspect', 'flatpak'), ('inspect', 'deb,deb'), ('containers', 'snap'), ('host', 'rpm'), ('host', 'unknown')]  # Cover empty, duplicate and unsupported coverage.
        for command, formats in cases:  # Exercise the actual public CLI boundary.
            with self.subTest(command=command, formats=formats), patch.object(smoke, 'load_receipt') as receipt, patch.object(smoke, 'host_run') as runner, patch.object(smoke, 'emit'):  # No native or artifact adapter may run.
                arguments = [command, '--arch', 'x86_64', '--packages', 'fixture', '--formats', formats]  # Use the preserved CLI option grammar.
                if command != 'inspect':  # Execution modes require their existing diagnostics option.
                    arguments += ['--log', 'fixture-log']  # This path must never be created by the rejected request.
                self.assertEqual(smoke.main(arguments), 1)  # Every malformed coverage request must exit nonzero.
                receipt.assert_not_called()  # Validate all format choices before reading inputs.
                runner.assert_not_called()  # Never partially install a valid prefix of an invalid list.

    def test_native_architecture_is_required_for_both_lanes(self):  # Configuration alone is not native execution evidence.
        for architecture, machine in (('x86_64', 'x86_64'), ('aarch64', 'aarch64')):  # Exercise both preserved architecture contracts.
            with self.subTest(architecture=architecture), patch.object(smoke.platform, 'system', return_value='Linux'), patch.object(smoke.platform, 'machine', return_value=machine):  # Supply explicit native identities.
                self.assertIn(machine, smoke.native_architecture(architecture))  # Matching native execution is admitted.
                other = 'aarch64' if architecture == 'x86_64' else 'x86_64'  # Select the opposite runtime architecture.
                with self.assertRaises(smoke.release_tool.ReleaseError):  # Emulated or mismatched lanes cannot qualify.
                    smoke.native_architecture(other)  # Exercise the same guard used before containers and host mutation.

    def test_transaction_preserves_primary_and_cleanup_failures(self):  # Successful cleanup cannot erase a failed install or verification.
        for failures in (set(), {'preflight'}, {'install'}, {'verify'}, {'remove'}, {'absence'}, {'verify', 'remove', 'absence'}):  # Include independent and combined outcomes.
            events = []  # Observe exactly which owned stages execute.
            def action(stage):  # Supply a safe native-operation adapter with a controlled outcome.
                def perform():  # Each callback represents one required gate.
                    events.append(stage)  # Record actual callback execution order.
                    if stage in failures:  # Raise a realistic checked-operation error.
                        raise smoke.release_tool.ReleaseError(stage + ' fixture failure')  # Preserve a distinct failure identity.
                return perform  # Return the complete adapter used by transaction.
            actions = {stage: action(stage) for stage in ('preflight', 'install', 'verify', 'remove', 'absence')}  # Keep all gate callbacks explicit.
            with self.subTest(failures=sorted(failures)):  # Report the exact failing combination if a regression occurs.
                stages, errors, owned = smoke.transaction(actions)  # Execute the real transaction state machine.
                self.assertEqual(bool(errors), bool(failures))  # No failed stage may report a successful transaction.
                if 'preflight' in failures:  # A foreign installation never becomes owned.
                    self.assertEqual(events, ['preflight'])  # Refuse install and uninstall after failed ownership proof.
                    self.assertFalse(owned)  # Preserve the pre-existing installation boundary.
                    continue  # Remaining assertions apply only to attempted owned installations.
                self.assertTrue(owned)  # Partial installs still require cleanup.
                self.assertEqual(events[-2:], ['remove', 'absence'])  # Readback must run even when removal fails.
                self.assertEqual({stage for stage, state in stages.items() if state == 'failed'}, failures)  # Retain each independent failure.

    def test_both_version_checks_and_removal_metadata_are_authoritative(self):  # Wrong successful binaries and removed=false are failures.
        for name in ('ilium', 'ilium-server'):  # Preserve exact client and sibling-server version expectations.
            for output, status in ((name + ' 0.1.0\n', 0), (name + ' 9.9.9\n', 0), ('', 9)):  # Include matching, wrong and failed commands.
                with self.subTest(name=name, output=output), tempfile.TemporaryDirectory() as temporary, patch.object(smoke.subprocess, 'run', return_value=subprocess.CompletedProcess([], status, output, '')):  # Execute only the logging/version adapter.
                    if output == name + ' 0.1.0\n' and status == 0:  # Establish a genuine positive parser path.
                        smoke.expect_version([name], name + ' 0.1.0', temporary, 'version', {})  # Exact output qualifies this one check.
                        continue  # Negative cases must raise instead.
                    with self.assertRaises(smoke.release_tool.ReleaseError):  # A zero exit code alone is insufficient.
                        smoke.expect_version([name], name + ' 0.1.0', temporary, 'version', {})  # Require the exact installed identity.
        with patch.object(smoke, 'emit') as emitted:  # Capture the existing public result schema.
            self.assertEqual(smoke.report('host', 'snap', 'fixture', subprocess.CompletedProcess([], 0, '', ''), {'removed': False}), 1)  # Reproduce the original Snap metadata false success.
        self.assertEqual(emitted.call_args.kwargs['state'], 'failed')  # The JSONL result must agree with the nonzero gate outcome.

    def test_timeout_and_startup_failure_keep_diagnostics(self):  # Failed native adapters must produce retained evidence.
        for error, expected in ((subprocess.TimeoutExpired('fixture', 1, output=b'partial output', stderr=b'partial error'), 124), (FileNotFoundError('missing fixture executable'), 127)):  # Cover both failure boundaries.
            with self.subTest(expected=expected), tempfile.TemporaryDirectory() as temporary, patch.object(smoke.subprocess, 'run', side_effect=error):  # No actual child is started.
                result = smoke.host_run(['fixture'], temporary, 'adapter')  # Exercise production error conversion and logging.
                self.assertEqual(result.returncode, expected)  # Neither failure may be confused with successful absence.
                self.assertIn('exit_code=' + str(expected), (Path(temporary) / 'adapter.log').read_text())  # Require retained command status.
                if expected == 124:  # Timeouts must preserve all partial output.
                    self.assertIn('partial output', result.stdout)  # Retain stdout produced before timeout.
                    self.assertIn('partial error', result.stderr)  # Retain stderr produced before timeout.


shell_available = sys.platform.startswith('linux') and all(shutil.which(tool) for tool in ('sh', 'awk', 'mktemp', 'timeout', 'sha256sum', 'find', 'sort', 'cmp'))  # POSIX shell fixtures are separate from portable Python regressions.


@unittest.skipUnless(shell_available, 'requires Linux shell/coreutils fixture tools; no native package operations')  # This fixture skip never changes production required-format gates.
class linux_smoke_shell_tests(unittest.TestCase):  # Run actual generated shell logic against owned temporary paths.
    def test_installed_body_requires_both_versions_and_real_lifecycle_success(self):  # Run the shared deb/RPM body with synthetic binaries and the actual lifecycle.
        client_source = '#!/bin/sh\nset -eu\nif [ "${1:-}" = --version ]; then printf "%s\\n" "${fixture_client_version:-ilium 0.1.0}"; exit 0; fi\nif [ "${1:-}" = release-animation-probe ]; then test "${fixture_animation_failure:-0}" = 0; printf "%s\\n" "$fixture_animation_output"; exit 0; fi\ncase "$3" in\nnew-pane) test "${fixture_lifecycle_failure:-0}" = 0; touch "$fixture_alive" ;;\nls) if [ -e "$fixture_alive" ]; then echo "default running"; else echo "default not running"; fi ;;\nkill-session) rm -f "$fixture_alive" ;;\n*) exit 2 ;;\nesac\n'  # This complete harmless CLI changes only a named fixture marker.
        server_source = '#!/bin/sh\nset -eu\nprintf "%s\\n" "${fixture_server_version:-ilium-server 0.1.0}"\n'  # Version changes are adapter outcomes rather than changed audited bytes.
        helper_source = '#!/bin/sh\nset -eu\nprintf "%s\\n" "$fixture_helper_version"\n'
        for mode in ('pass', 'wrong-client', 'wrong-server', 'wrong-helper', 'animation-fails', 'lifecycle-fails'):  # Every installed caller failure must disqualify the generated shell.
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:  # Own the complete installed tree and shell environment.
                tree, receipt = audited_tree(temporary, 'deb')  # Supply every audited installed member and the canonical notices.
                for name, source in (('ilium', client_source), ('ilium-server', server_source), ('ilium-animation-helper', helper_source)):  # Replace only nonexecuted fixture placeholders with harmless test programs.
                    path = tree / 'usr/lib/ilium' / name  # Keep both test programs beside the synthetic runtime.
                    path.write_text(source)  # Persist the exact fixture program bytes.
                    path.chmod(0o755)  # Permit only these owned programs to run.
                    receipt['package_files'][name] = hashlib.sha256(path.read_bytes()).hexdigest()  # Freeze their real bytes before testing output variations.
                (tree / 'usr/bin').mkdir()  # Supply the real relative launcher contract.
                for name in ('ilium', 'ilium-server'):  # Both installed commands must resolve the audited pair.
                    (tree / 'usr/bin' / name).symlink_to('../lib/ilium/' + name)  # Point only to the owned synthetic executables.
                checks = tree / 'checks'  # Keep generated acceptance inputs separate from the payload.
                checks.mkdir()  # Create the test-owned helper directory.
                smoke.write_smoke_files(checks, receipt)  # Copy the actual current bounded lifecycle unchanged.
                (checks / 'expected.sha256').write_text(smoke.expected_hashes(receipt).replace('  /usr/', '  ' + str(tree) + '/usr/'))  # Redirect checksum reads to actual fixture installation paths.
                script = smoke.installed_body('0.1.0').replace('/usr/', shlex.quote(str(tree)) + '/usr/').replace('/smoke/', shlex.quote(str(checks)) + '/').replace(smoke.UNPRIVILEGED, 'env')  # Substitute filesystem/identity adapters while preserving every production assertion.
                environment = {**os.environ, 'fixture_alive': str(tree / 'alive'), 'fixture_client_version': 'ilium 9.9.9' if mode == 'wrong-client' else 'ilium 0.1.0', 'fixture_server_version': 'ilium-server 9.9.9' if mode == 'wrong-server' else 'ilium-server 0.1.0', 'fixture_helper_version': release_tool.helper_version_record('9.9.9') if mode == 'wrong-helper' else release_tool.helper_version_record('0.1.0'), 'fixture_animation_failure': '1' if mode == 'animation-fails' else '0', 'fixture_animation_output': '{"type":"fixture"}', 'fixture_lifecycle_failure': '1' if mode == 'lifecycle-fails' else '0', 'ILIUM_SMOKE_BASE': str(tree / 'state'), 'TMPDIR': str(tree)}  # All application effects are temporary and explicit.
                result = subprocess.run(['sh', '-ec', script], env=environment, capture_output=True, text=True, timeout=20)  # No system binary or package manager runs through the installed paths.
                self.assertEqual(result.returncode == 0, mode == 'pass', result.stdout + result.stderr)  # Both version mismatches and actual lifecycle failure must disqualify the body.
                self.assertEqual('lifecycle: passed' in result.stdout, mode == 'pass')  # Require the real lifecycle to complete on the positive path.

    def test_absence_rejects_early_regular_and_dangling_leftovers(self):  # An early AND-list failure must not be hidden by a later absent path.
        with tempfile.TemporaryDirectory() as temporary:  # Own every queried path.
            first, last = Path(temporary) / 'first', Path(temporary) / 'last'  # Keep the later path absent throughout.
            script = smoke.absence_script([first, last])  # Generate the real production readback code.
            self.assertEqual(subprocess.run(['sh', '-ec', script], capture_output=True).returncode, 0)  # Establish the all-absent positive path.
            first.write_bytes(b'leftover')  # Simulate an ordinary installed member left behind.
            self.assertNotEqual(subprocess.run(['sh', '-ec', script], capture_output=True).returncode, 0)  # The first existing path must fail the entire script.
            first.unlink()  # Replace only the owned fixture member.
            first.symlink_to(Path(temporary) / 'missing')  # Simulate a dangling installed launcher.
            self.assertNotEqual(subprocess.run(['sh', '-ec', script], capture_output=True).returncode, 0)  # A dangling link also disqualifies absence.

    def test_inventory_requires_successful_query_and_filesystem_absence(self):  # Query errors cannot masquerade as an empty manager database.
        with tempfile.TemporaryDirectory() as temporary:  # Own the fake manager, payload and its scratch directory.
            directory = Path(temporary)  # Bind all shell adapters to this fixture.
            payload = directory / 'payload'  # Never query or remove real installation paths.
            for package_format, executable in (('deb', 'dpkg-query'), ('rpm', 'rpm')):  # Cover both documented manager inventory contracts.
                query = directory / executable  # Shadow only the manager invoked by this generated helper.
                query.write_text('#!/bin/sh\nprintf "%s\\n" "$fixture_listing"\nexit "$fixture_query_code"\n')  # Return controlled table bytes and status.
                query.chmod(0o755)  # Permit only this synthetic query program to execute.
                with patch.object(smoke, 'fhs_paths', return_value=[payload]):  # Redirect every filesystem readback into the fixture.
                    script = smoke.inventory_script(package_format)  # Keep the actual query/error/absence logic intact.
                for listing, query_code, leftover in (('', 0, False), ('other-package', 0, False), ('ilium', 0, False), ('', 7, False), ('', 0, True)):  # Include positive, registered, failed-query and leftover cases.
                    if leftover:  # Create the only allowed filesystem mutation in this case.
                        payload.write_bytes(b'owned leftover')  # A clean database must not hide remaining files.
                    environment = {**os.environ, 'PATH': str(directory) + os.pathsep + os.environ['PATH'], 'TMPDIR': temporary, 'fixture_listing': listing, 'fixture_query_code': str(query_code)}  # Ensure no real package manager is reached.
                    result = subprocess.run(['sh', '-ec', script], env=environment, capture_output=True, text=True, timeout=10)  # Execute harmless actual inventory logic.
                    with self.subTest(format=package_format, listing=listing, query_code=query_code, leftover=leftover):  # Attribute every rejection precisely.
                        self.assertEqual(result.returncode == 0, query_code == 0 and listing != 'ilium' and not leftover)  # Require all independent absence conditions.
                    payload.unlink(missing_ok=True)  # Remove only this iteration's owned synthetic leftover.

    def test_managed_shell_cleanup_never_hides_failure_or_adopts_foreign_state(self):  # Exercise the shared container transaction with safe operation adapters.
        for mode in ('pass', 'verify-fails', 'remove-fails', 'leftover', 'foreign'):  # Include a complete positive transaction and each ownership/cleanup failure.
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:  # Every adapter is confined to a fresh fixture.
                directory = Path(temporary)  # Hold the owned shell inputs and event log.
                payload, events = directory / 'payload', directory / 'events'  # Model only one disposable installed file.
                scripts = {'install': 'printf "install\\n" >> "$fixture_events"\n: > "$fixture_payload"\n', 'verify': 'printf "verify\\n" >> "$fixture_events"\ntest "$fixture_mode" != verify-fails\n', 'remove': 'printf "remove\\n" >> "$fixture_events"\nif [ "$fixture_mode" != leftover ]; then rm -f "$fixture_payload"; fi\ntest "$fixture_mode" != remove-fails\n', 'absent': 'set -eu\n' + smoke.absence_script([payload])}  # Supply complete harmless install/verify/remove/readback adapters.
                for name, source in scripts.items():  # Materialize adapters without invoking package managers.
                    (directory / (name + '.sh')).write_text(source)  # Every command can touch only quoted fixture paths.
                invoke = lambda name: 'sh ' + shlex.quote(str(directory / (name + '.sh')))  # Keep adapter argv shell-safe.
                script = smoke.managed_script('deb', ':', invoke('install'), invoke('remove'), invoke('verify')).replace('/smoke/absent-deb.sh', shlex.quote(str(directory / 'absent.sh')))  # Substitute the readback location while retaining production transaction control flow.
                if mode == 'foreign':  # Model data that predates this attempted transaction.
                    payload.write_bytes(b'foreign sentinel')  # The ownership preflight must protect it.
                environment = {**os.environ, 'fixture_payload': str(payload), 'fixture_events': str(events), 'fixture_mode': mode}  # Restrict every mutable target to the fixture.
                result = subprocess.run(['sh', '-ec', script], env=environment, capture_output=True, text=True, timeout=10)  # Execute actual trap and exit-status semantics.
                self.assertEqual(result.returncode == 0, mode == 'pass', result.stdout + result.stderr)  # Only the complete transaction qualifies.
                recorded = events.read_text().splitlines() if events.exists() else []  # Retain the observed operation sequence.
                if mode == 'foreign':  # A refused preflight must never install or remove anything.
                    self.assertEqual(recorded, [])  # Ownership must not be armed before absence succeeds.
                    self.assertEqual(payload.read_bytes(), b'foreign sentinel')  # Prove foreign data preservation.
                    continue  # Remaining checks concern this run's attempted installation.
                self.assertEqual(recorded[:2], ['install', 'verify'])  # The actual behavior stage must execute before positive removal.
                self.assertIn('remove', recorded)  # Cleanup must still run after a failed verifier.
                self.assertEqual(payload.exists(), mode == 'leftover')  # Readback detects retained files instead of deleting them to pass.


if __name__ == '__main__':  # Allow a focused portable regression command.
    unittest.main()  # Source fixtures never substitute for native release acceptance.
