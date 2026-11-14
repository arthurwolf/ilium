//! The "New board" dialog: a name, where the board is stored and the path of that storage.
//!
//! Moved from the kernel `Mode::CreateBoard` dialog (`ilium-client/src/app.rs`, `keys.rs`,
//! `ui.rs`). Everything here is plain data and rules; `client.rs` connects it to events and
//! intents. The kernel runs the `new-board` command with the group a new node goes into and,
//! without a path, the plugin shows this dialog; with a path (a file chosen from the Markdown
//! editor, the `ilium new-board` command line) the board is created at once.

use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, ButtonRole, ChoiceSpec, ChoiceStyle, Constraint, DialogButton,
    DialogKind, DialogSpec, DisabledMarker, NodeFlags, Span, Style, TextInputSpec, UiNode, UiTree,
    ViewTarget, Widget,
};

use crate::model::Storage;

/// Popup id of the dialog.
pub const POPUP: &str = "new-board";
/// Key of the name field.
pub const NAME_KEY: &str = "name";
/// Key of the storage choice.
pub const KIND_KEY: &str = "storage";
/// Key of the path field.
pub const PATH_KEY: &str = "path";
/// The folder every new board file starts in, relative to the project.
pub const DEFAULT_DIRECTORY: &str = ".ilium/boards";
/// The file name a new Markdown board starts with.
pub const DEFAULT_FILE: &str = "board.md";
/// The name a new board starts with.
pub const DEFAULT_NAME: &str = "Board";
/// Longest name the dialog accepts, in bytes.
pub const MAXIMUM_NAME_BYTES: usize = 8 * 1024;
/// Longest path the dialog accepts, in bytes.
pub const MAXIMUM_PATH_BYTES: usize = 64 * 1024;
/// Shown when the path field is empty.
pub const PATH_REQUIRED: &str = "Board storage path is required";
/// Shown when the name or path is longer than the limits.
pub const TOO_LONG: &str = "Board creation command exceeds retained byte limit";

/// The popup view of the dialog.
pub fn view() -> ViewTarget {
    ViewTarget::Popup(POPUP.to_owned())
}

/// Where the board lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// One Markdown file: a heading per column, a list item per card.
    MarkdownFile,
    /// A folder: one sub-folder per column, one Markdown file per card.
    Folder,
}

impl Kind {
    /// The label shown in the choice.
    pub fn label(self) -> &'static str {
        match self {
            Self::MarkdownFile => "Markdown file",
            Self::Folder => "Folder of cards",
        }
    }

    fn index(self) -> u32 {
        match self {
            Self::MarkdownFile => 0,
            Self::Folder => 1,
        }
    }

    fn from_index(index: u32) -> Self {
        if index == 0 {
            Self::MarkdownFile
        } else {
            Self::Folder
        }
    }
}

/// The default storage path of a board of this kind.
pub fn default_path(kind: Kind) -> String {
    match kind {
        Kind::MarkdownFile => format!("{DEFAULT_DIRECTORY}/{DEFAULT_FILE}"),
        Kind::Folder => format!("{DEFAULT_DIRECTORY}/board"),
    }
}

/// What the user decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Keep the dialog open.
    Stay,
    /// Close it and create nothing.
    Cancel,
    /// Close it and create this board.
    Create(Created),
}

/// The board the dialog asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    /// The group the pane goes into; `None` lets the kernel pick.
    pub parent: Option<u64>,
    /// The title of the pane.
    pub name: String,
    pub storage: Storage,
}

/// The open dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    parent: Option<u64>,
    name: String,
    kind: Kind,
    path: String,
    /// The path was typed or chosen; switching the kind then keeps it.
    path_edited: bool,
    notice: Option<String>,
}

