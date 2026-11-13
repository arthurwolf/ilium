### Added
- Native plugin test hosts deliver ordered event batches, track submission occurrences and completions, enforce canonical shape/grant admission and fixed-window deduplication, and preserve instance ownership during grant changes. The deterministic sound fake records accepted sounds; unsupported effects fail explicitly.

### Fixed
- Keep pending work with retained grants across reactivation without forwarding old completions into a new plugin instance.
- Arm duplicate-window test retries after completion callbacks and fake-clock advancement, retaining their assertions and limits.
