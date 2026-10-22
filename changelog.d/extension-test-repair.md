### Changed
- Removed the obsolete WebAssembly extension-system prohibition from project instructions, architecture and contribution guidance.

### Added
- Plugin API error helpers and resource-limit defaults, ceilings and clamping, with boundary coverage.
- Plugin host platform policy, checked artifact selection, call deadlines and time accounting, quota/reply rules, and automatic-update decision policy.

### Fixed
- Missing portable plugin artifacts now fail selection instead of reporting an unavailable fallback; disabled or metered automatic updates ignore stale candidates.
