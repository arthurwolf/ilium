"""One bounded Codex goal query, executed only inside a read-only sandbox.

The parent must pin the database and WAL files, close inherited descriptors,
verify session ownership and validate this response before using it. No provider
path, request, database error or objective is written to stderr.
"""

import json
import sqlite3
import sys


def main():
    request = sys.stdin.buffer.read(1025)
    if len(request) > 1024:
        return 1
    thread_id = json.loads(request)["thread_id"]
    if not isinstance(thread_id, str) or not 1 <= len(thread_id) <= 128:
        return 1
    # The sandbox exposes only pinned, read-only files at this fixed path.
    database = sqlite3.connect("file:/goal/goals_1.sqlite?mode=ro", uri=True, timeout=0.1)
    try:
        database.execute("PRAGMA query_only=ON")
        database.execute("BEGIN")
        version = database.execute("PRAGMA user_version").fetchone()[0]
        migrations = database.execute(
            "SELECT version, success, hex(checksum) FROM _sqlx_migrations ORDER BY version LIMIT 3"
        ).fetchall()
        columns = database.execute("PRAGMA table_info(thread_goals)").fetchall()
        if len(columns) > 32:
            return 1
        rows = database.execute(
            "SELECT thread_id, status, objective FROM thread_goals WHERE thread_id = ? LIMIT 2",
            (thread_id,),
        ).fetchall()
        if len(rows) > 1:
            return 1
        if rows and (not isinstance(rows[0][2], str) or len(rows[0][2].encode("utf-8")) > 1048576):
            return 1
        result = {"user_version": version, "migrations": migrations, "columns": columns, "rows": rows}
        encoded = json.dumps(result, ensure_ascii=True, separators=(",", ":")).encode("ascii")
        if len(encoded) > 8388608:
            return 1
        sys.stdout.buffer.write(encoded)
        return 0
    finally:
        database.close()


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, TypeError, sqlite3.Error):
        sys.exit(1)
