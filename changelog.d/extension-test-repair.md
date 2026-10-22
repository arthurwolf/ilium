### Changed
- Removed the obsolete WebAssembly extension-system prohibition from project instructions, architecture and contribution guidance.

### Added
- Plugin API error helpers and resource-limit defaults, ceilings and clamping, with boundary coverage.
- Plugin host platform policy, checked artifact selection, call deadlines and time accounting, quota/reply rules, and automatic-update decision policy.

### Fixed
- Missing portable plugin artifacts now fail selection instead of reporting an unavailable fallback; disabled or metered automatic updates ignore stale candidates.

- Implement bounded extension scheduler queues, fairness, cancellation and execution quota contracts; reconcile stale test expectations with documented never-drop input and minimum retry rules.

- Add real Wasmtime engine compilation and static component validation, a precompiler, shared-quota epoch ownership, and fail-closed embedded-artifact loading; full guest admission and lifecycle integration remain under implementation.
- Implement lifecycle transition guards and the exact ten-minute restart window, including explicit stopped completion and dependency wait expiry.
