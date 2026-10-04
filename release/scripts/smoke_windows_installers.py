"""Audited EXE/MSI smoke; native execution requires an exclusive disposable account."""  # Keep this gate separate from public-bootstrap acceptance.
from __future__ import annotations  # Permit portable import of native-only adapters.
import argparse  # Describe the existing parser's namespace.
import json  # Retain machine-readable evidence.
import os  # Read environment and durable filesystem state.
from pathlib import Path  # Keep all execution and evidence paths explicit.
import platform  # Record the actual native host.
import stat  # Reject links and non-regular payload entries.
import struct  # Refuse a WOW64 Python process for the x64 gate.
import time  # Bound readiness and removal waits.
import uuid  # Give every pane a fresh unpredictable marker.
import build_windows_installers as package  # Reuse unchanged producer identities and hashing.
import release_tool  # Reuse the supplied audit/archive validators.
import smoke_installed_animation as animation_gate  # Parse the installed helper's real IPC proof.
from windows_account import windows_account  # Read actual account registry and known folders.
from windows_job import windows_job  # Own only suspended-before-assignment descendants.
#
require = package.require  # Preserve the existing release error API.
uninstaller_names = {'unins000.exe', 'unins000.dat'}  # A clean Inno installation has one explicit uninstaller pair.
#
def plain(path: Path, *, directory: bool = False, missing: bool = False) -> Path:  # Reject reparse aliases before following any input.
    path = path.absolute()  # Do not use resolve(), which would hide a link.
    for ancestor in (path, *path.parents):  # Check the complete ancestry, including the leaf.
        try:  # A new owned leaf can legitimately be absent.
            entry = ancestor.lstat()  # Inspect the link itself.
        except FileNotFoundError:  # Absence does not authorize deletion or replacement.
            continue  # Existing ancestors are still checked.
        require(not stat.S_ISLNK(entry.st_mode) and not getattr(entry, 'st_file_attributes', 0) & 0x400, 'link/reparse path: ' + str(ancestor))  # Refuse junctions too.
    if missing and not path.exists():  # Admit an absent destination only when requested.
        return path  # No directory has been created yet.
    require(path.is_dir() if directory else path.is_file(), 'not a plain ' + ('directory: ' if directory else 'file: ') + str(path))  # Require the correct leaf kind.
    return path  # Return the checked, absolute spelling.
#
class journal:  # Keep acceptance evidence outside the installer inventory.
    def __init__(self, root: Path) -> None:  # Reserve a fresh evidence directory.
        self.root = plain(root, directory=True, missing=True)  # Validate ancestors without resolving them away.
        require(not self.root.exists(), '--log must be a new task-owned directory')  # Never append to another run.
        self.root.mkdir()  # The explicit existing parent remains untouched.
        self.path = self.root / 'smoke.jsonl'  # Match the existing always-retained workflow directory.
        self.path.touch(exist_ok=False)  # Reserve one journal for this run.
    def record(self, kind: str, **values: object) -> None:  # Persist observations before reporting them.
        record = {'type': kind, **values}  # Every row carries an explicit evidence category.
        with self.path.open('a', encoding='utf-8', newline='\n') as output:  # Preserve JSONL framing on Windows.
            output.write(json.dumps(record, ensure_ascii=True, sort_keys=True) + '\n')  # Retain complete structured values.
            output.flush()  # Push Python buffers before reporting progress.
            os.fsync(output.fileno())  # A success row must have reached the filesystem.
        release_tool.emit(record)  # Keep stdout compatible with the existing JSONL CLI.
#
def attempt(label: str, action, failures: list[str], evidence: journal):  # Run independent cleanup even after another cleanup fails.
    try:  # Preserve the failure's own phase.
        value = action()  # Execute only the explicitly supplied owned action/readback.
        evidence.record('cleanup', label=label, state='completed')  # A completed action is not package acceptance.
        return value  # Allow a readback to be used by subsequent cleanup.
    except BaseException as error:  # Also preserve cleanup on interruption.
        failures.append(label + ': ' + type(error).__name__ + ': ' + str(error) + '; '.join(getattr(error, '__notes__', [])))  # Retain nested cleanup notes too.
        try:  # A journal failure must not prevent other cleanup attempts.
            evidence.record('cleanup', label=label, state='failed', error=failures[-1])  # Retain the failure when possible.
        except BaseException as logging_error:  # Treat lost evidence as another failure.
            failures.append('cleanup journal: ' + str(logging_error))  # Never silently accept missing evidence.
        return None  # The caller must not treat this as a successful readback.
#
def finish(primary: BaseException | None, failures: list[str]) -> None:  # Preserve the primary exception object and traceback.
    if primary is not None:  # A successful compensating action cannot erase a failure.
        if failures:  # Keep secondary failures visible without replacing the primary.
            primary.add_note('cleanup failures: ' + '; '.join(failures))  # Python 3.11 is already the workflow baseline.
        raise primary  # Let the unchanged release CLI retain its normal failure exit.
    require(not failures, '; '.join(failures))  # Cleanup/readback failure alone also blocks acceptance.
