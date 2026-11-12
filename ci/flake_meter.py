"""Run the pty_tui_smoke harness repeatedly; write one JSONL result per iteration.

Scratch diagnostics for the v0.1.1-preview.6 Windows gate. Always exits 0 so
every iteration's log is uploaded; read results.jsonl for the outcome.
"""
import argparse
import json
import re
import subprocess
import time
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('logs', type=Path)
    parser.add_argument('--lane', choices=['parallel', 'serial'], required=True)
    parser.add_argument('--iterations', type=int, required=True)
    arguments = parser.parse_args()
    arguments.logs.mkdir(parents=True, exist_ok=True)
    command = ['cargo', 'test', '--locked', '--target', 'x86_64-pc-windows-msvc',
               '--package', 'ilium', '--test', 'pty_tui_smoke']
    if arguments.lane == 'serial':
        command += ['--', '--test-threads=1']
    results = arguments.logs / 'results.jsonl'
    for iteration in range(1, arguments.iterations + 1):
        started = time.monotonic()
        log = arguments.logs / f'iteration-{iteration}.log'
        with log.open('w', encoding='utf-8', errors='replace') as handle:
            exit_code = subprocess.run(command, stdout=handle, stderr=subprocess.STDOUT).returncode
        text = log.read_text(encoding='utf-8', errors='replace')
        failed = sorted(set(re.findall(r'^test (\S+) \.\.\. FAILED', text, re.MULTILINE)))
        panics = [line[:300] for line in text.splitlines() if 'panicked at' in line]
        record = {'type': 'result', 'lane': arguments.lane, 'iteration': iteration,
                  'exit_code': exit_code, 'seconds': round(time.monotonic() - started),
                  'failed': failed, 'panics': panics}
        with results.open('a', encoding='utf-8') as handle:
            handle.write(json.dumps(record) + '\n')
        print(json.dumps(record), flush=True)


if __name__ == '__main__':
    main()
