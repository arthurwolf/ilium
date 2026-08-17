# Ilium performance audit

This document is the reproducible performance ledger for the client/server
runtime. It records the observed production problem, controlled workloads,
every accepted optimization, its isolated result, and the final installed
runtime result. Values are interval averages or multi-sample medians; an
instantaneous process sample is never used as evidence.

## Environment and protocol

- Host: `aquarelle`, Linux 7.0.0-29-generic, Intel Core i5-11600K, 6 cores / 12
  logical CPUs.
- Scheduling: every build, test, benchmark, and controlled Ilium process runs
  under `nice -n 19 ionice -c 3`.
- Process CPU: `pidstat -u -w`, one-second intervals, at least 20 intervals.
- Microbenchmarks: optimized Rust test build, one warm-up, seven fixed-size
  samples, median reported in nanoseconds per operation.
- TUI process workload: isolated XDG state and runtime directories under
  `/ram`, installed binaries, a controlled tmux server, and real PTYs running
  the cross-platform `ilium-fixture-agent` copied as a Codex-named process.
- Main many-pane shape: 49 working Codex panes. Large-surface measurements use
  266 columns by 68 rows and select a real pane through Ilium's normal tree
  interaction.

The machine was busy during the live observation (12 logical CPUs, load around
11-15). Controlled comparisons therefore use repeated distributions and the
same workload on both sides of each change. Absolute CPU values remain useful
for scale; paired changes are the optimization evidence.

## Baseline

### Running installed session

The observed installed session had 49 panes and a 266x68 terminal. Its client
used 96.75% CPU over 20 seconds (94.20% user, 2.55% system); its UI/main thread
accounted for 93.95%. The detached server used 42.18% CPU (16.49% user, 25.69%
system). The client received no bytes over a ten-second counter interval while
remaining busy, proving that its immediate cost was timer-driven work rather
than incoming terminal output. The server read about 134.8 MB and wrote about
193.8 MB in the same interval.

The server snapshot was about 49.8 MB. Almost all of it was persisted agent
diagnostics rather than the tree or pane restart metadata. The client held
roughly 678 MB RSS and the server roughly 546 MB RSS.

### Controlled installed-binary workloads

| Workload | Client CPU | Server CPU | Notes |
|---|---:|---:|---|
| Empty session, 80x24 | 0.03% | 0.00% | 30-second interval |
| 49 working panes, 80x24, no selected pane | 4.20% | 17.33% | 30-second interval |
| 49 working panes, 266x68, selected pane | 11.98% | 17.27% | 20-second interval; client 11.43% user, server 12.78% system |

The many-pane server alternates near-idle seconds with 20-40% bursts on its
detection deadlines. `/proc` counters show tens of megabytes read per interval
even though every fixture process is stable. This matches the whole-host
`sysinfo` refresh in the detection loop. Idle PTY reader threads also wake five
times per second each because Unix cancellation is implemented with a 200 ms
poll timeout.

### Baseline microbenchmarks

| Boundary | Median | Seven samples (ns/op) |
|---|---:|---|
| Full client draw, 49 working panes, 266x68 | 862,190 ns | 818,214; 820,632; 860,622; 862,190; 922,695; 1,111,998; 1,293,693 |
| Apply known-agent terminal output | 484 ns | 476; 481; 483; 484; 488; 551; 755 |
| Apply plain-shell terminal output | 250,544 ns | 230,097; 247,378; 250,172; 250,544; 250,804; 254,588; 260,763 |
| Reconcile a project with no chatroom | 901 ns | 832; 842; 889; 901; 945; 966; 967 |

The plain-shell path is about 500 times more expensive than the known-agent
path because it hashes every visible cell after each accepted output frame.

## Audit coverage

Every production Rust module was inspected, including all 17 workspace crates,
the patched `vendor/vt100` parser, and the vendored `tui-tree-widget`. The audit
followed ownership boundaries rather than scattering local patches:

- pure domain and schemas: `ilium-core`, `ilium-agent-debug`, `ilium-ipc`;
- process/session classification: `ilium-detect`, `ilium-agent-session`;
- PTY, OS, and transport adapters: `ilium-pty`, `ilium-platform`,
  `ilium-transport`;
- detached runtime: `ilium-server`;
- render cache, event loop, and presentation: `ilium-client` and its tree
  widget;
