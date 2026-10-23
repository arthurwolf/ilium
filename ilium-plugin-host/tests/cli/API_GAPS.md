# API gaps: cli (conventions, new, build, check, test, pack, keys, trust, install) tests

Dev-dependency gap: ilium-plugin-host/Cargo.toml needs `sha2 = "0.10.9"` (hash checks in DEV-097, DEV-111, DEV-136).

- ilium-plugin-host `cli::CliEnv::{with_session(Arc<MemorySession>), with_probe(Arc<dyn ToolProbe>), with_builder(Arc<dyn BuildRunner>), with_runner(Arc<dyn ComponentRunner>), with_var(&str, &str), with_interrupt(Arc<AtomicBool>), with_dev_script(Vec<DevEvent>)}`; DEV-001..180. Why: the struct literal carries none of the injection points.
- ilium-plugin-host `cli::CliOutput.{stdout_raw: String, stdout_text: String}`; DEV-016, DEV-112, DEV-126. Why: the raw JSONL text and the human text are asserted separately.
- ilium-plugin-host `cli::{ToolProbe, BuildRunner, ComponentRunner, RunResult{submissions, outcomes, logs, trap}}` traits and struct with `ComponentRunner::run_project_tests(&self, &Path) -> i32`; DEV-040..089.
- ilium-plugin-host `cli::MemorySession::{new() -> Arc<MemorySession>, add_plugin(id, version, tier, state, dev: bool, grants: &[&str]), set_signed_by(id, key_id), plugin_state(id) -> Option<String>, set_client_attached(bool), review_dialog_count() -> usize, started_plugin_count() -> usize, granted(id) -> Vec<String>, is_high_risk(&str) -> bool, connect_attempts() -> usize, sessions_started() -> usize, journal() -> Vec<String>, add_log_line(..)}`; DEV-004..006, DEV-127, DEV-136..142.
- ilium-plugin-host `cli::project_input_hash(&Path) -> String`; DEV-095. Why: `write_dist` must write a build.json whose input hash matches the sources.
- ilium-plugin-host `package::ArchiveEntry{path, bytes}`, `Archive{manifest, entries, signature}` with `signature.key_id` and `Manifest.files[].{path, length, sha256}` public and `PartialEq`; DEV-097, DEV-117.
- ilium-plugin-host `cli::run` honouring `ILIUM_PLUGIN_SIGNING_KEY`, `ILIUM_TRUST_STORE`, `ILIUM_PLUGIN_UPDATE_GOLDEN` through `CliEnv::vars`; DEV-086, DEV-115, DEV-125.
- ilium-plugin-host `cli` error codes `lint.*` rules `lint.description-missing` and `lint.readme-missing`, and `lint.toml` suppression; DEV-075..078.
- ilium-plugin-host `cli::CliEnv.state_dir` trust store file `ilium/trust.json` with `format: "ilium-trust/1"`; DEV-125.
- ilium (bin) `ilium plugin` subcommands wired as in reference-plugin-cli.md, and `serde_json` + `tempfile` as dev-dependencies (already present); DEV-003..017 process tests.
- grants catalogue id `process.spawn` rated high risk (constant HIGH_RISK_GRANT in common.rs); DEV-141. Why: the real id comes from the grants page; adjust the constant.
- ilium-plugin-host `cli::conformance` run of the embedded suite (`conformance` command, DEV-103 and DEV-106): the `summary` record of the JSON output must carry a `suite` string field equal to the `suite` of the `--report` file of the same run (`cli::conformance::summary_suite_field`, DEV-103); and `--suite-version <MAJOR.MINOR>` must run that catalogue so that two MINOR versions can be compared case by case (`cli::conformance::suite_version_run`, DEV-106). Why: the CLI has no `conformance` subcommand or suite-version catalogue yet (no match in ilium-plugin-host/src/cli).
