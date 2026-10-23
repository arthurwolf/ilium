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
- Preserve bounded completion ownership through repeated event-queue backpressure, and classify real WebAssembly start deadlines and resource-limit traps.
- Implement API-version parsing, compatibility mappings and source-aware bounded event queues; align scheduler pointer and view-action delivery with the documented payload rules.
- Connect trusted no-import WebAssembly component fixtures to the real test harness, preserving capability rejection and completion ownership.
- Add pure dependency resolution and settings schema validation, UI-tree structure and resource checks, and constant-time token comparison through the established cryptographic utility.
- Implement deterministic stored-ZIP package encoding and strict Ed25519 signatures over exact manifest bytes; reject unverifiable key identifiers with the documented signature diagnostic.
- Add strict installed-record persistence and checked version paths, deterministic lifecycle ordering, offline package admission checks, consent timing and restrictive policy evaluation. Runtime policy, trust and durable-install integration remain in progress.
- Add grant review and explicit approval handling, shared durable audit refusal accounting, declared archive expansion limits and same-side toxic-permission policy review; remaining runtime and durable installation connections remain under implementation.
- Add explicit-context file-install command transport, read-only package inspection, real session grant inspection, bounded persistent plugin storage and telemetry, and verified registry metadata admission. Runtime command adapters remain under integration; no approval is inferred from presentation flags.
