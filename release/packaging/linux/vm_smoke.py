#!/usr/bin/env python3
"""Run the Linux package host smoke inside a disposable QEMU Ubuntu virtual machine.

Snap needs snapd with systemd and root, Flatpak a real sandbox, and the AppImage a
real FUSE mount, so these formats cannot be tested in a container and must never
be installed on a developer machine. This boots a throw-away copy-on-write overlay
of a SHA256-verified Ubuntu cloud image, copies the packages and smoke scripts in,
runs `smoke_linux_packages.py host` over SSH, copies the logs back and powers the
machine off. The base image is never modified. Stdout is JSONL.

    vm_smoke.py --packages DIR --audit-report FILE --workspace Cargo.toml --manifest release/targets.toml --arch x86_64 --work DIR --log DIR [--image FILE] [--accelerator kvm|tcg]
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import secrets
import shlex  # Quote each guest argument before sending it through SSH's shell.
import shutil
import subprocess
import sys
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import smoke_installed_animation as animation_gate
import validate_animation_smoke as sandbox_gate
import release_tool

IMAGE_URL = 'https://cloud-images.ubuntu.com/releases/22.04/release/'
IMAGE_NAME = 'ubuntu-22.04-server-cloudimg-amd64.img'
SSH_OPTIONS = ['-o', 'StrictHostKeyChecking=no', '-o', 'UserKnownHostsFile=/dev/null', '-o', 'LogLevel=ERROR', '-o', 'ConnectTimeout=5']
USER_DATA = '''#cloud-config
users:
  - name: tester
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    ssh_authorized_keys:
      - {key}
package_update: true
packages: [python3, bubblewrap, ffmpeg, dbus-user-session, flatpak, squashfs-tools, libfuse2, fuse3, rpm, rpm2cpio, cpio, snapd] # Supply the native inspection, mount and manager prerequisites.
runcmd:
  - [touch, /var/lib/cloud/instance/ilium-ready]
'''


def emit(kind, **values):
    print(json.dumps({'type': kind, **values}, separators=(',', ':')), flush=True)


def sha256(path):
    value = hashlib.sha256()
    with Path(path).open('rb') as source:
        while block := source.read(1 << 20):
            value.update(block)
    return value.hexdigest()


def upload_manifest(source, target):
    """Bind every transferred regular file to its exact absolute guest location."""
    source = Path(source)
    if source.is_symlink() or not source.exists():
        raise ValueError('upload source is missing or a symlink: ' + str(source))
    files = sorted(source.rglob('*')) if source.is_dir() else [source]
    result = {}
    for path in files:
        if path.is_symlink():
            raise ValueError('symlink in upload source: ' + str(path))
        if not path.is_file():
            continue
        guest = Path('/home/tester') / target
        if source.is_dir():
            guest /= path.relative_to(source)
        result[str(guest)] = sha256(path)
    if not result:
        raise ValueError('upload has no regular files: ' + str(source))
    return result


def guest_manifest_command(manifest):
    code = ('import hashlib,json,sys; from pathlib import Path; '
            'expected=json.loads(sys.argv[1]); '
            'bad=[name for name,digest in expected.items() if '
            'not Path(name).is_file() or Path(name).is_symlink() or '
            'hashlib.sha256(Path(name).read_bytes()).hexdigest()!=digest]; '
            'print(json.dumps({"type":"result","command":"vm-inputs",'
            '"state":"failed" if bad else "passed","files":len(expected),'
            '"mismatches":bad[:10]})); sys.exit(bool(bad))')
    return shlex.join(['python3', '-c', code, json.dumps(manifest, sort_keys=True)])


def verified_image(path):
    """The base image must match the published SHA256SUMS before it is booted."""
    with urllib.request.urlopen(IMAGE_URL + 'SHA256SUMS', timeout=60) as response:
        lines = response.read().decode().splitlines()
    expected = next(line.split()[0] for line in lines if line.split()[-1].lstrip('*') == IMAGE_NAME)
    if not Path(path).is_file():
        with urllib.request.urlopen(IMAGE_URL + IMAGE_NAME, timeout=60) as response, Path(path).open('wb') as target:
            shutil.copyfileobj(response, target)
    actual = sha256(path)
    if actual != expected:
        raise SystemExit('base image hash differs from the published SHA256SUMS')
    return actual


def run(command, **options):
    return subprocess.run([str(part) for part in command], capture_output=True, text=True, **options)


def ssh(port, key, command, timeout=3600):
    return run(['ssh', *SSH_OPTIONS, '-i', key, '-p', str(port), 'tester@127.0.0.1', command], timeout=timeout)


def format_plan(value):  # Validate the whole request before any image or VM operation.
    requested = value.split(',')  # Preserve the caller's explicit order.
    if not all(requested) or len(requested) != len(set(requested)):  # Reject empty members and duplicates rather than silently shrinking coverage.
        raise ValueError('--formats must contain distinct, nonempty format names')  # A malformed request cannot qualify anything.
    if not set(requested) <= {'deb', 'appimage', 'snap', 'flatpak'}:  # The VM always requires the host lane, which does not support RPM.
        raise ValueError('--formats supports only deb,appimage,snap,flatpak')  # Reject unsupported names before boot.
    return requested, [name for name in requested if name != 'flatpak']  # Flatpak's installed proof belongs to the mandatory host lane.


def capture_command(action):  # Preserve transfer or guest timeout evidence without skipping owned VM teardown.
    try:  # Keep the existing run and SSH adapters usable by callers and fixtures.
        return action()  # Retain the real exit status of completed commands.
    except subprocess.TimeoutExpired as error:  # SSH timeouts can still leave useful guest diagnostics to collect.
        stdout = error.stdout.decode(errors='replace') if isinstance(error.stdout, bytes) else (error.stdout or '')  # Preserve partial command output.
        stderr = error.stderr.decode(errors='replace') if isinstance(error.stderr, bytes) else (error.stderr or '')  # Preserve partial command errors.
        return subprocess.CompletedProcess(error.cmd, 124, stdout, stderr + '\ncommand timed out\n')  # A timeout is always a failed gate.
    except OSError as error:  # A missing transport executable cannot become successful coverage.
        return subprocess.CompletedProcess([], 127, '', str(error) + '\n')  # Let the caller retain the launch failure and stop safely.


def stop_owned_vm(process, port, key):
    """Reap our child before releasing its PID; never signal a detached PID."""
    if process.poll() is None:
        try:
            ssh(port, key, 'sudo poweroff', timeout=30)
        except (OSError, subprocess.TimeoutExpired) as error:
            emit('warning', message='guest shutdown failed; stopping owned QEMU child: ' + str(error)[:500])
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument('--native-directory', type=Path)
    parser.add_argument('--packages', type=Path, required=True)
    parser.add_argument('--audit-report', type=Path, required=True)
    parser.add_argument('--workspace', type=Path, required=True)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--arch', default='x86_64', choices=['x86_64'])
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--log', type=Path, required=True)
    parser.add_argument('--image', type=Path)
    parser.add_argument('--formats', default='deb,appimage,snap,flatpak')
    parser.add_argument('--memory', default='6G')
    parser.add_argument('--cpus', default='4')
    parser.add_argument('--accelerator', choices=['kvm', 'tcg'], default='kvm')
    arguments = parser.parse_args(argv)
    try:  # Reject unusable coverage before creating work or contacting the image server.
        requested, inspected = format_plan(arguments.formats)  # Plan mandatory host coverage and applicable offline coverage together.
    except ValueError as error:  # Preserve argparse's normal invalid-option behavior.
        parser.error(str(error))  # Exit before keys, transfers or QEMU exist.
    work, log = arguments.work.resolve(), arguments.log.resolve()
    work.mkdir(parents=True)
    log.mkdir(parents=True, exist_ok=True)
    base = arguments.image.resolve() if arguments.image else work / IMAGE_NAME
    emit('progress', stage='image', sha256=verified_image(base))
    key = work / 'key'
    run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', key], check=True)
    (work / 'user-data').write_text(USER_DATA.format(key=(work / 'key.pub').read_text().strip()), encoding='utf-8')
    (work / 'meta-data').write_text('instance-id: ilium-%s\nlocal-hostname: ilium-smoke\n' % secrets.token_hex(4), encoding='utf-8')
    run(['cloud-localds', work / 'seed.img', work / 'user-data', work / 'meta-data'], check=True)
    run(['qemu-img', 'create', '-q', '-f', 'qcow2', '-F', 'qcow2' if base.suffix == '.qcow2' else 'qcow2', '-b', base, work / 'overlay.qcow2', '30G'], check=True)
    port = 20000 + secrets.randbelow(20000)
    acceleration = ['-enable-kvm', '-cpu', 'host'] if arguments.accelerator == 'kvm' else ['-accel', 'tcg,thread=multi', '-cpu', 'max']
    command = ['qemu-system-x86_64', *acceleration, '-smp', arguments.cpus, '-m', arguments.memory, '-display', 'none',
               '-drive', 'file=%s,if=virtio' % (work / 'overlay.qcow2'), '-drive', 'file=%s,if=virtio,format=raw' % (work / 'seed.img'),
               '-nic', 'user,hostfwd=tcp:127.0.0.1:%d-:22' % port]
    qemu_log = (log / 'vm-qemu.log').open('x')
    try:
        process = subprocess.Popen([str(part) for part in command], stdout=qemu_log, stderr=subprocess.STDOUT)
    except OSError as error:
        qemu_log.close()
        emit('error', message='qemu failed to start: ' + str(error)[:500])
        return 1
    emit('progress', stage='boot', ssh_port=port, vm_process_id=process.pid)
    failed = 1
    try:
        for attempt in range(180):
            if process.poll() is not None:
                emit('error', message='qemu exited before cloud-init; see ' + str(log / 'vm-qemu.log'))
                return 1
            ready = capture_command(lambda: ssh(port, key, 'test -e /var/lib/cloud/instance/ilium-ready', timeout=30))  # A slow boot can exceed one SSH handshake deadline.
            (log / ('vm-ready-%d.log' % attempt)).write_text(ready.stdout + ready.stderr, encoding='utf-8')  # Preserve every failed readiness attempt before retry or teardown.
            if ready.returncode == 0:
                break
            time.sleep(5)
        else:
            emit('error', message='virtual machine never finished cloud-init')
            return 1
        emit('progress', stage='ready', os=ssh(port, key, '. /etc/os-release && echo "$PRETTY_NAME $(uname -r)"').stdout.strip())
        animation_sources = tuple((ROOT / name, 'repo/' + name) for name in animation_gate.SOURCE_FILES
                                  if not name.startswith('release/scripts/'))  # The smoke hashes each Rust source and Cargo.lock against native audit evidence.
        uploads = ((ROOT / 'release/scripts', 'repo/release/scripts'), (ROOT / 'release/packaging', 'repo/release/packaging'), (arguments.packages.resolve(), 'packages'), (ROOT / 'LICENSE', 'repo/LICENSE'), (arguments.workspace.resolve(), 'repo/Cargo.toml'), (arguments.manifest.resolve(), 'repo/release/targets.toml'), (arguments.audit_report.resolve(), 'repo/native-linux/native-audit.json'), *animation_sources)  # Transfer every required smoke, provenance and package input.
        if arguments.native_directory is not None:
            uploads += ((arguments.native_directory.resolve() / 'candidate', 'repo/native-linux/candidate'),
                        (arguments.native_directory.resolve() / 'evidence', 'repo/native-linux/evidence'),
                        (ROOT / 'ilium-platform/tests/native_animation_sandbox.rs',
                         'repo/ilium-platform/tests/native_animation_sandbox.rs'))
        manifest = {}
        for index, (source, target) in enumerate(uploads):  # Preserve transfer evidence separately from smoke output.
            manifest.update(upload_manifest(source, target))
            prepared = capture_command(lambda: ssh(port, key, shlex.join(['mkdir', '-p', str(Path(target).parent)]), timeout=30))  # Quote the remote directory argument and bound preparation.
            (log / ('vm-copy-%d-prepare.log' % index)).write_text(prepared.stdout + prepared.stderr, encoding='utf-8')  # Retain remote preparation diagnostics.
            if prepared.returncode != 0:  # Do not copy into an unverified guest location.
                emit('error', message='guest directory preparation failed: ' + prepared.stderr[-500:])  # Report the failed prerequisite.
                return 1  # The existing finally block still reaps the owned VM.
            fresh = capture_command(lambda: ssh(port, key, 'test ! -e ' + shlex.quote(target) + ' && test ! -L ' + shlex.quote(target), timeout=30))
            (log / ('vm-copy-%d-fresh.log' % index)).write_text(fresh.stdout + fresh.stderr, encoding='utf-8')
            if fresh.returncode != 0:
                emit('error', message='guest upload destination already exists: ' + target)
                return 1
            copied = capture_command(lambda: run(['scp', *SSH_OPTIONS, '-i', key, '-P', port, '-r', source, 'tester@127.0.0.1:' + target], timeout=600))  # Check all uploads, including LICENSE and Cargo.toml.
            (log / ('vm-copy-%d.log' % index)).write_text(copied.stdout + copied.stderr, encoding='utf-8')  # Keep diagnostics even for a failed file upload.
            if copied.returncode != 0:
                emit('error', message='scp failed: ' + copied.stderr[-500:])
                return 1
        (log / 'vm-inputs-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
        verified = capture_command(lambda: ssh(port, key, guest_manifest_command(manifest), timeout=120))
        (log / 'vm-inputs-verification.jsonl').write_text(verified.stdout + verified.stderr, encoding='utf-8')
        try:
            proof = json.loads(verified.stdout)
        except (ValueError, TypeError):
            proof = None
        expected_proof = {'type': 'result', 'command': 'vm-inputs', 'state': 'passed', 'files': len(manifest), 'mismatches': []}
        if verified.returncode != 0 or proof != expected_proof:
            emit('error', message='guest uploaded input hashes differ or verification is unavailable')
            return 1
        emit('result', command='vm-inputs', state='passed', files=len(manifest))
        if arguments.native_directory is not None:
            command = 'loginctl enable-linger tester && systemctl start user@$(id -u tester).service'
            prepared = capture_command(lambda: ssh(port, key, 'sudo sh -c ' + shlex.quote(command), timeout=60))
            (log / 'vm-sandbox-user-manager.log').write_text(prepared.stdout + prepared.stderr)
            if prepared.returncode != 0:
                emit('error', message='VM sandbox user manager preparation failed')
                return 1
            argv = ['python3', '/home/tester/repo/release/scripts/native_sandbox_runner.py',
                    '--artifact-directory', '/home/tester/repo/native-linux/evidence',
                    '--helper', '/home/tester/repo/native-linux/candidate/ilium-animation-helper',
                    '--workspace', '/home/tester/repo/Cargo.toml',
                    '--output', '/home/tester/log-sandbox']
            executed = capture_command(lambda: ssh(port, key,
                'XDG_RUNTIME_DIR=/run/user/$(id -u) ' + shlex.join(argv), timeout=900))
            (log / 'vm-sandbox-run.jsonl').write_text(executed.stdout + executed.stderr)
            copied = capture_command(lambda: run(['scp', *SSH_OPTIONS, '-i', key, '-P', port,
                '-r', 'tester@127.0.0.1:/home/tester/log-sandbox', log], timeout=120))
            if copied.returncode != 0 or executed.returncode != 0:
                emit('error', message='VM sandbox execution or evidence recovery failed')
                return 1
            artifact_path = arguments.native_directory / 'evidence/native-sandbox-artifact.json'
            artifact = release_tool.read_json(artifact_path)
            sandbox_gate.validate_native_sandbox_receipt(
                release_tool.read_json(log / 'log-sandbox/native-sandbox-tests.json'),
                artifact, sha256(artifact_path), artifact['target'], artifact['tag'])
            emit('result', command='vm-sandbox', state='passed')
        failed = 0  # Applicable inspection and mandatory host results can now only add failures.
        for subcommand in ('inspect', 'host'):
            formats = inspected if subcommand == 'inspect' else requested  # The host always receives every validated requested format.
            if not formats:  # Flatpak-only requests have no supported offline inspector.
                evidence = dict(command='vm-inspect', state='not-applicable', formats=[], required_host_formats=requested, reason='Flatpak has no offline inspector; host acceptance remains required')  # This is explicitly not a passed smoke result.
                (log / 'vm-inspect.jsonl').write_text(json.dumps({'type': 'result', **evidence}, separators=(',', ':')) + '\n', encoding='utf-8')  # Retain the exact applicability decision.
                emit('result', **evidence)  # Make the missing offline lane visible to the caller.
                continue  # The mandatory host iteration still follows.
            guest_command = ['python3', '/home/tester/repo/release/scripts/smoke_linux_packages.py', subcommand, '--arch', arguments.arch, '--packages', '/home/tester/packages', '--formats', ','.join(formats)]  # Build explicit guest argv without shell interpolation.
            if subcommand == 'host':  # Host logs and private Flatpak selection retain their public caller contracts.
                guest_command += ['--log', '/home/tester/log-host']  # Preserve the guest diagnostics destination.
                guest_command += ['--audit-report', '/home/tester/repo/native-linux/native-audit.json', '--workspace', '/home/tester/repo/Cargo.toml', '--manifest', '/home/tester/repo/release/targets.toml']  # Satisfy the real host parser with transferred audit and source inputs.
                if 'flatpak' in requested:  # Lifecycle isolates HOME/XDG, so keep installation lookup stable.
                    guest_command += ['--flatpak-user-dir', '/home/tester/flatpak-install']  # Preserve the primary's explicit private directory.
            result = capture_command(lambda: ssh(port, key, shlex.join(guest_command)))  # A guest timeout remains failed while allowing log collection.
            (log / ('vm-%s.jsonl' % subcommand)).write_text(result.stdout + result.stderr, encoding='utf-8')
            for line in result.stdout.splitlines():
                print(line, flush=True)
            failed = int(bool(failed or result.returncode != 0))  # A later success cannot erase an earlier required-stage failure.
            if subcommand == 'host':
                copied = capture_command(lambda: run(['scp', *SSH_OPTIONS, '-i', key, '-P', port, '-r', 'tester@127.0.0.1:log-host', log], timeout=600))  # Collect host diagnostics after success, failure or SSH timeout.
                (log / 'vm-host-log-copy.log').write_text(copied.stdout + copied.stderr, encoding='utf-8')  # Retain collection errors before VM teardown.
                if copied.returncode != 0:  # Missing required diagnostic evidence cannot qualify the VM run.
                    emit('error', message='host diagnostics copy failed: ' + copied.stderr[-500:])  # Surface the transfer failure alongside native output.
                    failed = 1  # Never overwrite a prior smoke failure with successful cleanup.
        emit('result', command='vm-smoke', state='failed' if failed else 'passed', log=str(log))
        return failed
    finally:
        try:
            stop_owned_vm(process, port, key)
        finally:
            qemu_log.close()


if __name__ == '__main__':
    sys.exit(main())
