use crate::support::*;

/// REQ: WID-002
#[test]
fn wid_002_flat_tree_with_root_zero_is_accepted() {
    let tree = chain(3);
    assert!(check(&tree).is_ok());
}

/// REQ: WID-002, HST-258
#[test]
fn wid_002_root_not_zero_traps_invalid_tree() {
    let mut tree = chain(3);
    tree.root = 1;
    assert_eq!(code_of(&tree), "ui.invalid-tree");
}

/// REQ: WID-002
#[test]
fn wid_002_child_index_out_of_range_traps_invalid_tree() {
    let mut tree = chain(3);
    tree.nodes[0].children = vec![99];
    assert_eq!(code_of(&tree), "ui.invalid-tree");
}

/// REQ: WID-002
#[test]
fn wid_002_child_index_not_greater_than_parent_traps_invalid_tree() {
    let mut tree = chain(3);
    tree.nodes[1].children = vec![1];
    assert_eq!(code_of(&tree), "ui.invalid-tree");
}

/// REQ: WID-002
#[test]
fn wid_002_two_parents_trap_invalid_tree() {
    let mut tree = chain(4);
    tree.nodes[0].children = vec![1, 2];
    tree.nodes[1].children = vec![3];
    tree.nodes[2].children = vec![3];
    assert_eq!(code_of(&tree), "ui.invalid-tree");
}

/// REQ: WID-002
#[test]
fn wid_002_cycle_traps_invalid_tree() {
    let mut tree = chain(3);
    tree.nodes[2].children = vec![0];
    assert_eq!(code_of(&tree), "ui.invalid-tree");
}

/// REQ: WID-002
#[test]
fn wid_002_empty_node_list_traps_invalid_tree() {
    let tree = ilium_plugin_api::generated::types::UiTree::default();
    assert_eq!(code_of(&tree), "ui.invalid-tree");
}

/// REQ: WID-002
#[test]
fn wid_002_orphan_node_traps_invalid_tree() {
    let mut tree = chain(3);
    tree.nodes[1].children = vec![];
    assert_eq!(code_of(&tree), "ui.invalid-tree");
}

/// REQ: HST-258, WID-014
#[test]
fn hst_258_invalid_tree_is_not_shown_and_previous_stays_stale() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.deliver_tree(view, chain(2)).unwrap();
    let mut bad = chain(3);
    bad.root = 2;
    let err = rig.deliver_tree(view, bad).unwrap_err();
    assert_eq!(err, "ui.invalid-tree");
    assert_eq!(rig.shown_tree(view), Some(chain(2)));
    assert!(rig.is_stale(view));
}

/// REQ: HST-258
#[test]
fn hst_258_each_named_ui_code_traps_the_instance() {
    for (tree, expected) in named_failure_trees() {
        let mut rig = rig_t2();
        let view = rig.open_view("main");
        assert_eq!(rig.deliver_tree(view, tree).unwrap_err(), expected);
        assert_eq!(rig.failures(), 1, "{expected}");
    }
}

/// REQ: HST-258, WID-014
#[test]
fn hst_258_trapped_render_logs_one_error_with_widget_path() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    let _ = rig.deliver_tree(view, duplicate_key_tree());
    let errors: Vec<_> = rig
        .log_lines()
        .into_iter()
        .filter(|(level, _)| level == "error")
        .collect();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].1.contains("save"));
}

/// REQ: WID-014
#[test]
fn wid_014_render_without_result_type_traps_with_matching_code() {
    for (tree, expected) in named_failure_trees() {
        assert_eq!(code_of(&tree), expected);
    }
}

/// REQ: WID-014
#[test]
fn wid_014_good_tree_stays_on_screen_after_trap() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.deliver_tree(view, chain(2)).unwrap();
    let _ = rig.deliver_tree(view, chain(40));
    assert_eq!(rig.shown_tree(view), Some(chain(2)));
}

/// REQ: WID-003, HST-272
#[test]
fn wid_003_duplicate_key_in_different_branches_traps_duplicate_key() {
    assert_eq!(code_of(&duplicate_key_tree()), "ui.duplicate-key");
}

/// REQ: WID-003
#[test]
fn wid_003_unique_keys_are_accepted() {
    let tree = keyed_chain(&["a", "b", "c"]);
    assert!(check(&tree).is_ok());
}

/// REQ: HST-272
#[test]
fn hst_272_sibling_duplicate_key_traps_duplicate_key() {
    let tree = keyed_chain(&["save", "save"]);
    assert_eq!(code_of(&tree), "ui.duplicate-key");
}

/// REQ: HST-272
#[test]
fn hst_272_duplicate_key_trap_counts_one_failure() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    assert_eq!(
        rig.deliver_tree(view, duplicate_key_tree()).unwrap_err(),
        "ui.duplicate-key"
    );
    assert_eq!(rig.failures(), 1);
}

