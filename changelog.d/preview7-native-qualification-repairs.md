### Fixed
- The macOS Intel preview build now has enough time for both the ONNX Runtime build and the Rust product build inside the hosted job's six-hour limit, and streams its output unbuffered.
- Linux ARM64 native test fixtures bind each fake agent to the home that holds its transcripts, so the server's process-ownership checks keep passing without being relaxed; resource-queue terminal tests match the current panel wording.
- The preview container-package workflow is accepted by GitHub again: `TMPDIR` is set on the steps that need it instead of the job, where the `runner` context is unavailable.
