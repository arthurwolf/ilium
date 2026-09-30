#!/usr/bin/env python3
"""Run the Linux package host smoke inside a disposable qemu/KVM Ubuntu virtual machine.

Snap needs snapd with systemd and root, Flatpak a real sandbox, and the AppImage a
real FUSE mount, so these formats cannot be tested in a container and must never
be installed on a developer machine. This boots a throw-away copy-on-write overlay
of a SHA256-verified Ubuntu cloud image, copies the packages and smoke scripts in,
runs `smoke_linux_packages.py host` over SSH, copies the logs back and powers the
machine off. The base image is never modified. Stdout is JSONL.

    vm_smoke.py --packages DIR --arch x86_64 --work DIR --log DIR [--image FILE]
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import secrets
import shutil
import subprocess
import sys
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[3]
IMAGE_URL = 'https://cloud-images.ubuntu.com/releases/24.04/release/'
IMAGE_NAME = 'ubuntu-24.04-server-cloudimg-amd64.img'
SSH_OPTIONS = ['-o', 'StrictHostKeyChecking=no', '-o', 'UserKnownHostsFile=/dev/null', '-o', 'LogLevel=ERROR', '-o', 'ConnectTimeout=5']
USER_DATA = '''#cloud-config
users:
  - name: tester
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    ssh_authorized_keys:
      - {key}
package_update: true
packages: [python3, flatpak, squashfs-tools, libfuse2t64, rpm]
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


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument('--packages', type=Path, required=True)
    parser.add_argument('--arch', default='x86_64', choices=['x86_64'])
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--log', type=Path, required=True)
    parser.add_argument('--image', type=Path)
    parser.add_argument('--formats', default='deb,appimage,snap,flatpak')
    parser.add_argument('--memory', default='6G')
    parser.add_argument('--cpus', default='4')
    arguments = parser.parse_args(argv)
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
    command = ['qemu-system-x86_64', '-enable-kvm', '-cpu', 'host', '-smp', arguments.cpus, '-m', arguments.memory, '-display', 'none',
               '-drive', 'file=%s,if=virtio' % (work / 'overlay.qcow2'), '-drive', 'file=%s,if=virtio,format=raw' % (work / 'seed.img'),
               '-nic', 'user,hostfwd=tcp:127.0.0.1:%d-:22' % port, '-pidfile', work / 'qemu.pid', '-daemonize']
    started = run(command)
    if started.returncode != 0:
        emit('error', message='qemu failed: ' + started.stderr[-500:])
        return 1
    emit('progress', stage='boot', ssh_port=port)
    pid = (work / 'qemu.pid').read_text().strip()
    failed = 1
    try:
        for _ in range(180):
            if ssh(port, key, 'test -e /var/lib/cloud/instance/ilium-ready', timeout=30).returncode == 0:
                break
            time.sleep(5)
        else:
            emit('error', message='virtual machine never finished cloud-init')
            return 1
        emit('progress', stage='ready', os=ssh(port, key, '. /etc/os-release && echo "$PRETTY_NAME $(uname -r)"').stdout.strip())
        for source, target in ((ROOT / 'release/scripts', 'repo/release/scripts'), (ROOT / 'release/packaging', 'repo/release/packaging'), (arguments.packages, 'packages')):
            ssh(port, key, 'mkdir -p ' + str(Path(target).parent))
            copied = run(['scp', *SSH_OPTIONS, '-i', key, '-P', port, '-r', source, 'tester@127.0.0.1:' + target])
            if copied.returncode != 0:
                emit('error', message='scp failed: ' + copied.stderr[-500:])
                return 1
        ssh(port, key, 'cp /home/tester/repo/release/scripts/../../LICENSE /dev/null 2>/dev/null; true')
        scp_license = run(['scp', *SSH_OPTIONS, '-i', key, '-P', port, ROOT / 'LICENSE', 'tester@127.0.0.1:repo/LICENSE'])
        run(['scp', *SSH_OPTIONS, '-i', key, '-P', port, ROOT / 'Cargo.toml', 'tester@127.0.0.1:repo/Cargo.toml'])
        for subcommand in ('inspect', 'host'):
            extra = ' --log /home/tester/log-%s' % subcommand if subcommand == 'host' else ''
            result = ssh(port, key, 'python3 /home/tester/repo/release/scripts/smoke_linux_packages.py %s --arch %s --packages /home/tester/packages --formats %s%s' % (
                subcommand, arguments.arch, arguments.formats if subcommand == 'host' else arguments.formats.replace('snap', 'snap'), extra))
            (log / ('vm-%s.jsonl' % subcommand)).write_text(result.stdout + result.stderr, encoding='utf-8')
            for line in result.stdout.splitlines():
                print(line, flush=True)
            failed = 0 if result.returncode == 0 and (failed == 0 or subcommand == 'inspect') else 1
            if subcommand == 'host':
                run(['scp', *SSH_OPTIONS, '-i', key, '-P', port, '-r', 'tester@127.0.0.1:log-host', log])
        emit('result', command='vm-smoke', state='failed' if failed else 'passed', log=str(log))
        return failed
    finally:
        ssh(port, key, 'sudo poweroff', timeout=30)
        time.sleep(5)
        run(['kill', pid])


if __name__ == '__main__':
    sys.exit(main())
