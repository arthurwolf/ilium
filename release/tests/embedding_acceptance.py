#!/usr/bin/env python3
"""Run real inference in installed Ilium; hold that same child for native auditing.

The runtime path in this receipt is the shipped candidate. audit_native.py must
independently prove that this PID maps it; this wrapper cannot certify mapping.
All model bytes must match the separately reviewed local model register.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import queue
import re
import subprocess
import sys
import tempfile
import threading
import time

FILES = ('model.onnx', 'tokenizer.json', 'config.json', 'special_tokens_map.json', 'tokenizer_config.json')
MAX_FILE = 134_217_728
MAX_OUTPUT = 1_000_000
ACK = 'native-audit-observed\n'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(path):
    result = hashlib.sha256()
    with path.open('rb') as source:
        while block := source.read(65536):
            result.update(block)
    return result.hexdigest()


def regular(path):
    require(path.is_file() and not path.is_symlink() and 0 < path.stat().st_size <= MAX_FILE,
            'Expected bounded plain file: ' + str(path))


def validate_model(model, lock_path):
    require(model.name == 'model.onnx', 'Model argument must name model.onnx with four tokenizer siblings')
    require(not model.parent.is_symlink(), 'Model directory must not be a symlink')
    register = json.loads(lock_path.read_text(encoding='utf-8'))
    require(register.get('schema') == 1 and register.get('reviewed') is True and register.get('dimension') == 384,
            'Embedding model register must be reviewed and specify the actual 384-dimensional MiniLM model')
    hashes = register.get('files')
    require(isinstance(hashes, dict) and set(hashes) == set(FILES), 'Register must hash all five model files')
    for name in FILES:
        require(isinstance(hashes[name], str) and re.fullmatch('[0-9a-f]{64}', hashes[name]), 'Invalid registered model hash')
        path = model.parent / name
        regular(path)
        require(sha(path) == hashes[name], 'Model checksum mismatch: ' + name)
    return hashes


def shipped_runtime(directory):
    matches = [path for path in directory.iterdir() if re.fullmatch(
        r'libonnxruntime\.[0-9]+\.[0-9]+\.[0-9]+\.dylib|libonnxruntime\.so(?:\.[0-9]+)*|onnxruntime\.dll', path.name)]
    # Linux may link ONNX statically. macOS qualification always requires a
    # single versioned shipped dylib and subsequently checks the native map.
    if not matches and sys.platform != 'darwin':
        return ''
    require(len(matches) == 1, 'Expected exactly one shipped ONNX runtime')
    runtime = matches[0]
    regular(runtime)
    if sys.platform == 'darwin':
        require(re.fullmatch(r'libonnxruntime\.[0-9]+\.[0-9]+\.[0-9]+\.dylib', runtime.name), 'macOS runtime must be versioned')
    return str(runtime)


def child_records(stream, events):
    total = 0
    try:
        while line := stream.readline(MAX_OUTPUT + 1):
            total += len(line)
            require(total <= MAX_OUTPUT and line.endswith(b'\n'), 'Child output exceeds bound or has incomplete JSONL')
            events.put(('record', json.loads(line)))
        events.put(('child-eof', None))
    except (ValueError, OSError) as error:
        events.put(('error', str(error)))


def acknowledgement(events):
    try:
        events.put(('ack', sys.stdin.readline(128)))
    except OSError as error:
        events.put(('error', str(error)))


def next_event(events, deadline):
    remaining = deadline - time.monotonic()
    require(remaining > 0, 'Embedding child timed out')
    try:
        kind, value = events.get(timeout=remaining)
    except queue.Empty as error:
        raise ValueError('Embedding child timed out') from error
    require(kind != 'error', 'Embedding child transport failed: ' + str(value))
    return kind, value


def prove(arguments):
    directory = arguments.installed_directory
    require(directory.is_absolute() and directory.is_dir() and not directory.is_symlink(), 'Installed directory must be a plain absolute directory')
    directory = directory.resolve()
    model = arguments.model
    require(model.is_absolute(), 'Model must be an absolute path')
    hashes = validate_model(model, arguments.model_lock)
    client = directory / ('ilium.exe' if sys.platform == 'win32' else 'ilium')
    # Executables are much larger than model tokenizers; use no model size cap.
    require(client.is_file() and not client.is_symlink(), 'Installed Ilium must be a plain executable')
    before = sha(client)
    runtime = shipped_runtime(directory)
    environment = dict(os.environ)
    for name in ('DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH', 'LD_LIBRARY_PATH', 'LD_PRELOAD', 'ORT_DYLIB_PATH'):
        environment.pop(name, None)
    environment.update(HF_HUB_OFFLINE='1', HF_ENDPOINT='http://127.0.0.1:9', NO_PROXY='*')
    command = [str(client), 'release-embedding-probe', '--model-directory', str(model.parent.resolve()), '--text', arguments.text]
    if arguments.hold_for_native_audit:
        command.append('--hold-for-native-audit')
    events = queue.Queue()
    deadline = time.monotonic() + arguments.timeout_seconds
    with tempfile.TemporaryFile() as errors:
        child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors, env=environment)
        reader = threading.Thread(target=child_records, args=(child.stdout, events), daemon=True)
        reader.start()
        try:
            kind, vector = next_event(events, deadline)
            require(kind == 'record' and isinstance(vector, dict) and vector.get('type') == 'release-embedding-vector', 'Child did not emit its single inference vector')
            require(type(vector.get('ilium_pid')) is int and vector['ilium_pid'] == child.pid, 'Inference receipt PID differs from owned installed child')
            require(vector.get('executable_path') == str(client), 'Inference receipt executable differs from installed child')
            require(vector.get('input') == arguments.text, 'Inference input differs from invoked text')
            values = vector.get('embedding')
            require(isinstance(values, list) and len(values) == 384 and all(type(value) in (int, float) and math.isfinite(value) for value in values) and any(value != 0 for value in values), 'Inference must return a finite nonzero 384-dimensional vector')
            require(sha(client) == before, 'Installed client changed during inference')
            require(validate_model(model, arguments.model_lock) == hashes, 'Model changed during inference')
            require(not arguments.hold_for_native_audit or child.poll() is None, 'Inference child exited before native audit')
            proof = dict(vector, type='embedding-proof', binary_sha256=before, model_sha256=hashes['model.onnx'], loaded_runtime=runtime)
            print(json.dumps(proof, allow_nan=False), flush=True)
            if arguments.hold_for_native_audit:
                threading.Thread(target=acknowledgement, args=(events,), daemon=True).start()
                kind, value = next_event(events, deadline)
                require(kind == 'ack', 'Inference child exited or emitted extra output before native audit')
                require(value in ('', ACK), 'Invalid native audit acknowledgement')
                if value:
                    child.stdin.write(ACK.encode())
                    child.stdin.flush()
            child.stdin.close()
            kind, _ = next_event(events, deadline)
            require(kind == 'child-eof', 'Inference child emitted duplicate or unexpected output')
            child.wait(timeout=max(0.1, deadline - time.monotonic()))
            require(child.returncode == 0, 'Installed embedding child failed: exit ' + str(child.returncode))
            require(sha(client) == before, 'Installed client changed after inference')
            require(validate_model(model, arguments.model_lock) == hashes, 'Model changed after inference')
        finally:
            if child.stdin and not child.stdin.closed:
                child.stdin.close()
            if child.poll() is None:
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.terminate()
                    try:
                        child.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait(timeout=5)
            reader.join(timeout=1)
            child.stdout.close()
            if child.returncode:
                errors.seek(0, 2)
                errors.seek(max(0, errors.tell() - 4096))
                sys.stderr.write(errors.read().decode('utf-8', errors='replace'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--installed-directory', type=Path, required=True)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--model-lock', type=Path, default=Path(__file__).resolve().parents[1] / 'embedding-model.json')
    parser.add_argument('--text', required=True)
    parser.add_argument('--hold-for-native-audit', action='store_true')
    parser.add_argument('--timeout-seconds', type=float, default=600)
    arguments = parser.parse_args()
    try:
        require(arguments.text.strip() and len(arguments.text) <= 8192, 'Input text must be nonempty and bounded')
        require(math.isfinite(arguments.timeout_seconds) and 0 < arguments.timeout_seconds <= 600, 'Timeout must be finite and between 0 and 600 seconds')
        prove(arguments)
        return 0
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(json.dumps({'type': 'error', 'message': str(error)[:2000]}), flush=True)
        return 1


if __name__ == '__main__':
    sys.exit(main())
