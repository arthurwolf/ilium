### Fixed
- Compile native-helper inspection and Unix process-restart test support only where their existing tests apply, keeping strict Clippy enabled on every release platform.
- Keep plugin-host signing, token rotation, store serialization, tamper detection and reinstall tests portable; isolate Unix permission-bit fixtures and retain their full Unix assertions.
- Canonicalize the session-conversion fixture project before creating its transcript store, matching production lookup on native temporary-directory aliases.
