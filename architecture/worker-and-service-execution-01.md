## Worker and service execution

`ilium-execution` starts fixed OS-thread banks for finite CPU and I/O jobs, plus
an optional bank for persistent services. Async tasks wait for typed receipts;
they do not provide CPU offloading. Each composition root owns the bank and
shares one `QuotaGroup` among its clients. Admission bounds outstanding jobs,
waiting slots, captured inputs, retained results, physical threads and declared
resident storage. A reservation precedes expensive capture or stateful
preparation. Reserved publication remains valid during a concurrent drain;
explicit cancellation returns work that never started. Callback panics produce
failed receipts and leave subsequent finite jobs runnable.

Client provider calls also share a process-scoped two-call limiter across
naming, Smart Copy and remote compaction. Each provider body runs inside its
already-admitted I/O job and requires a nonblocking host-level admission lock
as well. Refusal before the body preserves the original request for retry, so
adding client instances does not multiply the process provider quota or create
detached provider workers.

Server chatroom reference routing now reads `CHATROOM.md` through the existing
finite I/O bank. Each sequential job admits a bounded input/result cost and
returns at most 64 KiB and 64 records from a newline-aligned offset; a single
record is capped at 8 KiB. Incomplete trailing records remain unread, while
admission, read, and oversized-record failures leave the offset unchanged.
Initial backlog remains skipped and routing stays sequential. The source and
batch/admission regressions are in place; current-source tests and release
qualification remain pending.

Frozen-screen restoration uses bounded client I/O and CPU jobs: regular-file
reads are capped at 128 MiB and checked for cancellation, parsing validates the
saved terminal geometry, and retained immutable screens publish only while the
requesting pane generation is still frozen. Freeze-time serialization now runs
as an admitted CPU job, then transfers its bytes into shared storage admission
and a FIFO placeholder on the existing ordered writer. That I/O owner creates
the parent directory and performs the durable replacement; its acknowledgement
retains the pane identity so stale failures cannot overwrite a replacement's
status. Both paths cap snapshots at 128 MiB. FIFO responsiveness, durable
readback, distinct CPU/I/O ownership, and stale-pane regressions are authored;
current-source tests and runtime behavior remain unqualified.

New-worktree target-path normalization, canonical-parent verification,
metadata checks, and exact parent-directory creation run on the bounded server
I/O lane. Encoded path input is capped at 4 KiB to match workspace ownership
validation; its working and retained result costs are admitted before
execution, and the retained path receipt stays alive
through Git creation, include preparation, rollback, and pane publication.
`repo_facts` probes registered checkout directories and `.gitmodules` in
fixed batches of 32 on that same admitted lane. It reserves against borrowed
paths before cloning each batch, preserving the former `is_dir`/`is_file`
failure-as-absent behavior without a job per checkout. Repository inspection
and restore verification share the existing 1,024-worktree ceiling and refuse
larger Git listings before launching per-checkout probes.
Include preparation caps the include file, selected file count, and copied
bytes, and refuses after 10,000 visited source entries before copying.
Discovery, copying and identity-checked rollback also use the bounded server
I/O lane with declared working/result costs; retained receipts keep copied-path
identity alive until pane commit or rollback. The worker checks cancellation
between source entries and files, and attempts identity-safe cleanup before
returning a cancelled copy. The rollback process-use scan also uses bounded I/O
admission and retains its capped process evidence until the deletion decision.
Source changes are unqualified pending current-source server tests, strict lint
and release checks.

Workspace creation, retained pruning, removal and automatic pane close share a
server-owned registry capped at 16 admitted tasks. Admission and task spawning
are one synchronous operation, so overload and shutdown refusals occur before
the future can run. Finished handles release capacity on the next admission;
shutdown closes the registry and joins every accepted mutation before exit.

