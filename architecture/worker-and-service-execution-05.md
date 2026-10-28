# Worker and service execution — continued 5

without modifying the server's file, and closes on initial sync or terminal
shutdown. A blocked native read keeps its bank debit until it returns. No
additional thread, bank or provider quota is introduced. The oversized-file
regression fails against the original reader and passes with the bounded reader;
three exact-source record tests pass. The subsequent captured client run passes both production startup-reader tests,
including real-bank readback and close, plus three execution composition fixtures,
all five location-save fixtures, compilation and package formatting. The runner exit
and all nine log hashes are verified. Two concurrent statistics-file changes mean
this is captured-source qualification; strict lint, current-union release, runtime
and matched performance acceptance remain open.
The server's best-effort startup publisher now has one State-owned native file
owner on the existing I/O bank. Captured text is bounded and admitted before
copying; the original path remains charged through its last native callback.
Only native callbacks acquire the file mutex. Pane-name capture releases the tree
lock before awaiting publication. Each restore carries a generation token, and
ordered sequence fences prevent delayed writes after finish or an old finish
from deleting a newer phase. Shutdown submits final removal before cancelling
the bank. Refusing a courtesy update leaves the semantic restore input intact.
Four standalone tests using the real bank and native files verify off-caller
execution/readback, stale-write fencing, cancellation while blocked and finish
ordering when a younger update returns first. Finish always closes its own phase;
sequence coalescing applies only to publications. The ordering regression fails
against the earlier publisher and passes after this correction. These
use a minimal State fixture. The subsequent captured full server run passes
443 library tests with nine ignored, including all four production publisher tests,
three ready-log tests, bootstrap tests and legacy allocation-refusal/readback.
All-target compilation, strict Clippy, package formatting and the server release
build pass with actual runner exit zero. An independent audit matches all five
log hashes and 588 files in the server's transitive local dependency closure,
with no captured or current-source drift at audit time. The release binary is
built but not installed or live. Complete client integration, shutdown runtime
acceptance and matched performance measurements remain outstanding.

Rolling session backups now reserve typed job and scratch admission on the existing
server I/O bank before copying paths or session names. Started native work and its
original errors retain admission through actual return or final error disposal.
Copy-before-restore, synced temporary publication, half-hour retry buckets and the
existing newest-first retention tiers are preserved. Retention scans stop at 16,384
entries or 4,096 candidates and check filename/path capacity; sorting and deletion
plans borrow the retained candidate paths. A complete plan precedes any deletion,
so an incomplete scan preserves history. A published backup remains successful
when pruning refuses. Checked borrowed path lengths determine the cooperative
memory declaration; this is not an allocator or RSS guarantee. Six forcing tests
are authored for readback, admission refusal, blocked cancellation, original error
retention and incomplete pruning. The first capture passed its tests but stopped
at strict Clippy on a manual scan counter and unnecessary borrowed-result lifetimes.
After correcting both, the fresh capture passes 12 backup tests, all 451 server
library tests, all-target compilation, strict Clippy, package formatting and the
server release build. Actual runner exit and all six log hashes are verified;
backup and IPC source hashes still match that capture. The binary is built but
not installed or live. Later HTTP changes are outside this qualification.

IPC acceptance now waits before accepting a new stream when the existing tracked
connection registry contains 64 owners. Completion-aware handle polling reaps
actual completed owners and observes their original failure result; one saturation
and one resumed-capacity diagnostic are emitted per wait episode. Shutdown can
cancel the capacity wait through the existing outer select. Transport backpressure
preserves queued stream bytes without creating another worker or quota pool.
Two authored real-endpoint tests exercise four release/reaccept cycles, queued
request ordering, shutdown and terminal-subscription cleanup after abort. Scoped
formatting passes. Both real-endpoint tests and all-target compilation pass.
The separate original capture stopped on the older backup style findings; the
corrected combined server capture subsequently passes strict Clippy, package
formatting and release with this same IPC source. Idle/stalled clients can still
delay new management connections, and
this owner count does not establish a complete byte or RSS bound. Existing registry
insertion may prune a below-saturation completion before its error is observed.

