#!/usr/bin/env python3
from __future__ import annotations  # Keep imports portable; native operations require an explicit call.
import hashlib  # Bind fixtures, source and recovered output to exact bytes.
import json  # Serialize retained observations without emitting extra stdout records.
import os  # Inspect kernel identity and enforce the root fixture boundary.
from pathlib import Path  # Confine writable data to a fresh private directory.
import platform  # Reject cross-architecture execution.
import re  # Validate daemon, unit and cgroup identities.
import select  # Detect exit through a retained pidfd.
import shlex  # Quote receipt-bound package basenames only.
import shutil  # Copy inputs and remove only proven-retired private state.
import stat  # Refuse symlinked evidence files.
import subprocess  # Return the existing smoke adapter's CompletedProcess shape.
import tarfile  # Unpack an owned Docker export without escaping its destination.
import tempfile  # Allocate exclusive root-owned workspaces.
import time  # Bound every startup and cleanup loop.
import uuid  # Allocate distinct container and service ownership identities.
from typing import Callable  # Preserve the existing logged command-adapter seam.

source_files = ('release/scripts/linux_container_fixture.py', 'release/packaging/linux/container_fixture_guest.py')  # Seal both halves of the fixture.
namespace_names = ('mnt', 'pid', 'user', 'cgroup', 'net', 'ipc', 'uts')  # Require independent container namespaces.
image_identities = {'ubuntu:22.04': {'ID': 'ubuntu', 'VERSION_ID': '22.04'}, 'ubuntu:24.04': {'ID': 'ubuntu', 'VERSION_ID': '24.04'}, 'debian:12': {'ID': 'debian', 'VERSION_ID': '12'}, 'fedora:41': {'ID': 'fedora', 'VERSION_ID': '41'}, 'opensuse/leap:15.6': {'ID': 'opensuse-leap', 'VERSION_ID': '15.6'}}  # Preserve the supplied distribution identities.
lane_identities = {('deb', 'ubuntu:22.04'), ('deb', 'ubuntu:24.04'), ('deb', 'debian:12'), ('rpm', 'fedora:41'), ('rpm', 'opensuse/leap:15.6'), ('appimage', 'ubuntu:24.04')}  # Retain all six distinct acceptance lanes.
limits = {'container_memory_bytes': 3 * 1024**3, 'container_tasks': 1024, 'manager_memory_bytes': 2 * 1024**3, 'manager_tasks': 512, 'caller_memory_bytes': 768 * 1024**2, 'caller_tasks': 128, 'untrusted_memory_bytes': 384 * 1024**2, 'untrusted_tasks': 16}  # Trusted infrastructure has separate finite budgets.
docker = ['/usr/bin/docker', '--host=unix:///var/run/docker.sock']  # Refuse remote-daemon or ambient-context substitution.
unit_keys = ('LoadState', 'ActiveState', 'SubState', 'Description', 'ControlGroup', 'Delegate', 'Transient', 'MainPID', 'ExecMainCode', 'ExecMainStatus', 'Result')  # Read structured systemd state.
runner_type = Callable[..., subprocess.CompletedProcess[str]]  # The caller supplies its existing host_run logger.


def require(condition: bool, message: str) -> None:  # Keep all capability failures explicit.
    if not condition:  # Avoid nested failure paths.
        raise ValueError(message)  # The smoke CLI already converts this exception to failure.


def sha(path: Path) -> str:  # Hash regular source, export and diagnostic files.
    value = hashlib.sha256()  # Do not load large rootfs exports into memory.
    with path.open('rb') as stream:  # Read only the selected path.
        for block in iter(lambda: stream.read(1024 * 1024), b''):  # Bound each allocation.
            value.update(block)  # Bind the complete byte stream.
    return value.hexdigest()  # Return canonical lowercase SHA-256.


def content_sha(value: object) -> str:  # Match the release seal's canonical JSON digest.
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode('utf-8')).hexdigest()  # Exclude file formatting from semantic identity.


def plain_bytes(path: Path, bound: int = 8_000_000) -> bytes:  # Read only bounded regular retained evidence.
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as stream:
        metadata = os.fstat(stream.fileno())
        require(stat.S_ISREG(metadata.st_mode) and metadata.st_size <= bound, 'fixture evidence is nonregular or unbounded: ' + str(path))
        data = stream.read(bound + 1)
    require(len(data) <= bound, 'fixture evidence grew past its bound')
    return data


def plain_json(path: Path) -> dict:  # Reject incomplete or nonobject fixture records.
    data = plain_bytes(path, 128_000)  # Capability records are intentionally small.
    require(data.endswith(b'\n'), 'fixture JSON record is incomplete')  # Never consume a partial write.
    result = json.loads(data)  # Parse only complete bytes.
    require(isinstance(result, dict), 'fixture record must be an object')  # Preserve a fixed schema boundary.
    return result  # Detailed admission follows separately.


def rootfs_filter(member: tarfile.TarInfo, destination: str) -> tarfile.TarInfo:  # Preserve valid guest symlinks without permitting host hardlink adoption.
    require(member.isfile() or member.isdir() or member.issym() or member.islnk(), 'special node in Docker rootfs export')  # No host device nodes are materialized.
    filtered = tarfile.tar_filter(member, destination)  # Reject extraction through a directory link outside the private destination.
    if not member.islnk():  # Absolute guest symlink targets are legitimate rootfs metadata.
        return filtered  # Later extraction and fixture writes still reject traversal through those links.
    target = Path(member.linkname)  # Hardlinks must name an already extracted regular file inside this rootfs.
    root = Path(destination).resolve()  # Use the actual private extraction root.
    require(not target.is_absolute() and '..' not in target.parts, 'escaping rootfs hardlink')  # Never link an inode from the host filesystem.
    candidate = root / target  # Resolve the archive-relative hardlink target.
    require(root in candidate.resolve().parents and candidate.is_file() and not candidate.is_symlink(), 'unverified rootfs hardlink target')  # Forward or redirected hardlinks fail closed.
    return filtered  # tarfile may now create only an internal hardlink.


def call(command: list[str], runner: runner_type, log: Path, label: str, timeout: int = 60) -> subprocess.CompletedProcess[str]:  # Preserve command diagnostics through the supplied adapter.
    result = runner(command, log, label, timeout=timeout, env={'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LANG': 'C', 'LC_ALL': 'C'})  # Exclude ambient bus, Docker and nspawn overrides.
    require(result.returncode == 0, 'fixture command failed: ' + shlex.join(command) + ': ' + (result.stdout + result.stderr)[-1600:])  # Timeouts and launch errors remain disqualifying.
    return result  # A successful tool invocation is not yet package acceptance.


