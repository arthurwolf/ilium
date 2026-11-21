### Added
- Bounded polling-detection foundations for Claude and Codex activity, private evidence, and conservative spending accounting. Runtime coordination and the settings interface are still under integration; automatic detection is not active.

### Fixed
- Preserve polling ledger leadership across bounded configuration reloads and distinguish readable empty accounting from unavailable history in server projections.
- Keep polling endpoint publication independent of optional resources and preserve admitted evidence storage through server projections and verified acquisition. Private subscription authentication is verified; activity delivery remains under integration.
- Keep polling activity windows independent for different server incarnations even when session labels and pane numbers match; baseline private captures prospectively and refuse delivery after opt-out, lease loss or queue saturation.
- Coalesce numeric polling settings for 600 ms, retain drafts through delayed snapshots and failed writes, and display readable detector and save states.

### Changed
- Polling detection reviews the latest 30 agent events by default (was 15): on a labelled synthetic replay the 30-event window caught all six repeated-behaviour traces without false alarms, against three and four of six at 10 and 15 events, for about 650 estimated tokens per review.