The filesystem Explorer submits directory reads to the client's shared bounded
I/O lane. Each job declares 32 MiB of working input and 4 MiB of result space;
the scan also caps a listing at 8,192 entries and 4 MiB of retained paths and
names, checks cancellation between entries, and canonicalizes manually entered
paths on the worker. The UI retains the prior listing until a receipt matching
the current revision arrives; transient admission refusal keeps the request for
retry, while stale results cannot replace a newer navigation.

Text-trigger preview preparation uses the shared CPU bank. The dialog owner
admits CPU, source-retirement and result-retirement capacity before copying any
authored input, then captures at most 64 KiB and 256 sample lines per turn.
Regex compilation, matching and preview-string construction run in the CPU job;
the UI renders only the prepared visible lines. Results are keyed by draft
identity and revision, and stale work is canceled or discarded. Transient
admission refusal leaves the authored draft uncaptured and schedules a retry.
The capture limits bound authored input and output, but regex compiler scratch
is opaque and the declared job cost is cooperative rather than a strict peak
memory bound; this remains a resource-qualification gap.

Session conversion uses the process's shared bounded I/O bank and admits one
job with a working-set declaration of four times the verified source
transcript size plus 16 MiB, capped at 512 MiB, and a 2 MiB result allowance.
The worker checks that the transcript has not grown since admission before
conversion. Its progress channel is bounded and nonblocking; overload requests
cancellation, while semantic completion is delivered through the retained job
receipt so a full UI queue cannot hide a committed conversion. The worker owns
cancellation and physical completion through shutdown. Source and generated
transcripts are capped at 128 MiB, including bounded reads for inspection and
verification; the Codex import ledger is a regular file capped at 16 MiB.

Area 11 ambient-map loading now uses the host's shared physical worker quota
and retained-storage admission. Topographic decoding/generation reserves a
64 MiB worker peak and the maximum 8 MiB heightfield before allocation;
OpenStreetMap keeps its two-request cap while also reserving a 256 MiB worker
peak and 128 MiB for each retained map. The storage credit follows the result
through the scene cache and releases on eviction. Capacity refusal leaves the
interactive frame path nonblocking and retries on a later frame. Focused
capacity-recovery tests are source-integrated but await remote qualification.
The client cost-history scan now treats absent optional CLI roots as empty, but reports
non-missing root, directory-open/iteration, metadata, file-type, and
modification-time failures as incomplete calibration. A regression covers a
configured root that cannot be scanned. Its first remote build stopped before
the assertion because ni-vm lacks `openssl.pc`; a current-source snapshot is
transferring under monitor 140. Test and release qualification remain pending.

Bank construction also has an explicit partial-start owner:
`Execution::start_with_custody` returns the original failure together with any
bank whose workers already started. The caller cancels and observes that actual
bank before retrying. The existing `Execution::start` API performs this cleanup
on its bootstrap caller; if its five-second join deadline expires, the original
spawn error contains a typed `StartCleanupError` retaining the bank for later
background observation. A callback exit or elapsed deadline never establishes
physical shutdown. This constructor seam passed all 13 debug library tests,
strict all-target Clippy, its five new native cases in release mode and an
optimized library build. The 74 recorded source and manifest inputs matched
current readback; the release artifact and physical child cleanup were verified
(`ilium-worker-foundation-native-879-1312`). A separate integration run passed
42 debug cases across ten admission, quota, retirement, CPU and shutdown test
binaries, plus four optimized CPU, nested-shutdown and startup-ownership cases.
All 74 inputs matched current readback and every child was reaped before the
isolated target was removed (`ilium-worker-foundation-integration-879-1315`).
These checks qualify the execution foundation only. Client startup awaits one
bootstrap owner on the existing runtime.
A bounded transfer acknowledgement moves the actual bank only after the caller
has taken custody; cancellation leaves it with the bootstrap owner until its
native workers join. Failed admission retains the original initialized owners
in `ClientExecutionStartError`. This client seam is also source-integrated and
formatting checked, with five native cases authored but not yet executed.
Later service-startup rollback and complete client shutdown remain separate
integration and verification requirements.