/// REQ: WID-004
#[test]
fn wid_004_4096_nodes_are_accepted() {
    assert!(check(&chain_flat(4096)).is_ok());
}

/// REQ: WID-004, HST-258
#[test]
fn wid_004_4097_nodes_trap_too_large() {
    assert_eq!(code_of(&chain_flat(4097)), "ui.too-large");
}

/// REQ: WID-004
#[test]
fn wid_004_node_limit_follows_the_effective_limit() {
    let caps = ilium_plugin_api::uitree::UiCaps {
        max_nodes: 10,
        max_kib: 1024,
        max_depth: 32,
    };
    assert!(ilium_plugin_api::uitree::check_ui_tree(&chain_flat(10), caps).is_ok());
    assert_eq!(
        ilium_plugin_api::uitree::check_ui_tree(&chain_flat(11), caps)
            .unwrap_err()
            .as_str(),
        "ui.too-large"
    );
}

/// REQ: WID-004, HST-258
#[test]
fn wid_004_over_one_mebibyte_traps_too_large() {
    let mut tree = chain_flat(8);
    tree.nodes[1].key = Some("x".repeat(64));
    let caps = ilium_plugin_api::uitree::UiCaps {
        max_nodes: 4096,
        max_kib: 0,
        max_depth: 32,
    };
    assert_eq!(
        ilium_plugin_api::uitree::check_ui_tree(&tree, caps)
            .unwrap_err()
            .as_str(),
        "ui.too-large"
    );
}

/// REQ: WID-017, RND-020
#[test]
fn wid_017_depth_32_is_accepted() {
    assert!(check(&chain(32)).is_ok());
}

/// REQ: WID-017, RND-020, HST-258
#[test]
fn wid_017_depth_33_traps_too_deep() {
    assert_eq!(code_of(&chain(33)), "ui.too-deep");
}

/// REQ: RND-020
#[test]
fn rnd_020_root_counts_as_level_one() {
    assert!(check(&chain(1)).is_ok());
    assert_eq!(code_of(&chain(33)), "ui.too-deep");
}

/// REQ: RND-020, HST-258
#[test]
fn rnd_020_depth_trap_keeps_previous_tree() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.deliver_tree(view, chain(2)).unwrap();
    assert_eq!(
        rig.deliver_tree(view, chain(33)).unwrap_err(),
        "ui.too-deep"
    );
    assert_eq!(rig.shown_tree(view), Some(chain(2)));
}

/// REQ: WID-016, HST-258
#[test]
fn wid_016_settings_view_rejects_terminal_view_widget_not_allowed() {
    let mut rig = rig_t2();
    let view = rig.open_settings_view("settings");
    assert_eq!(
        rig.deliver_widget(view, "terminal-view").unwrap_err(),
        "ui.widget-not-allowed"
    );
    assert_eq!(rig.failures(), 1);
}

/// REQ: WID-016
#[test]
fn wid_016_settings_view_keeps_last_good_tree() {
    let mut rig = rig_t2();
    let view = rig.open_settings_view("settings");
    rig.deliver_tree(view, chain(2)).unwrap();
    let _ = rig.deliver_widget(view, "image");
    assert_eq!(rig.shown_tree(view), Some(chain(2)));
}

/// REQ: WID-016
#[test]
fn wid_016_settings_view_allows_toggle() {
    let mut rig = rig_t2();
    let view = rig.open_settings_view("settings");
    assert_eq!(rig.deliver_widget(view, "toggle").err(), None);
    assert_eq!(rig.failures(), 0);
}

/// REQ: WID-020
#[test]
fn wid_020_check_list_256_items_accepted_257_traps() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    assert!(rig
        .deliver_widget_with_items(view, "check-list", 256)
        .is_ok());
    assert!(rig
        .deliver_widget_with_items(view, "check-list", 257)
        .is_err());
    assert!(rig
        .log_lines()
        .iter()
        .any(|(_, m)| m.contains("invalid-argument")));
}

/// REQ: WID-020
#[test]
fn wid_020_list_editor_64_items_for_third_party_tier() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    assert!(rig
        .deliver_widget_with_items(view, "list-editor", 64)
        .is_ok());
    assert!(rig
        .deliver_widget_with_items(view, "list-editor", 65)
        .is_err());
    assert!(rig
        .log_lines()
        .iter()
        .any(|(_, m)| m.contains("invalid-argument")));
}

/// REQ: WID-020
#[test]
fn wid_020_list_editor_1024_items_for_first_party_tier() {
    let mut rig = rig_t1();
    let view = rig.open_view("main");
    assert!(rig
        .deliver_widget_with_items(view, "list-editor", 1024)
        .is_ok());
    assert!(rig
        .deliver_widget_with_items(view, "list-editor", 1025)
        .is_err());
}

