"""Exercise the real shell gate with labelled synthetic CLI readiness states."""
from pathlib import Path
import os
import shutil  # Detect the POSIX fixture tools without invoking native package operations.
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
CLIENT = r'''#!/bin/sh
set -eu
case "$3" in
new-pane) echo new >> "$FIXTURE_STATE/creates" ;;
ls)
    count=$(cat "$FIXTURE_STATE/count")
    echo $((count + 1)) > "$FIXTURE_STATE/count"
    if [ -e "$FIXTURE_STATE/ended" ]; then
        if [ "$FIXTURE_MODE" = 'delayed' ]; then echo 'default not running'; else echo 'no sessions'; fi
    elif [ "$FIXTURE_MODE" = 'delayed' ] && [ "$count" -lt 2 ]; then echo 'no sessions';
    elif [ "$FIXTURE_MODE" = 'stopped' ]; then echo 'default not running';
    elif [ "$FIXTURE_MODE" = 'foreign' ]; then echo 'other running';
    else echo 'default running'; fi ;;
kill-session) touch "$FIXTURE_STATE/ended" ;;
*) exit 2 ;;
esac
'''


@unittest.skipUnless(os.name == 'posix' and all(shutil.which(name) for name in ('sh', 'timeout', 'awk', 'mktemp')), 'POSIX lifecycle fixture tools are required')  # Keep Windows source discovery portable; native Linux gates remain mandatory.
class LinuxLifecycleTests(unittest.TestCase):
    def run_fixture(self, mode):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = Path(temporary)
            client = fixture / 'synthetic-client'; client.write_text(CLIENT); client.chmod(0o755)
            sleep = fixture / 'sleep'; sleep.write_text('#!/bin/sh\nexit 0\n'); sleep.chmod(0o755)
            (fixture / 'count').write_text('0')
            environment = dict(os.environ, FIXTURE_STATE=str(fixture), FIXTURE_MODE=mode,
                               ILIUM_SMOKE_BASE=str(fixture / 'owned-smoke'),
                               PATH=str(fixture) + os.pathsep + os.environ['PATH'])
            result = subprocess.run(['sh', str(ROOT / 'release/packaging/linux/lifecycle.sh'), str(client)],
                                    env=environment, capture_output=True, text=True, timeout=15)
            creates = (fixture / 'creates').read_text().splitlines()
            return result, creates, int((fixture / 'count').read_text())

    def test_delayed_snapshot_becomes_ready_without_respawning_the_pane(self):
        result, creates, listings = self.run_fixture('delayed')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('lifecycle: passed', result.stdout)
        self.assertEqual(creates, ['new'])
        self.assertGreater(listings, 2)

    def test_a_stopped_default_session_never_counts_as_ready(self):
        result, creates, listings = self.run_fixture('stopped')  # Retain the observed probe count for the bounded-readiness contract.
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn('lifecycle: passed', result.stdout)
        self.assertEqual(creates, ['new'])
        self.assertEqual(listings, 12)  # Exhaust exactly the qualified bounded probe window without creating another pane.

    def test_a_different_running_session_does_not_qualify_the_default_session(self):
        result, creates, listings = self.run_fixture('foreign')  # A different session must consume the same bounded readiness window.
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn('lifecycle: passed', result.stdout)
        self.assertEqual(creates, ['new'])
        self.assertEqual(listings, 12)  # Never qualify another session or wait indefinitely for the requested one.


if __name__ == '__main__':
    unittest.main()
