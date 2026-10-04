"""Parse the actual workflow graph and exercise release evidence gates offline."""
from copy import deepcopy
import hashlib
import json
from pathlib import Path, PureWindowsPath
import re
import sys
import tempfile
import shutil
import shlex
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import yaml

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import release_tool
import release_pipeline as pipeline
import pages
import build_linux_packages as linux_packages
import smoke_installed_animation as animation_gate
import validate_animation_smoke as animation_smoke


def synthetic_animation_receipt(target, pair_directory, source_inputs, *,
                                client_hash='e' * 64, server_hash='f' * 64,
                                helper_hash='0' * 64, audit_hash='c' * 64):
    """Create an inert but complete publication-validator fixture for one target."""
    pair = PureWindowsPath(pair_directory) if target['os'] == 'windows' else Path(pair_directory)
    client, server, helper = target['executables']
    catalogue = {'type': 'artifact', 'gate': 'installed_catalogue',
                 'packages': ['beach', 'carpet'], 'client_path': str(pair / client),
                 'client_sha256': client_hash, 'helper_path': str(pair / helper),
                 'helper_sha256': helper_hash, 'worker_threads_before': 1,
                 'worker_bytes_before': 1024}
    renders = [{'type': 'artifact', 'gate': 'installed_render',
                'package': filename.split('-')[0], 'archive_sha256': digest,
                'helper_sha256': helper_hash, 'rendered_frames': 2,
                'physical_retirement': True, 'worker_threads_before': 1,
                'worker_threads_after': 1, 'worker_bytes_before': 1024,
                'worker_bytes_after': 1024}
               for filename, digest in release_tool.APPROVED_PACKAGES.items()]
    result = {'type': 'result', 'gate': 'installed_animation', 'state': 'passed',
              'publication_allowed': False, 'packages': ['beach', 'carpet']}
    output = ''.join(json.dumps(row) + '\n' for row in [catalogue, *renders, result])
    return {'schema': 1, 'state': 'passed', 'publication_allowed': False,
            'scope': 'installed-animation-contract', 'format': 'archive',
            'target': target['rust_target'], 'tag': 'v0.1.0', 'version': '0.1.0',
            'installed_root': pair_directory, 'executable_root': pair_directory,
            'launcher_command': [str(pair / client), 'release-animation-probe'],
            'native_audit_sha256': audit_hash,
            'source_files': {name: source_inputs[name] for name in animation_gate.SOURCE_FILES},
            'installed_files': {client: client_hash, server: server_hash,
                                helper: helper_hash, **release_tool.APPROVED_PACKAGES},
            'native_identity': {'system': {'linux': 'Linux', 'macos': 'Darwin',
                                          'windows': 'Windows'}[target['os']],
                                'machine': target['arch']},
            'stdout': output, 'stdout_sha256': release_tool.digest(output.encode()),
            'stderr_sha256': release_tool.digest(b''),
            'catalogue': catalogue, 'renders': renders}


def synthetic_package_animation_marker(target, source, native, archive,
                                       package_directory, package_hashes, audit):
    """Inert receipt fixture; validation still reparses all nested records."""
    source_inputs = animation_smoke.source_hashes(source)
    marker = {'schema': 1, 'state': 'passed', 'publication_allowed': False,
              'target': target['rust_target'], 'tag': 'v0.1.0',
              'archive_sha256': pipeline.sha(archive),
              'native_audit_sha256': pipeline.sha(native / 'native-audit.json'),
              'source_files': source_inputs, 'package_files': package_hashes,
              'native_identity': {'system': {'linux': 'Linux', 'windows': 'Windows'}[target['os']],
                                  'machine': target['arch']}}
    if target['os'] == 'windows':
        directory = r'C:\Users\synthetic\AppData\Local\Programs\ilium'
        proof = synthetic_animation_receipt(
            target, directory, source_inputs,
            client_hash=audit['files']['ilium.exe'],
            server_hash=audit['files']['ilium-server.exe'],
            helper_hash=audit['files']['ilium-animation-helper.exe'],
            audit_hash=marker['native_audit_sha256'])
        expected_inputs = {'archive': marker['archive_sha256'],
                           'audit': marker['native_audit_sha256'],
                           'manifest': pipeline.sha(source / 'release/targets.toml'),
                           'receipt': pipeline.sha(package_directory /
                                                   pipeline.windows_installers.RECEIPT_NAME),
                           **package_hashes}
        rows = [{'type': 'binding', 'tag': 'v0.1.0', 'version': '0.1.0',
                 'package_files': audit['files'], 'sha256': expected_inputs,
                 'source_files': source_inputs},
                {'type': 'account-before',
                 'identity': {'local_app_data': r'C:\Users\synthetic\AppData\Local'},
                 'custody': 'github-hosted'}]
        for kind in ('msi', 'exe'):
            for phase in ('initial', 'repeated'):
                label = kind + '-' + phase
                command = [str(PureWindowsPath(directory) / 'ilium.exe'),
                           'release-animation-probe']
                rows.append({'type': 'command-exit', 'label': label + '-animation',
                             'returncode': 0, 'stdout': proof['stdout'], 'stderr': ''})
                rows.append({'type': 'installed-animation', 'label': label,
                             'command': command, 'stdout_sha256': proof['stdout_sha256'],
                             'catalogue': proof['catalogue'], 'renders': proof['renders'],
                             'job_empty': True})
            rows.append({'type': 'format-result', 'format': kind, 'state': 'passed'})
        rows.append({'type': 'result', 'state': 'passed', 'version': '0.1.0',
                     'installers': package_hashes, 'input_sha256': expected_inputs,
                     'source_files': source_inputs,
                     'native_exe_msi': True, 'public_release_verified': False})
        marker['journal'] = rows
        marker['proofs'] = animation_smoke.windows_events(rows, audit, expected_inputs,
                                                          source_inputs)
        filename = animation_smoke.WINDOWS_NAME
    else:
        container_proofs = {}
        for label, (kind, image) in animation_smoke.linux_container_labels().items():
            directory = ('/usr/lib/ilium' if kind in ('deb', 'rpm') else
                         '/tmp/ilium/appimage/0.1.0-' + audit['files']['ilium'][:16])
            proof = synthetic_animation_receipt(
                target, directory, source_inputs,
                client_hash=audit['files']['ilium'],
                server_hash=audit['files']['ilium-server'],
                helper_hash=audit['files']['ilium-animation-helper'],
                audit_hash=marker['native_audit_sha256'])
            container_proofs[label] = {
                'schema': 1, 'state': 'passed', 'publication_allowed': False,
                'scope': 'installed-animation-container', 'tag': 'v0.1.0',
                'format': kind, 'image': image, 'arch': target['arch'],
                'package': linux_packages.package_name(target['arch'], kind),
                'package_sha256': package_hashes[linux_packages.package_name(target['arch'], kind)],
                'source_archive_sha256': marker['archive_sha256'],
                'native_audit_sha256': marker['native_audit_sha256'],
                'source_files': {name: source_inputs[name] for name in animation_gate.SOURCE_FILES},
                'installed_files': proof['installed_files'],
                'client_path': proof['catalogue']['client_path'],
                'helper_path': proof['catalogue']['helper_path'],
                'stdout': proof['stdout'], 'stdout_sha256': proof['stdout_sha256'],
                'catalogue': proof['catalogue'], 'renders': proof['renders']}
        roots = {'deb': '/usr/lib/ilium',
                 'snap': '/snap/ilium/12/lib/ilium',
                 'flatpak': '/app/lib/ilium',
                 'appimage': '/tmp/ilium/appimage/fixture'}
        commands = {'deb': ['/usr/bin/ilium', 'release-animation-probe'],
                    'snap': ['snap', 'run', 'ilium', 'release-animation-probe'],
                    'flatpak': ['/tmp/flatpak-client.sh', 'release-animation-probe'],
                    'appimage': ['/tmp/appimage-client.sh', 'release-animation-probe']}
        host = {}
        for kind in animation_smoke.HOST_FORMATS:
            root = roots[kind]
            proof = synthetic_animation_receipt(
                target, root, source_inputs,
                client_hash=audit['files']['ilium'],
                server_hash=audit['files']['ilium-server'],
                helper_hash=audit['files']['ilium-animation-helper'],
                audit_hash=marker['native_audit_sha256'])
            proof.update(format=kind, launcher_command=commands[kind])
            if kind == 'flatpak':
                proof['installed_root'] = '/tmp/flatpak/installation/files/lib/ilium'
            host[kind] = proof
        marker['containers'] = container_proofs
        marker['host'] = host
        identity = {'system': 'Linux', 'machine': target['arch']}
        provenance = {'arch': target['arch'], 'tag': 'v0.1.0',
                      'source_archive_sha256': marker['archive_sha256'],
                      'native_audit_sha256': marker['native_audit_sha256'],
                      'source_files': {name: source_inputs[name]
                                       for name in animation_gate.SOURCE_FILES}}
        marker['container_events'] = [
            {'type': 'result', 'command': 'containers', 'format': kind,
             'environment': image, 'state': 'passed',
             'package': linux_packages.package_name(target['arch'], kind),
             'package_sha256': package_hashes[linux_packages.package_name(target['arch'], kind)],
             'execution': 'extract-and-run' if kind == 'appimage' else 'native-container',
             'animation': {'content_sha256': animation_smoke.content_sha(
                 container_proofs[kind + '-' + image.replace('/', '_').replace(':', '_')])},
             **provenance}
            for kind, image in animation_smoke.linux_container_labels().values()]
        marker['container_events'].append({'type': 'summary', 'command': 'containers',
                                           'arch': target['arch'], 'tag': 'v0.1.0',
                                           'native_identity': identity,
                                           'state': 'passed', 'failed': 0})
        marker['host_events'] = []
        for kind in animation_smoke.HOST_FORMATS:
            # The native host calls smoke_installed_animation.smoke() in-process;
            # its two JSONL rows precede the enclosing host terminal result.
            proof_bytes = (json.dumps(host[kind], indent=2, sort_keys=True) + '\n').encode('utf-8')
            proof_path = '/tmp/synthetic-host-logs/' + kind + '-host-installed-animation.json'
            marker['host_events'].append({
                'type': 'artifact', 'path': proof_path,
                'sha256': hashlib.sha256(proof_bytes).hexdigest(),
                'bytes': len(proof_bytes)})
            marker['host_events'].append({
                'type': 'result', 'state': 'passed',
                'scope': 'installed-animation-contract', 'format': kind,
                'publication_allowed': False})
            marker['host_events'].append({
                'type': 'result', 'command': 'host', 'format': kind,
                'state': 'passed', 'package': linux_packages.package_name(target['arch'], kind),
                'package_sha256': package_hashes[linux_packages.package_name(target['arch'], kind)],
                'execution': {'deb': 'native-host', 'snap': 'classic',
                              'flatpak': 'sandbox', 'appimage': 'fuse'}[kind],
                'gates': dict.fromkeys(('preflight', 'install', 'verify', 'remove',
                                        'absence', 'state_cleanup'), 'passed'),
                'removed': True, 'animation_sha256': animation_smoke.content_sha(host[kind]),
                'log': '/tmp/synthetic-host-logs/' + kind + '-host.log',
                **provenance})
        marker['host_events'].append({'type': 'summary', 'command': 'host',
                                      'arch': target['arch'], 'tag': 'v0.1.0',
                                      'native_identity': identity,
                                      'state': 'passed', 'failed': 0})
        marker['appimage_fuse'] = {'fuse_mount': '/tmp/synthetic-fuse',
                                   'mount_records': [['/tmp/synthetic-fuse', 'fuse.squashfuse']]}
        filename = animation_smoke.LINUX_NAME.format(arch=target['arch'])
    pipeline.write_json(package_directory / filename, marker)


