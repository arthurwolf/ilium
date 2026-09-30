"""Offline contracts for the Linux deb, rpm, AppImage, Flatpak and Snap packaging."""
import gzip
import hashlib
import io
import json
from pathlib import Path
import shutil
import struct
import sys
import tarfile
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import build_linux_packages as packages
import release_pipeline
import release_tool
import smoke_linux_packages as smoke

VERSION = '0.1.0'


def elf_with_needed(needed, glibc='GLIBC_2.35'):
    """Smallest ELF64 that carries a PT_DYNAMIC with DT_NEEDED entries and a glibc version string."""
    strings = b'\0' + b''.join(name.encode() + b'\0' for name in needed) + glibc.encode() + b'\0'
    offsets, position = [], 1
    for name in needed:
        offsets.append(position)
        position += len(name) + 1
    header_size, program_size = 64, 56
    dynamic_offset = header_size + 2 * program_size
    string_offset = dynamic_offset + 16 * (len(needed) + 2)
    dynamic = b''.join(struct.pack('<qQ', 1, offset) for offset in offsets) + struct.pack('<qQ', 5, string_offset) + struct.pack('<qQ', 0, 0)
    body = dynamic + strings
    total = dynamic_offset + len(body)
    identity = b'\x7fELF' + bytes([2, 1, 1, 0]) + bytes(8)
    header = identity + struct.pack('<HHIQQQIHHHHHH', 3, 62, 1, 0, header_size, 0, 0, header_size, program_size, 2, 0, 0, 0)
    load = struct.pack('<IIQQQQQQ', 1, 4, 0, 0, 0, total, total, 0x1000)
    dyn = struct.pack('<IIQQQQQQ', 2, 4, dynamic_offset, dynamic_offset, dynamic_offset, len(dynamic), len(dynamic), 8)
    return header + load + dyn + body


def make_archive(directory, architecture='x86_64', members=None, version=VERSION):
    client = elf_with_needed(['libonnxruntime.so.1', 'libasound.so.2', 'libssl.so.3', 'libcrypto.so.3', 'libc.so.6'])
    server = elf_with_needed(['libgcc_s.so.1', 'libm.so.6', 'libc.so.6'])
    runtime = elf_with_needed(['libstdc++.so.6', 'libc.so.6'])
    members = members or {'ilium': client, 'ilium-server': server, 'libonnxruntime.so.1': runtime,
                          'VERSION': (version + '\n').encode(), 'THIRD-PARTY.txt': b'notices'}
    prefix = 'ilium-linux-' + architecture
    archive = Path(directory) / (prefix + '.tar.gz')
    with tarfile.open(archive, 'w:gz') as package:
        directory_info = tarfile.TarInfo(prefix)
        directory_info.type = tarfile.DIRTYPE
        package.addfile(directory_info)
        for name, content in members.items():
            info = tarfile.TarInfo(prefix + '/' + name)
            info.size = len(content)
            package.addfile(info, io.BytesIO(content))
    return archive