Canonical bank shutdown now retains the actual `ClientExecution` in one shared
custody slot, observed by the existing runtime blocking task. A bounded transfer
acknowledgement separates observation from accepting its result. Deadline or
join refusal returns `ClientExecutionShutdownError` inside the public I/O error;
the caller can inspect its native report and repeat background cleanup on the
same bank. Observer-construction panics return that same typed custody instead
of unwinding the bank. Canceling an accepted observation leaves that task with the original owner
until physical/admitted shutdown completes, including the original quota charges.
It creates no replacement execution bank or supervisor. Final root aggregation
keeps an earlier client error alongside execution custody in
`ExecutionShutdownFailure`, rather than losing the latter through `Result::and`.
Other cleanup aggregation and complete App disposal remain separate gaps.

The original real-CPU deadline case reproduced the intended missing-custody
assertion. Eight native component cases now pass: same-bank deadline retry,
cancellation before observation, cancellation with a blocked CPU callback,
observer panic, prior-error aggregation with a live retirement reservation, and
public Send/Sync error retention, plus queued cancellation during runtime shutdown
and observer-construction panic. The queued case saturates Tokio's blocking pool
at one thread: the already accepted observer runs after runtime shutdown, and the
same tenant stays charged until physical cleanup. This disproves the initial
review suspicion of accepted queued callbacks being discarded in this path.
The construction-panic case reproduced its intended assertion before correction.
The isolated one-CPU fixture uses the real
execution foundation and Tokio; it excludes production process quotas and the
complete App graph. Its error adapter includes the exact production aggregation
helper and selected terminal-error variant, rather than the whole error enum.
All four captured input hashes were unchanged during the final run. Evidence was
kept in a scratch evidence folder (deleted 2026-10-10).
The canonical client library check passed in 17.15 s with 18 warnings; scoped
formatting passed before the construction-panic correction. Independent ownership
review has a retained correction addendum; full test-target qualification,
strict lint, release and runtime/performance acceptance remain outstanding.
A fresh canonical all-target check reached the client library with 18 warnings
but exceeded its 110-second command budget (exit 124). That attempt does not
establish all-target compilation or a source/compiler failure.

Sound-preview publication has one admission-status owner shared by settings,
the onboarding signature preview and the sound studio. Control commands return
the actual refusal instead of unconditionally claiming a queued request.
Accepted requests are reported as requested, rather than claiming playback. The original
refusal assertion reproduced; three exact-method component cases pass using real
bounded request admission and a physically joined isolated CPU bank. The minimal
App adapter includes the complete retained-byte visitor but excludes parser
barriers and the full settings/onboarding graph. Additional canonical caller
tests compile in the canonical client test target (15.79 s, exit 0). The first
compiler attempt rejected private-field accesses; the tests now use the existing
test drain. Full native caller execution remains unqualified.
Evidence was kept in
a scratch evidence folder (deleted 2026-10-10).
The UI's synchronous selected-file check, startup sound/device discovery and
server playback-result delivery still require worker integration and full runtime
qualification; this status correction does not complete the audio boundary.

Startup sound scanning and input/output device enumeration now run as one finite
job on the existing client I/O lane after bank construction. Startup awaits its
typed receipt instead of executing filesystem and native-device calls on the
runtime thread. Existing enumeration-error logging and empty-list fallback are
preserved. Cancellation requests stop between native phases; a blocked native
call retains its actual job credit until return. No additional bank or thread is
created. One client/job admits 64 MiB each for working and result storage; a
capacity-based audit rejects an oversized catalogue before UI installation.
The same retained result charge moves into the final App field, after both sound
discovery and device-name vectors in destruction order. It remains local until
the originals are installed, including failures during intermediate startup.
This conservatively retains the whole declaration through UI lifetime.

