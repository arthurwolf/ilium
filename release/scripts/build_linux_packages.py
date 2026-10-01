#!/usr/bin/env python3
"""Build the Linux deb, rpm, AppImage, Flatpak and Snap packages from the audited tarball.

Every package repackages the exact audited bytes of `ilium-linux-<arch>.tar.gz`;
`release_pipeline.py aggregate` refuses a receipt whose file hashes differ from
the native audit. Nothing here executes the audited binaries: the dependency
declarations are derived by parsing their ELF dynamic sections, and each
reviewed system library must be mapped explicitly. Stdout is JSONL.

`build` needs `mksquashfs` (AppImage), `rpmbuild` (rpm), `flatpak` (Flatpak) and
`snap` (Snap) on the native runner of the tarball's architecture. `smoke_linux_packages.py`
installs and runs the results.
"""
from __future__ import annotations

import hashlib
import io
import json
import lzma
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tarfile
import urllib.request

sys.path.insert(0, str(Path(__file__).resolve().parent))
import release_tool

ROOT = Path(__file__).resolve().parents[2]
PACKAGING = ROOT / 'release/packaging/linux'
FORMATS = ('deb', 'rpm', 'appimage', 'flatpak', 'snap')
EXTENSIONS = {'deb': 'deb', 'rpm': 'rpm', 'appimage': 'AppImage', 'flatpak': 'flatpak', 'snap': 'snap'}
# Native names of each target architecture inside every packaging ecosystem.
ARCHITECTURES = {
    'x86_64': {'deb': 'amd64', 'rpm': 'x86_64', 'snap': 'amd64', 'flatpak': 'x86_64', 'appimage': 'x86_64', 'interpreter': 'ld-linux-x86-64.so.2'},
    'aarch64': {'deb': 'arm64', 'rpm': 'aarch64', 'snap': 'arm64', 'flatpak': 'aarch64', 'appimage': 'aarch64', 'interpreter': 'ld-linux-aarch64.so.1'},
}
APP_ID = 'io.github.arthurwolf.Ilium'
# flatpak 1.12/1.14 (Ubuntu 22.04/24.04) resolve an unqualified `flatpak run APP` to the `master` branch only.
FLATPAK_BRANCH = 'master'
PUBLISHER = 'Arthur Wolf'
MAINTAINER = 'Arthur Wolf <noreply@github.com>'
HOMEPAGE = 'https://github.com/arthurwolf/ilium'
SUMMARY = 'Terminal multiplexer for AI coding agents'
DESCRIPTION = ('Ilium is a tmux-like terminal multiplexer that keeps every pane, project session and AI coding '
               'agent in one tree, and shows which agents are working, idle or waiting for approval.')
