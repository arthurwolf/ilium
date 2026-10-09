"""Registry publication must follow public native-release acceptance."""
from pathlib import Path
import unittest

import yaml


ROOT = Path(__file__).resolve().parents[2]


class RegistryWorkflowTests(unittest.TestCase):
    def test_registry_inputs_have_unique_dependency_ordered_producers(self):
        workflow = yaml.safe_load((ROOT / '.github/workflows/release.yml').read_text())
        jobs = workflow['jobs']
        ancestors = set()

        def visit(name, stack):
            self.assertNotIn(name, stack, 'Registry dependency cycle')
            needs = jobs[name].get('needs', [])
            if isinstance(needs, str):
                needs = [needs]
            for dependency in needs:
                self.assertIn(dependency, jobs)
                if dependency not in ancestors:
                    visit(dependency, stack | {name})
                    ancestors.add(dependency)

        visit('ghcr-package', set())
        required = {'qualified-candidate', 'final-qualification',
                    'publication-receipt', 'completed-public-qualification',
                    'final-channel-readback'}
        downloads = [step['with']['name']
                     for step in jobs['ghcr-package']['steps']
                     if step.get('uses', '').startswith('actions/download-artifact@')]
        self.assertEqual(set(downloads), required)
        self.assertEqual(len(downloads), len(required), 'Duplicate registry input')
        for artifact in required:
            producers = [name for name, job in jobs.items()
                         for step in job.get('steps', [])
                         if step.get('uses', '').startswith('actions/upload-artifact@')
                         and step.get('with', {}).get('name') == artifact]
            self.assertEqual(len(producers), 1,
                             f'{artifact} must have exactly one producer')
            self.assertIn(producers[0], ancestors,
                          f'Registry can start before {artifact} exists')

    def test_package_write_job_requires_completed_public_release(self):
        workflow = yaml.safe_load((ROOT / '.github/workflows/release.yml').read_text())
        publishers = [(name, job) for name, job in workflow['jobs'].items()
                      if job.get('permissions', {}).get('packages') == 'write']
        self.assertEqual(len(publishers), 1,
                         'A real GitHub Packages publisher with scoped write permission is required')
        name, job = publishers[0]
        self.assertEqual(job.get('if'), "github.event_name == 'push'",
                         'Manual workflow dispatch must never publish registry packages')
        self.assertIn('complete', job.get('needs', []),
                      'Registry publication must consume completed public release acceptance')
        self.assertNotEqual(workflow.get('permissions', {}).get('packages'), 'write',
                            'Package write permission belongs only to the publisher job')
        commands = '\n'.join(step.get('run', '') for step in job.get('steps', []))
        self.assertIn('publish', commands,
                      f'{name} must invoke the registry publisher, not only upload Actions artifacts')
        retained = [step for step in job.get('steps', [])
                    if step.get('uses', '').startswith('actions/upload-artifact@')]
        self.assertTrue(retained, 'Registry publication/readback evidence must be retained')


if __name__ == '__main__':
    unittest.main()
