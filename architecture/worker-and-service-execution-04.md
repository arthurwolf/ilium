# Worker and service execution — continued 4

receipt releases its separate storage admission. Diff encoding streams against
the last emitted frame without a whole-frame diff vector. Isolated release tests
cover refusal before spawn, blocked retirement and storage after actual join;
full-client output and measured memory qualification remain open.
Successful presentations expose `request_to_emission_us`,
`composition_to_emission_us`, `completed_frame_age_us`, terminal `frame_age_us`
and `emission_us`. Those acknowledgement-side measurements include the owner’s
flush boundary; rejected or uncertain writes are not successful presentation
samples. The matched before/after release measurements still need to report
p50/p95 input latency and frame age, client/server CPU and peak RSS for cached
playback, live playback and a large document workload.

Snapshot publication flushes and syncs the temporary data file, then uses the
platform durable replacement operation before reporting success. On Unix that
operation also syncs the containing directory; Windows uses write-through
replacement. Existing snapshot persistence readback and oversize-preservation
tests pass on Linux; crash/power-loss and Windows execution remain unverified.

The ordered snapshot disk service now reserves one physical worker from the
existing server execution quota before constructing its channels. Its declaration
includes a requested 2 MiB stack, the existing 64 MiB service allowance and bounded
command/path metadata; these are admission declarations, not measured RSS bounds.
The native supervisor retains that reservation through actual thread join even
after logical service drop. Production startup fails explicitly if the session
execution owner is absent; independent test roots create no additional execution
pool. Captured-source Linux qualification passes all12 snapshot-service tests, including
blocked retirement, exhausted worker admission and retry readback. This establishes
the tested service contracts; release and loaded-runtime qualification remain open.

Presentation acknowledgements retain the exact sources and hit targets actually
painted in their complete frame. Selection, context menus and Smart Copy use
those emitted sources. Detach consumes output acknowledgements while draining
ordered parser work, releasing geometry leases without repinning a finished
interaction; successful animation receipts still reach their exact scene.

Cost derivation and detail-card formatting use one finite CPU receipt on the
existing statistics execution client. Captures contain bounded topology IDs,
settings and immutable statistics identities. Structural revisions fence
installation independently of newer time requests. Installed statistics and
history snapshots carry independent storage leases, releasing finite job slots
after collection. The coordinator owns history scan receipts and drains them
before CPU admission, including when cost display is disabled or CPU work is
still pending. CPU derivation reads an immutable history snapshot. Cache-path
changes immediately invalidate queued or completed scans and fence captured CPU
generations with a separate epoch; ordinary cancellation preserves the previous
complete snapshot. Shutdown releases coordinator references before execution
drain, while independent readers retain the original storage lease. Frozen-source
checks cover10 history tests and26 cost-tracker tests, including queued/ready
cache changes and last-reader retirement. Full engine destruction, current-client
runtime and measured memory qualification remain open. Tree-row acknowledgements
retain the prepared card, its
displayed title and visibility policy; rendering borrows those immutable lines.
Cost CPU admission additionally records the actual rejecting ledger's quota
boundary, requested bytes and used/limit values from the existing admission lock.
The UI diagnostic and retry policy stay unchanged; repeated ticks with the same
diagnostic do not repeat the warning. Refusal leaves the engine in its coordinator,
and a later submission refusal returns the exact engine as before. This diagnostic
change uses the same client, bank and quotas. Scoped formatting passes; its fresh
cost/history/client checks and reproduction of the runtime refusal remain pending.
A changed acknowledged card requests one redraw, while an unchanged source
does not create an acknowledgement/redraw loop. These source contracts still
need integrated compiled and runtime qualification.