# One fixed instant for every embedded timestamp: packages must rebuild byte-identically.
EPOCH = 946684800
EPOCH_DATE = '2000-01-01'
LIBRARY_DIRECTORY = 'usr/lib/ilium'
VERSION_PATTERN = re.compile(r'\A[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?\Z')
MEMBER_PATTERN = re.compile(r'\A(?:ilium|ilium-server|VERSION|THIRD-PARTY\.txt|lib[A-Za-z0-9_+-]+\.so(?:\.[0-9]+)*)\Z')
EXECUTABLE_MEMBERS = ('ilium', 'ilium-server')
MAX_MEMBER_BYTES = 1_073_741_824
MAX_TOTAL_BYTES = 2_000_000_000
# The tarball is built on the ubuntu-22.04 runner; nothing it needs may exceed that glibc.
MAXIMUM_GLIBC = (2, 35)
# Reviewed system sonames and the package that provides each in every ecosystem.
# A NEEDED entry that is neither bundled nor listed here is an unreviewed dependency.
SYSTEM_LIBRARIES = {
    'libc.so.6': 'glibc', 'libm.so.6': 'glibc', 'libdl.so.2': 'glibc', 'librt.so.1': 'glibc',
    'libpthread.so.0': 'glibc', 'ld-linux-x86-64.so.2': 'glibc', 'ld-linux-aarch64.so.1': 'glibc',
    'libgcc_s.so.1': 'libgcc', 'libstdc++.so.6': 'libstdcxx', 'libasound.so.2': 'alsa',
    'libssl.so.3': 'openssl', 'libcrypto.so.3': 'openssl',
}
DEB_PACKAGES = {'libgcc': 'libgcc-s1', 'libstdcxx': 'libstdc++6', 'alsa': 'libasound2t64 | libasound2', 'openssl': 'libssl3t64 | libssl3'}
APPRUN = '''#!/bin/sh
# Ilium AppImage entry point. The session server daemonises and must outlive this
# AppImage's FUSE mount, so the payload is copied once into a per-version user
# directory and executed from there.
set -eu
here=$(dirname "$(readlink -f "$0")")
data_home=${XDG_DATA_HOME:-${HOME:?HOME is not set}/.local/share}
store=$data_home/ilium/appimage
target=$store/@VERSION@-@IDENT@
if [ ! -x "$target/ilium" ]; then
    mkdir -p "$store"
    staging=$(mktemp -d "$store/.stage.XXXXXX")
    cp -p "$here/usr/lib/ilium"/* "$staging"/
    if [ -d "$target" ]; then rm -rf "$staging"; else mv "$staging" "$target"; fi
fi
exec "$target/ilium" "$@"
'''


def require(condition, message):
    if not condition:
        raise release_tool.ReleaseError(message)


def emit(kind, **values):
    release_tool.emit({'type': kind, **values})


def sha(path):
    value = hashlib.sha256()
    with Path(path).open('rb') as source:
        while block := source.read(1024 * 1024):
            value.update(block)
    return value.hexdigest()


def package_name(architecture, package_format):
    return 'ilium-linux-%s.%s' % (architecture, EXTENSIONS[package_format])


def package_names(architecture):
    return tuple(package_name(architecture, package_format) for package_format in FORMATS)


def receipt_name(architecture):
    return 'linux-packages-%s.json' % architecture


def version_from_tag(tag):
    require(isinstance(tag, str) and tag.startswith('v') and VERSION_PATTERN.fullmatch(tag[1:]), 'package versions must be vMAJOR.MINOR.PATCH with an optional pre-release suffix')
    return tag[1:]


def deb_version(version):
    """Debian orders `~` before the release, so a pre-release sorts below its final."""
    return version.replace('-', '~', 1)


def snap_version(version):
    require(len(version) <= 32, 'snap versions are limited to 32 characters')
    return version


def extract_package(archive, architecture, version, destination):
    """Extract the validated tarball into a new flat directory; returns {name: sha256}."""
    archive, destination = Path(archive), Path(destination)
    require(archive.is_file() and not archive.is_symlink(), 'package archive must be a regular file')
    require(not destination.exists(), 'package directory must be new')
    prefix = 'ilium-linux-' + architecture
    destination.mkdir(parents=True)
    seen, total, count = set(), 0, 0
    with tarfile.open(archive, 'r:gz') as package:
        for member in package:
            count += 1
            require(count <= 64, 'unexpected tar member count')
            if member.name == prefix:
                require(member.isdir(), 'archive prefix must be a directory')
                continue
            require(member.name.startswith(prefix + '/') and member.isreg() and not member.linkname, 'unexpected tar member: ' + member.name)
            name = member.name[len(prefix) + 1:]
            require(MEMBER_PATTERN.fullmatch(name) and name not in seen, 'unsafe, duplicate or unexpected tar member: ' + name)
            require(0 < member.size <= MAX_MEMBER_BYTES, 'tar member size exceeds bound: ' + name)
            total += member.size
            require(total <= MAX_TOTAL_BYTES, 'tar expanded size exceeds bound')
            seen.add(name)
            source = package.extractfile(member)
            require(source is not None, 'tar member is unreadable: ' + name)
            (destination / name).write_bytes(source.read())
    require({'ilium', 'ilium-server', 'VERSION', 'THIRD-PARTY.txt'} <= seen, 'tarball is missing a required member')
    require((destination / 'VERSION').read_bytes() == (version + '\n').encode(), 'tarball VERSION differs from the release version')
    return {name: sha(destination / name) for name in sorted(seen)}


