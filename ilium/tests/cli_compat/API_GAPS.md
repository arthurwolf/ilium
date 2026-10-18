# API gaps (cli_compat)

- crate `ilium` dev-dependencies: `ilium-ambient`, `ilium-plugin-host`, `serde_json`, `tempfile` must be dev-dependencies of the `ilium` crate. Ids: F-093, F-094, F-102, COR-041. Why: animations.rs and common.rs use them.
- crate `ilium-plugin-host`: `pub fn package::read_package(path: &Path) -> Result<Package, PackageError>` with a Debug form containing `old-format` for `.iliumanim`. Id: F-102. Why: refusal check.
- crate `ilium-ambient`: `style::Appearance.color_mode` field and `dither::DitherMode::ALL`. Ids: F-093, F-094.
- binary `ilium`: env `ILIUM_TEST_FAIL_PLUGIN=<plugin id>` makes that plugin fail activation; `--no-plugins` flag and `ILIUM_SAFE_PLUGINS=1`. Ids: COR-042, F-111. Why: failed-owner and safe-start scenarios.
