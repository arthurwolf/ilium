"""Exercise native smoke ownership using temporary bytes and mocked native adapters."""  # These fixtures are not native qualification.
from contextlib import ExitStack, contextmanager  # Restore every native adapter after each fixture.
import importlib.util  # Load the actual guest controller without executing its CLI.
import json  # Inspect retained production fixture records.
import tarfile  # Supply harmless real Docker-export archive bytes.
import hashlib  # Compute real fixture-file identities without trusting a fake manager.
import os  # Supply isolated environments and inspect only temporary paths.
from pathlib import Path, PurePosixPath  # Retain the real filesystem class when the smoke path adapter is replaced.
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
import linux_container_fixture as container_fixture  # Run the real host custody implementation.
sys.path.insert(0, str(Path(__file__).resolve().parent))  # Support direct and package-style test discovery.
from container_fixture_support import fixture_record  # Never treat synthetic records as native qualification.
guest_specification = importlib.util.spec_from_file_location('tested_container_fixture_guest', root / 'release/packaging/linux/container_fixture_guest.py')  # Read the supplied complete guest implementation.
guest_fixture = importlib.util.module_from_spec(guest_specification)  # Keep this controller instance private to the regression module.
guest_specification.loader.exec_module(guest_fixture)  # Import definitions only; all native execution remains behind explicit function calls.


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


