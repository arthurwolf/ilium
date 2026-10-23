//! Check and lint helpers used only by the devtools crate's check, lint and template tests.
//! They stay out of the shared `common` module, which the gap-devtooling crate also includes.

use crate::common::*;
use ilium_plugin_host::cli::{CliEnv, CliOutput};
use serde_json::Value;
use std::path::Path;

/// The `rule` of every record of `record_type` (`check` or `lint`) in output order.
pub fn rules_of(output: &CliOutput, record_type: &str) -> Vec<String> {
    records_of(output, record_type)
        .iter()
        .filter_map(|record| {
            record
                .get("rule")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

/// The first record of `record_type` whose `rule` equals `rule`.
pub fn finding<'a>(output: &'a CliOutput, record_type: &str, rule: &str) -> Option<&'a Value> {
    records_of(output, record_type)
        .into_iter()
        .find(|record| record.get("rule").and_then(Value::as_str) == Some(rule))
}

/// True when a record of `record_type` carries `rule`.
pub fn has_rule(output: &CliOutput, record_type: &str, rule: &str) -> bool {
    finding(output, record_type, rule).is_some()
}

/// The `severity` of the first finding with `rule`.
pub fn severity_of(output: &CliOutput, record_type: &str, rule: &str) -> Option<String> {
    finding(output, record_type, rule)
        .and_then(|record| record.get("severity"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Runs `check <file> --json`.
pub fn check_file(env: &CliEnv, file: &str) -> CliOutput {
    run(env, &["check", file, "--json"])
}

/// Loads a built guest fixture component `plugins/fixtures/<name>/dist/<side>.wasm`. A fixture that
/// the toolchain has not built yet fails the test with `fixture.missing` (red until it is built).
pub fn fixture_component(name: &str, side: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("plugins")
        .join("fixtures")
        .join(name)
        .join("dist")
        .join(format!("{side}.wasm"));
    std::fs::read(&path).unwrap_or_else(|_| panic!("fixture.missing: {}", path.display()))
}

/// Writes a sparse file of exactly `size` bytes.
pub fn write_sparse(dir: &Path, relative: &str, size: u64) {
    let path = dir.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create folders");
    let file = std::fs::File::create(path).expect("create sparse file");
    file.set_len(size).expect("set length");
}

/// `Some(true)` when the lint record with `rule` is marked suppressed.
pub fn is_suppressed(output: &CliOutput, rule: &str) -> Option<bool> {
    finding(output, "lint", rule).and_then(|record| record["suppressed"].as_bool())
}