def elf_needed(path):
    """DT_NEEDED sonames of a little-endian ELF64 file, in link order."""
    data = Path(path).read_bytes()
    require(data[:4] == b'\x7fELF' and data[4] == 2 and data[5] == 1, 'not a little-endian ELF64: ' + Path(path).name)
    program_offset = struct.unpack_from('<Q', data, 0x20)[0]
    entry_size, entry_count = struct.unpack_from('<HH', data, 0x36)
    loads, dynamic = [], None
    for index in range(entry_count):
        kind, _flags, offset, address, _physical, file_size, _memory, _align = struct.unpack_from('<IIQQQQQQ', data, program_offset + index * entry_size)
        if kind == 1:
            loads.append((address, offset, file_size))
        elif kind == 2:
            dynamic = (offset, file_size)
    if dynamic is None:
        return []

    def file_offset(address):
        for start, offset, size in loads:
            if start <= address < start + size:
                return offset + address - start
        raise release_tool.ReleaseError('dynamic string table is outside every loadable segment')

    needed, string_table = [], None
    for position in range(dynamic[0], dynamic[0] + dynamic[1], 16):
        tag, value = struct.unpack_from('<qQ', data, position)
        if tag == 0:
            break
        if tag == 5:
            string_table = value
        elif tag == 1:
            needed.append(value)
    require(string_table is not None, 'ELF has no dynamic string table')
    base = file_offset(string_table)
    return [data[base + offset:data.index(b'\0', base + offset)].decode('ascii') for offset in needed]


def glibc_requirement(paths):
    """Highest GLIBC_x.y symbol version any payload file references."""
    highest = (0, 0)
    for path in paths:
        for major, minor in re.findall(rb'GLIBC_([0-9]+)\.([0-9]+)', Path(path).read_bytes()):
            highest = max(highest, (int(major), int(minor)))
    return highest


def dependency_plan(package_directory, architecture):
    """Reviewed system dependencies of the payload: {'glibc': (2, 35), 'sonames': [...], 'groups': {...}}."""
    package_directory = Path(package_directory)
    libraries = [path for path in sorted(package_directory.iterdir()) if path.name in EXECUTABLE_MEMBERS or path.name.startswith('lib')]
    bundled = {path.name for path in libraries if path.name.startswith('lib')}
    sonames = set()
    for path in libraries:
        for needed in elf_needed(path):
            if needed in bundled:
                continue
            require(needed in SYSTEM_LIBRARIES, 'unreviewed native dependency %s of %s' % (needed, path.name))
            sonames.add(needed)
    glibc = glibc_requirement(libraries)
    require(glibc <= MAXIMUM_GLIBC, 'payload needs glibc %d.%d, newer than the %d.%d build baseline' % (glibc + MAXIMUM_GLIBC))
    require(ARCHITECTURES[architecture]['interpreter'] in sonames or any(SYSTEM_LIBRARIES[name] == 'glibc' for name in sonames), 'payload does not depend on glibc')
    return {'glibc': glibc, 'sonames': sorted(sonames), 'groups': sorted({SYSTEM_LIBRARIES[name] for name in sonames})}


def deb_depends(plan):
    depends = ['libc6 (>= %d.%d)' % plan['glibc']]
    depends.extend(DEB_PACKAGES[group] for group in plan['groups'] if group in DEB_PACKAGES)
    return ', '.join(sorted(depends))


def rpm_requires(plan):
    requires = ['libc.so.6(GLIBC_%d.%d)(64bit)' % plan['glibc']]
    requires.extend('%s()(64bit)' % name for name in plan['sonames'] if SYSTEM_LIBRARIES[name] != 'glibc')
    return sorted(requires)


