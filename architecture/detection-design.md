## Detection design

### Why a process-tree check before text scraping

Text-scraping a banner is what most "detect the AI tool" hacks do, and it breaks the moment the tool changes its splash screen. Walking `/proc` (via `sysinfo`, cross-platform enough for Linux/macOS) to find that the pane's foreground process is literally named `claude`, `codex`, or `agy` is a much harder signal to break by accident, and it's cheap to compute alongside the poll. Text scraping is kept only as the activity signal, where there's no substitute (a process name can't tell you if the agent is mid-turn).

### Poll cadence

Observation health is separate from semantic activity: successful classifications carry the original observation timestamp and a server threshold of three polling intervals (minimum 30 seconds). Attach replay preserves timestamps. Clients replace stale or unchecked agent activity glyphs with a caution, retaining goal and unread-completion state, and redraw when an observation expires without requiring another server event. Current-frame capture never renews freshness from the retained presentation fallback; unavailable or oversized frames skip only their pane.

"A few times a minute" per pane, but adaptive rather than fixed:

- Panes currently `Working` or `WaitingApproval` poll every ~10s — for `Working` this still makes the state flip to `Done` prompt while avoiding needless process-tree and screen scans across a large fleet; for `WaitingApproval` it also corrects transient matches without leaving a stale "needs input" badge for a full slow-tier interval.
- `Done` is an unread completed-turn alert: the tree row pulses and the Agent Detection plugin (`ilium.core.agent-detect`) shows its completed-turn mark on the row until the user opens that pane, submits new terminal input, or detection observes renewed non-idle agent activity. The kernel no longer prepends a `[done]` text marker to pane titles. Opening the pane clears the bell and marker for every attached client.
- Panes `Idle`/`Done`/`PlainShell` poll slow (~30–60s) — none of those change on their own between polls, no reason to burn CPU reading their screen buffer.
- All intervals configurable in `~/.config/ilium/config.toml`.

### Custom tree status rules

Normal and Attention retain their existing projections. Custom evaluates the globally configured ordered rules against pane facts in `ilium-core`; the first enabled match selects one status visual. The client owns editing, persistence and terminal rendering. Flat expressions are represented as OR groups of AND terms, with negation attached to individual terms; an unknown observation never becomes a match through negation.

Only the server owns the terminal parser. For enabled screen-pattern conditions, the client sends a bounded, connection-scoped watch set over IPC. The server compiles the patterns, observes each current visible grid and sends match identifiers or unknown results, rather than sending the grid for this feature. Screen generations and subscription revisions fence stale observations. Busy or oversized grids are unknown, and replacing a terminal or tree subscription invalidates prior answers. Watch tasks belong to the connection and are replaced or stopped with their subscription. Pattern count, length, compiled size and screen bytes are bounded. The client retries a watch request refused by its local queue and uses the production tree renderer for the live settings preview.

Screen-pattern cadence deliberately differs from the adaptive poll above: a watched pane is evaluated when its screen changes, at most once every 50 ms, plus a 150 ms retry when the parser lock was busy. An unchanged pane costs nothing, and a connection without watches runs no observer. Only changed answers are sent, so a client knows when an answer last changed, not when it was last checked. Each evaluation runs at most 16 linear-time regular expressions (patterns of at most 256 characters, at most 1 MiB compiled) over at most 64 KiB of visible text. Watches belong to the connection, so two attached clients with watches do the work twice. This trades a bounded per-pane cost for answers that follow the screen within a frame instead of up to a minute later.

The matched text is the terminal's visible contents with soft-wrapped rows joined, trailing blanks trimmed and trailing blank rows dropped; first-lines and last-lines regions count those lines, and scrollback is never read. The 64 KiB budget charges that text, not the grid size, so a large but sparse pane stays observable; only grids above 262,144 cells or text above 64 KiB are unknown. When the server refuses a watch set (invalid pattern, length or count limit), it sends a typed refusal naming the revision and, when there is one, the offending watch; it stops the previous revision's observer, and the client keeps every watch unknown and shows the message until the rules change. The client validates patterns with the same limits before sending, so a refusal signals a client/server limit mismatch rather than a normal editing path.
