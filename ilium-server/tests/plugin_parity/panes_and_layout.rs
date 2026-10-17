//! Baseline scenarios of src/docs/panes-and-layout.md and how-it-works.md, run with plugins and
//! kernel only (COR-001): F-001..004, F-008, F-011..014.

use ilium_plugin_api::generated::types::{
    Delivery, ErrorCode, Modifiers, NodeClass, Outcome, Placement, SplitDirection,
};
use ilium_plugin_api::kernel::KernelFacade;
use ilium_plugin_host::lifecycle::StartMode;

use crate::common::*;

/// REQ: F-001, COR-001
#[test]
fn f_001_tree_holds_project_group_folder_and_pane_in_preorder() {
    let titles_seen = in_both_modes(|kernel| {
        let project = project(kernel, "api");
        let group = group(kernel, Some(project), "work");
        let folder = folder(kernel, Some(group), "docs");
        terminal(kernel, Some(folder), "shell", &["sh"]);
        titles(kernel)
    });
    assert_eq!(titles_seen, vec!["api", "work", "docs", "shell"]);
}

/// REQ: F-001, COR-001
#[test]
fn f_001_node_classes_are_container_container_folder_pane() {
    in_both_modes(|kernel| {
        let project = project(kernel, "api");
        let group = group(kernel, Some(project), "work");
        let folder = folder(kernel, Some(group), "docs");
        let pane = terminal(kernel, Some(folder), "shell", &["sh"]);
        assert_eq!(class_of(kernel, project), NodeClass::Container);
        assert_eq!(class_of(kernel, group), NodeClass::Container);
        assert_eq!(class_of(kernel, folder), NodeClass::Folder);
        assert_eq!(class_of(kernel, pane), NodeClass::Pane);
    });
}

/// REQ: F-001, COR-001
#[test]
fn f_001_every_child_names_its_parent() {
    in_both_modes(|kernel| {
        let project = project(kernel, "api");
        let pane = terminal(kernel, Some(project), "shell", &["sh"]);
        assert_eq!(node(kernel, pane).parent, Some(project));
        assert_eq!(node(kernel, project).parent, None);
    });
}

/// REQ: F-001, COR-001
#[test]
fn f_001_tree_epoch_grows_with_every_structural_change() {
    in_both_modes(|kernel| {
        let before = kernel.tree().epoch();
        project(kernel, "api");
        let after = kernel.tree().epoch();
        assert!(after > before, "{before} -> {after}");
    });
}

/// REQ: F-002, COR-001
#[test]
fn f_002_a_group_without_parent_is_a_top_level_destination() {
    in_both_modes(|kernel| {
        let top = group(kernel, None, "scratch");
        assert_eq!(node(kernel, top).parent, None);
    });
}

/// REQ: F-002, COR-001
#[test]
fn f_002_closing_the_last_item_removes_the_empty_group() {
    in_both_modes(|kernel| {
        let top = group(kernel, None, "scratch");
        let pane = terminal(kernel, Some(top), "shell", &["sh"]);
        close(kernel, pane);
        assert!(
            kernel.tree().node(top).is_none(),
            "an empty group is removed automatically"
        );
    });
}

/// REQ: F-002, COR-001
#[test]
fn f_002_removal_repeats_up_through_ancestors_that_become_empty() {
    in_both_modes(|kernel| {
        let outer = group(kernel, None, "outer");
        let inner = folder(kernel, Some(outer), "inner");
        let pane = terminal(kernel, Some(inner), "shell", &["sh"]);
        close(kernel, pane);
        assert!(kernel.tree().node(inner).is_none());
        assert!(kernel.tree().node(outer).is_none());
    });
}

/// REQ: F-002, COR-001
#[test]
fn f_002_a_project_is_never_removed_automatically() {
    in_both_modes(|kernel| {
        let root = project(kernel, "api");
        let pane = terminal(kernel, Some(root), "shell", &["sh"]);
        close(kernel, pane);
        assert!(kernel.tree().node(root).is_some());
    });
}

