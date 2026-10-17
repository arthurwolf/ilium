//! reference-plugin-distribution-cli.md PKG-446: secret variables never appear in output or
//! state. Lane `registry-publish`, part 2.

use crate::common::*;
use crate::pages_p2::*;

const PAGE: &str = "reference-plugin-distribution-cli.md";
const NOTES: &str = "com.acme.notes";
const TOKEN: &str = "tok-secret-4471-ZZ";
const PASSPHRASE: &str = "pass-secret-9902-QQ";

fn assert_absent(sandbox: &Sandbox, output: &ilium_plugin_host::cli::CliOutput, secret: &str) {
    assert!(
        !output_contains(output, secret),
        "secret in records: {output:?}"
    );
    for (path, bytes) in sandbox.snapshot() {
        assert!(
            !String::from_utf8_lossy(&bytes).contains(secret),
            "secret written to {path}"
        );
    }
}

/// REQ: PKG-446
#[test]
fn doc_pkg_446_a_secret_variable_never_appears_in_a_record_log_or_error() {
    assert_row(
        PAGE,
        "PKG-446",
        &[
            "secret variable MUST NOT appear",
            "record, log or error message",
        ],
    );
}

/// REQ: PKG-446
#[test]
fn pkg_446_the_registry_token_is_absent_after_a_successful_publish_attempt() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file("upload.iliumpkg", &signed_package(NOTES, "1.2.0"));
    let output = sandbox.run_with_vars(
        &[("ILIUM_REGISTRY_TOKEN", TOKEN)],
        &[
            "publish",
            path.to_str().expect("utf-8 path"),
            "--registry",
            BASE,
        ],
    );
    assert_absent(&sandbox, &output, TOKEN);
}

/// REQ: PKG-446
#[test]
fn pkg_446_the_registry_token_is_absent_when_the_publish_fails() {
    let sandbox = Sandbox::standard();
    let path = sandbox.project_file("upload.iliumpkg", &signed_package(NOTES, "1.0.0"));
    let output = sandbox.run_with_vars(
        &[("ILIUM_REGISTRY_TOKEN", TOKEN)],
        &[
            "publish",
            path.to_str().expect("utf-8 path"),
            "--registry",
            BASE,
        ],
    );
    assert!(
        !error_codes(&output).is_empty(),
        "version exists is an error: {output:?}"
    );
    assert_absent(&sandbox, &output, TOKEN);
}

/// REQ: PKG-446
#[test]
fn pkg_446_the_registry_token_is_absent_when_the_registry_is_down() {
    let sandbox = Sandbox::standard();
    sandbox.net.set_down(true);
    let path = sandbox.project_file("upload.iliumpkg", &signed_package(NOTES, "1.2.0"));
    let output = sandbox.run_with_vars(
        &[("ILIUM_REGISTRY_TOKEN", TOKEN)],
        &[
            "publish",
            path.to_str().expect("utf-8 path"),
            "--registry",
            BASE,
        ],
    );
    assert_absent(&sandbox, &output, TOKEN);
}

/// REQ: PKG-446
#[test]
fn pkg_446_the_key_passphrase_is_absent_from_a_failed_sign() {
    let sandbox = Sandbox::new();
    let path = sandbox.project_file("plugin.iliumpkg", &unsigned_package(NOTES, "1.0.0"));
    let output = sandbox.run_with_vars(
        &[
            ("ILIUM_PUBLISHER_KEY", "garbage-key"),
            ("ILIUM_KEY_PASSPHRASE", PASSPHRASE),
        ],
        &["sign", path.to_str().expect("utf-8 path")],
    );
    assert_absent(&sandbox, &output, PASSPHRASE);
    assert_absent(&sandbox, &output, "garbage-key");
}

/// REQ: PKG-446
#[test]
fn pkg_446_the_key_passphrase_is_absent_from_a_usage_error() {
    let sandbox = Sandbox::new();
    let output = sandbox.run_with_vars(&[("ILIUM_KEY_PASSPHRASE", PASSPHRASE)], &["sign"]);
    assert_eq!(output.exit_code, 2, "{output:?}");
    assert_absent(&sandbox, &output, PASSPHRASE);
}

/// REQ: PKG-446
#[test]
fn pkg_446_the_trust_store_path_variable_is_not_a_secret_but_a_secret_next_to_it_stays_hidden() {
    let sandbox = Sandbox::new();
    let output = sandbox.run_with_vars(
        &[
            ("ILIUM_REGISTRY_TOKEN", TOKEN),
            ("ILIUM_TRUST_STORE", "/nonexistent/trust.json"),
        ],
        &["trust", "list"],
    );
    assert_absent(&sandbox, &output, TOKEN);
}
