### Added
- Windows x86_64 preview downloads in prerelease v0.1.1-preview.6: per-user setup EXE, MSI and ZIP, built and installer-tested natively on GitHub Actions. They are unsigned, so SmartScreen warns. The release also carries the Linux files of v0.1.1-linux.3 and the macOS files of v0.1.1-preview.5 unchanged.

### Fixed
- On Windows, a deep `%LOCALAPPDATA%` (a long user name or a redirected profile) no longer stops Ilium with "project session socket path is too long". That limit belongs to Unix sockets; Windows sessions use named pipes named by a digest.
- Uninstalling the Windows MSI now restores the user PATH exactly as it was, including a trailing `;` that the previous WiX Environment entry dropped. The MSI records the original value at install time, as the setup EXE already did.
- On Windows, connecting to a busy session server now keeps retrying for up to 5 seconds instead of giving up after 20 attempts (about half a second), so a client connecting while the server is still restoring panes no longer fails with "every named pipe instance stayed busy".
- Restarting the client from the right-click menu on Windows no longer crashes the terminal input thread: its console read now stops cooperatively at the next 100 ms poll instead of being aborted with `CancelSynchronousIo`, which made crossterm panic on `ERROR_OPERATION_ABORTED`.
- A panic that the client caught and survived, such as a failing animation scene, no longer prints its message and backtrace over the full-screen UI. While the UI owns the terminal, panic reports go to the client log; a panic on the UI thread itself is printed after the terminal is restored.
- Emoji written with the U+FE0F presentation selector took one cell instead of two in Ilium's terminal parser, on every platform, and on Windows U+2637 took two cells where the console host draws one. A row filled to its last column then wrapped and pushed every later row down, which showed as shifted or missing Settings text in Windows panes. The vendored vt100 parser now counts both as the program that drew them does.
- The Growth background animation computed an out-of-range neighbour index before checking its bounds. Debug builds panicked on the multiplication overflow (the client caught it); release builds discarded the wrapped value, so installed binaries were not affected.
- The client failed to compile because the tree render borrowed the application state twice (ilium-client/src/ui.rs).
- A failed Windows native build now prints the compiler errors into the release job log.
- The workspace passes `cargo fmt --check` and Clippy with warnings as errors again, as the Windows release build requires (staged cache-wait modules in ilium-server, an unused voice parameter, two client lints).
- Several tests failed on Windows for reasons unrelated to the product: Git converting fixture line endings, short 8.3 temporary paths, the `.exe` suffix in help text, a verbatim `\\?\` path prefix and one racy redraw baseline. The fixtures and assertions now handle these; the earlier Windows-only repaint retries and the looser Settings check are gone. On Windows the terminal UI tests run one at a time, because several ConPTY sessions at once on a small runner lost input.
- The PowerShell installer can install a partial release, reading checksums from its AVAILABILITY file, and recovery keeps the resolved version.
- A pane invalidation that swept a suspended terminal command now reports the canceled command, like the other cancellation path; previously the silent sweep raced with it.
- The Windows dependency audit lists `bcryptprimitives.dll`, `psapi.dll` and `userenv.dll` as Windows system components; current Rust and process-inspection code import them, and the audit had blocked the release candidate.
- The Windows ONNX Runtime build receipt now passes its own audit: the compiler identity probe no longer suppresses the `cl` banner the audit reads (`/nologo`), and the audit expects the `CMAKE_GENERATOR` variable the build sets beside `CMAKE_GENERATOR_INSTANCE`. Checked against the receipt of a real Windows runner build. The retained CMake cache check likewise accepts that the two compiler paths come from CMake's generated compiler files, not the cache, which Visual Studio generators leave without them.
- The Windows installer job installs the pinned Inno Setup 6.5.4 even when the runner image already ships a newer one (`--allow-downgrade`).
- The Windows installers can be built for a preview release. The MSI carries a numeric ProductVersion (0.1.1.6 for 0.1.1-preview.6) and allows same-version upgrades, so a later preview or the final 0.1.1 replaces it. The setup EXE, the ZIP and `--version` keep the full preview version.
- Installer contract fixtures include the animation helper and bundled packs that the Windows installer requires.

### Changed
- The resource queue tests run only on Linux, and the persistent session-statistics cache tests only on Unix; other platforms instead assert that both refuse (fail closed).
- The release pipeline skips the Linux-host installer suites (POSIX install, Linux tarball, Linux VM) on non-Linux runners.