def unit_state(unit: str, runner: runner_type, log: Path, label: str) -> dict[str, str]:  # Query a single retained transient service.
    command = ['/usr/bin/systemctl', 'show', unit] + ['--property=' + key for key in unit_keys]  # Use documented properties instead of status prose.
    result = runner(command, log, label, timeout=15, env={'PATH': '/usr/bin:/bin', 'LANG': 'C', 'LC_ALL': 'C'})  # Bound an unavailable system manager.
    rows = [line.split('=', 1) for line in result.stdout.splitlines()]  # Require complete keyed output.
    require(all(len(row) == 2 for row in rows) and len(rows) == len(unit_keys), 'incomplete system manager readback')  # Do not interpret an error as unit absence.
    values = dict(rows)  # Duplicate keys reduce the set and are rejected next.
    require(set(values) == set(unit_keys), 'system manager property set differs')  # Reject ambiguous state.
    require(result.returncode == 0 or result.returncode == 4 and values['LoadState'] == 'not-found', 'system manager query failed')  # A nonzero result qualifies absence only with explicit structured evidence.
    return values  # Every mutation also verifies the ownership description.


def cgroup_path(value: str) -> Path:  # Validate a manager- or kernel-provided unified path.
    require(value.startswith('/') and value != '/' and all(part not in ('', '.', '..') for part in value[1:].split('/')), 'invalid or root cgroup path')  # Never address the host root hierarchy.
    return Path('/sys/fs/cgroup') / value[1:]  # Preserve the path's exact kernel spelling.


def docker_inventory(runner: runner_type, log: Path, label: str) -> dict[str, str]:  # Successful inventory distinguishes absence from daemon failure.
    output = call(docker + ['ps', '--all', '--no-trunc', '--format', '{{.ID}} {{.Names}}'], runner, log, label).stdout  # Retain complete daemon IDs.
    records = {}  # Index names without using a substring filter.
    for line in output.splitlines():  # Inspect every complete daemon row.
        fields = line.split()  # Docker container names contain no whitespace.
        require(len(fields) == 2 and re.fullmatch('[0-9a-f]{64}', fields[0]) is not None, 'unrecognized Docker inventory')  # A malformed listing cannot prove cleanup.
        for name in fields[1].split(','):  # Historical linked aliases remain separately identifiable.
            require(name not in records, 'duplicate Docker container name')  # Refuse ambiguous custody.
            records[name] = fields[0]  # Keep the exact immutable container ID.
    return records  # An empty successful listing is valid absence evidence.


def docker_owned(name: str, nonce: str, runner: runner_type, log: Path, label: str) -> str | None:  # Recover partial create attempts only through exact ownership.
    identity = docker_inventory(runner, log, label).get(name)  # Do not guess an ID from a timed-out CLI.
    if identity is None:  # No successful inventory row names this fixture.
        return None  # This is explicit daemon absence, not an ignored error.
    rows = json.loads(call(docker + ['inspect', identity], runner, log, label).stdout)  # Recheck ownership against daemon metadata.
    require(len(rows) == 1 and rows[0]['Id'] == identity and rows[0]['Name'] == '/' + name and rows[0]['Config']['Labels'].get('org.ilium.release.fixture') == nonce, 'Docker preparation identity is not ours')  # Never remove a merely similar container.
    return identity  # All subsequent stop/removal uses this complete ID.


def preparation(image: str, kind: str, package_name: str) -> str:  # Prepare dependencies without claiming installed-animation acceptance.
    package = shlex.quote('/packages/' + package_name)  # The smoke has already validated this receipt basename.
    lines = ['set -eu # Fail every prerequisite operation.']  # Root execution stays inside the ordinary Docker preparation container.
    if kind in ('deb', 'rpm'):  # Refuse pre-existing Ilium state before dependency preparation.
        lines += ['sh /smoke/absent-' + kind + '.sh # Verify fresh package ownership.']  # Use the original complete absence script.
    if image.startswith(('ubuntu:', 'debian:')):  # Preserve the three Debian-family base images.
        lines += ['export DEBIAN_FRONTEND=noninteractive # Disable package prompts.', 'rm -f /etc/dpkg/dpkg.cfg.d/excludes # Preserve audited documentation.', 'apt-get update -qq # Refresh only this preparation container.', 'apt-get install -y -qq systemd systemd-sysv dbus dbus-user-session python3 bubblewrap util-linux coreutils findutils diffutils ffmpeg # Install system and user-manager prerequisites.']  # No host service is changed.
        if kind == 'deb':  # Populate all actual package dependencies through its own metadata.
            lines += ['apt-get install -y -qq ' + package + ' # Resolve the receipt-bound package dependencies.', 'apt-get remove -y -qq ilium # Leave dependencies available for offline acceptance.']  # No application binary or lifecycle is executed here.
        else:  # The AppImage extraction lane remains Ubuntu 24.04.
            lines += ['apt-get install -y -qq libasound2t64 libssl3t64 libstdc++6 # Preserve the existing AppImage runtime prerequisites.']  # No image bytes are executed during preparation.
    elif image == 'fedora:41':  # Keep the Fedora RPM family and its packaged free codec build.
        lines += ['dnf install -y -q --setopt=tsflags= systemd dbus-daemon dbus-broker python3 bubblewrap util-linux coreutils findutils diffutils ffmpeg-free # Install fixture prerequisites.', 'dnf install -y -q --setopt=tsflags= ' + package + ' # Resolve the actual RPM dependencies.', 'dnf remove -y -q --setopt=clean_requirements_on_remove=False ilium # Retain dependencies for the isolated acceptance boot.']  # Repository unavailability must fail this lane.
    else:  # The only remaining admitted image is the supplied openSUSE baseline.
        require(image == 'opensuse/leap:15.6' and kind == 'rpm', 'unsupported preparation image')  # Never silently substitute another distribution.
        lines += ['zypper --non-interactive install systemd dbus-1 python3 bubblewrap util-linux coreutils findutils diffutils ffmpeg-4 # Install fixture prerequisites.', 'zypper --non-interactive --no-gpg-checks install --allow-unsigned-rpm ' + package + ' # Resolve the actual RPM dependencies.', 'zypper --non-interactive remove ilium # Leave a fresh registration namespace for acceptance.']  # The Leap 15.6 package is ffmpeg-4; repository availability still needs native proof.
    if kind in ('deb', 'rpm'):  # Dependency preparation must not leave an installed application behind.
        lines += ['sh /smoke/absent-' + kind + '.sh # Require manager and filesystem absence after preparation.']  # Preserve all original removal assertions.
    lines += ['test -x /usr/lib/systemd/systemd # Require the actual bootable manager.', 'test -x /usr/bin/bwrap # Require the actual sandbox launcher.', 'test -x /usr/bin/ffmpeg # Refuse missing native decoder prerequisites.', 'test -f /usr/lib/systemd/user/dbus.socket # Require the distribution user-bus unit.']  # These are prerequisites, never animation proof.
    return '\n'.join(lines) + '\n'  # Supply the complete preparation script as an immutable input.


