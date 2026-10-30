### Fixed
- An agent process whose start time the system reports one second differently between reads is no longer treated as a replacement, which let a quarantined old session ID rebind after `/resume`.
