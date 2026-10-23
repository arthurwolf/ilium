//! reference-check-and-lint-rules.md "What check does" and "Component rules"
//! (DEV-200..206, DEV-227). Channels: CLI JSONL records and exit code.

use ilium_plugin_api::manifest::CheckMode;
use ilium_plugin_host::cli::{CliEnv, CliOutput};
use ilium_plugin_host::install::Installer;
use ilium_plugin_host::registry::PackageStore;
use serde_json::{json, Value};

use crate::common::*;
use crate::lint_support::*;

const MIB: usize = 1024 * 1024;

fn setup(edit: impl FnOnce(&mut Value)) -> (tempfile::TempDir, CliEnv) {
    let root = scratch();
    let env = env(root.path());
    project_with(&project_dir(&env), edit);
    (root, env)
}

fn check(env: &CliEnv) -> CliOutput {
    run_in_project(env, "check", &[])
}

fn add_client(manifest: &mut Value) {
    manifest["client"] =
        json!({ "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": ["views"] });
    manifest["capabilities"]["client"] = json!([]);
}

fn with_good_component(edit: impl FnOnce(&mut Value)) -> (tempfile::TempDir, CliEnv) {
    let (root, env) = setup(edit);
    write_dist(&project_dir(&env), "server", &good_server_component());
    (root, env)
}

/// REQ: DEV-200
#[test]
fn dev_200_check_reports_a_manifest_code_for_a_bad_version_and_exits_3() {
    let (_root, env) = setup(|manifest| manifest["version"] = json!("1.x"));
    let output = check(&env);
    let codes = rules_of(&output, "check");
    assert!(
        codes.iter().any(|code| code.starts_with("manifest.")),
        "{codes:?}"
    );
    assert_eq!(output.exit_code, 3);
}

/// REQ: DEV-200
#[test]
fn dev_200_check_never_reports_a_package_code() {
    let (_root, env) = setup(|manifest| {
        manifest["version"] = json!("1.x");
        manifest["id"] = json!("Bad_Id");
    });
    let output = check(&env);
    assert!(rules_of(&output, "check")
        .iter()
        .all(|code| !code.starts_with("package.")));
}

/// REQ: DEV-200
#[test]
fn dev_200_check_repeats_the_manifest_codes_of_the_install_path_in_the_same_order() {
    let (_root, env) = setup(|manifest| {
        manifest["version"] = json!("1.x");
        manifest["id"] = json!("Bad_Id");
    });
    let manifest_bytes = std::fs::read(project_dir(&env).join("manifest.json")).expect("manifest");
    let package =
        ilium_plugin_host::package::write_package(&[("manifest.json".to_owned(), manifest_bytes)])
            .expect("package");
    let store = tempfile::tempdir().expect("store");
    let installer = Installer::new(PackageStore {
        root: store.path().to_path_buf(),
    });
    let install_codes: Vec<String> = installer
        .check(&package, CheckMode::Install)
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.0.to_string())
        .filter(|code| code.starts_with("manifest."))
        .collect();
    let check_codes: Vec<String> = rules_of(&check(&env), "check")
        .into_iter()
        .filter(|code| code.starts_with("manifest."))
        .collect();
    let common_in_check: Vec<&String> = check_codes
        .iter()
        .filter(|code| install_codes.contains(code))
        .collect();
    let common_in_install: Vec<&String> = install_codes
        .iter()
        .filter(|code| check_codes.contains(code))
        .collect();
    assert!(
        !common_in_check.is_empty(),
        "check {check_codes:?} install {install_codes:?}"
    );
    assert_eq!(common_in_check, common_in_install);
}

/// REQ: DEV-200
#[test]
fn dev_200_check_does_not_run_plugin_code_even_when_the_component_traps_at_start() {
    let (_root, env) = setup(|_| {});
    let trapping = wat_component_trapping_at_start();
    write_dist(&project_dir(&env), "server", &trapping);
    let output = check(&env);
    assert!(!rules_of(&output, "check")
        .iter()
        .any(|rule| rule.contains("trap")));
    assert!(output
        .stdout_jsonl
        .iter()
        .any(|record| record["type"] == "result"));
}

fn wat_component_trapping_at_start() -> Vec<u8> {
    let bytes = ilium_plugin_host::testing::wat_component(
        "(component (core module $m (func $t unreachable) (start $t)) (core instance (instantiate $m)))",
    );
    let bytes = with_custom_section(bytes, "ilium:api-version", &api_payload([0, 1, 0]));
    with_custom_section(bytes, WORLD_SECTION, b"ilium:server/base@0.1.0")
}

