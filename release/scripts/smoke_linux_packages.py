#!/usr/bin/env python3
"""Inspect, install and run the Linux packages built by build_linux_packages.py.

`inspect` never executes package bytes: it unpacks every package and compares the
payload with the audited file hashes in the build receipt, so it also covers an
architecture the current machine cannot run. `containers` installs the deb, rpm
and AppImage packages in disposable docker containers of several distributions.
`host` installs on the running machine, which must be disposable (a GitHub-hosted
runner or a virtual machine): Snap needs snapd and root, Flatpak a real sandbox,
and the AppImage a real FUSE mount. Every install runs the same lifecycle test
(`release/packaging/linux/lifecycle.sh`) as an unprivileged user. Stdout is JSONL.
"""
from __future__ import annotations

import io
import os
from pathlib import Path
import platform
import shlex
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import build_linux_packages as packages
import release_tool

ROOT = Path(__file__).resolve().parents[2]
LIFECYCLE = ROOT / 'release/packaging/linux/lifecycle.sh'
DEB_IMAGES = ('ubuntu:22.04', 'ubuntu:24.04', 'debian:12')
RPM_IMAGES = ('fedora:41', 'opensuse/leap:15.6')
APPIMAGE_IMAGE = 'ubuntu:24.04'
UNPRIVILEGED = 'setpriv --reuid=65534 --regid=65534 --clear-groups'


def require(condition, message):
    if not condition:
        raise release_tool.ReleaseError(message)


def emit(kind, **values):
    release_tool.emit({'type': kind, **values})


def load_receipt(directory, architecture):
    directory = Path(directory)
    receipt = release_tool.read_json(directory / packages.receipt_name(architecture))
    require(receipt.get('schema') == 1 and receipt.get('arch') == architecture, 'package receipt is for another architecture')
    for name, digest in receipt['packages'].items():
        require(packages.sha(directory / name) == digest, 'package bytes differ from the receipt: ' + name)
    return receipt


def squashfs_offset(path):
    """Size of the ELF runtime an AppImage starts with: the end of its section header table."""
    with Path(path).open('rb') as source:
        header = source.read(64)
    require(header[:4] == b'\x7fELF' and header[4] == 2, 'AppImage does not start with an ELF64 runtime')
    section_offset = struct.unpack_from('<Q', header, 0x28)[0]
    section_size, section_count = struct.unpack_from('<HH', header, 0x3A)
    return section_offset + section_size * section_count


def unpack(package_format, path, destination):
    destination = Path(destination)
    if package_format == 'deb':
        data = Path(path).read_bytes()
        require(data[:8] == b'!<arch>\n', 'deb is not an ar archive')
        position, members = 8, {}
        while position < len(data):
            header = data[position:position + 60]
            name, size = header[:16].decode().strip().rstrip('/'), int(header[48:58])
            members[name] = data[position + 60:position + 60 + size]
            position += 60 + size + size % 2
        require(list(members)[:1] == ['debian-binary'] and members['debian-binary'] == b'2.0\n', 'deb member order is wrong')
        with tarfile.open(fileobj=io.BytesIO(members['data.tar.xz']), mode='r:xz') as archive:
            archive.extractall(destination, filter='tar')
    elif package_format == 'rpm':
        require(shutil.which('rpm2cpio') and shutil.which('cpio'), 'rpm2cpio and cpio are required to inspect an rpm')
        destination.mkdir(parents=True)
        converted = subprocess.run(['rpm2cpio', str(path)], capture_output=True, check=True).stdout
        subprocess.run(['cpio', '-idm', '--quiet'], input=converted, cwd=destination, check=True, capture_output=True)
    elif package_format in ('appimage', 'snap'):
        require(shutil.which('unsquashfs') is not None, 'unsquashfs is required to inspect an AppImage or snap')
        offset = squashfs_offset(path) if package_format == 'appimage' else 0
        subprocess.run(['unsquashfs', '-quiet', '-no-progress', '-o', str(offset), '-d', str(destination), str(path)], check=True, capture_output=True)
    else:
        raise release_tool.ReleaseError('no offline unpacker for ' + package_format)


def payload_root(package_format, tree):
    tree = Path(tree)
    return tree / {'deb': 'usr/lib/ilium', 'rpm': 'usr/lib/ilium', 'appimage': 'usr/lib/ilium', 'snap': 'lib/ilium'}[package_format]