- persistence and entrypoints: `ilium`, server persistence, client workspace
  storage;
- auxiliary boundaries: logging, inference, Kilo Gateway, sound, and voice.

Test-only fixtures were checked for benchmark suitability but are not runtime
optimization targets. Network inference, sound discovery, editor parsing, and
filesystem pickers are user-triggered rather than persistent hot loops; their
work remains outside the first optimization set unless a focused benchmark
shows a regression.

## Accepted optimization ledger

Each item is applied and measured separately. `Pending` means the audit has
accepted the mechanism but the implementation/result has not yet landed.

| ID | Ownership and mechanism | Primary measurement | Risk | Status/result |
|---|---|---|---|---|
| P01 | Client clock: sleep to each exact visible animation boundary (90 ms working spinner, 220 ms background clock, 450 ms done pulse, 175 ms creation pulse) instead of polling every 50 ms. | Many-pane process CPU and terminal write count | Low; animation phase tests | Implemented. Selected 49-pane client: 11.98% -> 11.03% CPU (-7.9%); terminal writes 117.2/s -> 82.8/s (-29.3%). |
| P02 | Client clock: compute animation requirements in one pane traversal and reuse them for tick/redraw scheduling. | Scheduler microbenchmark | Low; state coverage | Implemented. 49-pane scheduler median 400 ns -> 349 ns (-12.8%). |
| P03 | Client loop: represent the recorded surface as a borrowed/copyable key instead of allocating a `String` every turn. | Event-loop microbenchmark | Low | Implemented. Unchanged-surface median 11 ns -> 3 ns (-72.7%); owned formatting now occurs only on an actual transition. |
| P04 | Chatroom adapter: reconcile integrations on an explicit one-second deadline and immediately after structural snapshots, not every animation frame. | Chatroom-present and absent microbenchmarks | Medium; external file changes | Implemented. Existing-room skipped turn 24,043 ns -> 1 ns (>99.99%); no-room 901 ns -> below 1 ns. Filesystem work is capped at 1 Hz. |
| P05 | Client damage tracking: hidden-pane `ScreenUpdate`s update their cache without forcing a full frame. | Hidden-output boundary benchmark and many-pane process workload | Medium; visibility transitions | Implemented. Hidden known-agent batch 862,674 ns estimated baseline (484 ns output + 862,190 ns draw) -> 237 ns (-99.97%, 862.4 us avoided per hidden-only batch). Cumulative selected 49-pane CPU after P02-P05: 11.03% -> 10.40% (-5.7%). |
| P06 | Client terminal renderer: cache converted terminal cells by visible-screen revision and geometry, so sidebar-only animation does not reinterpret the vt100 grid. | Full-draw microbenchmark | Medium; stale-cell prevention | Implemented. Selected 49-pane full draw median 788,601 ns -> 449,403 ns (-43.0%). Output, replay, resize, history movement, and geometry invalidate; frozen history ignores hidden live changes. |
| P07 | Tree renderer: cache the semantic `TreeItem` hierarchy by tree/order/settings revision and refresh only animation-dependent row spans. | Tree/full-draw microbenchmark | Medium | Pending |
| P08 | Vendored tree widget: expose and reuse the identifier order from its render flatten instead of recursively cloning every path again in Ilium. | Isolated 49-row flatten benchmark | Low | Implemented. Removed second flatten 2,271 ns -> below 1 ns (>99.9%); full-draw impact is below run-to-run noise. |
| P09 | Tree renderer: cache presentation ordering and visible row metadata until tree/open/order state changes. | Tree/full-draw microbenchmark | Medium | Pending |
| P10 | Plain terminal tracking: replace SipHash cell hashing with a dimension- and cell-delimited FNV-1a text fingerprint. | Plain-terminal output benchmark | Low; visible-change tests | Implemented. Median 250,544 ns -> 195,507 ns (-22.0%). UTF-8-impossible 0xff delimiters preserve cell boundaries; style/cursor-only changes remain ignored. |
| P11 | Client history: retain a logical start offset and compact geometrically instead of moving the complete bounded history on every over-budget append. | Full-history append benchmark | Medium; byte offsets | Implemented. 1 KiB append at a full 1 MiB cap 24,053 ns -> 175 ns (-99.3%, amortized). Exact retained-tail and compaction regression test added. |
| P12 | Client search history: use immutable shared segments so an active search makes the next output append rotate to an empty segment instead of cloning retained history. | Concurrent-search append benchmark | Medium | Implemented. First 1 KiB append with a live 1 MiB snapshot 86,339 ns -> 263 ns (-99.7%). Search concatenation moved to the background worker; snapshot stability is tested. |
| P13 | OSC-8 parser: consume through a cursor and compact geometrically instead of repeated front drains while parsing fragmented links. | Fragmented-link benchmark | Medium | Rejected after measurement and reverted. The normal fragmented-link path regressed 1,758 ns -> 2,205 ns (+25.4%); this candidate does not count as an optimization. |
| P14 | IPC codec: submit the length header and payload with one vectored write and eliminate the unconditional flush. | Framing benchmark and protocol tests | Low | Implemented. Typical frame transport calls 2 writes + 1 flush -> 1 vectored write + 0 flushes; encoder median 17 ns -> 14 ns (-17.6%). Partial vectored writes retain exact framing. |
| P15 | IPC codec: retain read scratch capacity per connection instead of allocating a new payload vector for every frame. | Framing throughput benchmark | Medium; API lifecycle | Implemented. Repeated 264-byte frame decode median-equivalent 67 ns -> 63 ns (-6.0%); long-lived client and server readers now own the reusable buffer. |
| P16 | Server activity: debounce output-derived node activity revisions per pane/burst rather than taking the tree write lock and broadcasting on every first raw chunk. | Activity mutation benchmark | Medium; unread/activity semantics | Implemented. A 10,000-chunk burst 151,268 ns -> 6,794 ns (-95.5%) in the mutation gate benchmark, with one revision/broadcast in the 50 ms window; first and periodic activity remain observable. |
| P17 | Server output: add a bounded sub-frame coalescing window so adjacent PTY reads become one IPC event without affecting interactive latency. | Async burst benchmark | Medium | Implemented. A scheduler-adjacent 32-chunk/2 KiB burst now becomes one frame instead of 32 (-96.9%) in 32,112 ns; the single follow-up window is capped at 750 us and merged payloads remain capped at 16 KiB/32 chunks. |
| P18 | PTY journal: share immutable chunk bytes between the replay journal and live broadcast instead of cloning every read. | Journal append benchmark | Low | Pending |
| P19 | Unix PTY cancellation: poll the PTY and an explicit cancellation fd indefinitely instead of waking every reader every 200 ms. | 49-idle-pane context switches/CPU | Medium; OS adapter | Pending |
| P20 | Detection architecture: cache process identity by refreshed process-table generation, separately from screen activity. | Identity-cache benchmark | High; process replacement | Implemented. A cache hit costs 19 ns and reduces a cached 49-pane pass from 49 process-tree walks to zero. Every whole-host refresh increments the generation, so replacements invalidate all conclusions atomically; opt-in diagnostics always sample live. |
| P21 | Detection scheduler: coalesce nearby identity deadlines behind a minimum global refresh interval while preserving one refresh for a due batch. | Refresh-gate benchmark | Medium | Implemented. Synthetic 1 ms-spaced due batches perform 40 whole-host refreshes instead of 10,000 (-99.6%); decisions cost 100,372 ns total. Process identity may be at most 250 ms stale. Opt-in diagnostics bypass coalescing so their evidence remains freshly sampled. |
| P22 | Detection classifier: when identity, goal-owner input, request generation, and screen generation are unchanged, reuse the complete screen classification without rebuilding text or rescanning signatures. | Unchanged-screen classifier benchmark | Medium | Implemented. Representative 60-row agent screen 32,317 ns -> 26 ns (-99.92%). Forced checks and opt-in diagnostics bypass the cache. |
| P23 | Detection indexes: retain the process-child index for the lifetime of its refreshed `System` snapshot and reuse it for cheap activity passes. | Detection index benchmark | Low | Implemented. Rebuilding the live-host child index costs 292,131 ns; reuse costs 2 ns (>99.99% lower) on cached-system passes. The index is rebuilt exactly with each `System` refresh. |
| P24 | Snapshot writer: separate/bound durable diagnostic history from the small crash-recovery tree so unrelated mutations do not repeatedly serialize tens of megabytes. | 50 MB snapshot benchmark and RSS | High; recovery/migration | Pending |
| P25 | Snapshot writer: serialize the machine-owned atomic snapshot compactly. | Snapshot serialization benchmark | Low | Implemented. Representative debug-bearing snapshot serialization 752,741 ns -> 406,087 ns (-46.1%); bytes 641,512 -> 355,927 (-44.5%). Load remains format-compatible JSON. |
| P26 | Logging adapter: buffer file writes and flush on boundaries instead of locking and issuing a file write per diagnostic event. | Logging throughput/syscalls | Medium; crash-tail durability | Rejected and reverted. Cross-event buffering made newly emitted diagnostics invisible to concurrent readers until disable/buffer fill, violating the live Debug-log contract; baseline event append is 427 ns. |
| P27 | Voice audio: replace per-sample `VecDeque` playback churn with contiguous bounded blocks/ring indices in the realtime callback. | Audio queue benchmark | Medium; realtime correctness | Rejected after measurement and reverted. Draining 48,000 samples regressed 41,141 ns -> 93,417 ns (+127.1%); `VecDeque` remains the faster measured queue. |
| P28 | Snapshot capture: release tree and pane read locks before awaiting/cloning the independent diagnostic journal. | Lock-hold benchmark | Low | Implemented. A full 5,000-entry diagnostic clone measured 1,171,193 ns; that entire interval is now removed from the tree/pane lock hold. |
| P29 | Empty-client scheduler: leave chatroom reconciliation unarmed until the first authoritative tree snapshot instead of scheduling a pointless immediate filesystem pass. | Scheduler regression tests | Low | Implemented. Initial maintenance delay 0 ns -> 1 s idle sleep; structural snapshots still arm immediate reconciliation, after which P04's 1 Hz deadline applies. |