def install_units(root: Path) -> None:  # Author only units inside the fresh exported rootfs.
    for relative in ('etc', 'etc/systemd', 'etc/systemd/system', 'etc/systemd/user', 'etc/systemd/user/default.target.wants', 'var', 'var/lib', 'var/lib/dbus'):  # Validate every parent used by fixture writes.
        directory = root / relative  # Never resolve a guest absolute symlink on the host.
        require(not directory.is_symlink(), 'fixture output parent is redirected: ' + relative)  # Refuse a rootfs path escaping the private tree.
        directory.mkdir(exist_ok=True)  # Parents are checked in order before children are created.
    units = root / 'etc/systemd/system'  # No host unit directory is addressed.
    units.mkdir(parents=True, exist_ok=True)  # Preserve existing distro units alongside the new fixture units.
    definitions = {  # Each service is scoped to this one container boot.
        'ilium-container.target': '[Unit]\nDescription=Ilium disposable container acceptance\nRequires=ilium-container-acceptance.service\nAfter=basic.target\n',  # Boot only the required acceptance service and its normal system dependencies.
        'ilium-container-user.service': '[Unit]\nDescription=Ilium disposable user manager\nAfter=basic.target\n[Service]\nType=simple\nUser=65534\nGroup=65534\nEnvironment=HOME=/var/lib/ilium-container-fixture/home\nEnvironment=XDG_RUNTIME_DIR=/run/user/65534\nExecStart=/usr/lib/systemd/systemd --user\nDelegate=cpu memory pids\nMemoryMax=2147483648\nMemorySwapMax=0\nTasksMax=512\nKillMode=control-group\nTimeoutStopSec=15\n',  # Delegate only this fixture's own user-manager service.
        'ilium-container-acceptance.service': '[Unit]\nDescription=Ilium installed package acceptance\nAfter=basic.target\n[Service]\nType=oneshot\nDelegate=no\nMemoryMax=805306368\nMemorySwapMax=0\nTasksMax=128\nUMask=0077\nTimeoutStartSec=1150\nTimeoutStopSec=20\nExecStart=/usr/bin/python3 /smoke/container_fixture_guest.py run\nExecStopPost=/usr/bin/systemctl --no-block poweroff\n',  # Never predelegate the ordinary installed application caller.
    }  # The new target never replaces the distribution's default target on disk.
    for name, text in definitions.items():  # Refuse an already present fixture unit name.
        with (units / name).open('x', encoding='utf-8') as stream:  # The fresh image cannot silently supply our controller definition.
            stream.write(text)  # Retain complete unit bytes.
    wants = root / 'etc/systemd/user/default.target.wants'  # Enable the distribution's actual user D-Bus socket for this rootfs only.
    wants.mkdir(parents=True, exist_ok=True)  # No host user configuration is changed.
    link = wants / 'dbus.socket'  # There must be exactly one intended socket dependency.
    if link.is_symlink():  # The distro may already enable this socket.
        require(os.readlink(link) == '/usr/lib/systemd/user/dbus.socket', 'existing user bus dependency differs')  # Do not replace an unexplained unit.
    else:  # Refuse a pre-existing regular file through exclusive symlink creation.
        link.symlink_to('/usr/lib/systemd/user/dbus.socket')  # Resolve this path inside the booted guest.
    control = root / 'var/lib/ilium-container-fixture'  # Keep application output out of volatile /run.
    control.mkdir(mode=0o755)  # UID 65534 may traverse to its own children but cannot alter root control files.
    machine_id = root / 'etc/machine-id'  # Give every boot a new machine identity.
    require(not machine_id.is_symlink(), 'exported machine-id is redirected')  # Never follow a guest absolute symlink on the host.
    machine_id.write_text(uuid.uuid4().hex + '\n', encoding='ascii')  # Modify only this private copy.
    bus_machine_id = root / 'var/lib/dbus/machine-id'  # Keep the guest system and session bus identity consistent.
    if os.path.lexists(bus_machine_id):  # A distro may use either a regular file or a symlink here.
        require(bus_machine_id.is_symlink() or bus_machine_id.is_file(), 'unexpected guest D-Bus machine identity node')  # Never remove an unexplained directory or special node.
        bus_machine_id.unlink()  # Replace only this private copy without following the old target.
    bus_machine_id.symlink_to('/etc/machine-id')  # Resolve the new identity inside the booted guest.


def owned_cgroup_files(unit_group: Path, *, directory_limit: int = 2048, entry_limit: int = 65536) -> list[Path]:  # Bound observation before allocating or descending.
    require(1 <= directory_limit <= 2048 and 1 <= entry_limit <= 65536, 'invalid cgroup observation bounds')  # Test seams may tighten, never raise, the production caps.
    groups, pending = [unit_group], [unit_group]  # Count the owned root once; both lists remain directory-bounded.
    examined = 0  # Charge every entry, including nonmatching control files.
    while pending:  # Keep at most one scandir iterator open at a time.
        parent = pending.pop()  # Descend only into directories admitted below this owned root.
        require(not parent.is_symlink() and parent.is_dir(), 'owned cgroup directory is redirected or absent')  # Never follow a substituted directory link.
        with os.scandir(parent) as entries:  # Close the iterator on success and every rejection path.
            for entry in entries:  # Consume incrementally, without list, glob or sorted materialization.
                examined += 1  # Include the one over-limit entry needed to detect exhaustion.
                require(examined <= entry_limit, 'owned cgroup entry enumeration exceeded its bound')  # Reject before inspecting or retaining this entry.
                require(not entry.is_symlink(), 'owned cgroup entry is a symlink')  # Cgroup traversal must not escape through a link.
                if not entry.is_dir(follow_symlinks=False):  # Ordinary control files consume budget but need no traversal.
                    continue  # Never search a non-directory for matching descendants.
                require(len(groups) < directory_limit, 'owned container cgroup tree is unbounded')  # Reject before allocating or queuing another child.
                child = parent / entry.name  # A scandir name is relative to the admitted parent.
                groups.append(child)  # Retain only the bounded identity inventory used during admission.
                pending.append(child)  # The traversal queue shares the same directory cap.
    return [group / 'cgroup.procs' for group in groups]  # Construct only the bounded final membership inventory.