def notices_path(package_format, tree):
    tree = Path(tree)
    return tree / ('share/doc/ilium' if package_format == 'snap' else 'usr/share/doc/ilium') / 'THIRD-PARTY.txt'


def inspect_package(package_format, path, receipt, architecture):
    """Unpack `path` and require its payload to equal the audited files byte for byte."""
    with tempfile.TemporaryDirectory(prefix='ilium-inspect-') as temporary:
        tree = Path(temporary) / 'tree'
        unpack(package_format, path, tree)
        root = payload_root(package_format, tree)
        expected = dict(receipt['package_files'])
        found = {}
        for name in sorted(expected):
            candidate = notices_path(package_format, tree) if name == 'THIRD-PARTY.txt' else root / name
            require(candidate.is_file() and not candidate.is_symlink(), '%s package lacks %s' % (package_format, name))
            found[name] = packages.sha(candidate)
        require(found == expected, '%s package payload differs from the audited files: %s' % (package_format, sorted(name for name in expected if found[name] != expected[name])))
        extra = sorted(set(item.name for item in root.iterdir()) - set(expected))
        require(not extra, '%s package carries unaudited files: %s' % (package_format, extra))
        return {'files': len(found)}


def inspect(arguments):
    receipt = load_receipt(arguments.packages, arguments.arch)
    formats = [item for item in packages.FORMATS if item in arguments.formats.split(',') and item != 'flatpak']
    failed = 0
    for package_format in formats:
        name = packages.package_name(arguments.arch, package_format)
        try:
            detail = inspect_package(package_format, arguments.packages / name, receipt, arguments.arch)
            emit('result', command='inspect', format=package_format, package=name, state='passed', **detail)
        except (release_tool.ReleaseError, subprocess.SubprocessError, OSError, tarfile.TarError) as error:
            failed += 1
            emit('result', command='inspect', format=package_format, package=name, state='failed', error=str(error)[:800])
    return failed


def expected_hashes(receipt):
    """`sha256sum -c` lines for the deb/rpm install paths of every audited file."""
    return ''.join('%s  %s\n' % (digest, '/usr/share/doc/ilium/THIRD-PARTY.txt' if name == 'THIRD-PARTY.txt' else '/usr/lib/ilium/' + name) for name, digest in sorted(receipt['package_files'].items()))


def report(command, package_format, environment, result, extra=None):
    state = 'passed' if result.returncode == 0 else 'failed'
    values = dict(command=command, format=package_format, environment=environment, state=state, **(extra or {}))
    if state == 'failed':
        values['error'] = (result.stdout + result.stderr)[-1200:]
    emit('result', **values)
    return 0 if state == 'passed' else 1


def deb_script(name, version):
    return f'''
export DEBIAN_FRONTEND=noninteractive
rm -f /etc/dpkg/dpkg.cfg.d/excludes
apt-get update -qq >/dev/null
apt-get install -y -qq /packages/{name} >/dev/null
dpkg -s ilium | grep -q '^Status: install ok installed'
(cd / && sha256sum -c /smoke/expected.sha256)
test "$(ilium --version)" = "ilium {version}"
test "$(ilium-server --version)" = "ilium-server {version}"
test "$(readlink -f /usr/bin/ilium)" = /usr/lib/ilium/ilium
{UNPRIVILEGED} sh /smoke/lifecycle.sh ilium
apt-get remove -y -qq ilium >/dev/null
test ! -e /usr/lib/ilium/ilium && test ! -e /usr/bin/ilium
'''


def rpm_script(image, name, version):
    install = 'zypper --non-interactive --no-gpg-checks install --allow-unsigned-rpm /packages/%s >/dev/null' % name if 'suse' in image else 'dnf install -y -q --setopt=tsflags= /packages/%s' % name
    remove = 'zypper --non-interactive remove ilium >/dev/null' if 'suse' in image else 'dnf remove -y -q ilium'
    prelude = '' if 'suse' in image else 'dnf install -y -q util-linux >/dev/null'
    return f'''
{prelude}
{install}
rpm -q ilium
rpm -V ilium
(cd / && sha256sum -c /smoke/expected.sha256)
test "$(ilium --version)" = "ilium {version}"
test "$(readlink -f /usr/bin/ilium)" = /usr/lib/ilium/ilium
{UNPRIVILEGED} sh /smoke/lifecycle.sh ilium
{remove}
test ! -e /usr/lib/ilium/ilium && test ! -e /usr/bin/ilium
'''