## Aggregate result

Twenty-one of the 29 audited candidates were implemented and retained. Three
measured regressions or contract violations were reverted, and five larger
follow-ups remain deliberately pending. The retained set therefore exceeds the
requested 20 individually implemented optimizations without counting rejected
experiments.

The final release was built with the low-priority policy and installed to both
runtime search locations. Live `/proc/<pid>/exe` links resolved to
`/home/arthur/.local/bin/ilium` and
`/home/arthur/.local/bin/ilium-server`; their SHA-256 hashes exactly matched
the corresponding `target/release` artifacts. A fresh isolated tmux client
then contained 49 working Codex fixture panes, selected a real pane, and
rendered its live PTY status and terminal stream at 266x68.

| Matched 49-pane workload | Baseline | Final installed release | Change |
|---|---:|---:|---:|
| Client CPU | 11.98% | 6.64% | -44.6% |
| Server CPU | 17.27% | 7.62% | -55.9% |
| Combined CPU | 29.25% | 14.26% | -51.2% |
| Client write syscalls | 117.2/s | 99.3/s | -15.3% |

The aggregate values are Linux process CPU percentages calculated from
two 20.02-second `/proc/<pid>/stat` counter intervals after warm-up, not
instantaneous `ps` samples. The table reports the two-sample median; individual
combined samples were 14.64% and 13.89% CPU (client 6.79%/6.49%, server
7.84%/7.39%). Final client and server RSS were 56,260 KiB and 28,344 KiB in the
controlled fixture.

