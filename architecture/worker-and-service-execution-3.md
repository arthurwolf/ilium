`Client.try_reserve_detailed` returns the same finite reservation with typed
refusal evidence. A failed quota check records the requested increment, observed
usage and limit for its actual dimension while the admission gate is held. The
boundary identifies the process root or an ancestor distance from the requesting
client, so a free filesystem child can report a refusal by its shared parent.
These immutable numbers describe that check; concurrent releases can make a
later usage snapshot different. Lifecycle, queue and busy-gate refusals carry no
invented quota observation. The reason-only entry point delegates this admission
path and preserves existing retry classification. Ordered filesystem writers
add typed evidence for their own 32 pending-write and 64 MiB retained-byte
limits, overflow, closed state and identifier exhaustion; execution refusal
keeps the rejecting ledger's evidence. Owned admission rejection returns the
original job. Lazy configuration, editor, board and integration adapters show
this distinction without changing capture order, FIFO, persistence or quotas.
The reason-only writer entry points use the same admission path. Seven focused
writer tests pass in the frozen client qualification; full current-workspace,
release and live checks remain outstanding, as do other admission adapters.

Terminal output and replay use a closed, single-copy byte adapter in the IPC
contract. Its bincode representation stays identical to the original byte
vector: a fixed-integer length followed by contiguous bytes. The bounded
decoder borrows that slice, admits its exact output allocation before copying,
and rejects nonborrowing callbacks. This lets the unchanged 32 MiB PTY journal
(including a truncated replay reset prefix) fit the existing 64 MiB decode
limit. Generic collections and user-input byte vectors retain their
conservative growth accounting; raw output ordering and replay watermarks
are unchanged.

`Retained<T>` carries its admission debit through clones and asynchronous
transfers. `Reservation::retention()` shares the original debit during preparation
and refused publication without admitting another payload; the guard alone
does not move destruction to a CPU worker or prove native retirement.
Keeping an error, result or receipt can keep that debit alive after
the callback finishes. Bank storage stays charged while a monitor or client
retains it after workers exit. Platform-owned join supervision retains physical
worker admission through actual thread exit, including blocked native calls and
thread-local destruction. External library workers need explicit
`WorkerAdmission`; a process-local quota does not impose a host-wide limit or
prove an allocator/RSS bound. Declared peak costs and native library parallelism
must be audited at each producer.

`Execution::retirement()` pre-admits up to 64 payload envelopes independently
of ordinary document-job slots. `Retiring<T>` moves a last owner onto the
existing CPU bank; `RetiringArc<T>` gives independently cloned leaves the same
contract. The original storage and job guards stay inside the envelope through
actual destruction. A separate finite recovery mailbox preserves the original
id, type, allocation and retry sender when publication fails. A connected
channel alone does not prove a CPU consumer remains: the last CPU body rescues
stranded entries, and shutdown reports incomplete while recoverable owners
remain. Callers must retain the execution/recovery owner through their final
frames and acknowledgements. These primitives are implemented and have focused
forcing coverage; editor, clipboard and context-menu ownership migration remains
unfinished. Queue admission and declared bytes do not establish a native RSS
bound or guarantee destruction after every permitted execution owner is lost.
An isolated native forcing test drops the combined error and execution bank
while the CPU callback is blocked. After release, both original 64 KiB payloads
retain their addresses and contents; the outer and two nested destructors run
on that same CPU worker. The test verifies all three retirement envelopes are
released and physical thread admission returns only after native exit. It keeps
client and observation capabilities through the check. This qualifies that
disposal sequence, not arbitrary owner loss or complete client shutdown.

The remaining editor migration must cover each independently retained leaf,
including syntax tokens, rendered documents, context text and TextArea undo/yank
storage. Charging a preparation result does not cover a separately cloned body
or defer its last-reader destruction. Context operations capture editor instance
and revision before preparation; admission precedes owned prompt and text copies.
Refused pane creation retains the original editable draft and captured parent,
and records focus only after publication. Editor copy dispatch now submits to
the same bounded clipboard owner and completion acknowledgements as terminal
copies, reporting success only after acknowledgement. The narrow caller is
applied; its frozen candidate compiled and passed the existing clipboard tests.
Current release and live acceptance remain unverified. The other editor
ownership requirements above remain incomplete; this caller change does not
qualify native memory or whole-context capture.

