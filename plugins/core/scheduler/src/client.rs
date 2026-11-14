//! The client side of the scheduler: the two input dialogs ("Schedule input" and "Queue prompt")
//! and the queue panel. The dialogs hold their draft here and send one message to the server side
//! when they are confirmed; the panel shows the state the server side pushes.

use std::cell::RefCell;

use ilium_plugin_sdk::host;
use ilium_plugin_sdk::host::HostClock;
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, Alignment, ButtonRole, ChoiceSpec, ChoiceStyle, Constraint,
    DialogButton, DialogKind, DialogSpec, DisabledMarker, Event, Intent, MessageSpec,
    MessageTarget, NodeFlags, NodeId, OpenViewSpec, ParagraphSpec, PluginId, PluginSide, Span,
    Style, Submission, TextInputSpec, ToggleSpec, ToggleStyle, UiNode, UiTree, ViewTarget, Widget,
};

use crate::queue::{cycle_delivery, validated_request, Delivery, DELIVERY_CHOICES};
use crate::schedule::{remaining_seconds, Draft};
use crate::server::format_remaining;
use crate::wire::{
    decode_state, pane_from_arguments, pane_payload, EnqueueRequest, ScheduleRequest, Shown,
    CANCEL_SCHEDULE_CHANNEL, CLEAR_QUEUE_CHANNEL, ENQUEUE_CHANNEL, SCHEDULE_CHANNEL, STATE_CHANNEL,
    STATE_REQUEST_CHANNEL,
};
use crate::PLUGIN_ID;

/// Command that opens the "Schedule input" dialog; arguments `{"pane": id}`.
pub const SCHEDULE_COMMAND: &str = "schedule-input";
/// Command that opens the "Queue prompt" dialog; arguments `{"pane": id}`.
pub const QUEUE_COMMAND: &str = "queue-prompt";
/// Command that empties a pane's queue; arguments `{"pane": id}`.
pub const CLEAR_COMMAND: &str = "clear-prompt-queue";
/// Command that drops a pane's pending schedule; arguments `{"pane": id}`.
pub const CANCEL_COMMAND: &str = "cancel-scheduled-input";
/// Popup id of the schedule dialog.
pub const SCHEDULE_POPUP: &str = "schedule-input";
/// Popup id of the queue dialog.
pub const QUEUE_POPUP: &str = "queue-prompt";
/// Panel id of the queue panel.
pub const QUEUE_PANEL: &str = "queue";

/// The schedule dialog: the three duration fields, the text and the Enter flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleDialog {
    pub pane: u64,
    pub draft: Draft,
    pub error: Option<String>,
}

/// The queue dialog: the prompt and how often it is delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueDialog {
    pub pane: u64,
    pub text: String,
    pub choice: Delivery,
    pub times: String,
    pub error: Option<String>,
}

/// What an action did to a dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step<T> {
    Stay,
    Confirm(T),
    Cancel,
}

impl ScheduleDialog {
    pub fn new(pane: u64) -> Self {
        Self {
            pane,
            draft: Draft::default(),
            error: None,
        }
    }

    pub fn act(
        &mut self,
        key: &str,
        action_id: &str,
        kind: ActionKind,
        value: Option<ActionValue>,
    ) -> Step<ScheduleRequest> {
        match (kind, value) {
            (ActionKind::Closed, _) => Step::Cancel,
            (ActionKind::Changed | ActionKind::Submitted, Some(ActionValue::Text(text))) => {
                match key {
                    "hours" => self.draft.hours = text,
                    "minutes" => self.draft.minutes = text,
                    "seconds" => self.draft.seconds = text,
                    "text" => self.draft.text = text,
                    _ => {}
                }
                self.error = None;
                if kind == ActionKind::Submitted {
                    return self.confirm();
                }
                Step::Stay
            }
            (ActionKind::Changed | ActionKind::Toggled, Some(ActionValue::Flag(flag)))
                if key == "send-enter" =>
            {
                self.draft.send_enter = flag;
                self.error = None;
                Step::Stay
            }
            (ActionKind::Activated, _) => match action_id {
                "schedule" => self.confirm(),
                "cancel" => Step::Cancel,
                _ => Step::Stay,
            },
            _ => Step::Stay,
        }
    }

