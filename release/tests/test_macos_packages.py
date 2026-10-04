"""Synthetic portable guards and explicit opt-in real native package acceptance."""  # Separate fixtures from native proof.
from __future__ import annotations  # Keep annotations portable.
import copy  # Mutate fixtures.
import contextlib  # Capture CLI output.
import ctypes  # Inspect native ABI mocks.
import errno  # Model native errors.
import gzip  # Construct transport fixtures.
import hashlib  # Hash fixtures independently.
import io  # Supply tar streams.
import json  # Encode receipt fixtures.
import os  # Inspect host capabilities.
from pathlib import Path  # Use native paths.
import platform  # Gate native acceptance.
import plistlib  # Encode native inventories.
import shutil  # Copy exact source bytes.
import stat  # Inspect file modes.
import struct  # Construct inert Mach-O headers.
import sys  # Import adjacent scripts.
import tarfile  # Encode malformed archives.
import tempfile  # Own temporary fixtures.
import tomllib  # Read source versions.
from types import SimpleNamespace  # Supply CLI namespaces.
import unittest  # Integrate unittest discovery.
from unittest import mock  # Mock native boundaries.
from unittest.mock import patch  # Isolate OS observations.
import xml.etree.ElementTree as xml_tree  # Inspect native XML.
import zipfile  # Inspect ZIP metadata.
import zlib  # Encode zlib fixtures.
root = Path(__file__).resolve().parents[2]  # Locate proposed source root.
sys.path.insert(0, str(root / 'release/scripts'))  # Import proposed scripts.
import build_macos_packages as packages  # Exercise corrected M2 builder.
import release_tool  # Use original archive contract.
from release_tool import ReleaseError as release_error  # Name policy failures.
import smoke_macos_packages as smoke  # Exercise M3 guards.
#
def digest(path):  # Hash fixtures independently.
    return hashlib.sha256(path.read_bytes()).hexdigest()  # Hash small owned file.
#
def json_file(path, document):  # Write JSON fixtures.
    path.write_text(json.dumps(document, sort_keys=True, allow_nan=False) + '\n', encoding='utf-8')  # Preserve JSON/LF.
#
def fixture_macho(arch, executable, label=b'payload'):  # Construct nonrunnable syntax fixtures.
    header = struct.pack('<IiiIIIII', 0xFEEDFACF, packages.architectures[arch][1], 0, 2 if executable else 6, 1, 24, 0, 0)  # Declare bounded thin header.
    return header + struct.pack('<II', 0x1B, 24) + hashlib.sha256(label).digest()[:16]  # Encode distinct LC_UUID bytes.
#
class native_fixture:  # Model synthetic custody.
    def __init__(self, base, arch):  # Build all receipt dependencies.
        self.base, self.arch = base, arch  # Retain fixture identity.
        self.source, self.native = base / 'source', base / 'native'  # Separate source/native roots.
        self.source.mkdir()  # Create synthetic source root.
        self.native.mkdir()  # Create native evidence root.
        self.workspace, self.manifest = self.source / 'Cargo.toml', self.source / 'release/targets.toml'  # Name exact policy inputs.
        self.version, self.tag, self.commit = '0.1.0', 'v0.1.0', 'a' * 40  # Set consistent synthetic identity.
        for name in packages.source_names:  # Bind M2 source input.
            output = self.source / name  # Preserve source-relative paths.
            output.parent.mkdir(parents=True, exist_ok=True)  # Create required directories.
            original = root / name  # Locate proposed source.
            if name == 'Cargo.toml':  # Define minimal workspace version.
                output.write_text('[workspace.package]\nversion = "0.1.0"\n', encoding='utf-8')  # Write valid source metadata.
            elif name == 'Cargo.lock':  # Provide distinct lock bytes.
                output.write_text('# Synthetic test lock; no build or licence proof.\n', encoding='utf-8')  # Label synthetic build evidence.
            elif name == 'release/targets.toml' or name.startswith('release/scripts/') or name.startswith('ilium-animation-js/assets/packages/'):  # Use exact current policy, helpers and binary packages.
                shutil.copyfile(original, output)  # Copy without executing source.
            else:  # Supply inert custody inputs.
                output.write_text('# Synthetic acceptance wrapper; never invoked.\n' if name.endswith('.py') else '{}\n', encoding='utf-8')  # Keep wrapper inert/JSON valid.
        self.target = release_tool.selected_target(self.manifest, arch + '-apple-darwin')  # Use unchanged target manifest.
        self.archive = self.native / self.target['archive']  # Preserve native archive name.
        self.runtime_name = 'libonnxruntime.1.24.2.dylib'  # Use versioned ORT name.
        self.payload = {'ilium': fixture_macho(arch, True, b'client'), 'ilium-server': fixture_macho(arch, True, b'server'), 'ilium-animation-helper': fixture_macho(arch, True, b'helper'), **{name: (root / 'ilium-animation-js/assets/packages' / name).read_bytes() for name in release_tool.APPROVED_PACKAGES}, self.runtime_name: fixture_macho(arch, False, b'packaged-runtime'), 'VERSION': b'0.1.0\n', 'THIRD-PARTY.txt': b'Reviewed synthetic licence fixture; no redistribution proof.\n'}  # Define exact synthetic payload.
        self.files = {name: hashlib.sha256(data).hexdigest() for name, data in self.payload.items()}  # Hash post-relocation payload.
        candidate = self.native / 'candidate'  # Retain separate candidate tree.
        candidate.mkdir()  # Create flat candidate root.
        for name, data in self.payload.items():  # Write admitted member.
            (candidate / name).write_bytes(data)  # Preserve distinct payload bytes.
        self.identity = {'system': 'Darwin', 'machine': packages.architectures[arch][0], 'release': '24.0.0', 'version': 'Darwin Kernel Version synthetic fixture', 'runner': self.target['runner']}  # Model synthetic runner identity.
        self.runtime = {'schema': 1, 'state': 'reviewed', 'publication_allowed': True, 'target': self.target['rust_target'], 'files': [{'name': self.runtime_name, 'sha256': hashlib.sha256(b'original-runtime-before-relocation').hexdigest(), 'version': '1.24.2', 'reviewed': True}], 'system_libraries': [{'name': '/usr/lib/libSystem.B.dylib', 'reviewed': True, 'source': 'synthetic fixture'}]}  # Preserve distinct pre-relocation hash.
        self.dependencies = {'schema': 1, 'state': 'reviewed', 'lock_sha256': digest(self.source / 'Cargo.lock'), 'packages': []}  # Bind synthetic licence metadata.
        graph = {'ilium': ['@executable_path/' + self.runtime_name, '/usr/lib/libSystem.B.dylib'], 'ilium-server': ['/usr/lib/libSystem.B.dylib'], 'ilium-animation-helper': ['/usr/lib/libSystem.B.dylib'], self.runtime_name: ['/usr/lib/libSystem.B.dylib']}  # Model loader closure.
        self.audit = {'schema': 1, 'state': 'passed', 'publication_allowed': True, 'target': self.target['rust_target'], 'os': 'macos', 'arch': arch, 'tag': self.tag, 'version': self.version, 'native_identity': dict(self.identity), 'dependency_closure': {'complete': True, 'graph': graph, 'bundled': [self.runtime_name]}, 'files': dict(self.files), 'binary_versions': {'ilium': 'ilium 0.1.0', 'ilium-server': 'ilium-server 0.1.0', 'ilium-animation-helper': 'ilium-animation-helper 0.1.0'}, 'notices': {'state': 'reviewed', 'sha256': self.files['THIRD-PARTY.txt']}, 'loader_paths': {'state': 'passed'}, 'embedding': {'state': 'passed'}, 'signing': {'state': 'unsigned', 'credentials_present': False, 'identity': None, 'nested_code': {name: {'verified': True, 'distribution_signed': False} for name in graph}}, 'notarization': {'state': 'disabled', 'credentials_present': False}, 'intel_ort': {'state': 'passed', 'source_tag': 'v1.24.2', 'source_commit': '058787ceead760166e3c50a0a4cba8a833a6f53f', 'source_sha256': 'b' * 64}}  # Model audit schema; no native proof.
        evidence = self.native / 'evidence'  # Create recursive evidence root.
        (evidence / 'harness').mkdir(parents=True)  # Create portable harness directory.
        (evidence / 'model').mkdir()  # Keep models outside product.
        (evidence / 'ORT-LICENSE-AND-NOTICES.txt').write_bytes(b'Synthetic reviewed ORT notice fixture.\n')  # Write synthetic ORT notices.
        harness_bytes = fixture_macho(arch, True, b'portable-harness')  # Construct inert harness bytes.
        (self.native / 'native-test-binary').write_bytes(harness_bytes)  # Retain root harness copy.
        (evidence / 'harness/native-test-binary').write_bytes(harness_bytes)  # Retain portable harness copy.
        (evidence / 'harness' / self.runtime_name).write_bytes(fixture_macho(arch, False, b'harness-runtime'))  # Preserve separately sealed harness hash.
        (evidence / 'model/model.onnx').write_bytes(b'Synthetic model bytes; never run inference.')  # Write nonexecuted model fixture.
        model_files = {'model.onnx': digest(evidence / 'model/model.onnx')}  # Bind model member.
        json_file(self.source / 'release/embedding-model.json', {'reviewed': True, 'files': model_files})  # Bind source model register.
        self.spec = {'schema': 1, 'state': 'reviewed', 'protocol': 'held-installed-process-v1', 'sha256': digest(self.source / 'release/tests/embedding_acceptance.py'), 'command': [str(self.source / 'release/tests/embedding_acceptance.py'), '--model-lock', str(self.source / 'release/embedding-model.json')]}  # Bind inert wrapper/protocol.
        json_file(self.native / 'embedding-command.json', self.spec)  # Retain command specification.
        json_file(self.native / 'embedding-receipt.json', {'type': 'embedding-proof', 'synthetic_fixture': True})  # Label synthetic inference evidence.
        self.harness = {'schema': 1, 'target': self.target['rust_target'], 'tag': self.tag, 'version': self.version, 'source_commit': self.commit, 'filename': 'native-test-binary', 'path': 'evidence/harness/native-test-binary', 'sha256': digest(self.native / 'native-test-binary'), 'runtime_files': {self.runtime_name: digest(evidence / 'harness' / self.runtime_name)}, 'evidence_files': {'evidence/' + path.relative_to(evidence).as_posix(): digest(path) for path in evidence.rglob('*') if path.is_file()}}  # Bind exact recursive harness/model files.
        self.bridge = {'schema': 1, 'state': 'passed', 'publication_allowed': True, 'target': self.target['rust_target'], 'tag': self.tag, 'native_identity': dict(self.identity), 'workspace_sha256': digest(self.workspace), 'lock_sha256': digest(self.source / 'Cargo.lock'), 'embedding_model_register_sha256': digest(self.source / 'release/embedding-model.json'), 'files': dict(self.files), 'official_packages': dict(release_tool.APPROVED_PACKAGES), 'build_outputs': {name: {'sha256': self.files[name]} for name in self.target['executables']}}  # Bind source/native bridge fields.
        release_tool.write_archive(self.archive, self.target, self.payload)  # Write canonical tar.
        self.refresh()  # Complete outer hash chain.
    #
    def refresh(self):  # Rebind outer hashes only.
        json_file(self.native / 'runtime-inventory.json', self.runtime)  # Write runtime evidence.
        json_file(self.native / 'dependency-inventory.json', self.dependencies)  # Write dependency evidence.
        self.audit['runtime_inventory_sha256'] = digest(self.native / 'runtime-inventory.json')  # Bind exact inventory bytes.
        self.audit['dependency_inventory_sha256'] = digest(self.native / 'dependency-inventory.json')  # Bind exact inventory bytes.
        json_file(self.native / 'native-audit.json', self.audit)  # Write audit claims.
        json_file(self.native / 'native-test-harness.json', self.harness)  # Keep inner expected hashes.
        for key, name in (('archive', self.target['archive']), ('native_audit', 'native-audit.json'), ('runtime_inventory', 'runtime-inventory.json'), ('dependency_inventory', 'dependency-inventory.json'), ('embedding_receipt', 'embedding-receipt.json')):  # Bind bridge edge.
            self.bridge[key] = {'path': str(self.native / name), 'sha256': digest(self.native / name)}  # Preserve intentionally invalid inner claims.
        json_file(self.native / 'native-candidate-receipt.json', self.bridge)  # Write native bridge.
        (self.native / 'SHA256SUMS').write_bytes((digest(self.archive) + '  ' + self.archive.name + '\n').encode('ascii'))  # Preserve exact LF bytes on every source runner.
    #
    def load(self):  # Invoke custody validator.
        return packages.load_native(self.native, self.archive, self.manifest, self.workspace, self.tag, self.arch, self.commit)  # Pass exact fixture identity.
    #
    def write_tar(self, members, change=None, archive_format=tarfile.USTAR_FORMAT):  # Encode controlled malformed tar.
        with self.archive.open('wb') as raw, gzip.GzipFile(filename='', fileobj=raw, mode='wb', mtime=0) as compressed, tarfile.open(fileobj=compressed, mode='w', format=archive_format) as archive:  # Use canonical gzip framing.
            for index, (name, data) in enumerate(members):  # Preserve malicious names literally.
                member = tarfile.TarInfo(name)  # Start normalized tar header.
                member.type, member.size = (tarfile.DIRTYPE, 0) if data is None else (tarfile.REGTYPE, len(data))  # Declare type/size.
                member.mode = 0o755 if data is None or name.rsplit('/', 1)[-1] in self.target['executables'] else 0o644  # Set canonical payload modes.
                if change is not None:  # Apply explicit test mutation.
                    change(index, member)  # Corrupt chosen header field.
                archive.addfile(member, io.BytesIO(data) if data is not None else None)  # Serialize header and content.
    #
    def members(self):  # Provide canonical member list.
        prefix = release_tool.archive_prefix(self.target)  # Use authoritative archive prefix.
        return [(prefix, None)] + [(prefix + '/' + name, data) for name, data in sorted(self.payload.items())]  # Include exact ordered inventory.