/// REQ: DEV-201
#[test]
fn dev_201_check_reports_one_component_missing_per_absent_side() {
    let (_root, env) = setup(add_client);
    let output = check(&env);
    let missing: Vec<&Value> = records_of(&output, "check")
        .into_iter()
        .filter(|record| record["rule"] == "component.missing")
        .collect();
    assert_eq!(missing.len(), 2);
    let files: Vec<&str> = missing
        .iter()
        .filter_map(|record| record["file"].as_str())
        .collect();
    assert!(
        files.contains(&"dist/server.wasm") && files.contains(&"dist/client.wasm"),
        "{files:?}"
    );
}

/// REQ: DEV-201
#[test]
fn dev_201_check_reports_component_missing_for_only_the_side_without_a_component() {
    let (_root, env) = with_good_component(add_client);
    let output = check(&env);
    let missing: Vec<&Value> = records_of(&output, "check")
        .into_iter()
        .filter(|record| record["rule"] == "component.missing")
        .collect();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0]["file"], "dist/client.wasm");
}

/// REQ: DEV-201
#[test]
fn dev_201_check_skips_component_rules_for_an_absent_side() {
    let (_root, env) = setup(|_| {});
    let output = check(&env);
    let component_rules: Vec<String> = rules_of(&output, "check")
        .into_iter()
        .filter(|rule| rule.starts_with("component.") && rule != "component.missing")
        .collect();
    assert!(component_rules.is_empty(), "{component_rules:?}");
}

/// REQ: DEV-201
#[test]
fn dev_201_check_reports_component_invalid_for_bytes_that_are_not_a_component() {
    let (_root, env) = setup(|_| {});
    write_dist(&project_dir(&env), "server", b"not a component");
    let output = check(&env);
    assert!(has_rule(&output, "check", "component.invalid"));
    assert_eq!(output.exit_code, 3);
}

/// REQ: DEV-201
#[test]
fn dev_201_check_reports_manifest_api_version_when_the_section_is_absent() {
    let (_root, env) = setup(|_| {});
    let bytes = with_custom_section(
        ilium_plugin_host::testing::wat_component(&server_component_text(&[])),
        WORLD_SECTION,
        b"ilium:server/base@0.1.0",
    );
    write_dist(&project_dir(&env), "server", &bytes);
    assert!(has_rule(&check(&env), "check", "manifest.api-version"));
}

/// REQ: DEV-201
#[test]
fn dev_201_check_reports_manifest_exports_mismatch_when_an_optional_export_is_unlisted() {
    let (_root, env) = setup(|manifest| manifest["server"]["exports"] = json!(["detector"]));
    write_dist(&project_dir(&env), "server", &good_server_component());
    assert!(has_rule(&check(&env), "check", "manifest.exports-mismatch"));
}

/// REQ: DEV-201
#[test]
fn dev_201_check_reports_component_import_unknown_for_an_interface_outside_the_contract() {
    let (_root, env) = setup(|_| {});
    let bytes = server_component(
        &[("ilium:bogus/thing@0.1.0", "poke")],
        "ilium:server/base@0.1.0",
        [0, 1, 0],
    );
    write_dist(&project_dir(&env), "server", &bytes);
    assert!(has_rule(&check(&env), "check", "component.import-unknown"));
}

/// REQ: DEV-202
#[test]
fn dev_202_every_finding_is_an_error_except_the_four_named_rules() {
    let (_root, env) = setup(add_client);
    let output = check(&env);
    for record in records_of(&output, "check") {
        let rule = record["rule"].as_str().expect("rule");
        let expected = match rule {
            "signature.absent" | "component.size-warning" | "check.stale-build" => "warn",
            "signature.fingerprint" => "info",
            _ => "error",
        };
        assert_eq!(record["severity"], expected, "rule {rule}");
    }
}

/// REQ: DEV-202
#[test]
fn dev_202_signature_absent_is_a_warning_for_an_unsigned_package() {
    let (root, env) = setup(|_| {});
    let path = write_package_file(
        root.path(),
        "unsigned.iliumpkg",
        &unsigned_package(&good_server_component()),
    );
    assert_eq!(
        severity_of(&check_file(&env, &path), "check", "signature.absent").as_deref(),
        Some("warn")
    );
}