    fn confirm(&mut self) -> Step<ScheduleRequest> {
        match self.draft.validated_request() {
            Ok(request) => Step::Confirm(ScheduleRequest {
                pane: self.pane,
                delay_seconds: request.delay_seconds,
                text: request.text,
                send_enter: request.send_enter,
            }),
            Err(message) => {
                self.error = Some(message);
                Step::Stay
            }
        }
    }

    pub fn render(&self) -> UiTree {
        let fields = vec![
            text_field("hours", "Hours", &self.draft.hours, Some("0")),
            text_field("minutes", "Minutes", &self.draft.minutes, Some("0")),
            text_field("seconds", "Seconds", &self.draft.seconds, Some("30")),
            text_field("text", "Text", &self.draft.text, None),
            node(
                "send-enter",
                Widget::Toggle(ToggleSpec {
                    label: "Send Enter".to_owned(),
                    value: self.draft.send_enter,
                    style: ToggleStyle::Checkbox,
                    tri_state: false,
                    icon_on: None,
                    icon_off: None,
                }),
            ),
        ];
        dialog_tree(
            "Schedule input",
            vec!["Type into this pane after a delay.".to_owned()],
            self.error.clone(),
            vec![
                button("schedule", "Schedule", ButtonRole::Primary, false),
                button("cancel", "Cancel", ButtonRole::Secondary, true),
            ],
            fields,
        )
    }
}

impl QueueDialog {
    pub fn new(pane: u64) -> Self {
        Self {
            pane,
            text: String::new(),
            choice: Delivery::Once,
            times: "2".to_owned(),
            error: None,
        }
    }

    pub fn act(
        &mut self,
        key: &str,
        action_id: &str,
        kind: ActionKind,
        value: Option<ActionValue>,
    ) -> Step<EnqueueRequest> {
        match (kind, value) {
            (ActionKind::Closed, _) => Step::Cancel,
            (ActionKind::Changed | ActionKind::Submitted, Some(ActionValue::Text(text))) => {
                match key {
                    "text" => self.text = text,
                    "times" => self.times = text,
                    _ => {}
                }
                self.error = None;
                Step::Stay
            }
            (ActionKind::Changed | ActionKind::Selected, Some(ActionValue::Index(index)))
                if key == "delivery" =>
            {
                let target = DELIVERY_CHOICES
                    .get(index as usize)
                    .cloned()
                    .unwrap_or_else(|| cycle_delivery(&self.choice, 1));
                self.choice = target;
                self.error = None;
                Step::Stay
            }
            (ActionKind::Activated, _) => match action_id {
                "enqueue" => self.confirm(),
                "cancel" => Step::Cancel,
                _ => Step::Stay,
            },
            _ => Step::Stay,
        }
    }

    fn confirm(&mut self) -> Step<EnqueueRequest> {
        match validated_request(&self.text, &self.choice, &self.times) {
            Ok((text, delivery)) => Step::Confirm(EnqueueRequest {
                pane: self.pane,
                text,
                delivery,
            }),
            Err(message) => {
                self.error = Some(message);
                Step::Stay
            }
        }
    }

    pub fn render(&self) -> UiTree {
        let selected = match self.choice {
            Delivery::Once => 0,
            Delivery::Times { .. } => 1,
            Delivery::Forever => 2,
        };
        let mut fields = vec![
            text_field("text", "Prompt", &self.text, None),
            node(
                "delivery",
                Widget::Choice(ChoiceSpec {
                    label: "Delivery".to_owned(),
                    options: vec![
                        "Once".to_owned(),
                        "Run X times".to_owned(),
                        "Forever".to_owned(),
                    ],
                    selected,
                    style: ChoiceStyle::Stepper,
                    items: Vec::new(),
                    source: None,
                    selected_id: None,
                    filter: false,
                    view_modes: Vec::new(),
                    disabled_marker: DisabledMarker::Reason,
                    allow_custom: false,
                }),
            ),
        ];
        if matches!(self.choice, Delivery::Times { .. }) {
            fields.push(text_field("times", "Runs", &self.times, Some("2")));
        }
        dialog_tree(
            "Queue prompt",
            vec!["Send this prompt each time the agent finishes a turn.".to_owned()],
            self.error.clone(),
            vec![
                button("enqueue", "Enqueue", ButtonRole::Primary, false),
                button("cancel", "Cancel", ButtonRole::Secondary, true),
            ],
            fields,
        )
    }
}