Loopback HTTP project resolution now reserves a job on the existing server I/O
bank before copying the requested project and directory inputs. Admission refusal
returns a retriable service-unavailable response before agent/workspace creation.
Path inputs and constructed paths are limited to 8,192 bytes, scans to 16,384
entries and 512 pending directories at depth two. At most two distinct canonical
candidates are retained; two establish ambiguity, while an incomplete capacity-
limited scan refuses rather than returning a unique prefix. Mid-enumeration
errors also refuse instead of silently yielding a partial candidate set. Existing hidden-path,
unavailable-directory, stale-session-directory and canonical deduplication behavior
is retained. Checked copied-path/FIFO/native-scratch declarations are cooperative
limits, not allocator or RSS guarantees. Success/error JSON buffers inherit the
original job retention through `Bytes::from_owner`, including byte-slice clones;
server-owned workspace creation also retains lookup admission if its HTTP caller
is cancelled. Native callbacks retain physical admission through actual return.
Six authored forcing tests cover bank-thread identity, path readback, last-byte
ownership, original error responses, pre-callback refusal/retry, cancellation and
incomplete/ambiguous scans. The initial test compile stopped because its
cancellation assertion required Debug on the successful retained value. A direct
error match fixes that fixture without expanding production contracts. Scoped
formatting passes; fresh frozen tests, all-target compilation, strict Clippy,
package format and release are running. Independent
advisor delivery was unavailable because required browser Pro was locked and both
prescribed native launch adapters hit the runtime agent-thread limit; exact frozen
sources and failures are retained. Original HTTP request parsing and unrelated
semantic/transport concurrency are separate admission boundaries, not proved
bounded by this lookup job. No HTTP runtime activation or performance benefit is
claimed.

Ready-log metadata publication now moves the original PID/path marker into the
existing server I/O bank, started after successful listener binding. The secure
temporary-file write and atomic rename retain their original fatal error contract.
Cancellation discards delivery but cannot undo a native callback that has started;
its physical job admission remains held until return. Error-path storage remains
charged through the caller's error value. An isolated thread-identity regression
fails against the original direct helper. The subsequent full server qualification
passes the real-bank and bootstrap tests, all-target compilation, strict Clippy,
package formatting and release build with Cargo, rustc, rustdoc and Clippy pinned
together. Earlier mixed-compiler, missing-error-Display, platform cast and orphaned
doc-comment failures remain retained as failed evidence. The release build is not
installed or activated; client, live terminal and performance acceptance remain open.



The platform registry caps running and retiring owners at1024, excluding its
supervisor. Neither that cap nor cooperative declarations establish a whole-process
OS-thread/RSS bound. Complete caller qualification, library thread census and
matched runtime measurements remain separate acceptance gates.

Integration acceptance remains separate from source implementation. The IPC
attach snapshot, lag resynchronization and visible-pane replay paths now estimate
retained PTY bytes, reserve storage before copying, revalidate the journal/session
identity, and carry the reservation through the bounded per-connection reply queue
and socket flush. A failed broadcast-replay recovery now closes the connection
rather than silently skipping bytes. These caller changes are awaiting the
current server compile and native verification; they do not yet qualify the
behavior. Server broadcast fan-out and other message producers still need byte
ownership through their consumers. In particular, `ServerState::broadcast` is a
synchronous `tokio::sync::broadcast` send used by many producers, so it cannot
await bounded admission; some payloads, including merged `ScreenUpdate` bytes,
are already assembled or cloned before reaching that method. Replacing this path
requires an ordered, bounded publication contract and producer-specific overload
handling, not just a larger queue. Its lag recovery rebuilds current tree and
pane state and replays retained terminal journals, but it does not replay every
semantic event already skipped by the broadcast receiver; for example,
`PanePromptSubmitted` is consumed as a trigger and has no snapshot equivalent.
Therefore byte admission cannot be implemented by silently dropping an event
when a slow receiver exhausts the budget: reliable semantic delivery needs its
own bounded retention/acknowledgement contract, while replaceable state and
terminal bytes can use their existing snapshot/journal recovery paths. Existing
provider/library workers also need
shared admission wired through actual retirement. Focused foundation tests do not
establish whole-workspace, release, live PTY or performance acceptance.

Initial state synchronization now estimates retained tree, evidence and terminal
replay cost, reserves connection-event storage before cloning the tree snapshot,
and carries the same lease on every ordered direct-queue entry through consumption.
It rechecks tree size after admission, rejects a prepared batch that exceeds its
reservation, and stops stale-state retries after eight attempts with an explicit
error. This source change and its queue regression are not yet compiled or run;
blocked-writer lifetime and current-source server/release qualification remain
open.

Matched frame-age measurements require equal clock boundaries. The retained
baseline draws synchronously; a measurement-only overlay now records field
request, field preparation completion and successful terminal flush without
modifying the original baseline. Its source hashes and scoped formatting are
verified; compilation, instrumentation overhead and runtime samples remain
outstanding. The candidate's terminal `prepared_at` is captured after buffer
validation, whereas the baseline overlay's terminal age starts before UI work.
Those terminal `frame_age_us` values must therefore be reported separately.
Animation field-ready age includes cached retrieval, and must not be described
as the age of the original cache generation. Matched workloads, logging settings,
warm-up and whole-process CPU/memory still require real release runs after builds
stop; source timestamps alone prove no performance improvement.
