//! reference-debugging-tools.md dev installs, and guide-ci-recipes.md
//! (DEV-710..713, 715, 716, 740..744).

use std::sync::{Arc, Mutex};

use ilium_plugin_api::Side;
use ilium_plugin_host::cli::{
    component_has_name_section, release_tag_matches, sha256_sidecar_line, source_map_path,
};
use ilium_plugin_host::package::write_package;
use ilium_plugin_host::telemetry::{DevBackend, DevDataRoots, DevLoop, RebuildOutcome, Refusal};

use crate::common::{env, error_code, records_of, run, scratch};

#[derive(Default)]
struct FakeDevBackend {
    release_installed: bool,
    outcomes: Mutex<Vec<RebuildOutcome>>,
    steps: Mutex<Vec<String>>,
    _data_context: Option<tempfile::TempDir>,
}

impl DevBackend for FakeDevBackend {
    fn release_installed(&self, _id: &str) -> bool {
        self.release_installed
    }
    fn replace_release(&self, _id: &str) -> Result<(), Refusal> {
        Ok(()) // Fixture installer transaction: no real release or running process exists.
    }
    fn build(&self) -> RebuildOutcome {
        self.outcomes.lock().expect("lock").remove(0)
    }
    fn deactivate(&self, reason: &str) {
        self.steps
            .lock()
            .expect("lock")
            .push(format!("deactivate:{reason}"));
    }
    fn load(&self, version: &str) {
        self.steps
            .lock()
            .expect("lock")
            .push(format!("load:{version}"));
    }
    fn activate(&self) {
        self.steps.lock().expect("lock").push("activate".to_owned());
    }
}

fn dev_loop(
    release_installed: bool,
    outcomes: Vec<RebuildOutcome>,
) -> (DevLoop, Arc<FakeDevBackend>) {
    let data_context = scratch();
    let roots = DevDataRoots {
        dev: data_context.path().join("dev"),
        release: data_context.path().join("release"),
    };
    let backend = Arc::new(FakeDevBackend {
        release_installed,
        outcomes: Mutex::new(outcomes),
        steps: Mutex::default(),
        _data_context: Some(data_context),
    });
    (DevLoop::with_data_roots(backend.clone(), roots), backend)
}

fn built(version: &str) -> RebuildOutcome {
    RebuildOutcome::Built {
        version: version.to_owned(),
    }
}

/// REQ: DEV-710
#[test]
fn dev_710_dev_install_carries_the_dev_badge() {
    let (mut dev, _) = dev_loop(false, vec![]);
    let install = dev.start("example.notes", false).expect("start");
    assert_eq!(install.badge, "[dev]");
}

/// REQ: DEV-710
#[test]
fn dev_710_badge_is_shown_in_the_plugin_list_and_in_settings() {
    let (mut dev, _) = dev_loop(false, vec![]);
    let install = dev.start("example.notes", false).expect("start");
    assert!(install.list_label.contains("[dev]") && install.settings_label.contains("[dev]"));
}

/// REQ: DEV-711
#[test]
fn dev_711_dev_install_has_its_own_data_folder() {
    let (mut dev, _) = dev_loop(false, vec![]);
    let install = dev.start("example.notes", false).expect("start");
    assert_ne!(install.data_dir, install.release_data_dir);
}

/// REQ: DEV-711
#[test]
fn dev_711_refuses_when_the_release_is_installed_dev_id_installed() {
    let (mut dev, _) = dev_loop(true, vec![]);
    let refusal = dev
        .start("example.notes", false)
        .expect_err("release installed");
    assert_eq!(refusal.code.as_str(), "dev.id-installed");
}

/// REQ: DEV-711
#[test]
fn dev_711_replace_installed_allows_it() {
    let (mut dev, _) = dev_loop(true, vec![]);
    assert!(dev.start("example.notes", true).is_ok());
}

/// REQ: DEV-712
#[test]
fn dev_712_rebuild_deactivates_with_upgrade_then_loads_then_activates() {
    let (mut dev, backend) = dev_loop(false, vec![built("0.1.1")]);
    dev.start("example.notes", false).expect("start");
    let report = dev.rebuild();
    assert!(report.reloaded);
    assert_eq!(
        *backend.steps.lock().expect("lock"),
        ["deactivate:upgrade", "load:0.1.1", "activate"]
    );
}

/// REQ: DEV-712
#[test]
fn dev_712_storage_is_kept_across_the_reload() {
    let (mut dev, _) = dev_loop(false, vec![built("0.1.1")]);
    dev.start("example.notes", false).expect("start");
    assert!(dev.rebuild().storage_kept);
}

/// REQ: DEV-713
#[test]
fn dev_713_failed_rebuild_keeps_the_running_version() {
    let failed = RebuildOutcome::Failed {
        errors: vec!["src/server.rs:12: expected `;`".to_owned()],
    };
    let (mut dev, backend) = dev_loop(false, vec![built("0.1.0"), failed]);
    dev.start("example.notes", false).expect("start");
    dev.rebuild();
    let report = dev.rebuild();
    assert!(!report.reloaded);
    assert_eq!(report.running_version, "0.1.0");
    assert_eq!(
        backend.steps.lock().expect("lock").len(),
        3,
        "only the first, successful rebuild reloaded"
    );
}