/// REQ: WID-021
#[test]
fn wid_021_meter_8_segments_accepted_9_traps() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    assert!(rig.deliver_widget_with_items(view, "meter", 8).is_ok());
    assert!(rig.deliver_widget_with_items(view, "meter", 9).is_err());
    assert!(rig
        .log_lines()
        .iter()
        .any(|(_, m)| m.contains("invalid-argument")));
}

/// REQ: WID-021
#[test]
fn wid_021_minimap_4096_marks_accepted_4097_traps() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    assert!(rig.deliver_widget_with_items(view, "minimap", 4096).is_ok());
    assert!(rig
        .deliver_widget_with_items(view, "minimap", 4097)
        .is_err());
}

/// REQ: WID-021
#[test]
fn wid_021_spinner_16_custom_frames_accepted_17_traps() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    assert!(rig.deliver_widget_with_items(view, "spinner", 16).is_ok());
    assert!(rig.deliver_widget_with_items(view, "spinner", 17).is_err());
}

/// REQ: WID-001
#[test]
fn wid_001_catalogue_holds_51_distinct_widgets() {
    let names = ilium_plugin_api::generated::types::Widget::CASE_NAMES;
    let distinct: std::collections::BTreeSet<_> = names.iter().collect();
    assert_eq!(distinct.len(), 51);
}

/// REQ: WID-001
#[test]
fn wid_001_catalogue_has_seven_families() {
    assert_eq!(widget_families().len(), 7);
    assert_eq!(
        widget_families().values().map(|v| v.len()).sum::<usize>(),
        51
    );
}

/// REQ: WID-019
#[test]
fn wid_019_wide_glyphs_count_two_cells_combining_zero() {
    assert_eq!(cell_width("\u{4e2d}\u{6587}\u{5b57}"), 6);
    assert_eq!(cell_width("e\u{301}"), 1);
    assert_eq!(cell_width("\u{301}"), 0);
}

/// REQ: RND-019
#[test]
fn rnd_019_control_chars_and_escapes_removed() {
    assert_eq!(sanitise("a\u{1b}[31mb\u{7}c", false), "abc");
}

/// REQ: RND-019
#[test]
fn rnd_019_tab_expands_to_next_multiple_of_four() {
    assert_eq!(sanitise("ab\tc", false), "ab  c");
    assert_eq!(sanitise("\tx", false), "    x");
}

/// REQ: RND-019
#[test]
fn rnd_019_single_line_turns_cr_lf_crlf_into_one_space() {
    for sep in ["\r", "\n", "\r\n"] {
        assert_eq!(sanitise(&format!("a{sep}b"), true), "a b");
    }
}

/// REQ: RND-019
#[test]
fn rnd_019_at_most_one_text_sanitized_warning_per_view() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    for _ in 0..5 {
        rig.deliver_tree(view, tree_with_text("a\u{1b}b")).unwrap();
    }
    let warnings = rig
        .log_lines()
        .into_iter()
        .filter(|(_, m)| m.contains("ui.text-sanitized"))
        .count();
    assert_eq!(warnings, 1);
}

/// REQ: WID-015
#[test]
fn wid_015_tooltip_over_200_scalars_cut_at_199_with_ellipsis() {
    let shown = tooltip_shown(&"x".repeat(300));
    assert_eq!(shown.chars().count(), 200);
    assert!(shown.ends_with('\u{2026}'));
}

/// REQ: WID-015
#[test]
fn wid_015_tooltip_of_200_scalars_is_not_cut() {
    let text = "y".repeat(200);
    assert_eq!(tooltip_shown(&text), text);
}

/// REQ: WID-015
#[test]
fn wid_015_long_tooltip_is_not_an_error() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    assert!(rig
        .deliver_tree(view, tree_with_tooltip(&"x".repeat(300)))
        .is_ok());
    assert_eq!(rig.failures(), 0);
}

/// REQ: WID-018
#[test]
fn wid_018_tab_order_positive_ascending_then_preorder() {
    let order = tab_order(&[
        (Some(2), false),
        (Some(1), false),
        (None, false),
        (Some(0), false),
    ]);
    assert_eq!(order, vec![2, 1, 3, 4]);
}

/// REQ: WID-018
#[test]
fn wid_018_negative_and_disabled_reason_skipped() {
    let order = tab_order(&[(Some(-1), false), (None, true), (None, false)]);
    assert_eq!(order, vec![3]);
}

/// REQ: WID-018
#[test]
fn wid_018_tab_wraps_around() {
    let order = tab_order(&[(None, false), (None, false)]);
    assert_eq!(order, vec![1, 2]);
    assert_eq!(tab_next(&order, 2), 1);
}
