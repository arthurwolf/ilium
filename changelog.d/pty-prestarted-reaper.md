### Fixed
- Establish supervised child cleanup before starting terminal processes, preserving ownership through partial startup failures. Failed child termination requests retry at a bounded cadence and status-query failures report that cleanup remains pending.
