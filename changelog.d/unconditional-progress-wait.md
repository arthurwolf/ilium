### Changed
- Progress waits (`ilium progress wait`, `ilium progress set --wait`, and `ilium wait`) no longer accept `--timeout-seconds` or end because a logical wait deadline elapsed. Existing flags now fail as unsupported; task, monitor, connection and interruption outcomes remain distinct. Transport and probe timeouts are unchanged.
- Updated progress command help, agent instructions and public documentation for indefinite logical waits. The accompanying native Codex integration bypasses terminal snapshots and outer execution cells; end-to-end behavior remains user-unverified.

### Added
- Codex 0.166.0 integration patch exposing a direct native `ilium_wait` tool for existing-monitor waits and registration with wait. It bypasses terminal snapshots and Code Mode cells and uses the existing sandbox, approval and cancellation lifecycle without a logical deadline. Local POSIX only; behavior is agent-unverified because feature testing is user-led.