Sound discovery also shares a 65,536-entry traversal budget across roots and
recursive calls, counting unsupported files, directories and failed entries.
Exhaustion stops further traversal and sets the existing UI truncation flag;
finishing exactly at the limit does not report truncation. One additional
iterator entry may be yielded to establish that more work exists; its metadata
and path are not inspected. The playable-file
and depth limits remain in place. This bounds entry inspection, not time spent
inside a blocking filesystem call or environment-root/path-byte allocations.
All 20 sound-library native tests, sound-crate strict Clippy, scoped formatting
and the affected release-library build pass. Three new small-budget real-file
fixtures cover nested unsupported entries, shared roots and exact completion;
they do not exercise 65,536 actual entries or provide an original failing
regression run. Independent source review found no new correctness defect.
Evidence was kept in a scratch evidence folder (deleted 2026-10-10).

The same shared scan owner now caps retained sound text at 8 MiB, charging actual
capacities of paths, canonical deduplication identities, display names and
collection strings before insertion. Checked arithmetic refuses overflow.
Path/name expansion is screened before display conversion and collection cloning;
canonicalization and directory iteration can still allocate native scratch before
validation. A refused candidate does not enter either the identity set or sound
vector. Container slots remain bounded separately by the 4,096-sound limit.
The existing truncation notice reports text refusal, and ordinary duplicates do
not consume text twice. Platform root/environment allocation and root-directory
storage remain outside this text budget; this is not a complete process-memory
or native-host bound.
After source review, remaining-credit checks run after supported-file filtering
and canonical deduplication, preserving skipped entries at exhausted credit.
Extension matching remains case-insensitive without allocating a lowercase copy.
All 24 native sound tests, scoped strict Clippy/formatting and the affected release
library build pass. Four new fixtures cover text refusal, actual-capacity debit,
exact/overflow admission and duplicate/unsupported entries at exhausted credit.
Evidence was kept in a scratch evidence folder (deleted 2026-10-10).

Platform sound-root candidates now share a 64-root and 256 KiB allocated-text
budget before retained vector insertion. The Linux XDG list is rejected when its
encoded length exceeds 256 KiB, before splitting into components. Ordinary
platform ordering and user-root fallback remain unchanged; admission refusal
sets the final truncation notice without preventing accepted roots from scanning.
The retained root deduplication set contains only admitted candidates; count and
text caps bound its copied paths. Root filtering may release candidates without
reusing their conservative credit. Environment reads and native candidate path
construction still happen before validation; these are not a hard transient
allocation, native-host or filesystem-blocking-time bound.
All 30 Linux native sound tests, scoped strict Clippy/formatting and the release
library build pass. Six root fixtures exercise count/capacity/overflow refusal,
accepted-root scanning and oversized/ordinary XDG lists without global environment
mutation. Source review covers the root builder; non-Linux branches have not been
compiled or executed in this qualification. Evidence was kept in
a scratch evidence folder (deleted 2026-10-10).

Input/output device catalogue preparation now shares one bounded collector:
256 processed devices, 4 KiB per formatted name and 64 KiB aggregate allocated
name capacity per direction. Formatting writes into a checked writer instead of
first constructing an unbounded `to_string()` result. Each chunk is checked
before reservation/append, and allocated capacity is validated before retention.
One additional iterator item may establish count overflow without formatting it.
Complete admitted names preserve ordinary sorting and deduplication. Refusal
returns a fixed typed error, so the existing startup error log/empty fallback
remains truthful; a partial catalogue is never reported as complete. Native host
construction, iterator internals and Display implementation scratch remain
outside this bound. Stream/callback ownership, provider execution and latency
are unchanged by this catalogue preparation boundary.
All 53 voice-library native tests pass, including six new collector fixtures for
sorted deduplication, infinite enumeration/count refusal, oversized names, shared
text exhaustion, exact limits and rejected formatting chunks. Scoped strict
Clippy/formatting pass. These fixtures exercise original bounded preparation
without requiring real host-device enumeration; native-host/runtime and full
client qualification remain separate requirements.
The affected voice release-library build also passes (93 s). Independent review
found no new catalogue defect and identified the separate saved-name
`find_device` enumeration as unbounded at that checkpoint; the following
integration applies the same preparation limits. Evidence was kept in
a scratch evidence folder (deleted 2026-10-10).

