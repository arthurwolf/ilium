### Added
- Windows x86_64 preview downloads in prerelease v0.1.1-preview.6: per-user setup EXE, MSI and ZIP, built and installer-tested natively on GitHub Actions. They are unsigned, so SmartScreen warns. The release also carries the Linux files of v0.1.1-linux.3 and the macOS files of v0.1.1-preview.5 unchanged.

### Fixed
- The client failed to compile because the tree render borrowed the application state twice (ilium-client/src/ui.rs).
- A failed Windows native build now prints the compiler errors into the release job log.
- The workspace passes `cargo fmt --check` and Clippy with warnings as errors again, as the Windows release build requires (staged cache-wait modules in ilium-server, an unused voice parameter, two client lints).
- Several tests failed on Windows because of the test environment: Git converting fixture line endings, short 8.3 temporary paths, the `.exe` suffix in help text, a verbatim `\\?\` path prefix and one racy redraw baseline. The fixtures and assertions now handle these.
- The PTY smoke tests now run one at a time on Windows and retry a screen check once after a forced full repaint, and one Settings check also accepts the Settings frame without its title. ConPTY, which also sits between Ilium and Windows Terminal, sometimes dropped or shifted cells around wide emoji and lost a key from a burst of keys under load. Windows users may see the same display glitches; this has not been checked in a real Windows Terminal.
- The PowerShell installer can install a partial release, reading checksums from its AVAILABILITY file, and recovery keeps the resolved version.
- A pane invalidation that swept a suspended terminal command now reports the canceled command, like the other cancellation path; previously the silent sweep raced with it.
- Installer contract fixtures include the animation helper and bundled packs that the Windows installer requires.

### Changed
- The resource queue tests run only on Linux, and the persistent session-statistics cache tests only on Unix; other platforms instead assert that both refuse (fail closed).
- The release pipeline skips the Linux-host installer suites (POSIX install, Linux tarball, Linux VM) on non-Linux runners.
