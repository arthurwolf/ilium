# Fixtures

`claude_session.jsonl`, `codex_session.jsonl` and `codex_subagent.jsonl` are **synthetic**
transcripts that copy the line shapes (field names, nesting, value types) of real Claude Code
and Codex logs. Every prompt, path, identifier and token count is invented; none comes from a
real session. They exercise, in order of appearance:

* Claude: a streamed message written twice (dedupe, last usage wins), an advisor turn with
  `usage.iterations` (the advisor iteration must not count), Haiku / `<synthetic>` / sidechain
  requests (skipped in main sessions), a non-JSON line, a 2-hour idle gap (cold cache), mixed
  5m/1h cache-write tiers, an automatic `compact_boundary` whose logged `postTokens` differs from
  the measured next request, its `isCompactSummary` line (7,000 characters), a manual compaction
  with no request after it, and a truncated last line.
* Codex: `session_meta` / `turn_context` / `thread_settings_applied` (model switch),
  `model_context_window` in a `token_count` event, a duplicated `token_usage_record`, a compaction
  request plus its `compacted` line (matched by `compaction_response_id`), an older-format
  `compacted` line without an id (unmatched), and a truncated record.
* Codex subagent: `session_meta.source.subagent` and a 272,000-token window.

Since trace format v2 the fixtures also carry tool calls whose paths, commands and a secret-looking
token (`TOPSECRETTOKEN`, under `/home/secret-user/projects/topsecret`) exist only to prove the
parsers keep hashes and never text:

* Claude: a Grep on a streamed line, a Read, a Bash `cat plan.md && rg ...` (one read-like command;
  its relative path resolves against the logged `cwd` to the same hash as the Read), a request
  whose tool uses are split over three lines (one tool-use id repeated), an Edit, and a `sed -i`
  write heuristic.
* Codex: an `exec` code block with `cmd:"cat ... && rg ..."`, a `function_call` shell command, an
  `apply_patch` inside code, a non-shell tool, a tool *output* line (not a call), and a later
  `sed -n` re-reading the same file.
