"""Portable negative tests for native animation receipt and launcher binding."""
import argparse
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import release_tool
import smoke_installed_animation as gate


class InstalledAnimationGateTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.source = self.root / 'source'
        self.source.mkdir()
        (self.source / 'Cargo.toml').write_text('[workspace]\n', encoding='utf-8')
        for name in gate.SOURCE_FILES:
            path = self.source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(('synthetic source ' + name).encode())
        self.installed = self.root / 'installed'
        self.installed.mkdir()
        self.target = release_tool.selected_target(ROOT / 'release/targets.toml',
                                                   'x86_64-unknown-linux-gnu')
        content = {'ilium': b'client', 'ilium-server': b'server',
                   'ilium-animation-helper': b'helper', 'VERSION': b'0.1.0\n',
                   'THIRD-PARTY.txt': b'reviewed notices'}
        for name in release_tool.APPROVED_PACKAGES:
            content[name] = (ROOT / 'ilium-animation-js/assets/packages' / name).read_bytes()
        self.files = {name: release_tool.digest(data) for name, data in content.items()}
        for name, data in content.items():
            (self.installed / name).write_bytes(data)
        audit = {'schema': 1, 'state': 'passed', 'publication_allowed': True,
                 'target': self.target['rust_target'], 'tag': 'v0.1.0', 'version': '0.1.0',
                 'os': 'linux', 'arch': 'x86_64', 'files': self.files,
                 'native_identity': {'system': 'Linux', 'machine': 'x86_64', 'runner': 'synthetic-only'},
                 'dependency_closure': {'complete': True},
                 'binary_versions': {name: name + ' 0.1.0' for name in self.target['executables']},
                 'notices': {'state': 'reviewed', 'sha256': self.files['THIRD-PARTY.txt']}}
        self.audit = self.root / 'audit.json'
        self.audit.write_text(json.dumps(audit), encoding='utf-8')
        self.command = [str(self.installed / 'ilium'), 'release-animation-probe']
        self.arguments = argparse.Namespace(workspace=self.source / 'Cargo.toml',
                                             root=self.installed,
                                             executable_root=None,
                                             manifest=ROOT / 'release/targets.toml',
                                             audit=self.audit,
                                             output=self.root / 'animation.json',
                                             os='linux', arch='x86_64', tag='v0.1.0',
                                             format='archive')

    def output(self):
        catalogue = {'type': 'artifact', 'gate': 'installed_catalogue',
                     'client_path': str(self.installed / 'ilium'),
                     'client_sha256': self.files['ilium'],
                     'helper_path': str(self.installed / 'ilium-animation-helper'),
                     'helper_sha256': self.files['ilium-animation-helper'],
                     'packages': ['beach', 'carpet'],
                     'worker_threads_before': 1, 'worker_bytes_before': 1024}
        renders = []
        for filename, digest in release_tool.APPROVED_PACKAGES.items():
            renders.append({'type': 'artifact', 'gate': 'installed_render',
                            'package': filename.split('-')[0],
                            'archive_sha256': digest,
                            'helper_sha256': self.files['ilium-animation-helper'],
                            'rendered_frames': 2, 'physical_retirement': True,
                            'worker_threads_before': 1, 'worker_threads_after': 1,
                            'worker_bytes_before': 1024, 'worker_bytes_after': 1024})
        result = {'type': 'result', 'gate': 'installed_animation', 'state': 'passed',
                  'publication_allowed': False, 'packages': ['beach', 'carpet']}
        return ''.join(json.dumps(row, sort_keys=True) + '\n'
                       for row in [catalogue, *renders, result])

    def run_synthetic_probe(self, output=None):
        completed = argparse.Namespace(returncode=0, stdout=self.output() if output is None else output,
                                       stderr='')
        with (patch.object(gate.subprocess, 'run', return_value=completed) as run,
              patch.object(gate.platform, 'system', return_value='Linux'),
              patch.object(gate.platform, 'machine', return_value='x86_64'),
              patch.object(gate.release_tool, 'emit')):
            receipt = gate.smoke(self.arguments)
        self.assertEqual(run.call_args.args[0], self.command)
        self.assertEqual(run.call_args.kwargs['timeout'], 300)
        return receipt

    def test_installed_command_and_both_render_receipts_are_bound(self):
        receipt = self.run_synthetic_probe()
        self.assertFalse(receipt['publication_allowed'])
        self.assertEqual(receipt['launcher_command'], self.command)
        self.assertEqual(receipt['installed_files']['beach-1.0.0.iliumanim'],
                         release_tool.APPROVED_PACKAGES['beach-1.0.0.iliumanim'])
        self.assertEqual(json.loads(self.arguments.output.read_text()), receipt)
        gate.validate_receipt(receipt, target=self.target, tag='v0.1.0',
                              package_format='archive', audit_path=self.audit,
                              installed_root=self.installed,
                              executable_root=self.installed,
                              command=self.command, source_root=self.source)

    def test_missing_frame_wrong_hash_or_unretired_worker_rejects_probe(self):
        rows = [json.loads(line) for line in self.output().splitlines()]
        for mutation in ('missing-carpet', 'archive-hash', 'helper-hash', 'worker-leak',
                         'not-accepted', 'extra-output'):
            changed = json.loads(json.dumps(rows))
            if mutation == 'missing-carpet':
                changed.pop(2)
            elif mutation == 'archive-hash':
                changed[1]['archive_sha256'] = '0' * 64
            elif mutation == 'helper-hash':
                changed[2]['helper_sha256'] = '0' * 64
            elif mutation == 'worker-leak':
                changed[1]['worker_threads_after'] = 2
            elif mutation == 'not-accepted':
                changed[3]['state'] = 'failed'
            else:
                changed.append(changed[3])
            with self.subTest(mutation=mutation), self.assertRaises(release_tool.ReleaseError):
                gate.parse_probe_output(''.join(json.dumps(row) + '\n' for row in changed))

    def test_receipt_tamper_and_missing_managed_launcher_fail_closed(self):
        receipt = self.run_synthetic_probe()
        for field, replacement in (('native_audit_sha256', '0' * 64),
                                   ('installed_root', '/foreign'),
                                   ('launcher_command', ['ilium', 'release-animation-probe']),
                                   ('stdout_sha256', '0' * 64),
                                   ('source_files', {}),
                                   ('installed_files', {})):
            changed = dict(receipt, **{field: replacement})
            with self.subTest(field=field), self.assertRaises(release_tool.ReleaseError):
                gate.validate_receipt(changed, target=self.target, tag='v0.1.0',
                                      package_format='archive', audit_path=self.audit,
                                      installed_root=self.installed,
                                      executable_root=self.installed,
                                      command=self.command, source_root=self.source)
        self.arguments.format = 'flatpak'
        self.arguments.output = self.root / 'managed.json'
        with self.assertRaisesRegex(release_tool.ReleaseError, 'real installed launcher'):
            gate.smoke(self.arguments)

    def test_managed_launcher_binds_host_payload_to_sandbox_executable_path(self):
        self.arguments.format = 'flatpak'
        self.arguments.executable_root = Path('/app/lib/ilium')
        self.arguments.output = self.root / 'flatpak-animation.json'
        launcher = [str(self.root / 'flatpak-client.sh'), 'release-animation-probe']
        output = self.output().replace(str(self.installed / 'ilium-animation-helper'),
                                       '/app/lib/ilium/ilium-animation-helper').replace(
                                           str(self.installed / 'ilium'), '/app/lib/ilium/ilium')
        completed = argparse.Namespace(returncode=0, stdout=output, stderr='')
        with (patch.object(gate.subprocess, 'run', return_value=completed) as run,
              patch.object(gate.platform, 'system', return_value='Linux'),
              patch.object(gate.platform, 'machine', return_value='x86_64'),
              patch.object(gate.release_tool, 'emit')):
            receipt = gate.smoke(self.arguments, command=launcher)
        self.assertEqual(run.call_args.args[0], launcher)
        self.assertEqual(receipt['installed_root'], str(self.installed))
        self.assertEqual(receipt['executable_root'], '/app/lib/ilium')
        gate.validate_receipt(receipt, target=self.target, tag='v0.1.0',
                              package_format='flatpak', audit_path=self.audit,
                              installed_root=self.installed,
                              executable_root=Path('/app/lib/ilium'),
                              command=launcher, source_root=self.source)


if __name__ == '__main__':
    unittest.main()