#
class macos_builder_tests(unittest.TestCase):  # Exercise real portable guards.
    def f(self, arch='aarch64'):  # Create isolated custody fixtures.
        temp = tempfile.TemporaryDirectory()  # Own temporary storage.
        self.addCleanup(temp.cleanup)  # Remove owned fixtures.
        return native_fixture(Path(temp.name), arch)  # Supply evidence.
    #
    def reject_archive(self, f):  # Check rejection before writes.
        output = f.base / 'extracted'  # Select absent destination.
        self.assertRaises(release_error, packages.extract_package, f.archive, f.target, f.audit, f.version, output)  # Require policy error.
        self.assertFalse(output.exists())  # Forbid partial materialization.
    #
    def test_canonical_both_architectures(self):  # Establish genuine parser success.
        for arch in packages.architectures:  # Cover both native CPUs.
            with self.subTest(arch=arch):  # Identify target failures.
                f = self.f(arch)  # Build synthetic custody chain.
                target, audit, runtime, binding = f.load()  # Validate native evidence.
                self.assertEqual(len({runtime['files'][0]['sha256'], audit['files'][f.runtime_name], f.harness['runtime_files'][f.runtime_name]}), 3)  # Preserve three sealing stages.
                self.assertEqual(binding['source_commit'], f.commit)  # Bind harness source identity.
                output = f.base / 'extracted'  # Use absent destination.
                actual = packages.extract_package(f.archive, target, audit, f.version, output)  # Extract tar bytes.
                self.assertEqual(actual, f.payload)  # Compare payload byte.
                self.assertEqual({name: (output / name).read_bytes() for name in actual}, actual)  # Verify materialized content.
                modes = {name: 0o755 if name in target['executables'] else 0o644 for name in actual}  # Define canonical permissions.
                if os.name != 'nt':  # Windows lacks POSIX execute bits.
                    self.assertEqual({name: stat.S_IMODE((output / name).stat().st_mode) for name in actual}, modes)  # Check filesystem modes.
                policy = dict(target, archive=packages.package_name(arch, 'zip'), format='zip')  # Derive additional ZIP policy.
                path = f.base / policy['archive']  # Preserve stable asset name.
                release_tool.write_archive(path, policy, actual)  # Write ZIP bytes.
                recovered = release_tool.read_archive(path, policy, audit)  # Parse finished ZIP strictly.
                release_tool.verify_content(recovered, audit, f.version)  # Verify hashes and VERSION.
                self.assertEqual(recovered, f.payload)  # Require archive payload parity.
                prefix = release_tool.archive_prefix(policy) + '/'  # Define exact wrapper.
                with zipfile.ZipFile(path) as archive:  # Inspect stored metadata portably.
                    self.assertEqual(archive.namelist(), [prefix] + [prefix + name for name in sorted(actual)])  # Reject hidden extras.
                    self.assertEqual({entry.filename: stat.S_IMODE(entry.external_attr >> 16) for entry in archive.infolist()}, {prefix: 0o755, **{prefix + name: mode for name, mode in modes.items()}})  # Verify stored Unix modes.
    #
    def test_tar_inventory(self):  # Reject malicious member lists.
        for case in ('missing', 'duplicate', 'extra', 'traversal', 'absolute', 'backslash', 'order'):  # Cover inventory failures.
            with self.subTest(case=case):  # Identify failing mutation.
                f = self.f()  # Reset valid archive.
                entries, prefix = f.members(), release_tool.archive_prefix(f.target)  # Preserve canonical ordering.
                variants = {'missing': entries[:-1], 'duplicate': entries + [entries[-1]], 'extra': entries + [(prefix + '/unreviewed.dylib', b'x')], 'traversal': entries + [(prefix + '/../escape', b'x')], 'absolute': entries + [('/escape', b'x')], 'backslash': entries + [(prefix + '\\escape', b'x')], 'order': entries[::-1]}  # Construct hostile paths.
                f.write_tar(variants[case])  # Serialize chosen mutation.
                self.reject_archive(f)  # Reject before materialization.
    #
    def test_tar_metadata_and_framing(self):  # Check headers and transport.
        variants = [('type', tarfile.SYMTYPE), ('type', tarfile.LNKTYPE), ('type', tarfile.FIFOTYPE), ('mode', 0o777), ('uid', 123), ('gid', 123), ('mtime', 1), ('uname', 'x'), ('gname', 'x')]  # Cover metadata fields.
        for field, value in variants:  # Corrupt one header each.
            with self.subTest(field=field, value=value):  # Identify rejected metadata.
                f = self.f()  # Preserve valid surrounding evidence.
                def change(index, member):  # Alter one real header.
                    if index == 1:  # Preserve root metadata.
                        setattr(member, field, value)  # Apply intended corruption.
                        if field == 'type':  # Keep forbidden types parseable.
                            member.size, member.linkname = 0, '../../outside'  # Avoid filesystem links.
                f.write_tar(f.members(), change)  # Write malformed tar bytes.
                self.reject_archive(f)  # Require early rejection.
        for case in ('trailer', 'timestamp', 'pax'):  # Cover hidden framing metadata.
            with self.subTest(case=case):  # Identify transport failure.
                f = self.f()  # Reset canonical transport.
                if case == 'trailer':  # Add hidden decompressed bytes.
                    f.archive.write_bytes(gzip.compress(gzip.decompress(f.archive.read_bytes()) + b'unknown', mtime=0))  # Preserve valid gzip framing.
                elif case == 'timestamp':  # Change normalized gzip time.
                    raw = bytearray(f.archive.read_bytes())  # Preserve compressed content.
                    raw[4] = 1  # Corrupt deterministic timestamp.
                    f.archive.write_bytes(raw)  # Store mutated header.
                else:  # Encode PAX metadata.
                    def change(index, member):  # Add forbidden extended record.
                        if index == 1:  # Keep root canonical.
                            member.pax_headers = {'comment': 'unreviewed'}  # Exercise hidden metadata guard.
                    f.write_tar(f.members(), change, tarfile.PAX_FORMAT)  # Serialize real PAX record.
                self.reject_archive(f)  # Reject hidden content.
    #
    def test_payload_semantics(self):  # Check content admission guards.
        for case in ('hash', 'version', 'notices', 'case'):  # Separate mismatch/semantic failures.
            with self.subTest(case=case):  # Identify expected rejection.
                f = self.f()  # Preserve valid audit shape.
                name, data = {'hash': ('ilium', fixture_macho(f.arch, True, b'changed')), 'version': ('VERSION', b'0.2.0\n'), 'notices': ('THIRD-PARTY.txt', b'state: blocked\n'), 'case': ('version', b'extra')}[case]  # Select one content mutation.
                f.payload[name] = data  # Alter archive content.
                if case != 'hash':  # Check semantics beyond hashing.
                    f.audit['files'][name] = hashlib.sha256(data).hexdigest()  # Bind deliberately invalid content.
                release_tool.write_archive(f.archive, f.target, f.payload)  # Preserve canonical transport.
                self.reject_archive(f)  # Require semantic rejection.
    #
    def test_macho_bounds_and_architecture(self):  # Parse real header mutations.
        for arch in packages.architectures:  # Cover both CPU encodings.
            good = fixture_macho(arch, True)  # Use valid thin syntax.
            other = 'x86_64' if arch == 'aarch64' else 'aarch64'  # Select opposite CPU.
            variants = {'truncated': good[:31], 'cpu': fixture_macho(other, True), 'kind': fixture_macho(arch, False)}  # Cover complete-header failures.
            for label, offset, value in (('fat', 0, 0xCAFEBABE), ('reserved', 28, 1), ('zero-count', 16, 0), ('count-limit', 16, 65537), ('region', 20, 4096), ('short', 36, 4), ('unaligned', 36, 12), ('overflow', 36, 32)):  # Corrupt concrete fields.
                data = bytearray(good)  # Preserve unrelated bytes.
                struct.pack_into('<I', data, offset, value)  # Encode corruption.
                variants[label] = bytes(data)  # Retain input.
            for label, data in variants.items():  # Exercise malformed header.
                with self.subTest(arch=arch, case=label):  # Identify CPU and failure.
                    self.assertRaises(release_error, packages.macho_header, data, arch, True)  # Require policy rejection.
    #
    def test_bridge_and_source_hashes(self):  # Bind custody edge.
        for field in ('archive', 'native_audit', 'runtime_inventory', 'dependency_inventory', 'embedding_receipt'):  # Cover bridge hashes.
            f = self.f()  # Preserve valid surrounding evidence.
            f.bridge[field]['sha256'] = '0' * 64  # Corrupt one expected digest.
            json_file(f.native / 'native-candidate-receipt.json', f.bridge)  # Preserve stale hash intentionally.
            self.assertRaisesRegex(release_error, 'bridge hash differs', f.load)  # Require bridge enforcement.
        for name in ('Cargo.toml', 'Cargo.lock', 'release/embedding-model.json', 'release/tests/embedding_acceptance.py'):  # Cover mutable source inputs.
            f = self.f()  # Retain source-bound receipt.
            path = f.source / name  # Select protected source file.
            path.write_bytes(path.read_bytes() + b'\n')  # Change source bytes.
            self.assertRaises(release_error, f.load)  # Require source binding.
    #
    def test_harness_identity_and_retained_bytes(self):  # Bind all execution evidence.
        fields = {'source_commit': 'b' * 40, 'target': 'x86_64-apple-darwin', 'tag': 'v0.2.0', 'version': '0.2.0', 'filename': '../native-test-binary', 'path': 'evidence/model/model.onnx', 'sha256': '0' * 64}  # Define identity mismatches.
        for field, value in fields.items():  # Corrupt one harness field.
            f = self.f()  # Reset evidence chain.
            f.harness[field] = value  # Change identity.
            f.refresh()  # Rebind outer hashes only.
            self.assertRaises(release_error, f.load)  # Reject semantic mismatch.
        for name in ('native-test-binary', 'evidence/harness/native-test-binary', 'evidence/harness/libonnxruntime.1.24.2.dylib', 'evidence/model/model.onnx'):  # Cover retained copy.
            f = self.f()  # Preserve expected evidence hashes.
            (f.native / name).write_bytes(b'changed evidence')  # Alter retained bytes.
            self.assertRaises(release_error, f.load)  # Check recursive custody.
    #
    def test_native_inventory(self):  # Reject unknown/missing entries.
        for name, is_directory in (('unknown.json', False), ('candidate/unreviewed.dylib', False), ('candidate/empty-extra', True), ('evidence/unlisted.json', False), ('evidence/empty-extra', True)):  # Cover custody boundary.
            f = self.f()  # Start with exact topology.
            path = f.native / name  # Select owned fixture path.
            path.mkdir() if is_directory else path.write_bytes(b'unknown')  # Include empty-directory extras.
            self.assertRaises(release_error, f.load)  # Require exact inventory.
        f = self.f()  # Reset missing-file case.
        (f.native / 'embedding-receipt.json').unlink()  # Remove mandatory evidence.
        self.assertRaises(release_error, f.load)  # Forbid implicit skipping.
    #
    def test_semantic_native_evidence(self):  # Check semantic custody guards.
        changes = {'blocked': lambda f: f.audit.update(state='blocked'), 'publication': lambda f: f.audit.update(publication_allowed=False), 'runtime-review': lambda f: f.runtime['files'][0].update(reviewed=False), 'unknown-import': lambda f: f.audit['dependency_closure']['graph']['ilium'].append('@executable_path/unreviewed.dylib'), 'missing-seal': lambda f: f.audit['signing']['nested_code'].pop('ilium-server'), 'invalid-seal': lambda f: f.audit['signing']['nested_code'][f.runtime_name].update(verified=False), 'unlisted-runtime': lambda f: f.runtime['files'].append(dict(f.runtime['files'][0], name='libunreviewed.1.dylib')), 'wrong-runner': lambda f: f.audit['native_identity'].update(runner='different-runner')}  # Mutate proof claims.
        for label, change in changes.items():  # Isolate rejection causes.
            with self.subTest(case=label):  # Preserve failure attribution.
                f = self.f()  # Reset valid inner evidence.
                change(f)  # Apply semantic mutation.
                f.refresh()  # Preserve all outer hashes.
                self.assertRaises(release_error, f.load)  # Reach semantic guard.
    #
    def test_json_bounds_and_shape(self):  # Check bounded strict parsing.
        f = self.f()  # Own small input files.
        path = f.base / 'input'  # Select document.
        for text in ('{"schema":1,"schema":2}', '[]', 'null', '{'):  # Cover duplicate/shape/syntax errors.
            path.write_text(text, encoding='utf-8')  # Write malformed JSON bytes.
            self.assertRaises(ValueError, packages.read_json, path)  # Require real parser rejection.
        path.write_bytes(b'12345')  # Exceed tiny test limit.
        self.assertRaises(release_error, packages.regular_file, path, limit=4)  # Check initial size bound.
        with patch.object(packages, 'max_payload_bytes', 4):  # Reach incremental hash bound.
            self.assertRaises(release_error, packages.sha, path)  # Preserve default initial limit.
        path.write_bytes(b'{"ok":true}\n')  # Use valid small JSON.
        with patch.object(packages, 'max_json_bytes', 4):  # Lower JSON budget only.
            self.assertRaises(release_error, packages.read_json, path)  # Reject before decoding.
    #
    def test_existing_output(self):  # Preserve prior materialization.
        f = self.f()  # Use valid audited archive.
        output = f.base / 'occupied'  # Select existing destination.
        output.mkdir()  # Reserve another owner's root.
        (output / 'sentinel').write_bytes(b'preserve')  # Make replacement observable.
        self.assertRaises(release_error, packages.extract_package, f.archive, f.target, f.audit, f.version, output)  # Reject existing output.
        self.assertEqual({path.name: path.read_bytes() for path in output.iterdir()}, {'sentinel': b'preserve'})  # Preserve exact prior contents.
    #
    def test_actual_symlink(self):  # Check filesystem alias admission.
        f = self.f()  # Own source and link.
        source, link = f.base / 'plain', f.base / 'link'  # Use portable paths.
        source.write_bytes(b'original')  # Create regular file.
        try:  # Windows may lack privileges.
            link.symlink_to(source)  # Exercise real filesystem alias.
        except (OSError, NotImplementedError) as error:  # Restrict capability-dependent skipping.
            self.skipTest('symlink unavailable: ' + str(error))  # Keep other tests active.
        self.assertRaises(release_error, packages.sha, link)  # Reject artifact alias.
        self.assertEqual(source.read_bytes(), b'original')  # Preserve original bytes.
    #
    def test_actual_hardlink(self):  # Distinguish artifact/system policies.
        f = self.f()  # Own linked file fixture.
        source, link = f.base / 'plain', f.base / 'link'  # Keep same filesystem.
        source.write_bytes(b'hardlink')  # Supply known fixture bytes.
        try:  # Capability varies by filesystem.
            os.link(source, link)  # Create hardlink.
        except (OSError, NotImplementedError) as error:  # Skip unavailable operation only.
            self.skipTest('hardlink unavailable: ' + str(error))  # Retain portable parser coverage.
        self.assertRaises(release_error, packages.sha, link)  # Reject linked release artifact.
        self.assertEqual(packages.sha(link, allow_hardlinks=True), digest(source))  # Require explicit system exception.
    #
    def test_distribution_layout(self):  # Inspect generated metadata.
        for arch in packages.architectures:  # Cover both package identities.
            layout = packages.package_layout(arch, '0.1.0')  # Derive source-defined layout.
            document = xml_tree.fromstring(packages.distribution_xml(layout))  # Parse XML bytes.
            options = document.find('options')  # Select executable policy.
            self.assertEqual((document.tag, options.get('hostArchitectures'), options.get('require-scripts'), options.get('allow-external-scripts')), ('installer-gui-script', packages.architectures[arch][0], 'false', 'false'))  # Require script-free native target.
            self.assertEqual([item.text for item in document.findall('pkg-ref')], [layout['pkg']['component']])  # Require sole component reference.
            self.assertFalse(document.findall('.//script'))  # Forbid script payload.
            self.assertEqual((layout['pkg']['administrator_required'], layout['pkg']['path_modified']), (True, False))  # Preserve installation contract.
            self.assertEqual(packages.package_names(arch), tuple('ilium-macos-' + arch + '.' + ext for ext in ('zip', 'pkg', 'dmg')))  # Preserve stable asset names.
    #
    def test_native_preflight(self):  # Reject before native execution.
        f = self.f()  # Obtain source-valid target.
        with patch.object(packages.platform, 'system', return_value='Linux'), patch.object(packages.ctypes, 'CDLL') as library:  # Guard native library loading.
            self.assertRaises(release_error, packages.native_identity, f.target, f.target['runner'])  # Reject wrong operating system.
            library.assert_not_called()  # Forbid Apple API access.
        work = f.base / 'work'  # Allocate runner-owned logs.
        work.mkdir()  # Create explicit work boundary.
        runner = packages.command_runner(work)  # Use command runner.
        with patch.object(packages.subprocess, 'Popen') as process:  # Guard process creation.
            for label, command in (('bad/label', [sys.executable]), ('valid', ['relative-tool'])):  # Cover command admission failures.
                self.assertRaises(release_error, runner.run, label, command)  # Require preflight rejection.
            runner.deadline = 0  # Exhaust operation budget.
            self.assertRaises(release_error, runner.run, 'expired', [str(Path(sys.executable).resolve())])  # Reject expired invocation.
            process.assert_not_called()  # No rejected command may spawn.
    #
    def test_malformed_receipts_emit_blocked_jsonl(self):  # Exercise CLI error boundary.
        for case in ('audit-files', 'runtime-member', 'harness-runtime'):  # Cover nested shape failures.
            f = self.f()  # Preserve unrelated custody evidence.
            record, field, value = {'audit-files': (f.audit, 'files', None), 'runtime-member': (f.runtime, 'files', [None]), 'harness-runtime': (f.harness, 'runtime_files', None)}[case]  # Select malformed input field.
            record[field] = value  # Apply intended shape corruption.
            f.refresh()  # Rebind outer receipt hashes.
            fields = {'manifest': f.manifest, 'workspace': f.workspace, 'tag': f.tag, 'arch': f.arch, 'source-commit': f.commit, 'runner-identity': f.target['runner'], 'native': f.native, 'archive': f.archive, 'work': f.base / 'work', 'output': f.base / 'output'}  # Supply CLI contract.
            stream = io.StringIO()  # Capture JSONL output.
            with contextlib.redirect_stdout(stream):  # Keep expected errors local.
                status = packages.main(['build'] + [str(part) for key, value in fields.items() for part in ('--' + key, value)])  # Invoke argument parser.
            records = [json.loads(line) for line in stream.getvalue().splitlines()]  # Require JSON on line.
            self.assertEqual((status, len(records)), (2, 1))  # Require single blocked result.
            self.assertEqual((records[0]['type'], records[0]['state'], records[0]['publication_allowed']), ('error', 'blocked', False))  # Forbid false qualification.
            self.assertFalse((f.base / 'output').exists())  # Forbid exposed package outputs.