Icon search has one supervised stateful owner per client, one replaceable pending
query and one immutable result mailbox. Result storage remains charged through
its last consumer. ONNX uses one execution thread and tokenizers parallelism is
disabled before initialization. One nonblocking OS lock (`engine-0.lock`) in the shared
model-cache namespace limits concurrent engines across clients sharing that cache;
separate cache namespaces remain independent. The512 MiB native declaration
and cold/warm model memory still require measured qualification.
Production currently constructs one icon owner, but the public constructor does
not enforce process-singleton ownership: separately constructed instances retain
independent worker slots while the composition-root declaration budgets one.
Independent mock owners exercise shared quota accounting; production engine
exclusivity belongs to the cache lock, not a per-role quota restriction. Native
worker admission retires at physical join even when observation tickets remain.
Additional in-process production owners and their aggregate memory remain
unqualified.

Media pause/resume effects have one persistent OS-thread owner. Normal voice
and demonstration mode publish bounded desired-state leases; the owner keeps
the exact positively acknowledged paused-player set and compensates a Stop
that arrived while Pause was blocked. Shutdown observes actual supervised
thread exit. The Tokio-backed zbus configuration avoids its extra internal
executor thread; its declared native/message peak still needs runtime proof.

Ambient helper `Worker` owners use that same platform supervisor. Scene drop
signals their existing cancellation flags without joining on the scene thread;
there is no separate ambient reaper queue. The supervisor retains blocked
callbacks until actual exit. Its registry limit covers live and retiring owners,
but shared process admission for those helpers and their native libraries is
still required; this lifecycle change alone does not supply their memory bounds.

Independent immutable caches use `StorageAdmission`: it debits declared storage
without inventing another physical thread. The last retained snapshot keeps
that storage debit even after its producing worker joins. Persistent actors
reserve mailbox/receipt metadata separately from that immutable allocation.
Typed channel adapters can split `Retained` into its original value and an
opaque `Retention` guard. The destination keeps that guard after its payload;
cloning a guard shares one charge and cannot authorize new heap allocations.

Durable Text Trigger reads use the server's bounded I/O bank and accept only
regular files within512 KiB. Regex validation uses its CPU bank. An immutable
settings allocation receives a separate storage lease before the read/validation
job envelope is released; the accepted state holds the lease until replacement.
Releasing the read envelope before CPU admission avoids a deadlock when several
completed reads would otherwise fill all result credits. Invalid or oversized
sources retain the last accepted settings and revision. Per-pane matching uses
resumable bounded CPU steps and retains its operation and engine state through
admission pressure; decisions flow into the leased ordered delivery queue below.

Matched Text Trigger deliveries retain their own storage leases in the64-item
FIFO. A full queue waits rather than discarding an already matched decision;
natural PTY EOF drains admitted deliveries. Each rule carries `delay_seconds` (default 60,
max 86400, `#[serde(default)]` so rules stored before the field existed load with
60). The per-pane delivery owner holds a due-time heap (at most 64 pending,
then the channel backpressures); a delivery is sent at detection time plus its
delay. Semantic writes check the original PTY instance after acquiring its input
gate and that the rule (id, enabled, message) is still current; unrelated
settings edits no longer cancel pending sends. Pane replacement and rule
removal, disabling or message edits cancel stale decisions explicitly. The
existing output sampler can still lose transient matches after broadcast gaps;
screen resynchronization does not establish lossless trigger observation.

The server's CPU bank has process-shared decoder and encoder tenants. Four
request envelopes cannot consume the two output envelopes; general evidence
and I/O jobs have separate tenant ceilings. The aggregate declared input and
result ceilings are each 1280 MiB, with general input512/result128 MiB and codec
input128/result192 MiB per admitted job. These bounds are shared across
connections and retain charges while handlers or stream flushes are blocked.
They describe admission envelopes, not measured resident memory.

CPU preparation adapters can query immutable job ceilings across the shared
quota root and every tenant ancestor. Disabled CPU workers or zero job allowances
produce permanent capability refusals. Callers still reserve capacity atomically
to handle current pressure and shutdown races.

