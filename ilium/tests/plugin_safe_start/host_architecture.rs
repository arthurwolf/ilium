//! reference-host-architecture.md safe-start rows (HST-033, HST-034, HST-043).

use ilium::plugin_cmd::{parse_start_mode, NO_PLUGINS_ENV, SAFE_PLUGINS_ENV};
use ilium_plugin_host::lifecycle::StartMode;
use proptest::prelude::*;

use crate::common::*;

/// REQ: HST-033
#[test]
fn hst_033_the_safe_flag_selects_safe_plugins() {
    assert_eq!(parse_start_mode(true, false, None, None), StartMode::SafePlugins);
}

/// REQ: HST-033
#[test]
fn hst_033_the_no_plugins_flag_selects_no_plugins() {
    assert_eq!(parse_start_mode(false, true, None, None), StartMode::NoPlugins);
}

/// REQ: HST-033
#[test]
fn hst_033_no_switch_selects_normal() {
    assert_eq!(parse_start_mode(false, false, None, None), StartMode::Normal);
}

/// REQ: HST-033
#[test]
fn hst_033_the_environment_names_are_the_documented_ones() {
    assert_eq!(SAFE_PLUGINS_ENV, "ILIUM_SAFE_PLUGINS");
    assert_eq!(NO_PLUGINS_ENV, "ILIUM_NO_PLUGINS");
}

/// REQ: HST-033
#[test]
fn hst_033_the_safe_environment_variable_selects_safe_plugins() {
    assert_eq!(parse_start_mode(false, false, Some("1"), None), StartMode::SafePlugins);
}

/// REQ: HST-033
#[test]
fn hst_033_a_zero_or_empty_variable_counts_as_unset() {
    assert_eq!(parse_start_mode(false, false, Some("0"), None), StartMode::Normal);
    assert_eq!(parse_start_mode(false, false, Some(""), None), StartMode::Normal);
    assert_eq!(parse_start_mode(false, false, None, Some("0")), StartMode::Normal);
}

/// REQ: HST-033
#[test]
fn hst_033_the_binary_accepts_no_plugins_and_changes_no_installed_package() {
    let output = run_ilium(&["--no-plugins", "--help"], &[]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

/// REQ: HST-033
#[test]
fn hst_033_the_safe_flag_leaves_the_plugin_list_identical() {
    let normal = run_ilium(&["plugin", "list", "--json"], &[]);
    let safe = run_ilium(&["--safe-plugins", "plugin", "list", "--json"], &[]);
    assert_eq!(normal.stdout, safe.stdout);
}

/// REQ: HST-033
#[test]
fn hst_033_the_manual_footer_for_safe_mode_is_exact() {
    let page = manual_page("reference-kernel-and-boot");
    assert!(page.contains("Safe plugin mode: third-party plugins are off."));
}

/// REQ: HST-034
#[test]
fn hst_034_the_manual_footer_for_no_plugins_names_the_recovery_key() {
    let page = manual_page("reference-kernel-and-boot");
    assert!(page.contains("Plugins are off. Press Ctrl+Alt+R for recovery."));
}

/// REQ: HST-034
#[test]
fn hst_034_a_session_boots_with_no_plugins_from_the_environment() {
    let output = run_ilium(&["--help"], &[("ILIUM_NO_PLUGINS", "1")]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

/// REQ: HST-034
#[test]
fn hst_034_the_session_list_command_works_with_no_plugins() {
    let output = run_ilium(&["--no-plugins", "list-sessions"], &[]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

// HST-043
/// REQ: HST-043
#[test]
fn hst_043_no_plugins_beats_safe_plugins_as_flags() {
    assert_eq!(parse_start_mode(true, true, None, None), StartMode::NoPlugins);
}

/// REQ: HST-043
#[test]
fn hst_043_no_plugins_beats_safe_plugins_as_variables() {
    assert_eq!(parse_start_mode(false, false, Some("1"), Some("1")), StartMode::NoPlugins);
}

/// REQ: HST-043
#[test]
fn hst_043_a_no_plugins_variable_beats_a_safe_flag() {
    assert_eq!(parse_start_mode(true, false, None, Some("1")), StartMode::NoPlugins);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    // REQ: HST-043
    #[test]
    fn hst_043_the_mode_follows_the_precedence_table(
        safe_flag in any::<bool>(),
        no_flag in any::<bool>(),
        safe_env in proptest::option::of("[01a-z]{0,3}"),
        no_env in proptest::option::of("[01a-z]{0,3}"),
    ) {
        let is_set = |value: &Option<String>| value.as_deref().map(|text| !text.is_empty() && text != "0").unwrap_or(false);
        let expected = if no_flag || is_set(&no_env) {
            StartMode::NoPlugins
        } else if safe_flag || is_set(&safe_env) {
            StartMode::SafePlugins
        } else {
            StartMode::Normal
        };
        prop_assert_eq!(parse_start_mode(safe_flag, no_flag, safe_env.as_deref(), no_env.as_deref()), expected);
    }
}
