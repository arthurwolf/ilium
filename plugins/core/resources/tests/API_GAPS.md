# API gaps of the resources feature tests

Items the tests call that the skeleton lacks, or manual details the tests had to assume.

- None yet: the crate is a skeleton and has no feature tests.

- Port status: `src/identity.rs` holds the pure sidecar rules of the project identity (UUID validity, schema version, size bound, never replace a broken sidecar) with three twin tests; the file access (`ResourceLedgerFile`, `secure_fs`) needs G10 (`fs`). The queue state machine (`store_rules`, with the 2434-line `service_tests.rs` twins), `memory`, `projections`, `cli` and `queue_view` are not ported: they need G10 (`journal`), G11 (CLI) and G8 (paged queue view). The manifest now declares the panels and extension points of the page.