The client owns interaction, layout and composition. Its animation service owns
scene construction, simulation, rendering and packing, and returns immutable
frames with native glyphs and styles. Each scene OS owner debits the same client
process thread quota through actual join, including blocked retirement. Packing
reserves a24 MiB frame envelope before allocating cells; the final snapshot
clone releases its independent storage charge even after the engine has joined.
The existing four-engine and three-frame limits also apply. This accounts for
owner threads and published frames; opaque scene caches and library-created
threads require their own domain accounting and do not have an RSS guarantee.
A composed frame retains its exact sealed
scene receipt until terminal emission succeeds. Time requests may replace older
render requests without invalidating the latest complete frame; semantic scene
and settings changes remain ordered. The surface configuration queue is capped
at 16 entries and reserves its final slot for the ordered pause emitted when a
scene host is released; when its 15 ordinary entries are occupied, a new
semantic change is refused without advancing desired settings or revision. One
supervised OS thread owns the terminal backend, diff base, encoding, output,
flush and restoration. The UI composes on an
inert buffer and admits at most two complete immutable frames, each with its
cursor and layout revision, before handing them to that owner. A frame is diffed
against the last successfully flushed buffer; a size change clears the terminal
and establishes a full-frame base. `PreparedFrame` transfers the existing cell
vector without compacting it on the interactive thread; admission charges its
retained capacity and a conservative allowance for visible symbol bytes. The
output writer and retained frame storage have explicit byte ceilings. Only a
successful flush advances the diff base and returns a presentation
acknowledgement; an uncertain partial write is reported
without retrying its prefix. The UI commits mouse geometry from that exact
acknowledged frame, including its viewport and terminal instance, so provisional
layout or pane replacement cannot redirect admitted input. Shutdown drains
accepted frames and restores terminal state on the output thread, while the UI
observes physical thread exit asynchronously. The output worker reservation
covers its bounded encoder, diff-mask buffers and an explicitly requested 2 MiB
native stack. This is a declared resource bound; actual platform stack commitment
still requires runtime measurement.

Document preparation is keyed by instance, source revision, width and settings.
Rendered-source capture uses the same interactive turn budget as source windows
and syntax capture: at most256 KiB and1024 lines in aggregate, with each pane
receiving at most64 KiB per turn. A partial capture retains its original job
admission and typed source retirement. Before the next chunk, the full key is
checked again; a changed editor instance, revision or layout retires the partial
source instead of mixing revisions. Whole-source size checks occur incrementally
rather than scanning the entire buffer on the interaction loop. Cross-line
syntax, Markdown layout and font preparation run on the finite banks. The
ambient slideshow retains one supervised, charged loader coordinator for
ordered requests; directory/local reads and URL fetches use the shared I/O
bank, while generic codec metadata inspection and decoding use admitted shared
CPU jobs. Generic decode reserves a conservative 36 bytes per source pixel,
encoded-copy/name/metadata costs, and a separate non-strict decoder allowance;
its output and retained error are also admitted. Cancellation reaches the
ordered loader and the finite jobs, and bounded readers check it during input.
These are source-level ownership contracts; current-source image tests, full
release qualification, native peak-memory measurement and live acceptance
remain open. The new document-capture regressions are authored but not yet
executed; existing whole-source limits and the separate TextArea bulk-mutation
gap remain open. Ordered file writers acknowledge durable publication before
clearing dirty state, retargeting Save As or dismissing a save dialog. Editor,
board and configuration consumers own conflict and rollback decisions; a failed
acknowledgement never proves that a rename was not published. Directory sync
reports unsupported platforms explicitly. Detection workers gather owned process
and transcript evidence; the server reconciles current revisions and exclusive
session claims centrally. Transcript discovery now stages bounded path and file
reads on the I/O bank, metadata decoding on the CPU bank, and prefix charging
and project verification back on I/O; batch, line, fetched-byte and staged-job
limits are shared across discovery ranks. The coordinator preflights retained
claim/evidence allocations, orders pane claims, and rechecks revisions, process
identity and applicable project cwd before publishing. Process-table refresh,
shell-ownership observation, process discovery and final identity probes run as
admitted I/O work; CPU classification uses the cached process-table view and
captured screen snapshots. The CPU stage still holds the cached-table mutex while
building its child index and classifying panes, so table refresh waits for that
bounded stage to release the guard. Current-source integration and runtime checks
remain necessary to qualify ordering and latency.
Bounded title-evidence reads reuse the locator's per-attempt line and byte
budget after metadata verification, and a refused record leaves evidence
unavailable rather than authorizing a title. Current-source test and release
qualification remain pending.