Saved-name selection now uses the same checked name preparation in one pass,
returning the original native device owner at the first matching name. It does
not clone the device or enumerate again. Discarded preceding names consume the
64 KiB aggregate preparation budget, bounding formatting work as well as retained
text; processed-device and per-name caps match catalogue preparation. Oversized
requested names fail before advancing the iterator. Missing names preserve the
existing named-device error, while an exhausted budget returns explicit resource
refusal. The default-device path for absent/blank configuration is unchanged.
Native host/iterator/Display internals remain outside these preparation limits.
All 58 native voice tests, scoped strict Clippy/formatting and the affected release
library build pass. Five new lookup tests cover original-allocation return and
early termination, empty/missing names, infinite scans, oversized requests and
cumulative discarded-name preparation. Actual native-host selection and full
client/runtime qualification remain unverified. Evidence was kept in
a scratch evidence folder (deleted 2026-10-10).
Current canonical client library/test-target compilation also passes (85 s,
existing warnings). Independent source review found no concrete lookup defect;
one extra device may be yielded before count refusal and native enumeration
starts before requested-name helper validation. This compiler check covers the
current callers but does not execute full App/native-host behavior or establish
an immutable whole-workspace qualification.

This is an offloading and ownership integration, not proof of capped upstream
allocation: environment-root collection, filesystem traversal scratch and native
audio-host allocation still require producer bounds and measurement. The existing
startup dialog also writes directly before presenter ownership begins. Four
native I/O/admission/cancellation/capacity tests pass against the complete original
module and actual dependency libraries. Canonical client library/test-target
compilation passes (77 s); complete App native execution, actual host enumeration,
strict lint, release and live behavior remain unqualified. Qualification was recorded in
a scratch evidence folder (deleted 2026-10-10).

The complete client ownership migration is not qualified yet. The latest
isolated all-target check reached the client and reported 163 error records
and 19 warnings; all 2,108 frozen inputs matched their retained originals.
Remaining callers still use removed prompt constructors, raw configuration
paths and incomplete animation controller APIs. Private corrections explicitly
convert cursor coordinates, borrow editor paths, retain the declared request
byte bound and let the durable editor writer borrow its retiring source on
I/O. Their exact source patches are verified, but compilation and native
persistence, ordering and final-disposal checks remain outstanding.

The canonical client library check on 2026-10-07 passed (`cargo check -p
ilium-client --lib --offline --locked -j 1`, 26.15 seconds), with 18 library
warnings. This check used the existing target and current canonical sources;
it is distinct from the private value-owner candidate's all-target failures.
It does not compile the test target or qualify strict lint, release or runtime
behavior. An independent canonical-only client audit covers the eleven requested areas
and records 43 unchanged source hashes in
a scratch evidence folder (deleted 2026-10-10).
It identifies six prioritized gaps: bank shutdown custody, zero-request flush
custody, modal paste replay, CPU save capture, startup discovery and truthful
Sound preview publication. Server-only boundaries are explicitly excluded from
that client evidence. The zero-request native regression attempt hit its
120-second limit before a test result. A subsequent native component harness
compiled the canonical shutdown module and exact FIFO-publication code. Both
zero-request pending/closed receipt cases reached their intended original
assertions, then passed after correcting the final-custody predicate; fixture
banks physically joined. Source hashes were unchanged within each run. Evidence
was kept in a scratch evidence folder (deleted 2026-10-10).
This harness excludes the complete App and codec-capture path, so it does not
qualify the full client test target.
An extended component run also compiles the complete canonical FIFO sender and
request-capacity admission functions. Four original test bodies pass: the two
empty-batch cases, full-queue cancellation, and closed-writer refusal with
additional cleanup originals. They verify allocation identity, unchanged
admission debits, overlapping-batch refusal and preservation of the earlier
cleanup error. The fixture uses actual execution banks, Tokio channels and
retained request guards; its notification service and selected `TerminalSetup`
error-variant adapter exclude the complete process/error graph. All five input
hashes remained unchanged during the run, and fixture banks physically joined.
The native log and dependency/source hashes were kept in
a scratch evidence folder (deleted 2026-10-10).
The subsequent canonical Cargo-emitted client library binary executed all six
request/flush cases, including the two stream/codec cases, successfully. The
same scoped run passed eight bank-shutdown, four startup-audio, three App
preview, two onboarding and one control-publication cases: 24 total. All 1,229
captured source hashes remained unchanged. Native linking used an existing
hash-verified ONNX Runtime 1.24.2 library through child-only aliases; no runtime
was installed. Evidence was kept in
a scratch evidence folder (deleted 2026-10-10).