def admit_guest(root: Path, unit_group: Path, ready: dict, host_namespaces: dict[str, int]) -> dict:  # Bind guest claims to live host kernel observations.
    observed = ready['namespace']  # Capability output alone cannot prove the writable subtree's ownership.
    require(set(observed['namespaces']) == set(namespace_names), 'guest namespace inventory differs')  # Require every boundary.
    candidates = []  # Locate container PID 1 only among this unit's current tasks.
    group_files = owned_cgroup_files(unit_group)  # Never walk a sibling or ancestor cgroup.
    require(len(group_files) <= 2048, 'owned container cgroup tree is unbounded')  # Bound host observation work.
    for file in group_files:  # Inspect only PIDs observed in this delegated subtree.
        for raw_pid in file.read_text().split():  # The kernel supplies decimal process identities.
            require(raw_pid.isdigit(), 'invalid owned cgroup PID')  # Refuse malformed membership.
            proc = Path('/proc') / raw_pid  # Read-only process observation follows.
            try:  # Short-lived boot processes can exit during enumeration.
                status = proc.joinpath('status').read_text()  # NSpid identifies the container init without a guessed child PID.
                nspid = next(line.split()[1:] for line in status.splitlines() if line.startswith('NSpid:'))  # Require the kernel's nested PID record.
                if nspid[-1:] == ['1'] and proc.joinpath('comm').read_text().strip() == 'systemd':  # Select only a real init process.
                    candidates.append(int(raw_pid))  # Retain candidates for an ambiguity check.
            except (FileNotFoundError, ProcessLookupError):  # A vanished unrelated boot task is not an init candidate.
                continue  # Never signal or adopt that PID.
    require(len(set(candidates)) == 1, 'owned container init is missing or ambiguous')  # Refuse another namespace root.
    pid = candidates[0]  # Exactly one live owned init candidate remains.
    descriptor = os.pidfd_open(pid, 0)  # Pin its identity before further host readbacks.
    try:  # Close the descriptor after recording admission; never signal through a saved PID.
        poller = select.poll()  # Readiness on a pidfd means the process has exited.
        poller.register(descriptor, select.POLLIN)  # Observe this exact process identity.
        proc = Path('/proc') / str(pid)  # Numeric access is guarded by the live pidfd.
        require(not poller.poll(0), 'container init exited during admission')  # Reject stale process paths.
        actual = {name: proc.joinpath('ns', name).stat().st_ino for name in namespace_names}  # Read actual namespaces from the host.
        require(actual == observed['namespaces'] and all(actual[name] != host_namespaces[name] for name in namespace_names), 'guest reused or misreported a host namespace')  # Require isolation and consistent observations.
        uid_map = [list(map(int, row.split())) for row in proc.joinpath('uid_map').read_text().splitlines()]  # Corroborate the guest's mapping through the host kernel view.
        gid_map = [list(map(int, row.split())) for row in proc.joinpath('gid_map').read_text().splitlines()]  # Verify group isolation independently too.
        require(uid_map == observed['uid_map'] and gid_map == observed['gid_map'], 'host and guest user mappings differ')  # A self-reported mapping is insufficient.
        require(proc.joinpath('root').stat().st_ino == root.stat().st_ino and proc.joinpath('root').stat().st_dev == root.stat().st_dev, 'container init uses another rootfs')  # Bind namespace admission to the exported filesystem.
        membership_rows = [line[3:] for line in proc.joinpath('cgroup').read_text().splitlines() if line.startswith('0::')]  # Host-side paths must resolve under the owned unit.
        require(len(membership_rows) == 1 and unit_group in cgroup_path(membership_rows[0]).parents, 'container init escaped its delegated service')  # Refuse a parent-rooted or sibling cgroup namespace.
        mounted = proc / 'root/sys/fs/cgroup'  # This resolves the actual mount seen by the container.
        identity = (mounted.stat().st_dev, mounted.stat().st_ino)  # Bind by kernel inode rather than pathname prose.
        groups = [file.parent for file in group_files]  # Search only the already bounded owned subtree.
        matches = {path for path in groups if (path.stat().st_dev, path.stat().st_ino) == identity}  # Identify the visible cgroup root independently.
        require(len(matches) == 1 and identity[1] == observed['cgroup_mount_inode'], 'guest writable cgroup root is outside the owned service')  # A whole-host writable mount cannot satisfy this check.
        visible_root = matches.pop()  # Retain the exact owned subtree root.
        require(not poller.poll(0), 'container init retired before admission completed')  # Do not release a token after the process died.
        return {'init_pid': pid, 'namespaces': actual, 'host_namespaces': host_namespaces, 'uid_map': uid_map, 'gid_map': gid_map, 'owned_unit_cgroup': str(unit_group), 'visible_cgroup_root': str(visible_root), 'visible_cgroup_inode': identity[1], 'rootfs_inode': root.stat().st_ino, 'pidfd_live_at_admission': True}  # These are observations of this run, not reusable platform qualifications.
    finally:  # Never leak the kernel process handle.
        os.close(descriptor)  # No signal is sent during admission.