#
class ownership_image_runner: # Fixture.
    def __init__(self, responses): self.responses, self.calls = list(responses), [] # Queue.
    def run(self, label, command, timeout=120, cleanup=False): # API.
        self.calls.append((label, list(map(str, command)), timeout, cleanup)) # Calls.
        if label == 'hdiutil-detach': return b'' # Exit only.
        if label != 'hdiutil-info' or not self.responses: raise AssertionError(label) # Guard.
        return self.responses.pop(0) # Observe.
#
class macos_ownership_tests(unittest.TestCase): # Custody.
    eq = unittest.TestCase.assertEqual # Assert.
    reject = unittest.TestCase.assertRaises # Assert.
    def f(self): # Own roots.
        temp = tempfile.TemporaryDirectory() # Portable.
        self.addCleanup(temp.cleanup) # Owned.
        base = Path(temp.name).resolve() # Canonical.
        owned, sibling = base / 'work', base / 'work-other' # Distinct.
        owned.mkdir(); sibling.mkdir() # Owned.
        return base, owned, sibling # Roots.
    def image(self, path, devices=('/dev/disk17',)): return {'image-path': str(path), 'system-entities': [{'dev-entry': item} for item in devices]} # Plist.
    def document(self, images): return plistlib.dumps({'images': images}) # Bytes.
    def test_cleanup(self): # Custody.
        _base, owned, sibling = self.f() # Owned.
        image = self.image(owned / 'a', ('/dev/disk17s1', '/dev/disk17')) # Ordering.
        foreign = self.image(sibling / 'a', ('/dev/disk99',)) # Foreign.
        before, after = self.document([image, foreign]), self.document([foreign]) # Reconcile.
        info = ('hdiutil-info', ['/usr/bin/hdiutil', 'info', '-plist'], 30, True) # Query.
        detach = ('hdiutil-detach', ['/usr/bin/hdiutil', 'detach', '/dev/disk17'], 60, True) # Whole disk.
        for final, fails in ((after, False), (before, True)): # Reconcile.
            runner = ownership_image_runner([before, final]) # Isolate.
            with self.reject(release_error) if fails else contextlib.nullcontext(): # Guard.
                packages.detach_owned_images(runner, (owned,)) # Exercise.
            self.eq(runner.calls, [info, detach, info]) # Exact calls.
    def test_invalid(self): # Malformed.
        _base, owned, _sibling = self.f() # Owned.
        path = owned / 'a' # Image.
        records = [{'image-path': str(path)}] + [self.image(path, value) for value in ((), ('/dev/disk17s1',), ('/dev/disk17', '/dev/disk18'))] # Ambiguity.
        data = [b'<plist><dict></plist>', self.document([17])] + [self.document([item]) for item in records] # Malformed.
        for value in data: # Isolate.
            runner = ownership_image_runner([value]) # Isolate.
            self.reject(release_error, packages.detach_owned_images, runner, (owned,)) # Reject.
            self.eq(len(runner.calls), 1) # No detach.
    def test_paths(self): # Preserve.
        base, owned, _sibling = self.f() # Parent.
        output, sentinel = owned / 'output', owned / 'sentinel' # Distinct.
        sentinel.write_bytes(b'preserve') # Sentinel.
        self.eq(packages.fresh_path(output), output) # No writes.
        for path in (sentinel, owned): self.reject(release_error, packages.fresh_path, path) # No overwrite.
        self.reject(FileNotFoundError, packages.fresh_path, base / 'missing/output') # Guard.
        self.eq(sentinel.read_bytes(), b'preserve') # Preserve.
        self.assertFalse(output.exists() or (base / 'missing').exists()) # No writes.
    def test_symlink(self): # Capability.
        base, owned, _sibling = self.f() # Foreign.
        link = owned / 'link' # Alias.
        try: link.symlink_to(base / 'absent') # Dangling.
        except (OSError, NotImplementedError) as error: self.skipTest(str(error)) # Skip only.
        runner = ownership_image_runner([self.document([self.image(link)])]) # Alias.
        self.eq(packages.owned_images(runner, (owned,)), []) # Foreign.
        self.reject(release_error, packages.fresh_path, link) # Reject.
        self.assertTrue(link.is_symlink()) # Preserve.
    def test_commit(self): # Mock ABI.
        for status in (0, -1): # Both paths.
            _base, source, output = self.f() # Distinct.
            for path in (source, output): (path / 'sentinel').write_bytes(path.name.encode()) # Sentinel.
            library = mock.Mock() # Mock.
            library.renamex_np.return_value = status # Result.
            with patch.multiple(packages.ctypes, CDLL=mock.Mock(return_value=library), get_errno=mock.Mock(return_value=errno.EEXIST)), patch.multiple(packages.os, rename=mock.DEFAULT, replace=mock.DEFAULT) as fallbacks: # Mock.
                with self.reject(release_error) if status else contextlib.nullcontext(): # Status.
                    packages.commit_directory(source, output) # Exercise.
                library.renamex_np.assert_called_once_with(os.fsencode(source), os.fsencode(output), 4) # Exclusive.
                self.eq(library.renamex_np.argtypes, [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]) # ABI.
                self.assertIs(library.renamex_np.restype, ctypes.c_int) # ABI.
                self.assertFalse(any(item.called for item in fallbacks.values())) # No fallback.
            for path in (source, output): self.eq((path / 'sentinel').read_bytes(), path.name.encode()) # Preserve.
    @unittest.skipUnless(platform.system() == 'Darwin', 'requires macOS') # Native gate.
    def test_native(self): # Owned.
        _base, source, output = self.f() # Owned.
        (source / 'sentinel').write_bytes(b'staged') # Sentinel.
        self.reject(release_error, packages.commit_directory, source, output) # Race case.
        self.eq((source / 'sentinel').read_bytes(), b'staged') # Preserve.
        self.eq(list(output.iterdir()), []) # Preserve.
        output.rmdir(); packages.commit_directory(source, output) # Owned move.
        self.assertFalse(source.exists()) # Transfer.
        self.eq((output / 'sentinel').read_bytes(), b'staged') # Bytes.
