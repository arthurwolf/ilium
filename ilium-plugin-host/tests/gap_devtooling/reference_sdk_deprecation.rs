//! SDK deprecation policy: deprecated SDK items warn with their replacement and stay available
//! until their removal release (DEV-996, DEV-997). Page: reference-sdk-deprecation.

use crate::common::*;
use crate::{
    add_deprecated_sdk_call, current_sdk_version, manual_page, new_rust_project,
    run_plugin_calling_deprecated_item, sdk_removal_release, with_code,
};

/// REQ: DEV-996
#[test]
fn deprecated_sdk_item_warns_with_its_replacement_and_names_the_removal_release() {
    let page = manual_page("reference-sdk-deprecation");
    assert!(
        page.to_lowercase().contains("replacement"),
        "the page names replacements"
    );
    let (_root, env, dir) = new_rust_project();
    add_deprecated_sdk_call(&dir, "old_settings_read", "settings_get");
    let out = run_in_project(&env, "check", &[]);
    let warnings = with_code(&out, "sdk.deprecated");
    assert!(
        !warnings.is_empty(),
        "a deprecated SDK item prints sdk.deprecated: {:?}",
        out.stdout_jsonl
    );
    let message = warnings[0].to_string();
    assert!(
        message.contains("settings_get"),
        "the warning names the replacement: {message}"
    );
    assert!(
        message.contains("removed in"),
        "the warning names the removal release: {message}"
    );
}

/// REQ: DEV-997
#[test]
fn deprecated_sdk_item_still_compiles_and_runs_until_its_announced_removal_release() {
    let (_root, env, dir) = new_rust_project();
    add_deprecated_sdk_call(&dir, "old_settings_read", "settings_get");
    let built = run_in_project(&env, "build", &[]);
    assert_eq!(
        built.exit_code, 0,
        "a deprecated item still builds: {:?}",
        built.stdout_jsonl
    );
    let removal = sdk_removal_release("old_settings_read");
    assert!(
        removal.as_str() > current_sdk_version(&env).as_str(),
        "the item is still present in the current release; removal is at {removal}"
    );
    let run = run_plugin_calling_deprecated_item(&env);
    assert_eq!(
        run.exit_code, 0,
        "the deprecated item still returns a value: {:?}",
        run.stdout_jsonl
    );
}
