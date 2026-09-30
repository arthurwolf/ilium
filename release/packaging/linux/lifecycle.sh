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
sleep 1
running=$(timeout 30 "$@" --cwd "$project" ls)
printf '%s\n' "$running"
case "$running" in *running*) ;; *) echo 'lifecycle: session is not running after new-pane' >&2; exit 1 ;; esac
timeout 30 "$@" --cwd "$project" kill-session default
ended=$(timeout 30 "$@" --cwd "$project" ls)
case "$ended" in *running*) echo 'lifecycle: session survived kill-session' >&2; exit 1 ;; esac
echo 'lifecycle: passed'