Server foreground-process evidence now uses the existing finite I/O workers.
Five observation paths capture shell/process evidence, release shared tree and
pane guards, then await a bounded native probe; title and input decisions
revalidate the captured PTY, session, agent and presentation fences before
applying results. The sixth path probes shell ownership within the detection
evidence worker after releasing its cached process-table guard. Refused,
unavailable or late evidence does not grant shell ownership. The native adapter
still uses ToolHelp on Windows and the narrower terminal foreground-process-group
query on Unix. This describes the current source; it does not establish Windows
runtime behavior or replace current-source integration and runtime checks.

Local PNG preparation uses the shared finite CPU bank. Its concrete decoder
preflights bounded chunk metadata, inflater buffers, pixel storage and resize
scratch before construction; cancellation checks also cover encoded-source reads.
The original encoded source and prepared pixel/name storage retain their admission
through queued results and the final scene or cache consumer. These declarations
bound owned work and storage; they do not establish a process-wide RSS limit or
qualify other image formats and remote transport.

Terminal parsing has an ordered OS owner and immutable UI snapshots. Byte,
replay, resize, scroll and negotiated-input barriers retain pane identity and
stream order. Smart Copy captures a matching historical viewport before later
output can change it. IPC framing separates fixed-size header reads, bounded
payload admission and pure encoding/decoding; idle connections do not reserve a
CPU job while waiting for a header. Actual stream flush establishes presentation
or transport delivery, while persistence and server application use their own
acknowledgements. These components are workers and services; the future
user-written JavaScript extension system has a separate contract.

Producer-side admission for server events remains incomplete. `ServerState::broadcast` fans out owned events before byte admission. Initial attach, lag recovery and visible-pane replay now estimate terminal history, reserve storage outside pane locks, revalidate the PTY session and sequence, and pass the lease through the direct connection queue into the socket writer. This caller integration is awaiting current-source compilation and native verification. It does not bound the earlier Tree snapshot clone or general broadcast allocations, and the raw event queue still admits unguarded nonterminal payloads.

Light Smart Copy reserves a position in the existing ordered clipboard service
before moving its original selection to CPU preparation. Later clipboard reads
and writes cannot pass that position. Prepared text and cached preview facts
retain their storage admission; successful native acknowledgement starts the
preview countdown. Preparation or write failure restores the original selection
for an explicit retry or cancellation. Shutdown drains acknowledgements and
returns any unresolved original through typed custody before the execution bank
closes. These source changes are integrated; their new recovery and ordering
fixtures are awaiting client qualification. Model-assisted selection preparation
and whole-editor bulk mutation remain separate open inventory items.

Process bootstrap now designates the shared quota before logging starts.
The supervisor keeps its permanent physical admission; the ordered logger keeps
its worker admission through actual join and its storage admission through the
last retained owner. The server creates `ServerResources` before logging and
moves that same quota and completion notification into its finite worker bank.
Independent test banks retain their own quota identities. A Linux qualification
forces a valid native thread request to return kernel `EAGAIN` in an isolated
child, verifies registry/custody rollback, restores only that child's soft limit,
and verifies a later real worker joins. This complements the separate actual
quota startup test; it does not prove all native library or process resources.