Source editor preparation uses the existing document CPU bank with a per-turn
capture budget, bounded viewport continuations and revision/width/cursor fences.
The UI composes immutable prepared rows and retains their geometry through the
matching presentation acknowledgement. Shutdown cancels preparation, joins the
presenter and releases its last editor frame owners before execution retirement
joins. This source-window integration is applied to the current tree; frozen
candidate checks do not qualify the newly combined tree or its release binary.
The complete editor engine also has an all-target type-check pass against a
frozen private union of the current callers. A corrected native run passed
13 original-ownership editor cases, one hidden undo/yank growth case and one
ordered durable-save readback case; all 1868 captured source hashes remained
unchanged. The original runner rejected a zero-test save filter, and the
corrected module filter executed the real readback test. During a CPU loan, the UI renders immutable acknowledged source rows or
already emitted Markdown cells and one captured set of chrome facts; it does
not read the loaned mutable document. The ordered save collector retains an
original durable completion while the same editor instance is loaned, then
settles that head before younger acknowledgements. The shared ordered writer
readiness and selected native loan/save tests pass. Combined editor/scene
shutdown forcing, the later complete caller union and whole-workspace strict
checks remain separate acceptance gates. These private qualification states
do not imply that the complete engine is applied, released or live.

Chapter line-offset discovery now counts disjoint source ranges rather than
repeated prefixes. Isolated original-versus-candidate chapter fixtures pass;
full-buffer context capture/parsing, TextArea storage and live acceptance remain
unfinished.

The document leaf migration is applied to the current tree: cloned syntax
tokens and rendered text retain their original physical storage charge until
the CPU retirement owner releases the last allocation. Its client build,
Markdown contracts and editor contracts passed, but the new forcing fixtures
initially declared job costs smaller than the captured types and were rejected
before exercising retirement. Corrected fixture declarations passed all six
preparation tests, including independent last-reader release and queued
cancellation, without weakening admission. Current combined compilation and
release acceptance remain unverified; TextArea and context ownership migration
is still incomplete.
The native input owner and complete terminal adapters are now integrated into
the current tree after producer/caller inventory and guarded three-way merges.
Input admission precedes terminal mutation. One admitted owner handles normal
input and terminal capability queries, retains refused semantic events, and
keeps its session claim through terminal restoration. Connection setup reuses
the shared codec bank instead of starting a duplicate bank per connection.
Shutdown errors retain undelivered events and live retirement tickets alongside
earlier client errors. The declared process limit includes these owners; it is
not an allocator or measured RSS guarantee. Frozen candidate checks cover the
selected contracts. Four real Linux PTY runs against the qualified debug native
input child now pass cancellation, release/retry, shared input/query ownership,
and an exact 40 MiB paste followed by a key. They verify physical join and zero
remaining admission; the large paste also verifies compacted capacity. The query
controller provides a synthetic primary device-attributes response only. The
original timeout and controller defects are retained alongside corrected proofs.
These runs do not qualify the Rust parent harness, whole TUI, release behavior,
RSS or matched performance. Ordinary pane-key forwarding still needs retained
retry custody when outbound request admission refuses; native reader preservation
does not prove that downstream contract. Current combined acceptance remains open.
Generic paste in character-oriented modes uses one ordered `PasteReplay`
continuation on the interactive owner. The continuation retains the original
admitted input envelope and storage lease, maps UTF-8-safe characters through
the existing key handler in batches capped at 128 keys, and keeps later input
behind the active paste while the event loop services other ready work. Native
terminal paste and protected input interception remain on their existing
single-event paths. The client reserves CPU retirement before replay; completion
transfers the original envelope to that owner for destruction, while interrupted
replay preserves the unchanged paste and reports the consumed byte prefix for
shutdown recovery. This path is integrated in source; current-source native,
release and interactive-latency qualification remain open.
The lower-level public `keys::handle_event` entrypoint still processes a direct
`Event::Paste` synchronously through its legacy key replay helper. The canonical
terminal loop captures paste into `PasteReplay` before calling that entrypoint,
but other direct consumers can bypass the continuation and are not covered by
the bounded-input claim. Keep those callers out of production dispatch or move
them onto an owner that can retain and advance the continuation before claiming
the API itself is bounded.
The parent input dispatcher now uses a fixed two-slot backlog, consuming retained
originals before fresh input and draining both slots during shutdown. Overflow
returns both incoming originals without changing existing custody. Seven native
checks cover actual input allocations, the production 64-event motion boundary
and the current dispatcher compiled against retained private App APIs. Both
captured current-client compiler modes now pass all-target type checks with and
without GPU support, plus input-module formatting; all 1850 captured source hashes
and three log hashes were independently verified. V8 download was skipped for
these compiler gates, so they provide no native or release execution proof.
These focused proofs do not qualify downstream semantic admission, the complete
current App or real TUI input latency. A separate private fixed instruction head distinguishes actual
editor installation from model acknowledgement, fences original allocation and
editor instance, and retains partial UTF8/CRLF offsets and terminal failures.
Five native-allocation boundary cases pass; their model facts are synthetic.
Two additional cases use actual shared CPU model callbacks: a partial Unicode
paste cannot release its original until the remaining bytes complete, and a
panicked model retains the same original for shutdown diagnostics. The first
also verifies a following key and actual committed text; both physically join
their test-owned worker banks. The 1882 captured source hashes and native logs
were independently verified. This head remains private pending complete App
caller, global/protected input precedence, completion and shutdown integration.
A private Save As adapter retains a CPU-prepared destination through admission
refusals and reuses the original editor source capture and ordered writer.
Two native cases verify the original CPU snapshot, exact destination allocation,
Unicode disk readback, model-return-before-write ordering and physical bank drain;
all 1882 frozen source hashes and the test log were independently verified.
These cases do not qualify the current App, dialog acknowledgement or full shutdown.
A later current-source capture with the private prompt callers failed its default
all-target compiler gate on missing producer APIs and caller contracts; its 1895
source hashes and diagnostic log were stable. The GPU gate did not run. Shared
editor input, final configuration-source release and whole-App shutdown integration
remain private and incomplete; the earlier compiler passes do not prove this union.
Private shutdown tests now exercise the actual CPU bank in debug and release:
original disposal, nested retirement, deadline ownership retention and retry,
and recovery with a target behind 63 originals of other types. They verify the
original allocation, CPU disposal, physical worker exit and released quotas.
The 48 frozen source hashes, locked dependency versions and four logs were
independently audited. The bounded recovery regression is also included in
`ilium-execution` tests. Its payloads are synthetic; these focused passes do not
qualify complete App shutdown, the current integrated client, or release latency.

