### Added
- Bounded monitoring event reads through `ilium ctl events` and `/v1/events`, with server-side filters, compact or JSON output, blocking follow, and named consumer acknowledgements through `ilium ctl ack`.
- Priority-sorted monitoring digests with evidence acknowledgements, bounded verified provider replies, minimal pane briefs, and event-driven waits over pane selections.
- Deterministic question and inactivity signals, local read/act/admin grants, durable write-attempt audit records, and a short agent monitoring guide.
- Cross-session tree, find, monitoring and wait reads with socket-scoped cursors, bounded aggregate output, partial errors and explicit-only write targets.

### Fixed
- Concurrent standalone connections retry transient admission-lock contention without retrying permanent capacity refusals.
- Monitoring acknowledgements commit only the delivered page, preserve omitted evidence, and suppress unchanged acknowledged context. Expired cursors report recovery positions; filtered completions clear old questions and questions retain priority over concurrent inactivity signals.
- Credential and audit storage reject nonregular files promptly; unavailable automation credentials preserve ordinary server startup.