The interactive client now reuses its seven-thread finite bank (two CPU, four
I/O and one service thread) for codecs and has an admitted terminal-input owner.
Codec jobs also share process-wide encoder and decoder admission groups, each
limited to two jobs and 128 MiB of input and results. The current focused test
covers two interactive clients sharing one bank; separate-bank sharing is not
qualified yet. Before constructing either existing Tokio
runtime, the process root reserves its two async and four blocking roles with
2 MiB requested stacks. The supervisor retains this fixed runtime declaration
for process lifetime: a timed runtime shutdown can leave blocked callbacks alive,
so dropping a local guard would release capacity prematurely. Same-root repeated
initialization is idempotent; a foreign root or changed declaration is refused.
The selected-client census includes one external-opener reaper and derives a
38-role ceiling from the current seven-thread bank plus its named owners, within
the unchanged 4096 MiB storage allowance. Current full-client qualification is
still required; standalone bank admission is not whole-process acceptance. Foundation qualification
covers runtime admission/refusal, idempotence and retained process custody;
captured-source server and CLI all-target compilation also passes. The later
client changes require their own qualification; no installed-runtime claim follows.

The shared local ordered writer can retain an accepted FIFO head while its
original source is prepared on the existing CPU bank. Its typed readiness
predicate runs before I/O submission; a younger ready write cannot overtake the
head, and waiting for preparation occupies no I/O worker. Existing editor, board,
configuration, integration and permission writers retain their default ready
behavior and original completion wakes. Eight isolated native writer/snapshot
regressions pass, including independent I/O during blocked CPU preparation,
ordered durable readback, shutdown and admission provenance. The full editor
model-loan, deferred save acknowledgement and source-capture caller integration
remains under current-source client verification; this boundary proof does not
establish workspace, release or installed-runtime acceptance.
In the current App path, `enqueue_editor_save` loans exclusive editor-model
custody to filesystem preparation and installs an identity-bound placeholder.
`EditorFiles::save` admits a bounded CPU measurement job before scanning the
lines; after measuring exact writer cost and admitting the ordered write,
`CaptureEditorSave` clones the immutable save revision on a CPU worker and
returns the editable model. Ordered writer acknowledgements and identity and
revision checks preserve save ordering and dirty/error semantics. This source
path is integrated; current-source Cargo, release and installed-runtime
qualification remain unverified.
Board source load/create already runs in the I/O job, and attached boards publish
through their ordered writer with acknowledgements and rollback custody. The
caller validates at most 1 MiB and 8,192 cards, then clones the board columns
inside the admitted job builder; this bounded UI preparation is materially
smaller than editor capture but is still caller-side work. Keep the board's
revision, conflict and rollback semantics when reducing that capture cost.

Configuration persistence already uses one ordered I/O writer for settings,
agent setup, text triggers and project animation values. Durable acknowledgements
gate dialog completion and saved-state publication; failed writes remain
visible as unsaved/errors, with session and agent-setup conflicts reconciled by
readback on the I/O worker. Admission currently normalizes selected values by
bounded JSON round-trip (64 KiB serialized input) and checks retained command
size on the caller before submission. This keeps disk work and merge/readback
off the interaction loop, while bounded serialization/preflight remains caller
CPU work; do not describe it as fully offloaded.

The snapshot service reserves its persistent OS worker and declared bounded
mailbox/write storage on the server's existing root before spawn. Its physical
admission lasts through actual join. Failed coalesced writes restore the dirty
obligation and reuse the existing750 ms debounce/notification instead of waiting
for a new user mutation. The ordered writer retains earlier failure history even
when a later retry succeeds. Captured-source qualification passes the shared-root and retry regressions,
including authoritative persistence readback. The write envelope alone does not
qualify native JSON or legacy YAML read/migration peaks; result custody and parser
admission have separate owners and qualification receipts. Startup normalization now detects a legacy
project wrapper from the root project predicate instead of cloning the entire
tree for equality comparison. Both launch-project and agent-resume normalization
still run. Its frozen-source qualification passes 30 persistence tests (two ignored),
server all-target compilation, strict Clippy and scoped formatting. These receipts
predate the later result-retirement changes.

