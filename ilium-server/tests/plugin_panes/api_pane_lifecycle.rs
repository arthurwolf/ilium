//! Pane process lifecycle intents (api-pane-lifecycle.md): PAN-001..007, 020..026, 032.

use ilium_plugin_api::generated::types::{
    ErrorCode, ExternalTarget, FreezePaneSpec, Intent, MoveNodeSpec, OpenExternalSpec, Outcome, PaneLifecycleState,
    Placement, RenameNodeSpec, ReplacePaneCommandSpec, ResizePaneSpec, RestructureOp, RestructureSpec,
    SetInferredIconSpec, TerminatePaneProcessSpec, TitleSource, UnfreezePaneSpec,
};
use proptest::prelude::*;

use crate::common::*;

const LIFECYCLE: &[&str] = &["panes.lifecycle", "panes.read"];
const TREE: &[&str] = &["panes.lifecycle", "tree.write", "tree.restructure", "tree.metadata", "tree.decorate", "ui.chrome"];

fn freeze(pane: ilium_plugin_api::generated::types::NodeId) -> Intent {
    Intent::FreezePane(FreezePaneSpec { pane })
}

fn unfreeze(pane: ilium_plugin_api::generated::types::NodeId) -> Intent {
    Intent::UnfreezePane(UnfreezePaneSpec { pane })
}

fn terminate(grace_ms: u32) -> Intent {
    Intent::TerminatePaneProcess(TerminatePaneProcessSpec { pane: RUNNING_PANE, grace_ms })
}

fn rename(node: ilium_plugin_api::generated::types::NodeId, title: &str, revision: Option<u64>) -> RenameNodeSpec {
    RenameNodeSpec { node, title: title.to_owned(), source: TitleSource::Plugin, expected_revision: revision }
}

// PAN-001 / PAN-002

/// REQ: PAN-001, PAN-002
#[test]
fn pan_001_freeze_then_unfreeze_reports_frozen_then_running() {
    let kernel = world(LIFECYCLE);
    assert!(matches!(submit(&kernel, freeze(RUNNING_PANE)), Outcome::Ok));
    assert!(matches!(submit(&kernel, unfreeze(RUNNING_PANE)), Outcome::Ok));
    assert_eq!(states(&kernel, RUNNING_PANE), vec![PaneLifecycleState::Frozen, PaneLifecycleState::Running]);
}

/// REQ: PAN-001
#[test]
fn pan_001_every_event_names_the_plugin_that_caused_the_change() {
    let kernel = world(LIFECYCLE);
    submit(&kernel, freeze(RUNNING_PANE));
    assert_eq!(kernel.lifecycle_event_causes(RUNNING_PANE), vec![Some(PLUGIN.to_owned())]);
}

/// REQ: PAN-002
#[test]
fn pan_002_unfreeze_a_running_pane_refused_conflict() {
    let kernel = world(LIFECYCLE);
    assert_eq!(refusal(&submit(&kernel, unfreeze(RUNNING_PANE))), Some(ErrorCode::Conflict));
}

/// REQ: PAN-002
#[test]
fn pan_002_freeze_a_frozen_pane_leaves_it_frozen_with_no_second_event() {
    let kernel = world(LIFECYCLE);
    submit(&kernel, freeze(RUNNING_PANE));
    let _ = submit(&kernel, freeze(RUNNING_PANE));
    assert_eq!(states(&kernel, RUNNING_PANE), vec![PaneLifecycleState::Frozen]);
}

/// REQ: PAN-002
#[test]
fn pan_002_freeze_an_unknown_pane_refused_not_found() {
    let kernel = world(LIFECYCLE);
    assert_eq!(refusal(&submit(&kernel, freeze(ilium_plugin_api::generated::types::NodeId(999)))), Some(ErrorCode::NotFound));
}

// PAN-003

/// REQ: PAN-003
#[cfg(windows)]
#[test]
fn pan_003_freeze_on_windows_refused_unsupported() {
    let kernel = world(LIFECYCLE);
    assert_eq!(refusal(&submit(&kernel, freeze(RUNNING_PANE))), Some(ErrorCode::Unsupported));
}

