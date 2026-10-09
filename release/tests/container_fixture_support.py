"""Synthetic records for adapter regressions; these are never native qualification."""  # Keep evidence scope explicit.
import hashlib  # Bind supplied test streams to their actual UTF-8 bytes.
import json  # Produce complete test JSONL rather than fixed passing parser results.
from pathlib import Path, PurePosixPath  # Separate Linux receipt paths from the source runner's filesystem.


def linux_record_path(*parts):  # Preserve Linux proof spelling on Windows without redirecting actual test files.
    spelling = PurePosixPath(*(str(part).replace('\\', '/') for part in parts)).as_posix()  # Normalize only for identifying synthetic Linux paths.
    if spelling.startswith(('/usr/lib/ilium', '/owned/appimage', '/sys/fs/cgroup')):  # These paths are read-only receipt identities in the consuming tests.
        return PurePosixPath(spelling)  # Keep absolute Linux ancestry and installed-path checks meaningful.
    return Path(*parts)  # Real temporary files, source hashes and audit reads retain native filesystem behavior.


def probe_stdout(files, approved, kind='deb'):  # Model the installed program boundary; the production parser remains real.
    prefix = '/usr/lib/ilium' if kind != 'appimage' else '/owned/appimage/0.1.0-' + files['ilium'][:16]  # Exercise the actual installed-path distinction.
    catalogue = {'type': 'artifact', 'gate': 'installed_catalogue', 'packages': ['beach', 'carpet'], 'client_path': prefix + '/ilium', 'client_sha256': files['ilium'], 'helper_path': prefix + '/ilium-animation-helper', 'helper_sha256': files['ilium-animation-helper']}  # Bind the independent fixture members.
    renders = [{'type': 'artifact', 'gate': 'installed_render', 'package': name, 'archive_sha256': next(value for filename, value in approved.items() if filename.startswith(name + '-')), 'helper_sha256': files['ilium-animation-helper'], 'rendered_frames': 2, 'physical_retirement': True, 'worker_threads_before': 1, 'worker_threads_after': 1, 'worker_bytes_before': 1024, 'worker_bytes_after': 1024} for name in ('beach', 'carpet')]  # Neither installed render may be omitted.
    result = {'type': 'result', 'gate': 'installed_animation', 'state': 'passed', 'publication_allowed': False, 'packages': ['beach', 'carpet']}  # A synthetic success word is insufficient without the preceding records.
    return 'ILIUM_ANIMATION_BEGIN\n' + ''.join(json.dumps(row) + '\n' for row in [catalogue, *renders, result]) + 'ILIUM_ANIMATION_END\n'  # Preserve both delimiters and every final newline.


def fixture_record(kind, image, arch, sources, stdout, stderr=''):  # Build an independent complete positive schema for mutation tests.
    nonce = '1234567890abcdef1234567890abcdef'  # Test-only identity; production still allocates a fresh UUID.
    unit = 'ilium-container-' + nonce + '.service'  # Preserve the ownership relationship under test.
    unit_group = '/sys/fs/cgroup/system.slice/' + unit  # Model one exact delegated host service.
    manager = '/sys/fs/cgroup/system.slice/ilium-container-user.service'  # Model the dedicated guest manager.
    probe = 'ilium-fixture-probe-' + nonce + '.service'  # Model only the sacrificial capability service.
    namespaces = {name: index + 100 for index, name in enumerate(('mnt', 'pid', 'user', 'cgroup', 'net', 'ipc', 'uts'))}  # Distinct synthetic kernel identities.
    mapping = [[0, 524288, 65536]]  # Refuse an identity-map positive fixture.
    identities = {'ubuntu:22.04': ('ubuntu', '22.04'), 'ubuntu:24.04': ('ubuntu', '24.04'), 'debian:12': ('debian', '12'), 'fedora:41': ('fedora', '41'), 'opensuse/leap:15.6': ('opensuse-leap', '15.6')}  # Independently retain every supplied distro baseline.
    namespace = {'namespaces': namespaces, 'uid_map': mapping, 'gid_map': mapping, 'cgroup_mount_inode': 77, 'cgroup_mount_root': '/', 'cgroup_filesystem': 'cgroup2', 'cgroup_writable': True, 'controllers': ['cpu', 'memory', 'pids'], 'caller_cgroup': '/sys/fs/cgroup/system.slice/ilium-container-acceptance.service', 'caller_delegate': False}  # A caller stays nondelegated.
    delegated = {'uid': 65534, 'owner_uid': 65534, 'unit': probe, 'cgroup': manager + '/app.slice/' + probe, 'controllers': ['cpu', 'memory', 'pids'], 'limits': {'memory.max': '402653184', 'memory.swap.max': '0', 'memory.oom.group': '1', 'pids.max': '16', 'cpu.max': '100000 100000'}, 'limited_child_started': True, 'limited_child_reaped': True, 'limited_cgroup_absent': True, 'bwrap_namespaces': True, 'unit_retired': True}  # Keep the original untrusted ceilings literal and independent.
    digest = lambda text: hashlib.sha256(text.encode('utf-8')).hexdigest()  # Hash observations instead of accepting fixed stream digests.
    guest = {'schema': 1, 'nonce': nonce, 'state': 'passed', 'case_exit_code': 0, 'errors': [], 'user_manager_retired': True, 'os_release': dict(zip(('ID', 'VERSION_ID'), identities[image])), 'machine': arch, 'namespace': namespace, 'delegation': delegated, 'manager_cgroup': manager, 'case.stdout_sha256': digest(stdout), 'case.stderr_sha256': digest(stderr)}  # The schema includes execution and retirement separately.
    animation = stdout.split('ILIUM_ANIMATION_BEGIN\n', 1)[-1].split('ILIUM_ANIMATION_END\n', 1)[0]  # Even malformed probe fixtures bind the bytes supplied to the parser.
    return {'synthetic_fixture': True, 'schema': 1, 'state': 'passed', 'engine': 'systemd-nspawn', 'format': kind, 'image': image, 'arch': arch, 'nonce': nonce, 'unit': unit, 'source_files': dict(sources), 'errors': [], 'limits': {'container_memory_bytes': 3221225472, 'container_tasks': 1024, 'manager_memory_bytes': 2147483648, 'manager_tasks': 512, 'caller_memory_bytes': 805306368, 'caller_tasks': 128, 'untrusted_memory_bytes': 402653184, 'untrusted_tasks': 16}, 'origin': {'os': 'linux', 'architecture': 'amd64' if arch == 'x86_64' else 'arm64', 'image_id': 'sha256:' + 'a' * 64, 'repo_digests': [image + '@sha256:' + 'b' * 64], 'prepared_rootfs_sha256': 'c' * 64}, 'guest': guest, 'host_admission': {'init_pid': 1701, 'namespaces': namespaces, 'host_namespaces': {name: value + 100 for name, value in namespaces.items()}, 'uid_map': mapping, 'gid_map': mapping, 'owned_unit_cgroup': unit_group, 'visible_cgroup_root': unit_group, 'visible_cgroup_inode': 77, 'rootfs_inode': 88, 'pidfd_live_at_admission': True}, 'runtime': {'Result': 'success', 'ExecMainCode': '1', 'ExecMainStatus': '0'}, 'cleanup': {'unit_retired': True, 'cgroup_absent': True, 'docker_removed': True, 'state_removed': True}, 'stdout_sha256': digest(stdout), 'stderr_sha256': digest(stderr), 'animation_stdout_sha256': digest(animation)}  # Consumers must still run the real render parser and source/stream checks.
