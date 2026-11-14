### Added
- Scene plugins can now call the host (`host()` with the grants of `capabilities.scene`) and export `job-runner`, so a scene's background work (downloads, decoding) can run as a host job instead of a kernel thread.