class PayloadTests(unittest.TestCase):
    def test_version_contract_and_ecosystem_spellings(self):
        self.assertEqual(packages.version_from_tag('v1.2.3'), '1.2.3')
        self.assertEqual(packages.version_from_tag('v1.2.3-rc.1'), '1.2.3-rc.1')
        self.assertEqual(packages.deb_version('1.2.3-rc.1'), '1.2.3~rc.1')
        for bad in ('1.2.3', 'v1.2', 'v1.2.3-', 'v1.2.3+build', 'vx'):
            with self.assertRaises(release_tool.ReleaseError):
                packages.version_from_tag(bad)

    def test_asset_names_are_stable_per_architecture(self):
        self.assertEqual(packages.package_names('x86_64'), ('ilium-linux-x86_64.deb', 'ilium-linux-x86_64.rpm', 'ilium-linux-x86_64.AppImage', 'ilium-linux-x86_64.flatpak', 'ilium-linux-x86_64.snap'))
        self.assertEqual(packages.receipt_name('aarch64'), 'linux-packages-aarch64.json')

    def test_extract_hashes_the_exact_members(self):
        with tempfile.TemporaryDirectory() as temporary:
            archive = make_archive(temporary)
            files = packages.extract_package(archive, 'x86_64', VERSION, Path(temporary) / 'package')
            self.assertEqual(set(files), {'ilium', 'ilium-server', 'libonnxruntime.so.1', 'VERSION', 'THIRD-PARTY.txt'})
            self.assertEqual(files['THIRD-PARTY.txt'], hashlib.sha256(b'notices').hexdigest())

    def test_extract_rejects_unexpected_members_and_version_drift(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = {'ilium': b'a', 'ilium-server': b'b', 'VERSION': b'0.1.0\n', 'THIRD-PARTY.txt': b'n'}
            for index, extra in enumerate(('../escape', 'nested/file', 'libevil.txt', 'install.sh')):
                archive = make_archive(temporary, members={**base, extra: b'x'})
                with self.assertRaises(release_tool.ReleaseError):
                    packages.extract_package(archive, 'x86_64', VERSION, Path(temporary) / ('case%d' % index))
            archive = make_archive(temporary, members={**base, 'VERSION': b'9.9.9\n'})
            with self.assertRaises(release_tool.ReleaseError):
                packages.extract_package(archive, 'x86_64', VERSION, Path(temporary) / 'version')

    def test_extract_rejects_links(self):
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary) / 'ilium-linux-x86_64.tar.gz'
            with tarfile.open(archive, 'w:gz') as package:
                info = tarfile.TarInfo('ilium-linux-x86_64/ilium')
                info.type, info.linkname = tarfile.SYMTYPE, '/etc/passwd'
                package.addfile(info)
            with self.assertRaises(release_tool.ReleaseError):
                packages.extract_package(archive, 'x86_64', VERSION, Path(temporary) / 'out')


