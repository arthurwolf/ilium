//! In-memory state contracts only; these do not qualify real process/device behavior.
use ilium_plugin_api::generated::types::{
    CreatePaneSpec, Delivery, EditorApplyEditsSpec, Event, Intent, NodeClass, NodeId, Outcome,
    RenameNodeSpec, SendInputSpec, Submission, TextEdit, TextPosition, TextRange, TitleSource,
};
use ilium_plugin_api::kernel::{KernelFacade, Origin, PaneRead, TreeNode, TreeRead};
use ilium_plugin_api::{ErrorCode, ProcessKind};
use ilium_plugin_host::testing::{FakeKernel, FakePaneScreen};
fn node(id: u64, parent: Option<u64>, class: NodeClass) -> TreeNode {
    TreeNode {
        id: NodeId(id),
        parent: parent.map(NodeId),
        class,
        content_kind: None,
        title: format!("node {id}"),
        cwd: None,
        agent: None,
        locked: false,
        revision: 0,
    }
}
fn kernel() -> FakeKernel {
    let kernel = FakeKernel::new(ProcessKind::Server);
    kernel.set_tree(vec![
        node(1, None, NodeClass::Container),
        node(2, Some(1), NodeClass::Pane),
    ]);
    kernel
}
fn rename(id: u64, title: &str) -> Intent {
    Intent::RenameNode(RenameNodeSpec {
        node: NodeId(id),
        title: title.into(),
        source: TitleSource::Plugin,
        expected_revision: None,
    })
}
fn send() -> Intent {
    Intent::SendInput(SendInputSpec {
        pane: NodeId(2),
        text: "hello".into(),
        delivery: Delivery::Submitted,
        prompt_epoch: None,
        stages: vec![],
        raw: None,
        mouse: None,
        if_current: None,
        lock_user_input: false,
        journal: true,
    })
}
#[test]
fn tree_mutations_update_reads_epoch_and_removal_closes_subtree() {
    let kernel = kernel();
    let epoch = kernel.epoch();
    assert_eq!(
        kernel.apply_intent(&Origin::User, rename(2, "changed")),
        Outcome::Ok
    );
    assert_eq!(kernel.node(NodeId(2)).unwrap().title, "changed");
    assert!(kernel.epoch() > epoch);
    kernel.remove_node(NodeId(1));
    assert!(kernel.nodes().is_empty());
    assert!(
        matches!(kernel.apply_intent(&Origin::User,rename(2,"gone")),Outcome::Failed(error) if error.code == ErrorCode::NotFound)
    );
    assert!(kernel
        .take_events()
        .iter()
        .any(|event| matches!(event, Event::NodeRemoved(NodeId(2)))));
}
#[test]
fn screen_history_and_cells_are_derived_from_configured_content() {
    let kernel = kernel();
    kernel.set_screen(
        NodeId(2),
        FakePaneScreen {
            rows: vec!["αb".into(), "second".into()],
            history: vec!["old".into(), "new".into()],
        },
    );
    assert_eq!(kernel.screen_text(NodeId(2)).as_deref(), Some("αb\nsecond"));
    assert_eq!(kernel.cells(NodeId(2), 0..1).unwrap()[0][0].text, "α");
    assert_eq!(kernel.history(NodeId(2), 1), Some(vec!["new".into()]));
    assert_eq!(kernel.history(NodeId(99), 1), None);
}
#[test]
fn edits_validate_all_ranges_before_mutating_and_respect_revision() {
    let kernel = kernel();
    let buffer = kernel.attach_buffer(NodeId(2), "abc\ndef");
    let edit = |line, column, replacement: &str| TextEdit {
        range: TextRange {
            start: TextPosition { line, column },
            end: TextPosition {
                line,
                column: column + 1,
            },
        },
        replacement: replacement.into(),
    };
    let invalid = Intent::EditorApplyEdits(EditorApplyEditsSpec {
        buffer,
        base_revision: 0,
        edits: vec![edit(0, 0, "A"), edit(9, 0, "B")],
    });
    assert!(
        matches!(kernel.apply_intent(&Origin::User,invalid),Outcome::Refused(error) if error.code == ErrorCode::InvalidArgument)
    );
    assert_eq!(kernel.state().buffers[&buffer], "abc\ndef");
    assert_eq!(
        kernel.apply_intent(
            &Origin::User,
            Intent::EditorApplyEdits(EditorApplyEditsSpec {
                buffer,
                base_revision: 0,
                edits: vec![edit(0, 0, "A")]
            })
        ),
        Outcome::Ok
    );
    assert_eq!(kernel.state().buffers[&buffer], "Abc\ndef");
    assert!(kernel.pane_is_dirty(NodeId(2)));
}
#[test]
fn journal_interruption_records_uncertainty_once_without_writing_or_replay() {
    let kernel = kernel();
    kernel.interrupt_after_journal(NodeId(2), "operation");
    kernel.apply_submission(
        &Origin::User,
        Submission {
            intent: send(),
            correlation: 1,
            dedup_key: Some("operation".into()),
        },
    );
    assert_eq!(kernel.step_log(), vec!["journal:operation"]);
    assert_eq!(kernel.raw_bytes_sent(), 0);
    kernel.recover_journaled_inputs();
    kernel.recover_journaled_inputs();
    assert_eq!(
        kernel
            .take_events()
            .iter()
            .filter(|event| matches!(event, Event::InputUncertain(_)))
            .count(),
        1
    );
    assert_eq!(kernel.raw_bytes_sent(), 0);
}
#[test]
fn prompt_tracking_handles_unicode_backspace_and_untrusted_automation() {
    let kernel = kernel();
    kernel.user_types_into(NodeId(2), "αb".as_bytes());
    kernel.user_types_into(NodeId(2), b"\x7f\r");
    assert_eq!(
        kernel
            .state()
            .last_prompts
            .get(&NodeId(2))
            .map(String::as_str),
        Some("α")
    );
    kernel.user_types_into(NodeId(2), b"user");
    kernel.automation_writes_into(NodeId(2), b"hidden");
    kernel.user_types_into(NodeId(2), b"\r");
    assert!(!kernel.state().last_prompts.contains_key(&NodeId(2)));
}
#[test]
fn created_nodes_use_fresh_ids_and_service_calls_have_no_fabricated_answers() {
    let kernel = kernel();
    let created = kernel.apply_intent(
        &Origin::User,
        Intent::CreatePane(CreatePaneSpec {
            parent: Some(NodeId(1)),
            title: "new".into(),
            content_kind: "ilium.terminal".into(),
            ..CreatePaneSpec::default()
        }),
    );
    assert_eq!(created, Outcome::OkNode(NodeId(3)));
    let first = kernel.start_http_call("GET", "/one");
    let second = kernel.start_http_call("POST", "/two");
    assert_ne!(first, second);
    assert!(kernel.http_response(first).is_none());
    assert_eq!(
        kernel.state().http_calls[&second],
        ("POST".into(), "/two".into())
    );
    let invocation = kernel.start_cli_invocation("command", "json");
    assert_eq!(kernel.cli_exit_code(invocation), None);
    assert!(kernel.cli_stdout_text(invocation).is_empty());
}

#[test]
fn attention_mark_and_ack_share_the_documented_per_node_rate_limit() {
    use ilium_plugin_api::generated::types::MarkAttentionSpec;
    let kernel = kernel();
    for index in 0..10 {
        let intent = if index % 2 == 0 {
            Intent::MarkAttention(MarkAttentionSpec {
                node: NodeId(2),
                reason: "needs attention".into(),
            })
        } else {
            Intent::AckAttention(NodeId(2))
        };
        assert_eq!(kernel.apply_intent(&Origin::User, intent), Outcome::Ok);
    }
    assert!(
        matches!(kernel.apply_intent(&Origin::User, Intent::AckAttention(NodeId(2))), Outcome::Refused(error) if error.code == ErrorCode::RateLimited)
    );
    kernel.state().clock += std::time::Duration::from_secs(1);
    assert_eq!(
        kernel.apply_intent(&Origin::User, Intent::AckAttention(NodeId(2))),
        Outcome::Ok
    );
}