def validate_record(record: dict, kind: str, image: str, arch: str, expected_sources: dict[str, str]) -> None:  # Used both immediately and by the independent release seal.
    require((kind, image) in lane_identities and arch in ('x86_64', 'aarch64'), 'fixture lane is outside the retained matrix')  # Never shrink or substitute the acceptance matrix.
    require(isinstance(record, dict) and record.get('schema') == 1 and record.get('state') == 'passed' and record.get('engine') == 'systemd-nspawn', 'container fixture did not complete')  # A capability probe alone cannot qualify.
    require(record.get('format') == kind and record.get('image') == image and record.get('arch') == arch and record.get('source_files') == expected_sources and record.get('limits') == limits and record.get('errors') == [], 'container fixture identity or source differs')  # Bind the exact lane and implementation.
    nonce = record.get('nonce', '')  # Names derive from one fresh ownership token.
    require(re.fullmatch('[0-9a-f]{32}', nonce) is not None and record.get('unit') == 'ilium-container-' + nonce + '.service', 'container fixture ownership identity differs')  # Preserve fresh service identity.
    origin = record.get('origin', {})  # Retain the precise distribution input used for preparation.
    require(origin.get('os') == 'linux' and origin.get('architecture') == ('amd64' if arch == 'x86_64' else 'arm64') and re.fullmatch('sha256:[0-9a-f]{64}', str(origin.get('image_id'))) is not None and isinstance(origin.get('repo_digests'), list) and bool(origin['repo_digests']) and all(re.fullmatch(r'[^\s]+@sha256:[0-9a-f]{64}', str(value)) for value in origin['repo_digests']) and re.fullmatch('[0-9a-f]{64}', str(origin.get('prepared_rootfs_sha256'))) is not None, 'distribution source identity is unverified')  # Never replace the retained image family with a generic VM.
    guest = record.get('guest', {})  # Inspect execution evidence as well as the outer state word.
    require(guest.get('schema') == 1 and guest.get('nonce') == nonce and guest.get('state') == 'passed' and type(guest.get('case_exit_code')) is int and guest.get('case_exit_code') == 0 and guest.get('errors') == [] and guest.get('user_manager_retired') is True and guest.get('os_release') == image_identities[image] and guest.get('machine') in ({'x86_64', 'AMD64'} if arch == 'x86_64' else {'aarch64', 'arm64'}), 'guest execution or manager retirement is unverified')  # Do not label emulation or partial startup as native success.
    namespace = guest.get('namespace', {})  # Require actual namespace and cgroup observations.
    host = record.get('host_admission', {})  # Host-side corroboration is mandatory.
    guest_ns, host_ns = namespace.get('namespaces', {}), host.get('host_namespaces', {})  # Compare the independently recorded views.
    require(set(guest_ns) == set(host_ns) == set(namespace_names) and host.get('namespaces') == guest_ns and all(type(guest_ns[key]) is int and type(host_ns[key]) is int and guest_ns[key] != host_ns[key] for key in namespace_names) and host.get('pidfd_live_at_admission') is True, 'container namespace admission differs')  # Missing and host-reused namespaces fail closed.
    unit_group = Path(str(host.get('owned_unit_cgroup', '')))  # Validate containment without relying on current-machine paths.
    visible = Path(str(host.get('visible_cgroup_root', '')))  # The retained inode maps this visible root to the owned service.
    require(Path('/sys/fs/cgroup') in unit_group.parents and '..' not in unit_group.parts and '..' not in visible.parts and type(host.get('init_pid')) is int and host['init_pid'] > 1, 'invalid host unit or init identity')  # Reject traversal and non-kernel resource paths.
    require(unit_group.is_absolute() and unit_group.name == record['unit'] and (visible == unit_group or unit_group in visible.parents) and namespace.get('cgroup_mount_inode') == host.get('visible_cgroup_inode') and type(host.get('visible_cgroup_inode')) is int and namespace.get('cgroup_mount_root') == '/' and namespace.get('cgroup_filesystem') == 'cgroup2' and namespace.get('cgroup_writable') is True and namespace.get('controllers') == ['cpu', 'memory', 'pids'] and namespace.get('caller_delegate') is False, 'container cgroup topology or caller authority differs')  # A flag or path string alone is insufficient.
    uid_map = namespace.get('uid_map')  # Require the actual isolated mapping retained during boot.
    require(isinstance(uid_map, list) and len(uid_map) == 1 and isinstance(uid_map[0], list) and len(uid_map[0]) == 3 and all(type(value) is int for value in uid_map[0]) and uid_map[0][0] == 0 and uid_map[0][1] >= 524288 and uid_map[0][2] == 65536 and namespace.get('gid_map') == uid_map, 'container user mapping differs')  # Refuse identity maps and missing group isolation.
    require(host.get('uid_map') == uid_map and host.get('gid_map') == uid_map and Path(str(namespace.get('caller_cgroup', ''))).name == 'ilium-container-acceptance.service', 'host mapping or ordinary caller evidence differs')  # Bind both independent views and the exact caller leaf.
    delegated = guest.get('delegation', {})  # The capability probe must exercise real non-root control writes and child retirement.
    expected_limits = {'memory.max': str(limits['untrusted_memory_bytes']), 'memory.swap.max': '0', 'memory.oom.group': '1', 'pids.max': str(limits['untrusted_tasks']), 'cpu.max': '100000 100000'}  # Keep the child ceilings immutable.
    require(delegated.get('uid') == delegated.get('owner_uid') == 65534 and delegated.get('unit') == 'ilium-fixture-probe-' + nonce + '.service' and delegated.get('controllers') == ['cpu', 'memory', 'pids'] and delegated.get('limits') == expected_limits and all(delegated.get(key) is True for key in ('limited_child_started', 'limited_child_reaped', 'limited_cgroup_absent', 'bwrap_namespaces', 'unit_retired')), 'user delegation capability evidence is incomplete')  # Do not accept a manager-version check in its place.
    manager_group = Path(str(guest.get('manager_cgroup', '')))  # Retain the dedicated user-manager ownership boundary.
    delegated_group = Path(str(delegated.get('cgroup', '')))  # Read the actual non-root capability service path.
    require(manager_group.is_absolute() and manager_group.name == 'ilium-container-user.service' and manager_group in delegated_group.parents and delegated_group.name == delegated['unit'], 'user capability cgroup escaped its manager')  # Reject a prepared ancestor outside the dedicated manager.
    require(record.get('runtime') == {'Result': 'success', 'ExecMainCode': '1', 'ExecMainStatus': '0'} and record.get('cleanup') == {'unit_retired': True, 'cgroup_absent': True, 'docker_removed': True, 'state_removed': True}, 'container completion or cleanup differs')  # No forced cleanup can erase failed package execution.
    require(record.get('stdout_sha256') == guest.get('case.stdout_sha256') and record.get('stderr_sha256') == guest.get('case.stderr_sha256'), 'recovered application streams differ')  # The caller also checks the actual recovered stream bytes.
    require(re.fullmatch('[0-9a-f]{64}', str(record.get('animation_stdout_sha256'))) is not None, 'fixture has no bound installed-animation output')  # Sealing compares this hash to the actual four probe records.