#
def wait_until(predicate, seconds: int, message: str) -> None:  # Poll only bounded task-owned conditions.
    deadline = time.monotonic() + seconds  # Clock changes cannot extend the deadline.
    while not predicate():  # Require an actual affirmative readback.
        require(time.monotonic() < deadline, message)  # Missing or late evidence fails closed.
        time.sleep(0.1)  # Avoid a busy polling loop.
#
def bind_inputs(arguments: argparse.Namespace, evidence: journal) -> dict:  # Validate custody before account mutation.
    import validate_animation_smoke as animation_seal  # Share the release sealer's complete source inventory.
    version = package.version_from_tag(arguments.tag)  # Preserve MSI-compatible version rules.
    installers = plain(arguments.installers, directory=True)  # Refuse aliased input directories.
    require(package.inventory_matches(installers), 'installer directory inventory differs')  # Keep the schema-1 producer contract.
    paths = {'archive': plain(arguments.archive), 'audit': plain(arguments.audit_report), 'manifest': plain(arguments.manifest), 'receipt': plain(installers / package.RECEIPT_NAME)}  # Name each independent authority.
    paths.update({name: plain(installers / name) for name in package.INSTALLER_NAMES})  # Check both original installer files.
    hashes = {name: package.sha(path) for name, path in paths.items()}  # Capture the bytes being qualified.
    target = release_tool.selected_target(paths['manifest'], 'x86_64-pc-windows-msvc')  # Use the existing five-target policy.
    require(paths['archive'].name == target['archive'], 'Windows archive basename differs')  # Bind the explicit retained artifact.
    audit = release_tool.audit_receipt(paths['audit'], target, version, arguments.tag)  # Require passed native audit and Windows runtime provenance.
    content = release_tool.read_archive(paths['archive'], target, audit)  # Reject malformed, extra, reordered or hidden members.
    release_tool.verify_content(content, audit, version)  # Verify every audited payload byte and VERSION.
    files = audit['files']  # This complete map is the installed-byte authority.
    require(all(package.MEMBER_PATTERN.fullmatch(name) for name in files) and len({name.casefold() for name in files}) == len(files), 'unsafe/case-colliding Windows payload')  # Reject Windows aliases before installation.
    require(files.get('onnxruntime.dll') is not None and files['onnxruntime.dll'] == audit['windows_ort'].get('built_runtime_sha256'), 'audited Windows runtime is missing or differs from provenance')  # Reject a partial runtime receipt too.
    require(sorted(name for name in files if name.endswith('.dll')) == audit['dependency_closure'].get('bundled'), 'audited runtime closure differs from payload')  # Bind the producer's complete bundled list.
    receipt = release_tool.read_json(paths['receipt'])  # Reject duplicate JSON keys using the supplied reader.
    require(receipt.get('schema') == 1 and receipt.get('tag') == arguments.tag and receipt.get('version') == version and receipt.get('source_archive') == target['archive'], 'installer receipt identity differs')  # Use existing producer fields exactly.
    require(receipt.get('source_archive_sha256') == hashes['archive'] and receipt.get('package_files') == files, 'installer receipt differs from audited archive')  # Bind installers to the native payload.
    require(receipt.get('installers') == {name: hashes[name] for name in package.INSTALLER_NAMES}, 'installer bytes differ from receipt')  # Refuse missing, extra or tampered installer hashes.
    require(all(package.sha(path) == hashes[name] for name, path in paths.items()), 'input changed during validation')  # Detect a changed validation input.
    source_files = animation_seal.source_hashes(Path(__file__).resolve().parents[2])
    evidence.record('binding', tag=arguments.tag, version=version, paths={name: str(path) for name, path in paths.items()}, sha256=hashes, package_files=files, source_files=source_files)  # Retain the exact authority chain.
    return {'version': version, 'paths': paths, 'hashes': hashes, 'files': files,
            'source_files': source_files}  # Keep native run source provenance.
#
def inventory(directory: Path, files: dict[str, str], kind: str) -> dict[str, str]:  # Rehash the complete flat installation.
    plain(directory, directory=True)  # An installed junction is never accepted.
    entries = list(directory.iterdir())  # Include unexpected entries in the inventory check.
    expected = set(files) | (uninstaller_names if kind == 'exe' else set())  # Only Inno's generated metadata is outside the audit.
    require({path.name for path in entries} == expected, kind + ' installed inventory differs')  # Missing runtime/server and extra directories both fail.
    observed = {path.name: package.sha(plain(path)) for path in entries}  # Reject links and directories before hashing.
    require(all(observed[name] == digest for name, digest in files.items()), kind + ' installed payload hash differs')  # Hash every runtime and both executables.
    return observed  # Generated uninstaller hashes are retained as transaction custody.
#
def allowed_paths(original: dict, directory: Path) -> list[dict]:  # Recognize only the supplied templates' narrow PATH transitions.
    value = original['value'] if original['exists'] else ''  # Preserve absence separately from an empty string.
    require(isinstance(value, str), 'user Path must be a string value')  # Do not coerce another registry type.
    separator = ';' if value and not value.endswith(';') else ''  # Match the source's literal append behavior.
    appended = {value + separator + str(directory), value + separator + str(directory) + '\\'}  # MSI can append a trailing backslash.
    removed = value[:-1] if value.endswith(';') else value  # Inno's known removal can lose one final empty segment.
    return [original, *({'exists': True, 'type': kind, 'value': text} for kind in (1, 2) for text in appended | {value, removed})]  # Only these exact states may be compensated.
