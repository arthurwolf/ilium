//! Typed semantic commands accepted by every control transport.

use serde::Deserialize;

/// Stable node reference. An omitted target means the currently selected or
/// active node; callers should prefer `id` after reading `ilium_get_state`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NodeTarget {
    pub id: Option<u64>,
    pub name: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateCommand {
    #[serde(default = "default_state_detail")]
    pub detail: StateDetail,
    #[serde(default)]
    pub target: NodeTarget,
}

fn default_state_detail() -> StateDetail {
    StateDetail::Compact
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateDetail {
    Compact,
    Full,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiCommand {
    pub action: UiAction,
    #[serde(default)]
    pub target: NodeTarget,
    pub tab: Option<String>,
    pub direction: Option<Direction>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiAction {
    FocusTree,
    FocusPane,
    FocusNextPane,
    FocusPreviousPane,
    FocusPaneDirection,
    ShowSplit,
    OpenSettings,
    ShowSettingsTab,
    OpenSearch,
    OpenHelp,
    CloseOverlay,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    pub const fn sign(self) -> i32 {
        match self {
            Self::Up | Self::Left => -1,
            Self::Down | Self::Right => 1,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeCommand {
    pub action: TreeAction,
    #[serde(default)]
    pub target: NodeTarget,
    #[serde(default)]
    pub parent: NodeTarget,
    pub name: Option<String>,
    pub path: Option<String>,
    pub command_line: Option<String>,
    pub initial_input: Option<String>,
    pub orientation: Option<SplitDirection>,
    #[serde(default)]
    pub members: Vec<NodeTarget>,
    pub index: Option<usize>,
    pub storage: Option<BoardStorageChoice>,
    pub provider: Option<AgentProviderChoice>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeAction {
    CreateTerminal,
    CreateAgent,
    CreateCommandPane,
    OpenEditor,
    AddFolder,
    AddProject,
    ChangeProjectFolder,
    CreateGroup,
    CreateBoard,
    CreateSplit,
    Rename,
    MoveUp,
    MoveDown,
    Reparent,
    Close,
    ToggleExpanded,
    Retitle,
    RestructureAllProjects,
    RestructureProject,
    RevertProjectRestructure,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProviderChoice {
    Claude,
    Codex,
    Antigravity,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardStorageChoice {
    Markdown,
    Folder,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalSubmissionCommand {
    #[serde(default)]
    pub target: NodeTarget,
    pub text: String,
    #[serde(default = "default_true")]
    pub send_enter: bool,
}

fn default_true() -> bool {
    true
}

impl From<TerminalSubmissionCommand> for TerminalCommand {
    fn from(command: TerminalSubmissionCommand) -> Self {
        Self {
            action: TerminalAction::Write,
            target: command.target,
            text: Some(command.text),
            key: None,
            lines: None,
            delay_seconds: None,
            send_enter: Some(command.send_enter),
            delivery: None,
            runs: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalCommand {
    pub action: TerminalAction,
    #[serde(default)]
    pub target: NodeTarget,
    pub text: Option<String>,
    pub key: Option<TerminalKey>,
    pub lines: Option<u16>,
    pub delay_seconds: Option<u64>,
    pub send_enter: Option<bool>,
    pub delivery: Option<PromptDeliveryChoice>,
    pub runs: Option<u32>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalAction {
    Write,
    PressKey,
    ScrollUp,
    ScrollDown,
    ScrollToBottom,
    ScheduleInput,
    QueuePrompt,
    ClearPromptQueue,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalKey {
    Enter,
    Escape,
    Tab,
    Backtab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Backspace,
    Delete,
    Space,
    ControlC,
    ControlD,
    ControlL,
    ControlZ,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptDeliveryChoice {
    Once,
    Times,
    Forever,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditorCommand {
    pub action: EditorAction,
    #[serde(default)]
    pub target: NodeTarget,
    pub text: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorAction {
    Save,
    SaveAs,
    InsertText,
    ReplaceDocument,
    JumpTo,
    ToggleRendered,
    ToggleLineNumbers,
    ToggleMinimap,
    ToggleAutosave,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardCommand {
    pub action: BoardAction,
    #[serde(default)]
    pub target: NodeTarget,
    pub title: Option<String>,
    pub body: Option<String>,
    pub column: Option<usize>,
    pub card: Option<usize>,
    pub destination_column: Option<usize>,
    pub destination_card: Option<usize>,
    pub checkbox: Option<usize>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardAction {
    SelectColumn,
    SelectCard,
    OpenCard,
    CloseCard,
    AddColumn,
    AddCard,
    UpdateCard,
    RenameColumn,
    MoveCard,
    ToggleCheckbox,
    DeleteCard,
    DeleteColumn,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsCommand {
    pub action: SettingsAction,
    pub path: Option<String>,
    pub value: Option<serde_json::Value>,
    pub direction: Option<Direction>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsAction {
    Get,
    Set,
    Adjust,
    TestInference,
    RefreshOllamaModels,
    PreviewSound,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchCommand {
    pub action: SearchAction,
    pub query: Option<String>,
    pub index: Option<usize>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchAction {
    Query,
    SelectNext,
    SelectPrevious,
    OpenResult,
    Close,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionCommand {
    pub action: SessionAction,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionAction {
    Detach,
    RestartClient,
    RestartServer,
    KillSession,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfirmationCommand {
    pub token: String,
    pub confirmed: bool,
}

#[derive(Debug, Clone)]
pub enum ControlCommand {
    State(StateCommand),
    Ui(UiCommand),
    Tree(TreeCommand),
    Terminal(TerminalCommand),
    Editor(EditorCommand),
    Board(BoardCommand),
    Settings(SettingsCommand),
    Search(SearchCommand),
    Session(SessionCommand),
}
