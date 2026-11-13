# API gaps of the Cost feature tests

Every gap-dependent item is used only in `tests/features/common.rs`, so a missing item breaks that file alone.

- `FakeHost::kernel_settings(&mut self) -> &mut FakeKernelSettings` with `set_tab(tab: &str, values: serde_json::Value)` and `written(&self) -> Vec<(String, String, String)>` (tab, key, JSON). Needed for the reset-planning monitors, the `[cost]` reads and kernel-tab writes (COR-033, F-116, F-088).
- `FakeHost::agent(&mut self) -> &mut FakeAgent` with `set_model(pane: u64, AgentModel)` and `set_transcript(pane: u64, Vec<TranscriptTurn>)` (F-038, F-039, F-040, F-031).
- `FakeHost::published_topics(&self) -> Vec<(String, String)>` (topic and payload JSON), for `ilium.core.cost/usage-changed`.
- Documentation: no page names the server-to-client message (channel and payload) that carries panel figures, so a data-driven golden render is not testable.
- Skeleton: `CostClient` has no `Views` export and the manifest has no `contributes.panels` entry for `cost.panel`, although activation and two figures use it.
- Documentation: the Reset planning settings page render is not specified.
