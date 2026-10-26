"""Synthetic receipt mutations; these tests do not execute or qualify macOS code."""
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import release_tool
import smoke_macos_packages as smoke
import smoke_installed_animation as animation_gate
from test_macos_packages import native_fixture, fixture_build_receipt, digest


def qualified_fixture(f, build, binding):
    """Model the real M3 receipt schema with explicitly inert process identities."""
    source_inputs = {name: hashlib.sha256(((f.source / name) if (f.source / name).is_file()
                                          else (ROOT / name)).read_bytes()).hexdigest()
                     for name in ('release/scripts/smoke_macos_packages.py', 'release/tests/test_macos_packages.py', *animation_gate.SOURCE_FILES)}
    smoke_root = f.base / ('smoke-root-' + f.arch)
    (smoke_root / 'acceptance-assets').mkdir(parents=True)
    audit_copy = smoke_root / 'acceptance-assets/native-audit.json'
    audit_copy.write_bytes((f.native / 'native-audit.json').read_bytes())
    root = str(smoke_root)
    formats = {}
    model = json.loads((f.source / 'release/embedding-model.json').read_text())['files']['model.onnx']
    for name in ('zip', 'pkg', 'dmg'):
        installed_path = smoke_root / name / 'installed'  # Model fixture storage with the executing host's path grammar.
        installed = str(installed_path)  # Serialize the local installed directory once.
        client_path = str(installed_path / 'ilium')  # Reuse the validator's exact client spelling.
        server_path = str(installed_path / 'ilium-server')  # Reuse the validator's exact server spelling.
        helper_path = str(installed_path / 'ilium-animation-helper')  # Keep catalogue identity consistent.
        runtime_path = str(installed_path / f.runtime_name)  # Keep runtime identity consistent.
        nonce = '1' * 32
        pane = {'pid': 101, 'ppid': 102, 'started': 'synthetic start', 'executable': '/bin/sh'}
        server = {'pid': 102, 'ppid': 99, 'started': 'synthetic start', 'executable': server_path}  # Retain inert process custody.
        ready = nonce + ' 101\n'
        session = {'state': 'passed', 'name': 'macos-smoke-' + nonce, 'nonce': nonce,
                   'fixture_sha256': 'a' * 64, 'ready_sha256': release_tool.digest(ready.encode()),
                   'ready': ready, 'pane': pane, 'server': server, 'ancestry': [pane, server],
                   'listing': 'macos-smoke-' + nonce, 'pane_stopped_by_kill_session': True}
        proof = {'type': 'embedding-proof', 'input': 'release embedding acceptance', 'model_sha256': model,
                 'embedding': [1.0] * 384, 'executable_path': client_path,  # Bind the embedding to the same local client.
                 'binary_sha256': f.files['ilium'], 'ilium_pid': 103,
                 'loaded_runtime': runtime_path}  # Bind the runtime to the same local installation.
        output = json.dumps(proof) + '\n'
        embedding = {'state': 'passed', 'dimensions': 384, 'model_sha256': model,
                     'input': proof['input'], 'loaded_runtime': proof['loaded_runtime'],
                     'runtime_sha256': f.files[f.runtime_name],
                     'vector_sha256': release_tool.digest(json.dumps(proof['embedding'], allow_nan=False).encode()),
                     'process': {'pid': 103, 'ppid': 99, 'started': 'synthetic start', 'executable': client_path},  # Match the proof's client.
                     'native_mappings': client_path + '\n' + runtime_path + '\n', 'proof': proof,  # Preserve exact mapped-image identities.
                     'command': ['/synthetic/python', str(smoke_root / 'assets' / 'embedding_acceptance.py')],  # Keep fixture-local storage paths coherent.
                     'wrapper_sha256': binding['source_inputs']['release/tests/embedding_acceptance.py'],
                     'register_sha256': binding['source_inputs']['release/embedding-model.json'],
                     'stdout': output, 'stdout_sha256': release_tool.digest(output.encode()), 'stderr': ''}
        animation_rows = [
            {'type': 'artifact', 'gate': 'installed_catalogue', 'packages': ['beach', 'carpet'],
             'client_path': client_path, 'client_sha256': f.files['ilium'],  # Reuse the same client in animation output.
             'helper_path': helper_path,  # Reuse the same helper in animation output.
             'helper_sha256': f.files['ilium-animation-helper'],
             'worker_threads_before': 1, 'worker_bytes_before': 1024},
            *({'type': 'artifact', 'gate': 'installed_render', 'package': filename.split('-')[0],
               'archive_sha256': digest_value, 'helper_sha256': f.files['ilium-animation-helper'],
               'rendered_frames': 2, 'physical_retirement': True,
               'worker_threads_before': 1, 'worker_threads_after': 1,
               'worker_bytes_before': 1024, 'worker_bytes_after': 1024}
              for filename, digest_value in release_tool.APPROVED_PACKAGES.items()),
            {'type': 'result', 'gate': 'installed_animation', 'state': 'passed',
             'publication_allowed': False, 'packages': ['beach', 'carpet']},
        ]
        animation_stdout = ''.join(json.dumps(row) + '\n' for row in animation_rows)
        animation = {'schema': 1, 'state': 'passed', 'publication_allowed': False,
                     'scope': 'installed-animation-contract', 'format': name,
                     'target': f.target['rust_target'], 'tag': f.tag, 'version': f.version,
                     'installed_root': installed, 'executable_root': installed,
                     'launcher_command': [client_path, 'release-animation-probe'],  # Match the validated installed command exactly.
                     'native_audit_sha256': digest(audit_copy),
                     'source_files': {source_name: source_inputs[source_name]
                                      for source_name in animation_gate.SOURCE_FILES},
                     'installed_files': {member: f.files[member]
                                         for member in (*f.target['executables'], *f.target['packages'])},
                     'native_identity': {'system': 'Darwin', 'machine': f.identity['machine']},
                     'stdout': animation_stdout,
                     'stdout_sha256': release_tool.digest(animation_stdout.encode()),
                     'stderr_sha256': release_tool.digest(b''),
                     'catalogue': animation_rows[0], 'renders': animation_rows[1:3]}
        animation_path = smoke_root / name / 'installed-animation.json'
        animation_path.parent.mkdir()
        animation_path.write_text(json.dumps(animation, sort_keys=True) + '\n')
        installation = {'format': name, 'payload_removed': True}
        if name == 'dmg':
            installation.update(source_image_detached_before_execution=True, source_image={'image': str(smoke_root / 'containers' / ('ilium-macos-' + f.arch + '.dmg')), 'device': '/dev/disk99', 'mount': str(smoke_root / 'dmg' / 'source-volume')})  # Join retained fixture paths without altering target device identity.
        if name == 'pkg':
            installation.update(receipt_removed=True, target_detached=True, target_image_removed=True, host_receipts_unchanged=True)
        formats[name] = {'state': 'passed', 'installed_directory': installed, 'payload_tree': build['payload_tree'],
                         'versions': f.audit['binary_versions'], 'session': session, 'embedding': embedding,
                         'animation': animation, 'animation_receipt_sha256': digest(animation_path),
                         'source_hiding': [{'original': '/synthetic/' + item, 'hidden': str(smoke_root / name / 'hidden-inputs' / str(index))} for index, item in enumerate(('native', 'products', 'build'))],  # Keep target originals POSIX and fixture destinations host-local.
                         'installation': installation, 'owned_processes_stopped': True}
    cleanup = {'state': 'passed', **dict.fromkeys(('sources_restored', 'owned_images_detached', 'installed_payloads_removed', 'package_receipts_removed', 'owned_processes_stopped', 'work_retained'), True)}
    return {'schema': 1, 'state': 'passed', 'publication_allowed': True, 'tag': f.tag, 'version': f.version,
            'arch': f.arch, 'target': f.target['rust_target'], **binding, 'smoke_source_inputs': source_inputs,
            'build_receipt_sha256': digest(f.base / 'products' / ('macos-packages-' + f.arch + '.json')),
            'packages': build['packages'], 'package_bytes': build['package_bytes'], 'package_files': f.files,
            'payload_tree': build['payload_tree'], 'native_identity': {**f.identity, 'translated': False},
            'formats': formats, 'cleanup': cleanup, 'commands': [{'label': 'synthetic native fixture', 'exit_code': 0}],
            'container_signing': build['container_signing'], 'container_notarization': build['container_notarization'],
            'credentials_used': False, 'root': root}


