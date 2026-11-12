## Scaling to hundreds of agents

Ilium has to stay responsive with 200 or more agent panes in one session. This section is the
design target for that scale: what the design would be if it were started today, and the rules
that keep the existing code moving towards it. The measured before/after numbers for each pass
are in `PERFORMANCE.md` ("Many-agent scale pass"); `tools/scale-bench/` reproduces them.

### Principle: cost proportional to change, not to population

At 200 panes, anything done per pane per tick, per pane per frame, or per host process per tick
dominates. Each subsystem therefore has to satisfy one rule: **steady-state work is proportional
to what changed, and the rest of the population costs nothing.** Concretely:

| Subsystem | Cost must scale with | Must never scale with |
| --- | --- | --- |
| Detection | panes whose deadline expired, and their own process trees | all host processes or threads, every tick |
| PTY transport | bytes moved | number of idle panes (no timed wake-ups per pane) |
| Server to client | panes whose visible state changed | whole-tree snapshots for a single-pane change |
| Client rendering | rows on screen that changed | panes in collapsed groups or off screen |
| Project maintenance | files that changed | number of projects multiplied by a fixed clock |

### Detection

Detection owns one CPU and one I/O worker, separately queued from general requests and codecs. At bootstrap a reserved quota partition carves 736 MiB (576 MiB input, 96 MiB results, 64 MiB worker storage) and two worker slots from the existing process worker ceiling. The partition ledger holds its parent lease through the final retained result and physical worker teardown; no process memory ceiling or codec/PTY capacity gate is raised or reduced. The input envelope covers the process table, classification, discovery and staged transcript reads concurrently. Failed per-pane observations receive a bounded retry deadline without renewing observation time, so an unavailable oldest batch cannot starve healthy panes. Slow title reconciliation and completion-triggered prompt delivery run in a pane-owned actor with a bounded, coalescing mailbox, outside the detector coordinator; generation and terminal identity are revalidated before effects, and delayed completion delivery also rechecks idle status under the input gate.

Expensive detection evidence has explicit execution ownership. The coordinator
snapshots due-pane inputs and current session claims under brief registry reads,
then dispatches process-tree identification and bounded screen classification
to an admitted CPU job. Native process refresh, per-pane process discovery and
transcript evidence reads use admitted I/O jobs. Workers return typed results
whose retained input and output sizes remain charged until reconciliation. The
server rechecks pane and settings generations and recomputes exclusive session
claims under its mutation owner before applying classifications; duplicate
claims have no arbitrary winner. Refused, stale or oversized evidence cannot be
treated as an empty snapshot that silently clears current state.

- **One host process snapshot per interval, shared by every pane.** The snapshot is taken without
  per-thread entries (`sysinfo` `without_tasks()`): Linux otherwise lists every thread of every
  process, which multiplied the scan by about eight on a workstation running agents. Snapshot reuse
  is bounded in age (5 s), and a forced refresh (user focus, Enter, a new pane) is rate-limited:
  the tick waits for the minimum refresh spacing instead of rescanning the host several times a
  second.
- **A failing tick must still make progress.** A detection tick that is refused (admission,
  deadline, an unexpected error) backs off exponentially. It never retries every 250 ms with a
  full host scan, and its reservations are sized so the normal case cannot exceed the shared
  execution budget. (Before this pass, one agent pane without a discoverable transcript made the
  evidence and discovery reservations add up to the whole 128 MiB budget. Every tick failed, three
  times a second, and each failure rescanned the host.)
- **Fair, deadline-ordered batches.** At most 32 panes are classified per tick, chosen
  oldest-deadline first. Choosing by pane ID made high-ID panes (the newest agents) wait until
  every lower ID was idle. A batch larger than the cap is a normal condition, not a reason to
  rescan the host.
- **Liveness without rescans.** Between snapshots, cached agent PIDs are checked with a signal-0
  probe, which is a syscall per due pane rather than a host walk.
- Future step (not needed for the measured targets): on Linux, walk only each pane's descendants
  (`/proc/<pid>/task/<tid>/children`) and refresh just those PIDs, falling back to the host
  snapshot elsewhere.

### PTY transport

