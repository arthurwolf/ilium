# API gaps, lane imports-core part 2

All items belong to `ilium_plugin_host::testing::imports_fixture::ImportsFixture` unless noted.
Tests use the accessors and builder methods of part 1 (`tier`, `grants`, `manifest_json`, `limit`,
`advance`, `pump`, `events`, `add_group`) as listed in API_GAPS.md.

- ilium-plugin-host: `pub fn storage(&mut self) -> &mut dyn HostStorage` (WIT:ilium:host/storage.*), accessor like `settings()`.
- ilium-plugin-host: `pub fn timers(&mut self) -> &mut dyn HostTimers` (WIT:ilium:host/timers.*), accessor.
- ilium-plugin-host: `pub fn tree(&mut self) -> &mut dyn HostTree` (WIT:ilium:host/tree.*), accessor.
- ilium-plugin-host: `pub fn info_text(&self, key: &str) -> Option<String>`, current text of a live info row (settings.set-info).
- ilium-plugin-host: `pub fn info_spans(&self, key: &str) -> Vec<RichSpan>`, current spans of a rich info row (settings.set-info-rich).
- ilium-plugin-host: `pub fn row_note(&self, key: &str) -> Option<String>` (settings.set-row-note).
- ilium-plugin-host: `pub fn row_state(&self, key: &str) -> RowState` (settings.set-row-state).
- ilium-plugin-host: `pub fn item_state(&self, key: &str, index: u32) -> Option<RowState>` (settings.set-item-states).
- ilium-plugin-host: `pub fn meter_segments(&self, id: &str) -> Vec<MeterSegment>` (settings.set-meter).
- ilium-plugin-host: `pub fn other_caller_lease(&mut self, key: &str, ttl_millis: u32) -> bool`, calls storage.lease as a second caller (another plugin/side) (storage.lease).
- ilium-plugin-host: `pub fn make_bucket_data_version_newer(&mut self, bucket: Bucket)`, simulates a newer data version on disk so write-batch reports `conflict` "stale-data-version" (storage.write-batch).
- ilium-plugin-host: `limit("storage_bucket_mib", u64)` limit name for the per-bucket quota (storage.put, storage.write-batch).
- ilium-plugin-host: `limit("node_metadata_kib", u64)` limit name, clamped to the tier ceiling (tree.set-metadata).
- ilium-plugin-host: `pub fn last_import_error_code(&self) -> Option<ErrorCode>`, code of the last refused infallible import call such as tree.get without grant (tree.get, tree.children, tree.roots, tree.find, tree.metadata).
- ilium-plugin-host: `pub fn add_group_under(&mut self, parent: NodeId, title: &str) -> NodeId` (tree.*).
- ilium-plugin-host: `pub fn add_terminal_pane_under(&mut self, parent: NodeId, title: &str, cols: u16, rows: u16) -> NodeId` (tree.*).
- ilium-plugin-host: `pub fn set_agent(&mut self, node: NodeId, signature: &str)` (tree.get, tree.find).
- ilium-plugin-host: `pub fn lock_node(&mut self, node: NodeId)` (tree.get).
- ilium-plugin-host: `pub fn rename_node(&mut self, node: NodeId, title: &str)` (tree.get, tree.metadata).
- ilium-plugin-host: `pub fn other_plugin_metadata(&mut self, node: NodeId) -> Option<Vec<u8>>`, reads metadata as a second installed plugin (tree.metadata).