class MacosPublicationTests(unittest.TestCase):
    def test_complete_native_receipt_schema_on_both_architectures(self):
        for arch in ('x86_64', 'aarch64'):
            with self.subTest(arch=arch), tempfile.TemporaryDirectory() as directory:
                f = native_fixture(Path(directory), arch)
                build, target, audit, binding = fixture_build_receipt(f, f.base / 'products')
                proof = qualified_fixture(f, build, binding)
                smoke.validate_smoke_receipt(proof, build, target, audit, binding, proof['build_receipt_sha256'], proof['smoke_source_inputs'], json.loads((f.source / 'release/embedding-model.json').read_text()))

    def test_installed_receipt_rejects_path_and_byte_substitutions(self) -> None:  # Keep path repairs subordinate to existing custody and byte gates.
        with tempfile.TemporaryDirectory() as directory:  # Retain all synthetic evidence inside an owned fixture directory.
            f = native_fixture(Path(directory), 'aarch64')  # Use inert package and process fixtures only.
            build, target, audit, binding = fixture_build_receipt(f, f.base / 'products')  # Preserve the complete source and native bindings.
            proof = qualified_fixture(f, build, binding)  # Generate a coherent baseline using host-local paths.
            model_register = json.loads((f.source / 'release/embedding-model.json').read_text())  # Reuse the model authority for every mutation.
            def validate(receipt: dict[str, object]) -> None:  # Exercise the unchanged complete production validator.
                smoke.validate_smoke_receipt(receipt, build, target, audit, binding, proof['build_receipt_sha256'], proof['smoke_source_inputs'], model_register)  # Keep expected bindings independent of each changed receipt.
            validate(proof)  # Ensure an unrelated fixture failure cannot satisfy a negative assertion.
            installed = Path(proof['formats']['zip']['installed_directory'])  # Derive mutations from the actual accepted fixture root.
            hidden = Path(proof['formats']['zip']['source_hiding'][0]['hidden'])  # Preserve the original hidden-input location for comparison.
            changes = {  # Associate each independent mutation with the gate that must reject it.
                'installed-traversal': ('escaped its smoke root', lambda value: value['formats']['zip'].update(installed_directory=str(installed / '..' / 'installed'))),  # Do not normalize traversal into an accepted path.
                'hidden-traversal': ('escaped its owned root', lambda value: value['formats']['zip']['source_hiding'][0].update(hidden=str(hidden / '..' / '0'))),  # Keep hidden-input custody strict.
                'foreign-original': ('hidden macOS input custody differs', lambda value: value['formats']['zip']['source_hiding'][0].update(original=r'C:\synthetic\native')),  # Target provenance must still use absolute POSIX paths.
                'foreign-server': ('session did not use the installed server', lambda value: value['formats']['zip']['session']['server'].update(executable=str(installed.parent / 'ilium-server'))),  # Reject a sibling image even with the correct basename.
                'foreign-runtime': ('embedding loaded an unshipped runtime', lambda value: value['formats']['zip']['embedding']['proof'].update(loaded_runtime=str(installed.parent / f.runtime_name))),  # Reject an adjacent but uninstalled runtime.
                'animation-hash': ('animation evidence is missing or changed', lambda value: value['formats']['zip'].update(animation_receipt_sha256='f' * 64)),  # Keep retained animation bytes bound to their recorded digest.
            }  # Leave every unrelated evidence field unchanged.
            for case, (message, mutate) in changes.items():  # Check each rejection independently from the valid baseline.
                with self.subTest(case=case):  # Identify the precise path or byte gate under test.
                    changed = deepcopy(proof)  # Preserve the accepted baseline and its shared process identities.
                    mutate(changed)  # Alter only the declared evidence field.
                    with self.assertRaisesRegex(release_tool.ReleaseError, message):  # Prevent an unrelated validation error from satisfying the test.
                        validate(changed)  # Retain all production checks in their original order.
            animation_path = Path(proof['root']) / 'zip' / 'installed-animation.json'  # Select the retained file the validator actually reopens.
            original_bytes = animation_path.read_bytes()  # Preserve exact bytes for restoration between negative cases.
            try:  # Keep fixture storage recoverable even if an assertion fails.
                animation_path.write_bytes(original_bytes + b' ')  # Change raw bytes while leaving the parsed JSON value identical.
                with self.assertRaisesRegex(release_tool.ReleaseError, 'animation evidence is missing or changed'):  # Require actual file hashing to catch the mutation.
                    validate(proof)  # Leave both the retained digest and embedded proof untouched.
                animation_path.unlink()  # Remove only this owned fixture artifact.
                with self.assertRaisesRegex(release_tool.ReleaseError, 'animation evidence is missing or changed'):  # Keep missing evidence fail-closed.
                    validate(proof)  # Never replace a missing file with embedded JSON alone.
            finally:  # Restore exact retained bytes after every test outcome.
                animation_path.write_bytes(original_bytes)  # Preserve the original fixture for cleanup and inspection.

    def test_publication_rejects_incomplete_changed_and_forged_claims(self):
        with tempfile.TemporaryDirectory() as directory:
            f = native_fixture(Path(directory), 'aarch64')
            build, target, audit, binding = fixture_build_receipt(f, f.base / 'products')
            proof = qualified_fixture(f, build, binding)
            smoke.validate_smoke_receipt(proof, build, target, audit, binding, proof['build_receipt_sha256'], proof['smoke_source_inputs'], json.loads((f.source / 'release/embedding-model.json').read_text()))  # Require a valid baseline before testing rejection.
            changes = {
                'build-only': lambda v: v.update(state='built-not-qualified'),
                'permission': lambda v: v.update(publication_allowed=False),
                'commit': lambda v: v.update(source_commit='b' * 40),
                'native-bind': lambda v: v['native_files'].update({'native-audit.json': 'b' * 64}),
                'build-receipt': lambda v: v.update(build_receipt_sha256='b' * 64),
                'smoke-source': lambda v: v['smoke_source_inputs'].update({'release/scripts/smoke_macos_packages.py': 'b' * 64}),
                'missing-format': lambda v: v['formats'].pop('dmg'),
                'extra-format': lambda v: v['formats'].update(tar={}),
                'translated': lambda v: v['native_identity'].update(translated=True),
                'cleanup': lambda v: v['cleanup'].update(sources_restored=False),
                'version': lambda v: v['formats']['zip']['versions'].update(ilium='ilium 9.9.9'),
                'payload': lambda v: v['formats']['zip']['payload_tree']['ilium'].update(sha256='b' * 64),
                'session': lambda v: v['formats']['zip']['session'].update(state='failed'),
                'pane-alive': lambda v: v['formats']['zip']['session'].update(pane_stopped_by_kill_session=False),
                'readiness': lambda v: v['formats']['zip']['session'].update(ready='wrong 101\n'),
                'foreign-server': lambda v: v['formats']['zip']['session']['server'].update(executable='/foreign/ilium-server'),
                'no-hiding': lambda v: v['formats']['zip'].update(source_hiding=[]),
                'embed-client': lambda v: v['formats']['zip']['embedding']['proof'].update(binary_sha256='b' * 64),
                'embed-zero': lambda v: v['formats']['zip']['embedding']['proof'].update(embedding=[0.0] * 384),
                'embed-nonfinite': lambda v: v['formats']['zip']['embedding']['proof'].update(embedding=[float('nan')] * 384),
                'embed-huge-int': lambda v: v['formats']['zip']['embedding']['proof'].update(embedding=[10 ** 400] * 384),
                'embed-null-path': lambda v: v['formats']['zip']['embedding']['proof'].update(loaded_runtime=None),
                'embed-model': lambda v: v['formats']['zip']['embedding'].update(model_sha256='b' * 64),
                'embed-runtime': lambda v: v['formats']['zip']['embedding'].update(runtime_sha256='b' * 64),
                'embed-wrapper': lambda v: v['formats']['zip']['embedding'].update(wrapper_sha256='b' * 64),
                'embed-mapping': lambda v: v['formats']['zip']['embedding'].update(native_mappings='/foreign/libonnxruntime.1.24.2.dylib\n'),
                'embed-output': lambda v: v['formats']['zip']['embedding'].update(stdout='{}\n'),
                'animation-retirement': lambda v: v['formats']['zip']['animation']['renders'][1].update(physical_retirement=False),
                'dmg-mounted': lambda v: v['formats']['dmg']['installation'].update(source_image_detached_before_execution=False),
                'pkg-receipt': lambda v: v['formats']['pkg']['installation'].update(receipt_removed=False),
                'pkg-host-change': lambda v: v['formats']['pkg']['installation'].update(host_receipts_unchanged=False),
                'process-alive': lambda v: v['formats']['zip'].update(owned_processes_stopped=False),
                'unsigned-claim': lambda v: v['container_signing'].update(pkg='verified'),
                'unknown-field': lambda v: v.update(unexpected=True),
                'null-root': lambda v: v.update(root=None),
                'null-installed': lambda v: v['formats']['zip'].update(installed_directory=None),
                'null-hidden': lambda v: v['formats']['zip']['source_hiding'][0].update(hidden=None),
            }
            for case, mutate in changes.items():
                with self.subTest(case=case):
                    changed = deepcopy(proof)
                    mutate(changed)
                    with self.assertRaises(release_tool.ReleaseError):
                        smoke.validate_smoke_receipt(changed, build, target, audit, binding, proof['build_receipt_sha256'], proof['smoke_source_inputs'], json.loads((f.source / 'release/embedding-model.json').read_text()))
