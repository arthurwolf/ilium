# API gaps found by part 2 of lane registry-publish

- crate ilium-plugin-host, `ilium_plugin_host::cli`: `trust add <file> --tier <T0..T3> --system` and a `trust remove` that refuses keys with `added_by: "admin"`; PKG-465; the flags are not in the `trust add` table, so the test only writes a store file and calls `trust remove`.
- crate ilium-plugin-host, error code naming: the guide says an expired timestamp reports `registry.stale` (PKG-468) while PKG-365/PKG-541 say `registry.metadata-expired`; tests accept either with exit 8, the manual should name one.
- crate ilium-plugin-host, `ilium_plugin_host::policy`: the policy page reports every block as `policy.blocked` with a detail prefix (`policy.id-denied:`), part 1 expects `policy.source-denied` as the code; tests assert exit 7 plus the detail text in the output; PKG-460..464, 470..472, 475.
- crate ilium-plugin-host, `pub fn auto_update_check(state: &State) -> AutoUpdateOutcome` (no CLI entry point exists for automatic updates); PKG-477; tests use `update` without a trust decision as a stand-in.
- crate ilium-plugin-host, store view model: `pub struct StoreView { rows, categories, empty_state, update_badges, stale: Option<Duration> }` built from the registry cache (Browse tab has no skeleton type); PKG-530..PKG-541; tests use `search`, `info` and `install` instead.
- crate ilium-plugin-host, `search --category <name>` flag with `usage.invalid-value` for names outside the closed list; PKG-531.
- crate ilium-plugin-host, publish review levels and the 20-per-hour limit have no wire format (PKG-400, PKG-401, PKG-507, PKG-504); only page-text tests exist.
- crate ilium-plugin-host dev-dependencies: `ed25519-dalek`, `sha2`, `base64` (already used by common.rs); PKG-652.
