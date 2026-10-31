### Fixed
- An agent process whose start time the system reports one second differently between reads is no longer treated as a replacement, which let a quarantined old session ID rebind after `/resume`.
- Widened the tolerated process start-time drift from one to three seconds, because a loaded machine reported a larger drift and the same agent was treated as replaced, which lifted the invalidated-session quarantine.
