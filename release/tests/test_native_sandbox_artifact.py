"""Retained native sandbox executables must match one real Cargo test artifact."""
from pathlib import Path
import hashlib
import json
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import release_pipeline as pipeline


class NativeSandboxArtifactTests(unittest.TestCase):
    def test_retains_unique_test_binary_and_checksum(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / 'compiled-test'
            binary.write_bytes(b'synthetic artifact custody fixture')
            log = root / 'cargo.jsonl'
            log.write_text(json.dumps({'reason': 'compiler-artifact', 'target': {
                'name': 'native_animation_sandbox', 'kind': ['test']},
                'executable': str(binary)}) + '\n')
            output = root / 'retained'
            receipt = pipeline.retain_native_sandbox_artifact(log, output)
            self.assertEqual((output / receipt['filename']).read_bytes(), binary.read_bytes())
            self.assertEqual(receipt['sha256'], hashlib.sha256(binary.read_bytes()).hexdigest())
            self.assertFalse(receipt['executed'])

    def test_rejects_missing_duplicate_or_symlink_artifacts(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / 'binary'
            binary.write_bytes(b'synthetic artifact')
            link = root / 'link'
            link.symlink_to(binary)
            log = root / 'cargo.jsonl'
            def record(path):
                return json.dumps({'reason': 'compiler-artifact', 'target': {
                    'name': 'native_animation_sandbox', 'kind': ['test']},
                    'executable': str(path)}) + '\n'
            for name, text in [('missing', ''), ('duplicate', record(binary) * 2),
                               ('symlink', record(link))]:
                with self.subTest(case=name):
                    log.write_text(text)
                    with self.assertRaises(pipeline.release_tool.ReleaseError):
                        pipeline.retain_native_sandbox_artifact(log, root / name)


if __name__ == '__main__':
    unittest.main()
