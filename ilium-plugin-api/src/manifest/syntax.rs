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

#[cfg(test)]
mod tests {
    use super::super::diagnostic::Stage;
    use super::*;

    /// The kinds that take `:<name>`, in the manual table order (reference-manifest.md
    /// "Activation triggers").
    const NAMED_KINDS_IN_MANUAL_ORDER: [&str; 15] = [
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

    /// REQ: PKG-059
    #[test]
    fn activation_kinds_match_manual_table_order() {
        assert_eq!(ACTIVATION_KINDS, NAMED_KINDS_IN_MANUAL_ORDER);
    }

    /// REQ: PKG-059
    #[test]
    fn parse_activation_startup_takes_no_name() {
        let trigger = parse_activation("startup").expect("startup is a valid trigger");
        assert_eq!(trigger, ActivationTrigger::Startup);
    }

    /// REQ: PKG-059
    #[test]
    fn parse_activation_durable_timer_takes_no_name() {
        let trigger = parse_activation("durable-timer").expect("durable-timer is a valid trigger");
        assert_eq!(trigger, ActivationTrigger::DurableTimer);
    }

    /// REQ: PKG-059
    #[test]
    fn parse_activation_maps_every_named_kind_to_its_variant() {
        let expected_pairs: [(&str, ActivationTrigger); 15] = [
            (
                "event:example.tick",
                ActivationTrigger::Event("example.tick".to_string()),
            ),
            (
                "command:example.demo/show",
                ActivationTrigger::Command("example.demo/show".to_string()),
            ),
            (
                "view:example.demo.view",
                ActivationTrigger::View("example.demo.view".to_string()),
            ),
            (
                "pane-kind:example.notes.note",
                ActivationTrigger::PaneKind("example.notes.note".to_string()),
            ),
            (
                "scene:example.wave",
                ActivationTrigger::Scene("example.wave".to_string()),
            ),
            ("cli:demo", ActivationTrigger::Cli("demo".to_string())),
            (
                "http:/status",
                ActivationTrigger::Http("/status".to_string()),
            ),
            (
                "service:example.store.v1",
                ActivationTrigger::Service("example.store.v1".to_string()),
            ),
            (
                "topic:example.demo.changed",
                ActivationTrigger::Topic("example.demo.changed".to_string()),
            ),
            (
                "settings:example.tab",
                ActivationTrigger::Settings("example.tab".to_string()),
            ),
            (
                "catalog:example.colours",
                ActivationTrigger::Catalog("example.colours".to_string()),
            ),
            (
                "menu:example.menu",
                ActivationTrigger::Menu("example.menu".to_string()),
            ),
            (
                "virtual-rows:example.row",
                ActivationTrigger::VirtualRows("example.row".to_string()),
            ),
            (
                "language:example-lang",
                ActivationTrigger::Language("example-lang".to_string()),
            ),
            (
                "client-request:example.ask",
                ActivationTrigger::ClientRequest("example.ask".to_string()),
            ),
        ];
        for (text, expected_trigger) in expected_pairs {
            let trigger = parse_activation(text)
                .unwrap_or_else(|diagnostic| panic!("{text} must parse: {diagnostic}"));
            assert_eq!(trigger, expected_trigger, "wrong variant for {text}");
        }
    }

    /// REQ: PKG-059
    #[test]
    fn parse_activation_keeps_slashes_and_colons_in_the_name() {
        // The manual pattern is `<kind>:[^\s]+`, so the name is everything after the first colon.
        let trigger = parse_activation("event:a:b").expect("event:a:b is a valid trigger");
        assert_eq!(trigger, ActivationTrigger::Event("a:b".to_string()));
    }

    /// REQ: PKG-059
    #[test]
    fn parse_activation_refuses_unknown_kind_with_manifest_schema() {
        let diagnostic =
            parse_activation("unknown:example").expect_err("an unknown kind must be refused");
        assert_eq!(diagnostic.code.as_str(), "manifest.schema");
        assert_eq!(diagnostic.stage, Some(Stage(3)));
    }

    /// REQ: PKG-059
    #[test]
    fn parse_activation_refuses_empty_input() {
        let diagnostic = parse_activation("").expect_err("empty input must be refused");
        assert_eq!(diagnostic.code.as_str(), "manifest.schema");
    }

    /// REQ: PKG-059
    #[test]
    fn parse_activation_refuses_kind_without_name() {
        // A named kind needs `:` and a name without spaces.
        let diagnostic = parse_activation("event").expect_err("event needs a name");
        assert_eq!(diagnostic.code.as_str(), "manifest.schema");
        let diagnostic = parse_activation("event:").expect_err("an empty name must be refused");
        assert_eq!(diagnostic.code.as_str(), "manifest.schema");
    }

    /// REQ: PKG-059
    #[test]
    fn parse_activation_refuses_names_with_whitespace() {
        for text in [
            "event:a b",
            "event: tick",
            "event:tick\t",
            "event:tick\nnext",
        ] {
            let diagnostic =
                parse_activation(text).expect_err("a name containing whitespace must be refused");
            assert_eq!(diagnostic.code.as_str(), "manifest.schema", "for {text:?}");
        }
    }

    /// REQ: PKG-059
    #[test]
    fn parse_activation_refuses_name_on_startup_and_durable_timer() {
        for text in ["startup:example", "durable-timer:example"] {
            let diagnostic =
                parse_activation(text).expect_err("startup and durable-timer take no name");
            assert_eq!(diagnostic.code.as_str(), "manifest.schema", "for {text}");
        }
    }

    /// REQ: PKG-059
    #[test]
    fn parse_activation_kind_is_case_sensitive() {
        let diagnostic = parse_activation("Event:tick").expect_err("kinds are lower case");
        assert_eq!(diagnostic.code.as_str(), "manifest.schema");
    }

    /// REQ: PKG-561
    #[test]
    fn parse_activation_accepts_name_up_to_schema_length_limit() {
        // The schema caps the whole trigger string at 256 characters; `event:` is 6 of them.
        let name_of_250 = "a".repeat(250);
        let exact_limit = format!("event:{name_of_250}");
        assert_eq!(exact_limit.len(), 256);
        assert!(
            parse_activation(&exact_limit).is_ok(),
            "256 characters is the limit"
        );
    }

    /// REQ: PKG-561
    #[test]
    fn parse_activation_refuses_trigger_over_schema_length_limit() {
        let name_of_251 = "a".repeat(251);
        let over_limit = format!("event:{name_of_251}");
        assert_eq!(over_limit.len(), 257);
        let diagnostic = parse_activation(&over_limit)
            .expect_err("257 characters exceeds the schema maxLength of 256");
        assert_eq!(diagnostic.code.as_str(), "manifest.schema");
    }

    /// REQ: PKG-567
    #[test]
    fn parse_when_accepts_manual_example() {
        assert!(
            parse_when(r#"pane.primitive == "agent" && agent.activity != "working""#).is_ok(),
            "the manual example must parse"
        );
    }

    /// REQ: PKG-567
    #[test]
    fn parse_when_accepts_every_documented_key_and_value() {
        let expressions = [
            r#"pane.primitive == "basic""#,
            r#"pane.primitive == "terminal""#,
            r#"pane.primitive == "agent""#,
            r#"pane.primitive == "editor""#,
            r#"pane.primitive == "none""#,
            r#"pane.kind == "example.notes.note""#,
            r#"agent.activity == "idle""#,
            r#"agent.activity == "working""#,
            r#"agent.activity == "awaiting-approval""#,
            r#"agent.activity == "menu""#,
            r#"agent.activity == "done""#,
            r#"node.kind == "pane""#,
            r#"node.kind == "group""#,
            r#"node.kind == "folder""#,
            r#"node.kind == "project""#,
            r#"node.file_extension == "rs""#,
            r#"agent.signature == "claude-code""#,
            r#"view.focused == "true""#,
            r#"view.focused == "false""#,
            r#"display_mode == "normal""#,
            r#"display_mode == "attention""#,
        ];
        for expression in expressions {
            assert!(parse_when(expression).is_ok(), "must parse: {expression}");
        }
    }

    /// REQ: PKG-567
    #[test]
    fn parse_when_accepts_grouping_negation_and_or() {
        let expressions = [
            r#"!(pane.primitive == "terminal")"#,
            r#"(node.kind == "pane" || node.kind == "group") && view.focused == "true""#,
            r#"((pane.primitive == "agent"))"#,
            r#"display_mode == "attention" || !(agent.activity == "idle")"#,
        ];
        for expression in expressions {
            assert!(parse_when(expression).is_ok(), "must parse: {expression}");
        }
    }

    /// REQ: PKG-567
    #[test]
    fn parse_when_refuses_empty_expression_as_syntax_error() {
        let diagnostic = parse_when("").expect_err("empty expression is not valid");
        assert_eq!(diagnostic.code.as_str(), "manifest.when-syntax");
        assert_eq!(diagnostic.stage, Some(Stage(3)));
    }

    /// REQ: PKG-567
    #[test]
    fn parse_when_refuses_bad_syntax_with_when_syntax() {
        let expressions = [
            r#"pane.primitive == agent"#,
            r#"pane.primitive = "agent""#,
            r#"pane.primitive < "agent""#,
            r#"pane.primitive == "agent" &&"#,
            r#"(pane.primitive == "agent""#,
            r#"pane.primitive == "agent")"#,
            r#"pane.primitive"#,
        ];
        for expression in expressions {
            let diagnostic = parse_when(expression).expect_err("bad syntax must be refused");
            assert_eq!(
                diagnostic.code.as_str(),
                "manifest.when-syntax",
                "for {expression}"
            );
        }
    }

    /// REQ: PKG-567
    #[test]
    fn parse_when_refuses_unknown_value_with_when_syntax() {
        let diagnostic = parse_when(r#"pane.primitive == "widget""#)
            .expect_err("a value outside the documented set must be refused");
        assert_eq!(diagnostic.code.as_str(), "manifest.when-syntax");
    }

    /// REQ: PKG-567
    #[test]
    fn parse_when_refuses_unknown_key_with_when_key() {
        let expressions = [
            r#"colour.name == "red""#,
            r#"pane.primitive == "agent" && agent.colour == "red""#,
            r#"Pane.primitive == "agent""#,
        ];
        for expression in expressions {
            let diagnostic = parse_when(expression).expect_err("an unknown key must be refused");
            assert_eq!(
                diagnostic.code.as_str(),
                "manifest.when-key",
                "for {expression}"
            );
            assert_eq!(diagnostic.stage, Some(Stage(3)));
        }
    }

    /// Manual: modifier \"+\") key"
    #[test]
    fn parse_chord_accepts_single_key_and_named_keys() {
        assert!(parse_chord("p").is_ok(), "a single printable key");
        assert!(parse_chord("up").is_ok(), "a named key");
        assert!(parse_chord("enter").is_ok(), "a named key");
        assert!(parse_chord("page-up").is_ok(), "a named key with a hyphen");
        assert!(
            parse_chord("space").is_ok(),
            "the space bar is written space"
        );
        assert!(parse_chord("f1").is_ok(), "the lowest function key");
        assert!(parse_chord("f24").is_ok(), "the highest function key");
    }

    /// Manual: modifier \"+\") key"
    #[test]
    fn parse_chord_accepts_each_modifier_and_plus_spelled_as_word() {
        assert!(parse_chord("ctrl+p").is_ok());
        assert!(parse_chord("alt+c").is_ok());
        assert!(parse_chord("shift+tab").is_ok());
        assert!(parse_chord("super+k").is_ok());
        assert!(
            parse_chord("ctrl+plus").is_ok(),
            "the plus key is written plus"
        );
        assert!(parse_chord("ctrl+space").is_ok());
        assert!(
            parse_chord("shift+a").is_ok(),
            "shift plus a letter is written shift+a"
        );
    }

    /// Manual: modifier \"+\") key"
    #[test]
    fn parse_chord_splits_modifiers_then_key_in_any_order() {
        let canonical = parse_chord("ctrl+shift+p").expect("ctrl+shift+p is valid");
        let reordered = parse_chord("shift+ctrl+p").expect("shift+ctrl+p is valid");
        let mut canonical_modifiers = canonical.0[..canonical.0.len() - 1].to_vec();
        let mut reordered_modifiers = reordered.0[..reordered.0.len() - 1].to_vec();
        canonical_modifiers.sort();
        reordered_modifiers.sort();
        assert_eq!(
            canonical_modifiers,
            vec!["ctrl".to_string(), "shift".to_string()]
        );
        assert_eq!(reordered_modifiers, canonical_modifiers);
        assert_eq!(canonical.0.last().map(String::as_str), Some("p"));
        assert_eq!(reordered.0.last().map(String::as_str), Some("p"));
    }

    /// Manual: modifier \"+\") key"
    #[test]
    fn parse_chord_single_key_has_no_modifiers() {
        let chord = parse_chord("p").expect("p is valid");
        assert_eq!(chord, Chord(vec!["p".to_string()]));
    }

    /// Manual: modifier \"+\") key"
    #[test]
    fn parse_chord_refuses_empty_input() {
        parse_chord("").expect_err("an empty chord is outside the grammar");
    }

    /// Manual: modifier \"+\") key"
    #[test]
    fn parse_chord_refuses_dangling_or_bare_plus() {
        for chord in ["ctrl+", "+p", "+", "ctrl++", "ctrl+shift+"] {
            parse_chord(chord).expect_err("a plus must join a modifier and a key");
        }
    }

    /// Manual: modifier \"+\") key"
    #[test]
    fn parse_chord_refuses_unknown_modifier() {
        for chord in ["meta+p", "control+p", "cmd+p"] {
            parse_chord(chord).expect_err("only ctrl, alt, shift and super are modifiers");
        }
    }

    /// Manual: modifier \"+\") key"
    #[test]
    fn parse_chord_refuses_multi_character_or_unknown_key() {
        for chord in [
            "ctrl+ab",
            "ctrl+enterkey",
            "ctrl+f0",
            "f25",
            "ctrl+f25",
            "ctrl+arrow-up",
        ] {
            parse_chord(chord).expect_err("the key must be one printable character or a named key");
        }
    }

    /// Manual: modifier \"+\") key"
    #[test]
    fn parse_chord_refuses_upper_case() {
        // The grammar is lower case: shift plus a letter is shift+a, not shift+A or Ctrl+p.
        for chord in ["Ctrl+p", "ctrl+P", "shift+A"] {
            parse_chord(chord).expect_err("chords are written in lower case");
        }
    }
}
