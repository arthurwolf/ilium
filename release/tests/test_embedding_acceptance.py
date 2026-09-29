"""Synthetic process fixtures test custody/transport, never real embedding evidence."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

WRAPPER = Path(__file__).with_name('embedding_acceptance.py')
FILES = ('model.onnx', 'tokenizer.json', 'config.json', 'special_tokens_map.json', 'tokenizer_config.json')


@unittest.skipIf(sys.platform == 'win32', 'POSIX synthetic executable fixtures; real Windows probe belongs to native acceptance')
class EmbeddingAcceptanceTests(unittest.TestCase):
    def fixture(self, directory, mode='valid'):
        installed = directory / 'installed'
        installed.mkdir()
        model = directory / 'model'
        model.mkdir()
        hashes = {}
        for name in FILES:
            data = ('synthetic fixture ' + name).encode()
            (model / name).write_bytes(data)
            hashes[name] = hashlib.sha256(data).hexdigest()
        lock = directory / 'model-lock.json'
        lock.write_text(json.dumps({'schema': 1, 'reviewed': True, 'dimension': 384, 'files': hashes}))
        (installed / 'libonnxruntime.1.24.2.dylib').write_bytes(b'synthetic runtime fixture')
        child = installed / 'ilium'
        child.write_text('#!' + sys.executable + '\n' + '''import argparse,json,os,sys
from pathlib import Path
p=argparse.ArgumentParser(); p.add_argument('command'); p.add_argument('--model-directory'); p.add_argument('--text'); p.add_argument('--hold-for-native-audit',action='store_true'); a=p.parse_args()
mode=MODE
if mode=='failure': sys.exit(7)
if mode=='wait-before-proof': sys.stdin.read(); sys.exit(0)
record={'type':'release-embedding-vector','ilium_pid':os.getpid(),'executable_path':str(Path(__file__).resolve()),'input':a.text,'embedding':[0.25]*384}
if mode=='bad-pid': record['ilium_pid']=1
if mode=='bad-vector': record['embedding']=[float('nan')]
if mode=='wrong-input': record['input']='different'
if mode=='malformed': print('{',flush=True)
else: print(json.dumps(record),flush=True)
if mode=='duplicate': print(json.dumps(record),flush=True)
if mode=='early-exit': sys.exit(0)
if a.hold_for_native_audit:
 line=sys.stdin.readline()
 Path(__file__).with_name('closed').write_text(line)
 if line and line!='native-audit-observed\\n': sys.exit(9)
'''.replace('MODE', repr(mode)))
        child.chmod(0o755)
        command = [sys.executable, str(WRAPPER), '--installed-directory', str(installed), '--model', str(model / 'model.onnx'), '--model-lock', str(lock), '--text', 'test input', '--hold-for-native-audit']
        return command, installed, model, lock

    def test_held_receipt_and_acknowledgement(self):
        with tempfile.TemporaryDirectory() as temporary:
            command, installed, _, _ = self.fixture(Path(temporary))
            process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            try:
                proof = json.loads(process.stdout.readline())
                self.assertEqual(proof['type'], 'embedding-proof')
                self.assertEqual(proof['input'], 'test input')
                self.assertEqual(proof['binary_sha256'], hashlib.sha256((installed / 'ilium').read_bytes()).hexdigest())
                self.assertIsNone(process.poll())
                remaining, errors = process.communicate('native-audit-observed\n', timeout=10)
                self.assertEqual(process.returncode, 0, errors)
                self.assertEqual(remaining, '')
                self.assertEqual((installed / 'closed').read_text(), 'native-audit-observed\n')
            finally:
                if process.poll() is None:
                    process.kill()
                process.communicate()

    def test_closed_auditor_stdin_releases_child(self):
        with tempfile.TemporaryDirectory() as temporary:
            command, installed, _, _ = self.fixture(Path(temporary))
            result = subprocess.run(command, input='', capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((installed / 'closed').read_text(), '')

    def test_invalid_transport_and_early_child_exit_fail(self):
        for mode in ('malformed', 'bad-pid', 'bad-vector', 'wrong-input', 'failure', 'duplicate'):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                command, _, _, _ = self.fixture(Path(temporary), mode)
                result = subprocess.run(command, input='native-audit-observed\n', capture_output=True, text=True, timeout=10)
                self.assertNotEqual(result.returncode, 0, result.stdout)

    def test_bounded_timeout_releases_owned_child_before_proof(self):
        with tempfile.TemporaryDirectory() as temporary:
            command, _, _, _ = self.fixture(Path(temporary), 'wait-before-proof')
            result = subprocess.run(command + ['--timeout-seconds', '0.1'], input='', capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('timed out', result.stdout)

    def test_nonheld_probe_exits_after_single_proof(self):
        with tempfile.TemporaryDirectory() as temporary:
            command, _, _, _ = self.fixture(Path(temporary))
            command.remove('--hold-for-native-audit')
            result = subprocess.run(command, input='', capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(len(result.stdout.splitlines()), 1)
            self.assertEqual(json.loads(result.stdout)['type'], 'embedding-proof')

    def test_child_exit_before_audit_acknowledgement_fails(self):
        with tempfile.TemporaryDirectory() as temporary:
            command, _, _, _ = self.fixture(Path(temporary), 'early-exit')
            process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            try:
                process.wait(timeout=10)
                output, _ = process.communicate()
                self.assertNotEqual(process.returncode, 0, output)
            finally:
                if process.poll() is None:
                    process.kill()
                    process.communicate()

    def test_wrong_acknowledgement_fails_and_reaps_child(self):
        with tempfile.TemporaryDirectory() as temporary:
            command, _, _, _ = self.fixture(Path(temporary))
            result = subprocess.run(command, input='wrong\n', capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)

    def test_changed_model_and_ambiguous_runtime_fail_before_spawn(self):
        for mode in ('changed-model', 'unreviewed', 'extra-runtime', 'symlink-model'):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                command, installed, model, lock = self.fixture(Path(temporary))
                if mode == 'changed-model':
                    (model / 'model.onnx').write_bytes(b'changed')
                elif mode == 'unreviewed':
                    data = json.loads(lock.read_text()); data['reviewed'] = False; lock.write_text(json.dumps(data))
                elif mode == 'extra-runtime':
                    (installed / 'libonnxruntime.1.23.0.dylib').write_bytes(b'other')
                else:
                    source = model / 'source'; (model / 'model.onnx').rename(source); (model / 'model.onnx').symlink_to(source)
                result = subprocess.run(command, input='', capture_output=True, text=True, timeout=10)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse((installed / 'closed').exists())


if __name__ == '__main__':
    unittest.main()
