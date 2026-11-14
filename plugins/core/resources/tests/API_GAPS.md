# API gaps of the resources feature tests

Items the tests call that the skeleton lacks, or manual details the tests had to assume.

- None yet: the crate is a skeleton and has no feature tests.

- Port status: `src/identity.rs` holds the pure sidecar rules of the project identity (UUID validity, schema version, size bound, never replace a broken sidecar) with three twin tests; the file access (`ResourceLedgerFile`, `secure_fs`) needs G10 (`fs`). The queue state machine (`store_rules`, with the 2434-line `service_tests.rs` twins), `memory`, `projections`, `cli` and `queue_view` are not ported: they need G10 (`journal`), G11 (CLI) and G8 (paged queue view). The manifest now declares the panels and extension points of the page.

## Re-check of the foundation against the tree (2026-11-14): what still blocks the moves

- Store (`ilium-server/src/resources/`): the reservation ledger is read and written synchronously by IPC handlers (`ClientRequest` resource operations) and by the client queue; the plugin cannot own it until those requests are answered by the plugin: wanted `ClientRequest::PluginCall { plugin, method, payload }` with a typed reply, and the `journal` import backed on the server (`imports/journal.rs`).
- CLI (`ilium/src/resource.rs`): `ilium/src/plugin_cli.rs` and `runtime/cli.rs` route contributed subcommands, but `ilium resource` talks to the store through the kernel client requests above; port it when the store is a plugin service.
- Queue panel and dialogs (`resource_queue/`): panels can scroll and edit now, popups cannot take text (see the tree gaps); the context-menu `Resource*` arms need the plugin commands `ilium.core.resources/*`.

## Re-verified against the tree (2026-11-14, cluster pass)

- `ClientRequest::PluginCall` exists (`ilium-ipc/src/plugin.rs:111`, server `plugin_host/plugin_calls.rs`), but its only backed interface is the append-only `journal` (`log_event`, `read`). The reservation ledger (`ilium-server/src/resources/store.rs`, 63 internal uses, queue state machine, pause/offline, recover, overdue/uncertain owners, project identity sidecar) is not a journal: the plugin needs a durable keyed store under the `data` scope with compare-and-swap semantics, a typed `ClientRequest` answer from the plugin for the resource operations, and a blocking `ilium resource wait` through `plugin_cli.rs`. None of the three exists, and the 2434-line `service_tests.rs` has to be ported with it.
- Outside consumers that must move in the same step: `ilium-ipc/tests/resources.rs`, `ilium-instructions/src/resources.rs`, `ilium/src/progress_wait.rs`, the `Resource*` context-menu arms, `resource_queue_grouping` in the client config, and `ilium-server/tests/resource_queue_runtime.rs`.