/// REQ: F-003, COR-001
#[test]
fn f_003_split_of_two_three_and_four_panes_is_created() {
    in_both_modes(|kernel| {
        let panes: Vec<_> = (0..4)
            .map(|n| terminal(kernel, None, &format!("p{n}"), &["sh"]))
            .collect();
        assert!(matches!(
            split(kernel, &panes[..2], SplitDirection::Vertical),
            Outcome::OkNode(_)
        ));
        let more: Vec<_> = (0..3)
            .map(|n| terminal(kernel, None, &format!("q{n}"), &["sh"]))
            .collect();
        assert!(matches!(
            split(kernel, &more, SplitDirection::Horizontal),
            Outcome::OkNode(_)
        ));
        let four: Vec<_> = (0..4)
            .map(|n| terminal(kernel, None, &format!("r{n}"), &["sh"]))
            .collect();
        assert!(matches!(
            split(kernel, &four, SplitDirection::Vertical),
            Outcome::OkNode(_)
        ));
    });
}

/// REQ: F-003, COR-001
#[test]
fn f_003_a_split_of_five_panes_is_refused() {
    in_both_modes(|kernel| {
        let panes: Vec<_> = (0..5)
            .map(|n| terminal(kernel, None, &format!("p{n}"), &["sh"]))
            .collect();
        assert!(is_refused(&split(kernel, &panes, SplitDirection::Vertical)));
    });
}

/// REQ: F-003, COR-001
#[test]
fn f_003_a_split_of_one_pane_is_refused() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "only", &["sh"]);
        assert!(is_refused(&split(
            kernel,
            &[pane],
            SplitDirection::Vertical
        )));
    });
}

/// REQ: F-003, COR-001
#[test]
fn f_003_a_pane_already_in_a_split_is_not_offered_to_a_second_split() {
    in_both_modes(|kernel| {
        let a = terminal(kernel, None, "a", &["sh"]);
        let b = terminal(kernel, None, "b", &["sh"]);
        let c = terminal(kernel, None, "c", &["sh"]);
        created(split(kernel, &[a, b], SplitDirection::Vertical));
        assert!(is_refused(&split(
            kernel,
            &[b, c],
            SplitDirection::Vertical
        )));
    });
}

/// REQ: F-003, COR-001
#[test]
fn f_003_two_panes_vertical_split_have_equal_columns_side_by_side() {
    in_both_modes(|kernel| {
        let a = terminal(kernel, None, "a", &["sh"]);
        let b = terminal(kernel, None, "b", &["sh"]);
        let split_id = created(split(kernel, &[a, b], SplitDirection::Vertical));
        let rects = kernel.split_rects(split_id, 100, 40);
        assert_eq!(rects, vec![(0, 0, 50, 40), (50, 0, 50, 40)]);
        rects
    });
}

/// REQ: F-003, COR-001
#[test]
fn f_003_three_panes_horizontal_split_have_equal_rows_stacked() {
    in_both_modes(|kernel| {
        let panes: Vec<_> = (0..3)
            .map(|n| terminal(kernel, None, &format!("p{n}"), &["sh"]))
            .collect();
        let split_id = created(split(kernel, &panes, SplitDirection::Horizontal));
        let rects = kernel.split_rects(split_id, 90, 30);
        assert_eq!(
            rects,
            vec![(0, 0, 90, 10), (0, 10, 90, 10), (0, 20, 90, 10)]
        );
        rects
    });
}

/// REQ: F-003, COR-001
#[test]
fn f_003_four_panes_are_a_two_by_two_grid_in_either_direction() {
    in_both_modes(|kernel| {
        let panes: Vec<_> = (0..4)
            .map(|n| terminal(kernel, None, &format!("p{n}"), &["sh"]))
            .collect();
        let vertical = created(split(kernel, &panes, SplitDirection::Vertical));
        let grid = vec![
            (0, 0, 50, 20),
            (50, 0, 50, 20),
            (0, 20, 50, 20),
            (50, 20, 50, 20),
        ];
        assert_eq!(kernel.split_rects(vertical, 100, 40), grid);
        let more: Vec<_> = (0..4)
            .map(|n| terminal(kernel, None, &format!("q{n}"), &["sh"]))
            .collect();
        let horizontal = created(split(kernel, &more, SplitDirection::Horizontal));
        assert_eq!(kernel.split_rects(horizontal, 100, 40), grid);
    });
}

/// REQ: F-003, COR-001
#[test]
fn f_003_a_single_pane_fills_the_area() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "only", &["sh"]);
        assert_eq!(kernel.layout_rect(pane, 80, 24), (0, 0, 80, 24));
    });
}

