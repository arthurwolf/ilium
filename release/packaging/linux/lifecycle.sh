#!/bin/sh
# Headless Ilium lifecycle against an installed client; the client command is "$@".
# It proves the installed pair resolves its sibling server and runtime libraries:
# new-pane spawns the detached server, ls sees it running, kill-session ends it.
# Every state directory is disposable and owned by this run. A sandboxed client
# (Flatpak) cannot see /tmp, so ILIUM_SMOKE_BASE may name a shared directory.
set -eu
base=${ILIUM_SMOKE_BASE:-${TMPDIR:-/tmp}}
mkdir -p "$base"
state=$(mktemp -d "$base/state.XXXXXX")
runtime=$(mktemp -d)
project=$(mktemp -d "$base/project.XXXXXX")
cleanup() {
    "$@" --cwd "$project" kill-session default >/dev/null 2>&1 || true
    rm -rf "$state" "$runtime" "$project"
}
trap 'cleanup "$@"' EXIT
export HOME="$state" XDG_DATA_HOME="$state/data" XDG_CONFIG_HOME="$state/config" XDG_RUNTIME_DIR="$runtime"
# The server's own readiness window is 5 s; a heavily loaded runner can miss it once.
attempt=1
until timeout 60 "$@" --cwd "$project" new-pane -- sh -c 'echo ilium-smoke-ok; sleep 120'; do
    [ "$attempt" -lt 3 ] || { echo 'lifecycle: new-pane failed three times' >&2; exit 1; }
    attempt=$((attempt + 1))
    "$@" --cwd "$project" kill-session default >/dev/null 2>&1 || true
    sleep 2
done
session_is_running() {
    printf '%s\n' "$1" | awk '$1 == "default" && $2 == "running" && NF == 2 { found=1 } END { exit !found }'
}
# The server can accept new-pane before its snapshot becomes visible to ls.
# Twelve bounded probes tolerate that publication delay without creating another pane.
attempt=1
while :; do
    running=$(timeout 5 "$@" --cwd "$project" ls)
    if session_is_running "$running"; then
        printf '%s\n' "$running"
        break
    fi
    [ "$attempt" -lt 12 ] || { printf '%s\n' "$running"; echo 'lifecycle: session is not running after new-pane' >&2; exit 1; }
    attempt=$((attempt + 1))
    sleep 1
done
timeout 30 "$@" --cwd "$project" kill-session default
ended=$(timeout 30 "$@" --cwd "$project" ls)
if session_is_running "$ended"; then echo 'lifecycle: session survived kill-session' >&2; exit 1; fi
echo 'lifecycle: passed'