#
def cpio_record(name, value, mode=0o100755, links=1, variant=b'070707', start=0):  # Serialize CPIO fixture
    named = name.encode('ascii') + b'\0'  # Terminate pathname
    if variant == b'070707':  # Encode portable octal
        values, widths = (0, 1, mode, 0, 0, links, 0, 0, len(named), len(value)), (6, 6, 6, 6, 6, 6, 6, 11, 6, 11)  # Preserve field ordering
        return variant + ''.join(format(item, '0' + str(width) + 'o') for item, width in zip(values, widths)).encode() + named + value  # Omit odc padding
    values = (1, mode, 0, 0, links, 0, len(value), 0, 0, 0, 0, len(named), sum(value) & 0xffffffff if variant == b'070702' else 0)  # Build newc fields
    data = variant + ''.join(format(item, '08x') for item in values).encode() + named  # Encode hexadecimal header
    data += b'\0' * (-(start + len(data)) % 4) + value  # Align file data
    return data + b'\0' * (-(start + len(data)) % 4)  # Align next header
#
def cpio_payload(payload, variant=b'070707', altered_name=None, file_mode=0o100755, links=1):  # Build fixture
    data = cpio_record('.', b'', 0o40755, 2, variant)  # Include canonical root
    for name, value in payload.items():  # Serialize controlled entries
        data += cpio_record(altered_name or './' + name, value, file_mode, links, variant, len(data))  # Preserve requested metadata
    data += cpio_record('TRAILER!!!', b'', 0, 1, variant, len(data))  # Terminate archive
    return data + b'\0' * (-len(data) % 512)  # Add block padding