- Each pane currently owns a small set of OS threads (reader, write pump, owner/state machine,
  queued-input deadline worker, child reaper). Idle threads must **block on an event**, never on a
  short timer. The write pump blocks until it receives a job or its cancellation wake (it used to
  wake every 10 ms). The child reaper backs off from 50 ms to 1 s while the child keeps running
  and is woken at once on cancellation; it is not on the exit-detection path, because the
  detection loop reads the child's exit status itself. A 10 ms receive timeout across 200 panes
  alone is 20,000 wake-ups a second for no work.
- **Ordered pane writes.** `ilium-pty::owner::Engine` admits one active payload at a time and sends
  the native blocking write to that pane's dedicated `AsyncWriter` thread. The owner retains the
  operation receipt and its exact success, failure, or uncertain-prefix outcome; input and paste
  bytes cannot be interleaved by a query reply. Resize and reply barriers stay ordered with accepted
  input, while pane-generation checks prevent queued input or late receipts from crossing a pane
  replacement. Regressions exercise query replies during input, replaced-pane input refusal, and
  stale post-admission acknowledgements.
- The worker ledger caps total owned threads (`MAX_OWNED_WORKERS`, 8,192) and their stack bytes.
  PTYs draw on a separate quota sized from that cap, about 1,600 panes on Unix. The earlier
  1,024-thread cap stopped pane creation at about 195 panes, and a shared 512 MiB budget later
  stopped a restore at about 25.
- From-scratch target: one readiness reactor (epoll/kqueue/IOCP) for all PTY masters and one small
  parser pool, so thread count is O(1) in panes. The current per-pane thread model stays until
  that reactor exists for all three platforms; the rules above remove its idle cost.

### Server to client

- Interactive reveal sends the current terminal snapshot first; scrollback is requested separately. A complete blank sequence-zero snapshot is ready, distinct from an uninitialized client view. Live output and history retain their sequence fences.
- Agent-menu repository facts run in connection-owned cancellable tasks. A bounded canonical-repository cache coalesces queries; short-lived facts are advisory, and launch ownership admission remains uncached. Status scans use bounded concurrency outside the repository mutation lock.
- Startup diagnostics use the `ilium_startup` tracing target at DEBUG. Numeric request, pane, child-process and output-sequence identities correlate client intent, server receipt, launch, first raw read, parsed publication and successful terminal emission. Per-process monotonic times measure local intervals; cross-process wall times require a stable clock. The dedicated 128-pane fresh-start qualification uses synthetic terminal fixtures and reports censoring separately from successful latency percentiles.

- State changes that concern one pane travel as per-pane events. Status, prompt, progress, git
  state and detection evidence already did; title changes now send `PaneNodeChanged` (that one
  node) instead of a full `TreeSnapshot`. A full snapshot is reserved for structural changes
  (create, close, move, regroup), for title resets that may also move the pane, and for attaching
  clients. Remaining single-pane snapshot senders (prompt queue, scheduled input, freeze) are the
  next candidates.
- A detection tick publishes at most one status change per pane, so a pane's status cannot
  produce more events than ticks.

### Client

- **Redraws are frame-capped and damage-driven.** Receiving a server event no longer marks the
  frame dirty by itself; the applied event's damage does. Per-pane state that background agents
  report continuously (status, detected state, evidence, prompt, progress, git) redraws in the next
  capped frame (at most about 30 per second) instead of forcing an immediate draw each, and
  terminal output for panes that are not displayed marks nothing. Input and structural events stay
  immediate.
- **The sidebar does not descend into collapsed groups.** Children of a closed container become
  placeholder rows (enough for the open/closed affordance) without sorting or formatting their
  subtree, and name-based sort orders lower-case each name once per sort instead of once per
  comparison. Design target, not built yet: a retained row model cached behind a structural and a
  presentation revision, so a status change re-renders one row. Animated rows (working spinners)
  bound how much such a cache can save.
- **Periodic work stays off the population clock.** Codex screen copies for model icons are taken
  only when the one-second model scan is due (not every tick), the client executable path is
  resolved once, auto-freeze returns before walking panes when it is disabled, and cost tracking
  covers up to 1,024 panes (it silently stopped above 128) with sorted-slice membership checks
  instead of an O(n²) `retain`/`contains`. Project maintenance (Chatroom, agent instructions,
  hooks) still re-checks each project once a second on a background worker; its cost scales with
  projects, not agents. Design target: drive it from file-change notifications with a slow safety
  rescan.