def appimage_script(name, version):
    return f'''
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq >/dev/null
apt-get install -y -qq libasound2t64 libssl3t64 libstdc++6 >/dev/null
cp /packages/{name} /tmp/ilium.AppImage
chmod 0755 /tmp/ilium.AppImage
chown 65534:65534 /tmp/ilium.AppImage
export APPIMAGE_EXTRACT_AND_RUN=1
test "$({UNPRIVILEGED} env HOME=/tmp/home XDG_DATA_HOME=/tmp/home/data APPIMAGE_EXTRACT_AND_RUN=1 /tmp/ilium.AppImage --version)" = "ilium {version}"
{UNPRIVILEGED} env APPIMAGE_EXTRACT_AND_RUN=1 sh /smoke/lifecycle.sh /tmp/ilium.AppImage
'''


def containers(arguments):
    require(shutil.which('docker') is not None, 'docker is required')
    receipt = load_receipt(arguments.packages, arguments.arch)
    version = receipt['version']
    log = arguments.log.resolve()
    failed = 0
    with tempfile.TemporaryDirectory(prefix='ilium-smoke-') as temporary:
        smoke = Path(temporary)
        smoke.chmod(0o755)
        shutil.copyfile(LIFECYCLE, smoke / 'lifecycle.sh')
        (smoke / 'expected.sha256').write_text(expected_hashes(receipt), encoding='ascii')
        for shared in smoke.iterdir():
            shared.chmod(0o644)
        mounts = ('%s:/smoke:ro' % smoke,)
        formats = arguments.formats.split(',')
        jobs = []
        if 'deb' in formats:
            name = packages.package_name(arguments.arch, 'deb')
            jobs += [('deb', image, deb_script(name, version)) for image in DEB_IMAGES]
        if 'rpm' in formats:
            name = packages.package_name(arguments.arch, 'rpm')
            jobs += [('rpm', image, rpm_script(image, name, version)) for image in RPM_IMAGES]
        if 'appimage' in formats:
            jobs.append(('appimage', APPIMAGE_IMAGE, appimage_script(packages.package_name(arguments.arch, 'appimage'), version)))
        for package_format, image, script in jobs:
            label = package_format + '-' + image.replace('/', '_').replace(':', '_')
            command = ['docker', 'run', '--rm', '-v', '%s:/packages:ro' % arguments.packages.resolve(), '-v', mounts[0], image, 'sh', '-ec', script]
            result = subprocess.run(command, capture_output=True, text=True, timeout=1800)
            log.mkdir(parents=True, exist_ok=True)
            (log / (label + '.log')).write_text(result.stdout + result.stderr, encoding='utf-8')
            failed += report('containers', package_format, image, result)
    return failed


def sudo():
    return [] if os.geteuid() == 0 else ['sudo', '-n']


def host_run(command, log, label, **options):
    result = subprocess.run(command, capture_output=True, text=True, timeout=1800, **options)
    Path(log).mkdir(parents=True, exist_ok=True)
    (Path(log) / (label + '.log')).write_text('$ %s\n%s%s' % (shlex.join([str(part) for part in command]), result.stdout, result.stderr), encoding='utf-8')
    return result


def host_snap(arguments, receipt, log):
    name = arguments.packages.resolve() / packages.package_name(arguments.arch, 'snap')
    version = receipt['version']
    script = f'''
set -eu
{shlex.join(sudo())} snap wait system seed.loaded
{shlex.join(sudo())} snap install --dangerous --classic {shlex.quote(str(name))}
trap '{shlex.join(sudo())} snap remove ilium >/dev/null 2>&1 || true' EXIT
test "$(ilium --version)" = "ilium {version}"
test "$(ilium.server --version)" = "ilium-server {version}"
sh {shlex.quote(str(LIFECYCLE))} ilium
'''
    result = host_run(['sh', '-ec', script], log, 'snap-host')
    leftover = subprocess.run(['snap', 'list', 'ilium'], capture_output=True, text=True)
    return report('host', 'snap', platform.platform(terse=True), result, {'removed': leftover.returncode != 0})


def host_appimage(arguments, receipt, log):
    name = arguments.packages.resolve() / packages.package_name(arguments.arch, 'appimage')
    version = receipt['version']
    if not (Path('/dev/fuse').exists() and (shutil.which('fusermount3') or shutil.which('fusermount'))):
        # The container smoke already ran the image through extract-and-run; only the FUSE mount is unproven.
        emit('result', command='host', format='appimage', environment=platform.platform(terse=True), state='skipped', reason='no usable FUSE on this host')
        return 0
    script = f'''
set -eu
test "$({shlex.quote(str(name))} --version)" = "ilium {version}"
sh {shlex.quote(str(LIFECYCLE))} {shlex.quote(str(name))}
'''
    return report('host', 'appimage', platform.platform(terse=True), host_run(['sh', '-ec', script], log, 'appimage-host'))


