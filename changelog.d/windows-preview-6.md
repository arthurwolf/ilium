### Added
- Windows x86_64 preview downloads in prerelease v0.1.1-preview.6: per-user setup EXE, MSI and ZIP, built and installer-tested natively on GitHub Actions. They are unsigned, so SmartScreen warns. The release also carries the Linux files of v0.1.1-linux.3 and the macOS files of v0.1.1-preview.5 unchanged.

### Fixed
- A panic that the client caught and survived, such as a failing animation scene, no longer prints its message and backtrace over the full-screen UI. While the UI owns the terminal, panic reports go to the client log; a panic on the UI thread itself is printed after the terminal is restored.
- Emoji written with the U+FE0F presentation selector took one cell instead of two in Ilium's terminal parser, on every platform, and on Windows U+2637 took two cells where the console host draws one. A row filled to its last column then wrapped and pushed every later row down, which showed as shifted or missing Settings text in Windows panes. The vendored vt100 parser now counts both as the program that drew them does.
- The Growth background animation computed an out-of-range neighbour index before checking its bounds. Debug builds panicked on the multiplication overflow (the client caught it); release builds discarded the wrapped value, so installed binaries were not affected.
- The client failed to compile because the tree render borrowed the application state twice (ilium-client/src/ui.rs).
- A failed Windows native build now prints the compiler errors into the release job log.
- The workspace passes `cargo fmt --check` and Clippy with warnings as errors again, as the Windows release build requires (staged cache-wait modules in ilium-server, an unused voice parameter, two client lints).
- Several tests failed on Windows for reasons unrelated to the product: Git converting fixture line endings, short 8.3 temporary paths, the `.exe` suffix in help text, a verbatim `\\?\` path prefix and one racy redraw baseline. The fixtures and assertions now handle these; the earlier Windows-only repaint retries and the looser Settings check are gone. On Windows the terminal UI tests run one at a time, because several ConPTY sessions at once on a small runner lost input.
- The PowerShell installer can install a partial release, reading checksums from its AVAILABILITY file, and recovery keeps the resolved version.
- A pane invalidation that swept a suspended terminal command now reports the canceled command, like the other cancellation path; previously the silent sweep raced with it.
- Installer contract fixtures include the animation helper and bundled packs that the Windows installer requires.

### Changed
- The resource queue tests run only on Linux, and the persistent session-statistics cache tests only on Unix; other platforms instead assert that both refuse (fail closed).
- The release pipeline skips the Linux-host installer suites (POSIX install, Linux tarball, Linux VM) on non-Linux runners.