/// REQ: PAN-003
#[cfg(not(windows))]
#[test]
fn pan_003_freeze_with_the_windows_platform_switch_refused_unsupported() {
    let kernel = world(LIFECYCLE);
    kernel.pretend_platform_windows();
    assert_eq!(refusal(&submit(&kernel, freeze(RUNNING_PANE))), Some(ErrorCode::Unsupported));
}

// PAN-004 / PAN-007

fn replace(pane: ilium_plugin_api::generated::types::NodeId, cwd: Option<&str>) -> Intent {
    Intent::ReplacePaneCommand(ReplacePaneCommandSpec {
        pane,
        command: vec!["/bin/sh".to_owned()],
        cwd: cwd.map(|path| ilium_plugin_api::generated::types::FsPath { scope: "project".into(), path: path.into() }),
        keep_scrollback: false,
    })
}

/// REQ: PAN-004
#[test]
fn pan_004_replace_command_reports_restarting_then_running_and_keeps_the_node() {
    let kernel = world(LIFECYCLE);
    let before = kernel.pane_identity(RUNNING_PANE);
    assert!(matches!(submit(&kernel, replace(RUNNING_PANE, None)), Outcome::Ok));
    assert_eq!(states(&kernel, RUNNING_PANE), vec![PaneLifecycleState::Restarting, PaneLifecycleState::Running]);
    assert_eq!(kernel.pane_identity(RUNNING_PANE), before, "id, title, place and size must be kept");
}

/// REQ: PAN-004
#[test]
fn pan_004_cwd_none_keeps_the_pane_directory() {
    let kernel = world(LIFECYCLE);
    let before = kernel.pane_cwd(RUNNING_PANE);
    submit(&kernel, replace(RUNNING_PANE, None));
    assert_eq!(kernel.pane_cwd(RUNNING_PANE), before);
}

/// REQ: PAN-007
#[test]
fn pan_007_replace_command_of_an_exited_pane_restarts_it() {
    let kernel = world(LIFECYCLE);
    assert!(matches!(submit(&kernel, replace(EXITED_PANE, None)), Outcome::Ok));
    assert_eq!(states(&kernel, EXITED_PANE), vec![PaneLifecycleState::Restarting, PaneLifecycleState::Running]);
}

// PAN-005 / PAN-006

/// REQ: PAN-005
#[test]
fn pan_005_terminate_ends_the_process_and_keeps_the_pane_open() {
    let kernel = world(LIFECYCLE);
    assert!(matches!(submit(&kernel, terminate(3_000)), Outcome::Ok));
    assert_eq!(states(&kernel, RUNNING_PANE).last(), Some(&PaneLifecycleState::Exited));
    assert!(kernel.node_exists(RUNNING_PANE));
    assert!(kernel.pane_shows_exit_status(RUNNING_PANE));
}

/// REQ: PAN-005
#[test]
fn pan_005_a_process_that_ignores_the_request_is_killed_after_the_grace() {
    let kernel = world(LIFECYCLE);
    kernel.make_process_ignore_terminate(RUNNING_PANE);
    submit(&kernel, terminate(200));
    assert!(kernel.kill_was_sent_after_ms(RUNNING_PANE) >= 200);
}

/// REQ: PAN-006
#[test]
fn pan_006_grace_10000_accepted() {
    let kernel = world(LIFECYCLE);
    assert!(matches!(submit(&kernel, terminate(10_000)), Outcome::Ok));
}

