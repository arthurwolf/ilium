//! Baseline scenarios of src/docs/settings.md: F-007 keymap, F-086 debug file logging,
//! F-091 config persistence and live mirror, F-092 themes, motion, density, stable glyphs.

use ilium_plugin_host::lifecycle::StartMode;

use crate::common::*;

/// REQ: F-007, COR-001
#[test]
fn f_007_default_prefix_is_ctrl_b() {
    in_both_modes(|kernel| {
        assert_eq!(kernel.setting("keyboard", "shortcut_base").as_deref(), Some("ctrl+b"));
    });
}

/// REQ: F-007, COR-001
#[test]
fn f_007_default_prefix_then_d_detaches_and_prefix_then_ampersand_kills_the_session() {
    in_both_modes(|kernel| {
        assert_eq!(kernel.resolve_key(&["ctrl+b", "d"]).as_deref(), Some("detach_client"));
        assert_eq!(kernel.resolve_key(&["ctrl+b", "&"]).as_deref(), Some("kill_session"));
    });
}

/// REQ: F-007, COR-001
#[test]
fn f_007_prefix_then_quote_opens_a_new_split_view() {
    in_both_modes(|kernel| {
        assert_eq!(kernel.resolve_key(&["ctrl+b", "\""]).as_deref(), Some("new_split_view"));
    });
}

/// REQ: F-007, COR-001
#[test]
fn f_007_documented_remap_example_moves_the_prefix_and_new_terminal() {
    let config = "[keyboard]\nshortcut_base = \"ctrl+a\"\n\n[keybindings]\nnew_terminal = \"c\"\ncycle_next_in_group = \"down\"\n";
    for mode in MODES {
        let kernel = world_with_config(mode, config);
        assert_eq!(kernel.resolve_key(&["ctrl+a", "c"]).as_deref(), Some("new_terminal"), "{mode:?}");
        assert_eq!(kernel.resolve_key(&["ctrl+a", "down"]).as_deref(), Some("cycle_next_in_group"), "{mode:?}");
    }
}

/// REQ: F-007, COR-001
#[test]
fn f_007_after_a_remap_the_old_prefix_no_longer_triggers_actions() {
    let config = "[keyboard]\nshortcut_base = \"ctrl+a\"\n";
    for mode in MODES {
        let kernel = world_with_config(mode, config);
        assert_eq!(kernel.resolve_key(&["ctrl+b", "d"]), None, "{mode:?}");
    }
}

/// REQ: F-007, COR-001
#[test]
fn f_007_an_action_name_that_does_not_exist_cannot_be_defined_by_remapping() {
    let config = "[keybindings]\nsummon_dragon = \"z\"\n";
    for mode in MODES {
        let kernel = world_with_config(mode, config);
        assert_eq!(kernel.resolve_key(&["ctrl+b", "z"]), None, "{mode:?}");
    }
}

/// REQ: F-007, COR-001
#[test]
fn f_007_a_prefix_that_is_not_a_ctrl_letter_is_rejected() {
    in_both_modes(|kernel| {
        assert!(kernel.apply_setting("keyboard", "shortcut_base", "alt+b").is_err());
        assert_eq!(kernel.setting("keyboard", "shortcut_base").as_deref(), Some("ctrl+b"));
    });
}

/// REQ: F-086, COR-001
#[test]
fn f_086_file_logging_is_off_by_default_and_writes_nothing() {
    in_both_modes(|kernel| {
        assert_eq!(kernel.setting("debug", "file_logging_enabled").as_deref(), Some("false"));
        project(kernel, "api");
        kernel.settle();
        assert!(kernel.debug_log_files().is_empty());
    });
}

/// REQ: F-086, COR-001
#[test]
fn f_086_enabling_file_logging_creates_a_log_under_the_debug_log_root() {
    in_both_modes(|kernel| {
        kernel.apply_setting("debug", "file_logging_enabled", "true").expect("enable");
        project(kernel, "api");
        kernel.settle();
        let files = kernel.debug_log_files();
        assert_eq!(files.len(), 1, "{files:?}");
        assert!(files[0].starts_with(&kernel.debug_log_root()), "{files:?}");
    });
}

/// REQ: F-086, COR-001
#[test]
fn f_086_the_five_newest_logs_per_session_are_kept() {
    in_both_modes(|kernel| {
        kernel.apply_setting("debug", "file_logging_enabled", "true").expect("enable");
        for _ in 0..7 {
            kernel.restart_logging_session();
        }
        assert_eq!(kernel.debug_log_files().len(), 5);
    });
}

/// REQ: F-086, COR-001
#[test]
fn f_086_log_root_follows_the_environment_override() {
    for mode in MODES {
        let kernel = world_with_config(mode, "");
        kernel.set_environment("ILIUM_DEBUG_LOG_DIR", Some("logs-here"));
        assert!(kernel.debug_log_root().ends_with("logs-here"), "{:?}", kernel.debug_log_root());
    }
}