Snapshot reads now return typed owned results with a separate storage lease from
the existing server quota. The operation reservation can be released at delivery
while pending recovery retains the decoded result. Restore transfers the same
lease to server state before its first await; queued snapshot capture handles
also retain it when they outlive that state. No second worker or quota is created,
and a pending recovery decision does not consume the write-operation budget.
Two new fixtures cover storage after actual worker join and storage refusal with
original-file readback. Frozen-source qualification passes 14 snapshot-service tests,
30 persistence tests (two ignored), server all-target compilation, strict Clippy
and scoped formatting. Source and log hashes were independently verified. Decoder
preallocation remains open, so this change does not establish a complete
read-memory bound. Production read results now also reserve a destruction
envelope on the existing CPU bank before decoding. Discard and cancelled
acknowledgements hand the original result and its same storage lease to that
bank; no new worker is created. Shutdown releases an unchosen pending recovery
result before closing the bank. Two additional fixtures park both CPU owners
and force retirement-slot refusal to check retained custody and unchanged disk
bytes. Their frozen-source qualification passes all 16 snapshot-service tests,
30 persistence tests (two ignored), server all-target compilation, strict Clippy
and scoped formatting. Source/log hashes match the capture; these receipts predate
the newer JSON admission and overload changes. Partial-restore cancellation and
exceptional shutdown custody remain unqualified.

The read owner additionally admits the actual restoration future frame before
decoding, using its compiler-derived size and the existing retirement metadata
allowance. Restore polls that owned frame on the coordination runtime; cancellation
hands its remaining input, state reference and storage lease to the existing CPU
destruction owner. No async task is claimed as CPU offloading. An unexpected
missing owner retains the original recovery result and refuses successful
resolution. Two fixtures cover cancellation before tree publication
with both CPU workers parked, and frame-admission refusal with exact disk readback.
Frozen-source qualification passes all 21 snapshot-service tests, including these
fixtures. An independent audit verifies 1,792 source files and eight logs. This
does not establish semantic recovery after a partially published restore, old-tree
destruction bounds or exceptional shutdown safety.

A single recovery owner now retains the pending original and one accepted
restore/discard coordination task independently of the requesting socket.
Admission reserves the compiler-derived task frame before transferring that
original. A refused restore or failed ordered discard returns the same original
to the retry slot; direct reply backpressure does not own semantic completion.
Attach waits for resolution, workspace pruning protects an unresolved operation,
and session kill or shutdown closes admission and joins it before clearing the
tree or draining final persistence. Native snapshot I/O and CPU destruction keep
their existing owners. Four real-bank regressions, server all-target strict
Clippy and formatting passed against 1820 unchanged captured files; the five
recovery files are applied. The server release build also passed against the
same 1820-file capture, with independently verified source, log and retained
binary hashes. The artifact is retained separately and is not installed; current
paired runtime acceptance remains outstanding. This release predates the
failure-handling proposal below.

Recovery failure and snapshot shutdown changes are now applied through exact
current-source deltas across seven server files. Accepted recovery failures latch
an error and keep the original-file write fence; KillSession returns an error
without clearing the tree or acknowledging success. A lexical dirty-claim guard
restores pending work on cancellation, unwind or failed write acknowledgement.
Shutdown cooperatively stops and physically joins the original coordinator
before its final conditional flush. Join failure remains latched across cancelled
and repeated shutdown attempts; clean discard and clean startup do not force a
save. Original native regressions reproduced both lost cancellation claims and
missed failed-write shutdown retries before the remedy. The frozen candidate
passes 11 recovery, 25 snapshot and one task-handle cancellation test, strict
server Clippy, scoped formatting and a private release build; all 1820 source
hashes and six log hashes were independently verified. Its new cancellation
case checks actual persisted readback and sticky failure after repeated drain.
The seven applied files match those qualified candidate bytes and pass formatting.
A fresh current-source gate additionally passes all 38 native cases, including
the actual KillSession failure handler, strict server Clippy and scoped formatting;
all 1850 captured source hashes and six log hashes were independently verified.
That gate built no release. The earlier private release is not installed, and
current paired runtime, whole-workspace and performance acceptance remain
outstanding.