class WorkflowTests(unittest.TestCase):
    def setUp(self):
        self.workflow = yaml.load((ROOT / '.github/workflows/release.yml').read_text(), Loader=yaml.BaseLoader)

    def ancestors(self, name, workflow=None):
        jobs = (workflow or self.workflow)['jobs']
        seen = set()
        def visit(node, stack):
            self.assertNotIn(node, stack, 'workflow dependency cycle')
            needs = jobs[node].get('needs', [])
            if isinstance(needs, str):
                needs = [needs]
            for dependency in needs:
                self.assertIn(dependency, jobs)
                if dependency not in seen:
                    visit(dependency, stack | {node})
                    seen.add(dependency)
        visit(name, set())
        return seen

    def test_native_package_smoke_is_sealed_before_artifact_upload(self):
        for job_name, smoke_token, marker_token in (
            ('windows-installers', 'build_windows_installers.py smoke',
             'validate_animation_smoke.py'),
            ('linux-packages', 'smoke_linux_packages.py host',
             'validate_animation_smoke.py')):
            with self.subTest(job=job_name):
                steps = self.workflow['jobs'][job_name]['steps']
                smoke = next(index for index, step in enumerate(steps)
                             if smoke_token in step.get('run', ''))
                seal = next(index for index, step in enumerate(steps)
                            if marker_token in step.get('run', ''))
                upload = next(index for index, step in enumerate(steps)
                              if step.get('with', {}).get('name', '').startswith(job_name))
                self.assertLess(smoke, seal)
                self.assertLess(seal, upload)
                self.assertNotIn('if', steps[seal])

    def test_five_manifest_driven_native_and_install_matrices(self):
        targets = release_tool.load_targets(ROOT / 'release/targets.toml')
        with patch.object(pipeline, 'git_identity', return_value='a' * 40):
            matrix, version = pipeline.source_matrix(ROOT / 'release/targets.toml', ROOT / 'Cargo.toml', None)
        self.assertEqual(matrix['include'], targets)
        self.assertEqual(len(matrix['include']), 5)
        self.assertEqual(version, 'v' + pipeline.workspace_version(ROOT / 'Cargo.toml'))
        for job in ('native', 'candidate-installs', 'github-installs', 'preview-tag-installs', 'previous-installs', 'preview-installs', 'public-installs'):
            actual = self.workflow['jobs'][job]
            self.assertEqual(actual['runs-on'], '${{ matrix.runner }}')
            self.assertEqual(actual['strategy']['matrix'], '${{ fromJSON(needs.source.outputs.matrix) }}')
            self.assertEqual(actual['strategy']['fail-fast'], 'false')
        steps = self.workflow['jobs']['source']['steps']
        self.assertTrue(any('release_pipeline.py source' in step.get('run', '') for step in steps))

    def test_linux_package_job_follows_the_manifest_and_feeds_the_candidate(self):
        targets = [row for row in release_tool.load_targets(ROOT / 'release/targets.toml') if row['os'] == 'linux']
        job = self.workflow['jobs']['linux-packages']
        self.assertEqual(sorted(job['needs']), ['native', 'source'])
        self.assertEqual(job['runs-on'], '${{ matrix.runner }}')
        self.assertEqual(job['strategy']['fail-fast'], 'false')
        self.assertEqual(sorted((row['arch'], row['rust_target'], row['runner']) for row in job['strategy']['matrix']['include']),
                         sorted((row['arch'], row['rust_target'], row['runner']) for row in targets))
        self.assertIn('linux-packages', self.workflow['jobs']['aggregate']['needs'])
        runs = '\n'.join(step.get('run', '') for step in job['steps'])
        for command in ('build_linux_packages.py build', 'smoke_linux_packages.py inspect', 'smoke_linux_packages.py containers', 'smoke_linux_packages.py host'):
            self.assertIn(command, runs)
        host = next(step['run'] for step in job['steps'] if 'smoke_linux_packages.py host' in step.get('run', ''))
        for package_format in ('snap', 'flatpak', 'deb', 'appimage'):
            self.assertIn(package_format, host)
        self.assertIn('--flatpak-user-dir', host,
                      'lifecycle HOME/XDG isolation must not change the Flatpak installation')
        aggregate = '\n'.join(step.get('run', '') for step in self.workflow['jobs']['aggregate']['steps'])
        self.assertIn('--linux-packages linux-packages', aggregate)
        subjects = next(step['with']['subject-path'] for step in self.workflow['jobs']['attest']['steps'] if 'subject-path' in step.get('with', {}))
        for pattern in ('candidate/*.deb', 'candidate/*.rpm', 'candidate/*.AppImage', 'candidate/*.flatpak', 'candidate/*.snap', 'candidate/linux-packages-*.json'):
            self.assertIn(pattern, subjects)

    def test_macos_packages_use_both_native_rows_and_complete_smoke_contract(self):
        targets = [row for row in release_tool.load_targets(ROOT / 'release/targets.toml') if row['os'] == 'macos']
        job = self.workflow['jobs']['macos-packages']
        self.assertEqual(sorted(job['needs']), ['native', 'source'])
        self.assertEqual(job['runs-on'], '${{ matrix.runner }}')
        self.assertEqual(job['strategy']['fail-fast'], 'false')
        self.assertEqual(sorted((row['arch'], row['rust_target'], row['runner']) for row in job['strategy']['matrix']['include']),
                         sorted((row['arch'], row['rust_target'], row['runner']) for row in targets))
        commands = {}
        for step in job['steps']:
            words = shlex.split(step.get('run', ''), comments=True)
            if len(words) > 1 and words[1] in ('release/scripts/build_macos_packages.py', 'release/scripts/smoke_macos_packages.py'):
                self.assertNotIn(words[1], commands)
                commands[words[1]] = words
        self.assertEqual(len(commands), 2)
        for script, words in commands.items():
            for flag, value in (('--tag', '${{ needs.source.outputs.tag }}'), ('--arch', '${{ matrix.arch }}'),
                                ('--source-commit', '${{ needs.source.outputs.commit }}'), ('--runner-identity', '${{ matrix.runner }}'),
                                ('--native', 'native-macos'), ('--archive', 'native-macos/ilium-macos-${{ matrix.arch }}.tar.gz')):
                self.assertEqual(words.count(flag), 1)
                self.assertEqual(words[words.index(flag) + 1], value)
        build = commands['release/scripts/build_macos_packages.py']
        smoke = commands['release/scripts/smoke_macos_packages.py']
        self.assertEqual(build[2], 'build')
        self.assertEqual(build[build.index('--output') + 1], 'macos-packages')
        self.assertEqual(build[build.index('--work') + 1], smoke[smoke.index('--build-work') + 1])
        self.assertEqual(smoke[smoke.index('--packages') + 1], 'macos-packages')
        self.assertEqual(smoke[smoke.index('--output') + 1], 'macos-packages/macos-smoke-${{ matrix.arch }}.json')
        self.assertEqual(smoke[smoke.index('--root') + 1], '$RUNNER_TEMP/macos-package-smoke')
        upload = next(step for step in job['steps'] if step.get('with', {}).get('name') == 'macos-packages-${{ matrix.arch }}')
        self.assertEqual(upload['with']['path'], 'macos-packages/')
        self.assertEqual(upload['with']['if-no-files-found'], 'error')
        diagnostics = next(step for step in job['steps'] if step.get('with', {}).get('name') == 'diagnostics-macos-packages-${{ matrix.arch }}')
        self.assertEqual(diagnostics['if'], 'always()')
        for path in ('${{ runner.temp }}/macos-package-work/', '${{ runner.temp }}/macos-package-smoke/'):
            self.assertIn(path, diagnostics['with']['path'])
        self.assertIn('macos-packages', self.workflow['jobs']['aggregate']['needs'])
        aggregate = '\n'.join(step.get('run', '') for step in self.workflow['jobs']['aggregate']['steps'])
        self.assertIn('--macos-packages macos-packages', aggregate)
        download = next(step for step in self.workflow['jobs']['aggregate']['steps'] if step.get('with', {}).get('pattern') == 'macos-packages-*')
        self.assertEqual(download['with']['path'], 'macos-packages')
        self.assertEqual(download['with']['merge-multiple'], 'true')
        subjects = next(step['with']['subject-path'] for step in self.workflow['jobs']['attest']['steps'] if 'subject-path' in step.get('with', {}))
        for pattern in ('candidate/*.zip', 'candidate/*.pkg', 'candidate/*.dmg', 'candidate/macos-packages-*.json', 'candidate/macos-smoke-*.json'):
            self.assertIn(pattern, subjects)

    def linux_smoke_commands(self) -> dict[str, list[str]]:  # Parse actual workflow commands instead of accepting loose format substrings.
        commands: dict[str, list[str]] = {}  # Keep exactly one invocation for each supported smoke mode.
        for step in self.workflow['jobs']['linux-packages']['steps']:  # Inspect only the bounded Linux packaging job.
            for line in step.get('run', '').splitlines():  # Preserve each shell command's own arguments.
                words = shlex.split(line, comments=True)  # Ignore explanatory shell comments while respecting quoted expressions.
                if words[:2] != ['python', 'release/scripts/smoke_linux_packages.py']:  # Skip unrelated setup and publication commands.
                    continue  # Only actual smoke invocations define native coverage.
                self.assertGreaterEqual(len(words), 3)  # A malformed invocation must not disappear from coverage.
                command = words[2]  # Match the CLI subcommand emitted by the real workflow.
                self.assertNotIn(command, commands)  # Duplicate invocations cannot hide a conflicting required-format list.
                commands[command] = words  # Retain exact flag values for every subsequent assertion.
        return commands  # Reuse this parser without inventing a second workflow matrix.

    def test_linux_smoke_modes_require_exact_explicit_format_coverage(self):  # Protect required native gates from silently shrinking.
        commands = self.linux_smoke_commands()  # Read the actual YAML run commands.
        expected = {'inspect': {'deb', 'rpm', 'appimage', 'snap'}, 'containers': {'deb', 'rpm', 'appimage'}, 'host': {'deb', 'appimage', 'snap', 'flatpak'}}  # Flatpak requires native private deployment, while RPM uses native distro containers.
        self.assertEqual(set(commands), set(expected))  # Every declared mode must run exactly once.
        for command, formats in expected.items():  # Check each mode independently of names elsewhere in the job.
            with self.subTest(command=command):  # Identify the exact mode whose coverage changed.
                words = commands[command]  # Work with shell-parsed arguments rather than substring matches.
                self.assertEqual(words.count('--formats'), 1)  # Require one explicit, unambiguous coverage selection.
                actual = words[words.index('--formats') + 1].split(',')  # Read the exact comma-separated public option.
                self.assertEqual(len(actual), len(set(actual)))  # Reject duplicated formats that might conceal a missing one.
                self.assertEqual(set(actual), formats)  # Reject missing and unsupported mode/format combinations.
                self.assertEqual(words[words.index('--arch') + 1], '${{ matrix.arch }}')  # Bind every mode to both native matrix rows.
                self.assertEqual(words[words.index('--packages') + 1], 'linux-packages')  # Inspect and execute the same built package directory.
        host = commands['host']  # Preserve primary's verified Flatpak caller correction.
        self.assertEqual(host.count('--flatpak-user-dir'), 1)  # Require an explicit installation identity across HOME/XDG isolation.
        self.assertEqual(host[host.index('--flatpak-user-dir') + 1], '$RUNNER_TEMP/flatpak-install')  # Keep the supplied fresh, task-owned installation path.

    def test_linux_inspection_retains_logs_without_masking_failure(self):  # A successful tee must never qualify a failed offline inspector.
        job = self.workflow['jobs']['linux-packages']  # Keep the existing package artifact boundary.
        inspection = next(step for step in job['steps'] if 'smoke_linux_packages.py inspect ' in step.get('run', ''))  # Find the actual inspector invocation.
        self.assertIn('set -o pipefail', inspection['run'])  # Propagate the inspector's failure through the logging pipeline.
        words = self.linux_smoke_commands()['inspect']  # Parse the command using the same public argument grammar.
        self.assertEqual(words[-3:], ['|', 'tee', '$RUNNER_TEMP/package-smoke-inspect/inspect.jsonl'])  # Retain the inspector's exact JSONL outside release assets.
        diagnostics = next(step for step in job['steps'] if step.get('with', {}).get('name') == 'diagnostics-linux-packages-${{ matrix.arch }}')  # Locate the existing diagnostics upload.
        self.assertEqual(diagnostics.get('if'), 'always()')  # Failed native checks must still upload available diagnostics.
        self.assertEqual(set(diagnostics['with']['path'].splitlines()), {'${{ runner.temp }}/package-smoke-inspect/', '${{ runner.temp }}/package-smoke/', '${{ runner.temp }}/package-smoke-host/'})  # Preserve every smoke mode's evidence directory.
        artifact = next(step for step in job['steps'] if step.get('with', {}).get('name') == 'linux-packages-${{ matrix.arch }}')  # Keep successful package upload separate from diagnostics.
        self.assertEqual(artifact['with']['path'], 'linux-packages/')  # Extra acceptance files must not change the pipeline's exact asset inventory.
        self.assertNotIn('if', artifact)  # Package upload retains the normal prior-step-success condition.

    def test_linux_packaging_declares_inspection_prerequisites(self):  # Avoid relying on rpm package transitive dependencies for offline inspection.
        setup = next(step['run'] for step in self.workflow['jobs']['linux-packages']['steps'] if step.get('name') == 'Packaging toolchain')  # Read the actual disposable-runner setup.
        install = next(shlex.split(line, comments=True) for line in setup.splitlines() if 'apt-get install ' in line)  # Parse the native package-manager invocation.
        self.assertTrue({'rpm', 'rpm2cpio', 'cpio', 'squashfs-tools', 'flatpak', 'fuse3'} <= set(install))  # Declare every nonbaseline inspector and sandbox/mount package.
        self.assertIn('command -v snap', setup)  # Missing Snap tooling must fail before the required build and host gate.
        self.assertIn('command -v docker', setup)  # Native container coverage cannot disappear when Docker is unavailable.

    def test_actual_cli_dispatches_hyphenated_and_baseline_commands(self):
        with patch.object(pipeline, 'capture_baseline') as baseline:
            self.assertEqual(pipeline.main(['baseline', '--output', '/tmp/synthetic-baseline-not-written']), 0)
        baseline.assert_called_once()
        with patch.object(pipeline, 'recovery_ready') as ready:
            self.assertEqual(pipeline.main(['recovery-ready', '--candidate', '/tmp/synthetic-candidate-not-read', '--baseline', '/tmp/synthetic-baseline-not-read', '--output', '/tmp/synthetic-ready-not-written', '--log', '/tmp/synthetic-log-not-written']), 0)
        ready.assert_called_once()

    def test_every_workflow_pipeline_invocation_matches_actual_parser(self):
        commands = set()
        for job_name, job in self.workflow['jobs'].items():
            for step in job['steps']:
                for line in step.get('run', '').splitlines():
                    if line.startswith('python release/scripts/release_pipeline.py '):
                        with self.subTest(job=job_name, step=step.get('name')):
                            arguments = pipeline.parser().parse_args(shlex.split(line.replace('$RECOVERY_FAULT', 'unknown'))[2:])
                            commands.add(arguments.command)
        self.assertEqual(commands, {'source', 'native', 'aggregate', 'qualify', 'draft', 'publish', 'latest', 'deploy', 'install', 'baseline', 'recovery-ready', 'readback', 'recover'})

    def test_all_actions_are_sha_pinned_and_checkout_cannot_push(self):
        for job in self.workflow['jobs'].values():
            for step in job['steps']:
                if 'uses' in step:
                    self.assertRegex(step['uses'], r'^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+@[0-9a-f]{40}$')
                    if step['uses'].startswith('actions/checkout@'):
                        self.assertEqual(step['with']['persist-credentials'], 'false')
                self.assertNotIn('continue-on-error', step)
            self.assertNotIn('continue-on-error', job)

    def test_complete_publication_dependency_graph(self):
        expected = {
            'native': {'source'},
            'aggregate': {'native'},
            'candidate-installs': {'aggregate'},
            'attest': {'aggregate', 'candidate-installs'},
            'draft': {'attest'},
            'qualify': {'candidate-installs', 'draft'},
            'publish': {'qualify'},
            'github-installs': {'publish'},
            'pages-preview': {'github-installs'},
            'latest': {'github-installs', 'preview-tag-qualification', 'previous-qualification', 'recovery-ready'},
            'preview-tag-installs': {'pages-preview'},
            'previous-installs': {'baseline', 'github-installs'},
            'preview-installs': {'latest'},
            'preview-qualification': {'preview-installs'},
            'pages-production': {'preview-qualification'},
            'public-installs': {'pages-production'},
            'complete': {'public-installs'},
        }
        for job, dependencies in expected.items():
            self.assertTrue(dependencies <= self.ancestors(job), (job, dependencies))
        for job in ('draft', 'publish', 'latest', 'pages-preview', 'pages-production', 'baseline', 'recovery-ready'):
            self.assertIn("github.event_name == 'push'", self.workflow['jobs'][job]['if'])
        self.assertIn('workflow_dispatch', self.workflow['on'])
        self.assertEqual(self.workflow['concurrency']['cancel-in-progress'], 'false')
        self.assertEqual(self.workflow['concurrency']['group'], 'ilium-native-release-publication')

    def test_least_privilege_and_secrets_are_scoped_to_mutations(self):
        self.assertEqual(self.workflow['permissions'], {'contents': 'read'})
        for name, job in self.workflow['jobs'].items():
            permissions = job.get('permissions', {'contents': 'read'})
            if name == 'attest':
                self.assertEqual(permissions, {'contents': 'read', 'id-token': 'write', 'attestations': 'write'})
            elif name in ('draft', 'publish', 'latest', 'recover'):
                self.assertEqual(permissions, {'contents': 'write'})
            else:
                self.assertEqual(permissions, {'contents': 'read'})
            text = json.dumps(job)
            if 'CLOUDFLARE_API_TOKEN' in text:
                self.assertIn(name, ('baseline', 'publish', 'latest', 'pages-preview', 'pages-production', 'recovery-ready', 'recover', 'complete'))
            if name in ('native', 'source', 'candidate-installs'):
                self.assertNotIn('secrets.', text)
        self.assertEqual(self.workflow['jobs']['qualify']['environment'], 'release-qualification')

    def test_full_native_product_checks_and_literal_commands(self):
        code = (ROOT / 'release/scripts/release_pipeline.py').read_text()
        self.assertIn("'test', '--locked', '--workspace', '--no-fail-fast'", code)
        self.assertIn("'clippy', '--locked', '--workspace', '--all-targets'", code)
        self.assertNotIn('--skip', code)
        self.assertNotIn('--exclude', code)
        self.assertNotIn('cargo clean', code)
        self.assertIn(pipeline.POSIX_COMMAND, code)
        self.assertIn(pipeline.WINDOWS_COMMAND, code)
        native_steps = self.workflow['jobs']['native']['steps']
        self.assertTrue(any('release_pipeline.py native' in step.get('run', '') for step in native_steps))
        self.assertTrue(any('release_pipeline.py install' in step.get('run', '') for step in self.workflow['jobs']['public-installs']['steps']))


class PipelineTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.targets = release_tool.load_targets(ROOT / 'release/targets.toml')

    def create_native_fixture(self):
        # Synthetic binaries/notices and receipts never enter release outputs.
        source = self.root / 'source'; (source / 'release/tests').mkdir(parents=True)
        shutil.copytree(ROOT / 'release/site', source / 'release/site')
        for filename in ('targets.toml', 'embedding-model.json', 'ort-source.json', 'ort-runtime.json', 'licence-sources.json', 'install.sh', 'install.ps1'):
            shutil.copyfile(ROOT / 'release' / filename, source / 'release' / filename)
        shutil.copyfile(ROOT / 'release/tests/embedding_acceptance.py', source / 'release/tests/embedding_acceptance.py')
        for filename in ('release/scripts/release_tool.py', 'release/scripts/audit_native.py', 'release/scripts/build_macos_packages.py', 'release/scripts/smoke_macos_packages.py', 'release/tests/test_macos_packages.py'):
            destination = source / filename
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / filename, destination)
        (source / 'Cargo.toml').write_text('[workspace.package]\nversion = "0.1.0"\n')
        (source / 'Cargo.lock').write_text('# Synthetic fixture lock bytes\n')
        for filename in (name for name in pipeline.macos_packages.source_names
                         if name.startswith(('ilium-animation-js/', 'ilium-client/', 'ilium-platform/'))):
            destination = source / filename
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / filename, destination)
        for filename in animation_smoke.SOURCE_FILES:
            destination = source / filename
            if not destination.exists():
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / filename, destination)
        artifacts = self.root / 'artifacts'; artifacts.mkdir()
        for target in self.targets:
            directory = artifacts / ('native-' + target['rust_target']); directory.mkdir()
            (directory / 'candidate').mkdir(); (directory / 'evidence').mkdir()
            content = {target['executables'][0]: b'fixture client', target['executables'][1]: b'fixture server', target['executables'][2]: b'fixture helper', **{name: (ROOT / 'ilium-animation-js/assets/packages' / name).read_bytes() for name in release_tool.APPROVED_PACKAGES}, 'VERSION': b'0.1.0\n', 'THIRD-PARTY.txt': b'Reviewed synthetic fixture notice bytes\n'}
            if target['os'] == 'macos':
                from test_macos_packages import fixture_macho
                content['ilium'] = fixture_macho(target['arch'], True, b'synthetic client')
                content['ilium-server'] = fixture_macho(target['arch'], True, b'synthetic server')
                content['ilium-animation-helper'] = fixture_macho(target['arch'], True, b'synthetic helper')
                content['libonnxruntime.1.24.2.dylib'] = fixture_macho(target['arch'], False, b'synthetic runtime')
            hashes = {name: hashlib.sha256(value).hexdigest() for name, value in content.items()}
            audit = {'schema': 1, 'state': 'passed', 'publication_allowed': True, 'target': target['rust_target'], 'tag': 'v0.1.0', 'version': '0.1.0', 'os': target['os'], 'arch': target['arch'], 'files': hashes, 'native_identity': {'system': {'linux': 'Linux', 'macos': 'Darwin', 'windows': 'Windows'}[target['os']], 'machine': target['arch'], 'runner': target['runner']}, 'dependency_closure': {'complete': True}, 'binary_versions': {name: name.removesuffix('.exe') + ' 0.1.0' for name in target['executables']}, 'notices': {'state': 'reviewed', 'sha256': hashes['THIRD-PARTY.txt']}}
            if target['os'] == 'macos':
                audit.update(loader_paths={'state': 'passed'}, embedding={'state': 'passed'}, signing={'state': 'unsigned'}, notarization={'state': 'disabled'})
                audit['native_identity']['machine'] = pipeline.macos_packages.architectures[target['arch']][0]
                if target['arch'] == 'x86_64':
                    audit['intel_ort'] = {'state': 'passed', 'source_tag': 'v1.24.2', 'source_commit': '058787ceead760166e3c50a0a4cba8a833a6f53f', 'source_sha256': 'a' * 64}
            windows_receipt = None
            if target['os'] == 'windows':
                from release.tests.test_native_audit import NativeAuditTests
                windows_receipt = NativeAuditTests().windows_build_receipt()
                cache_values = windows_receipt['cmake_cache']['values']
                cache_path = directory / 'evidence/windows-ort-CMakeCache.txt'
                cache_path.write_text(''.join(f'{key}:STRING={value}\n' for key, value in cache_values.items()))
                windows_receipt['cmake_cache']['sha256'] = pipeline.sha(cache_path)
                windows_receipt_path = directory / 'evidence/windows-ort-build-receipt.json'
                pipeline.write_json(windows_receipt_path, windows_receipt)
                audit['windows_ort'] = {'state': 'passed', 'source_tag': 'v1.24.2', 'source_commit': '058787ceead760166e3c50a0a4cba8a833a6f53f', 'source_sha256': 'a' * 64, 'built_runtime_sha256': 'c' * 64, 'rust_crt': 'static', 'ort_crt': 'static', 'build_receipt_sha256': pipeline.sha(windows_receipt_path)}
            pipeline.write_json(directory / 'native-audit.json', audit)
            archive = directory / target['archive']; release_tool.write_archive(archive, target, content)
            if target['os'] == 'windows':
                installers_directory = self.root / 'windows-installers'; installers_directory.mkdir()
                installer_hashes = {}
                for name in pipeline.windows_installers.INSTALLER_NAMES:
                    (installers_directory / name).write_bytes(b'synthetic fixture ' + name.encode())
                    installer_hashes[name] = pipeline.sha(installers_directory / name)
                pipeline.write_json(installers_directory / pipeline.windows_installers.RECEIPT_NAME, {'schema': 1, 'tag': 'v0.1.0', 'source_archive_sha256': pipeline.sha(archive), 'package_files': hashes, 'installers': installer_hashes})
                synthetic_package_animation_marker(target, source, directory, archive,
                                                   installers_directory, installer_hashes, audit)
            if target['os'] == 'linux':
                packages_directory = self.root / 'linux-packages'; packages_directory.mkdir(exist_ok=True)
                package_hashes = {}
                for name in linux_packages.package_names(target['arch']):
                    (packages_directory / name).write_bytes(b'synthetic fixture ' + name.encode())
                    package_hashes[name] = pipeline.sha(packages_directory / name)
                pipeline.write_json(packages_directory / linux_packages.receipt_name(target['arch']), {'schema': 1, 'tag': 'v0.1.0', 'arch': target['arch'], 'source_archive_sha256': pipeline.sha(archive), 'package_files': hashes, 'packages': package_hashes})
                synthetic_package_animation_marker(target, source, directory, archive,
                                                   packages_directory, package_hashes, audit)
            model_directory = directory / 'evidence/model'; model_directory.mkdir()
            model_hashes = {}
            for name in ('model.onnx', 'tokenizer.json', 'config.json', 'special_tokens_map.json', 'tokenizer_config.json'):
                (model_directory / name).write_bytes(('synthetic ' + name).encode())
                model_hashes[name] = pipeline.sha(model_directory / name)
            model_register = json.loads((source / 'release/embedding-model.json').read_text())
            model_register['files'] = model_hashes
            (source / 'release/embedding-model.json').write_text(json.dumps(model_register, sort_keys=True, indent=2) + '\n')
            embedding_wrapper_sha = pipeline.sha(source / 'release/tests/embedding_acceptance.py')
            for name in ('runtime-inventory.json', 'dependency-inventory.json', 'embedding-receipt.json'):
                value = {'synthetic_fixture': True}
                if name == 'runtime-inventory.json' and target['os'] == 'windows':
                    value = {'schema': 1, 'files': [{'name': 'onnxruntime.dll', 'version': '1.24.2.0', 'sha256': 'c' * 64}]}
                pipeline.write_json(directory / name, value)
            pipeline.write_json(directory / 'embedding-command.json', {'schema': 1, 'state': 'reviewed', 'protocol': 'held-installed-process-v1', 'command': [str(source / 'release/tests/embedding_acceptance.py'), '--model-lock', str(source / 'release/embedding-model.json')], 'sha256': embedding_wrapper_sha})
            harness_name = 'native-test-binary.exe' if target['os'] == 'windows' else 'native-test-binary'
            (directory / harness_name).write_bytes(b'nonexecutable synthetic fixture harness')
            harness_directory = directory / 'evidence/harness'; harness_directory.mkdir()
            shutil.copyfile(directory / harness_name, harness_directory / harness_name)
            (harness_directory / 'fixture-runtime.dll').write_bytes(b'synthetic fixture runtime; never executable')
            harness = {'filename': harness_name, 'path': 'evidence/harness/' + harness_name, 'runtime_files': {'fixture-runtime.dll': pipeline.sha(harness_directory / 'fixture-runtime.dll')}, 'target': target['rust_target'], 'tag': 'v0.1.0', 'sha256': pipeline.sha(directory / harness_name), 'source_commit': 'a' * 40}
            if windows_receipt is not None:
                harness['windows_ort_build_receipt'] = {'path': 'evidence/windows-ort-build-receipt.json', 'sha256': pipeline.sha(directory / 'evidence/windows-ort-build-receipt.json')}
                harness['windows_ort_cmake_cache'] = {'path': 'evidence/windows-ort-CMakeCache.txt', 'sha256': pipeline.sha(directory / 'evidence/windows-ort-CMakeCache.txt')}
            harness['evidence_files'] = pipeline.evidence_file_hashes(directory)
            pipeline.write_json(directory / 'native-test-harness.json', harness)
            (directory / 'SHA256SUMS').write_text(pipeline.sha(archive) + '  ' + target['archive'] + '\n')
            bridge = {'schema': 1, 'state': 'passed', 'publication_allowed': True, 'target': target['rust_target'], 'tag': 'v0.1.0', 'archive': {'sha256': pipeline.sha(archive)}, 'native_audit': {'sha256': pipeline.sha(directory / 'native-audit.json')}, 'files': hashes, 'official_packages': dict(release_tool.APPROVED_PACKAGES), 'build_outputs': {name: {'sha256': hashes[name]} for name in target['executables']}, 'workspace_sha256': pipeline.sha(source / 'Cargo.toml'), 'lock_sha256': pipeline.sha(source / 'Cargo.lock'), 'embedding_model_register_sha256': pipeline.sha(source / 'release/embedding-model.json'), 'embedding_wrapper_sha256': embedding_wrapper_sha, 'embedding_model_files': model_hashes}
            for key, filename in (('runtime_inventory', 'runtime-inventory.json'), ('dependency_inventory', 'dependency-inventory.json'), ('embedding_receipt', 'embedding-receipt.json')):
                bridge[key] = {'sha256': pipeline.sha(directory / filename)}
            if windows_receipt is not None:
                bridge['windows_ort_build_receipt'] = {'path': 'evidence/windows-ort-build-receipt.json', 'sha256': pipeline.sha(directory / 'evidence/windows-ort-build-receipt.json')}
                bridge['windows_ort_cmake_cache'] = {'path': 'evidence/windows-ort-CMakeCache.txt', 'sha256': pipeline.sha(directory / 'evidence/windows-ort-CMakeCache.txt')}
            pipeline.write_json(directory / 'native-candidate-receipt.json', bridge)
            if target['os'] == 'macos':
                self.create_macos_products(source, directory, target, audit, content)
        return SimpleNamespace(manifest=source / 'release/targets.toml', workspace=source / 'Cargo.toml', tag='v0.1.0', artifacts=artifacts, windows_installers=installers_directory, linux_packages=self.root / 'linux-packages', macos_packages=self.root / 'products', output=self.root / 'aggregate')

    def create_macos_products(self, source, native, target, audit, content):
        """Serialize inert ZIP/XAR/CPIO containers and synthetic installed receipts."""
        from test_macos_packages import cpio_record, fixture_package_info, xar_payload
        from test_macos_publication import qualified_fixture
        packages = pipeline.macos_packages
        products = self.root / 'products'; products.mkdir(exist_ok=True)
        binding = pipeline.macos_native_binding(native, target, source, 'a' * 40)
        layout = packages.package_layout(target['arch'], '0.1.0')
        tree = {name: {'kind': 'file', 'mode': 0o755 if name in target['executables'] else 0o644,
                       'bytes': len(value), 'sha256': hashlib.sha256(value).hexdigest()} for name, value in content.items()}
        zip_path = products / packages.package_name(target['arch'], 'zip')
        release_tool.write_archive(zip_path, dict(target, archive=zip_path.name, format='zip'), content)
        payload = cpio_record('.', b'', 0o40755, 2)
        for name, value in content.items():
            payload += cpio_record('./' + name, value, 0o100000 | tree[name]['mode'], 1)
        payload += cpio_record('TRAILER!!!', b'', 0, 1)
        payload += b'\0' * (-len(payload) % 512)
        parts = {'Distribution': packages.distribution_xml(layout),
                 'ilium-component.pkg/PackageInfo': fixture_package_info(layout, tree),
                 'ilium-component.pkg/Bom': b'BOMStore' + b'\0' * 24,
                 'ilium-component.pkg/Payload': payload}
        (products / packages.package_name(target['arch'], 'pkg')).write_bytes(xar_payload(parts))
        (products / packages.package_name(target['arch'], 'dmg')).write_bytes(b'nonmountable synthetic DMG fixture')
        build = {'schema': 1, 'state': 'built-not-qualified', 'publication_allowed': False, 'native_payload_executed': False,
                 'tag': 'v0.1.0', 'version': '0.1.0', 'arch': target['arch'], 'target': target['rust_target'], **binding,
                 'package_files': audit['files'], 'payload_tree': tree,
                 'packages': {name: pipeline.sha(products / name) for name in packages.package_names(target['arch'])},
                 'package_bytes': {name: (products / name).stat().st_size for name in packages.package_names(target['arch'])},
                 'layout': layout, 'distribution_sha256': release_tool.digest(parts['Distribution']), 'component_sha256': 'a' * 64,
                 'native_identity': {**audit['native_identity'], 'translated': False}, 'toolchain': {'synthetic_fixture': True},
                 'commands': [{'label': 'pkgbuild', 'command': ['/usr/bin/pkgbuild', '--compression', 'legacy'], 'exit_code': 0}],
                 'payload_signing': audit['signing'], 'payload_notarization': audit['notarization'],
                 'container_signing': dict.fromkeys(packages.formats, 'unsigned'),
                 'container_notarization': dict.fromkeys(packages.formats, 'disabled'), 'credentials_used': False,
                 'owned_images_detached': True, 'work_retained': True}
        pipeline.write_json(products / packages.receipt_name(target['arch']), build)
        fixture = SimpleNamespace(base=self.root, source=source, native=native, files=audit['files'], audit=audit,
                                  runtime_name='libonnxruntime.1.24.2.dylib', identity=audit['native_identity'],
                                  target=target, tag='v0.1.0', version='0.1.0', arch=target['arch'])
        proof = qualified_fixture(fixture, build, binding)
        proof['smoke_source_inputs'] = {name: pipeline.sha(source / name) for name in proof['smoke_source_inputs']}
        pipeline.write_json(products / packages.smoke_receipt_name(target['arch']), proof)

    def test_windows_native_uses_pinned_source_builder_without_crt_pool_arguments(self):
        arguments = pipeline.parser().parse_args(['native', '--tag', 'v0.1.0', '--target', 'x86_64-pc-windows-msvc', '--runner-identity', 'windows-2025', '--work', str(self.root / 'work'), '--output', str(self.root / 'output')])
        self.assertFalse(hasattr(arguments, 'runtime_directory'))
        self.assertFalse(hasattr(arguments, 'runtime_license_inventory'))
        code = (ROOT / 'release/scripts/release_pipeline.py').read_text()
        self.assertIn('build_windows_ort.py', code)
        self.assertIn("--windows-ort-report", code)
        self.assertNotIn('runner-generated reviewed CRT inventory', code)
        self.assertIn('build_windows_ort.py', (ROOT / '.github/workflows/release.yml').read_text())
        self.assertIn('${{ runner.temp }}/native-work/*/*.log', (ROOT / '.github/workflows/release.yml').read_text())

    def test_aggregate_real_archive_parsing_and_source_tamper_gates(self):
        arguments = self.create_native_fixture()
        with patch.object(pipeline, 'git_identity', return_value='a' * 40), patch.object(pipeline, 'emit'):
            pipeline.aggregate(arguments)
        metadata, targets = pipeline.candidate_data(arguments.output, arguments.manifest, arguments.workspace)
        self.assertEqual(set(metadata['archives']), {row['archive'] for row in self.targets})
        self.assertEqual(targets, self.targets)
        self.assertEqual(len((arguments.output / 'SHA256SUMS').read_text().splitlines()), 5)
        for target in self.targets:
            binding = metadata['target_receipts'][target['rust_target']]['installed_embedding']
            self.assertEqual(binding['wrapper_sha256'], pipeline.sha(arguments.workspace.parent / 'release/tests/embedding_acceptance.py'))
            self.assertEqual(set(binding['model_files']), {'model.onnx', 'tokenizer.json', 'config.json', 'special_tokens_map.json', 'tokenizer_config.json'})
        archive = arguments.output / self.targets[0]['archive']; archive.write_bytes(archive.read_bytes() + b'changed')
        with self.assertRaises(ValueError):
            pipeline.candidate_data(arguments.output, arguments.manifest, arguments.workspace)

    def test_aggregate_publishes_bound_windows_installers_and_rejects_tamper(self):
        arguments = self.create_native_fixture()
        with patch.object(pipeline, 'git_identity', return_value='a' * 40), patch.object(pipeline, 'emit'):
            pipeline.aggregate(arguments)
        metadata, _targets = pipeline.candidate_data(arguments.output, arguments.manifest, arguments.workspace)
        names = set(pipeline.windows_installers.INSTALLER_NAMES) | {pipeline.windows_installers.RECEIPT_NAME}
        self.assertEqual(set(metadata['windows_installers']), names)
        (arguments.output / 'qualification.json').write_text('{}')
        published = {path.name for path in pipeline.publication_files(arguments.output)}
        self.assertTrue(names <= published)
        self.assertEqual(len(published), 5 + 6 + 3 + 12 + 10)
        installer = arguments.output / pipeline.windows_installers.MSI_NAME
        installer.write_bytes(installer.read_bytes() + b'changed')
        with self.assertRaises(ValueError):
            pipeline.candidate_data(arguments.output, arguments.manifest, arguments.workspace)

    def test_aggregate_publishes_bound_linux_packages_and_rejects_tamper(self):
        arguments = self.create_native_fixture()
        with patch.object(pipeline, 'git_identity', return_value='a' * 40), patch.object(pipeline, 'emit'):
            pipeline.aggregate(arguments)
        metadata, _targets = pipeline.candidate_data(arguments.output, arguments.manifest, arguments.workspace)
        names = {name for arch in ('x86_64', 'aarch64') for name in (*linux_packages.package_names(arch), linux_packages.receipt_name(arch))}
        self.assertEqual(set(metadata['linux_packages']), names)
        (arguments.output / 'qualification.json').write_text('{}')
        self.assertTrue(names <= {path.name for path in pipeline.publication_files(arguments.output)})
        package = arguments.output / linux_packages.package_name('aarch64', 'snap')
        package.write_bytes(package.read_bytes() + b'changed')
        with self.assertRaises(ValueError):
            pipeline.candidate_data(arguments.output, arguments.manifest, arguments.workspace)

    def test_aggregate_requires_both_native_package_animation_markers(self):
        for index, os_name in enumerate(('windows', 'linux')):
            with self.subTest(os=os_name):
                self.root = Path(self.temp.name) / ('missing-smoke-' + str(index))
                self.root.mkdir()
                arguments = self.create_native_fixture()
                path = (arguments.windows_installers / animation_smoke.WINDOWS_NAME
                        if os_name == 'windows' else
                        arguments.linux_packages / animation_smoke.LINUX_NAME.format(arch='aarch64'))
                path.unlink()
                with patch.object(pipeline, 'git_identity', return_value='a' * 40), \
                     patch.object(pipeline, 'emit'), self.assertRaises((ValueError, OSError)):
                    pipeline.aggregate(arguments)

    def test_candidate_reparses_package_animation_after_coordinated_hash_change(self):
        arguments = self.create_native_fixture()
        with patch.object(pipeline, 'git_identity', return_value='a' * 40), patch.object(pipeline, 'emit'):
            pipeline.aggregate(arguments)
        path = arguments.output / 'audits' / animation_smoke.LINUX_NAME.format(arch='x86_64')
        marker = release_tool.read_json(path)
        flatpak = marker['host']['flatpak']
        rows = [json.loads(line) for line in flatpak['stdout'].splitlines()]
        rows[1]['physical_retirement'] = False
        flatpak['stdout'] = ''.join(json.dumps(row) + '\n' for row in rows)
        flatpak['stdout_sha256'] = release_tool.digest(flatpak['stdout'].encode())
        flatpak['renders'][0] = rows[1]
        path.write_text(json.dumps(marker, sort_keys=True) + '\n', encoding='utf-8')
        candidate = arguments.output / 'candidate.json'
        metadata = release_tool.read_json(candidate)
        metadata['package_animation_smoke']['x86_64-unknown-linux-gnu'] = pipeline.sha(path)
        candidate.write_text(json.dumps(metadata, sort_keys=True) + '\n', encoding='utf-8')
        with self.assertRaisesRegex(ValueError, 'physically retire'):
            pipeline.candidate_data(arguments.output, arguments.manifest, arguments.workspace)

    def test_package_animation_marker_rejects_foreign_installed_roots(self):
        arguments = self.create_native_fixture()
        for target in (row for row in self.targets if row['os'] in ('windows', 'linux')
                       and row['arch'] == 'x86_64'):
            with self.subTest(os=target['os']):
                native = arguments.artifacts / ('native-' + target['rust_target'])
                archive = native / target['archive']
                if target['os'] == 'windows':
                    packages = arguments.windows_installers
                    marker = release_tool.read_json(packages / animation_smoke.WINDOWS_NAME)
                    first = next(row for row in marker['journal']
                                 if row.get('type') == 'installed-animation')
                    first['command'][0] = r'C:\Foreign\ilium.exe'
                else:
                    packages = arguments.linux_packages
                    marker = release_tool.read_json(packages / animation_smoke.LINUX_NAME.format(arch='x86_64'))
                    marker['host']['flatpak']['executable_root'] = marker['host']['flatpak']['installed_root']
                with self.assertRaises(ValueError):
                    animation_smoke.validate(
                        marker, target=target, tag='v0.1.0',
                        audit_path=native / 'native-audit.json',
                        source_root=arguments.workspace.parent, packages=packages,
                        archive_sha256=pipeline.sha(archive))

    def test_package_animation_marker_rejects_stale_passing_package_smoke(self):
        arguments = self.create_native_fixture()
        cases = (
            ('x86_64-pc-windows-msvc', arguments.windows_installers,
             pipeline.windows_installers.MSI_NAME, animation_smoke.WINDOWS_NAME),
            ('x86_64-unknown-linux-gnu', arguments.linux_packages,
             linux_packages.package_name('x86_64', 'deb'),
             animation_smoke.LINUX_NAME.format(arch='x86_64')),
            ('x86_64-unknown-linux-gnu', arguments.linux_packages,
             linux_packages.package_name('x86_64', 'snap'),
             animation_smoke.LINUX_NAME.format(arch='x86_64')),
        )
        for target_name, packages_directory, package_name, marker_name in cases:
            with self.subTest(target=target_name, package=package_name):
                target = next(row for row in self.targets if row['rust_target'] == target_name)
                native = arguments.artifacts / ('native-' + target_name)
                package = packages_directory / package_name
                old = package.read_bytes()
                marker = release_tool.read_json(packages_directory / marker_name)
                try:
                    package.write_bytes(old + b'changed ancillary installer bytes')
                    # A sealer can record the new file hash, but old successful
                    # native journals must still fail against their consumed bytes.
                    marker['package_files'][package_name] = pipeline.sha(package)
                    with self.assertRaisesRegex(ValueError, 'consum|provenance'):
                        animation_smoke.validate(
                            marker, target=target, tag='v0.1.0',
                            audit_path=native / 'native-audit.json',
                            source_root=arguments.workspace.parent,
                            packages=packages_directory,
                            archive_sha256=pipeline.sha(native / target['archive']))
                finally:
                    package.write_bytes(old)

    def test_linux_animation_marker_requires_terminal_cleanup_and_group_summaries(self):
        arguments = self.create_native_fixture()
        target = next(row for row in self.targets
                      if row['rust_target'] == 'x86_64-unknown-linux-gnu')
        native = arguments.artifacts / ('native-' + target['rust_target'])
        original = release_tool.read_json(
            arguments.linux_packages /
            animation_smoke.LINUX_NAME.format(arch='x86_64'))
        self.assertEqual(len(original['host_events']), 13)
        animation_smoke.validate(
            original, target=target, tag='v0.1.0',
            audit_path=native / 'native-audit.json',
            source_root=arguments.workspace.parent,
            packages=arguments.linux_packages,
            archive_sha256=pipeline.sha(native / target['archive']))
        mutations = (
            lambda marker: marker['host_events'][2]['gates'].__setitem__('state_cleanup', 'failed'),
            lambda marker: marker['host_events'].__delitem__(-1),
            lambda marker: marker['container_events'][-1].__setitem__('state', 'failed'),
            lambda marker: marker['host_events'].__delitem__(1),
            lambda marker: marker['host_events'].insert(3, {'type': 'warning', 'message': 'unknown'}),
            lambda marker: marker['host_events'][1].__setitem__('format', 'snap'),
            lambda marker: marker['host_events'][0].__setitem__('sha256', 'f' * 64),
            lambda marker: marker['host_events'][2].__setitem__('log', '/tmp/foreign/deb-host.log'),
        )
        for mutate in mutations:
            with self.subTest(mutation=mutate.__code__.co_firstlineno):
                marker = deepcopy(original)
                mutate(marker)
                with self.assertRaises(ValueError):
                    animation_smoke.validate(
                        marker, target=target, tag='v0.1.0',
                        audit_path=native / 'native-audit.json',
                        source_root=arguments.workspace.parent,
                        packages=arguments.linux_packages,
                        archive_sha256=pipeline.sha(native / target['archive']))

    def test_windows_animation_marker_requires_original_smoke_source(self):
        arguments = self.create_native_fixture()
        target = next(row for row in self.targets if row['os'] == 'windows')
        native = arguments.artifacts / ('native-' + target['rust_target'])
        original = release_tool.read_json(
            arguments.windows_installers / animation_smoke.WINDOWS_NAME)
        for index in (0, -1):
            with self.subTest(journal_row=index):
                marker = deepcopy(original)
                marker['journal'][index]['source_files'][
                    'release/scripts/smoke_windows_installers.py'] = 'f' * 64
                with self.assertRaisesRegex(ValueError, 'Windows smoke run'):
                    animation_smoke.validate(
                        marker, target=target, tag='v0.1.0',
                        audit_path=native / 'native-audit.json',
                        source_root=arguments.workspace.parent,
                        packages=arguments.windows_installers,
                        archive_sha256=pipeline.sha(native / target['archive']))

    def test_native_package_sealer_requires_retained_real_probe_records(self):
        arguments = self.create_native_fixture()
        for target_name, marker_name in (
            ('x86_64-pc-windows-msvc', animation_smoke.WINDOWS_NAME),
            ('x86_64-unknown-linux-gnu', animation_smoke.LINUX_NAME.format(arch='x86_64'))):
            with self.subTest(target=target_name):
                target = next(row for row in self.targets if row['rust_target'] == target_name)
                native = arguments.artifacts / ('native-' + target_name)
                packages = (arguments.windows_installers if target['os'] == 'windows'
                            else arguments.linux_packages)
                marker_path = packages / marker_name
                marker = release_tool.read_json(marker_path)
                marker_path.unlink()
                log = self.root / ('seal-log-' + target['os'])
                log.mkdir()
                inputs = dict(workspace=arguments.workspace, manifest=arguments.manifest,
                              audit=native / 'native-audit.json',
                              archive=native / target['archive'], packages=packages,
                              target=target_name, tag='v0.1.0',
                              windows_journal=None, container_log=None, host_log=None)
                if target['os'] == 'windows':
                    journal = log / 'smoke.jsonl'
                    journal.write_text(''.join(json.dumps(row) + '\n' for row in marker['journal']))
                    inputs['windows_journal'] = journal
                else:
                    container_log = log / 'container'; container_log.mkdir()
                    host_log = log / 'host'; host_log.mkdir()
                    for offset, kind in enumerate(animation_smoke.HOST_FORMATS):
                        marker['host_events'][3 * offset]['path'] = str(
                            host_log / (kind + '-host-installed-animation.json'))
                        marker['host_events'][3 * offset + 2]['log'] = str(
                            host_log / (kind + '-host.log'))
                    for label, proof in marker['containers'].items():
                        (container_log / (label + '-installed-animation.json')).write_text(
                            json.dumps(proof) + '\n')
                    (container_log / 'containers-results.jsonl').write_text(
                        ''.join(json.dumps(row) + '\n' for row in marker['container_events']))
                    for kind, proof in marker['host'].items():
                        (host_log / (kind + '-host-installed-animation.json')).write_text(
                            json.dumps(proof, indent=2, sort_keys=True) + '\n')
                    (host_log / 'host-results.jsonl').write_text(
                        ''.join(json.dumps(row) + '\n' for row in marker['host_events']))
                    (host_log / 'appimage-host.log').write_text(
                        json.dumps(marker['appimage_fuse']) + '\n')
                    inputs['container_log'], inputs['host_log'] = container_log, host_log
                with patch.object(animation_smoke.platform, 'system', return_value=
                                  {'windows': 'Windows', 'linux': 'Linux'}[target['os']]), \
                     patch.object(animation_smoke.platform, 'machine', return_value=target['arch']), \
                     patch.object(release_tool, 'emit'):
                    animation_smoke.seal(SimpleNamespace(**inputs))
                self.assertEqual(release_tool.read_json(marker_path), marker)
                if target['os'] == 'linux':
                    foreign = deepcopy(marker['host_events'])
                    foreign[0]['path'] = '/tmp/foreign/deb-host-installed-animation.json'
                    foreign[2]['log'] = '/tmp/foreign/deb-host.log'
                    (inputs['host_log'] / 'host-results.jsonl').write_text(
                        ''.join(json.dumps(row) + '\n' for row in foreign))
                    with patch.object(animation_smoke.platform, 'system', return_value='Linux'), \
                         patch.object(animation_smoke.platform, 'machine', return_value=target['arch']), \
                         patch.object(release_tool, 'emit'), \
                         self.assertRaisesRegex(ValueError, 'another smoke log'):
                        animation_smoke.seal(SimpleNamespace(**inputs))
                    (inputs['host_log'] / 'host-results.jsonl').write_text(
                        ''.join(json.dumps(row) + '\n' for row in marker['host_events']))
                marker_path.unlink()
                if target['os'] == 'windows':
                    inputs['windows_journal'].write_text('{}\n')
                else:
                    (inputs['host_log'] / 'flatpak-host-installed-animation.json').unlink()
                with patch.object(animation_smoke.platform, 'system', return_value=
                                  {'windows': 'Windows', 'linux': 'Linux'}[target['os']]), \
                     patch.object(animation_smoke.platform, 'machine', return_value=target['arch']), \
                     patch.object(release_tool, 'emit'), self.assertRaises((ValueError, OSError)):
                    animation_smoke.seal(SimpleNamespace(**inputs))

    def test_aggregate_publishes_all_ten_bound_macos_assets_and_rejects_each_changed_file(self):
        packages = pipeline.macos_packages
        names = {name for arch in ('x86_64', 'aarch64') for name in (*packages.package_names(arch), packages.receipt_name(arch), packages.smoke_receipt_name(arch))}
        original_root = self.root
        for index, name in enumerate(sorted(names)):
            with self.subTest(asset=name):
                self.root = original_root / str(index); self.root.mkdir()
                arguments = self.create_native_fixture()
                with patch.object(pipeline, 'git_identity', return_value='a' * 40), patch.object(pipeline, 'emit'):
                    pipeline.aggregate(arguments)
                metadata, _ = pipeline.candidate_data(arguments.output, arguments.manifest, arguments.workspace)
                self.assertEqual(set(metadata['macos_packages']), names)
                (arguments.output / 'qualification.json').write_text('{}')
                self.assertTrue(names <= {path.name for path in pipeline.publication_files(arguments.output)})
                asset = arguments.output / name
                asset.write_bytes(asset.read_bytes() + b'changed')
                with self.assertRaises(ValueError):
                    pipeline.candidate_data(arguments.output, arguments.manifest, arguments.workspace)
        self.root = original_root

    def test_aggregate_requires_complete_macos_assets_and_passed_installed_receipt(self):
        original_root = self.root
        for index, case in enumerate(('missing-dmg', 'extra-file', 'failed-smoke', 'wrong-source', 'missing-format')):
            with self.subTest(case=case):
                self.root = original_root / str(index); self.root.mkdir()
                arguments = self.create_native_fixture()
                packages = pipeline.macos_packages
                if case == 'missing-dmg':
                    (arguments.macos_packages / packages.package_name('aarch64', 'dmg')).unlink()
                elif case == 'extra-file':
                    (arguments.macos_packages / 'foreign').write_bytes(b'foreign fixture')
                else:
                    path = arguments.macos_packages / packages.smoke_receipt_name('aarch64')
                    receipt = release_tool.read_json(path)
                    if case == 'failed-smoke': receipt['state'] = 'failed'
                    if case == 'wrong-source': receipt['source_commit'] = 'b' * 40
                    if case == 'missing-format': receipt['formats'].pop('pkg')
                    path.write_text(json.dumps(receipt) + '\n', encoding='utf-8')
                with patch.object(pipeline, 'git_identity', return_value='a' * 40), patch.object(pipeline, 'emit'), self.assertRaises(ValueError):
                    pipeline.aggregate(arguments)
        self.root = original_root

    def test_aggregate_reparses_macos_containers_after_coordinated_receipt_rehash(self):
        from test_macos_packages import xar_payload
        original_root = self.root
        for index, kind in enumerate(('zip', 'pkg')):
            with self.subTest(format=kind):
                self.root = original_root / kind; self.root.mkdir()
                arguments = self.create_native_fixture()
                packages = pipeline.macos_packages
                target = next(row for row in self.targets if row['os'] == 'macos' and row['arch'] == 'aarch64')
                asset = arguments.macos_packages / packages.package_name('aarch64', kind)
                if kind == 'zip':
                    native = arguments.artifacts / ('native-' + target['rust_target'])
                    audit = release_tool.read_json(native / 'native-audit.json')
                    content = release_tool.read_archive(asset, dict(target, archive=asset.name, format='zip'), audit)
                    content['ilium'] = b'foreign executable fixture'
                    asset.unlink()
                    release_tool.write_archive(asset, dict(target, archive=asset.name, format='zip'), content)
                else:
                    parts = pipeline.macos_smoke.product_parts(asset.read_bytes(), 'ilium-component.pkg')
                    parts['PackageInfo'] = parts['PackageInfo'].replace(b'/usr/local/lib/ilium/0.1.0/aarch64', b'/foreign/install')
                    asset.write_bytes(xar_payload({'Distribution': parts.pop('Distribution'), **{'ilium-component.pkg/' + name: value for name, value in parts.items()}}))
                build_path = arguments.macos_packages / packages.receipt_name('aarch64')
                build = release_tool.read_json(build_path)
                build['packages'][asset.name] = pipeline.sha(asset)
                build['package_bytes'][asset.name] = asset.stat().st_size
                build_path.write_text(json.dumps(build) + '\n', encoding='utf-8')
                smoke_path = arguments.macos_packages / packages.smoke_receipt_name('aarch64')
                proof = release_tool.read_json(smoke_path)
                proof.update(packages=build['packages'], package_bytes=build['package_bytes'], build_receipt_sha256=pipeline.sha(build_path))
                smoke_path.write_text(json.dumps(proof) + '\n', encoding='utf-8')
                with patch.object(pipeline, 'git_identity', return_value='a' * 40), patch.object(pipeline, 'emit'), self.assertRaises(ValueError):
                    pipeline.aggregate(arguments)
        self.root = original_root

    def test_aggregate_rejects_linux_packages_built_from_other_files(self):
        arguments = self.create_native_fixture()
        receipt = arguments.linux_packages / linux_packages.receipt_name('x86_64')
        value = json.loads(receipt.read_text())
        value['package_files']['ilium'] = 'f' * 64
        receipt.write_text(json.dumps(value))
        with patch.object(pipeline, 'git_identity', return_value='a' * 40), patch.object(pipeline, 'emit'):
            with self.assertRaises(ValueError):
                pipeline.aggregate(arguments)

    def test_aggregate_rejects_a_missing_or_extra_linux_package(self):
        arguments = self.create_native_fixture()
        (arguments.linux_packages / linux_packages.package_name('aarch64', 'rpm')).unlink()
        with patch.object(pipeline, 'git_identity', return_value='a' * 40), patch.object(pipeline, 'emit'):
            with self.assertRaises(ValueError):
                pipeline.aggregate(arguments)

    def test_aggregate_rejects_installers_built_from_other_files(self):
        arguments = self.create_native_fixture()
        receipt = arguments.windows_installers / pipeline.windows_installers.RECEIPT_NAME
        value = json.loads(receipt.read_text())
        value['package_files']['ilium.exe'] = 'f' * 64
        receipt.write_text(json.dumps(value))
        with patch.object(pipeline, 'git_identity', return_value='a' * 40), patch.object(pipeline, 'emit'):
            with self.assertRaises(ValueError):
                pipeline.aggregate(arguments)

    def test_install_rejects_changed_adjacent_harness_runtime_before_execution(self):
        arguments = self.create_native_fixture()
        with patch.object(pipeline, 'git_identity', return_value='a' * 40), patch.object(pipeline, 'emit'):
            pipeline.aggregate(arguments)
        target = self.targets[0]
        native = arguments.artifacts / ('native-' + target['rust_target'])
        (native / 'evidence/harness/fixture-runtime.dll').write_bytes(b'tampered fixture')
        install_arguments = SimpleNamespace(candidate=arguments.output, manifest=arguments.manifest, workspace=arguments.workspace, native=native, target=target['rust_target'], mode='candidate', baseline=None, output=self.root / 'receipt.json', log=self.root / 'install.log')
        with patch.object(pipeline, 'logged') as logged:
            with self.assertRaisesRegex(ValueError, 'portable harness runtime bytes changed'):
                pipeline.install(install_arguments)
        logged.assert_not_called()

    @unittest.skipIf(sys.platform == 'win32', 'aggregate/install POSIX-fixture flows; they only run on the Linux aggregate runner')
    def test_install_passes_aggregate_bound_embedding_inputs_to_native_harness(self):
        arguments = self.create_native_fixture()
        with patch.object(pipeline, 'git_identity', return_value='a' * 40), patch.object(pipeline, 'emit'):
            pipeline.aggregate(arguments)
        target = self.targets[0]
        native = arguments.artifacts / ('native-' + target['rust_target'])
        install_arguments = SimpleNamespace(candidate=arguments.output, manifest=arguments.manifest, workspace=arguments.workspace, native=native, target=target['rust_target'], mode='candidate', baseline=None, preview_receipt=None, output=self.root / 'receipt.json', log=self.root / 'install.log')
        with patch.object(pipeline, 'logged') as logged, patch.object(pipeline, 'validate_install_receipt'), patch.object(pipeline, 'validate_install_evidence'), patch.object(pipeline.release_tool, 'read_json', wraps=pipeline.release_tool.read_json) as read_json:
            read_json.side_effect = lambda path: ({'schema': 2, 'state': 'passed'} if Path(path) == install_arguments.output else json.loads(Path(path).read_text()))
            pipeline.install(install_arguments)
        command = list(map(str, logged.call_args.args[0]))
        self.assertEqual(command[command.index('--embedding-wrapper') + 1], str(arguments.workspace.parent / 'release/tests/embedding_acceptance.py'))
        self.assertEqual(command[command.index('--embedding-command') + 1], str(native / 'embedding-command.json'))
        self.assertEqual(command[command.index('--embedding-model') + 1], str(native / 'evidence/model/model.onnx'))
        self.assertEqual(command[command.index('--embedding-model-register') + 1], str(arguments.workspace.parent / 'release/embedding-model.json'))
        self.assertEqual(json.loads(command[command.index('--expected-embedding-runtime-files') + 1]),
                         json.loads((native / 'native-test-harness.json').read_text())['runtime_files'])

    def test_install_receipt_requires_bound_embedding_scenarios_and_evidence(self):
        arguments = self.create_native_fixture()
        with patch.object(pipeline, 'git_identity', return_value='a' * 40), patch.object(pipeline, 'emit'):
            pipeline.aggregate(arguments)
        metadata = json.loads((arguments.output / 'candidate.json').read_text())
        target = self.targets[0]
        expected = metadata['target_receipts'][target['rust_target']]
        suffix = '.exe' if target['os'] == 'windows' else ''
        proof = {'schema': 2, 'state': 'passed', 'publication_allowed': True, 'tag': metadata['tag'], 'target': target['rust_target'], 'archive_sha256': metadata['archives'][target['archive']], 'installed_client_sha256': expected['client_sha256'], 'installed_server_sha256': expected['server_sha256'], 'installed_helper_sha256': expected['helper_sha256'], 'installed_helper_version': release_tool.helper_version_record('0.1.0') + '\n', 'installed_official_packages_sha256': dict(release_tool.APPROVED_PACKAGES), 'installed_pair_sha256': {'ilium' + suffix: expected['client_sha256'], 'ilium-server' + suffix: expected['server_sha256']}, 'native_identity': {'system': {'linux': 'Linux', 'macos': 'Darwin', 'windows': 'Windows'}[target['os']], 'machine': target['arch'], 'runner': target['runner']}, 'origin': 'local', 'installer_source_mode': 'exact-functions-with-local-download-adapter' if target['os'] == 'windows' else 'unmodified-installer', 'public_transport_verified': False, 'isolated_state_cleaned': True, 'native_test_binary_sha256': expected['harness_sha256'], 'installer_sha256': metadata['installers']['install.ps1' if target['os'] == 'windows' else 'install.sh'], 'pty_tests': [{'name': name, 'command': ['/tmp/harness', name, '--exact', '--nocapture', '--test-threads=1'], 'exit_code': 0, 'stdout': {'path': 'x', 'sha256': '0' * 64}, 'stderr': {'path': 'y', 'sha256': '1' * 64}} for name in pipeline.PTY_TESTS]}
        proof['installed_pair_directory'] = '/tmp/install/versions/0.1.0/bin'
        proof['installed_animation'] = synthetic_animation_receipt(
            target, proof['installed_pair_directory'], metadata['source_inputs'],
            client_hash=expected['client_sha256'], server_hash=expected['server_sha256'],
            helper_hash=expected['helper_sha256'], audit_hash=expected['native_audit_sha256'])
        with self.assertRaisesRegex(ValueError, 'embedding'):
            pipeline.validate_install_receipt(metadata, proof, target, public=False)

    def test_aggregate_refuses_missing_and_extra_native_artifacts(self):
        arguments = self.create_native_fixture()
        (arguments.artifacts / 'unexpected').mkdir()
        with patch.object(pipeline, 'git_identity', return_value='a' * 40), self.assertRaises(ValueError):
            pipeline.aggregate(arguments)
        self.assertFalse(arguments.output.exists())
        (arguments.artifacts / 'unexpected').rmdir()
        native = arguments.artifacts / ('native-' + self.targets[0]['rust_target'])
        (native / 'unexpected.txt').write_text('retained')
        with patch.object(pipeline, 'git_identity', return_value='a' * 40), self.assertRaises(ValueError):
            pipeline.aggregate(arguments)
        self.assertEqual((native / 'unexpected.txt').read_text(), 'retained')

    def test_aggregate_rehashes_exact_windows_source_receipt(self):
        arguments = self.create_native_fixture()
        native = arguments.artifacts / 'native-x86_64-pc-windows-msvc'
        (native / 'evidence/windows-ort-build-receipt.json').write_text('{}\n')
        with patch.object(pipeline, 'git_identity', return_value='a' * 40), self.assertRaises(ValueError):
            pipeline.aggregate(arguments)

    def test_aggregate_rejects_unbound_nested_evidence(self):
        arguments = self.create_native_fixture()
        native = arguments.artifacts / 'native-x86_64-pc-windows-msvc'
        (native / 'evidence/unbound.txt').write_text('unbound evidence')
        with patch.object(pipeline, 'git_identity', return_value='a' * 40), self.assertRaises(ValueError):
            pipeline.aggregate(arguments)

    def test_aggregate_rejects_changed_retained_windows_cmake_cache(self):
        arguments = self.create_native_fixture()
        native = arguments.artifacts / 'native-x86_64-pc-windows-msvc'
        cache_path = native / 'evidence/windows-ort-CMakeCache.txt'
        cache_path.write_text('CMAKE_GENERATOR:STRING=changed\n')
        receipt_path = native / 'evidence/windows-ort-build-receipt.json'
        receipt = json.loads(receipt_path.read_text())
        receipt['cmake_cache']['sha256'] = pipeline.sha(cache_path)
        receipt_path.write_text(json.dumps(receipt, sort_keys=True, indent=2) + '\n')
        audit_path = native / 'native-audit.json'
        audit = json.loads(audit_path.read_text())
        audit['windows_ort']['build_receipt_sha256'] = pipeline.sha(receipt_path)
        audit_path.write_text(json.dumps(audit, sort_keys=True, indent=2) + '\n')
        bridge_path = native / 'native-candidate-receipt.json'
        bridge = json.loads(bridge_path.read_text())
        bridge['windows_ort_build_receipt']['sha256'] = pipeline.sha(receipt_path)
        bridge['windows_ort_cmake_cache']['sha256'] = pipeline.sha(cache_path)
        bridge['native_audit']['sha256'] = pipeline.sha(audit_path)
        bridge_path.write_text(json.dumps(bridge, sort_keys=True, indent=2) + '\n')
        harness_path = native / 'native-test-harness.json'
        harness = json.loads(harness_path.read_text())
        harness['windows_ort_build_receipt']['sha256'] = pipeline.sha(receipt_path)
        harness['windows_ort_cmake_cache']['sha256'] = pipeline.sha(cache_path)
        harness['evidence_files'] = pipeline.evidence_file_hashes(native)
        harness_path.write_text(json.dumps(harness, sort_keys=True, indent=2) + '\n')
        with patch.object(pipeline, 'git_identity', return_value='a' * 40), self.assertRaises(ValueError):
            pipeline.aggregate(arguments)

    def test_install_rejects_coordinated_native_evidence_and_harness_receipt_tamper(self):
        arguments = self.create_native_fixture()
        with patch.object(pipeline, 'git_identity', return_value='a' * 40), patch.object(pipeline, 'emit'):
            pipeline.aggregate(arguments)
        target = self.targets[0]
        native = arguments.artifacts / ('native-' + target['rust_target'])
        changed = native / 'evidence/harness/fixture-runtime.dll'
        changed.write_bytes(b'coordinated replacement')
        harness_path = native / 'native-test-harness.json'
        harness = json.loads(harness_path.read_text())
        harness['runtime_files']['fixture-runtime.dll'] = pipeline.sha(changed)
        harness['evidence_files'] = pipeline.evidence_file_hashes(native)
        harness_path.write_text(json.dumps(harness, sort_keys=True, indent=2) + '\n')
        install_arguments = SimpleNamespace(candidate=arguments.output, manifest=arguments.manifest, workspace=arguments.workspace, native=native, target=target['rust_target'], mode='candidate', baseline=None, output=self.root / 'receipt.json', log=self.root / 'install.log')
        with patch.object(pipeline, 'logged') as logged, self.assertRaisesRegex(ValueError, 'aggregate-bound'):
            pipeline.install(install_arguments)
        logged.assert_not_called()

    @unittest.skipIf(sys.platform == 'win32', 'aggregate/install POSIX-fixture flows; they only run on the Linux aggregate runner')
    def test_native_evidence_hash_and_nonempty_test_gate(self):
        path = self.root / 'receipt.json'; evidence = path.with_suffix('.evidence'); evidence.mkdir()
        def write(name, value):
            destination = evidence / name
            if isinstance(value, str): destination.write_text(value)
            else: destination.write_text(json.dumps(value, sort_keys=True, indent=2) + '\n')
            return {'path': name, 'sha256': pipeline.sha(destination)}
        prior_version = '0.0.0-task7b-prior'
        snapshot = {'schema': 1, 'version': prior_version, 'pointer': prior_version + '\n', 'install_files': {'current': pipeline.release_tool.digest((prior_version + '\n').encode()), 'versions/' + prior_version + '/bin/ilium': '1' * 64, 'versions/' + prior_version + '/bin/ilium-server': '2' * 64}, 'launcher_files': {'ilium': '3' * 64, 'ilium-server': '4' * 64}, 'path_state_sha256': 'c' * 64}
        prior = write('prior-state.json', snapshot); corrupt_after = write('corrupt-after-state.json', snapshot)
        repeat_snapshot = dict(snapshot, version='0.1.0', pointer='0.1.0\n', install_files={'current': pipeline.release_tool.digest(b'0.1.0\n'), 'versions/0.1.0/bin/ilium': '1' * 64, 'versions/0.1.0/bin/ilium-server': '2' * 64})
        repeat_before = write('repeat-before-state.json', repeat_snapshot); repeat_after = write('repeat-after-state.json', repeat_snapshot)
        profile_values = {'original': 'original', 'installed': 'installed', 'repeat': 'installed', 'restored': 'original'}
        state_digest = lambda value: pipeline.release_tool.digest(json.dumps(value, sort_keys=True, separators=(',', ':')).encode())
        path_value = {'schema': 1, 'kind': 'task-owned-posix-profile', 'bin_directory': '/tmp/bin', 'entry_count': 1, 'original_sha256': state_digest(profile_values['original']), 'installed_sha256': state_digest(profile_values['installed']), 'repeat_sha256': state_digest(profile_values['repeat']), 'restored_sha256': state_digest(profile_values['restored']), 'exact_values': profile_values}
        path_ref = write('path-lifecycle.json', path_value)
        sentinel_sha = pipeline.release_tool.digest(b'must survive ownership-aware uninstall\n')
        uninstall_ref = write('uninstall-state.json', {'schema': 1, 'pair_removed': True, 'launchers_removed': True, 'path_restored': True, 'sentinel_name': 'task-owned-unrelated-sentinel.txt', 'sentinel_sha256': sentinel_sha, 'sentinel_survived': True})
        def logs(label, exit_code):
            return {'exit_code': exit_code, 'stdout': write(label + '-stdout.txt', 'output\n'), 'stderr': write(label + '-stderr.txt', '')}
        corrupt_logs, upgrade_logs = logs('corrupt-candidate', 1), logs('candidate-upgrade', 0)
        (evidence / 'corrupt-candidate-stderr.txt').write_text('ilium-install: stage=checksum error=Archive SHA-256 mismatch\n')
        corrupt_logs['stderr']['sha256'] = pipeline.sha(evidence / 'corrupt-candidate-stderr.txt')
        repeat_logs, uninstall_logs = logs('repeat-install', 0), logs('owned-uninstall', 0)
        tests = []
        for name in pipeline.PTY_TESTS:
            tests.append({'name': name, 'command': ['/tmp/harness', name, '--exact', '--nocapture', '--test-threads=1'], 'exit_code': 0, 'stdout': write(name + '.stdout.txt', 'test result: ok. 1 passed; 0 failed;\n'), 'stderr': write(name + '.stderr.txt', '')})
        vector = [0.25] * 384
        embedding_raw = {'type': 'embedding-proof', 'executable_path': '/tmp/installed/bin/ilium', 'ilium_pid': 123, 'binary_sha256': '1' * 64, 'model_sha256': '2' * 64, 'loaded_runtime': '', 'embedding': vector}
        embedding_stdout = write('embedding-stdout.txt', json.dumps(embedding_raw) + '\n')
        embedding_stderr = write('embedding-stderr.txt', '')
        initial_logs = logs('initial-install', 0)
        scenarios = {'prior_fixture': {'state': 'passed', 'kind': 'task-owned-audited-bytes', 'fixture_version': '0.0.0-task7b-prior', 'candidate_version': '0.1.0', 'installer': initial_logs, 'snapshot': prior}, 'corrupt_candidate_rollback': {'state': 'passed', 'installer': corrupt_logs, 'before': prior, 'after': corrupt_after}, 'upgrade': {'state': 'passed', 'installer': upgrade_logs, 'from': '0.0.0-task7b-prior', 'to': '0.1.0'}, 'repeat': {'state': 'passed', 'installer': repeat_logs, 'before': repeat_before, 'after': repeat_after, 'version': '0.1.0'}, 'path_deduplication': {'state': 'passed', 'evidence': path_ref}, 'pty': {'state': 'passed', 'tests': tests}, 'uninstall': {'state': 'passed', 'installer': uninstall_logs, 'evidence': uninstall_ref}}
        embedding = {'state': 'passed', 'executable_path': embedding_raw['executable_path'], 'process_id': 123, 'binary_sha256': '1' * 64, 'model_sha256': '2' * 64, 'loaded_runtime': '', 'vector_sha256': pipeline.release_tool.digest(json.dumps(vector).encode()), 'stdout': embedding_stdout, 'stderr': embedding_stderr}
        animation = {'state': 'passed', 'scope': 'synthetic-evidence-only'}
        animation_reference = write('installed-animation.json', animation)
        proof = {'origin': 'local', 'tag': 'v0.1.0', 'installed_pair_sha256': {'ilium': '1' * 64, 'ilium-server': '2' * 64}, 'pty_tests': tests, 'scenarios': scenarios, 'installed_embedding': embedding, 'installed_animation': animation, 'installed_animation_evidence': animation_reference}
        def rebind():
            values = {item.relative_to(evidence).as_posix(): pipeline.sha(item) for item in evidence.iterdir()}
            proof['evidence_files'] = values; proof['evidence_files_sha256'] = pipeline.evidence_files_digest(values)
            for container in (scenarios['prior_fixture']['snapshot'], scenarios['corrupt_candidate_rollback']['before'], scenarios['corrupt_candidate_rollback']['after'], scenarios['repeat']['before'], scenarios['repeat']['after'], scenarios['path_deduplication']['evidence'], scenarios['uninstall']['evidence'], embedding['stdout'], embedding['stderr'], animation_reference):
                container['sha256'] = values[container['path']]
            for result in (initial_logs, corrupt_logs, upgrade_logs, repeat_logs, uninstall_logs, *tests):
                result['stdout']['sha256'] = values[result['stdout']['path']]; result['stderr']['sha256'] = values[result['stderr']['path']]
        rebind(); pipeline.validate_install_evidence(proof, path, self.targets[0])
        mutations = {
            'prior': ('prior-state.json', dict(snapshot, pointer='wrong\n')),
            'corrupt': ('corrupt-after-state.json', dict(snapshot, install_files={'current': '0' * 64})),
            'wrong-error': ('corrupt-candidate-stderr.txt', 'different nonzero installer failure\n'),
            'repeat': ('repeat-after-state.json', dict(repeat_snapshot, launcher_files={'ilium': '0' * 64})),
            'path': ('path-lifecycle.json', dict(path_value, entry_count=2)),
            'pty': (pipeline.PTY_TESTS[0] + '.stdout.txt', 'test result: ok. 0 passed; 0 failed;\n'),
            'uninstall': ('uninstall-state.json', {'schema': 1, 'pair_removed': True, 'launchers_removed': True, 'path_restored': True, 'sentinel_name': 'task-owned-unrelated-sentinel.txt', 'sentinel_sha256': sentinel_sha, 'sentinel_survived': False}),
            'embedding': ('embedding-stdout.txt', json.dumps(dict(embedding_raw, embedding=[0.0] * 384)) + '\n'),
        }
        originals = {name: (evidence / name).read_bytes() for name, _ in mutations.values()}
        for category, (name, value) in mutations.items():
            with self.subTest(category=category):
                write(name, value); rebind()
                with self.assertRaises(ValueError): pipeline.validate_install_evidence(proof, path, self.targets[0])
                (evidence / name).write_bytes(originals[name]); rebind()
        missing = evidence / 'embedding-stderr.txt'
        retained = missing.read_bytes(); missing.unlink()
        with self.assertRaises(ValueError): pipeline.validate_install_evidence(proof, path, self.targets[0])
        missing.write_bytes(retained); rebind()

    @unittest.skipIf(sys.platform == 'win32', 'aggregate/install POSIX-fixture flows; they only run on the Linux aggregate runner')
    def test_bound_qualification_rejects_missing_failed_and_changed_receipts(self):
        embedding_binding = {'wrapper_sha256': '5' * 64, 'command_sha256': '6' * 64, 'model_register_sha256': '7' * 64, 'model_files': {'model.onnx': '8' * 64}, 'runtime_files': {}}
        manifest = {'schema': 1, 'tag': 'v0.1.0', 'commit': 'a' * 40, 'source_inputs': {name: '4' * 64 for name in animation_gate.SOURCE_FILES}, 'archives': {row['archive']: 'b' * 64 for row in self.targets}, 'installers': {'install.sh': '2' * 64, 'install.ps1': '3' * 64}, 'target_receipts': {row['rust_target']: {'native_audit_sha256': 'c' * 64, 'candidate_receipt_sha256': 'd' * 64, 'client_sha256': 'e' * 64, 'server_sha256': 'f' * 64, 'helper_sha256': '0' * 64, 'official_packages': dict(release_tool.APPROVED_PACKAGES), 'harness_sha256': '1' * 64, 'installed_embedding': embedding_binding} for row in self.targets}}
        receipts = {}
        scenario_names = {'prior_fixture', 'corrupt_candidate_rollback', 'upgrade', 'repeat', 'path_deduplication', 'pty', 'uninstall'}
        for row in self.targets:
            evidence_files = {'synthetic.txt': '9' * 64}
            reference = {'path': 'synthetic.txt', 'sha256': '9' * 64}
            installer = {'exit_code': 0, 'stdout': reference, 'stderr': reference}
            failed_installer = {**installer, 'exit_code': 1}
            pair_directory = ('C:\\Users\\runner\\AppData\\Local\\ilium\\versions\\0.1.0\\bin'
                              if row['os'] == 'windows' else '/tmp/install/versions/0.1.0/bin')
            target_binding = deepcopy(embedding_binding)
            separator = '\\' if row['os'] == 'windows' else '/'
            installed_embedding = {**target_binding, 'state': 'passed', 'binary_sha256': 'e' * 64, 'model_sha256': '8' * 64, 'dimension': 384, 'finite_nonzero': True, 'loaded_runtime': '', 'runtime_sha256': None, 'vector_sha256': 'a' * 64, 'executable_path': pair_directory + separator + ('ilium.exe' if row['os'] == 'windows' else 'ilium'), 'process_id': 123, 'stdout': reference, 'stderr': reference}
            if row['os'] == 'windows':
                target_binding['runtime_files'] = {'onnxruntime.dll': '0' * 64}
                installed_embedding['runtime_files'] = target_binding['runtime_files']
                installed_embedding['runtime_sha256'] = '0' * 64
                installed_embedding['loaded_runtime'] = pair_directory + '\\onnxruntime.dll'
            elif row['os'] == 'macos':
                target_binding['runtime_files'] = {'libonnxruntime.1.24.2.dylib': '0' * 64}
                installed_embedding['runtime_files'] = target_binding['runtime_files']
                installed_embedding.update(runtime_sha256='0' * 64, loaded_runtime=pair_directory + '/libonnxruntime.1.24.2.dylib', native_mapping_verified=True, observed_with=['/bin/ps', '/usr/bin/vmmap'], observed_process=reference, native_mappings=reference)
            suffix = '.exe' if row['os'] == 'windows' else ''
            manifest['target_receipts'][row['rust_target']]['installed_embedding'] = target_binding
            pty_tests = [{'name': name, 'command': ['/tmp/harness', name, '--exact', '--nocapture', '--test-threads=1'], 'exit_code': 0, 'stdout': reference, 'stderr': reference} for name in pipeline.PTY_TESTS]
            scenarios = {'prior_fixture': {'state': 'passed', 'kind': 'task-owned-audited-bytes', 'fixture_version': '0.0.0-task7b-prior', 'candidate_version': '0.1.0', 'installer': installer, 'snapshot': reference}, 'corrupt_candidate_rollback': {'state': 'passed', 'installer': failed_installer, 'before': reference, 'after': reference}, 'upgrade': {'state': 'passed', 'installer': installer, 'from': '0.0.0-task7b-prior', 'to': '0.1.0'}, 'repeat': {'state': 'passed', 'installer': installer, 'before': reference, 'after': reference, 'version': '0.1.0'}, 'path_deduplication': {'state': 'passed', 'evidence': reference}, 'pty': {'state': 'passed', 'tests': pty_tests}, 'uninstall': {'state': 'passed', 'installer': installer, 'evidence': reference}}
            receipts[row['rust_target']] = {'schema': 2, 'state': 'passed', 'publication_allowed': True, 'tag': manifest['tag'], 'target': row['rust_target'], 'archive_sha256': manifest['archives'][row['archive']], 'installed_client_sha256': 'e' * 64, 'installed_server_sha256': 'f' * 64, 'installed_helper_sha256': '0' * 64, 'installed_helper_version': release_tool.helper_version_record('0.1.0') + '\n', 'installed_official_packages_sha256': dict(release_tool.APPROVED_PACKAGES), 'installed_pair_sha256': {'ilium' + suffix: 'e' * 64, 'ilium-server' + suffix: 'f' * 64}, 'installed_pair_directory': pair_directory, 'native_identity': {'system': {'linux': 'Linux', 'macos': 'Darwin', 'windows': 'Windows'}[row['os']], 'machine': row['arch'], 'runner': row['runner'], 'future_identity': 'retained'}, 'origin': 'local', 'installer_source_mode': 'exact-functions-with-local-download-adapter' if row['os'] == 'windows' else 'unmodified-installer', 'isolated_state_cleaned': True, 'public_transport_verified': False, 'installer_sha256': manifest['installers']['install.ps1' if row['os'] == 'windows' else 'install.sh'], 'native_test_binary_sha256': '1' * 64, 'pty_tests': pty_tests, 'installed_embedding': installed_embedding, 'installed_animation': synthetic_animation_receipt(row, pair_directory, manifest['source_inputs']), 'scenarios': scenarios, 'evidence_files': evidence_files, 'evidence_files_sha256': pipeline.evidence_files_digest(evidence_files)}
        qualified = pipeline.qualify_receipts(manifest, receipts, self.targets, public=False)
        self.assertEqual(qualified['archives'], manifest['archives'])
        self.assertEqual(qualified['commit'], manifest['commit'])
        for mode in ('missing', 'failed', 'hash', 'pair', 'helper-hash', 'helper-version', 'official-package', 'animation', 'public', 'pty', 'harness', 'installer', 'identity', 'pair-map', 'cleanup', 'source-mode', 'origin', 'prior-schema', 'corrupt-schema', 'upgrade-schema', 'repeat-schema', 'path-schema', 'pty-schema', 'uninstall-schema'):
            changed = deepcopy(receipts)
            first = self.targets[0]['rust_target']
            if mode == 'missing':
                del changed[first]
            elif mode == 'failed':
                changed[first]['state'] = 'failed'
            elif mode == 'hash':
                changed[first]['archive_sha256'] = '0' * 64
            elif mode == 'pair':
                changed[first]['installed_client_sha256'] = '0' * 64
            elif mode == 'helper-hash':
                changed[first]['installed_helper_sha256'] = '1' * 64
            elif mode == 'helper-version':
                changed[first]['installed_helper_version'] = release_tool.helper_version_record('9.9.9') + '\n'
            elif mode == 'official-package':
                changed[first]['installed_official_packages_sha256']['carpet-1.0.0.iliumanim'] = '0' * 64
            elif mode == 'animation':
                changed[first]['installed_animation']['renders'][1]['physical_retirement'] = False
            elif mode == 'pty':
                changed[first]['pty_tests'] = []
            elif mode == 'harness':
                changed[first]['native_test_binary_sha256'] = '0' * 64
            elif mode == 'installer':
                changed[first]['installer_sha256'] = '0' * 64
            elif mode == 'identity':
                changed[first]['native_identity'] = {'system': 'Wrong', 'machine': 'wrong', 'runner': 'wrong'}
            elif mode == 'pair-map':
                changed[first]['installed_pair_sha256'] = {'ilium': '0' * 64}
            elif mode == 'cleanup':
                changed[first]['isolated_state_cleaned'] = False
            elif mode == 'source-mode':
                changed[first]['installer_source_mode'] = 'invented'
            elif mode == 'origin':
                changed[first]['origin'] = 'https://unrelated.example'
            elif mode == 'prior-schema':
                del changed[first]['scenarios']['prior_fixture']['snapshot']
            elif mode == 'corrupt-schema':
                changed[first]['scenarios']['corrupt_candidate_rollback']['installer']['exit_code'] = 0
            elif mode == 'upgrade-schema':
                changed[first]['scenarios']['upgrade']['from'] = 'invented'
            elif mode == 'repeat-schema':
                del changed[first]['scenarios']['repeat']['after']
            elif mode == 'path-schema':
                changed[first]['scenarios']['path_deduplication']['extra'] = True
            elif mode == 'pty-schema':
                changed[first]['scenarios']['pty']['tests'] = []
            elif mode == 'uninstall-schema':
                changed[first]['scenarios']['uninstall']['installer']['exit_code'] = 1
            with self.subTest(mode=mode), self.assertRaises(ValueError):
                pipeline.qualify_receipts(manifest, changed, self.targets, public=(mode == 'public'))

    def test_release_readback_requires_exact_assets_digests_and_immutability(self):
        files = {'ilium-linux-x86_64.tar.gz': 'a' * 64, 'SHA256SUMS': 'b' * 64}
        response = {'id': 3, 'tag_name': 'v0.1.0', 'draft': False, 'immutable': True, 'assets': [{'name': name, 'digest': 'sha256:' + digest, 'size': 10} for name, digest in files.items()]}
        pipeline.validate_release(response, 'v0.1.0', files, draft=False, immutable=True)
        for key in ('digest', 'extra', 'immutable', 'draft'):
            changed = deepcopy(response)
            if key == 'digest':
                changed['assets'][0]['digest'] = 'sha256:' + '0' * 64
            elif key == 'extra':
                changed['assets'].append({'name': 'unexpected', 'digest': 'sha256:' + 'a' * 64, 'size': 10})
            else:
                changed[key] = not changed[key]
            with self.subTest(key=key), self.assertRaises(ValueError):
                pipeline.validate_release(changed, 'v0.1.0', files, draft=False, immutable=True)

    def recovery_fixture(self, previous=False, exposed=False, conflicting=False):
        # A synthetic provider state exercises compensation only; never network.
        files = {'asset.tar.gz': 'a' * 64, 'qualification.json': 'b' * 64}
        assets = [{'name': name, 'digest': 'sha256:' + value, 'size': 10} for name, value in files.items()]
        metadata = {'tag': 'v0.1.0', 'commit': 'a' * 40}
        current = {'id': 3, 'tag_name': 'v0.1.0', 'draft': False, 'immutable': True, 'prerelease': False, 'assets': deepcopy(assets)}
        prior_latest = {'id': 1, 'tag': 'v0.0.9', 'assets': {row['name']: row['digest'] for row in assets}} if previous else None
        prior_production = {'id': 'prior-prod', 'url': 'https://prior.ilium-setup.pages.dev', 'files': {}} if previous else None
        baseline = {'schema': 1, 'state': 'passed', 'tag': metadata['tag'], 'commit': metadata['commit'], 'host': pages.HOST, 'production_branch': 'master', 'previous_latest': prior_latest, 'previous_production': prior_production}
        latest = {'id': 3, 'tag': 'v0.1.0', 'assets': {row['name']: row['digest'] for row in assets}}
        production = {'id': 'candidate-prod', 'environment': 'production', 'deployment_trigger': {'metadata': {'commit_hash': metadata['commit'], 'commit_message': 'ilium-run:123:1:owned'}}} if exposed else ({'id': 'prior-prod'} if previous else None)
        if conflicting:
            production = {'id': 'another-owner', 'deployment_trigger': {'metadata': {'commit_hash': 'f' * 40, 'commit_message': 'other-owner'}}}
        state = {'latest': latest, 'production': production, 'candidate': current, 'writes': [], 'withdrawals': 0}
        prior_release = {'id': 1, 'tag_name': 'v0.0.9', 'immutable': True, 'draft': False, 'prerelease': False, 'assets': deepcopy(assets)}
        def github(path, method='GET', payload=None, **_kwargs):
            if method == 'PATCH':
                state['writes'].append(('github', path, payload))
                if path == 'releases/1':
                    state['latest'] = prior_latest
                else:
                    state['candidate']['prerelease'] = True
                    if state['latest'] and state['latest']['id'] == 3:
                        state['latest'] = None
                return deepcopy(state['candidate'])
            return deepcopy(prior_release if path == 'releases/1' else state['candidate'])
        def cloudflare(path, method='GET'):
            self.assertEqual(method, 'POST')
            state['writes'].append(('cloudflare', path, None))
            state['production'] = {'id': 'prior-prod'}
            return state['production']
        def upload(*_arguments):
            state['withdrawals'] += 1
            state['production'] = {'id': 'withdrawn-prod', 'environment': 'production'}
            return state['production']
        def verify_withdrawal(_directory, origin):
            if origin == 'https://' + pages.HOST and state['production'].get('id') != 'withdrawn-prod':
                raise ValueError('not withdrawn yet')
            return {}
        publication = self.root / 'published.json'; publication.write_text(json.dumps({'release_id': 3, **metadata, 'assets': files}))
        ready = self.root / 'ready'; ready.mkdir()
        (ready / 'recovery-ready.json').write_text(json.dumps({'schema': 1, 'state': 'passed', **metadata, 'withdrawal': {'url': 'https://recovery.ilium-setup.pages.dev'}}))
        arguments = SimpleNamespace(candidate=self.root, manifest=ROOT / 'release/targets.toml', workspace=ROOT / 'Cargo.toml', baseline=self.root, publication_receipt=publication, recovery_ready=ready, output=self.root / 'recovery.json', log=self.root / 'withdrawal.log')
        patches = [patch.object(pipeline, 'candidate_data', return_value=(metadata, self.targets)), patch.object(pipeline, 'load_baseline', return_value=baseline), patch.object(pipeline, 'asset_hashes', return_value=files), patch.object(pipeline, 'publication_files', return_value=[]), patch.object(pipeline, 'github', side_effect=github), patch.object(pipeline, 'latest_identity', side_effect=lambda: state['latest']), patch.object(pipeline, 'pages_project', side_effect=lambda: ('accounts/fixture/pages/projects/ilium-setup', {'canonical_deployment': state['production']})), patch.object(pipeline, 'cloudflare', side_effect=cloudflare), patch.object(pipeline, 'verify_baseline_bytes'), patch.object(pipeline, 'verify_withdrawal', side_effect=verify_withdrawal), patch.object(pipeline, 'upload_pages', side_effect=upload), patch.object(pipeline, 'emit'), patch.dict('os.environ', {'GITHUB_RUN_ID': '123'})]
        return arguments, state, patches

    def test_first_release_unexposed_quarantines_without_pages_write(self):
        from contextlib import ExitStack
        arguments, state, patches = self.recovery_fixture()
        with ExitStack() as stack:
            for item in patches:
                stack.enter_context(item)
            pipeline.recover(arguments)
        self.assertIsNone(state['latest'])
        self.assertEqual(state['withdrawals'], 0)
        self.assertEqual(state['writes'], [('github', 'releases/3', {'prerelease': True, 'make_latest': 'false'})])
        self.assertEqual(json.loads(arguments.output.read_text())['state'], 'restored')

    def test_subsequent_recovery_restores_saved_latest_and_production(self):
        from contextlib import ExitStack
        arguments, state, patches = self.recovery_fixture(previous=True, exposed=True)
        with ExitStack() as stack:
            for item in patches:
                stack.enter_context(item)
            pipeline.recover(arguments)
        self.assertEqual(state['latest']['id'], 1)
        self.assertEqual(state['production']['id'], 'prior-prod')
        self.assertEqual(state['writes'][0], ('github', 'releases/1', {'make_latest': 'true'}))
        self.assertEqual(state['writes'][1], ('github', 'releases/3', {'prerelease': True, 'make_latest': 'false'}))
        self.assertTrue(state['writes'][2][1].endswith('/deployments/prior-prod/rollback'))

    def test_uncertain_recovery_write_uses_readback_without_retry(self):
        state = {'id': 1, 'writes': 0}
        arguments = SimpleNamespace(output=self.root / 'result.json')
        def invoke():
            state.update(id=2, writes=state['writes'] + 1)
            raise OSError('synthetic lost response after write applied')
        actual = pipeline.recovery_write(arguments, 'synthetic-owned-operation', invoke, lambda: dict(state), lambda value: value['id'] == 2)
        self.assertEqual(actual['writes'], 1)
        readbacks = list(self.root.glob('*.readback.json'))
        self.assertEqual(len(readbacks), 1)
        self.assertTrue(json.loads(readbacks[0].read_text())['reconciled_uncertain_response'])

    def test_script_delivery_fault_restores_pages_before_latest(self):
        from contextlib import ExitStack
        arguments, state, patches = self.recovery_fixture(previous=True, exposed=True)
        arguments.fault = 'script-delivery'
        with ExitStack() as stack:
            for item in patches:
                stack.enter_context(item)
            pipeline.recover(arguments)
        self.assertEqual(state['writes'][0][0], 'cloudflare')
        self.assertEqual(state['writes'][1][0], 'github')
        self.assertEqual(json.loads(arguments.output.read_text())['order'], ['pages', 'latest'])

    def test_first_exposed_recovery_promotes_preverified_withdrawal(self):
        from contextlib import ExitStack
        arguments, state, patches = self.recovery_fixture(exposed=True)
        with ExitStack() as stack:
            for item in patches:
                stack.enter_context(item)
            pipeline.recover(arguments)
        self.assertEqual(state['withdrawals'], 1)
        self.assertEqual(state['production']['id'], 'withdrawn-prod')
        self.assertEqual(json.loads(arguments.output.read_text())['pages']['state'], 'withdrawn')

    def test_other_owner_conflict_prevents_both_channel_writes(self):
        from contextlib import ExitStack
        arguments, state, patches = self.recovery_fixture(exposed=True, conflicting=True)
        with ExitStack() as stack:
            for item in patches:
                stack.enter_context(item)
            with self.assertRaisesRegex(ValueError, 'conflict'):
                pipeline.recover(arguments)
        self.assertEqual(state['writes'], [])
        self.assertEqual(state['withdrawals'], 0)

    def test_https_redirect_rejects_cross_origin_authorization(self):
        import urllib.request
        request = urllib.request.Request('https://api.github.com/repos/x/y', headers={'Authorization': 'Bearer synthetic-fixture'})
        with self.assertRaises(ValueError):
            pipeline.HTTPSOnly().redirect_request(request, None, 302, 'redirect', {}, 'https://unrelated.example/path')
        with self.assertRaises(ValueError):
            pipeline.HTTPSOnly().redirect_request(request, None, 302, 'redirect', {}, 'http://api.github.com/path')

    def test_file_hashes_reject_extra_symlink_duplicate_assets(self):
        a = self.root / 'a'; a.write_bytes(b'a')
        b = self.root / 'b'; b.symlink_to(a)
        with self.assertRaises(ValueError):
            pipeline.asset_hashes([a, b])
        with self.assertRaises(ValueError):
            pipeline.asset_hashes([a, a])


if __name__ == '__main__':
    unittest.main()