/// REQ: F-091, COR-001
#[test]
fn f_091_a_changed_setting_is_written_to_config_toml() {
    in_both_modes(|kernel| {
        kernel.apply_setting("terminal", "scrollback_budget_mib", "16").expect("apply");
        let text = kernel.config_text();
        let table: toml::Table = text.parse().expect("valid TOML");
        assert_eq!(table["terminal"]["scrollback_budget_mib"].as_integer(), Some(16));
    });
}

/// REQ: F-091, COR-001
#[test]
fn f_091_keys_of_the_file_that_ilium_does_not_know_are_preserved() {
    for mode in MODES {
        let kernel = world_with_config(mode, "[custom]\nsomething = \"kept\"\n");
        kernel.apply_setting("terminal", "scrollback_budget_mib", "16").expect("apply");
        assert!(kernel.config_text().contains("something = \"kept\""), "{mode:?}");
    }
}

/// REQ: F-091, COR-001
#[test]
fn f_091_a_setting_changed_on_disk_reaches_the_live_server() {
    in_both_modes(|kernel| {
        kernel.write_config_file("[terminal]\nscrollback_budget_mib = 32\n");
        kernel.reload_config();
        assert_eq!(kernel.setting("terminal", "scrollback_budget_mib").as_deref(), Some("32"));
    });
}

/// REQ: F-091, COR-001
#[test]
fn f_091_a_broken_config_file_keeps_the_previous_values_live() {
    in_both_modes(|kernel| {
        kernel.apply_setting("terminal", "scrollback_budget_mib", "16").expect("apply");
        kernel.write_config_file("[terminal\nscrollback_budget_mib = ");
        kernel.reload_config();
        assert_eq!(kernel.setting("terminal", "scrollback_budget_mib").as_deref(), Some("16"));
    });
}

/// REQ: F-091, COR-001
#[test]
fn f_091_config_dir_override_is_made_absolute() {
    for mode in MODES {
        let kernel = world_with_config(mode, "");
        kernel.set_environment("ILIUM_CONFIG_DIR", Some("relative/dir"));
        assert!(kernel.config_dir().is_absolute(), "{:?}", kernel.config_dir());
        assert!(kernel.config_dir().ends_with("relative/dir"));
    }
}

/// REQ: F-092, COR-001
#[test]
fn f_092_defaults_are_dark_theme_reduced_motion_standard_density_stable_glyphs_off() {
    in_both_modes(|kernel| {
        assert_eq!(kernel.setting("ui", "motion_level").as_deref(), Some("reduced"));
        assert_eq!(kernel.setting("ui", "sidebar_density").as_deref(), Some("standard"));
        assert_eq!(kernel.setting("ui", "use_stable_glyphs").as_deref(), Some("false"));
        assert_eq!(kernel.setting("ui", "color_scheme").as_deref(), Some("dark"));
    });
}

/// REQ: F-092, COR-001
#[test]
fn f_092_motion_level_accepts_full_reduced_off_and_nothing_else() {
    in_both_modes(|kernel| {
        for value in ["full", "reduced", "off"] {
            assert_eq!(kernel.apply_setting("ui", "motion_level", value), Ok(()), "{value}");
        }
        assert!(kernel.apply_setting("ui", "motion_level", "ludicrous").is_err());
    });
}

/// REQ: F-092, COR-001
#[test]
fn f_092_sidebar_density_accepts_compact_standard_comfortable_and_nothing_else() {
    in_both_modes(|kernel| {
        for value in ["compact", "standard", "comfortable"] {
            assert_eq!(kernel.apply_setting("ui", "sidebar_density", value), Ok(()), "{value}");
        }
        assert!(kernel.apply_setting("ui", "sidebar_density", "cramped").is_err());
    });
}

/// REQ: F-092, COR-001
#[test]
fn f_092_theme_table_takes_four_hex_colour_overrides() {
    in_both_modes(|kernel| {
        for key in ["accent_bg", "accent_fg", "border_focused", "border_unfocused"] {
            assert_eq!(kernel.apply_setting("theme", key, "#1f6feb"), Ok(()), "{key}");
        }
    });
}

/// REQ: F-092, COR-001
#[test]
fn f_092_a_theme_override_that_is_not_hex_is_rejected() {
    in_both_modes(|kernel| {
        assert!(kernel.apply_setting("theme", "accent_bg", "blue").is_err());
        assert!(kernel.apply_setting("theme", "accent_bg", "#12345").is_err());
    });
}

/// REQ: F-092, COR-001
#[test]
fn f_092_stable_glyphs_stay_off_unless_the_user_turns_them_on() {
    for mode in [StartMode::Normal, StartMode::NoPlugins] {
        let kernel = world_with_config(mode, "");
        kernel.reload_config();
        assert_eq!(kernel.setting("ui", "use_stable_glyphs").as_deref(), Some("false"), "{mode:?}");
    }
}