The session shutdown sequence now drains accepted workspace semantic tasks while
keeping their existing execution bank open, then flushes and shuts down the ordered
snapshot service, and only then requests bank cancellation. The same production
drain helper is used by an authored regression that parks both actual CPU workers,
queues an accepted semantic mutation, and checks authoritative snapshot readback
after shutdown. Its first qualification failed before execution because the
fixture called a nonexistent receipt method. The test now uses the foundation's
existing `try_take` API and passes scoped formatting; fresh compilation and
execution remain outstanding. Recovery still runs inside the request
handler: ordinary socket closure does not immediately cancel that awaited handler,
but the server aborts connection tasks during normal shutdown. The ordering change
does not preserve a partially published restore across that abort or establish
old-tree destruction bounds. The production startup branches load at most one
snapshot per server lifetime; repeated replacement of the read-storage lease is
a future contract concern, not an observed production reload path.

Native JSON loading now uses the shared allocation-checked Serde visitors before
owned strings and collection growth, while preserving trailing-input rejection.
The caller admits parser scratch separately before queueing the read on the same
server root. Its conservative declaration covers the installed JSON parser's
scratch buffer and old/new Vec capacity coexistence; that lease releases after
the decoder actually returns, while the decoded result keeps its own lease.
Resource-admission errors have a typed snapshot error. Recovery startup returns
that error instead of starting a fresh writer that could overwrite the original.
Operation-capacity and queue-overflow refusals use the same typed error. Snapshot
worker construction failures also use it because construction admits/spawns a
worker before any disk operation. Three later fixtures force operation,
parser and worker-creation pressure, verify original-file readback and retry.
Their frozen-source qualification passes 19 snapshot-service tests, 32 persistence
tests (two ignored), 47 IPC tests (two ignored), the startup-refusal test, server
all-target compilation, server/IPC strict Clippy and scoped formatting. Independent
hash audit verifies 1,792 source files and eight logs. These receipts predate the
restoration-frame changes and legacy encoded-size refusal parity.
Four new fixtures cover the shared decoder, native refusal and readback, trailing
JSON, and actual refused server startup. Their frozen-source qualification passes
47 IPC tests (two ignored), 16 snapshot tests, 32 persistence tests (two ignored),
and the startup-refusal test, plus server all-target compilation, server/IPC strict
Clippy and scoped formatting. All 1,792 captured source hashes and eight log hashes
match. These receipts predate the newer overload and cancellation-frame changes.
Custom deserializer conversions remain an audit obligation; the shared visitors
are not a generic allocator or RSS guarantee.
Norway's eager YAML event/alias graph, migration coexistence, normalization
scratch and partial-restore/shutdown custody still need separate bounds.
Legacy YAML decoded values now use the native snapshot's allocation-checked
Serde policy, so compact repeated aliases cannot bypass collection/string/depth
admission. Refusal returns a typed recovery resource error before conversion or
native publication; malformed YAML keeps its original codec error. Two standalone
exact-LegacyWorkspace tests reproduce the original unchecked expansion, then
verify allocation refusal and ordinary decoding with the real IPC/Norway crates.
The production disk readback, no-publication and valid-alias retry fixture is
authored but unrun. This change does not bound Norway's eager event graph, custom
Serde conversions or migration/normalization coexistence. Integrated qualification
still needs current persistence source.