impl Dialog {
    /// A dialog that starts with the default name, a Markdown file and its default path.
    pub fn new(parent: Option<u64>) -> Self {
        Self {
            parent,
            name: DEFAULT_NAME.to_owned(),
            kind: Kind::MarkdownFile,
            path: default_path(Kind::MarkdownFile),
            path_edited: false,
            notice: None,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// The board this dialog would create, or why it cannot.
    pub fn answer(&self) -> Result<Created, &'static str> {
        if self.name.len() > MAXIMUM_NAME_BYTES || self.path.len() > MAXIMUM_PATH_BYTES {
            return Err(TOO_LONG);
        }
        let path = clean_path(&self.path);
        if path.is_empty() {
            return Err(PATH_REQUIRED);
        }
        let storage = match self.kind {
            Kind::MarkdownFile => Storage::MarkdownFile { path },
            Kind::Folder => Storage::Folder { path },
        };
        let name = self.name.trim();
        Ok(Created {
            parent: self.parent,
            name: if name.is_empty() {
                DEFAULT_NAME.to_owned()
            } else {
                name.to_owned()
            },
            storage,
        })
    }

    /// Applies one action the kernel reported for the popup.
    pub fn act(
        &mut self,
        key: &str,
        action_id: &str,
        kind: ActionKind,
        value: Option<ActionValue>,
    ) -> Step {
        match (kind, value) {
            (ActionKind::Closed, _) => Step::Cancel,
            (ActionKind::Changed | ActionKind::Submitted, Some(ActionValue::Text(text)))
                if key == NAME_KEY =>
            {
                self.name = text;
                Step::Stay
            }
            (ActionKind::Changed | ActionKind::Submitted, Some(ActionValue::Text(text)))
                if key == PATH_KEY =>
            {
                self.path = text;
                self.path_edited = true;
                Step::Stay
            }
            (ActionKind::Changed | ActionKind::Selected, Some(ActionValue::Index(index)))
                if key == KIND_KEY =>
            {
                let chosen = Kind::from_index(index);
                if chosen != self.kind {
                    self.kind = chosen;
                    if !self.path_edited {
                        self.path = default_path(chosen);
                    }
                }
                Step::Stay
            }
            (ActionKind::Activated, _) => match action_id {
                "cancel" => Step::Cancel,
                "create" => match self.answer() {
                    Ok(created) => Step::Create(created),
                    Err(message) => {
                        self.notice = Some(message.to_owned());
                        Step::Stay
                    }
                },
                _ => Step::Stay,
            },
            _ => Step::Stay,
        }
    }

    /// The popup view of the dialog.
    pub fn render(&self) -> UiTree {
        let mut message = vec![
            "Choose a name, where the board is stored and the path of that storage.".to_owned(),
            "A relative path starts in the project folder.".to_owned(),
        ];
        message.extend(self.notice.clone());
        let dialog = DialogSpec {
            title: vec![span("New board")],
            kind: DialogKind::Plain,
            buttons: vec![
                button("create", "Create", ButtonRole::Primary, false),
                button("cancel", "Cancel", ButtonRole::Secondary, true),
            ],
            width: Constraint::Length(72),
            height: Constraint::Min(0),
            message: Some(message.iter().map(|line| vec![span(line)]).collect()),
        };
        UiTree {
            version: 1,
            nodes: vec![
                UiNode {
                    children: vec![1, 2, 3],
                    ..node("dialog", Widget::Dialog(dialog))
                },
                node(
                    NAME_KEY,
                    Widget::TextInput(TextInputSpec {
                        label: "Name".to_owned(),
                        initial: self.name.clone(),
                        submit_on_enter: false,
                        ..TextInputSpec::default()
                    }),
                ),
                node(
                    KIND_KEY,
                    Widget::Choice(ChoiceSpec {
                        label: "Storage".to_owned(),
                        options: vec![
                            Kind::MarkdownFile.label().to_owned(),
                            Kind::Folder.label().to_owned(),
                        ],
                        selected: self.kind.index(),
                        style: ChoiceStyle::List,
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
                    PATH_KEY,
                    Widget::TextInput(TextInputSpec {
                        label: "Path".to_owned(),
                        initial: self.path.clone(),
                        submit_on_enter: false,
                        ..TextInputSpec::default()
                    }),
                ),
            ],
            root: 0,
        }
    }
}

/// A path as the board storage keeps it: trimmed, with a leading `./` removed.
pub fn clean_path(entered: &str) -> String {
    let trimmed = entered.trim();
    trimmed
        .strip_prefix("./")
        .unwrap_or(trimmed)
        .trim_end_matches('/')
        .to_owned()
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
    fn starts_with_the_default_markdown_board() {
        let dialog = Dialog::new(Some(4));
        let created = dialog.answer().expect("the default answer is valid");
        assert_eq!(created.parent, Some(4));
        assert_eq!(created.name, "Board");
        assert_eq!(
            created.storage,
            Storage::MarkdownFile {
                path: ".ilium/boards/board.md".to_owned()
            }
        );
    }

    #[test]
    fn choosing_a_folder_moves_the_default_path_until_the_path_is_typed() {
        let mut dialog = Dialog::new(None);
        dialog.act(
            KIND_KEY,
            "",
            ActionKind::Changed,
            Some(ActionValue::Index(1)),
        );
        assert_eq!(dialog.kind(), Kind::Folder);
        assert_eq!(dialog.path(), ".ilium/boards/board");
        dialog.act(PATH_KEY, "", ActionKind::Changed, text("plans/sprint"));
        dialog.act(
            KIND_KEY,
            "",
            ActionKind::Changed,
            Some(ActionValue::Index(0)),
        );
        assert_eq!(dialog.kind(), Kind::MarkdownFile);
        assert_eq!(dialog.path(), "plans/sprint", "a typed path stays");
    }

    #[test]
    fn create_refuses_an_empty_path_and_keeps_the_dialog_open() {
        let mut dialog = Dialog::new(None);
        dialog.act(PATH_KEY, "", ActionKind::Changed, text("   "));
        let step = dialog.act("dialog", "create", ActionKind::Activated, None);
        assert_eq!(step, Step::Stay);
        assert_eq!(dialog.notice(), Some(PATH_REQUIRED));
    }

    #[test]
    fn create_trims_the_name_and_cleans_the_path() {
        let mut dialog = Dialog::new(None);
        dialog.act(NAME_KEY, "", ActionKind::Changed, text("  Sprint  "));
        dialog.act(
            PATH_KEY,
            "",
            ActionKind::Changed,
            text(" ./plans/sprint.md "),
        );
        let Step::Create(created) = dialog.act("dialog", "create", ActionKind::Activated, None)
        else {
            panic!("a valid dialog creates");
        };
        assert_eq!(created.name, "Sprint");
        assert_eq!(
            created.storage,
            Storage::MarkdownFile {
                path: "plans/sprint.md".to_owned()
            }
        );
    }

    #[test]
    fn an_empty_name_falls_back_to_board() {
        let mut dialog = Dialog::new(None);
        dialog.act(NAME_KEY, "", ActionKind::Changed, text("  "));
        assert_eq!(dialog.answer().expect("valid").name, "Board");
    }

    #[test]
    fn too_long_input_is_refused() {
        let mut dialog = Dialog::new(None);
        dialog.act(
            NAME_KEY,
            "",
            ActionKind::Changed,
            text(&"n".repeat(MAXIMUM_NAME_BYTES + 1)),
        );
        assert_eq!(dialog.answer(), Err(TOO_LONG));
    }

    #[test]
    fn cancel_and_close_end_the_dialog() {
        let mut dialog = Dialog::new(None);
        assert_eq!(
            dialog.act("dialog", "cancel", ActionKind::Activated, None),
            Step::Cancel
        );
        assert_eq!(
            dialog.act("dialog", "", ActionKind::Closed, None),
            Step::Cancel
        );
    }

    #[test]
    fn the_view_holds_the_dialog_and_its_three_fields() {
        let tree = Dialog::new(None).render();
        assert!(matches!(tree.nodes[0].widget, Widget::Dialog(_)));
        assert_eq!(tree.nodes[0].children, vec![1, 2, 3]);
        let keys: Vec<_> = tree.nodes[1..]
            .iter()
            .filter_map(|node| node.key.as_deref())
            .collect();
        assert_eq!(keys, [NAME_KEY, KIND_KEY, PATH_KEY]);
    }
}