def render_template(text, values):
    for key, value in values.items():
        text = text.replace('@' + key + '@', value)
    require(re.search(r'@[A-Z_]+@', text) is None, 'unresolved packaging placeholder')
    return text


def desktop_entry():
    return render_template((PACKAGING / 'ilium.desktop').read_text(encoding='utf-8'), {'EXEC': 'ilium', 'ICON': APP_ID})


def metainfo(version):
    return render_template((PACKAGING / (APP_ID + '.metainfo.xml')).read_text(encoding='utf-8'), {'VERSION': version, 'DATE': EPOCH_DATE})


def write_file(path, content, mode=0o644):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content if isinstance(content, bytes) else content.encode('utf-8'))
    path.chmod(mode)
    os.utime(path, (EPOCH, EPOCH))


def copy_payload(package_directory, destination):
    destination = Path(destination)
    destination.mkdir(parents=True, exist_ok=True)
    for path in sorted(Path(package_directory).iterdir()):
        if path.name == 'THIRD-PARTY.txt':
            continue
        write_file(destination / path.name, path.read_bytes(), 0o755 if path.name in EXECUTABLE_MEMBERS else 0o644)


def link(path, target):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.symlink_to(target)


def install_tree(package_directory, destination, version, *, licence_path):
    """FHS layout shared by deb, rpm and the AppImage AppDir."""
    destination = Path(destination)
    package_directory = Path(package_directory)
    require(not destination.exists(), 'install tree must be new')
    copy_payload(package_directory, destination / LIBRARY_DIRECTORY)
    for name in EXECUTABLE_MEMBERS:
        link(destination / 'usr/bin' / name, '../lib/ilium/' + name)
    documents = destination / 'usr/share/doc/ilium'
    write_file(documents / 'THIRD-PARTY.txt', (package_directory / 'THIRD-PARTY.txt').read_bytes())
    licence = (ROOT / 'LICENSE').read_bytes()
    write_file(destination / licence_path, licence)
    write_file(destination / 'usr/share/applications' / (APP_ID + '.desktop'), desktop_entry())
    write_file(destination / 'usr/share/metainfo' / (APP_ID + '.metainfo.xml'), metainfo(version))
    write_file(destination / 'usr/share/icons/hicolor/256x256/apps' / (APP_ID + '.png'), (PACKAGING / 'ilium-256.png').read_bytes())
    write_file(destination / 'usr/share/icons/hicolor/scalable/apps' / (APP_ID + '.svg'), (ROOT / 'assets/ilium-mark.svg').read_bytes())
    return destination


def tree_entries(tree):
    """Deterministic (relative path, kind, mode, payload) rows of a tree, parents first."""
    tree = Path(tree)
    rows = []
    for path in sorted(tree.rglob('*'), key=lambda item: item.relative_to(tree).as_posix()):
        relative = path.relative_to(tree).as_posix()
        if path.is_symlink():
            rows.append((relative, 'link', 0o777, os.readlink(path)))
        elif path.is_dir():
            rows.append((relative, 'dir', 0o755, None))
        else:
            rows.append((relative, 'file', 0o755 if os.access(path, os.X_OK) else 0o644, path))
    return rows


def tar_bytes(rows, *, prefix='./'):
    """Normalised ustar stream (root:root, fixed mtime) of `rows`; the `./` root entry comes first."""
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode='w', format=tarfile.USTAR_FORMAT) as archive:
        def entry(name, kind):
            info = tarfile.TarInfo(name)
            info.uid = info.gid = 0
            info.uname = info.gname = 'root'
            info.mtime = EPOCH
            info.type = kind
            return info
        root = entry('./', tarfile.DIRTYPE)
        root.mode = 0o755
        archive.addfile(root)
        for relative, kind, mode, payload in rows:
            if kind == 'dir':
                info = entry(prefix + relative + '/', tarfile.DIRTYPE)
                info.mode = mode
                archive.addfile(info)
            elif kind == 'link':
                info = entry(prefix + relative, tarfile.SYMTYPE)
                info.mode = mode
                info.linkname = payload
                archive.addfile(info)
            else:
                data = payload if isinstance(payload, bytes) else Path(payload).read_bytes()
                info = entry(prefix + relative, tarfile.REGTYPE)
                info.mode = mode
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))
    return buffer.getvalue()