#
def xml_value(parent, name, value, attributes=None):  # Serialize scalar fields
    xml_tree.SubElement(parent, name, attributes or {}).text = str(value)  # Preserve explicit value
#
def xar_payload(files, kind=1, compress=True, extra_tag=None):  # Build XAR
    algorithm = {1: 'sha1', 3: 'sha256', 4: 'sha512'}[kind]  # Match Apple identifiers
    at = hashlib.new(algorithm).digest_size  # Reserve checksum prefix
    tree = xml_tree.Element('xar')  # Create native root
    toc = xml_tree.SubElement(tree, 'toc')  # Own one TOC
    checksum = xml_tree.SubElement(toc, 'checksum', {'style': algorithm})  # Bind checksum algorithm
    xml_value(checksum, 'offset', 0)  # Use heap prefix
    xml_value(checksum, 'size', at)  # Declare checksum length
    nodes, streams = {'': toc}, []  # Track explicit hierarchy
    paths = set(files) | {'/'.join(path.split('/')[:index]) for path in files for index in range(1, len(path.split('/')))}  # Include necessary ancestors
    for identifier, path in enumerate(sorted(paths), 1):  # Assign positive IDs
        parent, _separator, name = path.rpartition('/')  # Identify parent
        entry = xml_tree.SubElement(nodes[parent], 'file', {'id': str(identifier)})  # Preserve native hierarchy
        nodes[path] = entry  # Retain directory parent
        xml_value(entry, 'name', name)  # Preserve ASCII name
        xml_value(entry, 'type', 'file' if path in files else 'directory')  # Declare entry type
        if path not in files:  # Directories lack streams
            continue  # Continue child enumeration
        data = files[path]  # Select fixture bytes
        stream = zlib.compress(data) if compress else data  # Use zlib framing
        fields = xml_tree.SubElement(entry, 'data')  # Declare stream
        for key, value in (('offset', at), ('length', len(stream)), ('size', len(data))):  # Bind both extents
            xml_value(fields, key, value)  # Emit decimal values
        xml_tree.SubElement(fields, 'encoding', {'style': 'application/x-gzip' if compress else 'application/octet-stream'})  # Declare encoding
        for key, value in (('archived-checksum', stream), ('extracted-checksum', data)):  # Cover both domains
            xml_value(fields, key, hashlib.new(algorithm, value).hexdigest(), {'style': algorithm})  # Compute digests
        streams.append(stream)  # Preserve stored bytes
        at += len(stream)  # Advance contiguous heap
    if extra_tag:  # Inject semantic mutation
        xml_tree.SubElement(toc, extra_tag)  # Preserve outer integrity
    raw = xml_tree.tostring(tree, encoding='utf-8')  # Serialize TOC
    compressed = zlib.compress(raw)  # Apply native framing
    return struct.pack('>IHHQQI', 0x78617221, 28, 1, len(compressed), len(raw), kind) + compressed + hashlib.new(algorithm, compressed).digest() + b''.join(streams)  # Bind container
