### Added
- The plugin testkit retains native plugin instances, calls real activation/deactivation callbacks, and reconstructs fresh instances for grant changes and explicit restore restarts. Setup uses the canonical manifest parser and grant catalogue, closes at build or activation, and exposes deterministic clock and isolated in-memory storage snapshots. Component execution, SDK host imports and automatic failure recovery remain unfinished.

### Fixed
- The native harness manifest fixture lists only optional exports, keeping required lifecycle and handler exports implicit.
- Align the grant-change regression with the documented sorted activation-grant contract.
