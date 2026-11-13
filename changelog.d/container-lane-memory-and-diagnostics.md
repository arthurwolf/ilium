### Fixed

- The AppImage container qualification lane now works under `/var/tmp` instead of the guest's memory-backed `/tmp`, so its image, extracted copy and cache no longer count against the 768 MiB caller budget and get the guest killed.
- When the delegated user-service probe outlives its limit, the container guest now records the unit status, manager journal and process list so the stall can be diagnosed.