def host_flatpak(arguments, receipt, log):
    name = arguments.packages.resolve() / packages.package_name(arguments.arch, 'flatpak')
    version = receipt['version']
    environment = dict(os.environ)
    # The sandbox drops the caller's environment; only --env reaches the client.
    wrapper = Path(log) / 'flatpak-client.sh'
    Path(log).mkdir(parents=True, exist_ok=True)
    wrapper.write_text('#!/bin/sh\nexec flatpak run --env=XDG_DATA_HOME="$XDG_DATA_HOME" --env=XDG_CONFIG_HOME="$XDG_CONFIG_HOME" %s "$@"\n' % packages.APP_ID, encoding='utf-8')
    wrapper.chmod(0o755)
    if arguments.flatpak_user_dir:
        environment['FLATPAK_USER_DIR'] = str(arguments.flatpak_user_dir)
    script = f'''
set -eu
flatpak --user remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak --user install -y --noninteractive --bundle {shlex.quote(str(name))}
trap 'flatpak --user uninstall -y --noninteractive {packages.APP_ID} >/dev/null 2>&1 || true' EXIT
test "$(flatpak run {packages.APP_ID} --version)" = "ilium {version}"
ILIUM_SMOKE_BASE="$HOME/.cache/ilium-smoke" sh {shlex.quote(str(LIFECYCLE))} {shlex.quote(str(wrapper))}
'''
    return report('host', 'flatpak', platform.platform(terse=True), host_run(['sh', '-ec', script], log, 'flatpak-host', env=environment))


def host_deb(arguments, receipt, log):
    name = arguments.packages.resolve() / packages.package_name(arguments.arch, 'deb')
    version = receipt['version']
    prefix = shlex.join(sudo())
    script = f'''
set -eu
{prefix} apt-get install -y -qq {shlex.quote(str(name))} >/dev/null
trap '{prefix} apt-get remove -y -qq ilium >/dev/null 2>&1 || true' EXIT
test "$(ilium --version)" = "ilium {version}"
sh {shlex.quote(str(LIFECYCLE))} ilium
'''
    return report('host', 'deb', platform.platform(terse=True), host_run(['sh', '-ec', script], log, 'deb-host'))


def host(arguments):
    machine = platform.machine()
    require({'x86_64': {'x86_64', 'AMD64'}, 'aarch64': {'aarch64', 'arm64'}}[arguments.arch] >= {machine}, 'host architecture %s cannot run %s packages' % (machine, arguments.arch))
    receipt = load_receipt(arguments.packages, arguments.arch)
    log = arguments.log.resolve()
    runners = {'snap': host_snap, 'appimage': host_appimage, 'flatpak': host_flatpak, 'deb': host_deb}
    failed = 0
    for package_format in arguments.formats.split(','):
        require(package_format in runners, 'the host smoke does not cover ' + package_format)
        failed += runners[package_format](arguments, receipt, log)
    return failed


def parser():
    result = release_tool.JsonArgumentParser(description=__doc__, allow_abbrev=False)
    commands = result.add_subparsers(dest='command', required=True)
    for name in ('inspect', 'containers', 'host'):
        command = commands.add_parser(name, allow_abbrev=False)
        command.add_argument('--arch', required=True, choices=sorted(packages.ARCHITECTURES))
        command.add_argument('--packages', type=Path, required=True)
        command.add_argument('--formats', default=','.join(packages.FORMATS))
        if name != 'inspect':
            command.add_argument('--log', type=Path, required=True)
        if name == 'host':
            command.add_argument('--flatpak-user-dir', type=Path)
    return result


def main(argv=None):
    try:
        arguments = parser().parse_args(argv)
        failed = {'inspect': inspect, 'containers': containers, 'host': host}[arguments.command](arguments)
        emit('summary', command=arguments.command, state='failed' if failed else 'passed', failed=failed)
        return 1 if failed else 0
    except (ValueError, OSError, KeyError, subprocess.SubprocessError, tarfile.TarError) as error:
        emit('error', message=str(error)[:1200])
        return 1


if __name__ == '__main__':
    sys.exit(main())