/// REQ: DEV-202
#[test]
fn dev_202_signature_fingerprint_is_info_for_a_signed_package() {
    let (root, env) = setup(|_| {});
    let path = write_package_file(
        root.path(),
        "signed.iliumpkg",
        &signed_package(&good_server_component()),
    );
    let output = check_file(&env, &path);
    assert_eq!(
        severity_of(&output, "check", "signature.fingerprint").as_deref(),
        Some("info")
    );
    assert!(!has_rule(&output, "check", "signature.absent"));
}

/// REQ: DEV-202
#[test]
fn dev_202_stale_build_is_a_warning() {
    let (_root, env) = with_good_component(|_| {});
    write_text(
        &project_dir(&env),
        "dist/build.json",
        r#"{"profile":"release","api":"ilium:plugin@0.1.0","input_hash":"0000000000000000000000000000000000000000000000000000000000000000","sides":[],"toolchain":{},"cached":false}"#,
    );
    assert_eq!(
        severity_of(&check(&env), "check", "check.stale-build").as_deref(),
        Some("warn")
    );
}

/// REQ: DEV-203
#[test]
fn dev_203_check_states_the_fingerprint_in_an_info_finding_for_a_valid_signature() {
    let (root, env) = setup(|_| {});
    let path = write_package_file(
        root.path(),
        "signed.iliumpkg",
        &signed_package(&good_server_component()),
    );
    let output = check_file(&env, &path);
    let record = finding(&output, "check", "signature.fingerprint").expect("fingerprint finding");
    let message = record["message"].as_str().expect("message");
    assert!(
        message.chars().filter(|c| c.is_ascii_hexdigit()).count() >= 16,
        "{message}"
    );
}

/// REQ: DEV-203
#[test]
fn dev_203_check_does_not_check_trust_and_exits_0_for_an_untrusted_key() {
    let (root, env) = setup(|_| {});
    let path = write_package_file(
        root.path(),
        "signed.iliumpkg",
        &signed_package(&good_server_component()),
    );
    let output = check_file(&env, &path);
    assert!(rules_of(&output, "check")
        .iter()
        .all(|rule| !rule.starts_with("trust.")));
    assert_ne!(output.exit_code, 3, "{:?}", rules_of(&output, "check"));
}

/// REQ: DEV-203
#[test]
fn dev_203_a_tampered_manifest_does_not_get_a_fingerprint_finding() {
    let (root, env) = setup(|_| {});
    let mut bytes = signed_package(&good_server_component());
    let at = bytes
        .windows(10)
        .position(|window| window == b"Agent tool")
        .unwrap_or(bytes.len() / 3);
    bytes[at] ^= 0x01;
    let path = write_package_file(root.path(), "tampered.iliumpkg", &bytes);
    assert!(!has_rule(
        &check_file(&env, &path),
        "check",
        "signature.fingerprint"
    ));
}

fn ungranted_output(imports: &[(&str, &str)], server_grants: Value) -> CliOutput {
    let (_root, env) = setup(|manifest| manifest["capabilities"]["server"] = server_grants);
    write_dist(
        &project_dir(&env),
        "server",
        &server_component(imports, "ilium:server/base@0.1.0", [0, 1, 0]),
    );
    check(&env)
}

/// REQ: DEV-204
#[test]
fn dev_204_import_whose_grant_is_missing_fires_with_file_and_key_path() {
    let output = ungranted_output(&[("ilium:host/fs@0.1.0", "read-all")], json!([]));
    let record = finding(&output, "check", "component.import-ungranted").expect("finding");
    assert_eq!(record["severity"], "error");
    assert_eq!(record["file"], "dist/server.wasm");
    assert_eq!(record["key_path"], "/capabilities/server");
    assert!(record["message"]
        .as_str()
        .expect("message")
        .contains("read-all"));
    assert_eq!(output.exit_code, 3);
}

/// REQ: DEV-204
#[test]
fn dev_204_import_with_its_grant_does_not_fire() {
    let output = ungranted_output(
        &[("ilium:host/fs@0.1.0", "read-all")],
        json!(["fs.read:project"]),
    );
    assert!(!has_rule(&output, "check", "component.import-ungranted"));
}

/// REQ: DEV-204
#[test]
fn dev_204_always_granted_interface_does_not_fire() {
    let output = ungranted_output(&[("ilium:host/codec@0.1.0", "deflate")], json!([]));
    assert!(!has_rule(&output, "check", "component.import-ungranted"));
}

