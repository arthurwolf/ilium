### Fixed
- Completed server requests retain their audited backing-storage charge without monopolizing decoder execution permits throughout handlers or attachment barriers.
- Stalled request stages now log connection, operation, frame size, elapsed time, tenant and process quotas. Pre-dispatch decoder admission expires after 15 seconds; mutation handlers are never silently cancelled by that deadline.