/// REQ: PAN-006
#[test]
fn pan_006_grace_10001_refused_invalid_argument() {
    let kernel = world(LIFECYCLE);
    assert_eq!(refusal(&submit(&kernel, terminate(10_001))), Some(ErrorCode::InvalidArgument));
    assert!(states(&kernel, RUNNING_PANE).is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    // REQ: PAN-006
    #[test]
    fn pan_006_grace_is_valid_iff_at_most_10000(grace in 0u32..20_000) {
        let kernel = world(LIFECYCLE);
        let refused = refusal(&submit(&kernel, terminate(grace))) == Some(ErrorCode::InvalidArgument);
        prop_assert_eq!(refused, grace > 10_000);
    }
}

// PAN-020

/// REQ: PAN-020
#[test]
fn pan_020_resize_beyond_the_neighbour_minimum_refused_conflict_naming_the_neighbour() {
    let kernel = world(TREE);
    let outcome = submit(&kernel, Intent::ResizePane(ResizePaneSpec { pane: RUNNING_PANE, columns: Some(500), rows: None }));
    assert_eq!(refusal(&outcome), Some(ErrorCode::Conflict));
    assert!(message(&outcome).contains(&NEIGHBOUR_PANE.0.to_string()), "{}", message(&outcome));
}

/// REQ: PAN-020
#[test]
fn pan_020_resize_within_the_layout_accepted() {
    let kernel = world(TREE);
    let outcome = submit(&kernel, Intent::ResizePane(ResizePaneSpec { pane: RUNNING_PANE, columns: Some(30), rows: None }));
    assert!(matches!(outcome, Outcome::Ok));
}

// PAN-021 / PAN-022 / PAN-023

fn rename_ops(count: u64) -> Vec<RestructureOp> {
    (0..count).map(|index| RestructureOp::Rename(rename(RUNNING_PANE, &format!("t{index}"), None))).collect()
}

fn restructure(operations: Vec<RestructureOp>) -> Intent {
    Intent::Restructure(RestructureSpec { operations, label: "test".to_owned() })
}

/// REQ: PAN-021
#[test]
fn pan_021_256_operations_accepted() {
    let kernel = world(TREE);
    assert!(!matches!(refusal(&submit(&kernel, restructure(rename_ops(256)))), Some(ErrorCode::InvalidArgument)));
}

/// REQ: PAN-021
#[test]
fn pan_021_257_operations_refused_invalid_argument() {
    let kernel = world(TREE);
    assert_eq!(refusal(&submit(&kernel, restructure(rename_ops(257)))), Some(ErrorCode::InvalidArgument));
}

/// REQ: PAN-022
#[test]
fn pan_022_failing_operation_3_of_5_refused_conflict_naming_index_3_and_tree_unchanged() {
    let kernel = world(TREE);
    let before = kernel.tree_fingerprint();
    let mut ops = rename_ops(5);
    ops[2] = RestructureOp::Move(MoveNodeSpec { node: RUNNING_PANE, target: RUNNING_PANE, placement: Placement::Before });
    let outcome = submit(&kernel, restructure(ops));
    assert_eq!(refusal(&outcome), Some(ErrorCode::Conflict));
    assert!(message(&outcome).contains('3') || message(&outcome).contains('2'), "{}", message(&outcome));
    assert_eq!(kernel.tree_fingerprint(), before);
}

/// REQ: PAN-023
#[test]
fn pan_023_revert_after_a_later_change_refused_conflict() {
    let kernel = world(TREE);
    let outcome = submit(&kernel, restructure(rename_ops(1)));
    let token = kernel.last_restructure_token(&outcome);
    submit(&kernel, Intent::RenameNode(rename(RUNNING_PANE, "later", None)));
    assert_eq!(refusal(&submit(&kernel, Intent::RevertRestructure(token))), Some(ErrorCode::Conflict));
}

/// REQ: PAN-023
#[test]
fn pan_023_revert_with_nothing_touched_restores_the_tree() {
    let kernel = world(TREE);
    let before = kernel.tree_fingerprint();
    let outcome = submit(&kernel, restructure(rename_ops(1)));
    let token = kernel.last_restructure_token(&outcome);
    assert!(matches!(submit(&kernel, Intent::RevertRestructure(token)), Outcome::Ok));
    assert_eq!(kernel.tree_fingerprint(), before);
}

// PAN-024 / PAN-025 / PAN-026

/// REQ: PAN-024
#[test]
fn pan_024_rename_with_a_stale_revision_refused_conflict() {
    let kernel = world(TREE);
    let current = kernel.node_revision(RUNNING_PANE);
    let outcome = submit(&kernel, Intent::RenameNode(rename(RUNNING_PANE, "x", Some(current.wrapping_sub(1)))));
    assert_eq!(refusal(&outcome), Some(ErrorCode::Conflict));
}

/// REQ: PAN-024
#[test]
fn pan_024_rename_with_the_current_revision_accepted() {
    let kernel = world(TREE);
    let current = kernel.node_revision(RUNNING_PANE);
    assert!(matches!(submit(&kernel, Intent::RenameNode(rename(RUNNING_PANE, "x", Some(current)))), Outcome::Ok));
}

/// REQ: PAN-025
#[test]
fn pan_025_inferred_icon_over_a_manual_icon_refused_conflict() {
    let kernel = world(TREE);
    kernel.set_manual_icon(RUNNING_PANE, "star");
    let outcome = submit(&kernel, Intent::SetInferredIcon(SetInferredIconSpec { node: RUNNING_PANE, icon: Some("bolt".into()), source: "test".into() }));
    assert_eq!(refusal(&outcome), Some(ErrorCode::Conflict));
}

/// REQ: PAN-026
#[test]
fn pan_026_open_external_file_scheme_refused_not_permitted() {
    let kernel = world(TREE);
    let outcome = submit(&kernel, Intent::OpenExternal(OpenExternalSpec { target: ExternalTarget::Url("file:///etc/passwd".into()) }));
    assert_eq!(refusal(&outcome), Some(ErrorCode::NotPermitted));
}

/// REQ: PAN-026
#[test]
fn pan_026_http_https_and_mailto_accepted() {
    let kernel = world(TREE);
    for url in ["http://example.org", "https://example.org", "mailto:a@example.org"] {
        let outcome = submit(&kernel, Intent::OpenExternal(OpenExternalSpec { target: ExternalTarget::Url(url.into()) }));
        assert_ne!(refusal(&outcome), Some(ErrorCode::NotPermitted), "{url}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    // REQ: PAN-026
    #[test]
    fn pan_026_other_schemes_are_refused(scheme in "[a-z]{2,8}") {
        prop_assume!(!["http", "https", "mailto"].contains(&scheme.as_str()));
        let kernel = world(TREE);
        let outcome = submit(&kernel, Intent::OpenExternal(OpenExternalSpec { target: ExternalTarget::Url(format!("{scheme}://x")) }));
        prop_assert_eq!(refusal(&outcome), Some(ErrorCode::NotPermitted));
    }
}

// PAN-032

/// REQ: PAN-032
#[test]
fn pan_032_freeze_writes_a_log_line_naming_the_intent_and_the_pane() {
    let kernel = world(LIFECYCLE);
    submit(&kernel, freeze(RUNNING_PANE));
    let lines = kernel.plugin_log(PLUGIN);
    assert!(lines.iter().any(|line| line.contains("freeze-pane") && line.contains(&RUNNING_PANE.0.to_string())), "{lines:?}");
}

/// REQ: PAN-032
#[test]
fn pan_032_panes_lifecycle_is_a_high_risk_grant() {
    assert!(ilium_plugin_api::grants::is_high_risk("panes.lifecycle"));
}

/// REQ: PAN-032
#[test]
fn pan_032_a_refused_use_is_logged_too() {
    let kernel = world(LIFECYCLE);
    submit(&kernel, unfreeze(RUNNING_PANE));
    assert!(kernel.plugin_log(PLUGIN).iter().any(|line| line.contains("unfreeze-pane")));
}

/// REQ: PAN-032
#[test]
fn pan_032_without_the_grant_freeze_refused_not_permitted() {
    let kernel = world(&["panes.read"]);
    assert_eq!(refusal(&submit(&kernel, freeze(RUNNING_PANE))), Some(ErrorCode::NotPermitted));
}
