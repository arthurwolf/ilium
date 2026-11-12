#!/usr/bin/python3
"""Run the three release test stages (unit, doc, integration) and keep going after a failure.

Usage: gate_tests.py WORKSPACE_ROOT LOG_DIR [--target TRIPLE]
Writes LOG_DIR/{unit,doc,integration}.log and LOG_DIR/stages.jsonl; exit 0 always
(the summary lines say which stage failed).
"""
import json, os, subprocess, sys
from pathlib import Path

root, logs = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
target = sys.argv[sys.argv.index('--target') + 1] if '--target' in sys.argv else None
sys.path.insert(0, str(root / 'release/scripts'))
from native_test_scope import preview_integration_arguments

logs.mkdir(parents=True, exist_ok=True)
env = dict(os.environ, RUST_TEST_THREADS='2', RUST_BACKTRACE='1')
base = ['cargo', 'test', '--locked', '--workspace', '--exclude', 'ilium-plugin-*', '--no-fail-fast']
if target:
    base += ['--target', target]
output = subprocess.run(['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1'],
                        cwd=root, capture_output=True, text=True, check=True).stdout
# A build-service proxy may interleave its own JSONL progress lines; keep the metadata record.
metadata = [json.loads(line) for line in output.splitlines() if '"workspace_members"' in line][-1]
stages = [('unit', ['--lib', '--bins']), ('doc', ['--doc']), ('integration', preview_integration_arguments(metadata))]
with open(logs / 'stages.jsonl', 'w') as summary:
    for name, extra in stages:
        with open(logs / f'{name}.log', 'w', encoding='utf-8', errors='replace') as log:
            code = subprocess.run(base + extra, cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT).returncode
        line = json.dumps({'type': 'result', 'stage': name, 'exit_code': code})
        summary.write(line + '\n')
        summary.flush()
        print(line, flush=True)