/// REQ: F-004, COR-001
#[test]
fn f_004_rename_changes_the_title_and_bumps_the_revision() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "old", &["sh"]);
        let before = node(kernel, pane).revision;
        assert!(matches!(rename(kernel, pane, "new"), Outcome::Ok));
        let after = node(kernel, pane);
        assert_eq!(after.title, "new");
        assert!(after.revision > before);
    });
}

/// REQ: F-004, COR-001
#[test]
fn f_004_move_before_a_sibling_reorders_the_preorder() {
    let order = in_both_modes(|kernel| {
        let root = group(kernel, None, "g");
        let first = terminal(kernel, Some(root), "first", &["sh"]);
        let second = terminal(kernel, Some(root), "second", &["sh"]);
        assert!(matches!(
            move_node(kernel, second, first, Placement::Before),
            Outcome::Ok
        ));
        titles(kernel)
    });
    assert_eq!(order, vec!["g", "second", "first"]);
}

/// REQ: F-004, COR-001
#[test]
fn f_004_move_inside_last_reparents_the_node() {
    in_both_modes(|kernel| {
        let target = group(kernel, None, "target");
        let other = group(kernel, None, "other");
        let pane = terminal(kernel, Some(other), "p", &["sh"]);
        assert!(matches!(
            move_node(kernel, pane, target, Placement::InsideLast),
            Outcome::Ok
        ));
        assert_eq!(node(kernel, pane).parent, Some(target));
    });
}

/// REQ: F-004, COR-001
#[test]
fn f_004_moving_a_node_into_its_own_subtree_is_refused() {
    in_both_modes(|kernel| {
        let outer = group(kernel, None, "outer");
        let inner = group(kernel, Some(outer), "inner");
        assert!(is_refused(&move_node(
            kernel,
            outer,
            inner,
            Placement::InsideLast
        )));
    });
}

/// REQ: F-004, COR-001
#[test]
fn f_004_rename_of_a_missing_node_is_refused_not_found() {
    in_both_modes(|kernel| {
        let outcome = rename(
            kernel,
            ilium_plugin_api::generated::types::NodeId(9_999),
            "x",
        );
        assert_eq!(refusal_code(&outcome), Some(ErrorCode::NotFound));
    });
}

/// REQ: F-008, COR-001
#[test]
fn f_008_left_click_in_a_pane_with_sgr_mouse_reports_press_then_release_one_based() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "app", &["cat"]);
        kernel.feed_output(pane, b"\x1b[?1000h\x1b[?1006h");
        kernel.mouse_click(pane, 5, 3, 0);
        let log = kernel.pane_input_log(pane);
        assert_eq!(log, b"\x1b[<0;6;4M\x1b[<0;6;4m".to_vec());
    });
}

/// REQ: F-008, COR-001
#[test]
fn f_008_wheel_up_is_button_64_in_sgr_mode() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "app", &["cat"]);
        kernel.feed_output(pane, b"\x1b[?1000h\x1b[?1006h");
        kernel.mouse_wheel(pane, 0, 0, true);
        assert_eq!(kernel.pane_input_log(pane), b"\x1b[<64;1;1M".to_vec());
    });
}

/// REQ: F-008, COR-001
#[test]
fn f_008_a_click_in_a_pane_that_did_not_ask_for_mouse_sends_nothing() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "app", &["cat"]);
        kernel.mouse_click(pane, 5, 3, 0);
        assert!(kernel.pane_input_log(pane).is_empty());
    });
}

/// REQ: F-011, COR-001
#[test]
fn f_011_a_pane_runs_its_command_and_reports_pane_exited_when_it_ends() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "job", &["sh", "-c", "exit 3"]);
        kernel.settle();
        let names = kernel.event_names_for(pane);
        assert!(names.iter().any(|name| name == "pane-exited"), "{names:?}");
        assert_eq!(kernel.pane_exit_code(pane), Some(3));
    });
}

/// REQ: F-011, COR-001
#[test]
fn f_011_output_of_the_process_reaches_the_screen() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "echo", &["sh", "-c", "echo parity-marker"]);
        kernel.settle();
        let screen = kernel.panes().screen_text(pane).expect("screen");
        assert!(screen.contains("parity-marker"), "{screen:?}");
    });
}

/// REQ: F-011, COR-001
#[test]
fn f_011_an_unstartable_command_keeps_the_pane_in_the_tree() {
    in_both_modes(|kernel| {
        let pane = terminal(
            kernel,
            None,
            "broken",
            &["/nonexistent/ilium-parity-binary"],
        );
        kernel.settle();
        assert!(kernel.tree().node(pane).is_some());
    });
}