Legacy YAML encoded-size refusals now use the same typed resource error as native
snapshots, including an early file-metadata check and the bounded reader's
post-read growth check. Startup therefore refuses recovery rather than treating
an oversized legacy source as an empty session. A new fixture
checks exact original-file bytes, absence of native publication, and later valid
migration with native readback. Frozen-source qualification passes that fixture,
all 33 persistence tests (two ignored), 47 IPC tests (two ignored), the startup
refusal test, server all-target compilation, IPC strict Clippy and scoped formatting.
The owned refusal transfer now uses `ControlFlow` to retain the original snapshot
without another allocation. The subsequent captured-source qualification passes
22 snapshot tests, 33 persistence tests (two ignored), 47 IPC tests (two ignored),
the startup refusal test and the full server library (435 passed, nine ignored),
plus server all-target compilation, server/IPC strict Clippy and scoped formatting.
The independent audit verifies all 1,792 captured source hashes and nine log hashes;
actual controller exit is zero. This includes the semantic shutdown regression:
accepted work drains before the final durable snapshot and execution-bank stop.
Concurrent startup-dialog and hidden-terminal delivery changes arrived after that
capture, so these receipts do not qualify the current workspace union. YAML parser
peak memory and semantic restoration after partial publication remain unresolved.

Location search now uses one shared one-job tenant per `ClientExecution` on the
existing finite I/O bank, replacing its separate four-thread gate. Picker replacement
cancels delivery while a blocked callback retains its physical job debit. Typed
outcomes and input revisions fence late results; result and selected-candidate
allocations keep a separate process-root storage admission after job retirement.
Provider error strings follow that same storage contract through display.
Query capture is bounded at 64 KiB without truncating the authored input; configured
address-provider validation and cooldowns stay in their existing adapters.
The address-search 429/503 cooldown is currently process-local; the shared host
lease coordinates request spacing across processes but does not publish that
cooldown. Cross-process cooldown propagation is not currently guaranteed or
qualified.
The new real-bank forcing tests and private prompt reconciliation are unqualified.
The shared `ilium-http` adapter now keeps DNS on the already admitted caller at
all nine existing construction sites (seven formerly using the default resolver,
two map callers using the previous local owned resolver). Locked ureq 3.3.0 spawns
an untracked DNS helper for finite resolver timeouts; the shared resolver selects
its synchronous lookup path and reports an expired deadline after lookup returns.
Native lookup cannot be forcibly cancelled, so a blocked worker retains custody
and admission. Caller proxy, redirects, cooldowns and request settings remain owned
by their existing adapters. Captured qualification passes both resolver forcing
tests, checks and strict Clippy for the HTTP, ambient, Wikipedia, inference and
gateway crates. Its import-format gate failed and was corrected afterward; newer
client integration remains unqualified. These checks establish no measured
process-thread or latency bound.
Location confirmation now carries an allocation-free marker into App. Destination
storage is admitted before normalization and copying into authored settings, save
intents and picker receipts. Version leases follow acknowledged, failed and rollback
settings after picker disposal. Semantic cache misses reserve separate storage
before cloning authored and derived settings; each independently retained view
keeps that resolution's charge, avoiding unlimited escaped views under one fixed
version declaration. Admission refusal leaves the original picker and prior cache
available for retry. Declared copy/scratch allowances are cooperative accounting,
not measured RSS bounds. Five quota, writer-refusal, persistence and retained-view forcing tests pass in
the captured client Cargo run, together with test compilation and package formatting.
The tested Root-owned seams match current source at audit; two concurrently changed
statistics files keep whole-current client integration and release qualification open.
Picker-list admission alone does not qualify downstream consumers.

Startup-dialog observations now use the existing filesystem I/O client rather
than reading a file on each UI-loop pass. One active receipt and one immutable
charged observation retain their original admission; reads are paced at 100 ms
and fenced by the startup-file path. A missing, malformed or oversized record
uses the existing indeterminate display. The reader accepts at most 64 KiB,
