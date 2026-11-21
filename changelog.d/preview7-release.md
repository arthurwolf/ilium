### Added
- Added the published `v0.1.1-preview.7` prerelease: 23 files (the seventeen preview.6 files carried unchanged, the macOS Intel tar.gz, ZIP, PKG and DMG, and the Linux ARM64 RPM and Snap) with one `SHA256SUMS`; the README table and installation guide now link every format, and the shell installer pins the macOS Intel archive.

### Changed
- Preview qualification now accepts a native run whose failed jobs were rerun after a hosted-runner loss: the attempt number only has to be a real attempt, while the latest-attempt job and step checks and the same-run, same-commit artifact checks still prove every required lane passed.

### Fixed
- Installers now validate carried preview archives against their original embedded versions and exact published checksums when a newer preview release includes those unchanged files.
- The preview Pages and GHCR workflows no longer rewrite tracked Python bytecode files in their checkout, which made the qualification checks reject the checkout as modified.
- The README now documents the published `ghcr.io/arthurwolf/ilium-release:v0.1.1-preview.7` OCI bundle and how to pull and verify it.
- The macOS Apple Silicon installer pin now names the preview.5 archive that the complete preview carries (its bytes differ from preview.4), so the installer pins match the published checksums.
- The preview Pages baseline check now probes the expected-absent routes with the pipeline User-Agent, because Cloudflare answered the bare default Python agent with HTTP 403 instead of the 404 the check needs.
- The preview Pages deployment lookups now request 25 deployments per page, the most the Cloudflare API accepts, instead of 100, which it rejected with HTTP 400.
- Every HTTPS read-back in the preview Pages workflow (served bytes, headers and the exact custom 404) now sends the pipeline User-Agent, so Cloudflare no longer answers it with error 1010.
