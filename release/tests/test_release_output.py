"""Release metadata has identical bytes on every build host."""
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
import release_pipeline as pipeline
import native_candidate


class ReleaseOutputTests(unittest.TestCase):
    def test_ascii_metadata_uses_exact_lf_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'VERSION'
            pipeline.write_release_text(output, '0.1.1\n')
            self.assertEqual(output.read_bytes(), b'0.1.1\n')
            native_candidate.write_release_text(output, 'abc  package.zip\n')
            self.assertEqual(output.read_bytes(), b'abc  package.zip\n')


if __name__ == '__main__':
    unittest.main()
