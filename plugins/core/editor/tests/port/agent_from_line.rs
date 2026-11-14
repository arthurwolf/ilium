//! Twins of ilium-client/src/agent_from_line.rs: the prompt a TODO-line click hands to a new
//! terminal pane (F-023, F-114). Dialog layout and provider-button tests are covered by the
//! kernel-owned dialog (see skipped list).

use crate::helpers::{click_event, plain_host};
use ilium_plugin_sdk::generated::types::Intent;

fn prompt_for(file: &str, text: &str, line: u32) -> String {
    let mut host = plain_host(file, Some("rust"), text, &[]);
    let submissions = host.deliver_one(click_event(line, 1));
    assert_eq!(submissions.len(), 1, "one create-pane submission");
    let Intent::CreatePane(spec) = &submissions[0].intent else {
        return String::new();
    };
    spec.initial_input.clone().unwrap_or_default()
}

/// REQ: PORT:ilium-client/src/agent_from_line.rs::default_prompt_contains_line_file_and_one_based_line_number
#[test]
fn port_agent_from_line_prompt_contains_line_file_and_one_based_line_number() {
    let prompt = prompt_for("src/main.rs", "fn a() {}\nTODO fix_this();\n", 1);
    assert!(prompt.contains("TODO fix_this();"), "{prompt}");
    assert!(prompt.contains("src/main.rs"), "{prompt}");
    assert!(prompt.contains("at line 2"), "{prompt}");
}

/// REQ: PORT:ilium-client/src/agent_from_line.rs::default_prompt_preserves_source_text_without_template_or_html_interpretation
#[test]
fn port_agent_from_line_prompt_preserves_source_text_verbatim() {
    let line = "TODO {{untrusted}} <tag> & \"quoted\"";
    let prompt = prompt_for("src/main.rs", &format!("{line}\n"), 0);
    assert!(prompt.contains(line), "{prompt}");
    assert!(
        !prompt.contains("&amp;") && !prompt.contains("&lt;"),
        "{prompt}"
    );
}

/// REQ: PORT:ilium-client/src/mouse.rs::clicking_create_submits_the_dialog_prompt
#[test]
fn port_mouse_clicking_a_todo_line_submits_a_claude_pane_with_the_line_as_its_prompt() {
    let mut host = plain_host("src/main.rs", Some("rust"), "TODO do_work();\n", &[]);
    let submissions = host.deliver_one(click_event(0, 1));
    assert_eq!(submissions.len(), 1, "exactly one create-pane submission");
    let Intent::CreatePane(spec) = &submissions[0].intent else {
        panic!("expected create-pane, got {:?}", submissions[0].intent)
    };
    assert_eq!(spec.command, Some(vec!["claude".to_string()]));
    let prompt = spec
        .initial_input
        .clone()
        .expect("the prompt is handed over as initial input");
    assert!(prompt.contains("do_work();"), "{prompt}");
}

/// REQ: PORT:ilium-client/src/agent_from_line.rs::default_prompt_contains_line_file_and_one_based_line_number
#[test]
fn port_agent_from_line_prompt_names_the_third_line_as_line_three() {
    let prompt = prompt_for("lib/util.rs", "a\nb\nTODO third();\n", 2);
    assert!(prompt.contains("TODO third();"), "{prompt}");
    assert!(prompt.contains("lib/util.rs"), "{prompt}");
    assert!(prompt.contains("at line 3"), "{prompt}");
}

/// REQ: PORT:ilium-client/src/agent_from_line.rs::default_prompt_preserves_source_text_without_template_or_html_interpretation
#[test]
fn port_agent_from_line_prompt_keeps_backticks_and_shell_text_unchanged() {
    let line = "TODO `rm -rf $HOME` && echo '$(id)'";
    let prompt = prompt_for("src/main.rs", &format!("{line}\n"), 0);
    assert!(prompt.contains(line), "{prompt}");
}

/// REQ: PORT:ilium-client/src/mouse.rs::clicking_create_submits_the_dialog_prompt
#[test]
fn port_mouse_the_submitted_prompt_names_the_clicked_file() {
    let mut host = plain_host("src/main.rs", Some("rust"), "TODO do_work();\n", &[]);
    let submissions = host.deliver_one(click_event(0, 1));
    let Intent::CreatePane(spec) = &submissions[0].intent else {
        panic!("expected create-pane, got {:?}", submissions[0].intent)
    };
    let prompt = spec.initial_input.clone().expect("initial input");
    assert!(prompt.contains("src/main.rs"), "{prompt}");
}

// ---- the "Create agent from line" dialog (kernel line menu command, popup owned by the plugin)

use ilium_plugin_core_editor::agent_dialog;
use ilium_plugin_core_editor::EditorClient;
use ilium_plugin_sdk::generated::types::{
    ActionKind, ActionValue, CommandContext, CommandInvokedEvent, CommandOrigin, Event as SdkEvent,
    NodeId, Submission, UiAction, Widget,
};
use ilium_plugin_sdk::{Handler, Views};

