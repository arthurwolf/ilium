# Advisory polling detection

The detector is integrated and off by default. Nothing is sent to a provider
unless the user enables it, saves an OpenAI key and has budget left.

Polling detection is an opt-in advisory service. It distinguishes repeated
nonblocking status queries, repeated unchanged narration, and repeated resumes
of a blocking wait. A local candidate is a request for review, not a confirmed
finding. Automatic progress probes, one blocking wait, changing results and
independent work do not establish polling. The service never sends corrective
input to an agent or stops its process.

## Evidence ownership

The pure domain owns normalized activity and exact session/incarnation/pane/
generation bindings. The transcript adapter extracts bounded Claude and Codex
events, links tool results, deduplicates streaming records and retains incomplete
lines. Results do not increment the scheduling interval. The server establishes
process birth, transcript identity and generation before and after a read;
parsing and deserialization alone establish no provenance.

Acquisition moves the original admitted cursor and batch across its final fence;
cloning them must not release their storage charge. Completed reads retain
storage rather than occupied worker credits. Temporary JSON expansion and
retained backing capacities are separate budgets, so a small idle cursor does
not retain an entire maximum parse reservation. The storage-custody regression
and unchanged-bank 1024-baseline capacity regression both pass.

Activity windows are bounded and begin prospectively after opt-in. Disabled or
invalid configuration drops pending activity. Monotonic spacing and a reset
after an admitted check coalesce new activity without a catch-up burst. Closure
removes only the exact bound generation. A delayed close cannot clear its
replacement. Capacity refusal is counted, not handled by evicting live agents.

The pure ingestion boundary requires an empty prospective baseline before accepting
activity. Duplicate source ranges do not increment the interval. A gap or partial
overlap discards the incomplete window and its interval ticks while preserving
minimum spacing after the last admitted check. Server incarnation and pane
identify independent windows even when session labels match. These continuity
contracts pass package tests; the leader still needs to consume the feed.

## Recovery contract

A transcript checkpoint contains only file identity, a complete-record cursor,
an anchor digest and bounded record-identity digests. Partial lines and raw event
text are not durable checkpoint contents. Reader recovery alone establishes no
process or session authority: the server must re-establish the current boot,
process birth, session and pane generation, and fence the source before and after
reading. A process number or second-resolution start time is insufficient.

Persist window-start replay metadata separately from the last applied cursor.
After verified recovery, replay can rebuild an ephemeral window without counting
already applied events again. A durable logical-window identity prevents a replay
from buying a duplicate decision. If continuity cannot be proved, report a gap,
reset prospectively and avoid a catch-up burst. A new server incarnation cannot
adopt an old binding merely because its transcript path is unchanged.

Repository ownership must outlive an individual SQLite connection. The runtime
will retain one shared lease owner while bounded I/O jobs open and close their
connections; reopening under that same owner must not run leadership recovery or
cancel prepared work. A new owner alone performs crash recovery. An executing
job retains the lease through physical completion even when its async caller is
cancelled. Repository reconnect and startup storage custody have passing focused regressions; concurrent scheduling remains unconnected.

Dispatch admission returns an owned ticket tied to the ledger leadership epoch.
Provider work holds no mutable repository borrow. Completion must reject a ticket
from a different epoch before changing any attempt, including an attempt with the
same request identifier in another ledger. Unknown requests recovered after a
leadership change keep their reservation and are not automatically resent.

The pure decision queue passes fairness, coalescing, duplicate-completion and
capacity tests: two global active slots, one per agent and 128 pending agents.
A newer pending window keeps its agent's place, and a busy agent does not block
other agents. Closing a source discards pending work but never frees a physically
running slot. Epoch-fenced completion cannot release another owner's slot. The
runtime must retain ownership and release a slot only after physical work ends.

These are integration requirements; the checkpoint reader and admission APIs do
not by themselves implement durable window recovery or concurrent scheduling.

## Runtime composition

The server entrypoint supplies polling configuration, application data and
session-registry directories explicitly. Embedded and isolated server consumers
pass no polling paths by default, preventing implicit access to real user
configuration or a shared ledger. Registry membership is independent of the
optional resource-queue service. Configured servers publish an owner-only hint
through bounded native I/O, retain its inode and storage admission, and remove
only that same record before shutting down their execution bank. Startup and
normal-shutdown publication have passing real-transport regressions. A crash
may leave a stale hint, which still grants no authority. Startup loads settings on the bounded I/O lane
before requests are served. Acquiring the shared ledger lease gates credential
loading; a follower loads settings only and reports unavailable coordination
until it can obtain the leader projection. SQLite connections stay on I/O
workers, while the shared lease and its result-storage charge live with the
service. Startup, readable disabled accounting and serialized reload have passing
focused regressions. Reload preserves the accepted lease across invalid config
and reopens worker-local connections without repeating crash recovery. The
server-owned reload watcher has a passing no-client transport regression;
coordination tasks remain under integration.

## Cross-session authority

One elected process owns the user-wide detector lease, provider queue and
spending ledger. Registry records are endpoint hints, not authentication.
Discovery traverses bounded pages and repeats after exhaustion because a
directory scan is not a snapshot. Unreadable or malformed entries retain an
explicit incomplete state.

The serving publisher holds a kernel lock on its admitted private record inode.
The detector authenticates the kernel-reported connected peer against that lock,
process birth, exact endpoint and serving incarnation. Its separately opened
observation handle cannot extend the publisher lease or follow a replacement
inode. These authority, discovery and transport boundaries pass 28 focused
tests; connection consumption and scheduling still require integration.

