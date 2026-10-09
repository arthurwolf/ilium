#!/usr/bin/env python3
# Trusted guest controller; Python 3.6 syntax also covers Leap's default interpreter.
import hashlib  # Bind retained application output to actual bytes.
import json  # Keep capability and completion records machine readable.
import os  # Read namespace identities and change only this fixture's credentials.
from pathlib import Path  # Limit all writable state to owned guest paths.
import select  # Bound readiness without a blocking pipe read.
import stat  # Reject substituted files and runtime sockets.
import subprocess  # Retain child handles and explicit command status.
import sys  # Select one of the four complete private fixture modes.
import time  # Bound startup, admission and retirement separately.

state_root = Path('/var/lib/ilium-container-fixture')  # This directory is created before boot.
user_unit = 'ilium-container-user.service'  # This unit exists only in the disposable rootfs.
caller_unit = 'ilium-container-acceptance.service'  # Package operations stay in a Delegate=no unit.
namespace_names = ('mnt', 'pid', 'user', 'cgroup', 'net', 'ipc', 'uts')  # Require all intended namespace boundaries.
controllers = {'cpu', 'memory', 'pids'}  # Match the candidate controller's actual requirements.
memory_bytes = 384 * 1024 * 1024  # Preserve the untrusted child ceiling exactly.
maximum_tasks = 16  # Preserve the untrusted task ceiling exactly.


def require(condition, message):  # Fail a capability rather than substituting weaker execution.
    if not condition:  # Keep the failure path shallow.
        raise ValueError(message)  # The top-level record remains failed.


def atomic_json(path, value):  # Publish a complete record only after its bytes are ready.
    temporary = path.with_name(path.name + '.tmp')  # Both paths belong to this fresh rootfs.
    with temporary.open('x', encoding='utf-8') as stream:  # Refuse an earlier attempt's file.
        json.dump(value, stream, sort_keys=True)  # Retain deterministic compact JSON.
        stream.write('\n')  # Require a complete last record.
    os.replace(str(temporary), str(path))  # Readers never observe partial JSON.


def run(command, environment=None, timeout=15):  # Capture diagnostics without polluting application stdout.
    result = subprocess.run(command, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE, universal_newlines=True, timeout=timeout)  # Remain compatible with Python 3.6.
    require(result.returncode == 0, 'command failed: ' + repr(command) + ': ' + (result.stdout + result.stderr)[-1600:])  # Keep command failure authoritative.
    return result.stdout  # Parse only successful output.


def membership(text):  # Use the same absolute unified-path contract as the candidate.
    rows = [line[3:] for line in text.splitlines() if line.startswith('0::')]  # Exclude legacy controller lines.
    require(len(rows) == 1 and rows[0].startswith('/'), 'missing or duplicate unified membership')  # Require one usable cgroup-v2 path.
    require(rows[0] == '/' or all(part not in ('', '.', '..') for part in rows[0][1:].split('/')), 'escaping cgroup membership')  # Reject aliases and parent traversal.
    return Path('/sys/fs/cgroup') / rows[0].lstrip('/')  # Preserve the namespace-relative kernel spelling.


def properties(unit, user=False):  # Query actual manager ownership without interpreting error prose.
    command = ['/usr/bin/systemctl'] + (['--user'] if user else [])  # Select only the intended manager.
    command += ['show', unit, '--property=LoadState', '--property=ActiveState', '--property=ControlGroup', '--property=Delegate']  # Request a fixed documented property set.
    result = subprocess.run(command, env=user_environment() if user else None, stdout=subprocess.PIPE, stderr=subprocess.PIPE, universal_newlines=True, timeout=10)  # Bound unavailable managers.
    values = dict(line.split('=', 1) for line in result.stdout.splitlines() if '=' in line)  # Read keyed output instead of locale-dependent status text.
    require(set(values) == {'LoadState', 'ActiveState', 'ControlGroup', 'Delegate'}, 'incomplete manager properties')  # An unavailable manager is not an absent unit.
    require(result.returncode == 0 or result.returncode == 4 and values['LoadState'] == 'not-found', 'manager property query failed')  # Admit only a positively identified missing unit.
    return values  # Callers still enforce the expected state and resource path.