#[derive(Default)]
struct ClientState {
    schedule: Option<ScheduleDialog>,
    queue: Option<QueueDialog>,
    shown: Shown,
}

thread_local! {
    static STATE: RefCell<ClientState> = RefCell::new(ClientState::default());
}

fn with_state<T>(body: impl FnOnce(&mut ClientState) -> T) -> T {
    STATE.with(|cell| body(&mut cell.borrow_mut()))
}

/// Forgets every dialog and the shown state (activation).
pub fn reset() {
    with_state(|state| *state = ClientState::default());
}

fn message_to_server(channel: &str, payload: Vec<u8>) -> Submission {
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(PLUGIN_ID.to_string()),
            side: PluginSide::Server,
            client: None,
        },
        channel: channel.to_string(),
        payload,
    }))
}

fn open(popup: &str) -> Submission {
    Submission::new(Intent::OpenView(OpenViewSpec {
        view: ViewTarget::Popup(popup.to_owned()),
        focus: true,
    }))
}

fn close(popup: &str) -> Submission {
    Submission::new(Intent::CloseView(ViewTarget::Popup(popup.to_owned())))
}

/// The pane a command is about: the command context first, then `{"pane": id}`.
fn command_pane(context_pane: Option<NodeId>, arguments_json: &str) -> Option<u64> {
    context_pane
        .map(|pane| pane.0)
        .or_else(|| pane_from_arguments(arguments_json))
}

