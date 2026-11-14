//! Evaluates the `when` context expressions of contributed chrome (reference-manifest.md
//! "The when expression", api-commands.md "Context expressions").
//!
//! The manifest validator (`ilium_plugin_api::manifest::syntax::parse_when`) already refused
//! bad syntax and unknown keys at install, so this module only decides whether an
//! expression holds for one context. It is pure: the caller builds a [`WhenContext`] from
//! the client's state and asks [`evaluate`].
//!
//! Rules:
//! - A comparison is `key == "text"` or `key != "text"`, joined with `&&`, `||`, `!` and
//!   parentheses; `&&` binds tighter than `||`.
//! - A key with no value in scope (no pane selected for a pane key, no agent for an agent
//!   key) makes its comparison false, for `==` and `!=` alike.
//! - An expression that does not parse is false: `when` only hides entries, so an entry with
//!   a broken expression stays hidden rather than showing where it should not.

/// The values of the context keys for one place in the UI. `None` is a key with nothing in
/// scope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WhenContext {
    /// `basic`, `terminal`, `agent`, `editor`, or `none` when no pane is in scope.
    pub pane_primitive: Option<String>,
    pub pane_kind: Option<String>,
    /// `idle`, `working`, `awaiting-approval`, `menu`, `done`.
    pub agent_activity: Option<String>,
    /// `pane`, `group`, `folder`, `project`.
    pub node_kind: Option<String>,
    /// Without the dot.
    pub node_file_extension: Option<String>,
    pub agent_signature: Option<String>,
    /// `true` or `false`.
    pub view_focused: Option<String>,
    /// `normal`, `attention` or a plugin mode id.
    pub display_mode: Option<String>,
}

impl WhenContext {
    fn value(&self, key: &str) -> Option<&str> {
        match key {
            "pane.primitive" => self.pane_primitive.as_deref(),
            "pane.kind" => self.pane_kind.as_deref(),
            "agent.activity" => self.agent_activity.as_deref(),
            "node.kind" => self.node_kind.as_deref(),
            "node.file_extension" => self.node_file_extension.as_deref(),
            "agent.signature" => self.agent_signature.as_deref(),
            "view.focused" => self.view_focused.as_deref(),
            "display_mode" => self.display_mode.as_deref(),
            _ => None,
        }
    }
}

/// Whether an entry with `when` shows in `context`. No expression always shows.
pub(crate) fn shows(when: Option<&str>, context: &WhenContext) -> bool {
    when.is_none_or(|expression| evaluate(expression, context))
}

/// Whether `expression` holds in `context`; false for an expression that does not parse.
pub(crate) fn evaluate(expression: &str, context: &WhenContext) -> bool {
    let mut evaluator = Evaluator {
        input: expression,
        offset: 0,
        depth: 0,
        context,
    };
    let Some(result) = evaluator.disjunction() else {
        return false;
    };
    evaluator.space();
    evaluator.offset == expression.len() && result
}

/// Nesting deeper than the validator allows is refused here too.
const MAXIMUM_DEPTH: usize = 32;

struct Evaluator<'a> {
    input: &'a str,
    offset: usize,
    depth: usize,
    context: &'a WhenContext,
}

