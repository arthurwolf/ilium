"""Release builds must bound both Cargo and native child parallelism."""
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import release_pipeline as pipeline


class NativeBuildConcurrencyTests(unittest.TestCase):
    def test_host_parallelism_cannot_override_release_limits(self):
        for operating_system in ('linux', 'macos', 'windows'):
            with self.subTest(operating_system=operating_system), tempfile.TemporaryDirectory() as temporary:
                work = Path(temporary)
                environment = {name: '128' for name in (
                    'CARGO_BUILD_JOBS', 'CMAKE_BUILD_PARALLEL_LEVEL', 'NUM_JOBS',
                    'MAKEFLAGS', 'OMP_NUM_THREADS', 'RAYON_NUM_THREADS')}
                target = {'os': operating_system, 'rust_target': 'synthetic-target'}
                configured = pipeline.configure_workspace_test_environment(
                    environment, target, work, work / 'target')
                self.assertEqual(configured['CARGO_BUILD_JOBS'], '16')
                for name in ('CMAKE_BUILD_PARALLEL_LEVEL', 'NUM_JOBS',
                             'OMP_NUM_THREADS', 'RAYON_NUM_THREADS'):
                    self.assertEqual(configured[name], '1', name)
                self.assertEqual(configured['MAKEFLAGS'], '-j1')
                self.assertEqual(configured['RUST_TEST_THREADS'], '1')


if __name__ == '__main__':
    unittest.main()