Whole-client acceptance remains open. The broader run of that same binary
timed out after 174.06 seconds, without a final libtest summary. Partial output
records 2,658 passing, 26 failing and 40 ignored cases, including startup,
presentation, parser-admission, cost-preparation and animation/settings
failures. These partial counts are not final suite results. One animation UI
paint/hit case ran over 60 seconds. Source, binary and runtime hashes stayed
stable during the run; owned children were reaped and aliases removed. The
exact failed cases require diagnosis before another acceptance run. Evidence was kept in
a scratch evidence folder (deleted 2026-10-10).
Workspace lint/formatting, affected release artifacts, real terminal behavior
and matched performance measurements remain separate outstanding gates.

Follow-up native diagnosis reproduced obsolete test assumptions in scan-debit
sizes, aggregate-client registration and the presentation receipt sequence.
Fixture corrections leave production limits unchanged: the history-drain test
fills the same admission bands and requires the old scan receipt to release
before CPU admission; startup counts each actual aggregate/general/location
identity; presentation distinguishes physical join from surviving ticket
custody and consumes uncertain-frame custody before the terminal error.
Fresh binaries pass all five startup cases, seven history cases and all fifteen
presentation cases, including resize/diff-base, FIFO output, cancellation,
uncertainty, physical exit and last-owner allocation disposal. Nine incidental
semantic-presentation cases also pass. All 1,229 captured sources stayed stable
within each qualification run. Evidence is retained under the same native
integration directory in `gate-006/terminal-audit.json` and
`gate-007/terminal-audit.json`. The latter harness initially rejected its count
because Rust's substring filter selected nine additional cases; native exit was
zero and the audit confirms every required case passed. No rerun was needed.
Other broad-suite failures and its long-running animation case remain unresolved.

Further exact diagnosis separates obsolete settings fixtures from production
errors. Four corrected fixture cases pass in a fresh binary without changing
production controls or limits (`gate-009/result.json`). Remote-compaction
choice options now use the same stable `Technique::id()` values accepted by
the save parser. Its pointer dispatcher restores Settings ownership even when
a hit only selects the privacy-banner row. All twelve remote-compaction and
fifteen settings-choice native cases pass with acknowledged commit and disk
readback (`gate-010/terminal-audit.json`, 1,229 source hashes unchanged).
These focused checks do not qualify the whole client, release or live behavior.

The next canonical client all-target compilation passes (`gate-011`, 1,229
source hashes unchanged). Strict Clippy exits 101 in the ambient dependency:
Growth default/clamp/precedence expressions, the terrain multiple check, and
Wind's indexed loop and nine-argument function. This does not establish clean
client lint. The 27-record consumer census confirms active stream/location
storage guards, shared completion wakes and clipboard acknowledgement
collectors; unread lifetime guards must not be removed. Two separate
JavaScript-extension integration warnings were referred to their owner.
The document-window worker's redundant notification alias/accessor is removed;
its completion callback retains the actual wake. The fresh all-target compiler
passes (`gate-012`). Its first native compile reached the overall deadline;
the native-only `gate-013` then emitted a fresh binary. The exact inventory was
two window and three syntax cases; all five pass, including physical CPU
publication, retained emitted-frame ownership, cross-line syntax and unsplit
70 KiB lines. The count mismatch was a harness failure and was reconciled against
the unchanged native binary without another compilation. Receipt:
`gate-013/native-followup/terminal-audit.json`. Workspace tests, release, PTY and
matched performance remain open.

