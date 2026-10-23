//! Install size checks of the reference page `reference-install-and-update` (USR-250, USR-251).

use crate::common::*;

/// REQ: USR-250
#[test]
fn usr_250_understated_header_size_is_refused_by_counted_bytes() {
    let rig = rig();
    let spec = pkg(NOTES, "1.0.0").padding(256 * 1024).declared_size(1024);
    let output = install(&rig, &spec);
    assert_ne!(
        output.exit_code, 0,
        "a package that lies about its size must not install"
    );
    assert!(
        output.stderr.contains("package.too-large"),
        "expected package.too-large, got: {}",
        output.stderr
    );
}

/// REQ: USR-251
#[test]
fn usr_251_too_large_text_names_plugin_size_limit_and_source_in_cli_and_dialog() {
    let rig = rig();
    let spec = pkg(NOTES, "1.0.0").padding(256 * 1024);
    let output = install(&rig, &spec);
    let measured = spec.actual_size();
    let dialog_text = rig.last_review().join("\n");
    for text in [output.stderr.as_str(), dialog_text.as_str()] {
        assert!(
            text.contains("package.too-large"),
            "error code missing: {text}"
        );
        assert!(text.contains(NOTES), "plugin id missing: {text}");
        assert!(
            text.contains(&measured.to_string()),
            "measured size {measured} missing: {text}"
        );
        assert!(text.contains("limit"), "limit missing: {text}");
        assert!(
            text.contains("host") || text.contains("policy"),
            "source of the limit missing: {text}"
        );
    }
}
