#!/usr/bin/env python3
"""Verify a private loopback audio path for the voice demo, without a model call."""
import json
import math
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import uuid
import wave


def emit(kind, **fields):
    print(json.dumps({'type': kind, **fields}), flush=True)


def main():
    for executable in ('pactl', 'paplay', 'parec', 'findmnt'):
        if not shutil.which(executable):
            raise RuntimeError(f'missing required executable: {executable}')
    subprocess.run(['findmnt', '--mountpoint', '/media/arthur/tmp'], check=True, stdout=subprocess.DEVNULL)
    root = Path('/media/arthur/tmp/ilium-voice-audio-preflight-' + uuid.uuid4().hex[:12])
    root.mkdir(mode=0o700)
    (root / '.scratch-reaper').touch()
    sample = root / 'test-tone.wav'
    rate = 48000
    with wave.open(str(sample), 'wb') as output:
        output.setparams((1, 2, rate, 0, 'NONE', 'not compressed'))
        output.writeframes(b''.join(struct.pack('<h', int(12000 * math.sin(2 * math.pi * 440 * n / rate))) for n in range(rate * 2)))
    name = 'ilium_voice_demo_' + uuid.uuid4().hex[:12]
    module = None
    recorder = None
    recording = None
    result_fields = None
    try:
        result = subprocess.run(['pactl', 'load-module', 'module-null-sink', f'sink_name={name}'], check=True, capture_output=True, text=True, timeout=10)
        module = result.stdout.strip()
        if not module.isdigit():
            raise RuntimeError('audio server did not return a numeric module identifier')
        recording = (root / 'captured.raw').open('wb')
        recorder = subprocess.Popen(['parec', '--device', name + '.monitor', '--format=s16le', '--rate=48000', '--channels=1'], stdout=recording, stderr=subprocess.DEVNULL)
        subprocess.run(['paplay', '--device', name, str(sample)], check=True, timeout=10)
        recorder.terminate()
        recorder.wait(timeout=5)
        recording.close()
        captured = (root / 'captured.raw').read_bytes()
        samples = struct.unpack('<' + 'h' * (len(captured) // 2), captured[:len(captured) // 2 * 2])
        peak = max((abs(value) for value in samples), default=0)
        if peak < 1000:
            raise RuntimeError(f'loopback signal was absent or too quiet (peak {peak})')
        result_fields = dict(audio_loopback='passed', sample_rate_hz=rate, peak=peak, synthetic_tone=True, realtime_calls=0, scratch=str(root))
    finally:
        if recorder and recorder.poll() is None:
            recorder.terminate()
            recorder.wait(timeout=5)
        if recording and not recording.closed:
            recording.close()
        if module:
            subprocess.run(['pactl', 'unload-module', module], check=True, timeout=10)
    emit('result', **result_fields, temporary_audio_device_removed=True)


if __name__ == '__main__':
    try:
        main()
    except (OSError, subprocess.SubprocessError, RuntimeError) as error:
        emit('error', message=str(error))
        sys.exit(1)
