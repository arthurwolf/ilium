### Fixed
- Prevent attachment recap replay from reacquiring the tree read lock behind a queued writer, which could deadlock shared server state.
- Capture terminal state with nonblocking parser and journal reads outside shared server guards; retry transient contention with a fixed deadline and report the pane, operation, elapsed time and cause when capture cannot progress.
- Close failed initial attachments without draining a stalled writer, releasing their journal cursor; bound initial seed storage waits without cancelling accepted recovery or mutation work.
- Report admission pressure on persistent deadlines even when unrelated work keeps completing, including requested resources and tenant/process usage.
- Keep execution-owner metadata charged when completed decoded requests release decoder execution credits.
- Bound initial stream-control queue and acknowledgement waits before request dispatch; a stalled writer now closes its connection without pinning the request reader indefinitely.