/// REQ: F-012, COR-001
#[test]
fn f_012_screen_text_shows_fed_lines_in_order() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "t", &["cat"]);
        kernel.feed_output(pane, b"hello\r\nworld");
        let screen = kernel.panes().screen_text(pane).expect("screen");
        let rows: Vec<&str> = screen.lines().collect();
        assert_eq!(&rows[..2], &["hello", "world"]);
    });
}

/// REQ: F-012, COR-001
#[test]
fn f_012_clear_screen_sequence_empties_the_visible_text() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "t", &["cat"]);
        kernel.feed_output(pane, b"junk\x1b[2J\x1b[Hfresh");
        let screen = kernel.panes().screen_text(pane).expect("screen");
        assert!(!screen.contains("junk"), "{screen:?}");
        assert!(screen.starts_with("fresh"));
    });
}

/// REQ: F-012, COR-001
#[test]
fn f_012_cells_carry_the_style_of_sgr_sequences() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "t", &["cat"]);
        kernel.feed_output(pane, b"\x1b[1mB\x1b[0mn");
        let rows = kernel.panes().cells(pane, 0..1).expect("cells");
        assert_eq!(rows[0][0].text, "B");
        assert!(rows[0][0].add.contains(Modifiers::BOLD));
        assert!(!rows[0][1].add.contains(Modifiers::BOLD));
    });
}

/// REQ: F-012, COR-001
#[test]
fn f_012_screen_of_an_unknown_pane_is_none() {
    in_both_modes(|kernel| {
        assert!(kernel
            .panes()
            .screen_text(ilium_plugin_api::generated::types::NodeId(777))
            .is_none());
    });
}

/// REQ: F-013, COR-001
#[test]
fn f_013_history_keeps_the_last_lines_scrolled_off_the_screen_in_order() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "t", &["cat"]);
        let text: String = (0..200).map(|n| format!("line{n}\r\n")).collect();
        kernel.feed_output(pane, text.as_bytes());
        let history = kernel.panes().history(pane, 3).expect("history");
        assert_eq!(history.len(), 3);
        // The newest scrolled-off lines are the three just above the first visible row.
        let screen = kernel.panes().screen_text(pane).expect("screen");
        let first_visible: usize = screen
            .lines()
            .next()
            .and_then(|row| row.trim().strip_prefix("line"))
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("screen must start with a numbered line: {screen:?}"));
        assert!(first_visible >= 3, "output must have scrolled: {screen:?}");
        let expected: Vec<String> = (first_visible - 3..first_visible)
            .map(|n| format!("line{n}"))
            .collect();
        assert_eq!(history, expected);
    });
}

/// REQ: F-013, COR-001
#[test]
fn f_013_scrollback_budget_default_is_8_mib_and_accepts_4_to_512_in_steps_of_4() {
    for mode in MODES {
        let kernel = world(mode);
        assert_eq!(
            kernel
                .setting("terminal", "scrollback_budget_mib")
                .as_deref(),
            Some("8")
        );
        for accepted in ["4", "8", "512"] {
            assert_eq!(
                kernel.apply_setting("terminal", "scrollback_budget_mib", accepted),
                Ok(()),
                "{accepted}"
            );
        }
    }
}

/// REQ: F-013, COR-001
#[test]
fn f_013_scrollback_budget_outside_4_to_512_or_off_step_is_rejected() {
    for mode in MODES {
        let kernel = world(mode);
        for rejected in ["0", "3", "6", "516", "1024"] {
            assert!(
                kernel
                    .apply_setting("terminal", "scrollback_budget_mib", rejected)
                    .is_err(),
                "{rejected}"
            );
        }
        assert_eq!(
            kernel
                .setting("terminal", "scrollback_budget_mib")
                .as_deref(),
            Some("8")
        );
    }
}

/// REQ: F-013, COR-001
#[test]
fn f_013_parser_pool_default_is_off_with_zero_budget() {
    for mode in MODES {
        let kernel = world(mode);
        assert_eq!(
            kernel
                .setting("terminal", "engine_memory_budget_mib")
                .as_deref(),
            Some("0")
        );
    }
}

