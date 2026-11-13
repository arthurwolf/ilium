# Monitor agents from the command line

Use the change feed to sleep until a pane needs attention. Read a digest to choose what to inspect, then request only the detail needed for the next action. These commands address a local Ilium session; they do not expose SSH session sharing or an agent SDK.

## Read changes and acknowledge them

```sh
ilium ctl agent-guide
ilium ctl events --follow --as manager
ilium ctl digest --as manager --max-items 12 --max-chars 4000
ilium ctl ack SERVER_EPOCH:SEQUENCE@DELIVERY --as manager
```

The cursor in a response belongs to that session and server lifetime. Acknowledge it after handling the delivered items. Reading does not acknowledge. Treat the whole returned cursor as opaque: its delivery suffix identifies exactly the page received, even when another page has the same sequence. Acknowledging a different page cannot commit omitted evidence. Independent managers should use different `--as` names; unnamed outside callers share the session's default consumer. Consumer acknowledgements live in server memory and reset when the server restarts.

Use `--since` to resume from an explicit cursor. An expired, future or foreign cursor returns an error; it never silently skips events. An expired or foreign-cursor error supplies `resume_from=EPOCH:SEQUENCE` and `latest=EPOCH:SEQUENCE`. Inspect the current tree and pane briefs to reconcile any missing history, then pass the supplied resume position to `--since` to read retained events. After a restart, do the same reconciliation before starting a new consumer position. A full output page includes an overflow count and a cursor that retains undisplayed matches for the next page.

Static pane context is repeated until its delivery is acknowledged, then omitted while unchanged for that consumer. Rename or provider changes send fresh context. Delivery receipts retain at most 16 pending pages per consumer; an evicted receipt reports an explicit error, so read and handle the page again.

Compact output is the default for events and digests. A quiet single-session compact response is one line, `quiet cursor=...`. Use `--format json` for structured output. Filter before transfer with `--min-severity`, `--kinds`, `--project`, `--agent` or `--pane`. Routine progress is filtered out by default. A blocking follow gathers a burst for five seconds before returning; invoke it again after handling and acknowledging the response.

## Drill into a pane

```sh
ilium ctl pane 12 --brief
ilium ctl pane 12
ilium ctl screen 12 --lines 20
ilium ctl wait 12 18 --all --state idle,done
ilium ctl wait 12 18 --any --state waiting_approval,done
```

The brief contains the state, at most 300 characters of verified assistant text, and time in the observed state. Unavailable text or age is `null`. Provider transcript reads check the live process, open file and session identity; unsupported or uncertain ownership does not supply a message. The brief excludes prompts and screen contents.

Wait uses the live event stream. It completes when any selected pane matches by default, or when every selected pane matches with `--all`. A pane that closes completes its part of the wait. A disconnected server reports an unknown result. `--timeout SECONDS` bounds a wait when the caller needs a deadline; omitting it waits indefinitely.

## Read across sessions

```sh
ilium ctl tree --all
ilium ctl find --all --agent codex
ilium ctl digest --all --as manager --format json
ilium ctl events --all --follow --as manager --format json
ilium ctl --all-sessions wait 12 18 --all --state idle,done
```

`--all` selects every currently live session for tree, find, events and digest. The global `--all-sessions` also selects this scope; use it with wait, where `--all` means that every selected pane must match. Numeric pane selectors apply independently within each session. A missing selector is a session error, not evidence that its pane closed. Explicit `--session` or `--socket` conflicts with all-session scope. Writes reject that scope before connecting.

Every result identifies its session and socket. An all-session event or digest cursor is a JSON object mapping full socket paths to their independent cursors; pass that object to `--since` on the next all-session read. Acknowledge each delivered cursor separately with `ilium ctl --socket PATH ack CURSOR --as manager`. Digest drill commands already select the correct socket.

The item and character budgets cover the aggregate monitoring response. `more_sessions` reports session pages omitted by the budget or still waiting when a follow returns. Omitted pages retain their input cursor. Errors identify the affected socket; a partial read can succeed while reporting unavailable sessions. Follow opens owned readers for the discovered session snapshot, returns a successful response plus errors already observed, and cancels remaining readers. It does not discover sessions created after the command starts. Wait with `--any` returns the first successful session selection; `--all` requires every session selection and reports any failure.

A scan refuses more than 1024 live endpoints rather than silently limiting coverage. Remote reply retention is capped at 16 MiB; a page that cannot fit its allowance reports an explicit budget error. Increase output budgets or select a session when cursor metadata and results cannot fit together.

## Deterministic attention signals

Active readers evaluate idle assistant replies that end in a question, three identical error reports, five minutes of work without observed activity, three minutes of work without a running progress monitor, and one minute idle while holding a resource. These signals suggest attention; they do not prove a task failed.

Follow waits reevaluate time thresholds inside the server every 30 seconds. Each refresh checks transcripts for at most 32 panes and rotates through larger selections. Unchanged evidence stays deduplicated; uncertain transcript or resource ownership supplies no claim.

## Local HTTP reads

The loopback API uses the same event selection, budgets and brief reader:

```text
GET /v1/events?follow=true&as=manager
GET /v1/digest?as=manager&max-items=12&max-chars=4000
GET /v1/panes/12?brief=true
```

Acknowledge returned cursors with `ilium ctl ack`. See [automation](automation.md) for endpoint discovery, request formats and the existing control operations. A pane brief cannot be combined with screen or line parameters.

## Grant control and record writes

Outside a pane, the command defaults to read access. Inside a pane it defaults to act. Choose `--access read`, `--access act` or `--access admin` explicitly to limit or grant control. `terminate` and `kill-session` require admin. The command reads the selected grant from the session's owner-only runtime token file, beside its socket with the extension `.ctl-tokens.json`. `--token` or `ILIUM_CONTROL_TOKEN` supplies an explicit token instead.

HTTP reads need no token. HTTP writes require `Authorization: Bearer TOKEN`; use the session's act token for ordinary operations and admin token for termination. Invalid tokens return an authorization error. Token files survive a server restart; malformed, nonprivate and nonregular files are refused. If secure credential admission is unavailable, ordinary server use continues while automation writes fail closed. Platforms without proven owner/ACL validation and directory synchronization cannot grant automation writes.

Before dispatch, authorized command and HTTP writes commit a JSONL audit entry to `.ilium/logs/SESSION.ctl.jsonl`, including caller pane or token role, operation, target and timestamp. An audit failure refuses the write. The log rotates at 8 MiB and retains one previous file. An entry records an attempt, not proof that the operation succeeded.

These grants protect the local automation interface from accidental writes. They do not isolate programs running as the same operating-system user: that user owns the raw UI socket and all token roles. Caller pane identity in audit records is a supplied claim.