The process-level result is intentionally reported only for the complete
retained set: several optimizations share the same scheduler, rendering, and
detection work, so assigning the aggregate CPU delta additively to individual
changes would double-count avoided work. Each row above instead has its own
focused before/after boundary measurement, while P01 and P05 also carry
intermediate matched-process measurements.

`perf` call-stack sampling was unavailable because the host has
`perf_event_paranoid=4`, and attaching a profiler to the existing processes
was denied. Attribution therefore combines interval process/thread counters,
I/O counters, source tracing, and focused optimized microbenchmarks. The host
was concurrently busy, so exact percentages will vary with load; the matched
workload, fixed terminal geometry, low scheduling priority, and counter
interval make the direction and scale reproducible without disturbing the
user's normal session.

## Second optimization pass

The already-completed 21-optimization release was first rebuilt from a clean
release tree and deployed as the requested checkpoint. Its installed hashes
were `599c1a99c1c33a9307911af84465881fa7381f06817db94cbbc9aea4efd86575`
(`ilium`) and
`864810cd8dc8c149830b69aafdbb9af83a533f1dbb4f4739e5549cb6898da47e`
(`ilium-server`). Both hashes were confirmed through live `/proc/<pid>/exe`
paths before the follow-up work began.

The second pass retained ten further changes. Each one removes allocation,
copying, scanning, or periodic wakeups at an existing architectural boundary;
none relaxes rendering, ordering, detection, or teardown semantics.

