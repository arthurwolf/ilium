### Fixed
- Repair server-test merge syntax and Rust 2021 compatibility in plugin configuration fixtures so cross-platform release qualification can compile the integrated workspace.
- Align integrated client permission notifications and terminal input ownership with their current contracts, and simplify plugin fixture validation for strict Clippy qualification.
- Correct cross-platform release evidence fixtures while retaining strict path, byte, source and native-custody validation; exercise the portable contracts in every native CI test lane.
- Restore integrated editor-request, async input-lock and attach-handshake test contracts, and remove duplicate unreachable incoming event patterns.
- Include the plugin host’s declared Unicode normalization dependency in the lockfile so locked release builds can resolve the integrated workspace.
- Scope the Linux-only helper sandbox integration harness to its documented platform; retain its complete Linux behavior and security assertions.
