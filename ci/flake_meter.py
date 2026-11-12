"""Run the pty_tui_smoke harness repeatedly; write one JSONL result per iteration.

Scratch diagnostics for the v0.1.1-preview.6 Windows gate. Always exits 0 so
every iteration's log is uploaded; read results.jsonl for the outcome.
"""
import argparse
import json
import os
import re
import subprocess
import time
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('logs', type=Path)
    parser.add_argument('--lane', required=True)
    parser.add_argument('--threads', type=int)
    parser.add_argument('--exact')
    parser.add_argument('--iterations', type=int, required=True)
    arguments = parser.parse_args()
    arguments.logs.mkdir(parents=True, exist_ok=True)
    command = ['cargo', 'test', '--locked', '--target', 'x86_64-pc-windows-msvc',
               '--package', 'ilium', '--test', 'pty_tui_smoke']
    harness_arguments = []
    if arguments.threads:
        harness_arguments.append(f'--test-threads={arguments.threads}')
    if arguments.exact:
        harness_arguments += ['--exact', arguments.exact]
    if harness_arguments:
        command += ['--'] + harness_arguments
    environment = dict(os.environ, ILIUM_PTY_DUMP_DIR=str(arguments.logs / 'pty-dumps'))
    results = arguments.logs / 'results.jsonl'
    for iteration in range(1, arguments.iterations + 1):
        processes = subprocess.run(['tasklist', '/FO', 'CSV', '/NH'], capture_output=True, text=True).stdout
        leftovers = [line.split(',')[0].strip('"') for line in processes.splitlines()
                     if line.lower().startswith(('"ilium', '"conhost', '"openconsole'))]
        started = time.monotonic()
        log = arguments.logs / f'iteration-{iteration}.log'
        with log.open('w', encoding='utf-8', errors='replace') as handle:
            exit_code = subprocess.run(command, stdout=handle, stderr=subprocess.STDOUT, env=environment).returncode
        text = log.read_text(encoding='utf-8', errors='replace')
        failed = sorted(set(re.findall(r'^test (\S+) \.\.\. FAILED', text, re.MULTILINE)))
        panics = [line[:300] for line in text.splitlines() if 'panicked at' in line]
        record = {'type': 'result', 'lane': arguments.lane, 'iteration': iteration,
                  'exit_code': exit_code, 'seconds': round(time.monotonic() - started),
                  'failed': failed, 'panics': panics, 'processes_before': leftovers}
        with results.open('a', encoding='utf-8') as handle:
            handle.write(json.dumps(record) + '\n')
        print(json.dumps(record), flush=True)


if __name__ == '__main__':
    main()
