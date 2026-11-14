//! The client side of text triggers: the rules panel and the rule editor popup.
//!
//! The panel lists the rules the server side pushes. The editor holds its draft here; a save or a
//! delete is one message to the server side, which validates and stores the list. The preview of
//! the draft against its sample text is computed locally ([`crate::wire::preview`]).

use std::cell::RefCell;

use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, Alignment, ButtonRole, ButtonSpec, ChoiceSpec, ChoiceStyle,
    Constraint, DialogButton, DialogKind, DialogSpec, Direction, DisabledMarker, Event, Flex,
    Intent, LayoutSpec, Margin, MessageSpec, MessageTarget, NodeFlags, OpenViewSpec, ParagraphSpec,
    PluginId, PluginSide, Spacing, Span, Style, Submission, TextInputSpec, ToggleSpec, ToggleStyle,
    UiNode, UiTree, ViewTarget, Widget,
};

use crate::config::{validate, Rule, Target, DEFAULT_DELAY_SECONDS};
use crate::wire::{
    decode_state, id_payload, notice_from_payload, preview, rule_payload, Shown,
    DELETE_RULE_CHANNEL, NOTICE_CHANNEL, SAVE_RULE_CHANNEL, STATE_CHANNEL, STATE_REQUEST_CHANNEL,
};
use crate::PLUGIN_ID;

/// Popup id of the rule editor.
pub const EDITOR_POPUP: &str = "editor";
/// Panel id of the rules panel.
pub const RULES_PANEL: &str = "rules";
/// Key of the panel button that opens the editor.
const OPEN_EDITOR_KEY: &str = "open-editor";

/// The editor draft. `delay` stays text until it is saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    pub id: String,
    pub enabled: bool,
    pub regexp: String,
    pub message: String,
    pub target: Target,
    pub delay: String,
    pub sample: String,
    pub error: Option<String>,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            id: String::new(),
            enabled: true,
            regexp: String::new(),
            message: String::new(),
            target: Target::Both,
            delay: DEFAULT_DELAY_SECONDS.to_string(),
            sample: String::new(),
            error: None,
        }
    }
}

/// What an action did to the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Stay,
    Save(Rule),
    Delete(String),
    Cancel,
}

impl Editor {
    /// Loads an existing rule into the draft.
    pub fn from_rule(rule: &Rule) -> Self {
        Self {
            id: rule.id.clone(),
            enabled: rule.enabled,
            regexp: rule.regexp.clone(),
            message: rule.message.clone(),
            target: rule.target,
            delay: rule.delay_seconds.to_string(),
            sample: rule.sample_text.clone(),
            error: None,
        }
    }

    /// The draft as a rule; a blank delay means the default.
    pub fn to_rule(&self) -> Result<Rule, String> {
        let delay = self.delay.trim();
        let delay_seconds = if delay.is_empty() {
            DEFAULT_DELAY_SECONDS
        } else {
            delay
                .parse::<u32>()
                .map_err(|_| "Delay must be a whole number of seconds".to_string())?
        };
        let rule = Rule {
            id: self.id.trim().to_owned(),
            enabled: self.enabled,
            regexp: self.regexp.clone(),
            message: self.message.clone(),
            target: self.target,
            sample_text: self.sample.clone(),
            delay_seconds,
        };
        match validate(std::slice::from_ref(&rule)) {
            Some(reason) => Err(reason),
            None => Ok(rule),
        }
    }

    pub fn act(
        &mut self,
        key: &str,
        action_id: &str,
        kind: ActionKind,
        value: Option<ActionValue>,
    ) -> Step {
        match (kind, value) {
            (ActionKind::Closed, _) => Step::Cancel,
            (ActionKind::Changed | ActionKind::Submitted, Some(ActionValue::Text(text))) => {
                match key {
                    "id" => self.id = text,
                    "regexp" => self.regexp = text,
                    "message" => self.message = text,
                    "delay" => self.delay = text,
                    "sample" => self.sample = text,
                    _ => {}
                }
                self.error = None;
                Step::Stay
            }
            (ActionKind::Changed | ActionKind::Selected, Some(ActionValue::Index(index)))
                if key == "target" =>
            {
                if let Some(target) = Target::ALL.get(index as usize) {
                    self.target = *target;
                }
                Step::Stay
            }
            (ActionKind::Changed | ActionKind::Toggled, Some(ActionValue::Flag(flag)))
                if key == "enabled" =>
            {
                self.enabled = flag;
                Step::Stay
            }
            (ActionKind::Activated, _) => match action_id {
                "save" => match self.to_rule() {
                    Ok(rule) => Step::Save(rule),
                    Err(message) => {
                        self.error = Some(message);
                        Step::Stay
                    }
                },
                "delete" => Step::Delete(self.id.trim().to_owned()),
                "cancel" => Step::Cancel,
                _ => Step::Stay,
            },
            _ => Step::Stay,
        }
    }

