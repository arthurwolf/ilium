### Fixed
- Retry transient conversion admission contention and queue saturation instead of incorrectly reporting a busy worker or waiting for a notification that may never arrive.
- Keep conversion recovery available until the server confirms the replacement pane, and report lost confirmations as uncertain.