The canonical Markdown caller inventory confirms that `parse_bounded`, layout
preparation and image decoding are reached only inside CPU-bank callbacks;
image reads occur in the I/O stage. Synchronous `parse` callers are test-only.
UI drawing consumes the installed prepared document. This source inventory
does not establish large-document latency: bounded source capture still copies
lines on the event loop. Editor saves now loan the pane to a CPU worker for line
measurement, reserve the ordered writer from that measured cost, then run a
second CPU job to verify and clone the same revision into an immutable snapshot.
The pane returns with that snapshot, and the writer acknowledgement still
controls saved/error state. Admission refusal returns the original pane without
accepting a save. This avoids scanning or cloning all save lines on the event
loop and preserves the accepted revision; it does not remove the separate
interactive source-window copy or establish large-document latency and peak
memory bounds. The locked `ratatui-textarea` 0.9.2 stores text as `Vec<String>`
and exposes borrowed line slices, so save capture still needs a distinct owned
snapshot. Canonical save rejects more than 262,144 lines or 32 MiB of source;
these caps bound retained work and memory estimates, not measured process RSS.
The exact dependency and caller map is
`editor-source-integration-set.jsonl` in the inventory directory below.
The eleven-area inventory and precise Markdown amendment were kept in
a scratch evidence folder (deleted 2026-10-10). Historical qualification
limitations in that inventory are leads until current receipts are reconciled.

Text Trigger live previews now have a per-App finite owner on the existing
shared CPU bank. The UI copies admitted draft chunks up to 64 KiB/256 sample
lines per turn; regex compilation, matching, highlighting and reply-loop checks
run on CPU threads. Draft identity and semantic revision fence publication;
previous immutable output remains visible during preparation. Captures and
outputs have independent retirement admission, accepted receipts remain owned
through cancellation, and transient refusal retries the same captured job.
Limits cover two jobs, 256 KiB each for regexp/message, 1 MiB/16,384 sample lines
and 2 MiB preview text. Regex compilation explicitly caps the NFA at 10 MiB and
the hybrid DFA cache at 2 MiB; parser/compiler scratch remains opaque and may
exceed those figures. The cooperative job declaration is not a measured RSS
bound.
Shutdown closes the preview owner without waiting, collects its receipts while
the bank enforces the physical-exit deadline, and reports lost or unsettled
receipts. Config writes retain their separate ordered acknowledgement and exact
draft dismissal fence. This caller integration now has a focused canonical Cargo result: the retry-after-transient-refusal regression passes in a freshly built client test binary. That run did not capture an immutable source manifest, and it does not qualify rendering, persistence readback, the full client suite or a release artifact; its receipt was kept in a scratch evidence folder (deleted 2026-10-10). Earlier bounded compile attempts and their captured-source limits remain historical evidence. The
third gate reused that exact verified runtime path and eliminated dependency
rebuilds, but native client compilation still exceeded 175 s; zero cases ran.
A longer 900 s compile-and-native gate is registered with monitor83, awaiting
its terminal notification. Independent source review found no concrete preview
cancellation/modal/stale-result defect. Receipts were kept in
a scratch evidence folder (deleted 2026-10-10); native success
has not been established.

Generic non-terminal paste now has an authored bounded continuation. Its
incremental UTF-8 cursor consumes CRLF atomically; the event loop dispatches at
most 128 derived keys per turn, blocks later input receipt until the suffix is
finished, and retains the original `InputEvent` and storage lease throughout.
Shutdown transfers the untouched original plus its consumed UTF-8 byte count
into input failure custody. Before replay, the client reserves the existing
execution CPU-retirement service; successful completion transfers the original
envelope there for destruction, while refusal preserves the untouched paste.
Focused cursor, custody and CPU-thread disposal tests are authored. This
now also includes an overload regression using a dedicated one-thread bank;
it fills all retirement slots, verifies `QueueFull` before cursor advancement,
and checks that the original paste allocation and admission remain owned. The
integrated source has not yet passed its current-source compile/native gate, so
ordering, shutdown, refusal, CPU destruction, release and live behavior remain
unqualified. The source inventory was saved in
a scratch evidence folder (deleted 2026-10-10).

