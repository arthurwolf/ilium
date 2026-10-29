# API gaps of the manifest lane (part 1)

- `ilium-plugin-api/Cargo.toml`: needs `[[test]] name = "manifest" path = "tests/manifest/main.rs"` and dev-dependencies `proptest` and `serde_json` (PKG-001..PKG-577, the tests are one binary).
- `ilium-plugin-api::limits`: `pub fn effective(manifest: Option<u64>, user_plugin: Option<u64>, user_global: Option<u64>, key: LimitKey, tier: Tier) -> u64` (PKG-074: `min(ceiling, first set of the per-plugin, global and manifest values, default)`, 0 or missing means not set). Tests only cover `clamp`.
- `ilium-plugin-api::manifest::ValidationContext`: a `signing_key_display_name: Option<String>` field (PKG-017: `manifest.publisher-mismatch` is a stage 7 trust check that needs the key's display name); the test is a `doc_` test only.
- `ilium-plugin-api::manifest::Requires`: a `wait_ms: Option<u32>` field with 0..=5000 validation (PKG-027); tests reach it through JSON only.
- `ilium-plugin-api::manifest::Manifest` serialization order (PKG-010) assumes the struct field order equals the table order of reference-manifest.md.

# API gaps of the manifest lane (part 2)

- `ilium-plugin-api` (or host): PKG-581 `policy.source-denied` needs a source check, for example `pub fn check_source(source: &str, allowed_sources: &[String]) -> Result<(), Diagnostic>`; part 2 only has `doc_` tests of the catalogue row.
- `ilium-plugin-api::manifest::InstalledSummary`: a `cli_names: Vec<String>` field so `manifest.cli-name-taken` against another installed plugin (PKG-582, PKG-065) can be tested; tests cover same-plugin duplicates only.
- PKG-578: stage and entry ordering of a packed archive with two bad entries needs the package checker (ilium-plugin-host/cli lane); only manifest pointer order is tested here.
