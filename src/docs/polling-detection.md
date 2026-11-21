# Polling detection

**Settings → Polling Detection** contains the controls and accounting view for advisory review of repetitive agent activity. One elected detector for your whole user account watches the Claude and Codex sessions of every running Ilium session. When enabled it reviews bounded excerpts, within your monthly limit, and lists findings on this page, newest first, with the pane, the session, the model's confidence estimate and local counts of similar calls and identical updates. The page refreshes about every five seconds while it is open.

The intended findings distinguish three behaviors:

- **Possible polling:** repeated nonblocking checks of the same pending operation without meaningful new evidence or useful work.
- **Repeated updates:** unchanged progress narration.
- **Wait churn:** repeated visible continuations of one blocking wait, without claiming that each continuation queried the underlying job.

A normal progress monitor, one blocking wait, changing results or useful independent work does not by itself establish polling. Findings are advisory. Detection never sends commands to an agent, stops a process or restarts a session.

## Controls and saving

Detection starts off. The tab explains the OpenAI destination, bounded evidence and paid use before the enable control. **OpenAI credential** opens the existing key editor in Inference settings; it does not change your inference provider.

Numeric controls save after 600 ms without another change. Toggles and scope choices save immediately. The tab distinguishes pending changes, saving, saved, invalid values and failed saves. A failed save retains your draft. Change a value to retry after correcting the underlying file or configuration error. A delayed server update does not replace an outstanding local draft.

| Control | Default |
| --- | --- |
| Check every | 5 new agent events |
| Recent window | 30 events |
| Minimum check spacing | 60 s |
| Local candidate filter | On |
| Decision threshold | 80%, provisional |
| Maximum evidence | 3,000 estimated tokens |
| Scope | All sessions |
| Monthly limit | $5.00 |
| Unlimited budget | Off |
| Alert cooldown | 15 min |

Tool results provide context without incrementing the event interval. Settings are stored under `[polling_detection]` in `config.toml`.

## Status and accounting

**No API key**, **Budget reached**, **Offline / backoff**, **History unavailable** and **Unsupported agent** describe different conditions; none means that an agent was reviewed and found clean. **Waiting for activity** also does not establish that a review has happened.

Spending belongs to the detector's own requests. Today, this month and all-time totals distinguish a readable empty ledger (**No requests yet**) from unavailable history. Unknown or possibly charged requests remain explicit; missing pricing is not displayed as zero. Returned usage and locally estimated costs are different measurements, not a provider invoice.

## Evidence and privacy

The detector is intended for verified Claude Code and Codex main transcripts. Its provider input contains bounded short messages and local tool summaries, with secrets redacted again before sending. Full transcripts, arbitrary command arguments and raw tool output are excluded from the compact provider evidence. The permanent spending ledger stores request and accounting metadata rather than transcript text or the API key.

Related: [Agent monitoring](agent-monitoring.md), [Inference and privacy](inference-and-privacy.md), [Settings](settings.md).
