### Added
- Windows x86_64 preview downloads in prerelease v0.1.1-preview.6: per-user setup EXE, MSI and ZIP, built and installer-tested natively on GitHub Actions. They are unsigned, so SmartScreen warns. The release also carries the Linux and macOS files of v0.1.1-preview.5 unchanged.

### Fixed
- The client failed to compile because the tree render borrowed the application state twice (ilium-client/src/ui.rs).
- A failed Windows native build now prints the compiler errors into the release job log.
- The workspace passes `cargo fmt --check` and Clippy with warnings as errors again, as the Windows release build requires (staged cache-wait modules in ilium-server, an unused voice parameter, two client lints).