def xz(data):
    return lzma.compress(data, format=lzma.FORMAT_XZ, check=lzma.CHECK_CRC64, preset=6)


def ar_archive(members):
    output = bytearray(b'!<arch>\n')
    for name, data in members:
        header = '%-16s%-12d%-6d%-6d%-8s%-10d`\n' % (name + '/', EPOCH, 0, 0, '100644', len(data))
        output += header.encode('ascii') + data
        if len(data) % 2:
            output += b'\n'
    return bytes(output)


def build_deb(package_directory, architecture, version, work, destination):
    plan = dependency_plan(package_directory, architecture)
    tree = install_tree(package_directory, Path(work) / 'deb-tree', version, licence_path='usr/share/doc/ilium/copyright')
    rows = tree_entries(tree)
    size = sum(Path(payload).stat().st_size for _name, kind, _mode, payload in rows if kind == 'file')
    sums = ''.join('%s  %s\n' % (hashlib.md5(Path(payload).read_bytes()).hexdigest(), name) for name, kind, _mode, payload in rows if kind == 'file')
    control = (
        'Package: ilium\nVersion: %s\nArchitecture: %s\nMaintainer: %s\nInstalled-Size: %d\nDepends: %s\n'
        'Section: utils\nPriority: optional\nHomepage: %s\nDescription: %s\n %s\n'
    ) % (deb_version(version), ARCHITECTURES[architecture]['deb'], MAINTAINER, -(-size // 1024), deb_depends(plan), HOMEPAGE, SUMMARY, DESCRIPTION)
    control_rows = [('control', 'file', 0o644, control.encode('utf-8')), ('md5sums', 'file', 0o644, sums.encode('ascii'))]
    data = ar_archive([
        ('debian-binary', b'2.0\n'),
        ('control.tar.xz', xz(tar_bytes(control_rows))),
        ('data.tar.xz', xz(tar_bytes(rows))),
    ])
    Path(destination).write_bytes(data)
    return {'depends': deb_depends(plan)}


def render_spec(version, architecture, plan, rows):
    release = '1'
    rpm_version = version.replace('-', '~', 1)
    requires = ''.join('Requires: %s\n' % item for item in rpm_requires(plan))
    files = ''.join('%s\n' % name for name in rows)
    return f'''%global debug_package %{{nil}}
%global __os_install_post %{{nil}}
%global _build_id_links none
Name: ilium
Version: {rpm_version}
Release: {release}
Summary: {SUMMARY}
License: MIT
URL: {HOMEPAGE}
Packager: {MAINTAINER}
AutoReqProv: no
{requires}
%description
{DESCRIPTION}

%install
mkdir -p %{{buildroot}}
cp -a %{{_sourcedir}}/tree/. %{{buildroot}}/

%files
%defattr(-,root,root,-)
%license /usr/share/licenses/ilium/LICENSE
{files}
%changelog
* Sat Jan 01 2000 {MAINTAINER} - {rpm_version}-{release}
- Repackaged audited release bytes.
'''


def run(command, **options):
    emit('progress', command=[Path(str(command[0])).name, *[str(part) for part in command[1:3]]])
    result = subprocess.run([str(part) for part in command], capture_output=True, text=True, **options)
    if result.returncode != 0:
        raise release_tool.ReleaseError('%s exited %d: %s' % (Path(str(command[0])).name, result.returncode, (result.stdout + result.stderr)[-1200:]))
    return result


def find_tool(name):
    found = shutil.which(name)
    require(found is not None, 'required tool not found: ' + name)
    return found


def build_rpm(package_directory, architecture, version, work, destination):
    plan = dependency_plan(package_directory, architecture)
    work = Path(work)
    top = work / 'rpm'
    sources = top / 'SOURCES'
    sources.mkdir(parents=True)
    tree = install_tree(package_directory, sources / 'tree', version, licence_path='usr/share/licenses/ilium/LICENSE')
    listed = ['/usr/bin/ilium', '/usr/bin/ilium-server', '/usr/lib/ilium', '/usr/share/applications/%s.desktop' % APP_ID,
              '/usr/share/doc/ilium/THIRD-PARTY.txt', '/usr/share/icons/hicolor/256x256/apps/%s.png' % APP_ID,
              '/usr/share/icons/hicolor/scalable/apps/%s.svg' % APP_ID, '/usr/share/metainfo/%s.metainfo.xml' % APP_ID]
    for path in listed:
        require(os.path.lexists(tree / path.lstrip('/')), 'rpm file list names a missing path: ' + path)
    spec = top / 'ilium.spec'
    spec.write_text(render_spec(version, architecture, plan, listed), encoding='utf-8')
    environment = dict(os.environ, SOURCE_DATE_EPOCH=str(EPOCH), TZ='UTC', LC_ALL='C')
    run([find_tool('rpmbuild'), '-bb', '--target', ARCHITECTURES[architecture]['rpm'] + '-linux', '--define', '_topdir ' + str(top),
         '--define', '_rpmfilename %{NAME}.rpm', '--define', '_binary_payload w7.xzdio', '--define', '_buildhost reproducible',
         '--define', 'use_source_date_epoch_as_buildtime 1', '--define', 'clamp_mtime_to_source_date_epoch 1',
         '--define', '_source_filedigest_algorithm 8', '--define', '_binary_filedigest_algorithm 8', str(spec)], env=environment)
    built = top / 'RPMS' / 'ilium.rpm'
    require(built.is_file(), 'rpmbuild produced no package')
    shutil.copyfile(built, destination)
    return {'requires': rpm_requires(plan)}


def download(url, destination, expected_sha, expected_bytes):
    request = urllib.request.Request(url, headers={'User-Agent': 'ilium-release'})
    with urllib.request.urlopen(request, timeout=120) as response:
        data = response.read(expected_bytes + 1)
    require(len(data) == expected_bytes and hashlib.sha256(data).hexdigest() == expected_sha, 'downloaded runtime differs from its pinned identity: ' + url)
    Path(destination).write_bytes(data)


def appimage_runtime(architecture, work, runtime_file=None):
    tools = release_tool.read_json(PACKAGING / 'tools.json')['appimage_runtime']
    pinned = tools[architecture]
    if runtime_file is not None:
        runtime = Path(runtime_file)
        require(sha(runtime) == pinned['sha256'], 'supplied AppImage runtime differs from the pinned runtime')
        return runtime
    runtime = Path(work) / pinned['name']
    download(tools['source'] + pinned['name'], runtime, pinned['sha256'], pinned['bytes'])
    return runtime


def mksquashfs(source, destination, *, compression='zstd'):
    command = [find_tool('mksquashfs'), source, destination, '-noappend', '-quiet', '-root-owned', '-no-xattrs', '-all-time', str(EPOCH),
               '-mkfs-time', str(EPOCH), '-comp', compression, '-b', '256K']
    if compression == 'zstd':
        command += ['-Xcompression-level', '19']
    run(command)


def build_appimage(package_directory, architecture, version, work, destination, runtime_file=None):
    tree = install_tree(package_directory, Path(work) / 'AppDir', version, licence_path='usr/share/doc/ilium/copyright')
    ident = sha(Path(package_directory) / 'ilium')[:16]
    write_file(tree / 'AppRun', render_template(APPRUN, {'VERSION': version, 'IDENT': ident}), 0o755)
    write_file(tree / (APP_ID + '.desktop'), desktop_entry())
    write_file(tree / (APP_ID + '.png'), (PACKAGING / 'ilium-256.png').read_bytes())
    link(tree / '.DirIcon', APP_ID + '.png')
    runtime = appimage_runtime(architecture, work, runtime_file)
    image = Path(work) / 'appimage.squashfs'
    mksquashfs(tree, image)
    Path(destination).write_bytes(runtime.read_bytes() + image.read_bytes())
    Path(destination).chmod(0o755)
    return {'runtime_sha256': sha(runtime), 'payload_identity': ident}


def render_snap_yaml(version, architecture):
    base = release_tool.read_json(PACKAGING / 'tools.json')['snap']['base']
    return f'''name: ilium
version: '{snap_version(version)}'
summary: {SUMMARY}
description: |
  {DESCRIPTION}
license: MIT
base: {base}
confinement: classic
grade: stable
architectures:
  - {ARCHITECTURES[architecture]['snap']}
apps:
  ilium:
    command: lib/ilium/ilium
  server:
    command: lib/ilium/ilium-server
'''


def build_snap(package_directory, architecture, version, work, destination):
    root = Path(work) / 'snap-root'
    copy_payload(package_directory, root / 'lib/ilium')
    write_file(root / 'meta/snap.yaml', render_snap_yaml(version, architecture))
    write_file(root / 'share/doc/ilium/THIRD-PARTY.txt', (Path(package_directory) / 'THIRD-PARTY.txt').read_bytes())
    write_file(root / 'share/doc/ilium/LICENSE', (ROOT / 'LICENSE').read_bytes())
    output = Path(work) / 'snap-out'
    output.mkdir()
    environment = dict(os.environ, SOURCE_DATE_EPOCH=str(EPOCH))
    run([find_tool('snap'), 'pack', '--filename=ilium.snap', str(root), str(output)], env=environment)
    shutil.copyfile(output / 'ilium.snap', destination)
    return {}


def flatpak_metadata(architecture):
    tools = release_tool.read_json(PACKAGING / 'tools.json')['flatpak']
    arch = ARCHITECTURES[architecture]['flatpak']
    return f'''[Application]
name={APP_ID}
runtime={tools['runtime']}/{arch}/{tools['runtime_version']}
sdk={tools['runtime'].replace('Platform', 'Sdk')}/{arch}/{tools['runtime_version']}
command=ilium

[Context]
shared=network;
sockets=pulseaudio;
filesystems=host;
'''


def build_flatpak(package_directory, architecture, version, work, destination):
    tools = release_tool.read_json(PACKAGING / 'tools.json')['flatpak']
    application = Path(work) / 'flatpak-build'
    files = application / 'files'
    copy_payload(package_directory, files / 'lib/ilium')
    for name in EXECUTABLE_MEMBERS:
        link(files / 'bin' / name, '../lib/ilium/' + name)
    write_file(files / 'share/doc/ilium/THIRD-PARTY.txt', (Path(package_directory) / 'THIRD-PARTY.txt').read_bytes())
    write_file(files / 'share/doc/ilium/LICENSE', (ROOT / 'LICENSE').read_bytes())
    exported = application / 'export/share'
    write_file(exported / 'applications' / (APP_ID + '.desktop'), desktop_entry())
    write_file(exported / 'metainfo' / (APP_ID + '.metainfo.xml'), metainfo(version))
    write_file(exported / 'icons/hicolor/256x256/apps' / (APP_ID + '.png'), (PACKAGING / 'ilium-256.png').read_bytes())
    write_file(exported / 'icons/hicolor/scalable/apps' / (APP_ID + '.svg'), (ROOT / 'assets/ilium-mark.svg').read_bytes())
    write_file(application / 'metadata', flatpak_metadata(architecture))
    repository = Path(work) / 'flatpak-repo'
    flatpak = find_tool('flatpak')
    arch = ARCHITECTURES[architecture]['flatpak']
    run([flatpak, 'build-export', '--arch=' + arch, '--timestamp=' + EPOCH_DATE + 'T00:00:00Z', str(repository), str(application), FLATPAK_BRANCH])
    run([flatpak, 'build-bundle', '--arch=' + arch, '--runtime-repo=' + tools['runtime_repository'], str(repository), str(destination), APP_ID, FLATPAK_BRANCH])
    return {'runtime': '%s//%s' % (tools['runtime'], tools['runtime_version'])}


BUILDERS = {'deb': build_deb, 'rpm': build_rpm, 'appimage': build_appimage, 'flatpak': build_flatpak, 'snap': build_snap}


def tool_versions():
    versions = {}
    for name, flag in (('rpmbuild', '--version'), ('mksquashfs', '-version'), ('snap', 'version'), ('flatpak', '--version')):
        found = shutil.which(name)
        if found:
            result = subprocess.run([found, flag], capture_output=True, text=True)
            versions[name] = (result.stdout or result.stderr).strip().splitlines()[0][:120] if (result.stdout or result.stderr).strip() else 'unknown'
    return versions


def build(arguments):
    version = version_from_tag(arguments.tag)
    architecture = arguments.arch
    require(architecture in ARCHITECTURES, 'unsupported architecture')
    formats = tuple(arguments.formats.split(',')) if arguments.formats else FORMATS
    require(set(formats) <= set(FORMATS), 'unknown package format')
    work, output = arguments.work.resolve(), arguments.output.resolve()
    require(not output.exists(), 'package output must be new')
    work.mkdir(parents=True, exist_ok=True)
    package_directory = work / 'package'
    files = extract_package(arguments.archive, architecture, version, package_directory)
    output.mkdir(parents=True)
    packages, details = {}, {}
    for package_format in formats:
        name = package_name(architecture, package_format)
        emit('progress', stage='build', format=package_format, package=name)
        builder_work = work / package_format
        builder_work.mkdir()
        extra = {'runtime_file': arguments.appimage_runtime} if package_format == 'appimage' else {}
        details[package_format] = BUILDERS[package_format](package_directory, architecture, version, builder_work, output / name, **extra)
        require((output / name).is_file() and (output / name).stat().st_size > 0, 'package is empty: ' + name)
        packages[name] = sha(output / name)
        emit('artifact', format=package_format, path=str(output / name), sha256=packages[name], bytes=(output / name).stat().st_size)
    receipt = {'schema': 1, 'tag': arguments.tag, 'version': version, 'arch': architecture, 'source_archive': arguments.archive.name,
               'source_archive_sha256': sha(arguments.archive), 'package_files': files, 'packages': packages, 'details': details, 'tools': tool_versions()}
    (output / receipt_name(architecture)).write_text(json.dumps(receipt, indent=2, sort_keys=True) + '\n', encoding='ascii')
    emit('result', command='build', state='built', output=str(output), receipt=str(output / receipt_name(architecture)), packages=packages)


def parser():
    result = release_tool.JsonArgumentParser(description=__doc__, allow_abbrev=False)
    commands = result.add_subparsers(dest='command', required=True)
    command = commands.add_parser('build', allow_abbrev=False)
    command.add_argument('--tag', required=True)
    command.add_argument('--arch', required=True, choices=sorted(ARCHITECTURES))
    command.add_argument('--archive', type=Path, required=True)
    command.add_argument('--work', type=Path, required=True)
    command.add_argument('--output', type=Path, required=True)
    command.add_argument('--formats', help='comma separated subset of ' + ','.join(FORMATS))
    command.add_argument('--appimage-runtime', type=Path, help='pre-fetched runtime file; its hash must equal the pinned runtime')
    return result


def main(argv=None):
    try:
        arguments = parser().parse_args(argv)
        {'build': build}[arguments.command](arguments)
        return 0
    except (ValueError, OSError, KeyError, tarfile.TarError, subprocess.SubprocessError, struct.error) as error:
        emit('error', message=str(error)[:1200])
        return 1


if __name__ == '__main__':
    sys.exit(main())
