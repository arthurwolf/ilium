//! The rule table of the `[text_triggers]` Settings tab: the rule fields, their defaults and the
//! validation a candidate list must pass before it replaces the active one.
//!
//! Pure data and rules moved from `ilium-ipc/src/text_trigger.rs` and `ilium-server/src/text_triggers.rs`
//! (`validate_settings`). Time is whole seconds, strings are plain `String`s.

use std::collections::HashSet;

use serde_json::{json, Value};

/// Seconds between a match and the send when the rule does not say otherwise.
pub const DEFAULT_DELAY_SECONDS: u32 = 60;
/// Longest accepted delay (24 hours).
pub const MAX_DELAY_SECONDS: u32 = 24 * 60 * 60;
/// Most rules one list may hold.
pub const MAX_RULES: usize = 4096;

/// Which terminal identities a rule may observe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Target {
    Agents,
    Terminals,
    #[default]
    Both,
}

impl Target {
    pub const ALL: [Self; 3] = [Self::Agents, Self::Terminals, Self::Both];

    pub fn label(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
            Self::Terminals => "Terminals",
            Self::Both => "Both",
        }
    }

    /// The stored (snake_case) value.
    pub fn value(self) -> &'static str {
        match self {
            Self::Agents => "agents",
            Self::Terminals => "terminals",
            Self::Both => "both",
        }
    }

    pub fn from_value(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.value() == value)
    }
}

/// One ordered rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub enabled: bool,
    pub regexp: String,
    pub message: String,
    pub target: Target,
    pub sample_text: String,
    pub delay_seconds: u32,
}

impl Default for Rule {
    fn default() -> Self {
        Self {
            id: String::new(),
            enabled: true,
            regexp: String::new(),
            message: String::new(),
            target: Target::Both,
            sample_text: String::new(),
            delay_seconds: DEFAULT_DELAY_SECONDS,
        }
    }
}

impl Rule {
    /// Reads one rule from its JSON object. A missing key takes the default, so a rule stored
    /// before the delay existed loads with sixty seconds.
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or_else(|| "a rule must be an object".to_string())?;
        let mut rule = Self::default();
        if let Some(id) = object.get("id") {
            rule.id = id.as_str().ok_or("id must be a string")?.to_string();
        }
        if let Some(enabled) = object.get("enabled") {
            rule.enabled = enabled.as_bool().ok_or("enabled must be a boolean")?;
        }
        if let Some(regexp) = object.get("regexp") {
            rule.regexp = regexp
                .as_str()
                .ok_or("regexp must be a string")?
                .to_string();
        }
        if let Some(message) = object.get("message") {
            rule.message = message
                .as_str()
                .ok_or("message must be a string")?
                .to_string();
        }
        if let Some(sample) = object.get("sample_text") {
            rule.sample_text = sample
                .as_str()
                .ok_or("sample_text must be a string")?
                .to_string();
        }
        if let Some(target) = object.get("target") {
            let text = target.as_str().ok_or("target must be a string")?;
            rule.target =
                Target::from_value(text).ok_or_else(|| format!("unknown target {text}"))?;
        }
        if let Some(delay) = object.get("delay_seconds") {
            let number = delay
                .as_u64()
                .ok_or("delay_seconds must be a whole number")?;
            rule.delay_seconds =
                u32::try_from(number).map_err(|_| "delay_seconds is too large".to_string())?;
        }
        Ok(rule)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "enabled": self.enabled,
            "regexp": self.regexp,
            "message": self.message,
            "target": self.target.value(),
            "sample_text": self.sample_text,
            "delay_seconds": self.delay_seconds,
        })
    }
}

/// Reads a `{"triggers": [...]}` table. An absent table is unspecified (`Ok(None)`); a table
/// without an explicit `triggers` list is an error; an explicit empty list clears.
pub fn parse_table(document: &Value) -> Result<Option<Vec<Rule>>, String> {
    let Some(table) = document.get("text_triggers") else {
        return Ok(None);
    };
    let list = table
        .get("triggers")
        .ok_or_else(|| "[text_triggers] must contain an explicit triggers list".to_string())?;
    let items = list
        .as_array()
        .ok_or_else(|| "invalid [text_triggers] table: triggers must be a list".to_string())?;
    items
        .iter()
        .map(Rule::from_json)
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
        .map_err(|error| format!("invalid [text_triggers] table: {error}"))
}