#
def edit_xar_toc(data, change):  # Rebind mutated metadata
    magic, header, version, toc_len, _size, kind = struct.unpack_from('>IHHQQI', data)  # Locate fixture regions
    algorithm = {1: 'sha1', 3: 'sha256', 4: 'sha512'}[kind]  # Preserve checksum choice
    tree = xml_tree.fromstring(zlib.decompress(data[header:header + toc_len]))  # Decode fixture TOC
    change(tree)  # Apply semantic mutation
    raw = xml_tree.tostring(tree, encoding='utf-8')  # Serialize changed fields
    compressed = zlib.compress(raw)  # Preserve valid compression
    heap = data[header + toc_len + hashlib.new(algorithm).digest_size:]  # Preserve member bytes
    return struct.pack('>IHHQQI', magic, header, version, len(compressed), len(raw), kind) + compressed + hashlib.new(algorithm, compressed).digest() + heap  # Reach inner guards
#
class package_reader_tests(unittest.TestCase):  # Test parsing
    def rejected(self, function, *args, pattern='', **kwargs):  # Assert guarded rejection
        with self.assertRaisesRegex(ValueError, pattern):  # Match expected failure
            function(*args, **kwargs)  # Exercise parser
    #
    def f(self):  # Own synthetic bytes
        self.files = {'ilium': b'synthetic client'}  # Never execute fixture
        self.tree = {name: {'kind': 'file', 'mode': 0o755, 'bytes': len(value), 'sha256': hashlib.sha256(value).hexdigest()} for name, value in self.files.items()}  # Model audited inventory
        self.parts = {'Distribution': b'<installer-gui-script/>', 'ilium-component.pkg/Bom': b'BOMStore' + b'\0' * 24, 'ilium-component.pkg/PackageInfo': b'<pkg-info/>', 'ilium-component.pkg/Payload': gzip.compress(cpio_payload(self.files))}  # Model product topology
    #
    def test_cpio_formats(self):  # Cover admitted formats
        self.f()  # Create parser fixture.
        for variant in (b'070707', b'070701', b'070702'):  # Exercise ASCII dialects
            raw = cpio_payload(self.files, variant)  # Serialize fixture
            for data in (raw, gzip.compress(raw)):  # Cover both framings
                self.assertEqual(smoke.parse_cpio(data, self.tree), self.files)  # Require exact recovery
    #
    def test_cpio_metadata(self):  # Preserve payload safety
        self.f()  # Create parser fixture.
        for name, mode, links in (('../ilium', 0o100755, 1), ('./extra', 0o100755, 1), ('./nested/ilium', 0o100755, 1), ('./ilium', 0o120755, 1), ('./ilium', 0o100755, 2), ('./ilium', 0o100644, 1), ('./ilium', 0o104755, 1)):  # Isolate metadata faults
            self.rejected(smoke.parse_cpio, cpio_payload(self.files, altered_name=name, file_mode=mode, links=links), self.tree)  # Preserve matching bytes
        wrong = copy.deepcopy(self.tree)  # Isolate digest mutation
        wrong['ilium']['sha256'] = '0' * 64  # Change content binding
        self.rejected(smoke.parse_cpio, cpio_payload(self.files), wrong, pattern='audited bytes')  # Reject substituted content
    #
    def test_cpio_stream_bounds(self):  # Require streams
        self.f()  # Create parser fixture.
        raw = cpio_payload(self.files)  # Start from validity
        for data in (raw[:75], raw + b'x', raw[:-1] + b'x', gzip.compress(raw) * 2, b'pbzx' + b'\0' * 20):  # Target distinct bounds
            self.rejected(smoke.parse_cpio, data, self.tree)  # Keep inventory fixed
        self.rejected(smoke.bounded_inflate, zlib.compress(b'x' * 4096), 8, zlib.MAX_WBITS)  # Exercise expansion limit
    #
    def test_xar_formats(self):  # Cover Apple variants
        self.f()  # Create parser fixture.
        for algorithm in (1, 3, 4):  # Exercise supported hashes
            for compressed in (False, True):  # Exercise both encodings
                data = xar_payload(self.parts, algorithm, compressed)  # Build valid container
                self.assertEqual(smoke.parse_xar(data), self.parts)  # Require exact recovery
                self.assertEqual(set(smoke.product_parts(data, 'ilium-component.pkg')), {'Distribution', 'Bom', 'PackageInfo', 'Payload'})  # Validate expanded topology
        inner = {name.removeprefix('ilium-component.pkg/'): value for name, value in self.parts.items() if name != 'Distribution'}  # Isolate component contents
        data = xar_payload({'Distribution': self.parts['Distribution'], 'ilium-component.pkg': xar_payload(inner)})  # Build nested topology
        self.assertEqual(smoke.product_parts(data, 'ilium-component.pkg')['Payload'], inner['Payload'])  # Preserve nested bytes
    #
    def test_xar_hashes(self):  # Exercise both hashes
        self.f()  # Create parser fixture.
        data = xar_payload(self.parts)  # Build valid archive
        for position in (28 + struct.unpack_from('>IHHQQI', data)[3], len(data) - 1):  # Locate domains
            corrupted = bytearray(data)  # Isolate mutation
            corrupted[position] ^= 1  # Change one byte
            self.rejected(smoke.parse_xar, bytes(corrupted), pattern='checksum')  # Reject corrupted container
    #
    def test_xar_unsafe_metadata(self):  # Enforce unsigned topology
        self.f()  # Create parser fixture.
        for tag in ('signature', 'x-signature', 'unknown'):  # Cover Apple signatures
            self.rejected(smoke.parse_xar, xar_payload(self.parts, extra_tag=tag))  # Preserve valid checksum
        data = xar_payload({'one': b'data'})  # Isolate structural fixture
        for change in (lambda tree: setattr(tree.find('.//name'), 'text', ' one '), lambda tree: tree.find('.//name').set('enctype', 'base64'), lambda tree: tree.find('.//type').set('link', 'original')):  # Mutate entry semantics
            self.rejected(smoke.parse_xar, edit_xar_toc(data, change))  # Rebind outer digest
        self.rejected(smoke.xml_document, '<!DOCTYPE xar [<!ENTITY a "x">]><xar>&a;</xar>'.encode('utf-16'), 'xar')  # Prevent entity bypass
    #
    def test_xar_heap_inventory(self):  # Require unique ownership
        self.f()  # Create parser fixture.
        data = xar_payload({'one': b'identical', 'two': b'identical'})  # Keep stream hashes equal
        alias = edit_xar_toc(data, lambda tree: setattr(tree.findall('.//data/offset')[1], 'text', tree.findall('.//data/offset')[0].text))  # Create valid-hash overlap
        self.rejected(smoke.parse_xar, alias, pattern='overlapping')  # Reject heap alias
        self.rejected(smoke.parse_xar, data + b'\x00', pattern='trailer')  # Include hidden suffix
        self.rejected(smoke.parse_xar, xar_payload({'ilium': b'one', 'Ilium': b'two'}), pattern='case-colliding')  # Include distinct spellings
    #
    def test_product_extras(self):  # Enforce exact topology
        self.f()  # Create parser fixture.
        for name in ('ilium-component.pkg/Scripts', 'Resources/readme.txt', 'other-component.pkg/Payload'):  # Target extras
            self.rejected(smoke.product_parts, xar_payload({**self.parts, name: b'extra'}), 'ilium-component.pkg')  # Preserve valid container
