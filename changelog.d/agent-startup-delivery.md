### Fixed

- Valid blank terminal snapshots, including sequence zero, finish initial-screen loading. Current screens arrive before on-demand scrollback.
- Focused terminal delivery bypasses unrelated terminal backlog while retaining per-pane ordering, lifecycle barriers and shared memory limits.

### Added

- Opt-in numeric startup timing (`RUST_LOG=ilium_startup=debug` with file logging) records creation intent, request receipt, launch, first raw/parsed output and successful frame emission without terminal contents or command text.
- Separate 128-pane idle and busy runtime qualification measures fresh terminal creation rather than only selection recovery.