GPU lifecycle integration is source-integrated but still unqualified. The
three scene backends now admit one frame worker with an explicit 2 MiB native
stack through `SceneEnv.resources`, reject jobs with more than 64 uniform values
or outputs above four million pixels before allocation, publish the latest
frame through a storage-admitted immutable reference, and retire blocked device
calls through the platform supervisor rather than joining on Drop. The optional
startup probe starts after the client obtains shared ambient quota, reserves
one worker and an explicit 2 MiB stack, and remains owned until client shutdown
requests stop and joins its original ticket. Admission refusal publishes a
failure status while leaving the one-time guard retryable. The GPU parity test
remains a separate startup-probe consumer.
`SceneEnv.resources`, the admitted ambient worker and the platform supervisor
now own scene frame execution and its logical output storage in current source.
The scene-worker checkpoint passed scoped format and diff checks; the subsequent
GPU-probe, deferred-runner and quota-source edits have targeted diff checks only
and have not compiled or run. Separate frozen-source qualification passed 13 ambient, 21 default GPU,
24 feature GPU and one parity test, plus strict all-target Clippy in both modes.
The parity run used an NVIDIA GeForce RTX 3090: 34 cases had zero differences,
and the ten-second temporal control changed 12.08% of pixels. That result does
not qualify current source or client lifecycle. Default NotCompiled behavior,
software fallback and matching-size frame presentation must survive.
Driver/device library roles, native buffer
lifetime and hardware acceptance need separate evidence; shared admission does
not account for GPU memory or measured RSS. While availability is `Checking`,
`ilium_gpu::runner()` now returns a stable deferred source that forwards to the
runner published after the probe finishes, allowing already-built scenes to
activate without rebuilding. The client retires the probe and frame owners after animation drain
and before closing their admitting execution resources.