def user_environment():  # The manager and caller use this container's own user bus.
    return {'PATH': '/usr/bin:/bin', 'LANG': 'C', 'LC_ALL': 'C', 'HOME': str(state_root / 'home'), 'XDG_RUNTIME_DIR': '/run/user/65534', 'DBUS_SESSION_BUS_ADDRESS': 'unix:path=/run/user/65534/bus'}  # Never consume a host session address.


def as_user(command):  # Drop both identity and supplementary groups before any user operation.
    return ['setpriv', '--reuid=65534', '--regid=65534', '--clear-groups', 'env', '-i'] + [key + '=' + value for key, value in sorted(user_environment().items())] + command  # Preserve explicit argv boundaries.


def retire_user_unit(unit, cgroup):  # Verify physical collection of this exact completed transient probe.
    deadline = time.monotonic() + 15  # Physical cgroup removal can lag process exit.
    while time.monotonic() < deadline:  # Retry only the retained exact unit identity.
        values = json.loads(run(as_user(['/usr/bin/python3', __file__, 'unit-state', unit])))  # Query with the same non-root manager identity.
        if (values['LoadState'] == 'not-found' or values['ActiveState'] in ('inactive', 'failed')) and not cgroup.exists():  # A state word alone cannot establish retirement.
            return True  # Both manager state and kernel absence were observed.
        time.sleep(0.05)  # Avoid busy waiting.
    raise ValueError('delegation capability unit did not physically retire')  # Fail instead of deleting its cgroup manually.


def namespace_record():  # Observe actual kernel boundaries before starting the package transaction.
    require(Path('/proc/1/comm').read_text().strip() == 'systemd', 'container PID 1 is not systemd')  # A shell or tini is insufficient.
    namespaces = {name: os.stat('/proc/1/ns/' + name).st_ino for name in namespace_names}  # Capture the container init's namespace identities.
    require(all(os.stat('/proc/self/ns/' + name).st_ino == value for name, value in namespaces.items()), 'caller is outside the container init namespaces')  # Keep process and cgroup path interpretations aligned.
    mounts = []  # Locate the actual cgroup-v2 mount.
    for line in Path('/proc/self/mountinfo').read_text().splitlines():  # Read this process's kernel mount table.
        fields = line.split()  # The fixed mountpoint contains no escaped characters.
        if fields[4] == '/sys/fs/cgroup':  # Do not accept a second unrelated mount.
            mounts.append((fields, fields.index('-')))  # Retain filesystem type and mount-root fields.
    require(len(mounts) == 1, 'cgroup mount is missing or ambiguous')  # Reject guessed mount layouts.
    fields, separator = mounts[0]  # Use only the exact mounted cgroup root.
    require(fields[3] == '/' and 'rw' in fields[5].split(',') and fields[separator + 1] == 'cgroup2', 'cgroup namespace root is not a writable unified hierarchy')  # A host-relative subtree bind is not sufficient.
    require(controllers <= set(Path('/sys/fs/cgroup/cgroup.controllers').read_text().split()), 'container lacks cpu, memory or pids')  # Never enable an unavailable ancestor controller.
    init_group = membership(Path('/proc/1/cgroup').read_text())  # Bind PID 1 to the visible cgroup tree.
    require('1' in (init_group / 'cgroup.procs').read_text().split(), 'PID 1 membership does not match visible cgroups')  # Detect a mismatched cgroup namespace.
    current = membership(Path('/proc/self/cgroup').read_text())  # Observe this ordinary caller's actual leaf.
    values = properties(caller_unit)  # Check the root transaction service independently.
    require(values['Delegate'] == 'no' and current == Path('/sys/fs/cgroup') / values['ControlGroup'].lstrip('/'), 'package caller is delegated or in another unit')  # The fixture must not conceal automatic-controller startup.
    require((current / 'memory.max').read_text().strip() == str(768 * 1024 * 1024) and (current / 'pids.max').read_text().strip() == '128', 'trusted caller budget differs')  # Keep the caller's overhead separately bounded.
    uid_map = [list(map(int, row.split())) for row in Path('/proc/1/uid_map').read_text().splitlines()]  # Retain the kernel's actual user mapping.
    gid_map = [list(map(int, row.split())) for row in Path('/proc/1/gid_map').read_text().splitlines()]  # Verify matching group isolation.
    require(len(uid_map) == 1 and uid_map == gid_map and uid_map[0][0] == 0 and uid_map[0][1] >= 524288 and uid_map[0][2] == 65536, 'container UID/GID range is not isolated')  # Refuse identity mappings or -U fallback.
    return {'namespaces': namespaces, 'uid_map': uid_map, 'gid_map': gid_map, 'cgroup_mount_inode': Path('/sys/fs/cgroup').stat().st_ino, 'cgroup_mount_root': fields[3], 'cgroup_filesystem': fields[separator + 1], 'cgroup_writable': True, 'controllers': sorted(controllers), 'caller_cgroup': str(current), 'caller_delegate': False}  # This is capability evidence only.


