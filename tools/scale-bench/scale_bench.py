#!/usr/bin/env python3
"""Many-pane Ilium scale benchmark.

Launches a fully isolated Ilium session (own XDG config/data/runtime
directories, own scratch project, own tmux server) from a chosen directory
holding `ilium` and `ilium-server`, fills it with synthetic Codex-like panes
(tools/scale-bench/codex), and measures the client and server processes:

  * CPU (user+system) from /proc/<pid>/stat over fixed intervals
  * read/write syscalls and bytes from /proc/<pid>/io
  * voluntary + involuntary context switches summed over all threads
  * thread count and resident memory
  * key-to-visible latency: a tree-navigation key is sent through tmux and the
    rendered screen is captured until the sidebar selection highlight moves

Every line on stdout is one JSON object with a `type` field
(`progress`, `sample`, `latency`, `result`, `warning`, `error`).
Never point --project at a directory that already has a live ilium-server.

Example:
  nice -n 19 python3 tools/scale-bench/scale_bench.py --bin-dir /path/bin \
      --panes 100 --label baseline-100 --out /tmp/bench.jsonl
"""

import argparse
import json
import os
import random
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
CLOCK_TICKS = os.sysconf("SC_CLK_TCK")
SCRUBBED_ENVIRONMENT_PREFIXES = ("CLAUDE", "CODEX", "ILIUM_")


def emit(record, sink):
    line = json.dumps(record, sort_keys=True)
    print(line, flush=True)
    if sink:
        sink.write(line + "\n")
        sink.flush()


def read_process_counters(pid):
    with open(f"/proc/{pid}/stat") as handle:
        stat = handle.read()
    fields = stat[stat.rindex(")") + 2 :].split()
    cpu_ticks = int(fields[11]) + int(fields[12])
    counters = {"cpu_seconds": cpu_ticks / CLOCK_TICKS}
    with open(f"/proc/{pid}/io") as handle:
        for line in handle:
            key, value = line.split(":")
            counters[key] = int(value)
    voluntary = involuntary = 0
    threads = 0
    for task in os.listdir(f"/proc/{pid}/task"):
        try:
            with open(f"/proc/{pid}/task/{task}/status") as handle:
                for line in handle:
                    if line.startswith("voluntary_ctxt_switches"):
                        voluntary += int(line.split()[1])
                    elif line.startswith("nonvoluntary_ctxt_switches"):
                        involuntary += int(line.split()[1])
            threads += 1
        except OSError:
            pass
    counters["voluntary_switches"] = voluntary
    counters["involuntary_switches"] = involuntary
    counters["threads"] = threads
    with open(f"/proc/{pid}/status") as handle:
        for line in handle:
            if line.startswith("VmRSS"):
                counters["rss_kib"] = int(line.split()[1])
    return counters


def interval_rates(before, after, seconds):
    return {
        "cpu_percent": 100.0 * (after["cpu_seconds"] - before["cpu_seconds"]) / seconds,
        "read_syscalls_per_second": (after["syscr"] - before["syscr"]) / seconds,
        "write_syscalls_per_second": (after["syscw"] - before["syscw"]) / seconds,
        "read_megabytes_per_second": (after["rchar"] - before["rchar"]) / seconds / 1e6,
        "voluntary_switches_per_second": (after["voluntary_switches"] - before["voluntary_switches"]) / seconds,
        "involuntary_switches_per_second": (after["involuntary_switches"] - before["involuntary_switches"]) / seconds,
        "threads": after["threads"],
        "rss_kib": after.get("rss_kib", 0),
    }


def find_server_pid(runtime_directory):
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            with open(f"/proc/{entry}/cmdline", "rb") as handle:
                arguments = handle.read().split(b"\0")
        except OSError:
            continue
        if arguments and arguments[0].endswith(b"ilium-server") and any(
            runtime_directory.encode() in argument for argument in arguments
        ):
            return int(entry)
    return None


def free_local_port():
    import socket

    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def percentile(values, fraction):
    ordered = sorted(values)
    if not ordered:
        return None
    index = min(len(ordered) - 1, max(0, round(fraction * (len(ordered) - 1))))
    return ordered[index]


