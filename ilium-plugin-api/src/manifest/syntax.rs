//! Small manifest languages: activation triggers, keybinding chords and `when` expressions
//! (reference-manifest.md "Activation triggers", "Keybinding chords", "The when expression").

use super::Diagnostic;

/// Trigger kinds that take `:<name>`, in manual table order; `startup` and `durable-timer`
/// take no name (reference-manifest.md PKG-059).
pub const ACTIVATION_KINDS: [&str; 15] = [
    "event",
    "command",
    "view",
    "pane-kind",
    "scene",
    "cli",
    "http",
    "service",
    "topic",
    "settings",
    "catalog",
    "menu",
    "virtual-rows",
    "language",
    "client-request",
];

/// One parsed activation trigger (reference-manifest.md "Activation triggers").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationTrigger {
    /// `startup` (reference-manifest.md).
    Startup,
    /// `durable-timer` (reference-manifest.md).
    DurableTimer,
    /// `event:<event-name>` (reference-manifest.md).
    Event(String),
    /// `command:<id>` (reference-manifest.md).
    Command(String),
    /// `view:<id>` (reference-manifest.md).
    View(String),
    /// `pane-kind:<id>` (reference-manifest.md).
    PaneKind(String),
    /// `scene:<id>` (reference-manifest.md).
    Scene(String),
    /// `cli:<name>` (reference-manifest.md).
    Cli(String),
    /// `http:<path>` (reference-manifest.md).
    Http(String),
    /// `service:<interface>` (reference-manifest.md).
    Service(String),
    /// `topic:<topic>` (reference-manifest.md).
    Topic(String),
    /// `settings:<tab-id>` (reference-manifest.md).
    Settings(String),
    /// `catalog:<id>` (reference-manifest.md).
    Catalog(String),
    /// `menu:<id>` (reference-manifest.md).
    Menu(String),
    /// `virtual-rows:<row-kind>` (reference-manifest.md).
    VirtualRows(String),
    /// `language:<id>` (reference-manifest.md).
    Language(String),
    /// `client-request:<kind>` (reference-manifest.md).
    ClientRequest(String),
}

/// Parses one trigger; an unknown kind is refused (reference-manifest.md PKG-059).
pub fn parse_activation(_trigger: &str) -> Result<ActivationTrigger, Diagnostic> {
    unimplemented!("api::manifest::parse_activation")
}

/// A checked `when` expression (reference-manifest.md "The when expression").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhenExpr(pub String);

/// Parses a `when` expression (reference-manifest.md "The when expression").
pub fn parse_when(_expression: &str) -> Result<WhenExpr, Diagnostic> {
    unimplemented!("api::manifest::parse_when")
}

/// A checked keybinding chord: modifiers then key, lower case (reference-manifest.md
/// "Keybinding chords").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord(pub Vec<String>);

/// Parses a chord such as `ctrl+shift+p` (reference-manifest.md "Keybinding chords").
pub fn parse_chord(_chord: &str) -> Result<Chord, Diagnostic> {
    unimplemented!("api::manifest::parse_chord")
}
