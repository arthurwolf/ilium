#!/usr/bin/env bash
# Builds the plugin developer manual into docs/plugins-manual/_site/.
# Uses the real Jekyll build when `jekyll` is installed, otherwise build_fallback.py.
# Output: JSONL on stdout, one object per line with a "type" field
# (progress, result or error). Exit status is non-zero on any error.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FALLBACK_PYTHON="/usr/bin/python3"

emit_error() {
  printf '{"type":"error","step":"%s","message":"%s"}\n' "$1" "$2"
  exit 1
}

if command -v jekyll >/dev/null 2>&1; then
  printf '{"type":"progress","step":"jekyll_build","message":"Running jekyll build of the plugin developer manual"}\n'
  if ! (cd "$ROOT" && jekyll build --source "$ROOT" --destination "$ROOT/_site" --quiet) >&2; then
    emit_error "jekyll_build" "jekyll build failed; see stderr"
  fi
  file_count="$(find "$ROOT/_site" -type f | wc -l)"
  printf '{"type":"result","builder":"jekyll","output_dir":"%s","files":%s,"message":"Built with jekyll"}\n' \
    "$ROOT/_site" "$file_count"
  exit 0
fi

if [ ! -x "$FALLBACK_PYTHON" ]; then
  emit_error "select_builder" "jekyll is not installed and $FALLBACK_PYTHON is missing"
fi

printf '{"type":"progress","step":"fallback_build","message":"jekyll not installed; using build_fallback.py"}\n'
"$FALLBACK_PYTHON" "$ROOT/build_fallback.py"
status=$?
if [ "$status" -ne 0 ]; then
  emit_error "fallback_build" "build_fallback.py exited with status $status"
fi
exit 0
