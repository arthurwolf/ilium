## Detection design

### Why a process-tree check before text scraping

Text-scraping a banner is what most "detect the AI tool" hacks do, and it breaks the moment the tool changes its splash screen. Walking `/proc` (via `sysinfo`, cross-platform enough for Linux/macOS) to find that the pane's foreground process is literally named `claude`, `codex`, or `agy` is a much harder signal to break by accident, and it's cheap to compute alongside the poll. Text scraping is kept only as the activity signal, where there's no substitute (a process name can't tell you if the agent is mid-turn).

### Poll cadence

Observation health is separate from semantic activity: successful classifications carry the original observation timestamp and a server threshold of three polling intervals (minimum 30 seconds). Attach replay preserves timestamps. Clients replace stale or unchecked agent activity glyphs with a caution, retaining goal and unread-completion state, and redraw when an observation expires without requiring another server event. Current-frame capture never renews freshness from the retained presentation fallback; unavailable or oversized frames skip only their pane.

"A few times a minute" per pane, but adaptive rather than fixed:

- Panes currently `Working` or `WaitingApproval` poll every ~10s — for `Working` this still makes the state flip to `Done` prompt while avoiding needless process-tree and screen scans across a large fleet; for `WaitingApproval` it also corrects transient matches without leaving a stale "needs input" badge for a full slow-tier interval.
- `Done` is an unread completed-turn alert: both the tree and right-panel title prepend `« [done] »` until the user opens that pane, submits new terminal input, or detection observes renewed non-idle agent activity. Opening the pane clears the bell and marker for every attached client.
- Panes `Idle`/`Done`/`PlainShell` poll slow (~30–60s) — none of those change on their own between polls, no reason to burn CPU reading their screen buffer.
- All intervals configurable in `~/.config/ilium/config.toml`.