Subsequent final-reader forcing tests preserved original pixel bytes and frame
credits through CPU disposal. That private hardware run passed parity assertions
but crashed during process teardown, so assertion success was insufficient.
The next private candidate tracks every original frame-worker ticket in a fixed
32-slot controller custody before spawn, fences close against reserved starts,
and drains actual scene and frame joins before releasing the probe/device. Three
RTX 3090 parity processes exited zero with explicit teardown, and 17 managed
GPU tests passed; default strict Clippy then rejected one indexed loop. The
corrected frozen source now passes 17 ambient ownership tests, 21 GPU tests,
real hardware parity, default and GPU-feature strict all-target Clippy, and
formatting. All six commands exited zero and its 490-file source audit found
no drift. This qualifies the private GPU engine; the combined client lifecycle,
release, terminal and matched performance checks remain outstanding.
The crash did not reproduce under the original-binary debugger run, leaving the
exact crash cause unresolved. Client startup, physical scene join and finite-I/O
frame-drain compositions remain private and require combined qualification.

Server notification audio uses one ordered actor with a bounded queue of 64
requests. Playback is a typed finite I/O job on the existing server bank; the
actor retains the original request while waiting for admission. A running native
callback retains its job charge after the actor is cancelled, until playback
physically returns. The original cancellation regression failed before this
change. A frozen callback-source capture passed 458 server tests, all-target
checks, strict Clippy, formatting and a server release build. Its release is
retained separately from the subsequent settings/config integration.

Queued request bytes and immutable sound-settings publication now retain their
own storage admission. Producers reserve before constructing new labels;
startup, IPC updates and config reload publish one immutable admitted settings
allocation, shared by queued and native readers until the last reader releases
it. The config watcher uses the existing finite I/O bank and one bounded
256 KiB read for both fingerprint and parsing. It preserves first-tick refresh,
last-good settings, refusal retry and independent text-trigger refresh. The
13 integrated files match the exact private capture that passed 467 server
tests, including nine required ownership/refusal/watcher regressions, all-target
checks, strict Clippy and formatting. The combined settings server release has built successfully from that frozen
capture, with actual supervised Cargo and runner exit zero. It is retained as
a separate artifact; current-source union and live installation remain unqualified.
Full notification queues retain explicit logged overflow;
DSP preparation preserves existing bounded synthesis and playback order. Library
and child-process memory remains separate from the cooperative allocation ledger.

The captured CLI/server release pair has passed an isolated real-PTY attach,
project/tree render, physical Help input, resize and Help-dismissal check, with
loaded executable identity and scoped shutdown verified. This does not qualify
the complete current-source union, the full TUI suite, GPU hardware, RSS or
matched performance, and the pair has not been installed.

Exceptional scene shutdown must retain the complete original animation owner,
including accepted configuration and receipt queues and unsettled replay proofs.
A native join ticket or an open execution bank alone does not retain those
originals after App destruction. The private client proposal reserves one
exceptional retirement envelope before scene startup, moves the whole surface
without copying its payloads, and bounds receipt-drain observation. A failed
editor and scene shutdown must preserve both original errors under the same
outer execution owner; GPU retirement and bank closure follow proven physical
scene exit. The new exceptional-path fixtures and combined proposal have not
yet passed compiler or native execution qualification.

External file, directory and URL launches now use the existing shared I/O bank.
One client owner retains at most eight ordered requests, with explicit target
byte limits and launch acknowledgements. A newer click never coalesces an
earlier launch. Refusal, OS failure and an unknown worker outcome produce
distinct feedback; shutdown drains accepted requests before closing admission.
Terminal-link confirmation also uses the existing acknowledged clipboard owner
for Copy. The UI moves each prepared launch message with its original charge.