    /// The popup: the fields, then the preview of the pattern against the sample.
    pub fn render(&self) -> UiTree {
        let mut message = vec![
            "Send the reply to the pane that printed text matching the regexp.".to_owned(),
            "An id that already exists replaces that rule.".to_owned(),
        ];
        if !self.regexp.is_empty() && !self.sample.is_empty() {
            match preview(&self.regexp, &self.message, &self.sample) {
                Ok(result) => message.extend(result.lines),
                Err(error) => message.push(error),
            }
        }
        message.extend(self.error.clone());
        let selected = Target::ALL
            .iter()
            .position(|target| *target == self.target)
            .unwrap_or(0) as u32;
        let fields = vec![
            text_field("id", "Id", &self.id, None),
            text_field("regexp", "Regexp", &self.regexp, None),
            text_field("message", "Reply", &self.message, None),
            text_field("delay", "Delay (seconds)", &self.delay, Some("60")),
            node(
                "target",
                Widget::Choice(ChoiceSpec {
                    label: "Panes".to_owned(),
                    options: Target::ALL
                        .iter()
                        .map(|target| target.label().to_owned())
                        .collect(),
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
            node(
                "enabled",
                Widget::Toggle(ToggleSpec {
                    label: "Enabled".to_owned(),
                    value: self.enabled,
                    style: ToggleStyle::Checkbox,
                    tri_state: false,
                    icon_on: None,
                    icon_off: None,
                }),
            ),
            text_field("sample", "Sample text", &self.sample, None),
        ];
        let dialog = DialogSpec {
            title: vec![span("Text trigger")],
            kind: DialogKind::Plain,
            buttons: vec![
                button("save", "Save", ButtonRole::Primary, false),
                button("delete", "Delete", ButtonRole::Danger, false),
                button("cancel", "Cancel", ButtonRole::Secondary, true),
            ],
            width: Constraint::Length(70),
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
}

#[derive(Default)]
struct ClientState {
    editor: Option<Editor>,
    shown: Shown,
    notice: Option<String>,
}

thread_local! {
    static STATE: RefCell<ClientState> = RefCell::new(ClientState::default());
}

fn with_state<T>(body: impl FnOnce(&mut ClientState) -> T) -> T {
    STATE.with(|cell| body(&mut cell.borrow_mut()))
}

/// Forgets the editor and the shown rules (activation).
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

fn close_editor() -> Submission {
    Submission::new(Intent::CloseView(ViewTarget::Popup(
        EDITOR_POPUP.to_owned(),
    )))
}

/// Routes one event of the batch.
pub fn on_event(event: Event) -> Vec<Submission> {
    match event {
        Event::UiAction(action) => match &action.view {
            ViewTarget::Popup(popup) if popup == EDITOR_POPUP => {
                editor_action(&action.key, &action.action_id, action.kind, action.value)
            }
            ViewTarget::Panel(panel)
                if panel.ends_with(RULES_PANEL)
                    && action.key == OPEN_EDITOR_KEY
                    && action.kind == ActionKind::Activated =>
            {
                with_state(|state| state.editor = Some(Editor::default()));
                vec![Submission::new(Intent::OpenView(OpenViewSpec {
                    view: ViewTarget::Popup(EDITOR_POPUP.to_owned()),
                    focus: true,
                }))]
            }
            _ => Vec::new(),
        },
        Event::Message(message) if message.channel == STATE_CHANNEL => {
            if let Some(shown) = decode_state(&message.payload) {
                with_state(|state| {
                    state.shown = shown;
                    state.notice = None;
                });
            }
            Vec::new()
        }
        Event::Message(message) if message.channel == NOTICE_CHANNEL => {
            let text = notice_from_payload(&message.payload);
            with_state(|state| {
                if let (Some(editor), Some(text)) = (state.editor.as_mut(), text.clone()) {
                    editor.error = Some(text);
                } else {
                    state.notice = text;
                }
            });
            Vec::new()
        }
        Event::ViewVisibility(change) if change.visible => match change.view {
            ViewTarget::Panel(name) if name.ends_with(RULES_PANEL) => {
                vec![message_to_server(STATE_REQUEST_CHANNEL, Vec::new())]
            }
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn editor_action(
    key: &str,
    action_id: &str,
    kind: ActionKind,
    value: Option<ActionValue>,
) -> Vec<Submission> {
    let step = with_state(|state| {
        let step = state
            .editor
            .as_mut()
            .map(|editor| editor.act(key, action_id, kind, value))?;
        if !matches!(step, Step::Stay) {
            state.editor = None;
        }
        Some(step)
    });
    match step {
        Some(Step::Save(rule)) => vec![
            message_to_server(SAVE_RULE_CHANNEL, rule_payload(&rule)),
            close_editor(),
        ],
        Some(Step::Delete(id)) => vec![
            message_to_server(DELETE_RULE_CHANNEL, id_payload(&id)),
            close_editor(),
        ],
        // The kernel already removed a popup it closed itself.
        Some(Step::Cancel) if kind != ActionKind::Closed => vec![close_editor()],
        _ => Vec::new(),
    }
}

/// The view of one target of this plugin.
pub fn render(view: &ViewTarget) -> UiTree {
    with_state(|state| match view {
        ViewTarget::Popup(popup) if popup == EDITOR_POPUP => match &state.editor {
            Some(editor) => editor.render(),
            None => blank(),
        },
        _ => panel(&state.shown, state.notice.as_deref()),
    })
}

/// One line per rule: state, id, regexp, reply, target and delay.
pub fn rule_line(rule: &Rule) -> String {
    format!(
        "[{}] {}  /{}/ -> \"{}\"  ({}, after {} s)",
        if rule.enabled { "x" } else { " " },
        rule.id,
        rule.regexp,
        rule.message,
        rule.target.label(),
        rule.delay_seconds
    )
}

fn panel(shown: &Shown, notice: Option<&str>) -> UiTree {
    let mut lines: Vec<String> = shown.rules.iter().map(rule_line).collect();
    if lines.is_empty() {
        lines.push("No text triggers.".to_owned());
    }
    if shown.pending > 0 {
        lines.push(format!(
            "{} replies waiting for their delay.",
            shown.pending
        ));
    }
    lines.extend(notice.map(str::to_owned));
    let layout = LayoutSpec {
        direction: Direction::Vertical,
        constraints: vec![Constraint::Fill(1), Constraint::Length(1)],
        flex: Flex::Legacy,
        spacing: Spacing::Space(0),
        margin: Margin::default(),
    };
    let paragraph = ParagraphSpec {
        lines: lines.iter().map(|line| vec![span(line)]).collect(),
        rich: None,
        wrap: Some(true),
        alignment: Alignment::Left,
        style: Style::default(),
        scrollable: true,
    };
    let open = ButtonSpec {
        label: "Add or edit a rule...".to_owned(),
        icon: None,
        enabled: true,
        role: ButtonRole::Primary,
    };
    UiTree {
        version: 1,
        nodes: vec![
            UiNode {
                children: vec![1, 2],
                ..node("panel", Widget::Layout(layout))
            },
            node("rules", Widget::Paragraph(paragraph)),
            node(OPEN_EDITOR_KEY, Widget::Button(open)),
        ],
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

    fn filled() -> Editor {
        let mut editor = Editor::default();
        editor.act("id", "", ActionKind::Changed, text("go"));
        editor.act("regexp", "", ActionKind::Changed, text("proceed\\?"));
        editor.act("message", "", ActionKind::Changed, text("yes"));
        editor
    }

    #[test]
    fn a_valid_draft_saves_with_the_default_delay() {
        let mut editor = filled();
        match editor.act("", "save", ActionKind::Activated, None) {
            Step::Save(rule) => {
                assert_eq!(rule.id, "go");
                assert_eq!(rule.delay_seconds, DEFAULT_DELAY_SECONDS);
                assert_eq!(rule.target, Target::Both);
            }
            other => panic!("expected a save, got {other:?}"),
        }
    }

    #[test]
    fn an_invalid_regexp_keeps_the_editor_open_with_the_reason() {
        let mut editor = filled();
        editor.act("regexp", "", ActionKind::Changed, text("("));
        assert_eq!(
            editor.act("", "save", ActionKind::Activated, None),
            Step::Stay
        );
        assert!(editor.error.is_some_and(|error| error.contains("regexp")));
    }

    #[test]
    fn a_bad_delay_is_refused() {
        let mut editor = filled();
        editor.act("delay", "", ActionKind::Changed, text("soon"));
        assert_eq!(
            editor.act("", "save", ActionKind::Activated, None),
            Step::Stay
        );
        assert!(editor.error.is_some());
    }

    #[test]
    fn target_selection_and_delete_work() {
        let mut editor = filled();
        editor.act(
            "target",
            "",
            ActionKind::Selected,
            Some(ActionValue::Index(1)),
        );
        assert_eq!(editor.target, Target::Terminals);
        assert_eq!(
            editor.act("", "delete", ActionKind::Activated, None),
            Step::Delete("go".to_owned())
        );
    }

    #[test]
    fn the_rule_line_shows_state_and_delay() {
        let rule = Rule {
            id: "go".to_owned(),
            regexp: "a".to_owned(),
            message: "y".to_owned(),
            ..Rule::default()
        };
        assert_eq!(rule_line(&rule), "[x] go  /a/ -> \"y\"  (Both, after 60 s)");
    }
}
