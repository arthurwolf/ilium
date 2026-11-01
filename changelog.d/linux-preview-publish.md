### Added

- The release workflow can publish the Linux x86_64 preview prerelease (`v0.1.1-linux.1`) from the verified rehearsal artifacts: it checks every package hash, creates a draft, reads back the uploaded digests, and only then marks it a prerelease that is never the latest release. Without the explicit publish input it runs a dry run.
