### Changed
- The server no longer carries the per-pane "queued completion" bookkeeping of the old prompt queue (a completion epoch per pane, its claim check at input admission, and the matching `*_with_completion_locked` submit variants). The Scheduler plugin tracks completions itself, so automated input is admitted without it.
