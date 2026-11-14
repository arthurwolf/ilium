#!/usr/bin/env python3
"""Record a local-only Ilium microphone preflight; no recognition or provider calls.

Usage: python3 plugins/core/voice/tools/voice_capture_client_preflight.py [--wav /absolute/speech.wav]
The default input is a generated tone. A WAV can exercise the identical audio path.
For an explicitly authorized paid capture, pass --authorized-realtime-key-file
with a mode-600 key file and --wav. That mode contacts the real provider and
must not be run without the user's separate spending approval.
"""
import argparse
import json
import math
import os
from pathlib import Path
import shlex
import struct
import subprocess
import sys
import time
import uuid
import wave

HERE = Path(__file__).resolve().parent

def emit(kind, **values):
    print(json.dumps({'type': kind, **values}), flush=True)

def run(args, **kwargs):
    return subprocess.run(args, check=True, timeout=15, **kwargs)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--wav', type=Path, help='existing audio input, played into a private virtual microphone')
    parser.add_argument('--authorized-realtime-key-file', type=Path, help='EXPLICIT PAID MODE: key file; requires separate user approval and --wav')
    args = parser.parse_args()
    keyfile = args.authorized_realtime_key_file
    if keyfile and (not args.wav or not keyfile.is_absolute() or not keyfile.is_file() or keyfile.stat().st_mode & 0o077):
        raise ValueError('Paid mode requires --wav and an absolute private key file (mode 600)')
    if args.wav and (not args.wav.is_absolute() or not args.wav.is_file()):
        raise ValueError('--wav must name an existing absolute path')
    run(['findmnt', '--mountpoint', '/media/arthur/tmp'], stdout=subprocess.DEVNULL)
    token = uuid.uuid4().hex[:10]
    root = Path('/media/arthur/tmp') / ('ilium-voice-client-' + token)
    root.mkdir(mode=0o700)
    (root / '.scratch-reaper').touch()
    for p in ('project', 'config/ilium', 'data', 'runtime'):
        (root / p).mkdir(parents=True, mode=0o700)
    (root / 'config/ilium/config.toml').write_text('[onboarding]\nstarted = true\ncompleted = true\nai_enabled = false\n[triggers]\nstartup_complete = []\nagent_session_ready = []\nagent_prompt_submitted = []\nagent_started_working = []\nagent_waiting_background = []\nagent_approval_required = []\nagent_finished_work = []\nterminal_activity_checkpoint = []\n')
    alsa = root / 'alsa.conf'
    alsa.write_text('pcm.!default { type pulse }\nctl.!default { type pulse }\n')
    audio = args.wav or root / 'tone.wav'
    if not args.wav:
        with wave.open(str(audio), 'wb') as out:
            out.setparams((1, 2, 48000, 0, 'NONE', 'not compressed'))
            out.writeframes(b''.join(struct.pack('<h', int(12000 * math.sin(2 * math.pi * 440 * n / 48000))) for n in range(48000 * 4)))
    sink = 'ilium_voice_' + token
    session = 'ilium-voice-' + token
    mock = None
    module = None
    environment = None
    log = root / 'mock.jsonl'
    try:
        module = run(['pactl', 'load-module', 'module-null-sink', 'sink_name=' + sink], capture_output=True, text=True).stdout.strip()
        if not module.isdigit():
            raise RuntimeError('Audio module ID was not numeric')
        mock = subprocess.Popen(['/home/arthur/.bun/bin/bun', str(HERE / 'voice_capture_mock.ts')], stdout=subprocess.PIPE, stderr=(root / 'mock.stderr').open('w'), text=True)
        first = mock.stdout.readline()
        start = json.loads(first)
        if start.get('type') != 'listening':
            raise RuntimeError('Local audio meter did not start')
        with log.open('w') as output:
            output.write(first)
        # Copy the local-only mock stream without retaining audio or credentials.
        import threading
        def copy_log():
            with log.open('a') as output:
                for line in mock.stdout:
                    output.write(line); output.flush()
        reader = threading.Thread(target=copy_log, daemon=True)
        reader.start()
        environment = {'HOME':'/home/arthur', 'USER':'arthur', 'PATH':'/home/arthur/.cargo/bin:/usr/local/bin:/usr/bin:/bin', 'TERM':'xterm-256color', 'XDG_CONFIG_HOME':str(root/'config'), 'XDG_DATA_HOME':str(root/'data'), 'XDG_RUNTIME_DIR':str(root/'runtime'), 'PULSE_SERVER':'unix:/run/user/1000/pulse/native', 'PULSE_SOURCE':sink+'.monitor', 'PULSE_SINK':sink, 'ALSA_CONFIG_PATH':str(alsa), 'OPENAI_API_KEY':'local-test', 'ILIUM_VOICE_REALTIME_URL':f"ws://127.0.0.1:{start['port']}/v1/realtime", 'DBUS_SESSION_BUS_ADDRESS':'unix:path=/nonexistent'}
        recorder = ['/usr/bin/asciinema', 'rec', '--overwrite', '--cols', '110', '--rows', '36', '-c', f'/home/arthur/.cargo/bin/ilium --cwd {shlex.quote(str(root / "project"))}', str(root/'voice-preflight.cast')]
        if keyfile:
            del environment['ILIUM_VOICE_REALTIME_URL']
            del environment['OPENAI_API_KEY']
            # Read the key inside the isolated process, never in command arguments or logs.
            recorder = ['/bin/sh', '-c', 'export OPENAI_API_KEY="$(cat "$1")"; shift; exec "$@"', 'voice-capture', str(keyfile)] + recorder
        cmd = ['/usr/bin/env', '-i'] + [f'{k}={v}' for k,v in environment.items()] + recorder
        run(['tmux','-L',session,'new-session','-d','-s','capture','-x','110','-y','36',shlex.join(cmd)])
        time.sleep(3)
        run(['tmux','-L',session,'send-keys','-t','capture:0.0','F8'])
        time.sleep(4)
        run(['paplay', '--device', sink, str(audio)])
        time.sleep(2)
        screen = run(['tmux','-L',session,'capture-pane','-p','-t','capture:0.0'], capture_output=True, text=True).stdout
        (root / 'screen.txt').write_text(screen)
        events = [json.loads(line) for line in log.read_text().splitlines()]
        chunks = [event for event in events if event.get('type') == 'audio']
        total = sum(event['bytes'] for event in chunks)
        nonzero = sum(event['nonzero'] for event in chunks)
        peak = max((event['peak'] for event in chunks), default=0)
        result = dict(scratch=str(root), mock_session=any(e.get('type')=='session_update' for e in events), audio_bytes=total, nonzero_samples=nonzero, peak=peak, external_wav=bool(args.wav), provider_calls=0)
        if keyfile:
            # The real provider does not expose microphone sample counts here.
            emit('result', scratch=str(root), mode='authorized-realtime', screen_file=str(root/'screen.txt'), cast_file=str(root/'voice-preflight.cast'), recognition_verified=False, provider_calls='attempted')
        else:
            if not result['mock_session'] or peak < 1000 or nonzero == 0:
                raise RuntimeError('Client audio was not observed: ' + json.dumps(result))
            emit('result', **result)
    finally:
        if environment:
            subprocess.run(['/home/arthur/.cargo/bin/ilium','--cwd',str(root/'project'),'kill-session','default'], env=environment, capture_output=True, timeout=10)
            subprocess.run(['tmux','-L',session,'kill-server'], capture_output=True, timeout=10)
        if mock:
            mock.terminate()
            mock.wait(timeout=5)
        if module and module.isdigit():
            run(['pactl','unload-module',module], capture_output=True)
        emit('cleanup', scratch=str(root), private_audio_removed=bool(module))

if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        emit('error', message=str(error))
        sys.exit(1)
