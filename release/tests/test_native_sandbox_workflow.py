"""Require native execution receipts before Linux release package sealing."""
from pathlib import Path
import sys
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import validate_animation_smoke as smoke


class NativeSandboxWorkflowTests(unittest.TestCase):
    def test_candidate_provenance_binds_runner_and_native_test_source(self):
        for name in ('release/scripts/native_sandbox_runner.py',
                     'ilium-platform/tests/native_animation_sandbox.rs'):
            with self.subTest(name=name):
                self.assertIn(name, smoke.SOURCE_FILES,
                              'Candidate/seal source checks must reject changed sandbox qualification inputs')

    def test_linux_architectures_execute_sandbox_cases_before_sealing(self):
        workflow = yaml.safe_load((ROOT / '.github/workflows/release.yml').read_text())
        jobs = workflow['jobs']
        job = next(value for value in jobs.values()
                   if any('Seal native Linux animation evidence' == step.get('name')
                          for step in value.get('steps', [])))
        steps = job['steps']
        executions = [(index, step) for index, step in enumerate(steps)
                      if 'native_sandbox_runner.py' in step.get('run', '')]
        self.assertEqual(len(executions), 1, 'Both Linux architectures need the four-case execution gate')
        index, execution = executions[0]
        self.assertNotIn('if', execution, 'The sandbox gate must cover both Linux architectures')
        command = execution['run']
        self.assertIn('--artifact-directory native-linux/evidence', command)
        self.assertIn('--helper native-linux/candidate/ilium-animation-helper', command)
        seal_index, seal = next((index, step) for index, step in enumerate(steps)
                                if step.get('name') == 'Seal native Linux animation evidence')
        self.assertLess(index, seal_index)
        self.assertIn('--native-sandbox-receipt', seal['run'],
                      'A passing package smoke must consume the executed sandbox receipt')
        vm = next(step for step in steps if 'vm_smoke.py' in step.get('run', ''))
        self.assertIn('--native-directory native-linux', vm['run'],
                      'The minimum-system VM needs matching sandbox and helper/runtime artifacts')


if __name__ == '__main__':
    unittest.main()
