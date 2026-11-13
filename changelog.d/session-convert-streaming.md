### Changed
- Converting a session between Claude Code and Codex now streams the transcript with low memory instead of holding it several times in RAM. The source limit rises from 128 MiB to 64 GiB, the converted copy may reach 8 GiB (with a free-disk check first), and the worker reservation is a fixed 320 MiB regardless of file size.
- Codex to Claude conversion truncates each tool output to 30,000 characters (with a marker) and refuses a rollout that changes during conversion.

### Fixed
- Converting a large Codex session no longer fails with "The source transcript exceeds the 128 MiB conversion limit".
