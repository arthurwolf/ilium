### Fixed
- The macOS Intel preview build now has enough time for both the ONNX Runtime build and the Rust product build inside the hosted job's six-hour limit, and streams its output unbuffered.
- Linux ARM64 native test fixtures bind each fake agent to the home that holds its transcripts, so the server's process-ownership checks keep passing without being relaxed; resource-queue terminal tests match the current panel wording.
- The preview container-package workflow is accepted by GitHub again: `TMPDIR` is set on the steps that need it instead of the job, where the `runner` context is unavailable.

### Changed
- ONNX Runtime 1.24.2 build inputs now come from the project's own `ort-mirror-1.24.2` release because the upstream repository's downloads were removed. Runtime archives are re-assembled from the official NuGet package (Linux and Windows library hashes unchanged, macOS arm64 uses the NuGet build) and the source archive is the Software Heritage copy of the pinned commit, verified against its tree hash.

### Fixed
- The macOS package smoke test and image cleanup now retry a busy plain `hdiutil detach` (up to six attempts, never forced) instead of failing on a short-lived volume hold.
- Native Linux ARM64 qualification now limits Cargo to two parallel build jobs, because the hosted runner kept losing contact with GitHub while linking several debug test binaries at once; the macOS shutdown smoke test now prints the client's final screen when the client exits with an error.
- Added the published `v0.1.1-preview.7` prerelease: 23 files (the seventeen preview.6 files carried unchanged, the macOS Intel tar.gz, ZIP, PKG and DMG, and the Linux ARM64 RPM and Snap) with one `SHA256SUMS`; the README table and installation guide now link every format, and the shell installer pins the macOS Intel archive.
- Preview qualification now accepts a native run whose failed jobs were rerun after a hosted-runner loss: the attempt number only has to be a real attempt, while the latest-attempt job and step checks and the same-run, same-commit artifact checks still prove every required lane passed.