The transport handshake now carries the kernel-observed peer identity before
stream splitting; its positive real-transport admission regression has passed. It
has passing prospective baseline, append-only delivery, generation replacement and slow-consumer regressions. Opt-out and lease release refuse further private enqueue. The dedicated handshake suppresses unrelated session broadcasts before replying; this correction passes the selected server/private transport suite. A connection-owned authority task rechecks
opt-in and its original lease proof through bounded I/O and sends an explicit
stop reason on revocation. Real-transport release/revocation has a passing
regression. The evidence subscription must preserve this authority across the
transport boundary. A private registry file, claimed process number, or
contended lock alone does not prove leadership. Before releasing a batch, the
serving session verifies the peer's live birth identity and ownership of the
exact lease file. Unsupported or incomplete proof fails closed. The detector
checks the serving incarnation and each batch's agent generation and cursor.
Lease loss cancels subscriptions; gaps invalidate continuity.

The evidence feed has no provider-key field and exposes no transcript path.
Only the elected detector reads the saved global OpenAI key. Attached clients
receive settings, findings and accounting projections rather than private
cross-session evidence.

## Provider and accounting boundaries

The provider adapter uses the fixed OpenAI Decisions endpoint and validates
choices, confidence and usage. It cannot inherit a customized text-generation
endpoint. Compact evidence omits raw command arguments and tool output, uses
window-local opaque target identifiers and reports output changes locally.
Truncated or missing results remain unknown. Credential redaction is repeated
at the outbound boundary. Findings contain locally derived facts, not invented
model explanations.

The private SQLite repository holds an exclusive writer lease, uses full
synchronization, and reserves a conservative budget before marking dispatch.
Unknown transport outcomes retain their reservation. Restart cancels prepared
attempts and marks unresolved dispatched attempts unknown rather than replaying
them. Fixed-point costs retain their pricing inputs; unknown charges are never
displayed as zero. Accounting retains metadata indefinitely and stores no
prompt, tool output or provider response text.

History pages use a durable transactional revision and a bounded read snapshot.
Both insertion and accounting-state changes advance that revision. A cursor
from before completion or crash recovery is rejected rather than combining
records from inconsistent accounting states. The revision-safe repository API
and real-server history projection have passing focused regressions. Full
cross-session end-to-end verification remains outstanding.

## Candidate-selection qualification

The offline `polling_candidate_evaluation` example evaluates 13 explicitly
synthetic scenarios, including sparse repeated checks, changing progress,
independent work, one blocking wait and repeated wait resumptions. It exercises
the production activity window and local filter; it does not call a provider.
Run through the build dispatcher:

```sh
NI_BUILD_CACHE=off NI_BUILD_RETURN=logs ni-build cargo run -p ilium-agent-session --example polling_candidate_evaluation -- --jsonl
```

The verified comparison produced 78 case rows and six summary rows:

| Window events | Filter | Labelled candidates selected / 5 | Benign cases selected / 8 | Labelled candidates missed |
|---|---|---:|---:|---:|
| 10 | Off | 5 | 4 | 0 |
| 10 | On | 3 | 2 | 2 |
| 15 | Off | 5 | 4 | 0 |
| 15 | On | 3 | 2 | 2 |
| 30 | Off | 5 | 4 | 0 |
| 30 | On | 5 | 2 | 0 |

The short filtered windows miss the two widely spaced scenarios. Changing
progress and a purposeful bounded retry remain extra review candidates, not
confirmed findings. These hand-labelled synthetic cases establish a regression
comparison only: they do not measure real-world accuracy, provider classification
quality, token cost or an optimal threshold. Defaults remain provisional until
independent representative traces and fake-provider end-to-end evaluation pass.

## Leader runtime

`polling_leader` runs only in the elected leader. Every two seconds it follows
the saved settings, discovers live session endpoints, and keeps one
`PrivateConnection` per serving session (reconnect after five seconds). A
connection proves the serving peer, subscribes with the leader epoch and
revalidates both proofs before each evidence batch. Batches enter a bounded
64-entry queue; a full queue drops the subscription, and the reconnect baselines
again and records an explicit gap, never an invented continuity.

Batches feed `PollingWindows`. A window that is due passes the local candidate
filter, joins the fair scheduler (two in flight, 128 pending, one per agent) and
is dispatched on an admitted I/O worker through `polling_dispatch`: durable
ledger reserve, send, then completion from the returned usage. Missing usage is
an unknown charge, never zero. 401/403/400/404, a missing key or a rejected
endpoint pause dispatch until the configuration changes; 429 and 5xx back off
exponentially with jitter (honouring Retry-After) and give up on a window after
three failures; a reached budget blocks until the month changes. Per-agent
check times persist in `polling-checkpoints.json`, so a detector restart cannot
cause a burst of paid checks. Findings are published to every client through
the server projection; the leader never writes to a pane.

Assumptions: only the all-sessions scope is honoured because the current-session
and excluded-project selectors have no evidence source yet. A 429 or 5xx attempt
stays an unknown charge because the ledger has no known-no-charge transition for
a dispatched attempt. A finding's ownership is always shown as unknown.
Verification uses isolated state and a fake provider through the loopback-only
`ILIUM_POLLING_DECISIONS_ENDPOINT`; a real paid evaluation requires an explicit
request.