The platform adapter owns one reaper with an explicit 2 MiB stack and 32 fixed
slots covering both in-flight spawns and running children. Argument arrays,
non-UTF-8 paths, dash-prefixed filenames, null stdio and Windows shell error
mapping remain at that boundary. Closing never waits for or kills a user
application; surviving Unix children retain the admitted service through actual
join, and surviving handles retain its bookkeeping charge. The process
declaration now caps explicit owners at 44 roles under the unchanged 4096 MiB allowance;
this does not bound native allocator RSS or impose a host-wide limit. The five
caller/adapter files are applied and formatted. Their new ordering, overload,
shutdown and real-child fixtures are authored; captured qualification is
running, so compilation, execution and release acceptance remain unverified.

Board file completion retains its original charged result when outbound request
admission refuses an open. The collector retries that ordered head without
regenerating the file, copying its request prematurely or recording pending
focus. The captured parent must still accept normal children; a changed creation
dialog cancels only the automatic open and preserves the completed file.
Filesystem shutdown closes and drains editor, board, configuration and
integration owners independently, including when no editor service exists.
Their completion notifications are registered before collection, under the
existing shutdown deadline. Accepted board writes finish; automatic opens are
explicitly cancelled during shutdown and their disk data remains available.

Finite naming, model discovery, restructuring and Smart Copy also share two
native-account provider slots across cooperating clients. Naming captures one
immutable inference-settings snapshot and retains each original request under
byte admission; its shared I/O bank caps active jobs, holds completed results
until the event loop accepts them, and retries only pre-provider admission
refusals on a timer. The platform adapter
resolves the native account profile independently of HOME, XDG and project paths,
then probes two permanent owner-only lock files without waiting for a held lock.
Namespace discovery and probing execute within an admitted I/O job. The existing
process limiter and the host lock remain owned until the synchronous provider
body returns or unwinds; output collection and catalog CPU preparation do not
retain either permit. Only a preflight refusal returns the original captured
request for a dated retry. An invoked stream is never replayed, including an
empty response, a partial prefix or a provider failure. Persistent voice uses its
separate actor lifecycle. This bounds participating binaries using one stable
account filesystem; it does not constrain older binaries, remote provider work
or deliberate namespace replacement. An isolated two-child test verifies the
real shared lock limit and release on child exit despite different HOME, XDG and
project paths; it does not write the actual account namespace. Caller-ordering
and integrated release qualification remain open.

Normal voice and the onboarding demonstration prepare prompts, tool schemas,
configuration and context changes on the existing finite CPU bank. A separate
finite I/O phase reads an environment credential only when the captured setting
requires it; its completed original follows the same request into CPU preparation.
The actor accepts an owned startup proof and a separately preadmitted metadata
token rather than constructing configuration on the interaction loop. Temporary
metadata refusal retains the same prepared startup for a dated retry; the token
pins the actor's quota, so startup cannot substitute another ownership group. Normal context preparation reserves its exact FIFO slot,
so younger text, push-to-talk and tool commands cannot overtake it. Replacement
settings fence prepared results, while Stop cancels preparation and waits for
actual actor and native retirement before another owner can start.

The retained-source configuration transition is not integrated yet. Canonical
onboarding Test capture still clones the selected voice settings. The private
source-based callers retain the chosen original before admission, with runtime
identity inherited only after the CPU compares the runtime-relevant settings;
policy-only changes preserve a waiting Test. The original caller's policy test
passes natively, while the new source caller and acknowledged configuration
union have only parsing and source-hash evidence. Modal construction has three
passing native library admission/refusal tests. The private CPU editor and save
controller also have fourteen passing native tests covering original text and
hidden history, input ordering, refusal, panic, partial-paste cancellation,
surface and resize fences, and physical worker shutdown. The save-controller
fixtures inject acknowledgement facts; they do not establish actual writer
durability or persistence readback. Dialog caller integration, complete frame
composition, ordered disk acknowledgements and application shutdown remain
unqualified. None of these focused checks establishes a complete client release
or live transition.