#
def check_path(original: dict, installed: dict, directory: Path) -> None:  # Keep identity normalization separate from raw preservation.
    value = original['value'] if original['exists'] else ''  # Do not collapse the original typed snapshot.
    separator = ';' if value and not value.endswith(';') else ''  # Preserve every existing token and delimiter.
    expected = {value + separator + str(directory), value + separator + str(directory) + '\\'}  # Allow the two supplied installer spellings.
    require(installed['exists'] and installed['type'] in (1, 2) and installed['value'] in expected, 'install changed existing PATH text or did not append exactly once')  # No subset or membership-only acceptance.
#
def isolated_environment(root: Path, directory: Path, system: Path) -> dict[str, str]:  # Reuse the supplied native isolation variables.
    root.mkdir()  # Every lifecycle uses a new project and state root.
    environment = {name.upper(): value for name, value in os.environ.items() if not name.upper().startswith(('ILIUM_', 'ORT_', 'DYLD_', 'LD_'))}  # Remove inherited overrides and normalize Windows variable names.
    names = ('HOME', 'USERPROFILE', 'LOCALAPPDATA', 'APPDATA', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_RUNTIME_DIR', 'XDG_BIN_HOME', 'ILIUM_CONFIG_DIR', 'ILIUM_AGENT_SETUP_HOME', 'ILIUM_DEBUG_LOG_DIR', 'TEMP', 'TMP')  # Keep writes task-local.
    for name in names:  # Allocate every overridden directory explicitly.
        destination = root / name.lower()  # Names are fixed and cannot escape the owned root.
        destination.mkdir()  # Refuse stale state instead of reusing it.
        environment[name] = str(destination)  # Apply overrides only to child processes.
    environment.update(PATH=str(directory) + ';' + str(system), COMSPEC=str(system / 'cmd.exe'), SHELL=str(system / 'cmd.exe'), HF_HUB_OFFLINE='1', TERM='xterm-256color', COLORTERM='truecolor')  # Exclude checkout/build tools from executable resolution.
    return environment  # No persistent environment variable is changed.
#
def checked_run(job: windows_job, command: list[str], environment: dict[str, str], cwd: Path, evidence: journal, label: str, timeout: int = 60) -> dict:  # Retain every command outcome before validating it.
    evidence.record('command-start', label=label, command=command, cwd=str(cwd), timeout_seconds=timeout)  # Record intent before process creation.
    result = job.run(command, environment, cwd, timeout)  # The helper owns each process before its first instruction.
    evidence.record('command-exit', label=label, **result)  # Raw readbacks are independent from validation.
    require(result['returncode'] == 0, label + ' exited ' + str(result['returncode']))  # Restart-required and installer failure codes remain failures.
    return result  # Callers enforce their specific output contract.
#
def probe(directory: Path, binding: dict, kind: str, label: str, evidence: journal, system: Path, transaction: dict) -> None:  # Check both versions and a complete installed lifecycle.
    root = evidence.root / label  # Retain isolated state with this lifecycle's evidence.
    environment = isolated_environment(root, directory, system)  # No prior project/session state is reused.
    project = root / 'project'  # The supported --cwd contract supplies the session identity.
    project.mkdir()  # Ensure the native CLI sees an absolute existing project.
    evidence.record('execution-scope', label=label, project=str(project), path=environment['PATH'], source_resolution='absolute-installed-images-and-system-only-PATH', physical_source_hiding=False)  # Do not overclaim filesystem isolation.
    job, server, pane, primary, failures = windows_job(evidence.root, label), None, None, None, []  # Hold custody through final retirement.
    try:  # Always release this private job, including partial startup.
        observed = inventory(directory, binding['files'], kind)  # Recheck installed bytes immediately before execution.
        evidence.record('installed-files', label=label, files=observed)  # Separate payload from generated metadata by its names.
        for name in ('ilium.exe', 'ilium-server.exe', 'ilium-animation-helper.exe'):  # Every installed image must report its release identity.
            result = checked_run(job, [str(directory / name), '--version'], environment, project, evidence, label + '-' + name)  # Never resolve an executable through source PATH.
            require(release_tool.version_identity(name, result['stdout'], binding['version']) == name.removesuffix('.exe') + ' ' + binding['version'] and not result['stderr'], 'wrong exact installed version: ' + name)  # Parse the helper JSONL and reject false identities.
            require(job.active_count() == 0, '--version left a descendant running')  # Version probing must not start a hidden service.
        animation_command = [str(directory / 'ilium.exe'), 'release-animation-probe']
        animation_result = checked_run(job, animation_command, environment, project, evidence,
                                       label + '-animation', timeout=300)
        require(not animation_result['stderr'] and job.active_count() == 0,
                'installed animation probe left diagnostics or owned descendants')
        catalogue, renders = animation_gate.parse_probe_output(animation_result['stdout'])
        require(catalogue.get('client_path') == str(directory / 'ilium.exe') and
                catalogue.get('helper_path') == str(directory / 'ilium-animation-helper.exe') and
                catalogue.get('client_sha256') == observed['ilium.exe'] and
                catalogue.get('helper_sha256') == observed['ilium-animation-helper.exe'] and
                all(row['archive_sha256'] == observed[next(name for name in release_tool.APPROVED_PACKAGES
                                                             if name.startswith(row['package'] + '-'))]
                    for row in renders), 'installed animation did not use the audited client, helper and archives')
        evidence.record('installed-animation', label=label, command=animation_command,
                        stdout_sha256=release_tool.digest(animation_result['stdout'].encode('utf-8')),
                        catalogue=catalogue, renders=renders, job_empty=True)
        marker, nonce = project / 'pane-marker.txt', uuid.uuid4().hex  # The shell must produce this run's marker.
        command = [str(directory / 'ilium.exe'), '--cwd', str(project)]  # Use only the packet's public CLI.
        checked_run(job, command + ['new-pane', '--', str(system / 'cmd.exe'), '/d', '/q', '/k', 'echo ' + nonce + '>pane-marker.txt'], environment, project, evidence, label + '-new-pane')  # /k leaves the real Windows shell alive in its PTY.
        server = job.hold_image(directory / 'ilium-server.exe')  # Acquire a live original handle after job-membership verification.
        wait_until(lambda: marker.is_file(), 30, 'pane did not create its marker')  # Startup/readiness must produce actual evidence.
        require(plain(marker).read_text(encoding='ascii').strip() == nonce, 'pane marker differs')  # A stale or wrong project cannot pass.
        pane = job.hold_image(system / 'cmd.exe')  # Require the system shell inside this private job.
        require(server.alive() and pane.alive(), 'server or pane exited before readiness')  # Marker creation alone is insufficient.
        evidence.record('owned-processes', label=label, server=server.identity, pane=pane.identity, server_sha256=package.sha(directory / 'ilium-server.exe'), marker=nonce)  # Record handle-bound native identity.
        listed = checked_run(job, command + ['ls'], environment, project, evidence, label + '-ls')  # Retain the supported human-readable listing unchanged.
        require(bool(listed['stdout'].strip()) and server.alive() and pane.alive(), 'installed listing/liveness failed')  # Do not invent an undocumented listing parser.
        checked_run(job, command + ['kill-session', 'default'], environment, project, evidence, label + '-kill-session')  # Only an established task-owned session may be retired.
        require(server.wait(30) and pane.wait(30), 'original server or pane did not exit')  # Wait on retained handles, never a later PID lookup.
        evidence.record('retirement-readback', label=label, server=server.identity, pane=pane.identity, original_handles_signaled=True)  # Record original identity before releasing references.
        server.close()  # Keep exited-process references from delaying job accounting.
        pane.close()  # Both original exit observations remain in retained evidence.
        wait_until(lambda: job.active_count() == 0, 10, 'session left owned descendants running')  # Graceful success requires the complete job to empty.
        require(inventory(directory, binding['files'], kind) == observed, 'installed files changed during lifecycle')  # Include uninstaller custody in the within-run invariant.
        evidence.record('lifecycle', label=label, state='passed', original_server_exited=True, original_pane_exited=True, job_empty=True)  # This row follows graceful readbacks only.
    except BaseException as error:  # Preserve any partial-lifecycle failure.
        primary = error  # Cleanup cannot replace this primary error.
    finally:  # Never signal a foreign process by PID or image name.
        attempt(label + '-job-retirement', job.terminate_and_close, failures, evidence)  # Force only the private job on failure.
        for owned in (server, pane):  # Retained process handles outlive PID reuse.
            if owned is not None:  # Partial startup may not have acquired both handles.
                attempt(label + '-handle-close', owned.close, failures, evidence)  # Close only handles acquired by this gate.
        transaction['uncertain'] |= bool(failures)  # Do not uninstall while process retirement is unverified.
    finish(primary, failures)  # Failed lifecycle or cleanup remains failed.
#
def installer_command(kind: str, action: str, binding: dict, directory: Path, system: Path, log: Path) -> list[str]:  # Keep real installer defaults and explicit custody flags.
    if kind == 'msi':  # MSI repair must reinstall the payload, not merely re-enter maintenance.
        command = [str(system / 'msiexec.exe'), '/x' if action == 'uninstall' else '/i', str(binding['paths'][package.MSI_NAME]), '/qn', '/norestart', 'MSIRESTARTMANAGERCONTROL=Disable', '/l*v', str(log)]  # Disable application shutdown through Restart Manager.
        return command + (['REINSTALL=ALL', 'REINSTALLMODE=amus'] if action == 'reinstall' else [])  # Force same-package files and user/machine registration repair.
    if action == 'uninstall':  # Only documented Inno uninstaller switches belong here.
        return [str(directory / 'unins000.exe'), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/LOG=' + str(log)]  # Never pick an arbitrary wildcard match.
    return [str(binding['paths'][package.EXE_NAME]), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOCLOSEAPPLICATIONS', '/NORESTARTAPPLICATIONS', '/RESTARTEXITCODE=3010', '/LOG=' + str(log)]  # A required reboot cannot masquerade as exit zero.
#
def run_installer(kind: str, action: str, binding: dict, directory: Path, account: windows_account, evidence: journal, label: str, transaction: dict) -> None:  # Bound each installer client and its owned descendants.
    source = binding['paths'][package.MSI_NAME if kind == 'msi' else package.EXE_NAME]  # Recheck the exact original installer before every transaction.
    require(package.sha(plain(source)) == binding['hashes'][source.name], 'installer input changed')  # Do not execute mutated installer bytes.
    command = installer_command(kind, action, binding, directory, account.system_directory, evidence.root / (label + '.log'))  # Retain the native installer log.
    job, primary, failures, settled = windows_job(evidence.root, label), None, [], False  # Installer children are private; the MSI service is not.
    try:  # A timeout never authorizes killing the shared Windows Installer service.
        evidence.record('command-start', label=label, command=command, cwd=str(evidence.root), timeout_seconds=300)  # Log intent before process creation.
        result = job.run(command, dict(os.environ), evidence.root, 300)  # Leave actual account folders/default per-user behavior intact.
        wait_until(lambda: job.active_count() == 0, 30, label + ' left installer descendants')  # Require real process retirement.
        settled = True  # A returned exit and naturally empty client job acknowledge completion.
        evidence.record('command-exit', label=label, **result)  # Retain failure and restart codes too.
        require(result['returncode'] == 0, label + ' exited ' + str(result['returncode']))  # Never accept a required restart as immediate success.
    except BaseException as error:  # Retain the installation failure before cleanup.
        primary = error  # Subsequent removal remains compensating work.
        attempt(label + '-failure-journal', lambda: evidence.record('installer-failure', label=label, error=str(error), service_transaction_settled=settled if kind == 'msi' else 'not-applicable'), failures, evidence)  # A logging error cannot replace the installer failure.
    finally:  # Retire only the job created by this call.
        transaction['launched'] |= job.started  # Intent alone does not authorize account recovery.
        transaction['uncertain'] |= kind == 'msi' and job.started and not settled  # A timeout fences later service-racing cleanup writes.
        before_retirement = len(failures)  # Distinguish a logging defect from uncertain process retirement.
        attempt(label + '-job-retirement', job.terminate_and_close, failures, evidence)  # Never kill msiserver or scan process names.
        transaction['uncertain'] |= len(failures) != before_retirement  # Any unverified process retirement also fences recovery mutation.
    finish(primary, failures)  # Require both transaction and owned-process cleanup to succeed.
#
def capture_uninstaller(directory: Path) -> dict[str, str]:  # Establish custody separately from the audited payload.
    return {name: package.sha(plain(directory / name)) for name in sorted(uninstaller_names)}  # Require both exact regular-file names.
#
def removed(directory: Path, original: dict, account: windows_account, products: list[str], evidence: journal, label: str) -> None:  # Removal evidence comes from independent native state.
    failure = None  # Read registry state even when directory removal times out.
    try:  # Never recursively delete the package directory to manufacture success.
        wait_until(lambda: not directory.exists(), 60, 'uninstall left install directory')  # Bound the normal removal delay.
        plain(directory, directory=True, missing=True)  # Catch dangling reparse replacements too.
    except BaseException as error:  # Retain directory uncertainty while reading independent state.
        failure = error  # Do not lose the first removal defect.
    try:  # Registry/evidence failures must not mask a directory-removal failure.
        current = account.snapshot()  # Reread typed PATH and installer registrations.
        states = {code: account.product_state(code) for code in products}  # Query each original product identity once.
        evidence.record('removal-readback', label=label, snapshot=current, directory_absent=not directory.exists(), directory_error=None if failure is None else str(failure), product_states=states)  # Record failed removal readbacks too.
    except BaseException as error:  # Preserve both independent readback failures.
        if failure is None:  # There is no earlier failure to preserve.
            raise  # Retain the original registry/evidence error.
        failure.add_note('removal readback: ' + str(error))  # Surface the additional unavailable evidence.
        raise failure from error  # Keep the directory error primary.
    if failure is not None:  # A good registry read cannot excuse retained files.
        raise failure  # Keep the directory-removal failure primary.
    require(current == original and all(state == -1 for state in states.values()), 'uninstall did not restore exact account/registration state')  # INSTALLSTATE_UNKNOWN is required.
#
def suffix_fixture_baseline(original: dict, suffix: str) -> dict:
    """Expected account state after uninstall, retaining our explicit suffix fixture."""
    require(isinstance(suffix, str) and suffix.startswith(';') and len(suffix) > 1, 'invalid PATH suffix fixture')
    before = original['user_path']
    prefix = before['value'] if before['exists'] else ''
    value = prefix + suffix if prefix else suffix[1:]
    path = {'exists': True, 'type': before['type'] if before['exists'] else 1, 'value': value}
    return dict(original, user_path=path)


def run_format(kind: str, binding: dict, account: windows_account, original: dict, evidence: journal, *, borrowed_path: bool = False, suffix_fixture: str | None = None) -> None:  # Preserve one format's primary failure through all cleanup phases.
    require(suffix_fixture is None or kind == 'exe' and not borrowed_path, 'PATH suffix fixtures require an owned EXE entry')
    removal_baseline = original if suffix_fixture is None else suffix_fixture_baseline(original, suffix_fixture)
    # Until the fixture write completes, recovery belongs to the original baseline.
    recovery_baseline = original
    directory, primary, failures, uninstaller, products = account.directory, None, [], None, []  # All mutable state belongs to this transaction.
    transaction, removal_done = {'launched': False, 'uncertain': False}, False  # Distinguish acknowledged launch from speculative intent.
    acknowledged_paths = [original['user_path']]  # Only actual observed transitions can authorize PATH compensation.
    def acknowledge_path(label: str) -> None:  # Call only after a successful, settled installer operation.
        state = account.read_path()  # Obtain a fresh typed native value.
        known = state in allowed_paths(original['user_path'], directory)  # Recognize the supplied templates' possible transformations.
        evidence.record('path-acknowledgement', label=label, state=state, owned_transition=known)  # Retain acknowledgement independently of acceptance.
        if known and state not in acknowledged_paths:  # Never authorize an unobserved or foreign edit.
            acknowledged_paths.append(state)  # Recovery still requires an exact current match.
    try:  # Every attempted installation has a finalizer.
        run_installer(kind, 'install', binding, directory, account, evidence, kind + '-install', transaction)  # Install into the unchanged native per-user default.
        acknowledge_path(kind + '-install')  # Intent alone never authorizes a PATH write.
        if kind == 'exe':  # Capture generated metadata before validating the payload.
            uninstaller = capture_uninstaller(directory)  # A later payload fault must not lose cleanup custody.
        first = account.snapshot()  # Read actual registry state, not inherited process PATH.
        identity = account.validate_installed(kind, binding['version'], directory, first)  # Require the matching product/AppId and install location.
        products = [item['product_code'] for item in first['registrations']['msi_products']]  # Keep original product codes for post-removal queries.
        if borrowed_path:
            require(kind == 'exe' and account.path_has(original['user_path']['value'], str(directory)), 'borrowed fixture lacks an equivalent EXE entry')
            require(first['user_path'] == original['user_path'], 'EXE changed a borrowed PATH entry')
        else:
            check_path(original['user_path'], first['user_path'], directory)  # Preserve literal account PATH contents.
        path_receipt = None
        if kind == 'exe':
            original_path = original['user_path']
            require(first['user_path']['type'] == (original_path['type'] if original_path['exists'] else 1), 'EXE changed PATH registry type')
            path_receipt = account.inno_path_receipt()
            expected_receipt = {'IliumPathReceipt': 1, 'IliumPathExisted': int(original_path['exists']),
                                'IliumPathOwned': int(not borrowed_path), 'IliumPathBefore': original_path['value'] or '',
                                'IliumPathAfter': first['user_path']['value'], 'IliumPathEntry': str(directory)}
            require(path_receipt == expected_receipt, 'EXE ownership receipt differs from observed native transition')
            evidence.record('path-ownership-receipt', label=kind + '-install', receipt=path_receipt)
        require(first['machine_path'] == original['machine_path'], 'per-user install changed machine PATH')  # No machine-level PATH modification is allowed.
        evidence.record('installation-readback', label=kind + '-install', snapshot=first, identity=identity)  # Bind native registration to this phase.
        probe(directory, binding, kind, kind + '-initial', evidence, account.system_directory, transaction)  # Exercise the first installed pair.
        run_installer(kind, 'reinstall', binding, directory, account, evidence, kind + '-reinstall', transaction)  # Repeat the same retained package after server retirement.
        acknowledge_path(kind + '-reinstall')  # Bind only the completed transaction's observed state.
        if kind == 'exe':  # Inno can legitimately update its generated uninstall log.
            uninstaller = capture_uninstaller(directory)  # Bind the replacement metadata before later cleanup.
        repeated = account.snapshot()  # Obtain fresh authoritative state after reinstall.
        require(repeated == first and account.validate_installed(kind, binding['version'], directory, repeated) == identity, 'reinstall changed PATH or registration identity')  # Reject duplicates and product churn.
        if kind == 'exe':
            require(account.inno_path_receipt() == path_receipt, 'EXE reinstall replaced original PATH ownership')
            evidence.record('path-ownership-receipt', label=kind + '-reinstall', receipt=path_receipt)
        evidence.record('reinstall-readback', label=kind, snapshot=repeated, identity=identity)  # Reinstallation is independently observable.
        probe(directory, binding, kind, kind + '-repeated', evidence, account.system_directory, transaction)  # Recheck all bytes, both versions and a fresh lifecycle.
        if suffix_fixture is not None:
            require(not account.path_has(suffix_fixture, str(directory)), 'suffix fixture duplicates the owned entry')
            require(account.snapshot() == repeated, 'account changed before suffix fixture; preserved')
            edited = dict(repeated['user_path'], value=repeated['user_path']['value'] + suffix_fixture)
            account.restore_path(edited, [repeated['user_path']])
            recovery_baseline = removal_baseline
            acknowledged_paths.append(edited)
            expected_edited = dict(repeated, user_path=edited)
            require(account.snapshot() == expected_edited, 'suffix fixture changed unrelated account state')
            evidence.record('path-suffix-fixture', synthetic=True, before=repeated['user_path'], edited=edited,
                            expected_after_uninstall=removal_baseline['user_path'])
        if uninstaller is not None:  # Refuse a changed uninstaller before executing it.
            require(capture_uninstaller(directory) == uninstaller, 'owned uninstaller changed')  # Never trust only the uninstaller filename.
        run_installer(kind, 'uninstall', binding, directory, account, evidence, kind + '-uninstall', transaction)  # Exercise the actual format's removal path.
        removal_done = True  # Do not relaunch a successfully completed uninstaller after a readback failure.
        acknowledge_path(kind + '-uninstall')  # Recovery can observe a known lossy PATH removal without accepting it.
        removed(directory, removal_baseline, account, products, evidence, kind + '-uninstall')  # Compensating restoration cannot satisfy this check.
    except BaseException as error:  # Retain the precise first failing contract.
        primary = error  # Do not mask it with a cleanup exception.
    finally:  # Attempt removal, readback and PATH preservation independently.
        if transaction['launched'] and not removal_done and not transaction['uncertain']:  # Never race an unsettled service transaction.
            def uninstall_owned() -> None:  # Keep this action scoped to the attempted format.
                if kind == 'exe':  # Never execute an ambiguous or uncaptured partial Inno uninstaller.
                    require(uninstaller is not None and capture_uninstaller(directory) == uninstaller, 'partial EXE cleanup has no verified uninstaller custody')  # Leave residue reported rather than guessing.
                run_installer(kind, 'uninstall', binding, directory, account, evidence, kind + '-failure-uninstall', transaction)  # MSI uses the original hashed package, not a product-name scan.
                acknowledge_path(kind + '-failure-uninstall')  # Record only an actual completed recovery transition.
            attempt(kind + '-failure-uninstall', uninstall_owned, failures, evidence)  # Continue remaining readbacks even if this fails.
        if transaction['launched']:  # No recovery writes are authorized by a prelaunch failure.
            attempt(kind + '-final-removal-readback', lambda: removed(directory, recovery_baseline, account, products, evidence, kind + '-final'), failures, evidence)  # Retain residual bytes/registrations as failure.
            if transaction['uncertain']:  # A shared MSI operation may still be mutating this account.
                failures.append('transaction/process retirement unverified; no further uninstall or PATH writes; disposable account reconciliation is pending')  # Preserve custody and report the exact unfinished cleanup.
            else:  # Recovery requires both settlement and acknowledged value ownership.
                attempt(kind + '-restore-user-path', lambda: account.restore_path(recovery_baseline['user_path'], acknowledged_paths), failures, evidence)  # Refuse any unrecognized/concurrent PATH edit.
            attempt(kind + '-restored-account-readback', lambda: require(account.snapshot() == recovery_baseline, 'final account readback differs'), failures, evidence)  # Do not repair unrelated registry or machine state.
        attempt(kind + '-outcome-journal', lambda: evidence.record('format-result', format=kind, state='failed' if primary is not None or failures else 'passed', primary=None if primary is None else type(primary).__name__ + ': ' + str(primary), cleanup_errors=list(failures)), failures, evidence)  # Preserve primary and every cleanup failure in JSONL.
    finish(primary, failures)  # No environmental or cleanup failure becomes a successful format.
#
def exe_path_matrix(binding: dict, account: windows_account, original: dict, evidence: journal) -> None:
    """Execute real installers against typed PATH fixtures in the already exclusive account.

    Each fixture uses the same audited lifecycle/reinstall/removal gate. Restoration
    never accepts an unobserved installer or concurrent PATH state as fixture-owned.
    """
    system = str(account.system_directory)
    cases = [
        ('absent', {'exists': False, 'type': None, 'value': None}, False),
        ('empty-string', {'exists': True, 'type': 1, 'value': ''}, False),
        ('empty-expand', {'exists': True, 'type': 2, 'value': ''}, False),
        ('trailing-string', {'exists': True, 'type': 1, 'value': system + ';;'}, False),
        ('trailing-expand', {'exists': True, 'type': 2, 'value': '%SystemRoot%\\System32;;'}, False),
        ('borrowed-quoted', {'exists': True, 'type': 1, 'value': system + ';"' + str(account.directory).replace('\\', '/') + '/";;'}, True),
        ('borrowed-expanded', {'exists': True, 'type': 2, 'value': '%LOCALAPPDATA%\\Programs\\ilium;;'}, True),
    ]
    cases = [(label, fixture, borrowed, None) for label, fixture, borrowed in cases]
    suffix = ';' + system + ';%SystemRoot%\\System32;;'
    cases.extend([
        ('suffix-absent', {'exists': False, 'type': None, 'value': None}, False, suffix),
        ('suffix-empty-string', {'exists': True, 'type': 1, 'value': ''}, False, suffix),
        ('suffix-trailing-expand', {'exists': True, 'type': 2, 'value': '%SystemRoot%\\System32;;'}, False, suffix),
    ])
    for label, fixture, borrowed, suffix_fixture in cases:
        require(account.snapshot() == original, 'account changed before PATH fixture; preserved')
        primary, failures = None, []
        acknowledged_fixture_states = [fixture]
        try:
            account.restore_path(fixture, [original['user_path']])
            baseline = account.snapshot()
            require(baseline['user_path'] == fixture, 'PATH fixture readback differs')
            require(baseline['machine_path'] == original['machine_path'] and baseline['registrations'] == original['registrations'], 'fixture changed unrelated account state')
            child_evidence = journal(evidence.root / ('exe-path-' + label))
            child_evidence.record('path-fixture', label=label, synthetic=True, original=original['user_path'], fixture=fixture)
            run_format('exe', binding, account, baseline, child_evidence, borrowed_path=borrowed, suffix_fixture=suffix_fixture)
            expected = baseline if suffix_fixture is None else suffix_fixture_baseline(baseline, suffix_fixture)
            require(account.snapshot() == expected, 'EXE did not restore exact fixture state')
            acknowledged_fixture_states.append(expected['user_path'])
            evidence.record('path-fixture-result', label=label, state='passed', native_execution=True)
        except BaseException as error:
            primary = error
        finally:
            # Restore only the exact supplied fixture. A failed transaction that
            # retained a different state must be reconciled, never hidden by rollback.
            attempt(label + '-restore-original-path', lambda: account.restore_path(original['user_path'], acknowledged_fixture_states), failures, evidence)
            attempt(label + '-original-account-readback', lambda: require(account.snapshot() == original, 'account differs after PATH fixture'), failures, evidence)
        finish(primary, failures)


def smoke(arguments: argparse.Namespace) -> None:  # Entry point used by the existing builder CLI.
    evidence = journal(arguments.log)  # Establish durable diagnostics before preflight.
    try:  # Preflight failures also receive a terminal failed record.
        require(os.name == 'nt' and struct.calcsize('P') == 8 and platform.machine().casefold() in ('amd64', 'x86_64'), 'native x64 Windows and 64-bit Python are required')  # Never skip another platform into success.
        hosted = os.environ.get('GITHUB_ACTIONS') == 'true' and os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted'  # Reuse the supplied hosted-account boundary.
        require(hosted or arguments.disposable_account, 'use an exclusive disposable account; --disposable-account attests an operator-provisioned test account')  # A user workstation account is outside the gate's custody contract.
        binding = bind_inputs(arguments, evidence)  # Reject all artifact defects before installing anything.
        account = windows_account()  # Native APIs identify the actual current account and folders.
        plain(account.directory, directory=True, missing=True)  # Refuse a junction or pre-existing installation.
        require(not account.directory.exists(), 'default per-user installation already exists')  # Never overwrite user installation state.
        programs = account.directory.parent  # The MSI may remove only this empty parent.
        require(not programs.exists() or any(programs.iterdir()), 'pre-existing empty Programs directory requires an independently prepared disposable account')  # Preserve the known MSI empty-parent edge case without editing it.
        original = account.snapshot()  # Keep raw typed values, including absence.
        account.require_clean(original)  # Reject related MSI products, Inno registrations and equivalent PATH entries.
        require(not account.path_has(original['user_path']['value'] or '', str(account.directory)), 'account PATH already refers to this installation')  # Preserve authored quoted/expanded/slash aliases.
        for name in ('cmd.exe', 'msiexec.exe'):  # Select only actual native system tools.
            plain(account.system_directory / name)  # Do not fall back to PATH or Git Bash.
        evidence.record('account-before', identity=account.identity, snapshot=original, custody='github-hosted' if hosted else 'operator-attested-exclusive', native_platform=platform.platform())  # Record the material exclusivity assumption.
        for kind in ('msi', 'exe'):  # Do not proceed to the next format after any failed contract.
            run_format(kind, binding, account, original, evidence)  # Each format installs, exercises, reinstalls and removes.
        exe_path_matrix(binding, account, original, evidence)
        require(all(package.sha(plain(path)) == binding['hashes'][name] for name, path in binding['paths'].items()), 'bound input changed during native smoke')  # Bind final evidence to unchanged retained input bytes.
        import validate_animation_smoke as animation_seal
        require(animation_seal.source_hashes(Path(__file__).resolve().parents[2]) ==
                binding['source_files'], 'Windows smoke source changed during native execution')
        evidence.record('result', command='smoke', state='passed', version=binding['version'], installers={name: binding['hashes'][name] for name in package.INSTALLER_NAMES}, input_sha256=binding['hashes'], source_files=binding['source_files'], native_exe_msi=True, public_release_verified=False)  # Successful source/native smoke never claims public publication.
    except BaseException as error:  # Preserve all failure classes for the calling CLI.
        try:  # Evidence failure must not replace the operational failure.
            evidence.record('result', command='smoke', state='failed', error=type(error).__name__ + ': ' + str(error), notes=getattr(error, '__notes__', []), public_release_verified=False)  # Retain primary and attached cleanup errors.
        except BaseException as logging_error:  # A broken evidence sink is never a pass.
            error.add_note('final journal failure: ' + str(logging_error))  # Keep the original failure as primary.
        raise  # Preserve the existing caller's nonzero-error semantics.
