//! Plugin settings schema validation and the kernel settings tabs (reference-settings.md).

use ilium_plugin_api::manifest::Diagnostic;

/// The number of frozen kernel settings tabs (reference-settings.md).
pub const KERNEL_TAB_COUNT: usize = 25;

/// The ids of the frozen kernel settings tabs, in display order (reference-settings.md).
pub fn kernel_tab_ids() -> &'static [&'static str] {
    unimplemented!("host::settings::kernel_tab_ids")
}

/// Validates a settings value against a plugin's declared schema.
pub fn validate_settings(_schema_json: &str, _value_json: &str) -> Result<(), Vec<Diagnostic>> {
    unimplemented!("host::settings::validate_settings")
}

/// Whether a plugin may write a kernel settings tab (`settings.write-kernel:<tab>`).
pub fn may_write_kernel_tab(_granted: &[ilium_plugin_api::grants::Grant], _tab: &str) -> bool {
    unimplemented!("host::settings::may_write_kernel_tab")
}

#[cfg(test)]
mod tests {
    use super::{kernel_tab_ids, may_write_kernel_tab, validate_settings, KERNEL_TAB_COUNT};
    use ilium_plugin_api::codes::settings as settings_codes;
    use ilium_plugin_api::grants::Grant;
    use ilium_plugin_api::manifest::Diagnostic;

    /// Object schema with one property per row type the validator must check: a bounded
    /// integer, a length-limited string, an enum and a boolean (reference-settings-schema.md).
    const SETTINGS_SCHEMA: &str = r#"{"type":"object","properties":{"max_entries":{"type":"integer","minimum":10,"maximum":5000},"label":{"type":"string","maxLength":40},"mode":{"type":"string","enum":["text","rich"]},"enabled":{"type":"boolean"}}}"#;

    fn grant(text: &str) -> Grant {
        Grant::parse(text).expect("test grant text must parse")
    }

    fn expect_rejected(value_json: &str) -> Vec<Diagnostic> {
        match validate_settings(SETTINGS_SCHEMA, value_json) {
            Ok(()) => panic!("value {value_json} must be rejected"),
            Err(diagnostics) => diagnostics,
        }
    }

    #[test]
    fn kernel_tab_ids_count_matches_frozen_constant() {
        assert_eq!(kernel_tab_ids().len(), KERNEL_TAB_COUNT);
        assert_eq!(KERNEL_TAB_COUNT, 25);
    }

    #[test]
    fn kernel_tab_ids_follow_rail_order() {
        let expected = [
            "user-interface",
            "icons",
            "agent-monitoring",
            "agent-cost",
            "optimization",
            "remote-compaction",
            "keyboard",
            "terminal",
            "editor",
            "session",
            "git",
            "kanban-board",
            "animations",
            "sound",
            "voice-control",
            "reset-planning",
            "inference",
            "llm-instructions",
            "titles",
            "triggers",
            "text-triggers",
            "debug",
            "api",
            "about",
            "setup",
        ];
        assert_eq!(kernel_tab_ids(), &expected[..]);
    }

    #[test]
    fn kernel_tab_ids_are_unique() {
        let ids = kernel_tab_ids();
        for (index, id) in ids.iter().enumerate() {
            assert!(
                !ids[index + 1..].contains(id),
                "kernel tab id {id} appears twice"
            );
        }
    }

    #[test]
    fn kernel_tab_ids_match_tab_id_grammar() {
        for id in kernel_tab_ids() {
            let mut characters = id.chars();
            let first_is_letter = characters.next().is_some_and(|c| c.is_ascii_lowercase());
            let rest_is_valid = characters
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            assert!(first_is_letter && rest_is_valid, "bad id {id}");
            assert!(id.len() <= 32, "id {id} is longer than 32 characters");
        }
    }

    /// REQ: SET-021
    #[test]
    fn validate_accepts_a_value_that_passes_every_row() {
        let value = r#"{"max_entries":500,"label":"work","mode":"text","enabled":true}"#;
        assert!(validate_settings(SETTINGS_SCHEMA, value).is_ok());
    }

    /// REQ: SET-011
    #[test]
    fn validate_accepts_an_empty_object_so_defaults_apply() {
        assert!(validate_settings(SETTINGS_SCHEMA, "{}").is_ok());
    }