Audio callbacks use bounded preallocated sample rings. The existing persistent
DSP owner performs sample conversion in both directions; playback pressure retains
original pending samples there while capture remains serviced. Capture overflow
reports an incomplete utterance. Push-to-talk commit drains through the last
admitted capture ordinal, and shutdown can interrupt a blocked audio-delta enqueue.
These are current source contracts awaiting integrated qualification, not claims
of real-time deadlines or allocator/RSS bounds. The first environment read still
allocates before its accepted-size cap; arbitrary external JSON maps have opaque
spare capacity and require a producer backing declaration. Native stream creation
reserves a 64 MiB backend declaration and its inspected Rust thread count before
device startup. CPAL 0.18.1 ALSA and WASAPI declare two stream threads whose handles
join on stream drop; CoreAudio declares four disconnect monitors but cannot prove
their exit or account for opaque callback threads, so attempted admission remains
charged until process exit. Uninspected backends declare no thread count and retain
opaque storage credit. A separately admitted creator-thread owner constructs and
destroys the non-Send streams; `AudioCustody` retains at most two actual owner
receipts, and its blocking joins belong on a lifecycle observer rather than the UI
or a Tokio coordination loop. These source declarations and isolated custody tests
do not establish native OS thread/RSS bounds or integrated device shutdown on every
platform.

The live Spectrum analyzer separately admits its persistent analysis worker,
the CPAL backend stream roles, and each helper-process/pipe-reader pair through
the ambient resource quota. Current CPAL declarations account for two Linux/
Windows stream threads and four CoreAudio disconnect roles; unsupported targets
refuse CPAL admission. Because cpal cannot prove the CoreAudio disconnect
monitors exited, macOS stream admission remains charged through stream teardown
and until process exit; this prevents repeated scene replacement from erasing
unverified backend ownership. Those declarations do not establish complete
macOS callback-thread retirement or every backend's internal thread count.
CPAL callbacks reject buffers over 8,192 frames before
allocation and only try-send into a 64-slot queue. The helper reader decodes on
its supervised worker; source teardown kills and reaps the owned helper before
joining that reader for at most two seconds, leaving any survivor in platform
retirement custody. Declared worker bytes cover the selected stack and bounded
capture buffers, not opaque backend allocations or uninspected library threads.
These latest ownership edits have formatting/source checks only; current-source
native tests, platform-specific audio qualification and release checks remain
open.

The client bank constructor and selected-composition fixture now share one
configuration: two CPU, four I/O and one service thread. The selected feature
owners contribute another 27 roles, including up to five Spectrum capture
roles; supervisor/logger/input add three, the
external opener one, and the existing Tokio runtime six. The root ceiling is
derived from those bank constants and the named set: 44 roles under the unchanged
4096 MiB allowance. Earlier fixtures counted only five bank threads, undercounting
the selected feature scenario by two. The selected feature storage declaration is
now 3451 MiB plus 128 KiB before additive bootstrap/runtime storage. A standalone
forcing test starts the real seven-thread bank: the original 36-role ceiling
refuses the remaining 37 declarations, while the corrected ceiling admits them,
refuses a further role, and releases bank admission after all seven native owners
join. Those other roles are synthetic declarations in this test; their engines
and children are not started. Full-client compilation and the updated selected
storage fixture remain unqualified. Retiring owners compete for their original
admission. These declarations do not prove whole-process OS threads, allocator
RSS, native library completeness or measured memory acceptance.

Ambient scene construction receives the composition root's existing execution
client through `AmbientResources` and `SceneEnv`. Finite carpet-chess searches
use that CPU bank, one owned receipt and a captured board identity; cancellation
is checked inside search nodes. Returned moves keep their result admission until
application. This conversion does not complete admission for the other native
scene workers: image decoding, caches, helper processes and opaque library
threads still need producer-specific peak and retirement qualification. Audio
has source-level native-stream admission and creator-thread teardown; whole-client
shutdown integration and platform runtime qualification remain open.

Images discovery transfers the original list and diagnostic allocations through
one shared, independently charged owner; worker, result and scene references
retain that same allocation. Failed list preparation publishes a terminal error
state and returns to idle cadence rather than regenerating an empty cache.
Frozen-source Linux checks cover90 Images tests, all-target Clippy and formatting.
This qualification does not complete PNG/JPEG/GIF/WebP decoding admission,
remote discovery or native library memory accounting.

The presentation owner reserves one physical thread with96 MiB of declared
encoder/resize working storage and an explicit 2 MiB native stack request before
spawning, plus97 MiB of independently retained frame and control storage. The
quota charges the stack request with the worker, while the platform reservation
applies it to the OS thread. Both use the composition root quota. Actual
join releases physical admission; the last retained frame or original failure
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