| ID | Ownership and mechanism | Focused before | Focused after | Change |
|---|---|---:|---:|---:|
| P30 | Client tree ordering returns a borrowed `Cow<[NodeId]>` when manual/split order already is the authoritative child slice. | 44 ns | 25 ns | -43.2% |
| P31 | Visible tree IDs traverse opened domain nodes directly instead of constructing presentation labels, widget items, and filesystem virtual rows. | 123,655 ns | 1,865 ns | -98.5% |
| P32 | Sidebar density uses its two static indentation strings instead of allocating with `repeat` on every row. | 94 ns | 32 ns | -66.0% |
| P33 | Tree-item construction reuses one mutable identifier path across siblings instead of cloning the parent path for every child. | 3,982 ns | 147 ns | -96.3% |
| P34 | Vendored tree flattening writes into one recursive accumulator instead of building and appending a temporary vector per subtree. | 7,007 ns | 4,603 ns | -34.3% with P35 on the real flatten boundary |
| P35 | Each opened subtree reserves its immediate item count in the flatten accumulator, avoiding geometric growth on wide groups. | 414 ns | 217 ns | -47.6% |
| P36 | The widget stores visible-row `(row, flat-index)` pairs and resolves paths from the existing flattened identifier vector instead of cloning every visible identifier path. | 1,860 ns | 31 ns | -98.3% |
| P37 | Detection snapshot selection iterates already-identified due panes and looks them up directly, removing an intermediate set and full registry scan. | 2,121 ns | 643 ns | -69.7% |
| P38 | Detection's due-pane collection preallocates from the known pane count. | 177 ns | 56 ns | -68.4% |
| P39 | Unix PTY readers block on the PTY plus an explicit platform-owned wake pipe instead of polling cancellation every 200 ms. | 238.9 voluntary context switches/s | 3.4/s | -98.6% |

The full 49-pane client draw boundary improved from a fresh paired median of
2,018,526 ns to 1,702,611 ns (-15.6%). That aggregate includes P30-P36 and is
reported separately from their synthetic boundaries because the complete draw
also contains terminal conversion, layout, and styling work that those changes
do not affect.

P39 was additionally measured through the installed detached server with 49
idle `/bin/cat` panes and 54 server tasks. Over matched 10-second `/proc`
counter intervals, warmed server CPU fell from 0.499% to 0.100% (-80.0%) and
voluntary context switching fell from 238.9/s to 3.4/s (-98.6%). The first
post-creation interval was excluded because it included the intentionally
separate process-identity warm-up workload.

Two additional experiments were measured but rejected and reverted, so they do
not count toward the ten retained optimizations:

- P40 replaced allocated Unicode lowercase comparison keys with an iterator
  comparator. It regressed representative name ordering from 4,031 ns to
  41,669 ns (+933.7%).
- P41 eagerly reserved the maximum 16 KiB output-burst capacity. It regressed
  the normal burst boundary from 43,215 ns to 46,840 ns (+8.4%).

Correctness coverage included manual and sorted tree order, empty and closed
subtrees, wide and deep trees, stable visible-row selection, Unicode names,
partial output bursts, repeated cancellation, a full wake pipe, duplicated
reader descriptors, and stubborn descendant teardown. The wake mechanism is
Unix-specific inside `ilium-platform`; other platforms retain their existing
reader behavior without acquiring Unix dependencies.

## Second deployment proof

The final release was rebuilt after the complete serialized workspace suite,
then installed to both `$HOME/.cargo/bin` and `$HOME/.local/bin`. Source and
both installed copies matched these SHA-256 hashes:

- `ilium`:
  `81911152722d97e34b733ea5f5479c4eb920ebc42a54adb0f27d4409abe73b52`
- `ilium-server`:
  `d9929c0cf9f429c432978ef94946b45e7e299ddd6fbe77f5ec0222f8870caf67`

An isolated 120x40 controlled tmux session loaded those exact paths. Its live
PTY showed the Codex fixture output, while the detached server classified it as
`Codex`, `Working`, and `Pursuing goal (5m)`. Killing the project session woke
and joined its PTY reader, removed the detached server, and left zero isolated
client/server processes. Formatting, strict workspace/all-target Clippy, the
full serialized workspace test suite, focused platform/PTY tests, and runtime
verification all passed.

