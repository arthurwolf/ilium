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
pub fn parse_activation(trigger: &str) -> Result<ActivationTrigger, Diagnostic> {
    if trigger == "startup" {
        return Ok(ActivationTrigger::Startup);
    }
    if trigger == "durable-timer" {
        return Ok(ActivationTrigger::DurableTimer);
    }
    let invalid = || {
        super::validate::finding(
            "manifest.schema",
            "/activation",
            "Invalid activation trigger.",
            3,
        )
    };
    let (kind, name) = trigger.split_once(':').ok_or_else(invalid)?;
    if name.is_empty() || name.chars().any(char::is_whitespace) || trigger.chars().count() > 256 {
        return Err(invalid());
    }
    Ok(match kind {
        "event" => ActivationTrigger::Event(name.into()),
        "command" => ActivationTrigger::Command(name.into()),
        "view" => ActivationTrigger::View(name.into()),
        "pane-kind" => ActivationTrigger::PaneKind(name.into()),
        "scene" => ActivationTrigger::Scene(name.into()),
        "cli" => ActivationTrigger::Cli(name.into()),
        "http" => ActivationTrigger::Http(name.into()),
        "service" => ActivationTrigger::Service(name.into()),
        "topic" => ActivationTrigger::Topic(name.into()),
        "settings" => ActivationTrigger::Settings(name.into()),
        "catalog" => ActivationTrigger::Catalog(name.into()),
        "menu" => ActivationTrigger::Menu(name.into()),
        "virtual-rows" => ActivationTrigger::VirtualRows(name.into()),
        "language" => ActivationTrigger::Language(name.into()),
        "client-request" => ActivationTrigger::ClientRequest(name.into()),
        _ => return Err(invalid()),
    })
}

/// A checked `when` expression (reference-manifest.md "The when expression").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhenExpr(pub String);

/// Parses a `when` expression (reference-manifest.md "The when expression").
pub fn parse_when(expression: &str) -> Result<WhenExpr, Diagnostic> {
    let mut parser = WhenParser {
        input: expression,
        offset: 0,
        depth: 0,
    };
    parser.expression()?;
    parser.space();
    if parser.offset != expression.len() {
        return Err(parser.invalid());
    }
    Ok(WhenExpr(expression.into()))
}

/// A checked keybinding chord: modifiers then key, lower case (reference-manifest.md
/// "Keybinding chords").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord(pub Vec<String>);

/// Parses a chord such as `ctrl+shift+p` (reference-manifest.md "Keybinding chords").
pub fn parse_chord(chord: &str) -> Result<Chord, Diagnostic> {
    let invalid =
        || super::validate::finding("manifest.schema", "", "Invalid keybinding chord.", 3);
    let parts: Vec<_> = chord.split('+').collect();
    let Some((key, modifiers)) = parts.split_last() else {
        return Err(invalid());
    };
    let mut seen = std::collections::BTreeSet::new();
    if modifiers.iter().any(|modifier| {
        !["ctrl", "alt", "shift", "super"].contains(modifier) || !seen.insert(*modifier)
    }) {
        return Err(invalid());
    }
    let printable = key.chars().count() == 1
        && key
            .chars()
            .all(|c| !c.is_control() && !c.is_whitespace() && !c.is_uppercase());
    let named = [
        "plus",
        "enter",
        "escape",
        "tab",
        "backspace",
        "delete",
        "insert",
        "home",
        "end",
        "page-up",
        "page-down",
        "up",
        "down",
        "left",
        "right",
    ]
    .contains(key);
    let function = key
        .strip_prefix('f')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=24).contains(&number) && *key == format!("f{number}"));
    if !printable && !named && !function {
        return Err(invalid());
    }
    Ok(Chord(parts.into_iter().map(str::to_owned).collect()))
}

struct WhenParser<'a> {
    input: &'a str,
    offset: usize,
    depth: usize,
}
impl WhenParser<'_> {
    fn invalid(&self) -> Diagnostic {
        super::validate::finding(
            "manifest.when-syntax",
            "",
            format!(
                "The when expression is not valid at column {}.",
                self.input[..self.offset].chars().count() + 1
            ),
            3,
        )
    }
    fn space(&mut self) {
        while let Some(character) = self.input[self.offset..]
            .chars()
            .next()
            .filter(|c| c.is_whitespace())
        {
            self.offset += character.len_utf8();
        }
    }
    fn take(&mut self, token: &str) -> bool {
        self.space();
        if self.input[self.offset..].starts_with(token) {
            self.offset += token.len();
            true
        } else {
            false
        }
    }
    fn expression(&mut self) -> Result<(), Diagnostic> {
        self.conjunction()?;
        while self.take("||") {
            self.conjunction()?;
        }
        Ok(())
    }
    fn conjunction(&mut self) -> Result<(), Diagnostic> {
        self.primary()?;
        while self.take("&&") {
            self.primary()?;
        }
        Ok(())
    }
    fn primary(&mut self) -> Result<(), Diagnostic> {
        if self.depth >= 32 {
            return Err(self.invalid());
        }
        self.depth += 1;
        if self.take("!") {
            self.primary()?;
            self.depth -= 1;
            return Ok(());
        }
        if self.take("(") {
            self.expression()?;
            if !self.take(")") {
                return Err(self.invalid());
            }
            self.depth -= 1;
            return Ok(());
        }
        self.space();
        let start = self.offset;
        while let Some(c) = self.input[self.offset..]
            .chars()
            .next()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            self.offset += c.len_utf8();
        }
        let key = &self.input[start..self.offset];
        if key.is_empty() {
            return Err(self.invalid());
        }
        if ![
            "pane.primitive",
            "pane.kind",
            "agent.activity",
            "node.kind",
            "node.file_extension",
            "agent.signature",
            "view.focused",
            "display_mode",
        ]
        .contains(&key)
        {
            return Err(super::validate::finding(
                "manifest.when-key",
                "",
                format!("{key:?} is not a context key."),
                3,
            ));
        }
        if !self.take("==") && !self.take("!=") {
            return Err(self.invalid());
        }
        self.space();
        let start = self.offset;
        if !self.input[self.offset..].starts_with('"') {
            return Err(self.invalid());
        }
        self.offset += 1;
        let mut escaped = false;
        while let Some(character) = self.input[self.offset..].chars().next() {
            self.offset += character.len_utf8();
            if character == '"' && !escaped {
                let value = &self.input[start..self.offset];
                if serde_json::from_str::<String>(value).is_err() {
                    return Err(self.invalid());
                }
                self.depth -= 1;
                return Ok(());
            }
            escaped = character == '\\' && !escaped;
        }
        Err(self.invalid())
    }
}