#
@unittest.skipUnless(os.environ.get('ILIUM_MACOS_TEST_NATIVE'), 'set explicit ILIUM_MACOS_TEST_NATIVE and source fixture paths for real native package acceptance')  # Require explicit native fixture.
class macos_native_package_tests(unittest.TestCase):  # Run opt-in native acceptance.
    def test_real_build_and_three_format_lifecycle(self):  # Require real lifecycle proof.
        self.assertEqual(platform.system(), 'Darwin', 'native package opt-in requires an actual macOS host')  # Fail invalid explicit opt-in.
        for variable in ('ILIUM_MACOS_TEST_WORKSPACE', 'ILIUM_MACOS_TEST_SOURCE_COMMIT', 'ILIUM_MACOS_TEST_RUNNER'):  # Require explicit source/runner inputs.
            self.assertTrue(os.environ.get(variable), 'missing explicit native fixture value: ' + variable)  # Forbid inferred fixture identity.
        workspace = Path(os.environ['ILIUM_MACOS_TEST_WORKSPACE']).resolve(strict=True)  # Resolve exact source workspace.
        native = Path(os.environ['ILIUM_MACOS_TEST_NATIVE']).resolve(strict=True)  # Resolve native artifact.
        manifest = workspace.parent / 'release/targets.toml'  # Use authoritative source manifest.
        machine = platform.machine()  # Observe native process architecture.
        self.assertIn(machine, ('x86_64', 'arm64'), 'unsupported native macOS test architecture')  # Reject unsupported host architecture.
        arch = 'aarch64' if machine == 'arm64' else 'x86_64'  # Map native CPU spelling.
        target = release_tool.selected_target(manifest, arch + '-apple-darwin')  # Select unchanged target contract.
        runner_identity = os.environ['ILIUM_MACOS_TEST_RUNNER']  # Read explicit runner identity.
        packages.native_identity(target, runner_identity)  # Reject Rosetta/runner mismatch.
        version = packages.version_from_tag('v' + tomllib.loads(workspace.read_text(encoding='utf-8'))['workspace']['package']['version'])  # Validate source version.
        tag = 'v' + version  # Derive exact source tag.
        source_commit = os.environ['ILIUM_MACOS_TEST_SOURCE_COMMIT']  # Require M2 verification against HEAD.
        base = Path(tempfile.mkdtemp(prefix='ilium-macos-native-acceptance-')).resolve()  # Retain evidence; avoid traversing failed mounts.
        build_work, output, smoke_root = base / 'build-work', base / 'packages', base / 'smoke-root'  # Separate initially absent destinations.
        build_arguments = SimpleNamespace(manifest=manifest, workspace=workspace, tag=tag, arch=arch, source_commit=source_commit, runner_identity=runner_identity, native=native, archive=native / target['archive'], work=build_work, output=output)  # Supply M2 arguments.
        built = packages.build(build_arguments)  # Run native construction.
        self.assertEqual((built['state'], built['publication_allowed']), ('built-not-qualified', False))  # Keep build success unqualified.
        self.assertEqual(set(built['packages']), set(packages.package_names(arch)))  # Require all three artifacts.
        receipt_path = output / packages.smoke_receipt_name(arch)  # Keep smoke/build receipts separate.
        smoke_arguments = SimpleNamespace(manifest=manifest, workspace=workspace, tag=tag, arch=arch, source_commit=source_commit, runner_identity=runner_identity, native=native, archive=native / target['archive'], packages=output, build_work=build_work, root=smoke_root, output=receipt_path)  # Supply M3 arguments.
        observed = smoke.smoke(smoke_arguments)  # Run installed lifecycles.
        retained = release_tool.read_json(receipt_path)  # Read emitted smoke receipt.
        self.assertEqual(observed, retained)  # Compare returned/retained evidence.
        self.assertEqual((retained['schema'], retained['state'], retained['publication_allowed']), (1, 'passed', True))  # Require completed native qualification.
        self.assertEqual(retained['build_receipt_sha256'], digest(output / packages.receipt_name(arch)))  # Bind immutable build receipt.
        self.assertEqual(retained['packages'], built['packages'])  # Bind tested container.
        self.assertEqual(set(retained['formats']), set(packages.formats))  # Require all format lifecycles.
        for fmt, result in retained['formats'].items():  # Inspect lifecycle.
            with self.subTest(fmt=fmt):  # Identify format-specific failures.
                self.assertEqual(result['state'], 'passed')  # Require completed format acceptance.
                self.assertEqual(result['payload_tree'], built['payload_tree'])  # Require installed byte/mode parity.
                self.assertIn('session', result)  # Require real session evidence.
                self.assertIn('embedding', result)  # Require real embedding evidence.
                self.assertIn('installation', result)  # Require native installation evidence.
        self.assertEqual(retained['cleanup']['state'], 'passed')  # Require owned cleanup.
        for field in ('sources_restored', 'owned_images_detached', 'installed_payloads_removed', 'package_receipts_removed', 'owned_processes_stopped', 'work_retained'):  # Check cleanup commitment.
            self.assertIs(retained['cleanup'][field], True)  # Reject partial cleanup claims.
        self.assertEqual(set(path.name for path in output.iterdir()), set(packages.package_names(arch)) | {packages.receipt_name(arch), packages.smoke_receipt_name(arch)})  # Require exact final five-file set.
#
def fixture_build_receipt(f, base):  # Model exact construction receipt.
    target, audit, _runtime, binding = f.load()  # Validate custody fixture.
    base.mkdir()  # Create exact container directory.
    for name in packages.package_names(f.arch):  # Write required container.
        (base / name).write_bytes(('synthetic parser fixture ' + name).encode('ascii'))  # Use explicitly synthetic container bytes.
    tree = {name: {'kind': 'file', 'mode': 0o755 if name in target['executables'] else 0o644, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()} for name, data in f.payload.items()}  # Define modes independently of host.
    layout = packages.package_layout(f.arch, f.version)  # Use exact M2 format contract.
    record = {'schema': 1, 'state': 'built-not-qualified', 'publication_allowed': False, 'native_payload_executed': False, 'tag': f.tag, 'version': f.version, 'arch': f.arch, 'target': target['rust_target'], **binding, 'package_files': dict(audit['files']), 'payload_tree': tree, 'packages': {name: digest(base / name) for name in packages.package_names(f.arch)}, 'package_bytes': {name: (base / name).stat().st_size for name in packages.package_names(f.arch)}, 'layout': layout, 'distribution_sha256': hashlib.sha256(packages.distribution_xml(layout)).hexdigest(), 'component_sha256': hashlib.sha256(b'synthetic component').hexdigest(), 'native_identity': {**f.identity, 'translated': False}, 'toolchain': {'synthetic_fixture': True}, 'commands': [{'label': 'pkgbuild', 'command': ['/usr/bin/pkgbuild', '--compression', 'legacy'], 'exit_code': 0}], 'payload_signing': copy.deepcopy(audit['signing']), 'payload_notarization': copy.deepcopy(audit['notarization']), 'container_signing': dict.fromkeys(packages.formats, 'unsigned'), 'container_notarization': dict.fromkeys(packages.formats, 'disabled'), 'credentials_used': False, 'owned_images_detached': True, 'work_retained': True}  # Model construction schema; no native proof.
    json_file(base / packages.receipt_name(f.arch), record)  # Write receipt fixture.
    return record, target, audit, binding  # Return expected bindings.
#
def fixture_package_info(layout, tree):  # Encode PackageInfo fixture.
    document = xml_tree.Element('pkg-info', {'format-version': '2', 'identifier': layout['pkg']['identifier'], 'version': layout['pkg']['version'], 'install-location': layout['pkg']['install_location'], 'auth': 'root'})  # Declare expected native installation.
    xml_tree.SubElement(document, 'payload', {'installKBytes': '1', 'numberOfFiles': str(len(tree) + 1)})  # Count files plus archive root.
    for name in ('bundle-version', 'upgrade-bundle', 'update-bundle', 'atomic-update-bundle', 'strict-identifier', 'relocate'):  # Include documented empty bookkeeping.
        xml_tree.SubElement(document, name)  # Keep bookkeeping elements empty.
    return xml_tree.tostring(document, encoding='utf-8', xml_declaration=True)  # Encode XML bytes.