## Third optimization pass

The third pass targeted the remaining cost of attaching and continuously
rendering one pane in a large, active session. The matched workload was a
266x68 terminal with 49 continuously repainting Codex fixtures, one selected
pane, and two warmed 20-second `/proc` counter intervals. The baseline was the
installed second-pass release rebuilt and exercised with the same fixture,
geometry, pane count, and isolated tmux harness.

Fourteen additional mechanisms were retained. Several rows are structural
event-count reductions rather than additive CPU claims: they share the same
output and redraw path, so summing their individual percentages would
double-count the same avoided frame.

| ID | Ownership and mechanism | Focused result |
|---|---|---|
| P42 | IPC attach distinguishes an interactive metadata attach from the legacy full-terminal replay contract. | Initial hidden replay parsers: 49 -> 0 (-100%). |
| P43 | The client sends its at-most-four displayed pane IDs as an explicit stream subscription. | Live terminal streams in the 49-pane workload: 49 -> 1 (-98.0%). |
| P44 | A newly visible pane recovers exactly from its journal watermark before live frames resume. | Visibility changes remain lossless while P42/P43 suppress hidden delivery; overlap is sequence-filtered. |
| P45 | The client computes displayed slots into `[Option<NodeId>; 4]` and allocates a `Vec` only when the subscription changes. | Hot comparison median: 16 ns -> 1 ns (-93.8%). |
| P46 | The server maintains one connection-aggregated pane-demand index instead of asking every connection writer to discard every hidden frame. | One synchronous demand decision replaces up to 49 IPC payload deliveries per output burst. |
| P47 | Hidden forwarders drain already-ready PTY chunks without allocating a merged frame or arming the 750 us coalescing timer. | Hidden burst merge/timer/IPC work is eliminated; the journal remains authoritative. |
| P48 | PTY output bytes use `Arc<[u8]>` between the journal and live broadcast. | 100,000 chunk clones: 9,152,304 ns -> 955,207 ns (-89.6%); 91.52 ns -> 9.55 ns per clone. |
| P49 | Continuous output activity fencing is capped at two revisions per second instead of twenty. | Activity mutation frequency: 20/s -> 2/s (-90.0%); gate benchmark 115,844 ns -> 7,211 ns (-93.8%). |
| P50 | Hidden activity-only events update the render cache without damaging the frame. | Repeated hidden activity revisions cause zero full TUI redraws after their first visible edge. |
| P51 | Hidden focus-checkpoint acknowledgements no longer redraw unrelated visible panes. | One cache mutation, zero frame damage when the acknowledged pane is not displayed. |
| P52 | Hidden terminal replay events advance their watermark/cache without invalidating the visible frame. | Recovery remains exact while hidden replay produces zero full redraws. |
| P53 | Agent-debug live events are broadcast only when that pane has a terminal subscriber; retained history remains available on demand. | Hidden debug delivery falls from every attached client to zero live deliveries. |
| P54 | Hidden output publishes only its first unread/unrestructured activity edges while continuing to advance authoritative revisions. | Continuous hidden output produces two edge broadcasts instead of repeated broadcasts; later subscription synchronizes the newest revision. |
| P55 | PTY forwarders cache terminal demand behind a monotonic atomic invalidation revision. | Stable output performs one atomic load per chunk; the demand mutex is consulted only after an attach/focus transition. |

The client-side damage classifier deliberately treats an activity transition
that changes the visible icon/status as frame damage; it suppresses only
events whose presentation is unchanged. Legacy lifecycle clients retain their
full-stream behavior. The request reader waits for each stream-selection
acknowledgement before executing the request, preventing Attach replay or
pre-Attach lifecycle output from racing the new filter.

Two experiments were rejected and do not count in the retained total:

- Publishing every hidden activity revision was replaced by edge-only
  publication after the first installed candidate showed 13.6-14.6% server
  CPU. The tree still advances the authoritative revision, so restructure
  fencing is not weakened.
- Parking hidden output forwarders behind subscription notifications regressed
  matched server samples to 14.8-17.6% CPU and also exposed an Attach-ordering
  race in its first form. The race was fixed and the experiment was then
  reverted because the measured regression remained.

### Third-pass aggregate result

