"""Exercise native smoke ownership using temporary bytes and mocked native adapters."""  # These fixtures are not native qualification.
from contextlib import ExitStack, contextmanager  # Restore every native adapter after each fixture.
import hashlib  # Compute real fixture-file identities without trusting a fake manager.
import os  # Supply isolated environments and inspect only temporary paths.
from pathlib import Path  # Retain the real filesystem class when the smoke path adapter is replaced.
import shutil  # Populate and remove only owned fixture trees.
import signal  # Verify the retained-child interruption contract without signaling a process.
import subprocess  # Construct adapter results and run only explicitly created shell fixtures.
import sys  # Import the repository's production smoke module.
import tempfile  # Own every fixture directory.
from types import SimpleNamespace  # Keep explicit native-adapter state together.
import unittest  # Integrate with the existing portable release test discovery.
from unittest.mock import Mock, patch  # Replace managers, mounts, process creation and account lookup.

root = Path(__file__).resolve().parents[2]  # Use the same repository-root convention as existing release tests.
sys.path.insert(0, str(root / 'release/scripts'))  # Import actual production helpers rather than copying their logic.
import smoke_linux_packages as smoke  # The module must remain importable on non-POSIX source-test runners.
import release_tool  # Bind exact official animation bytes and helper version.


class native_smoke_tests(unittest.TestCase):  # Native actions are represented exclusively by explicit fixture adapters.
    @contextmanager  # Keep generated files and all patches bounded to one test.
    def fixture(self, package_format, **overrides):  # Run real host flow against owned temporary filesystem state.
        with tempfile.TemporaryDirectory(prefix='ilium-native-fixture-') as temporary, ExitStack() as stack:  # Never touch a real installation.
            base = Path(temporary).resolve()  # Resolve system temporary-directory aliases before strict path checks.
            home = base / 'home'  # Make Path.home point at a fresh disposable account home.
            home.mkdir()  # Preserve every real account directory.
            members = {'ilium': b'fixture-client', 'ilium-server': b'fixture-server', 'ilium-animation-helper': b'fixture-helper', **{name: (root / 'ilium-animation-js/assets/packages' / name).read_bytes() for name in release_tool.APPROVED_PACKAGES}, 'libfixture.so.1': b'fixture-runtime', 'VERSION': b'0.1.0\n', 'THIRD-PARTY.txt': b'fixture-notices'}  # Hash actual distinguishable members.
            artifact = base / smoke.packages.package_name('x86_64', package_format)  # Preserve production artifact-name selection.
            artifact.write_bytes(b'fixture-package')  # No executable package bytes are provided.
            receipt = {'version': '0.1.0', 'tag': 'v0.1.0', 'arch': 'x86_64', 'source_archive_sha256': 'a' * 64, 'package_files': {name: hashlib.sha256(data).hexdigest() for name, data in members.items()}, 'packages': {artifact.name: hashlib.sha256(artifact.read_bytes()).hexdigest()}}  # Bind every executed fixture readback.
            state = SimpleNamespace(base=base, home=home, artifact=artifact, receipt=receipt, calls=[], events=[], animation_calls=[], installed=False, removed=False, keep_removed=False, bad_hash=False, fail_sandbox=False, fail_animation=False, query_error=False, missing_fuse=False, lingering_mount=False, mount_active=False, process_alive=True, require_privileged_store=True)  # Model required positive and adverse native outcomes.
            state.delayed_unmount = False
            state.mount_observations_after_stop = 0
            state.snapshots = ['foreign-retained']  # Model an unrelated existing snapshot which teardown must retain.
            for name, value in overrides.items():  # Alter only the requested forcing condition.
                setattr(state, name, value)  # Keep positive behavior identical across negative cases.
            state.mount = base / 'virtual/snap'  # Redirect both documented Snap mount prefixes to one owned tree.
            state.store = base / 'virtual/store'  # Never read the machine's real root-owned Snap blobs.
            state.foreign_data = home / 'snap/ilium'  # Represent prior account data with safe real files.
            state.fuse = base / 'fuse'  # Use an ordinary placeholder and mock only the kernel prerequisite result.
            if not state.missing_fuse:  # Leave the missing-device case observably absent.
                state.fuse.touch()  # Do not create a device node.
            if getattr(state, 'prior_data', False):  # Prepare the foreign-state preservation regression.
                state.foreign_data.mkdir(parents=True)  # This tree must never be adopted or deleted by the smoke.
                (state.foreign_data / 'authored.txt').write_text('preserve me', encoding='utf-8')  # Use a distinguishable foreign-content sentinel.
            routes = {'/var/lib/snapd/snaps': state.store, '/var/lib/snapd/snap': state.mount, '/var/snap': base / 'virtual/data', '/snap': state.mount, '/dev/fuse': state.fuse}  # Redirect only native installation/device paths.
            def route(value):  # Leave all ordinary repository and temporary paths untouched.
                raw = str(value)  # Native literals arrive before Path normalizes platform-specific separators.
                for prefix, replacement in routes.items():  # Prefer longer declared prefixes through insertion order.
                    if raw == prefix or raw.startswith(prefix + '/'):  # Keep sibling paths outside the selected prefix distinct.
                        return replacement / raw[len(prefix):].lstrip('/')  # Read and write only beneath the fixture root.
                return Path(value)  # Retain ordinary pathlib behavior everywhere else.
            path_adapter = Mock(side_effect=route)  # Supply the existing Path-constructor seam without changing production APIs.
            path_adapter.home.return_value = home  # Ensure host_case creates its owned workspace beneath the fixture home.
            def make_layout(selected, directory):  # Create every expected installed leaf with real bytes.
                directory.mkdir(parents=True, exist_ok=True)  # Own the full generated layout.
                for relative, target in smoke.expected_layout(selected, receipt).items():  # Reuse only the production inventory, not verification results.
                    entry = directory / relative  # Keep the intended package-relative path.
                    entry.parent.mkdir(parents=True, exist_ok=True)  # Build required structural parents.
                    if target is not None:  # Model the builder's real relative launchers.
                        try:  # Windows source runners may lack permission to create symbolic links.
                            entry.symlink_to(target)  # Never point outside the owned package tree.
                        except (OSError, NotImplementedError) as error:  # Report an unavailable fixture capability without pretending native success.
                            self.skipTest('fixture symlinks unavailable: ' + str(error))  # These tests remain safe on restricted source runners.
                        continue  # Do not hash through declared launcher links.
                    data = members.get(entry.name, b'fixture-integration')  # Keep every audited member distinct and deterministic.
                    if relative == '.ref':  # Flatpak creates an empty deployment lock marker.
                        data = b''  # Preserve the strict manager-marker contract.
                    if relative == 'meta/snap.yaml':  # Use the supplied exact Snap command and architecture metadata.
                        data = smoke.packages.render_snap_yaml('0.1.0', 'x86_64').encode()  # Avoid invented manager fields.
                    entry.write_bytes(data)  # Real hash comparisons still execute in the production verifier.
                return smoke.layout_reference(selected, directory, receipt)  # Require the positive fixture to pass complete layout admission.
            def package_reference(selected, path, candidate_receipt, architecture, destination):  # Replace only external package unpacking.
                self.assertEqual((path, candidate_receipt, architecture), (artifact, receipt, 'x86_64'))  # Preserve exact input binding at the adapter boundary.
                return make_layout(selected, Path(destination))  # Admit real temporary members through the production layout verifier.
            original_sha = smoke.packages.sha  # Preserve real hashing for every user-readable fixture member.
            def fixture_sha(path):  # Force the actual root-only blob access constraint without changing file modes.
                if state.require_privileged_store and state.store in Path(path).parents:  # A successful Snap gate must use privileged readback for this path.
                    raise PermissionError('snapd stores installed blobs root-owned with mode 0600')  # Reproduce the known positive-path defect portably.
                return original_sha(path)  # Exercise actual core, notice and ancillary hashes.
            def checked(command, log, label, **options):  # Model native adapters while leaving verification and transaction logic intact.
                argv = [str(part) for part in command]  # Preserve the exact attempted native argv for assertions.
                state.calls.append((argv, dict(options.get('env', {}))))  # Retain installation selection and privilege context.
                selected = argv[2:] if argv[:2] == ['sudo', '-n'] else argv  # Interpret only the smoke's existing sudo prefix.
                result = subprocess.CompletedProcess(argv, 0, '', '')  # Every failure below is explicitly selected and reported.
                if selected[:2] == ['snap', 'wait']:  # Model a ready disposable snapd instance.
                    return result  # No real service readiness command runs.
                if selected[:2] == ['snap', 'list']:  # Return the documented manager inventory shape.
                    result.stdout = 'Name Version Rev Tracking Publisher Notes\n' + ('ilium 0.1.0 x1 - fixture classic\n' if state.installed else '')  # Bind mounted revision selection.
                    return result  # Production parsing remains active.
                if selected[:2] == ['snap', 'install']:  # Materialize only this fixture's installed mount and stored blob.
                    state.events.append('install')  # Verify preflight occurs before any mutation.
                    make_layout('snap', state.mount / 'ilium/x1')  # Leave payload hash readback real.
                    try:  # Windows source runners may disallow the manager's directory-link fixture.
                        (state.mount / 'ilium/current').symlink_to('x1', target_is_directory=True)  # Model snapd's active revision link.
                    except (OSError, NotImplementedError) as error:  # Do not mistake missing fixture privileges for a native package failure.
                        self.skipTest('fixture symlinks unavailable: ' + str(error))  # Preserve portable source-test execution.
                    state.store.mkdir(parents=True, exist_ok=True)  # Own the simulated snapd blob store.
                    (state.store / 'ilium_x1.snap').write_bytes(artifact.read_bytes())  # Store exactly the receipt-bound input bytes.
                    if state.bad_hash:  # An altered installed image must fail even with a successful privileged read.
                        (state.store / 'ilium_x1.snap').write_bytes(b'changed installed image')  # Preserve the original input artifact and expected digest.
                    state.installed = True  # Subsequent manager queries now report the owned revision.
                    return result  # Actual snapd never runs.
                if selected[:2] == ['snap', 'remove']:  # Model successful manager removal independently of its readback.
                    state.events.append('remove')  # Preserve teardown ordering evidence.
                    if '--purge' not in selected:  # snapd saves an automatic data snapshot on normal uninstall.
                        state.snapshots.append('owned-ilium')  # Reflect the documented manager side effect, independently of path removal.
                    state.installed, state.removed = False, True  # Manager success alone must not establish absence.
                    shutil.rmtree(state.mount / 'ilium')  # Remove only the owned simulated mounted tree.
                    (state.store / 'ilium_x1.snap').unlink()  # Remove only the owned installed blob.
                    return result  # The production absence phase still has to run.
                if selected[:2] == ['flatpak', 'list']:  # Model exact application, architecture and branch columns.
                    if state.query_error and state.removed:  # A failed post-uninstall query must never mean absent.
                        raise smoke.release_tool.ReleaseError('fixture Flatpak inventory failed')  # Keep manager availability separate from registration.
                    result.stdout = '%s x86_64 master\n' % smoke.packages.APP_ID if state.installed else ''  # Omit nonexistent application rows.
                    return result  # Production inventory validation remains active.
                if selected[:2] == ['flatpak', 'remote-add']:  # Model dependency-source configuration inside the private installation.
                    return result  # Never add a real remote.
                if selected[:2] == ['flatpak', 'install']:  # Materialize a private deployment using the actual pinned user directory.
                    state.events.append('install')  # Record manager mutation after preflight.
                    state.user_directory = Path(options['env']['FLATPAK_USER_DIR'])  # The smoke chooses this owned location.
                    state.deployment = state.user_directory / 'app' / smoke.packages.APP_ID / 'x86_64/master/fixture-commit'  # Represent one resolved deployed commit.
                    make_layout('flatpak', state.deployment / 'files')  # Real inventory and digest checks run later.
                    (state.deployment / 'metadata').write_text(smoke.packages.flatpak_metadata('x86_64'), encoding='utf-8')  # Preserve actual runtime and command metadata.
                    if state.bad_hash:  # Force a byte-level rejection after otherwise successful installation.
                        (state.deployment / 'files/lib/ilium/ilium').write_bytes(b'changed')  # Only the installed client differs from the receipt.
                    state.installed = True  # Make the manager query agree with the owned deployment.
                    return result  # The production verify callback reads these actual files.
                if selected[:2] == ['flatpak', 'info']:  # Return the documented deployed-location readback.
                    result.stdout = str(state.deployment) + '\n'  # Production containment and resolution checks remain active.
                    return result  # No guessed native commit path is consumed by production code.
                if selected[:2] == ['flatpak', 'uninstall']:  # Separate a successful manager exit from actual deployment disposal.
                    self.assertEqual(selected[-1], 'app/%s/x86_64/master' % smoke.packages.APP_ID)  # Require the exact owned app ref.
                    state.events.append('remove')  # Verify all functional failures still trigger ownership-limited cleanup.
                    state.installed, state.removed = False, True  # The list query becomes empty even in the retained-deployment case.
                    if state.keep_removed:  # Model Flatpak's still-locked removed deployment area.
                        removed = state.user_directory / '.removed/fixture-commit'  # Retained package bytes must block successful qualification.
                        removed.parent.mkdir()  # Own this synthetic manager staging area.
                        shutil.move(str(state.deployment), removed)  # Keep real payload files available for the authoritative check.
                        return result  # A zero manager exit does not justify deleting this evidence.
                    shutil.rmtree(state.deployment)  # Remove only the owned committed deployment.
                    return result  # Empty structural parents may remain legitimately.
                if selected[:2] == ['sh', '-ec']:  # Model read-only privileged filesystem probes.
                    script = selected[-1]  # Interpret only the test's safe adapter boundary.
                    if 'sha256sum' in script and str(state.store) in script:  # The Snap blob is accessible only through privileged readback.
                        self.assertEqual(argv[:2], ['sudo', '-n'])  # A plain user read must not qualify this store.
                        stored = state.store / 'ilium_x1.snap'  # Hash the exact owned revision bytes.
                        result.stdout = hashlib.sha256(stored.read_bytes()).hexdigest() + '  ' + str(stored) + '\n'  # Return real data, not a preset expected digest.
                        return result  # The production digest parser still validates the result.
                    if state.foreign_data.exists():  # Preserve prior account state discovered by the production account inventory.
                        self.assertIn(str(state.foreign_data), script)  # Verify the actual account path reaches the read-only probe.
                        raise smoke.release_tool.ReleaseError('fixture prior Snap account data exists')  # No install or uninstall may follow.
                    return result  # Absence of the owned fixture paths is the modeled manager probe result.
                if selected[-1:] == ['--version']:  # Model successful or blocked native execution after actual installed-byte checks.
                    if state.fail_sandbox:  # Missing sandbox capability must fail even with correct deployed bytes.
                        raise smoke.release_tool.ReleaseError('fixture sandbox execution unavailable')  # Teardown must still run.
                    server = 'server' in selected[0] or 'ilium.server' in selected  # Distinguish both installed command expectations.
                    helper = any('helper' in part for part in selected)  # Recognize direct, Snap and Flatpak helper execution.
                    result.stdout = release_tool.helper_version_record('0.1.0') + '\n' if helper else ('ilium-server' if server else 'ilium') + ' 0.1.0\n'  # Return exact version strings.
                    return result  # Production expect_version validation stays active.
                if selected[:1] == ['sh'] and Path(selected[1]).name == 'lifecycle.sh':  # Replace native application execution, not the lifecycle source file.
                    state.events.append('lifecycle')  # Confirm the real host flow reaches its lifecycle adapter.
                    if getattr(state, 'fail_lifecycle', False):  # A failed behavioral gate must remain failed after successful uninstall.
                        raise smoke.release_tool.ReleaseError('fixture lifecycle failed')  # Exercise the actual host cleanup and result path.
                    return result  # The existing independent shell suite exercises the lifecycle itself.
                raise AssertionError('unexpected native adapter call: ' + repr(argv))  # Never fall through to a real native tool.
            process = Mock(pid=424242)  # A retained in-memory child handle is the only signal target.
            process.poll.side_effect = lambda: None if state.process_alive else 0  # Model live and reaped process states explicitly.
            def wait(timeout):  # Complete only the owned mock mount's shutdown.
                state.process_alive = False  # Return a reaped process without any OS signaling.
                state.mount_active = state.lingering_mount or state.delayed_unmount  # Kernel unmount completion can lag child retirement.
                return 0  # Successful wait alone does not qualify cleanup.
            process.wait.side_effect = wait  # Preserve the retained-child teardown API.
            def popen(command, stdout, stderr, env):  # Replace all AppImage execution with an owned mount fixture.
                state.events.append('mount')  # Record that FUSE prerequisites were passed first.
                state.appdir = base / 'appdir'  # Keep the fake mounted tree separate from host_case's owned cache scope.
                make_layout('appimage', state.appdir)  # Exercise real mounted-payload hashes and namespace checks.
                stdout.write(str(state.appdir) + '\n')  # Supply the documented mountpoint report.
                stdout.flush()  # Let the production readiness poll observe it immediately.
                state.mount_active = True  # Expose the declared kernel readback only while modeled mounted.
                self.assertNotIn('APPIMAGE_EXTRACT_AND_RUN', env)  # Inherited extraction overrides cannot satisfy a host FUSE gate.
                return process  # Retain custody of exactly this mock handle.
            fake_pwd = SimpleNamespace(getpwall=lambda: [SimpleNamespace(pw_dir=str(home))])  # Supply account data without importing POSIX modules here.
            stack.enter_context(patch.dict(sys.modules, {'pwd': fake_pwd}))  # Support production's portable local Linux-only import.
            for target, value, create in ((smoke, {'Path': path_adapter, 'checked': checked, 'package_reference': package_reference, 'pwd': fake_pwd}, True), (smoke.os, {'geteuid': lambda: 1000}, True), (smoke.platform, {'system': lambda: 'Linux', 'platform': lambda terse=False: 'fixture-Linux'}, False), (smoke.packages, {'sha': fixture_sha}, False), (smoke.stat, {'S_ISCHR': lambda mode: True}, False), (smoke.shutil, {'which': lambda name: '/fixture/' + name}, False), (smoke.subprocess, {'Popen': popen}, False)):  # Patch only explicit platform and native-adapter seams.
                stack.enter_context(patch.multiple(target, create=create, **value))  # Restore original shared modules on every exit.
            def kernel_mount_records():  # Model the native log's asynchronous mount-table transition.
                if not state.process_alive and state.mount_active and state.delayed_unmount:
                    state.mount_observations_after_stop += 1
                    if state.mount_observations_after_stop >= 3:
                        state.mount_active = False
                return [(state.appdir, 'fuse.fixture')] if state.mount_active else []
            stack.enter_context(patch.object(smoke, 'mount_records', side_effect=kernel_mount_records))  # No real mount is read or changed.
            def installed_animation(*arguments):
                state.animation_calls.append(arguments[4])
                if state.fail_animation:
                    raise smoke.release_tool.ReleaseError('fixture installed animation failed')
                proof_path = Path(arguments[2]) / (arguments[3] + '-installed-animation.json')
                proof_path.write_text('{"synthetic_fixture":true}\n', encoding='utf-8')
                return {'state': 'passed', 'synthetic': True}
            stack.enter_context(patch.object(smoke, 'installed_animation', side_effect=installed_animation))
            stack.enter_context(patch.object(smoke, 'emit', side_effect=lambda kind, **values: setattr(state, 'result', values)))  # Inspect the real host result after its cleanup gates.
            audit_path = base / 'native-audit.json'
            audit_path.write_text('{}\n', encoding='utf-8')
            state.arguments = SimpleNamespace(packages=base, arch='x86_64',
                                              flatpak_user_dir=None,
                                              audit_report=audit_path,
                                              workspace=root / 'Cargo.toml')  # Preserve source and audit identity for the terminal host result.
            state.run = lambda: getattr(smoke, 'host_' + package_format)(state.arguments, receipt, base / 'logs')  # Execute actual host_case and transaction code.
            state.process = process  # Allow teardown assertions without exposing a real PID.
            yield state  # Tests assert results while preserved adverse evidence still exists.

    def test_snap_prior_account_data_prevents_install_and_uninstall(self):  # Existing author-owned data must never be adopted.
        with self.fixture('snap', prior_data=True) as state:  # Return an empty manager inventory alongside real prior data.
            self.assertEqual(state.run(), 1)  # Ownership failure must fail requested qualification.
            self.assertEqual(state.events, [])  # Neither native installation nor removal may run.
            self.assertEqual((state.foreign_data / 'authored.txt').read_text(), 'preserve me')  # Retain the authored bytes exactly.

    def test_snap_positive_uses_privileged_blob_readback(self):  # Root-only snapd blobs must not make every valid gate fail.
        for bad_hash in (False, True):  # Pair the root-owned positive path with an actual installed-image hash mismatch.
            with self.subTest(bad_hash=bad_hash), self.fixture('snap', bad_hash=bad_hash) as state:  # Direct user hash reads deliberately raise PermissionError in both cases.
                self.assertEqual(state.run(), int(bad_hash), getattr(state, 'result', {}))  # Require privileged readback and the exact installed-image digest.
                self.assertEqual(state.events, ['install', 'remove'] if bad_hash else ['install', 'lifecycle', 'remove'])  # Reject changed installed images before application execution.
                self.assertTrue(state.result['removed'])  # Require successful manager removal and final absence independently.

    def test_snap_teardown_leaves_no_new_snapshot_and_preserves_existing_snapshots(self):
        for failed_lifecycle in (False, True):
            with self.subTest(failed_lifecycle=failed_lifecycle), self.fixture('snap', fail_lifecycle=failed_lifecycle) as state:
                self.assertEqual(state.run(), int(failed_lifecycle), getattr(state, 'result', {}))
                self.assertTrue(state.result['removed'])
                self.assertEqual(state.snapshots, ['foreign-retained'])

    def test_flatpak_positive_and_installed_corruption(self):  # Real temporary hashes distinguish a valid deployment from altered bytes.
        for bad_hash in (False, True):  # Keep a positive path beside the rejection path.
            with self.subTest(bad_hash=bad_hash), self.fixture('flatpak', bad_hash=bad_hash) as state:  # Rebuild fresh private state for each case.
                self.assertEqual(state.run(), int(bad_hash), getattr(state, 'result', {}))  # Wrong installed bytes must fail despite successful manager callbacks.
                self.assertEqual(state.events[-1], 'remove')  # Functional rejection still requires cleanup.
                self.assertTrue(state.result['removed'])  # Preserve failure separately from successful teardown.

    def test_flatpak_retained_removed_deployment_fails_and_is_preserved(self):  # Manager success and an empty list are insufficient.
        with self.fixture('flatpak', keep_removed=True) as state:  # Move real installed bytes into the manager's retained-deployment area.
            self.assertEqual(state.run(), 1)  # Authoritative leftover readback must disqualify the result.
            self.assertFalse(state.result['removed'])  # Do not report a verified uninstall.
            self.assertTrue((state.user_directory / '.removed/fixture-commit/files/lib/ilium/ilium').is_file())  # Preserve failed-removal evidence instead of erasing it.

    def test_flatpak_missing_sandbox_and_failed_absence_query_fail(self):  # Correct installed bytes cannot replace required execution or readback.
        for field in ('fail_sandbox', 'query_error'):  # Test independent native prerequisites and cleanup uncertainty.
            with self.subTest(field=field), self.fixture('flatpak', **{field: True}) as state:  # Change only one adapter outcome per transaction.
                self.assertEqual(state.run(), 1)  # Both error classes must fail the requested format.
                self.assertIn('remove', state.events)  # Preserve cleanup after functional failure.

    def test_installed_animation_failure_blocks_each_host_format_and_still_removes(self):
        for package_format in ('snap', 'flatpak', 'appimage'):
            with self.subTest(package_format=package_format), self.fixture(package_format, fail_animation=True) as state:
                self.assertEqual(state.run(), 1)
                self.assertEqual(state.animation_calls, [package_format])
                self.assertTrue(state.result['removed'])

    def test_appimage_missing_fuse_never_starts_a_mount(self):  # Extract-and-run must not substitute for a required host mount.
        with self.fixture('appimage', missing_fuse=True) as state:  # Keep the private device placeholder absent.
            self.assertEqual(state.run(), 1)  # A missing FUSE prerequisite is disqualifying.
            self.assertEqual(state.events, [])  # No mount or application execution may be attempted.
            state.process.send_signal.assert_not_called()  # No unowned process may be signaled during setup failure.

    def test_failed_host_lifecycle_remains_failed_after_successful_removal(self):  # Preserve behavioral failure independently of cleanup success.
        for package_format in ('snap', 'flatpak', 'appimage'):  # Exercise each actual host adapter and shared transaction.
            with self.subTest(format=package_format), self.fixture(package_format, fail_lifecycle=True) as state:  # Change only the native lifecycle adapter outcome.
                self.assertEqual(state.run(), 1)  # Successful byte checks and removal must not qualify a failed lifecycle.
                self.assertIn('lifecycle', state.events)  # The failure must come from actual host lifecycle wiring.
                self.assertEqual(state.result['gates']['verify'], 'failed')  # Preserve the primary behavior failure in JSONL evidence.
                self.assertTrue(state.result['removed'])  # Require independent successful manager/mount teardown.

    def test_appimage_waits_for_kernel_unmount_after_its_child_is_reaped(self):
        with self.fixture('appimage', delayed_unmount=True) as state, patch.object(smoke.time, 'sleep'):
            self.assertEqual(state.run(), 0, getattr(state, 'result', {}))
            self.assertGreaterEqual(state.mount_observations_after_stop, 3)
            self.assertTrue(state.result['removed'])
            self.assertFalse(state.process_alive)

    def test_appimage_positive_and_dead_child_with_lingering_mount(self):  # Kernel readback remains independent of child reap.
        for lingering in (False, True):  # Pair successful removal with the observed false-success risk.
            with self.subTest(lingering=lingering), self.fixture('appimage', lingering_mount=lingering) as state, patch.object(smoke.time, 'sleep'), patch.object(smoke.time, 'monotonic', side_effect=iter(range(100))):  # Use the same real temporary package tree.
                self.assertEqual(state.run(), int(lingering), getattr(state, 'result', {}))  # A lingering mount must fail even after the child exits.
                state.process.send_signal.assert_called_once_with(signal.SIGINT)  # Interrupt only the retained mock child.
                state.process.wait.assert_called_once_with(timeout=10)  # Require explicit bounded reap.
                self.assertEqual(state.result['removed'], not lingering)  # Removal evidence must reflect mount-table state.

    @unittest.skipUnless(os.name == 'posix' and shutil.which('sh'), 'POSIX shell fixture required')  # No native Flatpak installation is needed.
    def test_flatpak_wrapper_pins_manager_environment_and_forwards_lifecycle_state(self):  # Execute the actual generated wrapper against a harmless recorder.
        with tempfile.TemporaryDirectory(prefix='ilium-flatpak-wrapper-') as temporary:  # Own every command and captured byte.
            base = Path(temporary).resolve()  # Avoid symlinked system temporary prefixes.
            recorder = base / 'flatpak'  # PATH selects only this explicitly created shell fixture.
            recorder.write_text('#!/bin/sh\nset -eu # Fail recording errors.\nprintf "%s\\n" "$FLATPAK_USER_DIR" "$HOME" "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR" "$@" > "$FIXTURE_CAPTURE" # Record launcher environment and exact argv.\n', encoding='utf-8')  # Never invoke the real Flatpak executable.
            recorder.chmod(0o755)  # Permit only the fixture's intended execution.
            keys = ('HOME', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR', 'TMPDIR')  # Cover state overwritten by lifecycle and required by the launcher.
            fixed = {key: str(base / ('host-' + key.lower())) for key in keys}  # Keep manager-side state independent of the current invocation.
            environment = dict(os.environ, PATH=str(base) + os.pathsep + os.environ.get('PATH', ''), FIXTURE_CAPTURE=str(base / 'capture'))  # Preserve only ordinary shell runtime discovery.
            environment.update({key: str(base / ('lifecycle-' + key.lower())) for key in keys})  # Model the real lifecycle's replaced state directories.
            wrapper = base / 'wrapper.sh'  # Execute the complete production-generated wrapper source.
            wrapper.write_text(smoke.flatpak_wrapper(base / 'private-install', fixed, 'x86_64'), encoding='utf-8')  # Use the real application identity and branch.
            result = subprocess.run(['sh', str(wrapper), '--cwd', '/fixture-project', 'ls'], capture_output=True, text=True, env=environment, timeout=10)  # Run only the owned wrapper and recorder.
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)  # Require the fixture itself to execute successfully.
            rows = (base / 'capture').read_text().splitlines()  # Inspect actual environment/argv expansion rather than source substrings.
            self.assertEqual(rows[:4], [str(base / 'private-install'), fixed['HOME'], fixed['XDG_DATA_HOME'], fixed['XDG_RUNTIME_DIR']])  # Pin launcher installation and session context.
            self.assertEqual(rows[4:8], ['run', '--user', '--arch=x86_64', '--branch=' + smoke.packages.FLATPAK_BRANCH])  # Select the exact private native application variant.
            for key in keys:  # Every intended client state value must cross the sandbox boundary explicitly.
                self.assertIn('--env=' + key + '=' + environment[key], rows)  # Preserve lifecycle values while fixing launcher-side environment.
            self.assertEqual(rows[-4:], [smoke.packages.APP_ID, '--cwd', '/fixture-project', 'ls'])  # Preserve the exact application and original command arguments.


if __name__ == '__main__':  # Support isolated local verification as well as suite discovery.
    unittest.main()  # Run only these safe fixture tests.