#
class macos_smoke_contract_tests(unittest.TestCase):  # Check pure smoke admission.
    def f(self):  # Allocate receipt fixture.
        temp = tempfile.TemporaryDirectory()  # Own synthetic filesystem.
        self.addCleanup(temp.cleanup)  # Remove nonmounted fixtures.
        f = native_fixture(Path(temp.name), 'aarch64')  # Bind full native evidence.
        base = f.base / 'packages'  # Separate container directory.
        return f, base, *fixture_build_receipt(f, base)  # Return exact expected bindings.
    #
    def test_build_receipt(self):  # Admit only bound construction evidence.
        _f, base, good, target, audit, binding = self.f()  # Obtain real custody fixture.
        smoke.validate_build_receipt(good, target, audit, binding)  # Establish valid schema path.
        self.assertEqual(smoke.read_build(base, target, audit, binding), good)  # Rehash container files.
        self.assertEqual((good['state'], good['publication_allowed']), ('built-not-qualified', False))  # Forbid build-only qualification.
        changes = {'unknown-field': lambda v: v.update(unexpected=True), 'missing-field': lambda v: v.pop('native_payload_executed'), 'promoted': lambda v: v.update(state='passed', publication_allowed=True), 'executed': lambda v: v.update(native_payload_executed=True), 'commit': lambda v: v.update(source_commit='b' * 40), 'target': lambda v: v.update(target='x86_64-apple-darwin'), 'missing-file': lambda v: v['package_files'].pop('ilium-server'), 'mode': lambda v: v['payload_tree']['ilium'].update(mode=0o644), 'boolean-size': lambda v: v['payload_tree']['ilium'].update(bytes=True), 'extra-file': lambda v: v['payload_tree'].update(extra=dict(v['payload_tree']['ilium'])), 'missing-format': lambda v: v['packages'].pop(packages.package_name('aarch64', 'dmg')), 'size': lambda v: v['package_bytes'].update({packages.package_name('aarch64', 'pkg'): '100'}), 'layout': lambda v: v['layout']['pkg'].update(install_location='/tmp/other'), 'distribution': lambda v: v.update(distribution_sha256='0' * 64), 'payload-signing': lambda v: v['payload_signing'].update(state='verified'), 'container-signing': lambda v: v['container_signing'].update(pkg='verified'), 'notarization': lambda v: v['container_notarization'].update(dmg='verified'), 'translated': lambda v: v['native_identity'].update(translated=True), 'command': lambda v: v['commands'][0].update(exit_code=1), 'compression': lambda v: v['commands'][0].update(command=['/usr/bin/pkgbuild']), 'duplicate-command': lambda v: v['commands'].append(copy.deepcopy(v['commands'][0]))}  # Define semantic mutations.
        for label, change in changes.items():  # Cover exact schema/trust gates.
            with self.subTest(case=label):  # Identify rejected claim.
                record = copy.deepcopy(good)  # Preserve valid baseline.
                change(record)  # Mutate one admission invariant.
                self.assertRaises(release_error, smoke.validate_build_receipt, record, target, audit, binding)  # Require fail-closed admission.
    #
    def test_read_build_inventory_and_hashes(self):  # Bind retained containers.
        for case in ('zip', 'pkg', 'dmg', 'missing', 'extra', 'prior-smoke'):  # Cover byte/inventory edge.
            with self.subTest(case=case):  # Identify changed artifact.
                f, base, _record, target, audit, binding = self.f()  # Reset exact four-file set.
                if case in packages.formats:  # Corrupt container bytes.
                    (base / packages.package_name(f.arch, case)).write_bytes(b'changed')  # Keep stale expected digest.
                elif case == 'missing':  # Remove required package.
                    (base / packages.package_name(f.arch, 'pkg')).unlink()  # Break exact inventory.
                else:  # Add unknown/stale evidence.
                    name = packages.smoke_receipt_name(f.arch) if case == 'prior-smoke' else 'unexpected'  # Distinguish old qualification.
                    (base / name).write_bytes(b'{}\n')  # Materialize extra file.
                self.assertRaises(release_error, smoke.read_build, base, target, audit, binding)  # Reject mixed-generation products.
    #
    def test_package_metadata(self):  # Parse native installer metadata.
        _f, _base, record, _target, _audit, _binding = self.f()  # Obtain source-defined contract.
        layout, tree = record['layout'], record['payload_tree']  # Preserve expected payload/layout.
        dist = xml_tree.fromstring(packages.distribution_xml(layout))  # Parse builder Distribution bytes.
        reference = dist.find('pkg-ref')  # Select sole component.
        reference.text = '#' + layout['pkg']['component']  # Permit generated local prefix.
        reference.attrib.update(installKBytes='1', archiveKBytes='1', auth='root')  # Permit generated size bookkeeping.
        xml_tree.SubElement(reference, 'bundle-version')  # Permit empty bundle bookkeeping.
        result = smoke.validate_package_metadata(xml_tree.tostring(dist), fixture_package_info(layout, tree), layout, tree)  # Establish valid native metadata.
        self.assertEqual((result['install_location'], result['scripts'], result['container_signing'], result['container_notarization']), (layout['pkg']['install_location'], False, 'unsigned', 'disabled'))  # Preserve declared installation/trust.
        for case in ('scripts', 'location', 'arch', 'domain', 'component', 'count', 'utf16', 'depth'):  # Cover dangerous metadata changes.
            with self.subTest(case=case):  # Identify preinstallation rejection.
                dist = xml_tree.fromstring(packages.distribution_xml(layout))  # Reset Distribution.
                info = xml_tree.fromstring(fixture_package_info(layout, tree))  # Reset PackageInfo.
                if case == 'scripts':  # Add forbidden active behavior.
                    xml_tree.SubElement(info, 'scripts')  # Reject even empty scripts.
                elif case == 'location':  # Redirect installed payload.
                    info.set('install-location', '/tmp/unreviewed')  # Change native destination.
                elif case == 'arch':  # Admit wrong native CPU.
                    dist.find('options').set('hostArchitectures', 'x86_64')  # Oppose Apple Silicon fixture.
                elif case == 'domain':  # Enable unreviewed installer domain.
                    dist.find('domains').set('enable_currentUserHome', 'true')  # Change real native policy.
                elif case == 'component':  # Add another installable component.
                    xml_tree.SubElement(dist, 'pkg-ref', {'id': 'unknown'}).text = 'other.pkg'  # Break exact product topology.
                elif case == 'count':  # Lie about payload inventory.
                    info.find('payload').set('numberOfFiles', '999')  # Preserve numeric field syntax.
                dist_bytes, info_bytes = xml_tree.tostring(dist), xml_tree.tostring(info)  # Serialize chosen mutation.
                if case == 'utf16':  # Hide DTD from ASCII scans.
                    info_bytes = ('<!DOCTYPE pkg-info [<!ENTITY unused "x">]>' + info_bytes.decode()).encode('utf-16')  # Exercise alternate-encoding guard.
                elif case == 'depth':  # Bound recursive comparisons.
                    dist_bytes = b'<installer-gui-script>' + b'<x>' * 1200 + b'</x>' * 1200 + b'</installer-gui-script>'  # Stay below node cap.
                self.assertRaises(release_error, smoke.validate_package_metadata, dist_bytes, info_bytes, layout, tree)  # Require structured policy rejection.
    #
    def test_bom(self):  # Check native inventory.
        _f, _base, record, _target, _audit, _binding = self.f()  # Retain audit-bound expected tree.
        tree = record['payload_tree']  # Use canonical modes/lengths.
        rows = ['.\tdrwxr-xr-x\t40755\t0\t0\t0'] + ['./' + name + '\t' + ('-rwxr-xr-x' if entry['mode'] == 0o755 else '-rw-r--r--') + '\t' + format(stat.S_IFREG | entry['mode'], 'o') + '\t0\t0\t' + str(entry['bytes']) for name, entry in sorted(tree.items())]  # Encode six lsbom fields.
        smoke.validate_bom('\n'.join(rows) + '\n', tree)  # Establish parser success.
        for case in ('missing', 'duplicate', 'extra', 'uid', 'mode', 'length', 'link'):  # Corrupt BOM claims.
            changed = list(rows)  # Keep baseline immutable.
            if case == 'missing':  # Omit expected file.
                changed.pop()  # Break inventory.
            elif case == 'duplicate':  # Repeat valid file entry.
                changed.append(changed[1])  # Break unique ownership.
            elif case == 'extra':  # Add unreviewed regular file.
                changed.append('./unknown\t-rw-r--r--\t100644\t0\t0\t1')  # Preserve row framing.
            else:  # Alter metadata field only.
                fields = changed[1].split('\t')  # Preserve pathname.
                index, value = {'uid': (3, '1'), 'mode': (2, '104755'), 'length': (5, '999'), 'link': (1, 'lrwxr-xr-x')}[case]  # Select semantic corruption.
                fields[index] = value  # Change exactly one field.
                changed[1] = '\t'.join(fields)  # Preserve six-column record.
            with self.subTest(case=case):  # Identify rejected BOM claim.
                self.assertRaises(release_error, smoke.validate_bom, '\n'.join(changed) + '\n', tree)  # Require exact ownership metadata.
    #
    def test_timeout_is_never_diagnostic_success(self):  # Preserve runner failure categories.
        bound = 'native command exceeded time/output bound'  # Match bounded runner failure.
        for code, message, expected in ((0, bound, None), (1, bound, None), (1, 'native command failed; inspect fixture-log', b'diagnostic')):  # Preserve allowed nonzero diagnostics.
            runner = SimpleNamespace(records=[], work=Path('.'), environment={})  # Model command boundary only.
            def fail(*_args, **_kwargs):  # Simulate late natural exit.
                runner.records.append({'exit_code': code, 'stdout': 'diagnostic'})  # Retain exit/output evidence.
                raise release_error(message)  # Preserve runner failure cause.
            runner.run = fail  # Inject bounded runner failure.
            if expected is None:  # Reject deadline failures.
                self.assertRaises(release_error, smoke.run_command, runner, 'probe', ['/bin/false'], allowed=(0, 1))  # Never promote timeout to success.
            else:  # Admit explicit diagnostic status.
                self.assertEqual(smoke.run_command(runner, 'probe', ['/bin/false'], allowed=(0, 1)), expected)  # Preserve legitimate nonzero output.
#
if __name__ == "__main__":  # Support direct test execution.
    unittest.main()  # Run discovered regressions.