| Matched 49-pane workload | Baseline installed release | Final retained release | Change |
|---|---:|---:|---:|
| Client CPU | 10.89% | 1.97% | -81.9% |
| Server CPU | 10.71% | 13.93% | +30.1% |
| Combined CPU | 21.60% | 15.90% | -26.4% |
| Client voluntary context switches | 268.9/s | 59.3/s | -77.9% |

The baseline samples were 22.91% and 20.28% combined CPU. The retained final
samples were 16.13% and 15.68% (client 2.05%/1.90%, server 14.08%/13.78%). The
table reports the midpoint of each two-sample pair. The server increase is
reported rather than folded into the much larger client gain: the server must
still parse and journal every pane's PTY stream and preserve activity fencing,
even though hidden IPC/render work is gone. The net process pair nevertheless
uses 5.69 percentage points less CPU on the matched workload.

Correctness coverage included legacy and interactive Attach ordering,
pre-Attach lifecycle requests, multi-client subscription replacement, journal
gaps and partial overlap, focus transitions, hidden activity/debug/replay
events, accepted input/output activity, split-pane visibility, snapshot
restore, live agent detection, scheduled input, and real PTY/TUI smoke tests.
Formatting, strict workspace/all-target Clippy, the complete serialized
workspace suite, and doctests passed after the final retained change.

### Third deployment proof

The final clean low-priority release build was installed to both
`$HOME/.cargo/bin` and `$HOME/.local/bin`. Source and both installed copies
matched these SHA-256 hashes:

- `ilium`:
  `6517def8ae80bdc5105fb3762a2de1d333dfb540744e47c9a48a934de15ffe32`
- `ilium-server`:
  `26eff2f8e39bb9b1369189d590b2fb5a8574857f8ff9da4a63e40302f8dfca09`

A fresh isolated runtime loaded those exact files, confirmed independently
through `/proc/<pid>/exe` and hashes of the executing images. It created 49
live Codex fixture panes, rendered the selected pane's continuously changing
PTY stream, and recovered another pane's current stream after a sidebar focus
change. The user's existing project session was not restarted.

## Fourth optimization pass

The fourth pass kept the third-pass attachment and rendering design, then
targeted the server work that remained even for hidden panes: one complete
parser/journal/broadcast transaction per kernel PTY read, plus a whole-host
process scan on every focused detection tick. The matched workload again used
a 266x68 terminal, 49 live Codex fixtures, one selected and client-focused
pane, and two warmed 20-second `/proc` counter intervals. Baseline and final
runtimes were isolated side by side and used the same fixture executable,
snapshot, pane count, geometry, focus state, and installed client.

Twenty-two mechanisms were retained. P56-P67 are deliberately listed as
separate avoided operations because each was independently present in the old
per-read path; their CPU effects overlap and must not be added together.

