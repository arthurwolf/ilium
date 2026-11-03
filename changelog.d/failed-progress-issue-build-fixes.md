### Fixed
- The plugin API manifest validator and the plugin host compile again on current toolchains: a schema pattern holds escaped rather than raw bidirectional control characters, and grants parse and print through `FromStr` and `Display`.
- The server runtime thread stack is 8 MiB instead of 2 MiB, so debug builds no longer overflow while booting the plugin host and restart paths.