/// REQ: F-014, COR-001
#[test]
fn f_014_typed_input_reaches_the_process_unchanged() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "t", &["cat"]);
        assert!(matches!(type_text(kernel, pane, "ls -l"), Outcome::Ok));
        assert_eq!(kernel.pane_input_log(pane), b"ls -l".to_vec());
    });
}

/// REQ: F-014, COR-001
#[test]
fn f_014_paste_is_wrapped_in_bracketed_paste_markers_when_the_pane_enabled_it() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "t", &["cat"]);
        kernel.feed_output(pane, b"\x1b[?2004h");
        let intent = ilium_plugin_api::generated::types::Intent::SendInput(
            ilium_plugin_api::generated::types::SendInputSpec {
                pane,
                text: "a\nb".to_owned(),
                delivery: Delivery::Pasted,
                prompt_epoch: None,
                stages: Vec::new(),
                raw: None,
                mouse: None,
                if_current: None,
                lock_user_input: false,
                journal: false,
            },
        );
        apply(kernel, intent);
        assert_eq!(
            kernel.pane_input_log(pane),
            b"\x1b[200~a\nb\x1b[201~".to_vec()
        );
    });
}

/// REQ: F-014, COR-001
#[test]
fn f_014_input_to_an_unknown_pane_is_refused_not_found() {
    in_both_modes(|kernel| {
        let outcome = type_text(
            kernel,
            ilium_plugin_api::generated::types::NodeId(4242),
            "x",
        );
        assert_eq!(refusal_code(&outcome), Some(ErrorCode::NotFound));
    });
}

/// REQ: F-014, COR-001
#[test]
fn f_014_input_to_an_exited_pane_does_not_reach_a_process() {
    in_both_modes(|kernel| {
        let pane = terminal(kernel, None, "gone", &["sh", "-c", "exit 0"]);
        kernel.settle();
        let outcome = type_text(kernel, pane, "x");
        // primitives-terminal.md gives no code for an exited pane; the intent must still be refused, never ok.
        assert!(is_refused(&outcome), "{outcome:?}");
        assert!(kernel.pane_input_log(pane).is_empty());
    });
}

/// REQ: COR-001
#[test]
fn cor_001_the_plugin_build_has_the_same_tree_as_the_kernel_only_build_after_one_scenario() {
    let with_plugins = {
        let kernel = world(StartMode::Normal);
        let root = project(&kernel, "api");
        group(&kernel, Some(root), "g");
        titles(&kernel)
    };
    let kernel_only = {
        let kernel = world(StartMode::NoPlugins);
        let root = project(&kernel, "api");
        group(&kernel, Some(root), "g");
        titles(&kernel)
    };
    assert_eq!(with_plugins, kernel_only);
}

/// REQ: F-001, COR-001
#[test]
fn f_001_tree_order_accepts_the_six_documented_values_and_nothing_else() {
    in_both_modes(|kernel| {
        for value in [
            "manual",
            "type",
            "age_ascending",
            "age_descending",
            "name_ascending",
            "name_descending",
        ] {
            assert_eq!(
                kernel.apply_setting("ui", "tree_order", value),
                Ok(()),
                "{value}"
            );
        }
        let error = kernel
            .apply_setting("ui", "tree_order", "random")
            .expect_err("random is not a documented value");
        assert!(error.contains("invalid-argument"), "{error}");
    });
}

/// REQ: F-003, COR-001
#[test]
fn f_003_left_panel_sizing_mode_accepts_the_three_documented_values() {
    in_both_modes(|kernel| {
        for value in ["fixed", "focus_dependent", "terminal_width_dependent"] {
            assert_eq!(
                kernel.apply_setting("ui", "left_panel_sizing_mode", value),
                Ok(()),
                "{value}"
            );
        }
        assert!(kernel
            .apply_setting("ui", "left_panel_sizing_mode", "floating")
            .is_err());
    });
}

/// REQ: F-003, COR-001
#[test]
fn f_003_left_panel_fixed_width_boundaries_are_16_and_80() {
    in_both_modes(|kernel| {
        for value in ["16", "17", "79", "80"] {
            assert_eq!(
                kernel.apply_setting("ui", "left_panel_fixed_width", value),
                Ok(()),
                "{value}"
            );
        }
        for value in ["15", "81"] {
            assert!(
                kernel
                    .apply_setting("ui", "left_panel_fixed_width", value)
                    .is_err(),
                "{value}"
            );
        }
    });
}