fn invoke(editor: &mut EditorClient, arguments: &str) -> Vec<Submission> {
    Handler::on_events(
        editor,
        vec![SdkEvent::CommandInvoked(CommandInvokedEvent {
            command: agent_dialog::COMMAND.to_string(),
            arguments_json: arguments.to_string(),
            origin: CommandOrigin::Menu,
            context: CommandContext {
                pane: Some(NodeId(7)),
                view: None,
                buffer: None,
                line: None,
                virtual_row: None,
                menu_item: None,
            },
        })],
    )
}

fn act(
    editor: &mut EditorClient,
    key: &str,
    action_id: &str,
    kind: ActionKind,
    value: Option<ActionValue>,
) -> Vec<Submission> {
    Handler::on_events(
        editor,
        vec![SdkEvent::UiAction(UiAction {
            view: agent_dialog::view(),
            key: key.to_string(),
            action_id: action_id.to_string(),
            kind,
            value,
        })],
    )
}

const ARGUMENTS: &str = r#"{"path":"src/main.rs","line":2,"text":"TODO fix_this();","parent":5}"#;

/// REQ: PORT:ilium-client/src/agent_from_line.rs::agent_registry_steps_through_every_provider_and_wraps, F-023
#[test]
fn port_agent_from_line_dialog_offers_every_provider_in_order_and_starts_the_chosen_one() {
    let mut editor = crate::support::direct("{}");
    let opened = invoke(&mut editor, ARGUMENTS);
    assert!(
        matches!(opened.as_slice(), [Submission { intent: Intent::OpenView(spec), .. }] if spec.view == agent_dialog::view()),
        "the command opens the popup"
    );
    let tree = Views::render(
        &mut editor,
        agent_dialog::view(),
        ilium_plugin_sdk::generated::types::Area {
            columns: 80,
            rows: 24,
        },
    );
    let Some(Widget::Choice(choice)) = tree
        .nodes
        .iter()
        .map(|node| &node.widget)
        .find(|widget| matches!(widget, Widget::Choice(_)))
    else {
        panic!("the dialog has the agent choice");
    };
    assert_eq!(choice.options, ["Claude", "Codex", "Antigravity"]);
    for (index, command) in [(0u32, "claude"), (1, "codex"), (2, "agy")] {
        let mut editor = crate::support::direct("{}");
        invoke(&mut editor, ARGUMENTS);
        act(
            &mut editor,
            agent_dialog::AGENT_KEY,
            "",
            ActionKind::Changed,
            Some(ActionValue::Index(index)),
        );
        let created = act(&mut editor, "dialog", "create", ActionKind::Activated, None);
        let Some(Intent::CreatePane(spec)) = created
            .iter()
            .map(|s| &s.intent)
            .find(|i| matches!(i, Intent::CreatePane(_)))
        else {
            panic!("create submits a pane");
        };
        assert_eq!(spec.command, Some(vec![command.to_string()]));
        assert_eq!(spec.parent, Some(NodeId(5)));
        assert_eq!(spec.content_kind, "ilium.terminal");
        let prompt = spec.initial_input.clone().unwrap_or_default();
        assert!(
            prompt.contains("TODO fix_this();") && prompt.contains("at line 2"),
            "{prompt}"
        );
    }
}

/// REQ: PORT:ilium-client/src/agent_from_line.rs::provider_control_paints_and_hits_exact_buttons_on_responsive_rows, PORT:ilium-client/src/agent_from_line.rs::dialog_layout_stays_inside_small_screens, F-023
#[test]
fn port_agent_from_line_dialog_is_a_popup_the_host_lays_out_with_create_and_cancel_buttons() {
    let mut editor = crate::support::direct("{}");
    invoke(&mut editor, ARGUMENTS);
    let tree = Views::render(
        &mut editor,
        agent_dialog::view(),
        ilium_plugin_sdk::generated::types::Area {
            columns: 20,
            rows: 6,
        },
    );
    let Widget::Dialog(dialog) = &tree.nodes[tree.root as usize].widget else {
        panic!("the root is the dialog");
    };
    let ids: Vec<_> = dialog
        .buttons
        .iter()
        .map(|button| button.id.as_str())
        .collect();
    assert_eq!(ids, ["create", "cancel"]);
}

/// REQ: PORT:ilium-client/src/agent_from_line.rs::agent_registry_steps_through_every_provider_and_wraps, F-023
#[test]
fn port_agent_from_line_cancel_and_a_bad_command_create_no_pane() {
    let mut editor = crate::support::direct("{}");
    invoke(&mut editor, ARGUMENTS);
    let cancelled = act(&mut editor, "dialog", "cancel", ActionKind::Activated, None);
    assert!(cancelled
        .iter()
        .all(|submission| !matches!(submission.intent, Intent::CreatePane(_))));
    let refused = invoke(&mut editor, "{}");
    assert!(matches!(
        refused.as_slice(),
        [Submission {
            intent: Intent::Toast(_),
            ..
        }]
    ));
}