    /// REQ: SET-020
    #[test]
    fn validate_accepts_number_at_minimum_boundary() {
        assert!(validate_settings(SETTINGS_SCHEMA, r#"{"max_entries":10}"#).is_ok());
    }

    /// REQ: SET-020
    #[test]
    fn validate_accepts_number_at_maximum_boundary() {
        assert!(validate_settings(SETTINGS_SCHEMA, r#"{"max_entries":5000}"#).is_ok());
    }

    /// REQ: SET-020
    #[test]
    fn validate_rejects_number_one_below_minimum_with_invalid_value_code() {
        let diagnostics = expect_rejected(r#"{"max_entries":9}"#);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code.as_str(),
            settings_codes::INVALID_VALUE
        );
    }

    /// REQ: SET-020
    #[test]
    fn validate_rejects_number_one_above_maximum_with_invalid_value_code() {
        let diagnostics = expect_rejected(r#"{"max_entries":5001}"#);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code.as_str(),
            settings_codes::INVALID_VALUE
        );
    }

    /// REQ: SET-021
    #[test]
    fn validate_accepts_string_at_max_length_boundary() {
        let label = "a".repeat(40);
        let value = format!(r#"{{"label":"{label}"}}"#);
        assert!(validate_settings(SETTINGS_SCHEMA, &value).is_ok());
    }

    /// REQ: SET-021
    #[test]
    fn validate_rejects_string_one_past_max_length() {
        let label = "a".repeat(41);
        let value = format!(r#"{{"label":"{label}"}}"#);
        let diagnostics = expect_rejected(&value);
        assert!(!diagnostics.is_empty());
    }

    /// REQ: SET-021
    #[test]
    fn validate_rejects_enum_value_outside_options() {
        let diagnostics = expect_rejected(r#"{"mode":"other"}"#);
        assert!(!diagnostics.is_empty());
    }

    /// REQ: SET-021
    #[test]
    fn validate_rejects_number_sent_as_string() {
        let diagnostics = expect_rejected(r#"{"max_entries":"500"}"#);
        assert!(!diagnostics.is_empty());
    }

    /// REQ: SET-021
    #[test]
    fn validate_rejects_boolean_sent_as_string() {
        let diagnostics = expect_rejected(r#"{"enabled":"yes"}"#);
        assert!(!diagnostics.is_empty());
    }

    /// REQ: COR-031
    #[test]
    fn write_kernel_grant_permits_its_own_tab() {
        let granted = vec![grant("settings.write-kernel:api")];
        assert!(may_write_kernel_tab(&granted, "api"));
    }

    /// REQ: COR-031
    #[test]
    fn write_kernel_grant_does_not_permit_another_tab() {
        let granted = vec![grant("settings.write-kernel:api")];
        assert!(!may_write_kernel_tab(&granted, "terminal"));
    }

    /// REQ: COR-031
    #[test]
    fn write_kernel_grant_matches_tab_id_exactly_not_by_prefix() {
        let granted = vec![grant("settings.write-kernel:api")];
        assert!(!may_write_kernel_tab(&granted, "ap"));
        assert!(!may_write_kernel_tab(&granted, "api-keys"));
    }

    /// REQ: COR-031
    #[test]
    fn empty_tab_id_is_never_writable() {
        let granted = vec![grant("settings.write-kernel:api")];
        assert!(!may_write_kernel_tab(&granted, ""));
    }

    /// REQ: COR-031
    #[test]
    fn no_grants_permit_no_tab() {
        assert!(!may_write_kernel_tab(&[], "api"));
    }

    /// REQ: COR-031
    #[test]
    fn read_kernel_grant_does_not_permit_writes() {
        let granted = vec![grant("settings.read-kernel:api")];
        assert!(!may_write_kernel_tab(&granted, "api"));
    }

    /// REQ: COR-031
    #[test]
    fn write_all_grant_does_not_permit_kernel_tab_writes() {
        let granted = vec![grant("settings.write-all")];
        assert!(!may_write_kernel_tab(&granted, "api"));
    }

    /// REQ: COR-031
    #[test]
    fn matching_grant_is_found_among_unrelated_grants() {
        let granted = vec![
            grant("settings.read-kernel:terminal"),
            grant("settings.write-all"),
            grant("settings.write-kernel:terminal"),
        ];
        assert!(may_write_kernel_tab(&granted, "terminal"));
        assert!(!may_write_kernel_tab(&granted, "editor"));
    }
}