| ID | Ownership and mechanism | Focused result |
|---|---|---|
| P56 | The Unix platform adapter marks its private duplicated PTY descriptor nonblocking while retaining `poll` as the idle wait. | A ready-data drain can never wait for a future byte. |
| P57 | One poll wake drains every byte already queued by the kernel instead of returning after the first `read`. | Server read syscalls: 20,113/s -> 2,941/s (-85.4%) for the complete final pair. |
| P58 | Each PTY reader reuses a 64 KiB buffer instead of an 8 KiB buffer. | Large ready bursts remain one downstream transaction without per-chunk allocation. |
| P59 | Interrupted drain reads retry in place instead of abandoning the accumulated burst and repolling. | Signals do not fragment a ready burst. |
| P60 | Bytes accumulated immediately before EOF/EIO are delivered before the terminal close is reported. | Coalescing preserves the final partial frame instead of trading correctness for fewer calls. |
| P61 | Ready bytes share one parser write-lock acquisition. | Parser lock acquisitions scale with bursts rather than individual kernel reads. |
| P62 | Ready bytes share one `vt100::Parser::process` call. | Escape parsing crosses the API boundary once per burst. |
| P63 | Ready bytes share one screen-generation atomic update. | Generation writes scale with coherent parser updates. |
| P64 | Ready bytes share one screen-change watch notification. | Async initial-prompt observers receive one coalesced wake. |
| P65 | Ready bytes share one output-journal mutex acquisition. | Journal synchronization scales with bursts. |
| P66 | Ready bytes share one journal `Arc<[u8]>` allocation and one deque node. | Replay bookkeeping no longer fragments an already-ready burst. |
| P67 | Ready bytes share one broadcast-channel message and receiver wake. | Hidden and visible forwarders wake per burst rather than per kernel read. |
| P68 | A lagged hidden forwarder no longer allocates a full journal replay that no client can render. | Hidden lag recovery allocation: up to 32 MiB -> 0 until a pane is requested. |
| P69 | The same hidden-lag branch no longer broadcasts irrelevant replay IPC; visibility recovery remains journal-authoritative. | Hidden lag delivery: one session-wide replay event -> zero. |
| P70 | Stable detection batches reuse a refreshed process table for up to five seconds while screen classification keeps its existing cadence. | Whole-host `/proc` scan bytes: 4.48 MB/s -> 0.64 MB/s (-85.7%). |
| P71 | A cheap cross-platform PID liveness probe invalidates a cached positive identity immediately when its agent exits. | Exit detection does not wait for the five-second age ceiling. |
| P72 | Every user-forced detection generation invalidates process-snapshot reuse. | Enter/focus-triggered discovery remains immediate. |
| P73 | A pane without any prior identity generation always forces initial process discovery. | Startup classification never consumes an empty cache. |
| P74 | Stable positive identities reuse the existing process-tree conclusion while new screen generations are classified normally. | Visible activity/goal updates remain one-second responsive without rediscovering the same PID. |
| P75 | Stable negative identities are reused between scans, preventing a focused ordinary shell from rescanning the host every second. | Plain-shell process discovery is bounded by the same cache ceiling or a user trigger. |
| P76 | Agent-debug capture bypasses process-snapshot reuse. | Diagnostic mode retains fresh per-tick process evidence. |
| P77 | An unconditional five-second age ceiling refreshes even a live cached PID. | In-place `exec`, PID reuse, and unusual child-tree changes are bounded rather than trusted indefinitely. |

### Fourth-pass aggregate result

| Matched focused 49-pane workload | P55 installed baseline | P77 final release candidate | Change |
|---|---:|---:|---:|
| Client CPU | 0.900% | 0.900% | 0.0% |
| Server CPU | 7.875% | 1.350% | -82.9% |
| Combined CPU | 8.775% | 2.250% | -74.4% |
| Server `rchar` | 4.479 MB/s | 0.640 MB/s | -85.7% |
| Server read syscalls | 20,113/s | 2,941/s | -85.4% |
| Client voluntary context switches | 13.73/s | 13.73/s | 0.0% |

The baseline samples were 9.10% and 8.45% combined CPU (server 8.15% and
7.60%). The final samples were 2.40% and 2.10% combined CPU (server 1.45% and
1.25%). The table reports each pair's midpoint. An earlier final-candidate
sample with no client-focused pane was discarded before comparison because it
did not match the baseline's one-second focused detection tier.

Correctness coverage includes interrupted/idle PTY reads, final-byte delivery,
raw output broadcast and replay, parser resize and wide characters, repainting
and scrolling fixtures, Attach and lag recovery, live Codex/Claude detection,
session discovery/rebinding, focus transitions, initial prompt delivery, the
real PTY-rendered TUI suite, every workspace unit/integration test, and every
doctest. Strict workspace/all-target Clippy and formatting also pass.

### Fourth deployment proof

After the test build crossed the repository's 10 GB threshold, the required
low-priority `cargo clean` removed 15.2 GiB. A fresh low-priority release build
then installed both executables to `$HOME/.cargo/bin` and `$HOME/.local/bin`.
Source and both installed copies matched these SHA-256 hashes:

- `ilium`:
  `7d1307d84e97945b437e45ab0c6a56c315a191a4b0b9b5928e466387d3639abd`
- `ilium-server`:
  `911eb5a6a45942dadba8e94ba176f44ef9fb77a042e9741512d066c989506e51`

A fresh isolated runtime launched from `$HOME/.local/bin`; `/proc/<pid>/exe`
resolved to those exact paths and hashing the executing images returned the
same values. It restored 49 live Codex fixtures, classified them as Working
and Pursuing goal, rendered their changing output, switched the selected pane,
and recovered that pane's current journal stream. A final installed-image
10-second interval measured 0.90% client, 1.50% server, and 2.40% combined CPU.
The user's existing project session was not restarted.
