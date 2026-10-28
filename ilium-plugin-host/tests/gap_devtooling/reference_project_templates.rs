//! Project templates: `.gitignore`, `README.md`, licence files and template drift (DEV-935,
//! DEV-937). Page: reference-project-templates.

use serde_json::Value;

use crate::common::*;
use crate::{cli_with_changed_template_files, new_rust_project, snapshot, utf8, with_code};

/// REQ: DEV-935
#[test]
fn new_writes_gitignore_and_readme_naming_the_build_test_dev_and_pack_commands() {
    let (_root, _env, dir) = new_rust_project();
    let ignore = read_text(&dir, ".gitignore");
    for entry in [
        "dist/",
        "target/",
        "node_modules/",
        "publisher.key",
        "*.tmp",
        ".ilium/",
    ] {
        assert!(
            ignore.lines().any(|line| line.trim() == entry),
            ".gitignore lists {entry}: {ignore}"
        );
    }
    let readme = read_text(&dir, "README.md");
    for command in ["build", "test", "dev", "pack"] {
        assert!(
            readme.contains(command),
            "README.md names the {command} command"
        );
    }
}

/// REQ: DEV-935
#[test]
fn new_with_license_writes_license_file_and_manifest_field_and_unknown_id_writes_nothing() {
    let root = scratch();
    let cli_env = env(root.path());
    let licensed = root.path().join("licensed");
    let made = run(
        &cli_env,
        &[
            "new",
            utf8(&licensed),
            "--id",
            "example.agent-tools",
            "--lang",
            "rust",
            "--template",
            "server",
            "--license",
            "MIT",
            "--json",
        ],
    );
    assert_eq!(made.exit_code, 0, "{:?}", made.stdout_jsonl);
    assert!(
        licensed.join("LICENSE").is_file(),
        "a LICENSE file is written"
    );
    let manifest: Value =
        serde_json::from_str(&read_text(&licensed, "manifest.json")).expect("manifest JSON");
    assert_eq!(
        manifest["license"], "MIT",
        "the manifest license field is set"
    );

    let refused_dir = root.path().join("refused");
    let refused = run(
        &cli_env,
        &[
            "new",
            utf8(&refused_dir),
            "--id",
            "example.agent-tools",
            "--lang",
            "rust",
            "--template",
            "server",
            "--license",
            "NOT-A-REAL-LICENSE-ID",
            "--json",
        ],
    );
    assert_eq!(
        error_code(&refused).as_deref(),
        Some("new.license-unknown"),
        "{:?}",
        refused.stdout_jsonl
    );
    assert!(
        !refused_dir.exists(),
        "no file is written before the refusal"
    );
}

/// REQ: DEV-937
#[test]
fn template_drift_warns_once_per_changed_file_without_modifying_the_project() {
    let (_root, env, dir) = new_rust_project();
    let drifted_env = cli_with_changed_template_files(&env, &["src/lib.rs", "README.md"]);
    let before = snapshot(&dir);
    let out = run_in_project(&drifted_env, "check", &["--template-drift"]);
    let drift = with_code(&out, "template.drift");
    assert_eq!(
        drift.len(),
        2,
        "one template.drift warning per changed file: {:?}",
        out.stdout_jsonl
    );
    for record in &drift {
        assert_eq!(record["severity"], "warn", "{record}");
    }
    assert_eq!(snapshot(&dir), before, "the drift check modifies nothing");
}

/// REQ: DEV-937
#[test]
fn template_drift_reports_nothing_when_no_template_file_changed() {
    let (_root, env, _dir) = new_rust_project();
    let out = run_in_project(&env, "check", &["--template-drift"]);
    assert!(
        with_code(&out, "template.drift").is_empty(),
        "no template file changed, so no drift warning: {:?}",
        out.stdout_jsonl
    );
}