An immutable 1,232-file workspace snapshot failed the complete formatting check
in 14 files, with no capture drift or snapshot mutation. Three root-owned
preview integration hunks have since been corrected and pass scoped formatting;
the other owners' changes remain intact. The complete workspace check still
needs to pass. Source/log identities and cleanup evidence were kept in
a scratch evidence folder (deleted 2026-10-10). Blanket freezes are not a
qualification mechanism; future native checks use immutable source snapshots,
and gate004's source identity must be reconciled with subsequent edits.

Configuration-directory consumers are being migrated to the existing
`Source<PathBuf>` and `Snapshot<PathBuf>` ownership boundary. Writer payloads
must retain the same admitted path through rejection and acknowledgement,
and borrow that path during I/O. Startup currently constructs its prepared
state on the terminal-owning caller; wrapping a path there does not establish
CPU offloading or bound its original production. The admitted startup producer,
all callers and the final source-release gate must close together before this
boundary can be accepted.

The private directory-source integration now includes the Cost, Voice, Remote,
Git, Inference and Agent writer-refusal slots. Each slot keeps the rejected
command, destination and intent for an ordered retry instead of rebuilding a
write. Cost preparation captures its destination and checks source identity
before publication. The two Cost modules are wired into the private client
graph. Exact patch replay and scoped formatting pass; these sources have not
been compiled or exercised. The actual startup producer, configuration-delivery
consumers and value-dialog cancellation/shutdown ownership remain open.

The private caller inventory also covers directory assignment fixtures: 61
scoped edits reuse the existing CPU-admitted directory helper and preserve the
same destination alias in a keyboard-prefix target. Inference acknowledgement
fixtures retain the actual delivered snapshot through their callback, and the
missing-directory Git fixture explicitly retires its retained terminal state.
These fixtures use the existing shared test client; they do not establish
isolated-bank qualification. Exact forward and inverse patches and Rust parsing
pass, while current-client compilation and native execution remain unrun.

Numeric initial sources now have private signed-integer and fixed-decimal
variants in the existing admitted CPU constructor. Fixed precision preserves
Sound Pulse Rate's trailing decimal place; signed formatting preserves negative
Sweep and Harmony values. The declared body bound includes the requested
precision, and nonfinite values retain the original failure request. Two
actual-bank formatting regression cases are authored but have not run. Complete
Sound producer, edit, acknowledgement and shutdown integration is still required.

Private animation and Sound companion sources now compose with the current
directory and prompt changes. This includes CPU edit/resolution controllers,
immutable animation row sharing, Sound model preparation and cached previews,
plus the module declarations required to expose them. Recommendation capture
shares the admitted incoming projection instead of copying its body on the UI.
Exact forward/inverse source replay passes; this combined client graph remains
uncompiled. Animation destination publication, save-pump migration, complete
Sound caller/publication integration, and native fixtures are still required.

The Sound writer migration must preserve the existing 64 KiB serialized-command
limit and bounded serde normalization before publishing prepared settings. The
private Sound Update job now reuses the existing sink and normalization on its
CPU worker before constructing a non-Clone validated write object. Local and
Preview keep their existing behavior. The existing ordered writer also has a
private family entry that accepts the captured destination snapshot and returns
that same original with payload and intent on admission refusal. No new writer
or execution bank is introduced. Exactly three production Sound save callers
still require migration. A preparation receipt is not a durable
save acknowledgement, and Studio Save must report success only after the ordered
writer confirms the original destination and desired settings.

The private Sound family writer now consumes the validated prepared object and
calls the existing borrowed durable saver. Its receipt returns the original
settings snapshot. The application acknowledgement checks both that snapshot
