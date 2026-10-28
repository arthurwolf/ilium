//! Rule statements of the guide-porting-from-other-plugin-systems page as the manual states them (doc-lint tests, part 2).
use crate::support::{assert_page_line, assert_rule};

/// REQ: AUT-701 WARN
#[test]
fn doc_aut_701_pane_logic_on_s_drawing_on_c_with_messages() {
    assert_rule(
        "AUT-701",
        "SHOULD",
        &[
            "SHOULD put logic about panes and agents on S and drawing on C",
            "SHOULD send messages between them rather than share state",
        ],
    );
}

/// REQ: AUT-702
#[test]
fn doc_aut_702_every_call_grant_declared_import_ungranted_reported() {
    assert_rule(
        "AUT-702",
        "MUST",
        &[
            "MUST declare every grant its calls need",
            "`component.import-ungranted`",
        ],
    );
}

/// REQ: AUT-703
#[test]
fn doc_aut_703_waiting_loop_rewritten_as_request_completed_event() {
    assert_rule(
        "AUT-703",
        "MUST",
        &["MUST be rewritten as events", "`request-completed`"],
    );
}

/// REQ: AUT-704 WARN
#[test]
fn doc_aut_704_typed_primitive_preferred_over_program() {
    assert_rule(
        "AUT-704",
        "SHOULD",
        &[
            "SHOULD prefer the typed primitive (`terminal.read-screen`) over a program",
            "needs a weaker grant",
        ],
    );
}

/// REQ: AUT-705
#[test]
fn doc_aut_705_write_grant_not_wider_than_written_folder() {
    assert_rule(
        "AUT-705",
        "MUST NOT",
        &["MUST NOT ask for a write grant wider than the folder it writes to"],
    );
}

/// REQ: AUT-706 WARN
#[test]
fn doc_aut_706_port_is_new_plugin_with_new_id_and_distinct_name() {
    assert_rule(
        "AUT-706",
        "SHOULD NOT",
        &[
            "SHOULD be written as a new plugin with a new id",
            "SHOULD NOT reuse the name of the original",
        ],
    );
}

/// REQ: AUT-701
#[test]
fn doc_aut_701_port_points_to_the_patterns_cookbook_for_messages() {
    assert_page_line(
        "AUT-701",
        &["See [Patterns cookbook](/pages/guide-patterns-cookbook/) for the message pattern."],
    );
}

/// REQ: AUT-702
#[test]
fn doc_aut_702_ungranted_import_is_reported_by_check() {
    assert_page_line(
        "AUT-702",
        &["reports an import without a grant as `component.import-ungranted`"],
    );
}

/// REQ: AUT-703
#[test]
fn doc_aut_703_no_eval_no_shared_state_no_blocking_wait() {
    assert_page_line(
        "AUT-703",
        &["There is no `eval`, no global state shared between plugins and no synchronous block while a job runs."],
    );
}

/// REQ: AUT-704
#[test]
fn doc_aut_704_tmux_wrapper_often_needs_a_process_run_grant() {
    assert_page_line(
        "AUT-704",
        &["A tmux plugin that only wraps shell commands often becomes a plugin with a `process.run:<program>` grant."],
    );
}

/// REQ: AUT-705
#[test]
fn doc_aut_705_write_goes_to_a_granted_scope_not_an_arbitrary_path() {
    assert_page_line(
        "AUT-705",
        &["The plugin writes the file to a granted scope, not to an arbitrary path."],
    );
}

/// REQ: AUT-706
#[test]
fn doc_aut_706_new_name_must_not_suggest_the_same_product() {
    assert_page_line("AUT-706", &["suggests it is the same product"]);
}