impl Evaluator<'_> {
    fn rest(&self) -> &str {
        &self.input[self.offset..]
    }

    fn space(&mut self) {
        let trimmed = self.rest().trim_start();
        self.offset = self.input.len() - trimmed.len();
    }

    fn take(&mut self, token: &str) -> bool {
        self.space();
        if self.rest().starts_with(token) {
            self.offset += token.len();
            true
        } else {
            false
        }
    }

    /// Evaluates every operand, never short-circuiting, so a syntax error later in the
    /// expression is still found.
    fn disjunction(&mut self) -> Option<bool> {
        let mut result = self.conjunction()?;
        while self.take("||") {
            result |= self.conjunction()?;
        }
        Some(result)
    }

    fn conjunction(&mut self) -> Option<bool> {
        let mut result = self.primary()?;
        while self.take("&&") {
            result &= self.primary()?;
        }
        Some(result)
    }

    fn primary(&mut self) -> Option<bool> {
        if self.depth >= MAXIMUM_DEPTH {
            return None;
        }
        self.depth += 1;
        let result = self.primary_inner();
        self.depth -= 1;
        result
    }

    fn primary_inner(&mut self) -> Option<bool> {
        // `!=` never starts a primary, so a leading `!` is always negation.
        if self.take("!") {
            return self.primary().map(|value| !value);
        }
        if self.take("(") {
            let value = self.disjunction()?;
            return self.take(")").then_some(value);
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Option<bool> {
        self.space();
        let key_length = self
            .rest()
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
            .unwrap_or(self.rest().len());
        if key_length == 0 {
            return None;
        }
        let key = &self.input[self.offset..self.offset + key_length];
        self.offset += key_length;
        let equal = if self.take("==") {
            true
        } else if self.take("!=") {
            false
        } else {
            return None;
        };
        let text = self.string()?;
        Some(
            self.context
                .value(key)
                .is_some_and(|value| (value == text) == equal),
        )
    }

    /// A JSON string literal, decoded.
    fn string(&mut self) -> Option<String> {
        self.space();
        if !self.rest().starts_with('"') {
            return None;
        }
        let mut escaped = false;
        for (index, character) in self.rest().char_indices().skip(1) {
            if character == '"' && !escaped {
                let literal = &self.rest()[..=index];
                let decoded = serde_json::from_str::<String>(literal).ok()?;
                self.offset += index + 1;
                return Some(decoded);
            }
            escaped = character == '\\' && !escaped;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent_pane() -> WhenContext {
        WhenContext {
            pane_primitive: Some("agent".to_owned()),
            pane_kind: Some("terminal".to_owned()),
            agent_activity: Some("working".to_owned()),
            node_kind: Some("pane".to_owned()),
            node_file_extension: None,
            agent_signature: Some("claude".to_owned()),
            view_focused: Some("false".to_owned()),
            display_mode: Some("normal".to_owned()),
        }
    }

    #[test]
    fn the_manual_example_holds_only_while_the_agent_is_not_working() {
        let expression = r#"pane.primitive == "agent" && agent.activity != "working""#;
        let mut context = agent_pane();
        assert!(!evaluate(expression, &context));
        context.agent_activity = Some("idle".to_owned());
        assert!(evaluate(expression, &context));
    }

    #[test]
    fn and_binds_tighter_than_or_and_parentheses_and_negation_group() {
        let context = agent_pane();
        assert!(evaluate(
            r#"node.kind == "group" || node.kind == "pane" && agent.signature == "claude""#,
            &context
        ));
        assert!(!evaluate(
            r#"(node.kind == "group" || node.kind == "pane") && agent.signature == "codex""#,
            &context
        ));
        assert!(evaluate(r#"!(agent.signature == "codex")"#, &context));
        assert!(evaluate(r#"!!(display_mode == "normal")"#, &context));
    }

    #[test]
    fn a_key_with_nothing_in_scope_is_false_for_both_comparisons() {
        let group = WhenContext {
            pane_primitive: Some("none".to_owned()),
            node_kind: Some("group".to_owned()),
            ..WhenContext::default()
        };
        assert!(!evaluate(r#"agent.activity == "idle""#, &group));
        assert!(!evaluate(r#"agent.activity != "idle""#, &group));
        assert!(!evaluate(r#"node.file_extension != "md""#, &group));
        assert!(evaluate(r#"pane.primitive == "none""#, &group));
        // Negating a false comparison shows the entry, as the grammar says.
        assert!(evaluate(r#"!(agent.activity == "idle")"#, &group));
    }

    #[test]
    fn a_broken_expression_hides_its_entry() {
        let context = agent_pane();
        for broken in [
            "",
            "node.kind",
            r#"node.kind == pane"#,
            r#"node.kind == "pane" &&"#,
            r#"(node.kind == "pane""#,
            r#"node.kind == "pane" extra"#,
            r#"node.kind = "pane""#,
        ] {
            assert!(!evaluate(broken, &context), "{broken:?}");
        }
        let deep = format!("{}node.kind == \"pane\"{}", "(".repeat(40), ")".repeat(40));
        assert!(!evaluate(&deep, &context));
    }

    #[test]
    fn escaped_quotes_in_values_decode_like_json() {
        let context = WhenContext {
            pane_kind: Some("say \"hi\"".to_owned()),
            ..WhenContext::default()
        };
        assert!(evaluate(r#"pane.kind == "say \"hi\"""#, &context));
    }

    #[test]
    fn no_expression_always_shows() {
        assert!(shows(None, &WhenContext::default()));
        assert!(!shows(
            Some(r#"view.focused == "true""#),
            &WhenContext::default()
        ));
    }
}