/// Checks authored identities and delays without normalizing them.
pub fn validate_identities(rules: &[Rule]) -> Result<(), String> {
    let mut ids = HashSet::new();
    for (index, rule) in rules.iter().enumerate() {
        if rule.id.trim().is_empty() || rule.id.chars().any(char::is_control) {
            return Err(format!(
                "Text Trigger {} has an invalid stable id",
                index + 1
            ));
        }
        if !ids.insert(rule.id.as_str()) {
            return Err(format!("Text Trigger {} repeats an existing id", index + 1));
        }
        if rule.delay_seconds > MAX_DELAY_SECONDS {
            return Err(format!(
                "Text Trigger {} delay exceeds {MAX_DELAY_SECONDS} seconds",
                index + 1
            ));
        }
    }
    Ok(())
}

/// Validates a candidate list before it replaces the active one. Messages stay literal single
/// lines so each rule has one text stage and one later Enter stage. `Some(reason)` rejects.
pub fn validate(rules: &[Rule]) -> Option<String> {
    if rules.len() > MAX_RULES {
        return Some(format!("at most {MAX_RULES} Text Triggers are allowed"));
    }
    if let Err(message) = validate_identities(rules) {
        return Some(message);
    }
    for (index, rule) in rules.iter().enumerate() {
        let number = index + 1;
        if rule.regexp.is_empty() {
            return Some(format!("Text Trigger {number} regexp must not be empty"));
        }
        if rule.delay_seconds > MAX_DELAY_SECONDS {
            return Some(format!(
                "Text Trigger {number} delay must be at most {MAX_DELAY_SECONDS} seconds"
            ));
        }
        if rule.message.contains(['\r', '\n']) {
            return Some(format!("Text Trigger {number} message must be one line"));
        }
        if let Err(error) = regex::Regex::new(&rule.regexp) {
            return Some(format!("Text Trigger {number} regexp is invalid: {error}"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str) -> Rule {
        Rule {
            id: id.to_string(),
            regexp: "ready$".to_string(),
            message: "go".to_string(),
            ..Rule::default()
        }
    }

    #[test]
    fn a_rule_stored_before_the_delay_setting_loads_with_sixty_seconds() {
        let document = json!({"text_triggers": {"triggers": [
            {"id": "old", "enabled": true, "regexp": "ready$", "message": "go", "target": "both", "sample_text": ""},
            {"id": "new", "regexp": "x", "message": "y", "delay_seconds": 5}
        ]}});
        let rules = parse_table(&document).unwrap().unwrap();
        assert_eq!(rules[0].delay_seconds, 60);
        assert_eq!(rules[1].delay_seconds, 5);
        assert_eq!(Rule::default().delay_seconds, 60);
    }

    #[test]
    fn absent_table_is_unspecified_and_explicit_empty_clears() {
        assert_eq!(parse_table(&json!({"sound": {}})).unwrap(), None);
        assert_eq!(
            parse_table(&json!({"text_triggers": {"triggers": []}})).unwrap(),
            Some(Vec::new())
        );
        assert!(parse_table(&json!({"text_triggers": {}})).is_err());
        assert!(parse_table(&json!({"text_triggers": {"triggers": 12}})).is_err());
    }

    #[test]
    fn rules_round_trip_through_json_including_zero_delay() {
        for delay in [0, 1, 90, MAX_DELAY_SECONDS] {
            let mut original = rule("r");
            original.delay_seconds = delay;
            original.target = Target::Agents;
            assert_eq!(Rule::from_json(&original.to_json()).unwrap(), original);
        }
    }

    #[test]
    fn authored_non_uuid_ids_are_valid_and_blank_control_duplicate_ids_are_not() {
        assert_eq!(validate_identities(&[rule("authored-rule")]), Ok(()));
        assert_eq!(validate_identities(&[]), Ok(()));
        for id in ["", "   ", "rule\nname", "\t"] {
            assert!(validate_identities(&[rule(id)]).is_err(), "{id:?}");
        }
        assert!(validate_identities(&[rule("stable"), rule("stable")]).is_err());
    }

    #[test]
    fn validation_rejects_each_invalid_candidate() {
        assert_eq!(validate(&[rule("a")]), None);
        let mut empty = rule("a");
        empty.regexp.clear();
        let mut late = rule("a");
        late.delay_seconds = MAX_DELAY_SECONDS + 1;
        let mut multiline = rule("a");
        multiline.message = "one\ntwo".to_string();
        let mut bad = rule("a");
        bad.regexp = "[".to_string();
        for candidate in [empty, late, multiline, bad] {
            assert!(validate(&[candidate]).is_some());
        }
        assert!(validate(&[rule("a"), rule("a")]).is_some());
    }

    #[test]
    fn unknown_target_and_wrong_types_are_refused() {
        assert!(Rule::from_json(&json!({"target": "everyone"})).is_err());
        assert!(Rule::from_json(&json!({"delay_seconds": -1})).is_err());
        assert!(Rule::from_json(&json!({"enabled": "yes"})).is_err());
        assert!(Rule::from_json(&json!(5)).is_err());
    }
}