class container_fixture_tests(unittest.TestCase):  # Exercise production orchestration with explicit synthetic OS adapters.
    @contextmanager  # Restore every adapter and discard only test-owned files.
    def host_fixture(self, mode):  # Model one host fixture without invoking Docker, systemd or a native child.
        with tempfile.TemporaryDirectory(prefix='ilium-container-test-') as temporary, ExitStack() as stack:  # Allocate all state before replacing tempfile's adapter.
            base = Path(temporary).resolve()  # Keep every test mutation below this private directory.
            work, inputs, logs = base / 'work', base / 'inputs', base / 'logs'  # Separate supplied inputs from owned runtime state.
            inputs.mkdir()  # Production copytree must copy a real input directory.
            logs.mkdir()  # The real runner contract retains diagnostics here.
            platform_root = base / 'platform'  # Redirect absolute kernel/tool observations to ordinary files.
            def route(*parts):  # Preserve ordinary repository and private-work paths.
                value = Path(*parts)  # Accept the same multi-part constructor calls as pathlib.
                spelling = PurePosixPath(*(str(part).replace('\\', '/') for part in parts)).as_posix()  # Recognize native literals before Windows changes their separators.
                return platform_root / spelling.lstrip('/') if spelling.startswith(('/proc/', '/sys/', '/usr/bin/')) else value  # Never access the live native boundaries.
            for name, content in {'/proc/1/comm': 'systemd\n', '/proc/self/mountinfo': '', '/sys/fs/cgroup/cgroup.controllers': 'cpu memory\n' if mode == 'controllers' else 'cpu memory pids\n'}.items():  # Model required preflight facts independently.
                path = route(name)  # Write only a synthetic platform file.
                path.parent.mkdir(parents=True, exist_ok=True)  # Create private ancestors.
                path.write_text(content, encoding='utf-8')  # Preserve actual read_text behavior in production.
            for name in (*('/usr/bin/' + tool for tool in ('systemd-run', 'systemctl', 'systemd-nspawn', 'docker')), *('/proc/self/ns/' + name for name in container_fixture.namespace_names)):  # Use real file inode identities for harmless observations.
                path = route(name)  # Keep tool and namespace placeholders private.
                path.parent.mkdir(parents=True, exist_ok=True)  # Own every required ancestor.
                path.touch()  # No placeholder is executable or executed.
            state = SimpleNamespace(mode=mode, base=base, work=work, logs=logs, calls=[], created=False, started=False, stopped=False, clock=0.0)  # Track resource custody independently of returned evidence.
            sources = {name: container_fixture.sha(root / name) for name in container_fixture.source_files}  # Use real current implementation hashes.
            stdout = 'ILIUM_ANIMATION_BEGIN\n{}\n{}\n{}\n{}\nILIUM_ANIMATION_END\n'  # Deliberately not animation qualification; this fixture tests the runtime layer only.
            record = fixture_record('deb', 'ubuntu:22.04', 'x86_64', sources, stdout, 'fixture stderr\n')  # Supply complete synthetic guest evidence for the runtime positive case.
            nonce, unit = record['nonce'], record['unit']  # Preserve exact fresh-name relationships.
            group = route('/sys/fs/cgroup/system.slice/' + unit)  # Test the exact fallback used even after partial startup.
            identity = 'd' * 64  # Model a complete immutable Docker ID.
            def allocate(**options):  # Replace only the private-work allocator.
                self.assertEqual(options, {'prefix': 'ilium-container-', 'dir': '/var/tmp'})  # Keep the intended production scope explicit.
                work.mkdir()  # Never reuse a prior test's workspace.
                return str(work)  # All later production filesystem cleanup stays here.
            def pause(seconds):  # Advance simulated deadlines without blocking or changing wall time.
                state.clock += 1200 if state.started and not state.stopped else 5  # Runtime expiry and cleanup expiry remain independently observable.
            def runner(command, log, label, **options):  # Interpret an explicit allowlist of production native commands.
                argv = [str(part) for part in command]  # Retain exact custody-bearing arguments.
                state.calls.append(argv)  # Unexpected mutations remain visible to assertions.
                output, error, code = '', '', 0  # Select every adverse adapter outcome explicitly.
                if argv == ['/usr/bin/systemd-nspawn', '--version']:  # Admit the stated minimum interface only.
                    output = 'systemd 249\n'  # This is a test prerequisite, never runtime qualification.
                elif argv[:len(container_fixture.docker)] == container_fixture.docker:  # All preparation operations use the explicit local daemon.
                    operation = argv[len(container_fixture.docker):]  # Parse only documented command boundaries.
                    if operation[0] == 'pull':
                        self.assertEqual(operation, ['pull', '--platform=linux/amd64', 'ubuntu:22.04'])
                        output = 'synthetic image pull\n'
                    elif operation[0] == 'info':  # Model native daemon capability readback.
                        output = json.dumps({'OSType': 'linux', 'Architecture': 'x86_64', 'CgroupVersion': '2'})  # No remote daemon is contacted.
                    elif operation[0] == 'ps':  # Successful complete inventory is independent of command exit status.
                        output = identity + ' ilium-preparation-' + nonce + '\n' if state.created else ''  # Retain exact identity after a partial create.
                    elif operation[:2] == ['image', 'inspect']:  # Bind the base image before preparation.
                        output = json.dumps([{'Id': 'sha256:' + 'a' * 64, 'Os': 'linux', 'Architecture': 'amd64', 'RepoDigests': ['ubuntu@sha256:' + 'b' * 64], 'Config': {}}])  # No floating replacement image qualifies.
                    elif operation[0] == 'create':  # Arm actual synthetic resource state before returning success or timeout.
                        state.created = True  # Cleanup must discover this resource through inventory.
                        if mode == 'docker-timeout':  # Model a timed-out client after daemon-side creation.
                            code, error = 124, 'partial Docker startup timed out'  # A missing returned ID must not lose custody.
                    elif operation[0] == 'inspect':  # Corroborate name, complete ID and nonce before mutation.
                        output = json.dumps([{'Id': identity, 'Name': '/ilium-preparation-' + nonce, 'Config': {'Labels': {'org.ilium.release.fixture': 'foreign' if mode == 'docker-foreign' else nonce}}, 'State': {'Status': 'exited', 'ExitCode': 0, 'OOMKilled': False}}])  # Foreign labels must forbid force removal.
                    elif operation[0] == 'start':  # Dependency preparation is only a native adapter outcome here.
                        output = 'synthetic dependency preparation\n'  # No installed application output is fabricated.
                    elif operation[0] == 'export':  # Supply a real harmless archive to the production safe extractor.
                        with tarfile.open(operation[1].removeprefix('--output='), 'w'):  # Write only the owned export destination.
                            pass  # An empty rootfs is sufficient for testing private unit installation.
                    elif operation[0] == 'rm':  # Allow removal only of the retained immutable ID.
                        self.assertEqual(operation[-1], identity)  # A name, prefix or unrelated container must not be removed.
                        state.created = False  # Inventory must subsequently prove absence.
                    else:  # No unlisted native command may escape to a real tool.
                        raise AssertionError(argv)  # Keep the adapter fail closed.
                elif argv[:2] == ['/usr/bin/systemctl', 'show']:  # Exercise the real structured unit_state parser.
                    values = dict.fromkeys(container_fixture.unit_keys, '')  # Include every required property exactly once.
                    missing = not state.started or mode == 'collected-live'  # A collected unit can still have a retained physical group.
                    values.update(LoadState='not-found' if missing else 'loaded', ActiveState='inactive' if missing or state.stopped else 'active', SubState='exited' if mode == 'complete' else 'running', Description='foreign' if mode == 'foreign-unit' else 'Ilium distribution fixture ' + nonce, Transient='yes', Delegate='yes', ControlGroup='' if missing or state.stopped else '/system.slice/' + ('foreign.service' if mode == 'foreign-group' else unit), Result='success', ExecMainCode='1', ExecMainStatus='0')  # Model manager state separately from kernel presence.
                    output = ''.join(key + '=' + values[key] + '\n' for key in container_fixture.unit_keys)  # Retain the actual property parsing boundary.
                    code = 4 if missing else 0  # Only explicit not-found evidence permits this nonzero result.
                elif argv[0] == '/usr/bin/systemd-run':  # Model a partial or completed service start.
                    state.started = True  # Arm retained resource state before reporting the startup result.
                    group.mkdir(parents=True)  # No real cgroup is ever created.
                    (group / 'memory.max').write_text('3221225472')  # Keep outer budget readback active in the positive case.
                    (group / 'pids.max').write_text('1024')  # Never alter the untrusted leaf ceilings.
                    control = work / 'rootfs/var/lib/ilium-container-fixture'  # This directory was made by the real install_units function.
                    (control / 'case.stdout').write_text(stdout, encoding='utf-8')  # Retain actual diagnostic bytes through the production recovery path.
                    (control / 'case.stderr').write_text('fixture stderr\n', encoding='utf-8')  # Keep stdout and stderr separate.
                    (control / 'result.json').write_text(json.dumps(record['guest']) + '\n', encoding='utf-8')  # A forged guest pass cannot erase host failure.
                    if mode == 'complete':  # Only the positive runtime fixture reaches host admission.
                        (control / 'ready.json').write_text(json.dumps(record['guest']) + '\n', encoding='utf-8')  # Preserve nonce and namespace fields.
                    if mode in ('start-failure', 'collected-live'):  # The manager may have created a group before the client failed.
                        code, error = 124, 'partial service startup timed out'  # The original failure must remain authoritative after cleanup.
                elif argv[:3] == ['/usr/bin/systemctl', '--no-block', 'stop']:  # Cleanup may address only this fixture's exact service.
                    self.assertEqual(argv[3:], [unit])  # No broad stop or foreign unit mutation is allowed.
                    state.stopped = True  # Manager inactivity does not imply kernel disappearance.
                    if mode != 'cleanup-live':  # Retain a physical group to discriminate cleanup from successful stop.
                        shutil.rmtree(group)  # Remove only ordinary test-owned files.
                elif argv[0] == '/usr/bin/journalctl':  # Preserve bounded exact-unit diagnostic recovery.
                    self.assertIn('--unit=' + unit, argv)  # Do not collect another service's journal.
                    output = 'synthetic retained journal\n'  # The runner must log this independently of application output.
                else:  # Reject any unexpected host operation.
                    raise AssertionError(argv)  # Nothing falls back to the operating system.
                with (Path(log) / (label + '.log')).open('a', encoding='utf-8') as stream:  # Honor the production runner's retained-diagnostics contract.
                    stream.write(repr(argv) + '\n' + output + error + '\n')  # Preserve failed startup and cleanup observations.
                return subprocess.CompletedProcess(argv, code, output, error)  # Let real production control flow interpret the outcome.
            stack.enter_context(patch.object(container_fixture, 'Path', side_effect=route))  # Redirect every native path constructor.
            stack.enter_context(patch.object(container_fixture.os, 'geteuid', return_value=0, create=True))  # Model only the explicit root prerequisite.
            stack.enter_context(patch.object(container_fixture.os, 'access', return_value=True))  # The tool placeholders are never executed.
            stack.enter_context(patch.object(container_fixture.os, 'pidfd_open', return_value=90, create=True))  # Admission itself is exercised separately below.
            stack.enter_context(patch.object(container_fixture.subprocess, 'run', side_effect=AssertionError('unadapted native execution')))  # Never fall through to a real command.
            stack.enter_context(patch.object(container_fixture.subprocess, 'Popen', side_effect=AssertionError('unadapted native child')))  # Retain a fail-closed process boundary.
            stack.enter_context(patch.object(Path, 'symlink_to', autospec=True, side_effect=lambda path, target, **options: path.write_text(str(target), encoding='utf-8')))  # Model private rootfs link creation without requiring Windows symlink privileges.
            stack.enter_context(patch.object(container_fixture.platform, 'system', return_value='Linux'))  # Keep the runtime preflight deterministic.
            stack.enter_context(patch.object(container_fixture.platform, 'machine', return_value='x86_64'))  # Refuse accidental cross-architecture assumptions.
            stack.enter_context(patch.object(container_fixture.uuid, 'uuid4', return_value=SimpleNamespace(hex=nonce)))  # Make exact resource identities assertable.
            stack.enter_context(patch.object(container_fixture.tempfile, 'mkdtemp', side_effect=allocate))  # Allocate only inside the test scope.
            stack.enter_context(patch.object(container_fixture.time, 'monotonic', side_effect=lambda: state.clock))  # Bound every loop with simulated time.
            stack.enter_context(patch.object(container_fixture.time, 'sleep', side_effect=pause))  # Never perform a blocking wait.
            stack.enter_context(patch.object(container_fixture, 'admit_guest', return_value=record['host_admission']))  # Replace only kernel admission for this orchestration test; separate tests run that function.
            state.result, state.record = container_fixture.run_fixture('ubuntu:22.04', 'deb', 'x86_64', base, inputs, logs, 'lane', 'false # synthetic transaction; never executed\n', 'fixture.deb', runner, sources)  # Run the real complete allocation, preparation, cleanup and evidence flow.
            yield state  # Assert custody before the outer test scope removes any retained failures.

    @unittest.skipUnless(os.name == 'posix', 'POSIX bind-path fixture required; all real Linux acceptance lanes remain mandatory')  # A Windows drive colon is intentionally invalid in the production Linux bind grammar.
    def test_host_runtime_success_and_each_failure_preserve_custody(self):  # Require failure-specific behavior, not source-string matching.
        modes = ('complete', 'controllers', 'docker-timeout', 'docker-foreign', 'start-failure', 'timeout', 'foreign-unit', 'foreign-group', 'collected-live', 'cleanup-live')  # Cover prerequisites, partial startup, expiry, substitution and incomplete retirement.
        for mode in modes:  # Keep each failure independent of the positive fixture.
            with self.subTest(mode=mode), self.host_fixture(mode) as state:  # All native operations remain explicit adapters.
                self.assertEqual(state.result.returncode, int(mode != 'complete'), state.record)  # A guest pass never hides host failure.
                self.assertEqual(state.record['state'], 'passed' if mode == 'complete' else 'failed')  # Exit status and retained state agree.
                stops = [call for call in state.calls if call[:3] == ['/usr/bin/systemctl', '--no-block', 'stop']]  # Inspect actual cleanup calls.
                retained = mode in ('docker-foreign', 'foreign-unit', 'foreign-group', 'collected-live', 'cleanup-live')  # These cases have unretired or unowned resources.
                self.assertEqual(state.work.exists(), retained)  # Never delete live or foreign-owned state to manufacture absence.
                self.assertEqual(state.record['cleanup']['state_removed'], not retained)  # Report actual filesystem custody truthfully.
                self.assertEqual(bool(stops), mode in ('complete', 'start-failure', 'timeout', 'cleanup-live'))  # Never stop a foreign or positively absent unit.
                if retained:  # A remaining scope must be discoverable without guessing its path.
                    self.assertEqual(state.record['retained_work'], str(state.work))  # Preserve failed evidence for its owner.
                if state.started:  # Startup and timeout evidence must survive service cleanup.
                    self.assertIn('ILIUM_ANIMATION_BEGIN', (state.logs / 'lane-case.stdout.log').read_text())  # Preserve actual recovered stdout bytes.
                    self.assertIn('fixture stderr', (state.logs / 'lane-case.stderr.log').read_text())  # Preserve stderr separately.
                    self.assertIn('synthetic retained journal', (state.logs / 'lane.log').read_text())  # Retain the owned unit's diagnostics too.
                if mode in ('start-failure', 'timeout', 'cleanup-live', 'collected-live'):  # Primary failure must remain visible after all teardown attempts.
                    self.assertIn('timed out', '\n'.join(state.record['errors']))  # Successful cleanup cannot erase the original timeout.
                if mode in ('cleanup-live', 'collected-live'):  # Inactive or missing manager state alone cannot prove retirement.
                    self.assertFalse(state.record['cleanup']['cgroup_absent'])  # Require physical kernel absence independently.
                if mode == 'docker-timeout':  # A partial create still belongs to the exact nonce-labelled daemon object.
                    self.assertIn(container_fixture.docker + ['rm', '--force', 'd' * 64], state.calls)  # Recover by complete ID despite no successful create response.
                if mode == 'docker-foreign':  # Matching names cannot establish ownership.
                    self.assertFalse(any('rm' in call for call in state.calls))  # Preserve the foreign container rather than force-removing it.

    def test_structured_manager_errors_are_never_absence(self):  # Exercise actual property parsing instead of mocked state dictionaries.
        for stdout, status in (('', 1), ('LoadState=not-found\n', 4), (''.join(key + '=\n' for key in container_fixture.unit_keys), 1)):  # Include missing, partial and nonzero complete readbacks.
            runner = Mock(return_value=subprocess.CompletedProcess([], status, stdout, 'manager unavailable'))  # No system manager is contacted.
            with self.subTest(status=status, stdout=stdout), self.assertRaises(ValueError):  # Error prose must not qualify an absent unit.
                container_fixture.unit_state('ilium-container-fixture.service', runner, Path('unused'), 'unused')  # Execute the real parser.

    def test_enumeration_is_incremental_in_the_admission_path(self):  # An eager list or recursive glob must fail this discriminating regression.
        with tempfile.TemporaryDirectory() as temporary:  # Supply only one ordinary owned root.
            owned = Path(temporary)  # No live cgroup is inspected.
            observed, closed, opened = [], [], []  # Record consumption and iterator custody.
            def entries():  # Permit exactly the directory count needed to detect overflow.
                for index in range(2048):  # The root already consumes one of the 2048 allowed directories.
                    observed.append(index)  # Count actual next() consumption, not a final materialized length.
                    yield SimpleNamespace(name='child-' + str(index), is_symlink=lambda: False, is_dir=lambda **options: True)  # Every returned child consumes directory budget.
                raise AssertionError('enumerated past the first over-limit directory')  # Eager exhaustion must not pass by failing later.
            @contextmanager  # Require iterator closure even when admission rejects the tree.
            def scan(parent):  # All children must be rejected or queued before any descent.
                opened.append(parent)  # Detect an attempted traversal past the root's overflowing enumeration.
                try:  # Preserve closure on the expected ValueError.
                    yield entries()  # Do not materialize the poison iterator in the fixture.
                finally:  # The production context manager must exit on rejection.
                    closed.append(parent)  # Retain the observed cleanup.
            with patch.object(container_fixture.os, 'scandir', side_effect=scan), patch.object(Path, 'rglob', side_effect=AssertionError('recursive glob bypassed bounded enumeration')), patch.object(container_fixture.os, 'pidfd_open', create=True) as pidfd:  # No actual process handle may be acquired after overflow.
                with self.assertRaisesRegex(ValueError, 'tree is unbounded'):  # A post-materialization AssertionError or unrelated filesystem error must fail the test.
                    container_fixture.admit_guest(owned, owned, {'namespace': {'namespaces': dict.fromkeys(container_fixture.namespace_names, 1)}}, {})  # Exercise the actual corrected production caller.
            self.assertEqual(len(observed), 2048)  # Stop at the first excess directory, before any further next().
            self.assertEqual(opened, [owned])  # Never traverse an unadmitted child.
            self.assertEqual(closed, [owned])  # Reject without leaking the active iterator.
            pidfd.assert_not_called()  # Resource overflow must precede PID admission.

    def test_nonmatching_entries_are_bounded_and_small_tree_is_complete(self):  # A lazy rglob still permits unbounded searching between matches.
        with tempfile.TemporaryDirectory() as temporary:  # Keep all positive filesystem observations real and private.
            owned = Path(temporary)  # Count this root once.
            (owned / 'one/two').mkdir(parents=True)  # Exercise nested traversal rather than only a flat list.
            expected = {owned / 'cgroup.procs', owned / 'one/cgroup.procs', owned / 'one/two/cgroup.procs'}  # Independent exact inventory.
            for file in expected:  # Materialize the actual matching leaves.
                file.write_text('', encoding='utf-8')  # No real process belongs to these fixture directories.
            self.assertEqual(set(container_fixture.owned_cgroup_files(owned, directory_limit=3, entry_limit=5)), expected)  # Accept exactly the allowed boundary without duplication or omission.
            count, closed = [], []  # Observe every nonmatching entry and iterator exit.
            def controls():  # Model arbitrarily many non-directory control entries with no matching descendants.
                for index in range(9):  # An eight-entry limit requires only the ninth item to reject.
                    count.append(index)  # Detect overconsumption independently of return values.
                    yield SimpleNamespace(name='control-' + str(index), is_symlink=lambda: False, is_dir=lambda **options: False)  # Nonmatching entries still consume observation budget.
                raise AssertionError('nonmatching traversal exceeded its bound')  # Eager or match-only enumeration cannot satisfy the test.
            @contextmanager  # Model a closeable directory iterator.
            def scan(parent):  # No real directories are scanned in the overflow case.
                try:  # Ensure the failure path closes this iterator.
                    yield controls()  # Preserve laziness in the adapter itself.
                finally:  # Production must exit the context on rejection.
                    closed.append(parent)  # Record the actual close boundary.
            with patch.object(container_fixture.os, 'scandir', side_effect=scan), self.assertRaisesRegex(ValueError, 'entry enumeration'):  # A final-list size check cannot detect this case correctly.
                container_fixture.owned_cgroup_files(owned, entry_limit=8)  # Tighten only the internal observation budget.
            self.assertEqual(len(count), 9)  # Consume no more than the first over-limit entry.
            self.assertEqual(closed, [owned])  # No iterator remains live after rejection.
            redirected = SimpleNamespace(name='foreign', is_symlink=lambda: True, is_dir=Mock(side_effect=AssertionError('followed a symlink')))  # Do not require OS symlink privileges on portable test runners.
            with patch.object(container_fixture.os, 'scandir') as scan:  # Keep link custody at the directory-entry adapter boundary.
                scan.return_value.__enter__.return_value = iter([redirected])  # Return one hostile entry lazily.
                with self.assertRaisesRegex(ValueError, 'symlink'):  # Refuse it before classification or descent.
                    container_fixture.owned_cgroup_files(owned)  # Never inspect a foreign subtree.

    def test_live_admission_binds_namespaces_rootfs_and_owned_cgroup(self):  # Run the actual admission function against a read-only synthetic kernel view.
        for mode in ('pass', 'shared-namespace', 'misreported-namespace', 'foreign-mount', 'foreign-membership', 'wrong-rootfs', 'wrong-map', 'exited'):  # Vary each independent custody fact.
            with self.subTest(mode=mode), ExitStack() as stack:  # Restore the kernel adapters after each attempt.
                owned = '/sys/fs/cgroup/system.slice/ilium-container-fixture.service'  # Use the actual unified path shape.
                namespace = {name: index + 100 for index, name in enumerate(container_fixture.namespace_names)}  # Distinct guest observations.
                host_namespace = {name: value + 100 for name, value in namespace.items()}  # Independently distinct host observations.
                observed = {'namespace': {'namespaces': dict(namespace), 'uid_map': [[0, 524288, 65536]], 'gid_map': [[0, 524288, 65536]], 'cgroup_mount_inode': 77}}  # Model a ready guest's claims.
                text = {owned + '/cgroup.procs': '', owned + '/init.scope/cgroup.procs': '1701\n', '/proc/1701/status': 'NSpid:\t1701\t1\n', '/proc/1701/comm': 'systemd\n', '/proc/1701/uid_map': '0 524288 65536\n', '/proc/1701/gid_map': '0 524288 65536\n', '/proc/1701/cgroup': '0::/system.slice/ilium-container-fixture.service/init.scope\n'}  # Kernel membership selects init; no PID is guessed by the implementation.
                inodes = {owned: 77, owned + '/init.scope': 78, '/fixture-root': 88, '/proc/1701/root': 88, '/proc/1701/root/sys/fs/cgroup': 77, **{'/proc/1701/ns/' + name: value for name, value in namespace.items()}}  # Tie visible cgroup and rootfs to independent inode observations.
                if mode == 'shared-namespace':  # Matching guest/host reports still cannot admit a shared host namespace.
                    inodes['/proc/1701/ns/cgroup'] = observed['namespace']['namespaces']['cgroup'] = host_namespace['cgroup']  # Preserve report agreement while removing isolation.
                if mode == 'misreported-namespace':  # A self-reported distinct namespace is insufficient.
                    inodes['/proc/1701/ns/cgroup'] = 999  # The host observes different kernel bytes.
                if mode == 'foreign-mount':  # A writable whole-host or sibling mount must not match the bounded owned inventory.
                    inodes['/proc/1701/root/sys/fs/cgroup'] = 999  # Retain correct namespace claims but change mount identity.
                if mode == 'foreign-membership':  # Init must remain below the exact service that owns the fixture.
                    text['/proc/1701/cgroup'] = '0::/system.slice/foreign.service/init.scope\n'  # Refuse an unrelated subtree.
                if mode == 'wrong-rootfs':  # Guest PID 1 must execute from the exported rootfs being qualified.
                    inodes['/proc/1701/root'] = 999  # Preserve other namespace observations.
                if mode == 'wrong-map':  # Host corroboration must agree with the guest's UID mapping.
                    text['/proc/1701/uid_map'] = '0 589824 65536\n'  # A self-reported mapping alone cannot qualify.
                class observed_path(PurePosixPath):  # Keep path arithmetic real while replacing kernel file reads.
                    def read_text(self):  # No live procfs or cgroup file is read.
                        return text[str(self)]  # Unknown paths fail the fixture immediately.
                    def stat(self):  # Return independent synthetic inode and device observations.
                        return SimpleNamespace(st_ino=inodes[str(self)], st_dev=1)  # Foreign paths cannot silently gain a matching identity.
                    def is_symlink(self):  # This positive topology has no redirected ancestors.
                        return False  # Entry-level link rejection is separately exercised above.
                    def is_dir(self):  # Admit only the two explicitly owned directories.
                        return str(self) in (owned, owned + '/init.scope')  # Ancestors and siblings are not traversal candidates.
                @contextmanager  # Model one closeable read-only directory iterator.
                def scan(parent):  # Refuse traversal outside the declared owned topology.
                    self.assertIn(str(parent), (owned, owned + '/init.scope'))  # Catch any parent or sibling scan.
                    entries = [SimpleNamespace(name='cgroup.procs', is_symlink=lambda: False, is_dir=lambda **options: False)]  # Count the ordinary control entry.
                    if str(parent) == owned:  # Only the owned root contains one child scope.
                        entries.append(SimpleNamespace(name='init.scope', is_symlink=lambda: False, is_dir=lambda **options: True))  # Keep the bounded traversal active.
                    yield iter(entries)  # The implementation consumes entries incrementally.
                poller = Mock()  # No real file descriptor or process is polled.
                poller.poll.return_value = [(90, 1)] if mode == 'exited' else []  # A dead pidfd must prevent admission.
                stack.enter_context(patch.object(container_fixture, 'Path', observed_path))  # Preserve pure path equality and ancestry comparisons.
                stack.enter_context(patch.object(container_fixture.os, 'scandir', side_effect=scan))  # Replace the read-only kernel enumeration boundary.
                pidfd = stack.enter_context(patch.object(container_fixture.os, 'pidfd_open', return_value=90, create=True))  # Retain one explicit synthetic handle.
                close = stack.enter_context(patch.object(container_fixture.os, 'close'))  # Never close a real process-owned descriptor.
                stack.enter_context(patch.object(container_fixture.select, 'poll', return_value=poller, create=True))  # Permit portable source-suite execution.
                stack.enter_context(patch.object(container_fixture.select, 'POLLIN', 1, create=True))  # Supply the unavailable Windows constant only within this adapter.
                if mode == 'pass':  # Establish the actual function's complete positive custody path.
                    admitted = container_fixture.admit_guest(observed_path('/fixture-root'), observed_path(owned), observed, host_namespace)  # No admission or validation function is mocked here.
                    self.assertEqual(admitted['visible_cgroup_root'], owned)  # Require the independently matched owned mount.
                    self.assertEqual(admitted['init_pid'], 1701)  # Bind the selected process to cgroup and NSpid evidence.
                else:  # Every changed native observation is independently disqualifying.
                    with self.assertRaises(ValueError):  # None may be repaired by adopting a broader scope.
                        container_fixture.admit_guest(observed_path('/fixture-root'), observed_path(owned), observed, host_namespace)  # Keep all production custody checks active.
                pidfd.assert_called_once_with(1701, 0)  # Pin only the process discovered inside the owned tree.
                close.assert_called_once_with(90)  # Both success and rejection release the retained handle.

    def test_guest_missing_manager_and_partial_start_keep_failure_and_cleanup(self):  # Exercise the actual guest main flow before package execution.
        for mode in ('absent-bus', 'partial-start', 'retained-group', 'cleanup-error'):  # Isolate startup and retirement outcomes.
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary, ExitStack() as stack:  # Own every redirected guest file.
                base = Path(temporary)  # No host user runtime directory is ever touched.
                control = base / 'state'  # Real atomic result files remain inspectable.
                control.mkdir()  # Model the fixture-authored control directory.
                def route(value):  # Redirect every absolute guest path below this test root.
                    return base / str(value).lstrip('/')  # Production main only constructs explicit guest absolute paths here.
                contents = {'/smoke/fixture.json': json.dumps({'nonce': 'a' * 32, 'os_release': {'ID': 'ubuntu', 'VERSION_ID': '22.04'}}), '/etc/os-release': 'ID=ubuntu\nVERSION_ID="22.04"\n'}  # Supply real immutable guest inputs.
                for name in ('/usr/bin/systemd-run', '/usr/bin/systemctl', '/usr/lib/systemd/systemd', '/usr/bin/bwrap', '/usr/bin/ffmpeg', '/usr/lib/systemd/user/dbus.socket'):  # Keep prerequisite existence distinct from manager readiness.
                    contents[name] = ''  # Placeholders are never executed.
                for name, value in contents.items():  # Create only the exact synthetic guest inputs.
                    path = route(name)  # Confine all bytes to this test.
                    path.parent.mkdir(parents=True, exist_ok=True)  # Own each necessary ancestor.
                    path.write_text(value, encoding='utf-8')  # Leave real filesystem readback enabled.
                route('/run').mkdir()  # Guest main creates only the runtime's immediate parent.
                group = route('/sys/fs/cgroup/system.slice') / guest_fixture.user_unit  # Exercise the same partial-start fallback as production.
                if mode == 'retained-group':  # Model a manager which reports inactive while its kernel domain remains.
                    group.mkdir(parents=True)  # Never create a real cgroup.
                calls, clock = [], [0.0]  # Record exact guest systemctl actions and simulated deadlines.
                def run(command, **options):  # Replace the only commands permitted before readiness.
                    calls.append(command)  # Preserve order and exact service identity.
                    self.assertIn(command, [['/usr/bin/systemctl', '--no-block', action, guest_fixture.user_unit] for action in ('start', 'stop')])  # No package or user command may run without a ready bus.
                    if mode == 'partial-start' and command[2] == 'start':  # Arm cleanup before startup reports failure.
                        raise ValueError('partial user manager startup')  # Preserve the primary failure distinctly.
                    if mode == 'cleanup-error' and command[2] == 'stop':  # Successful readiness waiting cannot hide teardown errors.
                        raise ValueError('user manager stop failed')  # The failed cleanup remains an independent cause.
                    return ''  # No native executable runs.
                def pause(seconds):  # Expire waits deterministically without sleeping.
                    clock[0] += 10  # Keep readiness and cleanup deadlines independent.
                stack.enter_context(patch.object(guest_fixture, 'Path', side_effect=route))  # Redirect guest absolute paths only.
                stack.enter_context(patch.object(guest_fixture, 'state_root', control))  # Keep result and stream writes in the owned control directory.
                stack.enter_context(patch.object(guest_fixture, 'namespace_record', return_value={}))  # This test begins after namespace admission; the next test checks it directly.
                stack.enter_context(patch.object(guest_fixture.os, 'geteuid', return_value=0, create=True))  # Model container root without changing credentials.
                stack.enter_context(patch.object(guest_fixture.os, 'uname', return_value=SimpleNamespace(machine='x86_64'), create=True))  # Preserve native architecture checking portably.
                stack.enter_context(patch.object(guest_fixture.os, 'chown', create=True))  # No real credential or ownership mutation is authorized.
                stack.enter_context(patch.object(guest_fixture, 'run', side_effect=run))  # Replace only system manager command execution.
                stack.enter_context(patch.object(guest_fixture, 'properties', return_value={'ActiveState': 'inactive'}))  # Cleanup still has to check the physical group independently.
                stack.enter_context(patch.object(guest_fixture.time, 'monotonic', side_effect=lambda: clock[0]))  # Bound readiness and teardown loops.
                stack.enter_context(patch.object(guest_fixture.time, 'sleep', side_effect=pause))  # No blocking sleeps occur.
                execute = stack.enter_context(patch.object(guest_fixture.subprocess, 'run', side_effect=AssertionError('package ran without a user manager')))  # Missing capability can never fall through to acceptance.
                self.assertEqual(guest_fixture.main(), 1)  # Every missing-manager scenario must fail.
                result = json.loads((control / 'result.json').read_text())  # Inspect actual atomically written production evidence.
                self.assertEqual(result['state'], 'failed')  # Never relabel missing capability as a skip or pass.
                self.assertIsNone(result['case_exit_code'])  # No package acceptance ran.
                self.assertEqual([command[2] for command in calls], ['start', 'stop'])  # Partial startup still requires exact owned cleanup.
                self.assertEqual(result['user_manager_retired'], mode in ('absent-bus', 'partial-start'))  # An inactive state word cannot hide a retained domain or stop error.
                self.assertFalse((control / 'ready.json').exists())  # No admission request precedes complete manager capability.
                self.assertTrue(result['errors'])  # Preserve the actual missing-readiness or startup diagnosis.
                execute.assert_not_called()  # Version output and package execution cannot replace a manager.

    def test_guest_requires_writable_unified_controllers_and_owned_probe(self):  # Test guest capability guards before any cgroup write.
        self.assertEqual((guest_fixture.memory_bytes, guest_fixture.maximum_tasks), (402653184, 16))  # Freeze the original untrusted ceilings independently of fixture overhead.
        for mount_root, options, controllers in (('/', 'rw', 'cpu memory'), ('/foreign', 'rw', 'cpu memory pids'), ('/', 'ro', 'cpu memory pids')):  # Missing controllers, wrong namespace root and read-only mounts are independent failures.
            records = {'/proc/1/comm': 'systemd\n', '/proc/self/mountinfo': '1 0 0:1 ' + mount_root + ' /sys/fs/cgroup ' + options + ' - cgroup2 cgroup rw\n', '/sys/fs/cgroup/cgroup.controllers': controllers}  # Supply only reads before the required failure.
            def path(value):  # Reject unexpected filesystem activity during capability admission.
                return SimpleNamespace(read_text=lambda: records[str(value)])  # Unknown paths fail the test rather than reaching the host.
            with self.subTest(root=mount_root, options=options, controllers=controllers), patch.object(guest_fixture, 'Path', side_effect=path), patch.object(guest_fixture.os, 'stat', return_value=SimpleNamespace(st_ino=100)), patch.object(guest_fixture, 'properties') as properties:  # Model only read-only kernel observations.
                with self.assertRaises(ValueError):  # A missing capability is not an empty successful observation.
                    guest_fixture.namespace_record()  # Exercise the actual topology/controller guards.
                properties.assert_not_called()  # Do not reach manager or package admission after the failed kernel prerequisite.
        parent = SimpleNamespace(name='ilium-fixture-probe-test.service', stat=lambda: SimpleNamespace(st_uid=0))  # Model a matching name owned by the wrong UID.
        with patch.object(guest_fixture.os, 'geteuid', return_value=65534, create=True), patch.object(guest_fixture, 'membership', return_value=parent), patch.object(guest_fixture, 'Path', return_value=SimpleNamespace(read_text=lambda: '0::/foreign\n')), patch.object(guest_fixture.subprocess, 'Popen') as child:  # Keep the failed ownership proof entirely read-only.
            with self.assertRaisesRegex(ValueError, 'name or owner'):  # Root ownership cannot satisfy non-root delegation.
                guest_fixture.delegated_check(parent.name)  # Refuse before moving any process or enabling a controller.
            child.assert_not_called()  # Never start an untrusted or sacrificial child in a foreign domain.



if __name__ == '__main__':  # Support isolated local verification as well as suite discovery.
    unittest.main()  # Run only these safe fixture tests.