/// REQ: DEV-713
#[test]
fn dev_713_failed_rebuild_prints_the_build_errors() {
    let failed = RebuildOutcome::Failed {
        errors: vec!["src/server.rs:12: expected `;`".to_owned()],
    };
    let (mut dev, _) = dev_loop(false, vec![failed]);
    dev.start("example.notes", false).expect("start");
    let report = dev.rebuild();
    assert_eq!(report.errors, ["src/server.rs:12: expected `;`"]);
}

/// REQ: DEV-713
#[test]
fn dev_713_failed_rebuild_does_not_deactivate() {
    let failed = RebuildOutcome::Failed {
        errors: vec!["boom".to_owned()],
    };
    let (mut dev, backend) = dev_loop(false, vec![failed]);
    dev.start("example.notes", false).expect("start");
    dev.rebuild();
    assert!(backend.steps.lock().expect("lock").is_empty());
}

/// REQ: DEV-715
#[test]
fn dev_715_source_map_lives_at_dist_side_wasm_map() {
    assert_eq!(
        source_map_path(Side::Server).to_str(),
        Some("dist/server.wasm.map")
    );
    assert_eq!(
        source_map_path(Side::Client).to_str(),
        Some("dist/client.wasm.map")
    );
}

/// REQ: DEV-716
#[test]
fn dev_716_name_section_is_detected() {
    let mut with_name = b"\0asm\x0d\0\x01\0".to_vec();
    with_name.extend_from_slice(&[0x00, 0x05, 0x04, b'n', b'a', b'm', b'e']);
    assert!(component_has_name_section(&with_name));
}

/// REQ: DEV-716
#[test]
fn dev_716_stripped_component_has_no_name_section() {
    assert!(!component_has_name_section(b"\0asm\x0d\0\x01\0"));
}

/// REQ: DEV-740
#[test]
fn dev_740_conformance_flag_is_accepted_by_test() {
    let scratch = scratch();
    let output = run(&env(scratch.path()), &["test", "--conformance"]);
    // manual does not pin the exit code of a conformance run in an empty directory: the flag
    // must be known (not a usage error) and the run must end with a result record
    assert_ne!(output.exit_code, 2, "--conformance is a known flag");
    assert!(!error_code(&output)
        .unwrap_or_default()
        .starts_with("usage."));
    let result = records_of(&output, "result");
    assert_eq!(result.len(), 1, "{:?}", output.stdout_jsonl);
    assert_eq!(result[0]["exit_code"], output.exit_code);
}

/// REQ: DEV-741
#[test]
fn dev_741_tag_must_equal_v_plus_version() {
    assert!(release_tag_matches("v1.2.3", "1.2.3"));
    assert!(!release_tag_matches("1.2.3", "1.2.3"));
    assert!(!release_tag_matches("v1.2.4", "1.2.3"));
}

/// REQ: DEV-742
#[test]
fn dev_742_sha256_sidecar_names_the_package() {
    let line = sha256_sidecar_line(b"abc", "example.notes-0.1.0.ilp");
    assert_eq!(
        line,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  example.notes-0.1.0.ilp"
    );
}

/// REQ: DEV-743
#[test]
fn dev_743_every_error_record_has_code_and_message() {
    let scratch = scratch();
    let output = run(
        &env(scratch.path()),
        &[
            "inspect",
            scratch.path().join("missing.ilp").to_str().expect("utf8"),
            "--json",
        ],
    );
    let errors = records_of(&output, "error");
    assert!(!errors.is_empty());
    for error in errors {
        assert!(
            error["code"].is_string() && error["message"].is_string(),
            "{error}"
        );
    }
}

/// REQ: DEV-744
#[test]
fn dev_744_pack_without_a_built_side_fails_pack_not_built() {
    let scratch = scratch();
    let env = env(scratch.path());
    let project = env.cwd.clone();
    std::fs::write(
        project.join("manifest.json"),
        r#"{"id":"example.notes","version":"0.1.0","sides":["server"]}"#,
    )
    .expect("manifest");
    let output = run(&env, &["pack", "--json"]);
    assert_eq!(error_code(&output).as_deref(), Some("pack.not-built"));
    let _ = write_package;
}

/// REQ: DEV-742
#[test]
fn dev_742_sha256_sidecar_of_empty_input_is_the_known_empty_digest() {
    let line = sha256_sidecar_line(b"", "empty.ilp");
    assert_eq!(
        line,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  empty.ilp"
    );
}

/// REQ: DEV-743
#[test]
fn dev_743_every_error_record_code_is_a_dotted_catalogue_style_code() {
    let scratch = scratch();
    let output = run(
        &env(scratch.path()),
        &[
            "inspect",
            scratch.path().join("missing.ilp").to_str().expect("utf8"),
            "--json",
        ],
    );
    let errors = records_of(&output, "error");
    assert!(!errors.is_empty());
    for error in errors {
        assert!(
            error["code"]
                .as_str()
                .is_some_and(|code| code.contains('.')),
            "CI logs need a namespaced code, got {error}"
        );
    }
}

/// REQ: DEV-744
#[test]
fn dev_744_pack_with_no_declared_side_does_not_report_pack_not_built() {
    let scratch = scratch();
    let env = env(scratch.path());
    let project = env.cwd.clone();
    std::fs::write(
        project.join("manifest.json"),
        r#"{"id":"example.notes","version":"0.1.0","sides":[]}"#,
    )
    .expect("manifest");
    let output = run(&env, &["pack", "--json"]);
    assert_ne!(
        error_code(&output).as_deref(),
        Some("pack.not-built"),
        "pack.not-built is only for a declared side without a component"
    );
}