def output_sha(path, limit=8_000_000):
    descriptor = os.open(str(path), os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as stream:
        require(stat.S_ISREG(os.fstat(stream.fileno()).st_mode), 'output is not a regular file')
        value, size = hashlib.sha256(), 0
        while True:
            chunk = stream.read(min(65536, limit - size + 1))
            if not chunk:
                return value.hexdigest()
            size += len(chunk)
            require(size <= limit, 'output exceeds recovery bound')
            value.update(chunk)


def read_ready(stream, timeout=5):
    deadline = time.monotonic() + timeout
    value = b''
    while value != b'ready\n':
        remaining = deadline - time.monotonic()
        require(remaining > 0 and select.select([stream], [], [], max(0, remaining))[0], 'limited child readiness timed out')
        chunk = os.read(stream.fileno(), 1)
        require(chunk, 'limited child readiness ended early')
        value += chunk
        require(b'ready\n'.startswith(value), 'limited child readiness differs')
    return value


def delegated_check(unit):  # Exercise delegation only in this fresh probe service.
    require(os.geteuid() == 65534, 'delegation probe did not run as UID 65534')  # Root success would not prove the required authority.
    parent = membership(Path('/proc/self/cgroup').read_text())  # Discover the actual service path.
    require(parent.name == unit and parent.stat().st_uid == 65534, 'probe service cgroup name or owner differs')  # Never adopt an ancestor or sibling domain.
    require((parent / 'cgroup.procs').read_text().split() == [str(os.getpid())], 'probe service contains another process')  # Preserve the cgroup-v2 no-internal-process rule.
    require(controllers <= set((parent / 'cgroup.controllers').read_text().split()), 'user service controllers were not delegated')  # Require the same controls as the candidate.
    controller = parent / 'controller'  # This subgroup belongs only to the current probe.
    controller.mkdir()  # Exclusive creation rejects pre-existing state.
    (controller / 'cgroup.procs').write_text('0')  # Move only this trusted process out of its parent.
    require(not (parent / 'cgroup.procs').read_text().strip(), 'probe parent is not empty')  # Do not move another process to repair an unexpected hierarchy.
    (parent / 'cgroup.subtree_control').write_text('+cpu +memory +pids')  # Modify only this freshly delegated service.
    require(controllers <= set((parent / 'cgroup.subtree_control').read_text().split()), 'probe controllers did not enable')  # Read back actual kernel acceptance.
    leaf = parent / 'limited'  # Give the sacrificial process a separate limited leaf.
    leaf.mkdir()  # Never adopt an existing child domain.
    limits = {'memory.max': str(memory_bytes), 'memory.swap.max': '0', 'memory.oom.group': '1', 'pids.max': str(maximum_tasks), 'cpu.max': '100000 100000'}  # Preserve every candidate leaf ceiling.
    for name, value in limits.items():  # Exercise each required cgroup control file.
        (leaf / name).write_text(value)  # All writes stay inside the new leaf.
        require((leaf / name).read_text().strip() == value, 'cgroup limit readback differs: ' + name)  # Kernel capability is measured, not inferred from flags.
    child = None  # Retain ownership before any possible startup failure.
    try:  # Drain the exact leaf even if readiness or a later capability check fails.
        child = subprocess.Popen(['/usr/bin/python3', __file__, 'leaf', str(leaf)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)  # Launch only this file's harmless private leaf mode.
        read_ready(child.stdout)  # Apply the same deadline to every byte, including a partial line.
        require((leaf / 'cgroup.procs').read_text().split() == [str(child.pid)], 'limited process membership differs')  # Bind the live child to the actual limited cgroup.
    finally:  # Kernel teardown must precede releasing the retained child handle.
        (leaf / 'cgroup.kill').write_text('1')  # Kill only the newly created limited domain.
        if child is not None:  # A failed Popen produced no child handle to signal.
            child.wait(timeout=10)  # Require reaping rather than retaining a reusable PID.
    require('populated 0' in (leaf / 'cgroup.events').read_text().splitlines() and (leaf / 'pids.current').read_text().strip() == '0', 'limited child domain did not drain')  # Check actual kernel retirement.
    leaf.rmdir()  # Remove only the empty leaf created above.
    bwrap = ['/usr/bin/bwrap', '--unshare-user', '--unshare-ipc', '--unshare-pid', '--unshare-net', '--unshare-uts', '--unshare-cgroup', '--as-pid-1', '--die-with-parent', '--new-session', '--cap-drop', 'ALL', '--clearenv', '--ro-bind', '/usr', '/usr', '--tmpfs', '/tmp', '--proc', '/proc', '--dev', '/dev']  # Require the candidate's namespace creation capabilities.
    for library in ('/lib', '/lib64'):  # AArch64 need not have both runtime directories.
        if Path(library).exists():  # Preserve the candidate's conditional library mounts.
            bwrap += ['--ro-bind', library, library]  # Expose immutable runtime libraries only.
    run(bwrap + ['--', '/usr/bin/true'], timeout=15)  # This capability probe cannot replace installed animation rendering.
    print(json.dumps({'uid': os.geteuid(), 'unit': unit, 'cgroup': str(parent), 'owner_uid': parent.stat().st_uid, 'controllers': sorted(controllers), 'limits': limits, 'limited_child_started': True, 'limited_child_reaped': True, 'limited_cgroup_absent': not leaf.exists(), 'bwrap_namespaces': True}), flush=True)  # Return only observed capability facts.


def main():  # Run only in the fixture's root transaction service.
    require(os.geteuid() == 0, 'guest controller must start as container root')  # Unprivileged package execution is enforced separately.
    config = json.loads(Path('/smoke/fixture.json').read_text())  # Consume the host's read-only exact lane input.
    result = {'schema': 1, 'nonce': config['nonce'], 'state': 'failed', 'case_exit_code': None, 'user_manager_retired': False, 'errors': []}  # Never initialize a passing record.
    manager_group = None  # Retain the exact guest user-manager cgroup once observed.
    manager_attempted = False  # Partial manager startup also requires cleanup.
    try:  # Preserve diagnostic and cleanup evidence after any startup failure.
        result['namespace'] = namespace_record()  # Reject incorrect namespace topology before a package install.
        result['machine'] = os.uname().machine  # Match this booted container to the requested native architecture.
        os_release = {}  # Parse only the two required distro identity fields.
        for line in Path('/etc/os-release').read_text().splitlines():  # Use the booted distribution's own identity file.
            if line.startswith(('ID=', 'VERSION_ID=')):  # Ignore unrelated display metadata.
                key, value = line.split('=', 1)  # Preserve exact version spelling.
                os_release[key] = value.strip('"')  # Distro identity fields use simple quoted values.
        require(os_release == config['os_release'], 'booted distribution identity differs')  # A substituted base cannot qualify another lane.
        result['os_release'] = os_release  # Retain the actual identity readback.
        for path in ('/usr/bin/systemd-run', '/usr/bin/systemctl', '/usr/lib/systemd/systemd', '/usr/bin/bwrap', '/usr/bin/ffmpeg', '/usr/lib/systemd/user/dbus.socket'):  # Require every used executable and the real user-bus socket unit.
            require(Path(path).is_file(), 'guest prerequisite absent: ' + path)  # Missing package availability is a blocked capability, not a skip.
        runtime = Path('/run/user/65534')  # This tmpfs belongs only to this boot.
        runtime.parent.mkdir(exist_ok=True)  # The shared guest parent remains root-owned.
        runtime.mkdir(mode=0o700)  # Refuse a pre-existing user runtime directory.
        os.chown(str(runtime), 65534, 65534)  # Grant only this guest UID its runtime directory.
        for leaf in ('home', 'data', 'config', 'cache', 'tmp'):  # Create isolated state for the real installed application.
            directory = state_root / leaf  # Keep all writable application data in this disposable rootfs.
            directory.mkdir(mode=0o700)  # Never adopt an earlier run's directory.
            os.chown(str(directory), 65534, 65534)  # Preserve actual non-root package execution.
        manager_attempted = True  # Arm cleanup before the manager can partially start.
        run(['/usr/bin/systemctl', '--no-block', 'start', user_unit])  # Start only this guest's dedicated manager service.
        deadline = time.monotonic() + 45  # Bound manager and socket readiness.
        while time.monotonic() < deadline:  # Wait for both the manager and its real D-Bus socket.
            if (runtime / 'bus').exists():  # A missing path is only pending until the deadline.
                require(stat.S_ISSOCK((runtime / 'bus').lstat().st_mode) and (runtime / 'bus').stat().st_uid == 65534, 'user bus is not this UID\'s socket')  # Reject a symlink or another account's bus.
                values = properties(user_unit)  # Read the system manager's actual delegation decision.
                require(values['ActiveState'] == 'active' and values['Delegate'] == 'yes', 'guest user manager is not active and delegated')  # Environment variables cannot substitute for a manager.
                manager_group = Path('/sys/fs/cgroup') / values['ControlGroup'].lstrip('/')  # Retain only the observed dedicated service domain.
                require(manager_group.name == user_unit and manager_group.stat().st_uid == 65534, 'user manager cgroup ownership differs')  # Never adopt a broader guest ancestor.
                require((manager_group / 'memory.max').read_text().strip() == str(2 * 1024 * 1024 * 1024) and (manager_group / 'pids.max').read_text().strip() == '512', 'trusted user-manager budget differs')  # Keep trusted overhead bounded separately.
                break  # Functional transient-service creation follows next.
            time.sleep(0.05)  # Avoid unbounded or busy startup waits.
        require(manager_group is not None, 'guest user manager or user bus did not become ready')  # Fail instead of changing host login state.
        probe_unit = 'ilium-fixture-probe-' + config['nonce'] + '.service'  # This name cannot collide across fresh root filesystems.
        absent = json.loads(run(as_user(['/usr/bin/python3', __file__, 'unit-state', probe_unit])))  # Require a real manager response before adopting a name.
        require(absent['LoadState'] == 'not-found', 'capability service identity already exists')  # Never stop or reuse another service.
        command = ['/usr/bin/systemd-run', '--user', '--wait', '--pipe', '--collect', '--quiet', '--unit=' + probe_unit, '--property=Type=exec', '--property=Delegate=yes', '--property=MemoryMax=536870912', '--property=MemorySwapMax=0', '--property=TasksMax=64', '--property=RuntimeMaxSec=45', '/usr/bin/python3', __file__, 'delegate', probe_unit]  # Delegate only the fresh sacrificial capability service.
        result['delegation'] = json.loads(run(as_user(command), timeout=60))  # A failure prevents actual package qualification.
        probe_group = Path(result['delegation']['cgroup'])  # Retain the path reported by the independently executed user process.
        require(manager_group in probe_group.parents and probe_group.name == probe_unit, 'capability service escaped its manager')  # Confine retirement to this manager's subtree.
        retire_user_unit(probe_unit, probe_group)  # Require --wait/--collect to physically retire the successful probe before package execution.
        result['delegation']['unit_retired'] = True  # Record verified capability-probe retirement.
        result['manager_cgroup'] = str(manager_group)  # Retain ownership and budget scope.
        atomic_json(state_root / 'ready.json', result)  # Ask the host to verify the kernel namespace and subtree before installation.
        deadline = time.monotonic() + 60  # Host admission must also be bounded.
        admitted = state_root / 'admitted'  # Only the root-owned host/guest control directory can contain this token.
        while not admitted.exists() and time.monotonic() < deadline:  # Wait for independent host observation.
            time.sleep(0.05)  # Never infer admission from elapsed time.
        require(admitted.is_file() and not admitted.is_symlink() and admitted.read_text() == config['nonce'] + '\n', 'host namespace admission missing')  # Fail if host custody was not verified.
        environment = user_environment()  # Pin only this fixture's own bus and application state.
        environment.update(XDG_DATA_HOME=str(state_root / 'data'), XDG_CONFIG_HOME=str(state_root / 'config'), XDG_CACHE_HOME=str(state_root / 'cache'), TMPDIR=str(state_root / 'tmp'), ILIUM_SMOKE_BASE=str(state_root / 'tmp'))  # The unchanged lifecycle may further isolate these paths.
        with (state_root / 'case.stdout').open('xb') as stdout, (state_root / 'case.stderr').open('xb') as stderr:  # Keep manager logs separate from real application output.
            case = subprocess.run(['/bin/sh', '-e', '/smoke/case.sh'], env=environment, stdout=stdout, stderr=stderr, timeout=1000)  # Run the entire existing package transaction and actual installed animation probe.
        result['case_exit_code'] = case.returncode  # Preserve install, animation, lifecycle and removal failures.
        require(case.returncode == 0, 'installed package transaction failed')  # Cleanup cannot change a failed transaction into success.
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:  # All expected capability and process errors remain failed.
        result['errors'].append(str(error)[-2000:])  # Preserve the discriminating failure.
    finally:  # Stop the dedicated manager even after partial startup or package timeout.
        try:  # Retain cleanup failure independently of the primary failure.
            if manager_attempted:  # Only this fixture's attempted manager can be stopped.
                run(['/usr/bin/systemctl', '--no-block', 'stop', user_unit])  # Never address a host unit or a real login account.
                deadline = time.monotonic() + 20  # Allow only bounded guest teardown.
                while time.monotonic() < deadline:  # Require kernel absence as well as an inactive manager.
                    values = properties(user_unit)  # A query failure is not manager absence.
                    actual_group = manager_group or Path('/sys/fs/cgroup/system.slice') / user_unit  # The fixed path is used only for this fixture-authored system unit after partial startup.
                    if values['ActiveState'] in ('inactive', 'failed') and not actual_group.exists():  # Do not infer physical retirement from stop success.
                        result['user_manager_retired'] = True  # Both required cleanup observations passed.
                        break  # Do not stop unrelated services.
                    time.sleep(0.05)  # Preserve a small bounded polling interval.
                require(result['user_manager_retired'], 'guest user manager cgroup survived teardown')  # Missing cleanup evidence fails the lane.
        except (OSError, ValueError, subprocess.SubprocessError) as error:  # Report cleanup errors alongside the original cause.
            result['errors'].append('manager cleanup: ' + str(error)[-1600:])  # Never erase the primary failure.
        for name in ('case.stdout', 'case.stderr'):  # Retain hashes even when execution never started.
            path = state_root / name  # Only fresh root-owned controller output is read.
            if not path.exists():  # Missing output means an unattempted case, not successful execution.
                path.write_bytes(b'')  # Permit uniform diagnostic recovery without manufacturing a passing case.
            try:
                result[name + '_sha256'] = output_sha(path)  # Bound hashing and reject redirected output.
            except (OSError, ValueError) as error:
                result[name + '_sha256'] = None
                result['errors'].append('output recovery: ' + str(error)[-1600:])
        if result['case_exit_code'] == 0 and result['user_manager_retired'] and not result['errors']:  # Every independent gate must succeed.
            result['state'] = 'passed'  # Host retirement and installed-render validation are still outstanding.
        atomic_json(state_root / 'result.json', result)  # ExecStopPost powers off the container after this complete record.
    return int(result['state'] != 'passed')  # Preserve the failure in systemd's service result too.


if __name__ == '__main__':  # Keep all private modes explicit and complete.
    if sys.argv[1:] == ['run']:  # Only the root acceptance service selects the transaction mode.
        sys.exit(main())  # Never fall through after a failed transaction.
    if len(sys.argv) == 3 and sys.argv[1] == 'unit-state':  # Query the manager as the already selected identity.
        print(json.dumps(properties(sys.argv[2], user=True)))  # This is capability data, never package qualification.
        sys.exit(0)  # End after the exact readback.
    if len(sys.argv) == 3 and sys.argv[1] == 'delegate':  # This mode runs only inside the fresh delegated capability service.
        delegated_check(sys.argv[2])  # Exercise and retire a real limited child before returning facts.
        sys.exit(0)  # Keep probe failure visible through systemd-run.
    if len(sys.argv) == 3 and sys.argv[1] == 'leaf':  # Start the harmless sacrificial child only in the supplied owned leaf.
        leaf_path = Path(sys.argv[2])  # Its parent controller supplied this private path.
        require(leaf_path.name == 'limited' and leaf_path.parent.name.startswith('ilium-fixture-probe-') and leaf_path.stat().st_uid == os.geteuid(), 'invalid sacrificial leaf')  # Refuse another resource domain.
        (leaf_path / 'cgroup.procs').write_text('0')  # Join before reporting actual readiness.
        print('ready', flush=True)  # The controller observes this only after successful membership.
        time.sleep(30)  # The owned cgroup kill ends this bounded sacrificial process.
        sys.exit(1)  # Natural expiry is not the required retirement path.
    raise SystemExit('invalid private fixture mode')  # Unknown modes never imply capability or acceptance.
