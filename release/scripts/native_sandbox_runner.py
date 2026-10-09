#!/usr/bin/env python3
"""Execute checksum-bound sandbox tests in disposable native Linux accounts."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import uuid

CASES = (
    'owned_domain_kills_descendants_and_releases_tasks',
    'kernel_rejects_tasks_beyond_the_owned_domain_limit',
    'kernel_oom_kills_a_real_physical_allocation',
    'ordinary_service_launches_helper_without_manual_delegation',
)


CGROUP_REPORT_PREFIX = 'ILIUM_NATIVE_CGROUP '
CGROUP_WRAPPER = """import json, os, pathlib, sys
rows = pathlib.Path('/proc/self/cgroup').read_text().splitlines()
paths = [row[3:] for row in rows if row.startswith('0::')]
if len(paths) != 1:
    raise SystemExit('native service lacks a unique unified cgroup')
path = paths[0]
info = os.stat('/sys/fs/cgroup' + path, follow_symlinks=False)
print('ILIUM_NATIVE_CGROUP ' + json.dumps({'path': path, 'device': info.st_dev,
      'inode': info.st_ino}), flush=True)
os.execv(sys.argv[1], sys.argv[1:])
"""


def cgroup_identity(output, unit):
    reports = [line[len(CGROUP_REPORT_PREFIX):] for line in output.splitlines()
               if line.startswith(CGROUP_REPORT_PREFIX)]
    if len(reports) != 1:
        raise ValueError('native service lacks exactly one kernel cgroup admission report')
    report = json.loads(reports[0])
    if not isinstance(report, dict) or set(report) != {'path', 'device', 'inode'}:
        raise ValueError('native service kernel cgroup report schema differs')
    group = report.get('path')
    if (not isinstance(group, str) or not group.startswith('/')
            or any(part in ('', '.', '..') for part in group.split('/')[1:])
            or Path(group).name != unit + '.service'
            or any(type(report.get(key)) is not int or report[key] <= 0
                   for key in ('device', 'inode'))):
        raise ValueError('native service kernel cgroup identity differs')
    return report


def kernel_retirement(output, unit, manager_group, root=Path('/sys/fs/cgroup')):
    report = cgroup_identity(output, unit)
    group = report['path']
    if manager_group and manager_group != group:
        raise ValueError('systemd ControlGroup differs from admitted kernel cgroup')
    if not root.is_dir():
        raise ValueError('kernel cgroup mount is unavailable during retirement check')
    target = root / group.lstrip('/')
    try:
        remaining = target.lstat()
    except FileNotFoundError:
        return dict(report, absent=True, manager_control_group=manager_group)
    raise ValueError('native service kernel cgroup remains after cleanup: ' + str(target))


def test_command(binary, case, unit):
    if case not in CASES or not Path(binary).is_absolute():
        raise ValueError('native sandbox test case or executable is invalid')
    if not re.fullmatch(r'ilium-native-test-[a-z0-9-]+', unit):
        raise ValueError('native sandbox service identity is invalid')
    return ['systemd-run', '--user', '--wait', '--pipe', '--collect',
            '--unit=' + unit, '--property=Delegate=no',
            '--property=RuntimeMaxSec=120', '/usr/bin/python3', '-c',
            CGROUP_WRAPPER, str(binary), case,
            '--exact', '--ignored', '--test-threads=1', '--nocapture']


def case_passed(case, exit_code, output):
    if case not in CASES or exit_code != 0:
        return False
    executed = re.findall(r'^test ([A-Za-z0-9_]+) \.\.\. ok$', output, re.MULTILINE)
    summaries = re.findall(r'^test result: ok\. (\d+) passed; (\d+) failed; '
                           r'(\d+) ignored; (\d+) measured;', output, re.MULTILINE)
    return executed == [case] and summaries == [('1', '0', '0', '0')]


def sha(path):
    path = Path(path)
    if not path.is_absolute() or not path.is_file() or path.is_symlink():
        raise ValueError('native sandbox input must be an absolute regular file')
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument('--artifact-directory', type=Path, required=True)
    parser.add_argument('--helper', type=Path, required=True)
    parser.add_argument('--workspace', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args(argv)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    receipt = {'schema': 1, 'state': 'failed', 'scope': 'native-linux-sandbox-tests',
               'publication_allowed': False, 'cases': {}, 'uid': os.getuid()}
    try:
        if platform.system() != 'Linux' or os.getuid() == 0:
            raise ValueError('native sandbox qualification requires a disposable non-root Linux account')
        artifact_directory = args.artifact_directory.resolve(strict=True)
        artifact_path = artifact_directory / 'native-sandbox-artifact.json'
        sha(artifact_path)
        artifact = json.loads(artifact_path.read_text())
        machine = platform.machine()
        targets = {'x86_64-unknown-linux-gnu': {'x86_64', 'AMD64'},
                   'aarch64-unknown-linux-gnu': {'aarch64', 'arm64'}}
        if (artifact.get('schema') != 1 or artifact.get('state') != 'compiled-not-qualified'
                or artifact.get('executed') is not False or artifact.get('filename') != 'native-sandbox-test-binary'
                or machine not in targets.get(artifact.get('target'), set())):
            raise ValueError('native sandbox artifact identity or architecture differs')
        binary = artifact_directory / 'sandbox' / artifact['filename']
        helper = args.helper.absolute()
        source = args.workspace.resolve(strict=True).parent / 'ilium-platform/tests/native_animation_sandbox.rs'
        if (sha(binary) != artifact.get('sha256') or sha(helper) != artifact.get('helper_sha256')
                or sha(source) != artifact.get('source_sha256')):
            raise ValueError('native sandbox executable, helper or test source checksum differs')
        if not os.access(binary, os.X_OK) or not os.access(helper, os.X_OK):
            raise ValueError('native sandbox input is not executable')
        receipt.update(target=artifact['target'], tag=artifact['tag'],
                       artifact_receipt_sha256=sha(artifact_path),
                       binary_sha256=sha(binary), helper_sha256=sha(helper),
                       source_sha256=sha(source), system=platform.system(), machine=machine)
        environment = dict(os.environ, XDG_RUNTIME_DIR='/run/user/' + str(os.getuid()),
                           ILIUM_NATIVE_HELPER_TEST_BINARY=str(helper),
                           ILIUM_NATIVE_HELPER_TEST_SHA256=artifact['helper_sha256'])
        for case in CASES:
            unit = 'ilium-native-test-' + uuid.uuid4().hex
            command = test_command(binary, case, unit)
            # systemd-run does not forward arbitrary environment variables to the service.
            command[1:1] = ['--setenv=ILIUM_NATIVE_HELPER_TEST_BINARY=' + str(helper),
                            '--setenv=ILIUM_NATIVE_HELPER_TEST_SHA256=' + artifact['helper_sha256']]
            existing = subprocess.run(['systemctl', '--user', 'show', unit + '.service',
                                       '--property=LoadState', '--value'], env=environment,
                                      text=True, capture_output=True, timeout=15)
            absent = (existing.returncode == 0 and existing.stdout.strip() == 'not-found'
                      or existing.returncode != 0 and 'not found' in existing.stderr.lower())
            if not absent:
                raise ValueError('native sandbox service identity is not verified fresh: ' + unit)
            print(json.dumps({'type': 'progress', 'case': case, 'unit': unit, 'output': str(output)}), flush=True)
            cleanup_error = None
            try:
                result = subprocess.run(command, env=environment, text=True, capture_output=True, timeout=180)
            except subprocess.TimeoutExpired as error:
                stdout = error.stdout.decode(errors='replace') if isinstance(error.stdout, bytes) else (error.stdout or '')
                stderr = error.stderr.decode(errors='replace') if isinstance(error.stderr, bytes) else (error.stderr or '')
                result = subprocess.CompletedProcess(command, 124, stdout, stderr + '\nnative sandbox test timed out\n')
            finally:
                # Only the fresh UUID identity admitted above belongs to this invocation.
                try:
                    stopped = subprocess.run(['systemctl', '--user', 'stop', unit + '.service'],
                                             env=environment, text=True, capture_output=True, timeout=30)
                    if stopped.returncode != 0 and 'not loaded' not in stopped.stderr.lower() and 'not found' not in stopped.stderr.lower():
                        cleanup_error = stopped.stderr or 'owned native test service could not be stopped'
                except (OSError, subprocess.SubprocessError) as error:
                    cleanup_error = str(error)
            log = output / (case + '.log')
            log.write_text(result.stdout + result.stderr)
            state = subprocess.run(['systemctl', '--user', 'show', unit + '.service',
                                    '--property=ActiveState', '--value'], env=environment,
                                   text=True, capture_output=True, timeout=15)
            absent = state.returncode != 0 and 'not found' in state.stderr.lower()
            retired = absent or state.returncode == 0 and state.stdout.strip() in ('inactive', 'failed')
            kernel = None
            try:
                group_state = subprocess.run(
                    ['systemctl', '--user', 'show', unit + '.service',
                     '--property=ControlGroup', '--value'], env=environment,
                    text=True, capture_output=True, timeout=15)
                if group_state.returncode and 'not found' not in group_state.stderr.lower():
                    raise ValueError('systemd ControlGroup readback failed')
                kernel = kernel_retirement(result.stdout, unit, group_state.stdout.strip())
            except (OSError, ValueError, subprocess.SubprocessError) as error:
                cleanup_error = str(error)
            passed = (case_passed(case, result.returncode, result.stdout) and retired
                      and kernel is not None and cleanup_error is None)
            receipt['cases'][case] = {'state': 'passed' if passed else 'failed',
                                       'exit_code': result.returncode, 'unit': unit + '.service',
                                       'delegate': False, 'unit_retired': retired,
                                       'unit_fresh': True, 'kernel_retirement': kernel,
                                       'cleanup_error': cleanup_error,
                                       'stdout': result.stdout, 'stderr': result.stderr,
                                       'log_sha256': sha(log), 'command': command}
        stable = sha(binary) == receipt['binary_sha256'] and sha(helper) == receipt['helper_sha256'] and sha(source) == receipt['source_sha256']
        receipt['inputs_stable'] = stable
        if stable and all(row['state'] == 'passed' for row in receipt['cases'].values()):
            receipt['state'] = 'passed'
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        receipt['error'] = str(error)
    path = output / 'native-sandbox-tests.json'
    path.write_text(json.dumps(receipt, sort_keys=True, indent=2) + '\n')
    print(json.dumps({'type': 'result', 'state': receipt['state'], 'receipt': str(path)}), flush=True)
    return int(receipt['state'] != 'passed')


if __name__ == '__main__':
    raise SystemExit(main())