class DependencyTests(unittest.TestCase):
    def package_directory(self, temporary, **overrides):
        archive = make_archive(temporary)
        directory = Path(temporary) / 'package'
        packages.extract_package(archive, 'x86_64', VERSION, directory)
        for name, content in overrides.items():
            (directory / name).write_bytes(content)
        return directory

    def test_elf_needed_reads_link_order(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'elf'
            path.write_bytes(elf_with_needed(['liba.so.1', 'libb.so.2']))
            self.assertEqual(packages.elf_needed(path), ['liba.so.1', 'libb.so.2'])

    def test_bundled_runtime_is_not_a_system_dependency(self):
        with tempfile.TemporaryDirectory() as temporary:
            plan = packages.dependency_plan(self.package_directory(temporary), 'x86_64')
            self.assertNotIn('libonnxruntime.so.1', plan['sonames'])
            self.assertEqual(packages.deb_depends(plan), 'libasound2t64 | libasound2, libc6 (>= 2.35), libgcc-s1, libssl3t64 | libssl3, libstdc++6')
            self.assertEqual(packages.rpm_requires(plan), ['libasound.so.2()(64bit)', 'libc.so.6(GLIBC_2.35)(64bit)', 'libcrypto.so.3()(64bit)',
                                                          'libgcc_s.so.1()(64bit)', 'libssl.so.3()(64bit)', 'libstdc++.so.6()(64bit)'])

    def test_unreviewed_dependency_is_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = self.package_directory(temporary, ilium=elf_with_needed(['libsurprise.so.9', 'libc.so.6']))
            with self.assertRaises(release_tool.ReleaseError):
                packages.dependency_plan(directory, 'x86_64')

    def test_glibc_newer_than_the_build_baseline_is_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = self.package_directory(temporary, ilium=elf_with_needed(['libc.so.6'], glibc='GLIBC_2.38'))
            with self.assertRaises(release_tool.ReleaseError):
                packages.dependency_plan(directory, 'x86_64')


class BuildTests(unittest.TestCase):
    def built_deb(self, temporary, name):
        archive = make_archive(temporary)
        directory = Path(temporary) / ('package-' + name)
        files = packages.extract_package(archive, 'x86_64', VERSION, directory)
        destination = Path(temporary) / (name + '.deb')
        packages.build_deb(directory, 'x86_64', VERSION, Path(temporary) / ('work-' + name), destination)
        return destination, files

    def test_deb_payload_equals_the_audited_files_and_rebuilds_identically(self):
        with tempfile.TemporaryDirectory() as temporary:
            first, files = self.built_deb(temporary, 'first')
            second, _files = self.built_deb(temporary, 'second')
            self.assertEqual(first.read_bytes(), second.read_bytes())
            receipt = {'package_files': files}
            self.assertEqual(smoke.inspect_package('deb', first, receipt, 'x86_64'), {'files': 5})

    def test_deb_inspection_detects_tampered_payload(self):
        with tempfile.TemporaryDirectory() as temporary:
            deb, files = self.built_deb(temporary, 'tampered')
            files['ilium'] = '0' * 64
            with self.assertRaises(release_tool.ReleaseError):
                smoke.inspect_package('deb', deb, {'package_files': files}, 'x86_64')

    def test_deb_metadata_symlinks_and_ownership(self):
        with tempfile.TemporaryDirectory() as temporary:
            deb, _files = self.built_deb(temporary, 'metadata')
            data = deb.read_bytes()
            self.assertTrue(data.startswith(b'!<arch>\ndebian-binary'))
            tree = Path(temporary) / 'tree'
            smoke.unpack('deb', deb, tree)
            self.assertEqual((tree / 'usr/bin/ilium').readlink().as_posix(), '../lib/ilium/ilium')
            with tarfile.open(fileobj=io.BytesIO(__import__('lzma').decompress(self.member(data, 'data.tar.xz')))) as archive:
                for member in archive:
                    self.assertEqual((member.uid, member.gid, member.uname, member.gname, member.mtime), (0, 0, 'root', 'root', packages.EPOCH))
            with tarfile.open(fileobj=io.BytesIO(__import__('lzma').decompress(self.member(data, 'control.tar.xz')))) as archive:
                control = archive.extractfile('./control').read().decode()
            self.assertIn('Depends: libasound2t64 | libasound2, libc6 (>= 2.35)', control)
            self.assertIn('Architecture: amd64', control)

    @staticmethod
    def member(data, name):
        position = 8
        while position < len(data):
            header = data[position:position + 60]
            size = int(header[48:58])
            if header[:16].decode().strip().rstrip('/') == name:
                return data[position + 60:position + 60 + size]
            position += 60 + size + size % 2
        raise AssertionError(name)


class RenderTests(unittest.TestCase):
    def plan(self):
        return {'glibc': (2, 35), 'sonames': ['libasound.so.2', 'libc.so.6'], 'groups': ['alsa', 'glibc']}

    def test_rpm_spec_freezes_the_audited_bytes(self):
        spec = packages.render_spec('0.1.0-rc.1', 'x86_64', self.plan(), ['/usr/bin/ilium'])
        self.assertIn('Version: 0.1.0~rc.1', spec)
        self.assertIn('Requires: libasound.so.2()(64bit)', spec)
        self.assertIn('Requires: libc.so.6(GLIBC_2.35)(64bit)', spec)
        for directive in ('%global debug_package %{nil}', '%global __os_install_post %{nil}', 'AutoReqProv: no'):
            self.assertIn(directive, spec)
        self.assertNotIn('@', spec.replace(packages.MAINTAINER, ''))

    def test_snap_is_classic_and_architecture_specific(self):
        document = yaml.safe_load(packages.render_snap_yaml('0.1.0', 'aarch64'))
        self.assertEqual(document['confinement'], 'classic')
        self.assertEqual(document['architectures'], ['arm64'])
        self.assertEqual(document['apps']['ilium']['command'], 'lib/ilium/ilium')
        self.assertEqual(document['version'], '0.1.0')
        with self.assertRaises(release_tool.ReleaseError):
            packages.snap_version('1' * 33)

    def test_flatpak_metadata_pins_the_runtime(self):
        metadata = packages.flatpak_metadata('aarch64')
        self.assertIn('runtime=org.freedesktop.Platform/aarch64/24.08', metadata)
        self.assertIn('command=ilium', metadata)

    def test_desktop_entry_and_metainfo_are_complete(self):
        entry = packages.desktop_entry()
        self.assertIn('Exec=ilium', entry)
        self.assertIn('Icon=' + packages.APP_ID, entry)
        self.assertIn('Terminal=true', entry)
        self.assertIn('<release version="0.1.0"', packages.metainfo('0.1.0'))

    def test_apprun_keeps_the_server_outside_the_fuse_mount(self):
        script = packages.render_template(packages.APPRUN, {'VERSION': '0.1.0', 'IDENT': 'abcdef0123456789'})
        self.assertIn('target=$store/0.1.0-abcdef0123456789', script)
        self.assertIn('exec "$target/ilium" "$@"', script)
        self.assertNotIn('@VERSION@', script)

    def test_runtime_pins_are_complete_hashes(self):
        tools = release_tool.read_json(packages.PACKAGING / 'tools.json')
        for architecture in packages.ARCHITECTURES:
            pinned = tools['appimage_runtime'][architecture]
            self.assertRegex(pinned['sha256'], '^[0-9a-f]{64}$')
            self.assertGreater(pinned['bytes'], 100_000)
        self.assertTrue(tools['appimage_runtime']['source'].startswith('https://github.com/AppImage/type2-runtime/releases/download/2'))

    @unittest.skipUnless(shutil.which('mksquashfs') and shutil.which('unsquashfs'), 'squashfs-tools are required')
    def test_appimage_payload_round_trips_and_rebuilds_identically(self):
        with tempfile.TemporaryDirectory() as temporary:
            archive = make_archive(temporary)
            directory = Path(temporary) / 'package'
            files = packages.extract_package(archive, 'x86_64', VERSION, directory)
            runtime = Path(temporary) / 'runtime'
            fake_runtime = bytearray(elf_with_needed([]))
            struct.pack_into('<Q', fake_runtime, 0x28, len(fake_runtime))
            struct.pack_into('<HH', fake_runtime, 0x3A, 64, 0)
            runtime.write_bytes(bytes(fake_runtime))
            tools = release_tool.read_json(packages.PACKAGING / 'tools.json')['appimage_runtime']['x86_64']
            original = dict(tools)
            tools['sha256'] = hashlib.sha256(runtime.read_bytes()).hexdigest()
            real_reader = release_tool.read_json
            try:
                release_tool.read_json = lambda path: {**real_reader(path), 'appimage_runtime': {'x86_64': tools, 'source': 'unused'}} if Path(path).name == 'tools.json' else real_reader(path)
                first, second = Path(temporary) / 'first.AppImage', Path(temporary) / 'second.AppImage'
                packages.build_appimage(directory, 'x86_64', VERSION, Path(temporary) / 'w1', first, runtime)
                packages.build_appimage(directory, 'x86_64', VERSION, Path(temporary) / 'w2', second, runtime)
            finally:
                release_tool.read_json = real_reader
                tools.update(original)
            self.assertEqual(first.read_bytes(), second.read_bytes())
            self.assertEqual(smoke.inspect_package('appimage', first, {'package_files': files}, 'x86_64'), {'files': 5})

    @unittest.skipUnless(shutil.which('mksquashfs') and shutil.which('unsquashfs'), 'squashfs-tools are required')
    def test_snap_layout_round_trips_through_squashfs(self):
        with tempfile.TemporaryDirectory() as temporary:
            archive = make_archive(temporary)
            directory = Path(temporary) / 'package'
            files = packages.extract_package(archive, 'x86_64', VERSION, directory)
            root = Path(temporary) / 'snap-root'
            packages.copy_payload(directory, root / 'lib/ilium')
            (root / 'share/doc/ilium').mkdir(parents=True)
            (root / 'share/doc/ilium/THIRD-PARTY.txt').write_bytes((directory / 'THIRD-PARTY.txt').read_bytes())
            image = Path(temporary) / 'ilium.snap'
            packages.mksquashfs(root, image)
            self.assertEqual(smoke.inspect_package('snap', image, {'package_files': files}, 'x86_64'), {'files': 5})


class PipelineContractTests(unittest.TestCase):
    def test_smoke_hash_file_maps_notices_outside_the_library_directory(self):
        receipt = {'package_files': {'ilium': 'a' * 64, 'THIRD-PARTY.txt': 'b' * 64}}
        self.assertEqual(smoke.expected_hashes(receipt), 'b' * 64 + '  /usr/share/doc/ilium/THIRD-PARTY.txt\n' + 'a' * 64 + '  /usr/lib/ilium/ilium\n')

    def test_squashfs_offset_is_the_end_of_the_elf_section_table(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'runtime'
            header = bytearray(elf_with_needed([])[:64])
            struct.pack_into('<Q', header, 0x28, 1000)
            struct.pack_into('<HH', header, 0x3A, 64, 10)
            path.write_bytes(bytes(header))
            self.assertEqual(smoke.squashfs_offset(path), 1000 + 64 * 10)


if __name__ == '__main__':
    unittest.main()