/// REQ: DEV-204
#[test]
fn dev_204_fires_once_per_ungranted_function_and_not_for_the_granted_one() {
    let output = ungranted_output(
        &[
            ("ilium:host/fs@0.1.0", "read-all"),
            ("ilium:host/fs@0.1.0", "write"),
            ("ilium:host/fs@0.1.0", "remove"),
        ],
        json!(["fs.read:project"]),
    );
    let messages: Vec<String> = records_of(&output, "check")
        .iter()
        .filter(|record| record["rule"] == "component.import-ungranted")
        .map(|record| record["message"].as_str().unwrap_or("").to_owned())
        .collect();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages.iter().any(|message| message.contains("write")));
    assert!(messages.iter().any(|message| message.contains("remove")));
    assert!(!messages.iter().any(|message| message.contains("read-all")));
}

/// REQ: DEV-205
#[test]
fn dev_205_world_mismatch_is_read_from_the_embedded_world_not_the_file_name() {
    let (_root, env) = setup(|_| {});
    // The file is called server.wasm but its embedded world is the client world.
    write_dist(
        &project_dir(&env),
        "server",
        &server_component(&[], "ilium:client/base@0.1.0", [0, 1, 0]),
    );
    assert!(has_rule(&check(&env), "check", "component.world-mismatch"));
}

/// REQ: DEV-205
#[test]
fn dev_205_matching_embedded_world_passes_even_for_an_odd_file_name_entry() {
    let (_root, env) = setup(|manifest| manifest["server"]["entry"] = json!("client-looking.wasm"));
    write_bytes(
        &project_dir(&env),
        "dist/client-looking.wasm",
        &good_server_component(),
    );
    assert!(!has_rule(&check(&env), "check", "component.world-mismatch"));
}

/// REQ: DEV-205
#[test]
fn dev_205_api_version_is_read_from_the_embedded_section() {
    let (_root, env) = setup(|_| {});
    write_dist(
        &project_dir(&env),
        "server",
        &server_component(&[], "ilium:server/base@0.1.0", [9, 9, 9]),
    );
    assert!(has_rule(&check(&env), "check", "manifest.api-version"));
}

/// REQ: DEV-205
#[test]
fn dev_205_matching_api_version_section_passes() {
    let (_root, env) = with_good_component(|_| {});
    assert!(!has_rule(&check(&env), "check", "manifest.api-version"));
}

fn size_output(size: usize) -> CliOutput {
    let (_root, env) = setup(|_| {});
    write_dist(
        &project_dir(&env),
        "server",
        &server_component_of_size(size),
    );
    check(&env)
}

/// REQ: DEV-206
#[test]
fn dev_206_component_of_exactly_8_mib_does_not_warn() {
    assert!(!has_rule(
        &size_output(8 * MIB),
        "check",
        "component.size-warning"
    ));
}

/// REQ: DEV-206
#[test]
fn dev_206_component_of_8_mib_plus_one_byte_warns() {
    let output = size_output(8 * MIB + 1);
    assert_eq!(
        severity_of(&output, "check", "component.size-warning").as_deref(),
        Some("warn")
    );
}

/// REQ: DEV-206
#[test]
fn dev_206_component_of_exactly_32_mib_is_still_a_warning() {
    let output = size_output(32 * MIB);
    assert_eq!(
        severity_of(&output, "check", "component.size-warning").as_deref(),
        Some("warn")
    );
}

/// REQ: DEV-206
#[test]
fn dev_206_component_above_32_mib_is_an_error_with_the_same_code() {
    let output = size_output(32 * MIB + 1);
    assert_eq!(
        severity_of(&output, "check", "component.size-warning").as_deref(),
        Some("error")
    );
    assert_eq!(output.exit_code, 3);
}

/// REQ: DEV-227
#[test]
fn dev_227_check_exits_3_when_an_error_finding_exists() {
    let (_root, env) = setup(|_| {});
    assert_eq!(check(&env).exit_code, 3);
}

/// REQ: DEV-227
#[test]
fn dev_227_check_exits_0_when_only_warnings_exist() {
    let (_root, env) = with_good_component(|_| {});
    write_text(
        &project_dir(&env),
        "dist/build.json",
        r#"{"profile":"release","api":"ilium:plugin@0.1.0","input_hash":"0000000000000000000000000000000000000000000000000000000000000000","sides":[],"toolchain":{},"cached":false}"#,
    );
    let output = check(&env);
    assert!(has_rule(&output, "check", "check.stale-build"));
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-227
#[test]
fn dev_227_check_has_no_deny_flag() {
    let (_root, env) = with_good_component(|_| {});
    let output = run_in_project(&env, "check", &["--deny", "warnings"]);
    assert_eq!(error_code(&output).as_deref(), Some("usage.unknown-flag"));
    assert_eq!(output.exit_code, 2);
}
