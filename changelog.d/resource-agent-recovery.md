### Changed
- `ilium resource recover` now lets an agent of the project clear a stale resource owner (after a reboot or a dead owning process) in one step, without a typed confirmation; only revoking a live owner still needs the user. The generated agent instructions say so, and `--expected-revision` is optional.