/// Routes one event of the batch.
pub fn on_event(event: Event) -> Vec<Submission> {
    match event {
        Event::CommandInvoked(invoked) => {
            let Some(pane) = command_pane(invoked.context.pane, &invoked.arguments_json) else {
                return Vec::new();
            };
            match invoked.command.as_str() {
                SCHEDULE_COMMAND => {
                    // Arguments that already carry the whole request (the control surface) are
                    // sent as they are; a bare pane opens the dialog.
                    if let Some(request) =
                        ScheduleRequest::from_payload(invoked.arguments_json.as_bytes())
                    {
                        return vec![message_to_server(SCHEDULE_CHANNEL, request.to_payload())];
                    }
                    with_state(|state| state.schedule = Some(ScheduleDialog::new(pane)));
                    vec![open(SCHEDULE_POPUP)]
                }
                QUEUE_COMMAND => {
                    if let Some(request) =
                        EnqueueRequest::from_payload(invoked.arguments_json.as_bytes())
                    {
                        return vec![message_to_server(ENQUEUE_CHANNEL, request.to_payload())];
                    }
                    with_state(|state| state.queue = Some(QueueDialog::new(pane)));
                    vec![open(QUEUE_POPUP)]
                }
                CLEAR_COMMAND => vec![message_to_server(CLEAR_QUEUE_CHANNEL, pane_payload(pane))],
                CANCEL_COMMAND => {
                    vec![message_to_server(
                        CANCEL_SCHEDULE_CHANNEL,
                        pane_payload(pane),
                    )]
                }
                _ => Vec::new(),
            }
        }
        Event::UiAction(action) => match &action.view {
            ViewTarget::Popup(popup) if popup == SCHEDULE_POPUP => {
                schedule_action(&action.key, &action.action_id, action.kind, action.value)
            }
            ViewTarget::Popup(popup) if popup == QUEUE_POPUP => {
                queue_action(&action.key, &action.action_id, action.kind, action.value)
            }
            _ => Vec::new(),
        },
        Event::Message(message) if message.channel == STATE_CHANNEL => {
            if let Some(shown) = decode_state(&message.payload) {
                with_state(|state| state.shown = shown);
            }
            Vec::new()
        }
        Event::ViewVisibility(change) if change.visible => match change.view {
            ViewTarget::Panel(name) if name.ends_with(QUEUE_PANEL) => {
                vec![message_to_server(STATE_REQUEST_CHANNEL, Vec::new())]
            }
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn schedule_action(
    key: &str,
    action_id: &str,
    kind: ActionKind,
    value: Option<ActionValue>,
) -> Vec<Submission> {
    let step = with_state(|state| {
        let step = state
            .schedule
            .as_mut()
            .map(|dialog| dialog.act(key, action_id, kind, value));
        if matches!(step, Some(Step::Confirm(_) | Step::Cancel)) {
            state.schedule = None;
        }
        step
    });
    match step {
        Some(Step::Confirm(request)) => vec![
            message_to_server(SCHEDULE_CHANNEL, request.to_payload()),
            close(SCHEDULE_POPUP),
        ],
        // The kernel already removed a popup it closed itself.
        Some(Step::Cancel) if kind != ActionKind::Closed => vec![close(SCHEDULE_POPUP)],
        _ => Vec::new(),
    }
}

fn queue_action(
    key: &str,
    action_id: &str,
    kind: ActionKind,
    value: Option<ActionValue>,
) -> Vec<Submission> {
    let step = with_state(|state| {
        let step = state
            .queue
            .as_mut()
            .map(|dialog| dialog.act(key, action_id, kind, value));
        if matches!(step, Some(Step::Confirm(_) | Step::Cancel)) {
            state.queue = None;
        }
        step
    });
    match step {
        Some(Step::Confirm(request)) => vec![
            message_to_server(ENQUEUE_CHANNEL, request.to_payload()),
            close(QUEUE_POPUP),
        ],
        Some(Step::Cancel) if kind != ActionKind::Closed => vec![close(QUEUE_POPUP)],
        _ => Vec::new(),
    }
}

/// The view of one target of this plugin.
pub fn render(view: &ViewTarget) -> UiTree {
    with_state(|state| match view {
        ViewTarget::Popup(popup) if popup == SCHEDULE_POPUP => match &state.schedule {
            Some(dialog) => dialog.render(),
            None => blank(),
        },
        ViewTarget::Popup(popup) if popup == QUEUE_POPUP => match &state.queue {
            Some(dialog) => dialog.render(),
            None => blank(),
        },
        _ => panel(&state.shown),
    })
}

/// The queue panel: one line per pending countdown and per non-empty queue.
fn panel(shown: &Shown) -> UiTree {
    let now = {
        let mut api = host();
        HostClock::now_unix_millis(&mut *api)
    };
    let mut lines: Vec<String> = Vec::new();
    for schedule in &shown.schedules {
        let what = match (schedule.text.is_empty(), schedule.send_enter) {
            (false, true) => format!("\"{}\" and Enter", schedule.text),
            (false, false) => format!("\"{}\"", schedule.text),
            (true, _) => "Enter".to_owned(),
        };
        lines.push(format!(
            "Pane {}: {} in {}",
            schedule.pane,
            what,
            format_remaining(remaining_seconds(schedule.at, now))
        ));
    }
    for (pane, length) in &shown.queues {
        lines.push(format!("Pane {pane}: {length} queued"));
    }
    if lines.is_empty() {
        lines.push("Nothing scheduled or queued.".to_owned());
    }
    UiTree {
        version: 1,
        nodes: vec![node(
            "queue",
            Widget::Paragraph(ParagraphSpec {
                lines: lines.iter().map(|line| vec![span(line)]).collect(),
                rich: None,
                wrap: Some(true),
                alignment: Alignment::Left,
                style: Style::default(),
                scrollable: true,
            }),
        )],
        root: 0,
    }
}

fn blank() -> UiTree {
    UiTree {
        version: 1,
        nodes: vec![node(
            "blank",
            Widget::Paragraph(ParagraphSpec {
                lines: Vec::new(),
                rich: None,
                wrap: None,
                alignment: Alignment::Left,
                style: Style::default(),
                scrollable: false,
            }),
        )],
        root: 0,
    }
}

fn dialog_tree(
    title: &str,
    mut message: Vec<String>,
    error: Option<String>,
    buttons: Vec<DialogButton>,
    fields: Vec<UiNode>,
) -> UiTree {
    message.extend(error);
    let dialog = DialogSpec {
        title: vec![span(title)],
        kind: DialogKind::Plain,
        buttons,
        width: Constraint::Length(60),
        height: Constraint::Min(0),
        message: Some(message.iter().map(|line| vec![span(line)]).collect()),
    };
    let mut nodes = vec![UiNode {
        children: (1..=fields.len() as u32).collect(),
        ..node("dialog", Widget::Dialog(dialog))
    }];
    nodes.extend(fields);
    UiTree {
        version: 1,
        nodes,
        root: 0,
    }
}

fn text_field(key: &str, label: &str, initial: &str, placeholder: Option<&str>) -> UiNode {
    node(
        key,
        Widget::TextInput(TextInputSpec {
            label: label.to_owned(),
            initial: initial.to_owned(),
            placeholder: placeholder.map(str::to_owned),
            secret: false,
            reveal_last: 0,
            reveal: false,
            max_length: 4096,
            submit_on_enter: false,
            debounce_ms: 0,
            validation: None,
            suggestions: None,
        }),
    )
}

fn span(text: &str) -> Span {
    Span {
        text: text.to_owned(),
        style: Style::default(),
    }
}

fn button(id: &str, label: &str, role: ButtonRole, is_cancel: bool) -> DialogButton {
    DialogButton {
        id: id.to_owned(),
        label: label.to_owned(),
        role,
        is_cancel,
    }
}

fn node(key: &str, widget: Widget) -> UiNode {
    UiNode {
        key: Some(key.to_owned()),
        widget,
        children: Vec::new(),
        flags: NodeFlags::default(),
        tooltip: None,
        tab_index: None,
        disabled_reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Option<ActionValue> {
        Some(ActionValue::Text(value.to_owned()))
    }

    #[test]
    fn schedule_dialog_confirms_a_valid_draft() {
        let mut dialog = ScheduleDialog::new(5);
        dialog.act("seconds", "", ActionKind::Changed, text("45"));
        dialog.act("text", "", ActionKind::Changed, text("continue"));
        match dialog.act("", "schedule", ActionKind::Activated, None) {
            Step::Confirm(request) => {
                assert_eq!(request.pane, 5);
                assert_eq!(request.delay_seconds, 45);
                assert_eq!(request.text, "continue");
                assert!(request.send_enter);
            }
            other => panic!("expected a confirmation, got {other:?}"),
        }
    }

    #[test]
    fn schedule_dialog_keeps_open_with_an_error_for_an_empty_delay() {
        let mut dialog = ScheduleDialog::new(5);
        dialog.act("seconds", "", ActionKind::Changed, text("0"));
        assert_eq!(
            dialog.act("", "schedule", ActionKind::Activated, None),
            Step::Stay
        );
        assert!(dialog.error.is_some());
    }

    #[test]
    fn queue_dialog_requires_a_prompt_and_a_positive_run_count() {
        let mut dialog = QueueDialog::new(2);
        assert_eq!(
            dialog.act("", "enqueue", ActionKind::Activated, None),
            Step::Stay
        );
        assert!(dialog.error.is_some());
        dialog.act("text", "", ActionKind::Changed, text("next"));
        dialog.act(
            "delivery",
            "",
            ActionKind::Selected,
            Some(ActionValue::Index(1)),
        );
        dialog.act("times", "", ActionKind::Changed, text("0"));
        assert_eq!(
            dialog.act("", "enqueue", ActionKind::Activated, None),
            Step::Stay
        );
        dialog.act("times", "", ActionKind::Changed, text("3"));
        match dialog.act("", "enqueue", ActionKind::Activated, None) {
            Step::Confirm(request) => {
                assert_eq!(request.delivery, Delivery::Times { remaining_runs: 3 });
            }
            other => panic!("expected a confirmation, got {other:?}"),
        }
    }

    #[test]
    fn closing_a_popup_cancels_the_dialog() {
        let mut dialog = ScheduleDialog::new(1);
        assert_eq!(dialog.act("", "", ActionKind::Closed, None), Step::Cancel);
    }

    #[test]
    fn commands_read_the_pane_from_context_or_arguments() {
        assert_eq!(command_pane(Some(NodeId(3)), "{}"), Some(3));
        assert_eq!(command_pane(None, r#"{"pane": 9}"#), Some(9));
        assert_eq!(command_pane(None, "{}"), None);
    }
}
