//! The two rows the manifest contributes (section `appearance` of the User Interface tab): the
//! `density` choice and the `accent` colour. Validation follows reference-settings-rows.md:
//! a choice value that is not an option and a colour that does not match `^#[0-9a-fA-F]{6}$`
//! are `invalid-argument`; `null` resets a key to its default.

use ilium_plugin_sdk::types::ErrorCode;
use proptest::prelude::*;
use serde_json::{Value, json};

use crate::common::*;

/// REQ: F-090
#[test]
fn rows_090_density_defaults_to_comfortable() {
    let mut host = client_host();
    assert_eq!(host.settings().get("density"), Some(json!("comfortable")));
}

/// REQ: F-090
#[test]
fn rows_090_density_accepts_each_declared_option() {
    for option in ["compact", "comfortable"] {
        let mut host = client_host();
        host.settings().set("density", json!(option)).expect("accepted");
        assert_eq!(host.settings().get("density"), Some(json!(option)));
    }
}

/// REQ: F-090
#[test]
fn rows_090_density_refuses_a_value_that_is_not_an_option_with_invalid_argument() {
    let mut host = client_host();
    let error = host.settings().set("density", json!("spacious")).expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(host.settings().get("density"), Some(json!("comfortable")));
}

/// REQ: F-090
#[test]
fn rows_090_density_refuses_the_standard_option_of_the_user_interface_tab_invalid_argument() {
    // The sidebar density of the tree has three options; this plugin row has two.
    let mut host = client_host();
    let error = host.settings().set("density", json!("standard")).expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: F-090
#[test]
fn rows_090_density_refuses_a_non_string_value_with_invalid_argument() {
    let mut host = client_host();
    for value in [json!(1), json!(true), json!(["compact"]), json!({"value": "compact"})] {
        let error = host.settings().set("density", value).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }
}

/// REQ: F-090
#[test]
fn rows_090_density_null_resets_to_the_default() {
    let mut host = client_host();
    host.settings().set("density", json!("compact")).expect("accepted");
    host.settings().set("density", Value::Null).expect("null resets");
    assert_eq!(host.settings().get("density"), Some(json!("comfortable")));
}

/// REQ: F-092
#[test]
fn rows_092_accent_defaults_to_4a9eff() {
    let mut host = client_host();
    assert_eq!(host.settings().get("accent"), Some(json!("#4a9eff")));
}

/// REQ: F-092
#[test]
fn rows_092_accent_accepts_lower_and_upper_case_hex() {
    for colour in ["#000000", "#ffffff", "#FFFFFF", "#1f6feb", "#AbCdEf"] {
        let mut host = client_host();
        host.settings().set("accent", json!(colour)).expect("accepted");
        assert_eq!(host.settings().get("accent"), Some(json!(colour)));
    }
}

/// REQ: F-092
#[test]
fn rows_092_accent_refuses_text_that_is_not_rrggbb_with_invalid_argument() {
    for colour in ["red", "4a9eff", "#4a9ef", "#4a9eff0", "#4a9eg0", "", "#", "# 4a9eff", "#4a9eff "] {
        let mut host = client_host();
        let error = host.settings().set("accent", json!(colour)).expect_err(colour);
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{colour}");
        assert_eq!(host.settings().get("accent"), Some(json!("#4a9eff")), "{colour}");
    }
}

/// REQ: F-092
#[test]
fn rows_092_accent_refuses_a_non_string_value_with_invalid_argument() {
    let mut host = client_host();
    for value in [json!(255), json!(true)] {
        let error = host.settings().set("accent", value).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }
}

/// REQ: F-092
#[test]
fn rows_092_accent_null_resets_to_the_default() {
    let mut host = client_host();
    host.settings().set("accent", json!("#000000")).expect("accepted");
    host.settings().set("accent", Value::Null).expect("null resets");
    assert_eq!(host.settings().get("accent"), Some(json!("#4a9eff")));
}

/// REQ: F-090
#[test]
fn rows_090_an_unknown_key_of_the_appearance_section_is_refused_with_invalid_argument() {
    let mut host = client_host();
    let error = host.settings().set("no_such_row", json!(1)).expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

proptest! {
    // REQ: F-092
    #[test]
    fn rows_092_every_rrggbb_value_is_accepted_and_read_back(colour in "#[0-9a-fA-F]{6}") {
        let mut host = client_host();
        prop_assert!(host.settings().set("accent", json!(colour.clone())).is_ok());
        prop_assert_eq!(host.settings().get("accent"), Some(json!(colour)));
    }

    // REQ: F-092
    #[test]
    fn rows_092_values_of_the_wrong_length_are_refused(colour in "#[0-9a-f]{0,5}|#[0-9a-f]{7,12}") {
        let mut host = client_host();
        let error = host.settings().set("accent", json!(colour)).expect_err("refused");
        prop_assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    // REQ: F-090
    #[test]
    fn rows_090_density_accepts_only_its_two_options(word in "[a-z]{1,12}") {
        let mut host = client_host();
        let outcome = host.settings().set("density", json!(word.clone()));
        if word == "compact" || word == "comfortable" {
            prop_assert!(outcome.is_ok());
        } else {
            prop_assert_eq!(outcome.expect_err("refused").code, ErrorCode::InvalidArgument);
        }
    }
}