class IsolatedSession:
    def __init__(self, arguments, sink):
        self.arguments = arguments
        self.sink = sink
        self.root = tempfile.mkdtemp(prefix="sb", dir=arguments.scratch)
        self.tmux_server = "sb" + os.path.basename(self.root)[2:]
        self.binaries = os.path.join(self.root, "bin")
        self.project = os.path.join(self.root, "p")
        self.runtime = os.path.join(self.root, "r")
        self.environment = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith(SCRUBBED_ENVIRONMENT_PREFIXES)
        }
        self.environment.update(
            XDG_CONFIG_HOME=os.path.join(self.root, "c"),
            XDG_DATA_HOME=os.path.join(self.root, "d"),
            XDG_STATE_HOME=os.path.join(self.root, "s"),
            XDG_CACHE_HOME=os.path.join(self.root, "k"),
            XDG_RUNTIME_DIR=self.runtime,
            TERM="xterm-256color",
        )
        self.client_pid = None
        self.server_pid = None
        self.api_port = free_local_port()

    def prepare(self):
        for directory in ("bin", "p", "r", "c/ilium", "d", "s", "k", "a"):
            os.makedirs(os.path.join(self.root, directory), exist_ok=True)
        os.chmod(self.runtime, 0o700)
        for name in ("ilium", "ilium-server"):
            shutil.copy2(os.path.join(self.arguments.bin_dir, name), os.path.join(self.binaries, name))
        agent = os.path.join(self.root, "a", "codex")
        shutil.copy2(os.path.join(HERE, "codex"), agent)
        os.chmod(agent, 0o755)
        self.agent = agent
        self.write_config()

    def write_config(self):
        """Copies the user's configuration so the visual settings (background
        animation, sidebar density, theme, keyboard preset) match the real
        installation, minus everything with an outside effect: credentials are
        dropped (no provider call can run), desktop notifications and sounds are
        switched off, and the local HTTP API moves to a private port so it never
        collides with a live session's listener."""
        source = self.arguments.config
        target = os.path.join(self.root, "c", "ilium", "config.toml")
        overrides = {
            "notifications": {"enabled": "false"},
            "sound.events": {key: "false" for key in (
                "agent_finished", "agent_started", "approval_required",
                "task_failed", "task_succeeded", "waiting_background")},
            "debug": {"file_logging_enabled": "true" if self.arguments.server_log else "false"},
        }
        lines = []
        if source and os.path.exists(source):
            with open(source) as handle:
                lines = handle.read().splitlines()
        output = []
        section = None
        seen_sections = set()

        def flush_missing(name):
            for key, value in overrides.get(name, {}).items():
                if (name, key) not in written:
                    output.append(f"{key} = {value}")

        written = set()
        for line in lines:
            stripped = line.strip()
            if stripped.startswith("api_key"):
                continue
            if stripped.startswith("[") and not stripped.startswith("[["):
                if section is not None:
                    flush_missing(section)
                section = stripped.strip("[]").strip()
                seen_sections.add(section)
                output.append(line)
                continue
            if stripped.startswith("[["):
                if section is not None:
                    flush_missing(section)
                section = None
                output.append(line)
                continue
            key = stripped.split("=", 1)[0].strip() if "=" in stripped else None
            if section in overrides and key in overrides[section]:
                output.append(f"{key} = {overrides[section][key]}")
                written.add((section, key))
                continue
            output.append(line)
        if section is not None:
            flush_missing(section)
        for name, values in overrides.items():
            if name not in seen_sections:
                output.append(f"[{name}]")
                output.extend(f"{key} = {value}" for key, value in values.items())
        if "api" not in seen_sections:
            output.append("[api]")
            output.append(f"port = {self.api_port}")
        with open(target, "w") as handle:
            handle.write("\n".join(output) + "\n")

    def ilium(self, *command, check=True):
        return subprocess.run(
            [os.path.join(self.binaries, "ilium"), "--cwd", self.project, *command],
            env=self.environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=60,
            check=check,
        )

    def tmux(self, *command, capture=False):
        result = subprocess.run(
            ["tmux", "-L", self.tmux_server, *command],
            env=self.environment,
            stdout=subprocess.PIPE if capture else subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
        return result.stdout if capture else result.returncode

    def start(self):
        columns, rows = self.arguments.geometry.split("x")
        launch = (
            f"exec env {' '.join(f'{k}={v}' for k, v in self.environment.items() if k.startswith('XDG_'))} "
            f"{os.path.join(self.binaries, 'ilium')} --cwd {self.project}"
        )
        self.tmux("new-session", "-d", "-s", "bench", "-x", columns, "-y", rows, launch)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            self.server_pid = find_server_pid(self.runtime)
            pane_pid = self.tmux("display-message", "-p", "-t", "bench:0.0", "#{pane_pid}", capture=True).strip()
            if self.server_pid and pane_pid.isdigit():
                self.client_pid = int(pane_pid)
                return
            time.sleep(0.2)
        screen = self.tmux("capture-pane", "-p", "-t", "bench:0.0", capture=True)
        raise RuntimeError(
            "isolated ilium session did not start within 30 s: "
            f"server_pid={self.server_pid} pane_pid={pane_pid!r} "
            f"screen_tail={screen.strip()[-600:]!r}"
        )

    def add_panes(self):
        modes = (
            ["working"] * self.arguments.working
            + ["timer"] * self.arguments.timer
        )
        modes += ["idle"] * max(0, self.arguments.panes - len(modes))
        random.Random(7).shuffle(modes)
        for index, mode in enumerate(modes):
            try:
                self.ilium(
                    "new-pane",
                    "--",
                    self.agent,
                    mode,
                    "--threads",
                    str(self.arguments.agent_threads),
                )
            except subprocess.CalledProcessError as error:
                raise RuntimeError(
                    f"new-pane {index + 1} of {len(modes)} failed (exit {error.returncode}): "
                    f"stderr={error.stderr.strip()[-800:]!r} stdout={error.stdout.strip()[-400:]!r}"
                ) from error
            if index % 25 == 24:
                emit({"type": "progress", "step": "add_panes", "created": index + 1, "total": len(modes)}, self.sink)
        return modes

    def server_log_summary(self):
        """Counts detection-loop failures in the server's own log (only written
        when --server-log is set; logging forces extra detection work, so
        never combine it with a measured run)."""
        try:
            with open(f"/proc/{self.server_pid}/cmdline", "rb") as handle:
                arguments = [a.decode() for a in handle.read().split(b"\0")]
            log_path = arguments[arguments.index("--log-path") + 1]
            with open(log_path, errors="replace") as handle:
                text = handle.read()
        except (OSError, ValueError, IndexError) as error:
            return {"available": False, "reason": str(error)}
        failures = [line for line in text.splitlines() if "tick failed" in line]
        return {
            "available": True,
            "tick_failures": len(failures),
            "first_failure": failures[0][:240] if failures else None,
        }

    def capture(self):
        return self.tmux("capture-pane", "-e", "-p", "-t", "bench:0.0", capture=True)

    def selected_row(self):
        """Row index of the sidebar selection highlight: the first screen row
        whose sidebar part (before the panel divider) sets a background
        colour. Comparing only this row index, not the whole screen, keeps
        animated status glyphs and repainting panes from counting as the
        key's effect."""
        for index, line in enumerate(self.capture().splitlines()):
            parts = line.split("\u2502")
            sidebar = "\u2502".join(parts[:2]) if len(parts) > 2 else line
            if "\x1b[48;" in sidebar:
                return index
        return None

    def measure_latency(self, presses):
        """Moves the sidebar selection with the arrow keys after focusing the
        tree (prefix, then `w`), timing each press until the selection
        highlight is drawn on a different row."""
        samples = []
        timeouts = 0
        self.tmux("send-keys", "-t", "bench:0.0", "C-b")
        time.sleep(0.2)
        self.tmux("send-keys", "-t", "bench:0.0", "w")
        time.sleep(0.5)
        for index in range(presses):
            key = "Down" if index % 2 == 0 else "Up"
            before = self.selected_row()
            started = time.perf_counter()
            self.tmux("send-keys", "-t", "bench:0.0", key)
            changed = False
            while time.perf_counter() - started < 3.0:
                if self.selected_row() != before:
                    changed = True
                    break
            elapsed_ms = (time.perf_counter() - started) * 1000
            if changed:
                samples.append(elapsed_ms)
            else:
                timeouts += 1
            time.sleep(0.25)
        return samples, timeouts

    def stop(self):
        try:
            self.ilium("kill-session", "default", check=False)
        except Exception as error:  # noqa: BLE001 - teardown must continue
            emit({"type": "warning", "step": "kill_session", "message": str(error)}, self.sink)
        self.tmux("kill-server")
        time.sleep(1.0)
        subprocess.run(["pkill", "-f", os.path.join(self.root, "a", "codex")], check=False)
        if self.server_pid:
            try:
                os.kill(self.server_pid, 15)
            except ProcessLookupError:
                pass
        shutil.rmtree(self.root, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--bin-dir", required=True, help="directory holding ilium and ilium-server")
    parser.add_argument("--label", required=True)
    parser.add_argument("--panes", type=int, default=50)
    parser.add_argument("--working", type=int, default=None, help="continuously repainting panes (default 30%%)")
    parser.add_argument("--timer", type=int, default=None, help="1 Hz footer-clock panes (default 30%%)")
    parser.add_argument("--agent-threads", type=int, default=16)
    parser.add_argument("--geometry", default="266x68")
    parser.add_argument("--warmup", type=float, default=20.0)
    parser.add_argument("--interval", type=float, default=10.0)
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--latency-presses", type=int, default=30)
    parser.add_argument("--config", default=os.path.expanduser("~/.config/ilium/config.toml"))
    parser.add_argument("--scratch", default="/media/arthur/tmp")
    parser.add_argument("--server-log", action="store_true",
                        help="enable server file logging and report detection tick failures")
    parser.add_argument("--out", default=None, help="also append every JSON line to this file")
    arguments = parser.parse_args()
    if arguments.working is None:
        arguments.working = round(arguments.panes * 0.3)
    if arguments.timer is None:
        arguments.timer = round(arguments.panes * 0.3)

    sink = open(arguments.out, "a") if arguments.out else None
    session = IsolatedSession(arguments, sink)
    try:
        session.prepare()
        session.start()
        emit({"type": "progress", "step": "started", "client_pid": session.client_pid,
              "server_pid": session.server_pid, "root": session.root}, sink)
        modes = session.add_panes()
        emit({"type": "progress", "step": "warmup", "seconds": arguments.warmup,
              "working": modes.count("working"), "timer": modes.count("timer"), "idle": modes.count("idle")}, sink)
        time.sleep(arguments.warmup)
        # Dismiss the first-run "set up agent facilities" dialog with "Not now":
        # applying it would install hooks, which a benchmark must not do.
        session.tmux("send-keys", "-t", "bench:0.0", "Escape")
        time.sleep(1.0)
        emit({"type": "progress", "step": "screen_after_warmup",
              "screen": [line.rstrip() for line in session.capture().splitlines()[:30]]}, sink)

        intervals = []
        for index in range(arguments.samples):
            before = {role: read_process_counters(pid) for role, pid in
                      (("client", session.client_pid), ("server", session.server_pid))}
            time.sleep(arguments.interval)
            after = {role: read_process_counters(pid) for role, pid in
                     (("client", session.client_pid), ("server", session.server_pid))}
            sample = {role: interval_rates(before[role], after[role], arguments.interval) for role in before}
            intervals.append(sample)
            emit({"type": "sample", "label": arguments.label, "index": index, **sample}, sink)

        latencies, timeouts = session.measure_latency(arguments.latency_presses)
        latency_record = {"type": "latency", "label": arguments.label,
                          "samples_ms": [round(v, 1) for v in latencies], "timeouts": timeouts}
        if timeouts:
            latency_record["screen"] = session.capture().splitlines()[:40]
        emit(latency_record, sink)

        summary = {}
        for role in ("client", "server"):
            summary[role] = {
                metric: round(statistics.median(sample[role][metric] for sample in intervals), 3)
                for metric in intervals[0][role]
            }
        summary["combined_cpu_percent"] = round(summary["client"]["cpu_percent"] + summary["server"]["cpu_percent"], 3)
        summary["latency_ms"] = {
            "p50": percentile(latencies, 0.5),
            "p95": percentile(latencies, 0.95),
            "max": max(latencies) if latencies else None,
            "timeouts": timeouts,
        }
        if arguments.server_log:
            summary["server_log"] = session.server_log_summary()
        emit({"type": "result", "label": arguments.label, "panes": arguments.panes,
              "working": modes.count("working"), "timer": modes.count("timer"), "idle": modes.count("idle"),
              "geometry": arguments.geometry, "interval_seconds": arguments.interval,
              "samples": arguments.samples, **summary}, sink)
    except Exception as error:  # noqa: BLE001 - reported as a typed error line
        emit({"type": "error", "label": arguments.label, "message": repr(error)}, sink)
        failed = True
    else:
        failed = False
    finally:
        session.stop()
        if sink:
            sink.close()
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