def run_fixture(image: str, kind: str, arch: str, packages: Path, smoke: Path, log: Path, label: str, script: str, package_name: str, runner: runner_type, expected_sources: dict[str, str]) -> tuple[subprocess.CompletedProcess[str], dict]:  # Execute one independently owned distribution lane.
    nonce = uuid.uuid4().hex  # Allocate ownership before any side effect.
    unit, name = 'ilium-container-' + nonce + '.service', 'ilium-preparation-' + nonce  # Never reuse shared unit or container names.
    description = 'Ilium distribution fixture ' + nonce  # Verify this description before every stop operation.
    record = {'schema': 1, 'state': 'failed', 'engine': 'systemd-nspawn', 'format': kind, 'image': image, 'arch': arch, 'nonce': nonce, 'unit': unit, 'source_files': expected_sources, 'limits': limits, 'errors': [], 'cleanup': {'unit_retired': False, 'cgroup_absent': False, 'docker_removed': False, 'state_removed': False}}  # No optimistic success fields.
    work, attempted = None, False  # Track ownership even across partial startup.
    unit_group = Path('/sys/fs/cgroup/system.slice') / unit  # The explicitly selected slice gives partial startup an exact absence check.
    stdout, stderr, command = '', '', []  # Failed startup still returns the existing adapter shape.
    try:  # Every post-allocation failure flows through bounded cleanup.
        require(platform.system() == 'Linux' and os.geteuid() == 0, 'container fixture requires explicit root execution on a disposable native runner')  # Do not silently escalate the public CLI.
        require((kind, image) in lane_identities and arch in ('x86_64', 'aarch64') and platform.machine() in ({'x86_64', 'AMD64'} if arch == 'x86_64' else {'aarch64', 'arm64'}), 'unsupported or non-native distribution fixture')  # Keep native architecture truthful.
        require(hasattr(tarfile, 'tar_filter'), 'host Python lacks the required safe tar extraction filter')  # Reject unsupported Python patch levels before allocation.
        require(Path('/proc/1/comm').read_text().strip() == 'systemd' and {'cpu', 'memory', 'pids'} <= set(Path('/sys/fs/cgroup/cgroup.controllers').read_text().split()), 'host systemd/cgroup-v2 controllers unavailable')  # Never repair shared host ancestors.
        require(hasattr(os, 'pidfd_open'), 'host Python/kernel pidfd admission unavailable')  # PID strings alone cannot establish custody.
        for executable in ('/usr/bin/systemd-run', '/usr/bin/systemctl', '/usr/bin/systemd-nspawn', '/usr/bin/docker'):  # Require the actual fixture tools before allocation.
            require(Path(executable).is_file() and os.access(executable, os.X_OK), 'fixture tool absent: ' + executable)  # Missing tooling is a failed lane.
        version = call(['/usr/bin/systemd-nspawn', '--version'], runner, log, label).stdout.split()  # Record and check the documented baseline interface.
        require(len(version) > 1 and version[1].isdigit() and int(version[1]) >= 249, 'systemd-nspawn 249 or later is required')  # Do not silently select an older interface.
        info = json.loads(call(docker + ['info', '--format', '{{json .}}'], runner, log, label).stdout)  # Inspect the explicit local daemon.
        require(info.get('OSType') == 'linux' and info.get('Architecture') in ({'x86_64', 'amd64'} if arch == 'x86_64' else {'aarch64', 'arm64'}) and str(info.get('CgroupVersion')) == '2', 'local Docker architecture or cgroup-v2 capability differs')  # Export preparation must also use native images.
        require(name not in docker_inventory(runner, log, label) and unit_state(unit, runner, log, label)['LoadState'] == 'not-found', 'fixture ownership name already exists')  # Never adopt an existing container or service.
        source_root = Path(__file__).resolve().parents[2]  # Use the same checkout as the imported smoke harness.
        require(set(expected_sources) == set(source_files) and expected_sources == {path: sha(source_root / path) for path in source_files}, 'fixture source authority differs from the selected workspace')  # Prevent mixed harness/checkouts.
        work = Path(tempfile.mkdtemp(prefix='ilium-container-', dir='/var/tmp'))  # All mutable host filesystem state belongs to this run.
        inputs = work / 'inputs'  # Retain immutable inputs even after the caller's temporary scope closes.
        shutil.copytree(smoke, inputs, symlinks=True)  # Preserve the original reference, lifecycle and checksum inputs.
        inputs.chmod(0o755)  # The mapped guest UID must be able to read the bind mount.
        shutil.copyfile(source_root / source_files[1], inputs / 'container_fixture_guest.py')  # Copy the complete reviewed guest controller.
        (inputs / 'case.sh').write_text(script, encoding='utf-8')  # Run the complete existing installed-package transaction.
        (inputs / 'prepare.sh').write_text(preparation(image, kind, package_name), encoding='utf-8')  # Keep dependency preparation separate from acceptance.
        config = {'nonce': nonce, 'os_release': image_identities[image]}  # Supply only immutable per-lane identity.
        (inputs / 'fixture.json').write_text(json.dumps(config) + '\n', encoding='utf-8')  # This file never contains host credentials or user-bus paths.
        for path in inputs.rglob('*'):  # Give mapped guest users read access without changing source artifacts.
            if path.is_symlink():  # Preserve the reference tree's declared launcher links.
                continue  # Never chmod through a guest or reference symlink.
            path.chmod(0o755 if path.is_dir() else 0o644)  # Inputs are read-only bind mounts and are invoked through interpreters.
        docker_arch = 'amd64' if arch == 'x86_64' else 'arm64'  # Preserve Docker's native architecture spellings.
        require(all(not any(char in str(path) for char in ',:\n\r') for path in (packages, inputs)), 'fixture bind path contains an unsupported delimiter')  # Fail before Docker or nspawn can reinterpret a mount argument.
        call(docker + ['pull', '--platform=linux/' + docker_arch, image], runner, log, label, 300)  # Fetch exactly the retained lane image tag.
        images = json.loads(call(docker + ['image', 'inspect', image], runner, log, label).stdout)  # Freeze the resolved immutable image identity.
        require(len(images) == 1 and images[0]['Os'] == 'linux' and images[0]['Architecture'] == docker_arch and not images[0]['Config'].get('Volumes'), 'base image architecture or anonymous-volume contract differs')  # Docker export excludes volume contents.
        origin = {'image_id': images[0]['Id'], 'repo_digests': images[0].get('RepoDigests'), 'architecture': images[0]['Architecture'], 'os': images[0]['Os']}  # Preserve tag resolution without inventing a fixed current digest.
        require(re.fullmatch('sha256:[0-9a-f]{64}', str(origin['image_id'])) is not None and isinstance(origin['repo_digests'], list) and bool(origin['repo_digests']) and all(re.fullmatch(r'[^\s]+@sha256:[0-9a-f]{64}', str(value)) for value in origin['repo_digests']), 'resolved distribution image digest is missing')  # Bind preparation before executing any image command.
        record['origin'] = origin  # Retain the resolved base before preparation.
        create = docker + ['create', '--name=' + name, '--label=org.ilium.release.fixture=' + nonce, '--platform=linux/' + docker_arch, '--cgroupns=private', '--network=bridge', '--memory=3g', '--memory-swap=3g', '--pids-limit=1024', '--cpus=2', '--mount=type=bind,src=' + str(packages) + ',dst=/packages,readonly', '--mount=type=bind,src=' + str(inputs) + ',dst=/smoke,readonly', '--entrypoint=/bin/sh', origin['image_id'], '-ec', 'exec timeout --kill-after=10 600 /bin/sh -e /smoke/prepare.sh']  # Use ordinary Docker confinement for package preparation only.
        call(create, runner, log, label)  # Partial creation is recovered by nonce-labelled inventory in finally.
        identity = docker_owned(name, nonce, runner, log, label)  # Bind subsequent operations to the exact created container.
        require(identity is not None, 'Docker preparation container was not retained')  # No ID means no ownership proof.
        call(docker + ['start', '--attach', identity], runner, log, label, 630)  # A killed or timed-out client cannot qualify preparation.
        prepared = json.loads(call(docker + ['inspect', identity], runner, log, label).stdout)[0]  # Read actual daemon completion separately.
        require(prepared['State']['Status'] == 'exited' and prepared['State']['ExitCode'] == 0 and not prepared['State']['OOMKilled'], 'Docker preparation did not complete cleanly')  # CLI attachment success is insufficient.
        archive = work / 'rootfs.tar'  # Export only this fixture's stopped filesystem.
        call(docker + ['export', '--output=' + str(archive), identity], runner, log, label, 180)  # Preserve package database and dependency state.
        origin['prepared_rootfs_sha256'] = sha(archive)  # Bind the complete prepared distribution filesystem.
        root = work / 'rootfs'  # Extraction never touches a repository or live installation.
        root.mkdir()  # Create an empty exclusive destination.
        with tarfile.open(archive, mode='r:') as exported:  # Docker emits an uncompressed filesystem tar.
            exported.extractall(root, filter=rootfs_filter)  # Preserve internal hardlinks while rejecting host inode adoption.
        install_units(root)  # Author only the reviewed guest fixture units.
        call(docker + ['rm', identity], runner, log, label)  # Retire preparation before booting the acceptance container.
        require(name not in docker_inventory(runner, log, label), 'Docker preparation survived removal')  # Require actual daemon absence.
        record['cleanup']['docker_removed'] = True  # The acceptance phase has no Docker network or daemon access.
        host_namespaces = {key: Path('/proc/self/ns', key).stat().st_ino for key in namespace_names}  # Capture independent host namespace identities.
        nspawn = ['/usr/bin/systemd-nspawn', '--quiet', '--boot', '--keep-unit', '--register=no', '--settings=no', '--private-users=pick', '--private-users-ownership=chown', '--private-network', '--link-journal=no', '--resolv-conf=off', '--timezone=off', '--console=read-only', '--machine=ilium-' + nonce, '--directory=' + str(root), '--bind-ro=' + str(packages) + ':/packages', '--bind-ro=' + str(inputs) + ':/smoke', '--', '--unit=ilium-container.target']  # systemd 249 documents these interfaces; runtime observations still determine admission.
        command = ['/usr/bin/systemd-run', '--quiet', '--collect', '--unit=' + unit, '--description=' + description, '--property=Type=exec', '--property=Slice=system.slice', '--property=Delegate=yes', '--property=MemoryMax=3221225472', '--property=MemorySwapMax=0', '--property=TasksMax=1024', '--property=CPUQuota=200%', '--property=RuntimeMaxSec=1200', '--property=TimeoutStopSec=20', '--property=KillMode=control-group', '--property=RemainAfterExit=yes', *nspawn]  # Delegate only the fresh bounded container service, never a shared slice.
        attempted = True  # Partial transient-service startup belongs to this fixture too.
        call(command, runner, log, label, 30)  # The returned start acknowledgement is not qualification.
        deadline = time.monotonic() + 1180  # Leave the service's own deadline as an independent backstop.
        control = root / 'var/lib/ilium-container-fixture'  # Guest control files persist outside its /run tmpfs.
        admitted = False  # Installed execution needs independent host admission first.
        while time.monotonic() < deadline:  # Observe only this exact owned transient unit.
            values = unit_state(unit, runner, log, label)  # Preserve manager errors as failures.
            require(values['Description'] == description and values['Transient'] == 'yes' and values['Delegate'] == 'yes', 'container service ownership or delegation differs')  # Never stop or accept a substituted unit.
            if values['ControlGroup']:  # Capture the exact path while the unit is live.
                require(cgroup_path(values['ControlGroup']) == unit_group, 'container service owns an unexpected cgroup')  # Require the exact path in the explicitly selected slice.
            if not admitted and (control / 'ready.json').exists():  # The guest is waiting without installing the application yet.
                require(unit_group is not None and unit_group.is_dir(), 'container service cgroup is absent at admission')  # Do not infer its identity from the unit name.
                require((unit_group / 'memory.max').read_text().strip() == str(limits['container_memory_bytes']) and (unit_group / 'pids.max').read_text().strip() == str(limits['container_tasks']), 'trusted container budget differs')  # Read back actual outer kernel limits.
                ready = plain_json(control / 'ready.json')  # Read one atomic guest capability record.
                require(ready.get('nonce') == nonce, 'guest readiness belongs to another fixture')  # Bind startup to this run.
                record['host_admission'] = admit_guest(root, unit_group, ready, host_namespaces)  # Prove cgroup-root containment and distinct namespaces.
                with (control / 'admitted').open('x', encoding='ascii') as stream:  # Refuse a pre-existing admission token.
                    stream.write(nonce + '\n')  # Release only this prepared rootfs's package transaction.
                admitted = True  # Host and guest observations now agree.
            if values['SubState'] in ('exited', 'dead', 'failed') or values['ActiveState'] in ('inactive', 'failed'):  # Wait for actual nspawn process completion.
                record['runtime'] = {key: values[key] for key in ('Result', 'ExecMainCode', 'ExecMainStatus')}  # Preserve outer process result independently of guest JSON.
                break  # Collect outputs even after a guest failure.
            time.sleep(0.1)  # Keep polling bounded without a long blocking wait.
        else:  # The CLI deadline expired before actual completion.
            raise ValueError('container acceptance timed out')  # Finally still stops the exact retained service.
        record['guest'] = plain_json(control / 'result.json')  # Missing or partial guest evidence fails the lane.
        stdout = plain_bytes(control / 'case.stdout').decode('utf-8')  # Recover only actual installed-program stdout.
        stderr = plain_bytes(control / 'case.stderr').decode('utf-8')  # Preserve diagnostics separately from JSONL proof records.
        record['stdout_sha256'] = hashlib.sha256(stdout.encode()).hexdigest()  # Bind recovered streams to guest hashes.
        record['stderr_sha256'] = hashlib.sha256(stderr.encode()).hexdigest()  # A receipt cannot substitute for the actual files.
        require(admitted and record['runtime'] == {'Result': 'success', 'ExecMainCode': '1', 'ExecMainStatus': '0'}, 'container runtime did not complete successfully')  # A powered-off failed guest must remain failed.
        require(expected_sources == {path: sha(source_root / path) for path in source_files}, 'fixture source changed during execution')  # Keep source identity stable across the entire lane.
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError, tarfile.TarError) as error:  # Convert every expected setup/runtime failure to retained evidence.
        record['errors'].append(str(error)[-2000:])  # Preserve the cause without hiding later cleanup failures.
    finally:  # Service, daemon and filesystem retirement independently gate success.
        try:  # Stop only an exact verified owned transient service.
            if attempted:  # No service mutation occurred before ownership was armed.
                values = unit_state(unit, runner, log, label)  # Revalidate identity after any partial startup.
                if values['LoadState'] != 'not-found':  # A missing unit needs no stop request.
                    require(values['Description'] == description and values['Transient'] == 'yes', 'refusing to stop a foreign container service')  # Never stop a substituted name.
                    if values['ControlGroup']:  # Preserve the actual group for post-stop absence.
                        require(cgroup_path(values['ControlGroup']) == unit_group, 'refusing cleanup of a foreign cgroup')  # Never remove or alter a shared ancestor.
                    call(['/usr/bin/systemctl', '--no-block', 'stop', unit], runner, log, label, 15)  # Systemd owns descendant termination and reap.
                deadline = time.monotonic() + 30  # Bound actual physical retirement.
                while time.monotonic() < deadline:  # Query the same unit through a successful manager readback.
                    values = unit_state(unit, runner, log, label)  # Do not turn a query failure into absence.
                    if values['LoadState'] == 'not-found' or values['ActiveState'] in ('inactive', 'failed'):  # The unit must be non-running.
                        if not unit_group.exists():  # Check the exact authored path even when startup returned no ControlGroup.
                            record['cleanup']['unit_retired'] = True  # Retain manager retirement evidence.
                            record['cleanup']['cgroup_absent'] = True  # Retain independent kernel absence.
                            break  # Cleanup never writes cgroup.kill or subtree_control on the host.
                    time.sleep(0.05)  # Avoid spinning during systemd reap.
                require(record['cleanup']['cgroup_absent'], 'owned container service or cgroup survived cleanup')  # Never claim success after a stop timeout.
            else:  # No transient service was attempted.
                record['cleanup']['unit_retired'] = True  # There is no owned service to terminate.
                record['cleanup']['cgroup_absent'] = True  # No owned runtime domain was created.
        except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:  # Retain cleanup failure alongside the original cause.
            record['errors'].append('service cleanup: ' + str(error)[-1600:])  # The lane remains failed.
        try:  # Preserve container boot and shutdown messages even after a startup failure.
            if attempted:  # Read only the journal of the freshly allocated fixture unit.
                call(['/usr/bin/journalctl', '--no-pager', '--unit=' + unit, '--output=short-iso', '-n', '200'], runner, log, label, 15)  # Keep bounded manager diagnostics in the original lane log.
        except (OSError, ValueError, subprocess.SubprocessError) as error:  # A missing required transcript remains visible.
            record['errors'].append('journal recovery: ' + str(error)[-1600:])  # Never call failed evidence recovery successful qualification.
        try:  # Recover and remove a partial preparation container by exact nonce custody.
            if work is not None:  # Preflight failure did not create any Docker resource.
                identity = docker_owned(name, nonce, runner, log, label)  # Verify ownership before removal.
                if identity is not None:  # Only this fixture's container may be force-removed after timeout.
                    call(docker + ['rm', '--force', identity], runner, log, label, 30)  # Never prune images or stop other containers.
                require(name not in docker_inventory(runner, log, label), 'preparation container survived cleanup')  # Require successful daemon absence readback.
            record['cleanup']['docker_removed'] = True  # No running preparation resource remains.
        except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:  # Daemon failure prevents successful cleanup claims.
            record['errors'].append('Docker cleanup: ' + str(error)[-1600:])  # Retain uncertainty explicitly.
        try:  # Recover startup and timeout diagnostics after the exact service has been stopped.
            if work is not None and (work / 'rootfs/var/lib/ilium-container-fixture').is_dir():  # A failed export or extraction may never have booted a guest.
                control = work / 'rootfs/var/lib/ilium-container-fixture'  # Read only the retained private controller output directory.
                if (control / 'result.json').exists():  # A failed guest can still supply complete diagnostic evidence.
                    record['guest'] = plain_json(control / 'result.json')  # Retain its failed state without changing the outer outcome.
                for name in ('case.stdout', 'case.stderr'):  # Keep partial application output after a timeout too.
                    if not (control / name).exists():  # No output file means the package transaction never started.
                        continue  # Do not manufacture application output.
                    data = plain_bytes(control / name)  # Enforce the same recovery bound on failure paths.
                    (log / (label + '-' + name + '.log')).write_bytes(data)  # Preserve raw bytes even if UTF-8 decoding fails.
                    if name == 'case.stdout':  # Preserve the installed probe's original stdout markers and rows.
                        stdout = data.decode('utf-8')  # Invalid output remains a failed contract.
                        record['stdout_sha256'] = hashlib.sha256(data).hexdigest()  # Bind the final recovered stream.
                    else:  # Keep diagnostics separate from the JSONL proof stream.
                        stderr = data.decode('utf-8')  # Do not mix manager output into application stdout.
                        record['stderr_sha256'] = hashlib.sha256(data).hexdigest()  # Bind the final recovered diagnostics.
        except (OSError, ValueError, KeyError) as error:  # Missing required evidence never qualifies a lane.
            record['errors'].append('diagnostic recovery: ' + str(error)[-1600:])  # Preserve this independent failure.
        try:  # Preserve failed live state rather than deleting through a mount.
            if work is not None:  # Remove only a directory allocated by this invocation.
                require(record['cleanup']['cgroup_absent'] and record['cleanup']['docker_removed'], 'live fixture resources prevent filesystem cleanup')  # Teardown precedes deletion.
                mounts = [line.split()[4] for line in Path('/proc/self/mountinfo').read_text().splitlines()]  # Inspect host-visible mounts before recursive deletion.
                require(not any(path == str(work) or path.startswith(str(work) + '/') for path in mounts), 'owned fixture directory still contains a mount')  # Never delete through a mounted tree.
                require(not work.is_symlink(), 'owned fixture directory was replaced')  # Preserve scope ownership.
                shutil.rmtree(work)  # Remove only the now-retired private rootfs, export and immutable inputs.
                require(not os.path.lexists(work), 'private fixture state survived cleanup')  # Verify actual removal.
            record['cleanup']['state_removed'] = True  # Record completion only after absence.
        except (OSError, ValueError) as error:  # Filesystem cleanup failure also rejects acceptance.
            record['errors'].append('state cleanup: ' + str(error)[-1600:])  # Preserve the retained workspace for diagnosis.
    if not record['errors']:  # All mutations have completed or retired before a passing record is considered.
        record['state'] = 'passed'  # The independent validator must still accept every observation.
        try:  # A missing capability field or guest failure cannot be hidden by outer success.
            require(stdout.count('ILIUM_ANIMATION_BEGIN\n') == 1 and stdout.count('ILIUM_ANIMATION_END\n') == 1, 'installed-animation delimiters are absent or ambiguous')  # Bind the real probe stream to this completed fixture.
            probe = stdout.split('ILIUM_ANIMATION_BEGIN\n', 1)[1].split('ILIUM_ANIMATION_END\n', 1)[0]  # Keep the existing four-record probe contract unchanged.
            record['animation_stdout_sha256'] = hashlib.sha256(probe.encode('utf-8')).hexdigest()  # The independent seal must match these exact probe bytes.
            validate_record(record, kind, image, arch, expected_sources)  # Apply the same contract used by release sealing.
        except (ValueError, KeyError, TypeError) as error:  # Incomplete evidence remains failed.
            record['state'] = 'failed'  # Revoke the tentative state before writing evidence.
            record['errors'].append('fixture evidence: ' + str(error)[-1600:])  # Explain the exact evidence gap.
    if work is not None and work.exists():  # Expose only a genuinely retained failed private scope.
        record['retained_work'] = str(work)  # Operators can recover diagnostics without guessing paths.
    log.mkdir(parents=True, exist_ok=True)  # Preserve the existing per-lane log destination.
    with (log / (label + '-fixture.json')).open('x', encoding='utf-8') as stream:  # Never overwrite an earlier lane's evidence.
        json.dump(record, stream, indent=2, sort_keys=True)  # Retain complete fixture observations on success or failure.
        stream.write('\n')  # Keep the final record complete.
    result = subprocess.CompletedProcess(command, 0 if record['state'] == 'passed' else 1, stdout, stderr + ('\n' + '\n'.join(record['errors']) if record['errors'] else ''))  # Preserve application output and sticky failure.
    return result, record  # The caller must still validate both real installed animation renders.
