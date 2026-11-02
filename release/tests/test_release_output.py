"""Release metadata has identical bytes on every build host."""
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
import release_pipeline as pipeline


class ReleaseOutputTests(unittest.TestCase):
    def test_ascii_metadata_uses_exact_lf_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'VERSION'
            pipeline.write_release_text(output, '0.1.1\n')
            self.assertEqual(output.read_bytes(), b'0.1.1\n')


if __name__ == '__main__':
    unittest.main()
