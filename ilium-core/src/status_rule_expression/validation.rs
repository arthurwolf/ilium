//! Typed validation of a status ruleset.
//!
//! Core checks structure and parameter ranges. Regex syntax and terminal cell
//! widths are checked by the client adapters, which report through the same
//! [`ValidationIssue`] type so the editor can attach every issue to its rule.

use std::collections::HashSet;
use std::fmt;

use super::{shadowed_by, Condition, ScreenPattern, ScreenRegion, StatusRules};

pub const MAXIMUM_RULES: usize = 64;
pub const MAXIMUM_GROUPS_PER_RULE: usize = 8;
pub const MAXIMUM_CONDITIONS_PER_GROUP: usize = 8;
pub const MAXIMUM_CONDITIONS_PER_RULE: usize = 64;
pub const MAXIMUM_ENABLED_SCREEN_PATTERNS: usize = 16;
pub const MAXIMUM_PATTERN_CHARS: usize = 256;
pub const MAXIMUM_NAME_CHARS: usize = 48;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IssueSeverity {
    /// The ruleset must not be saved or applied.
    Error,
    /// The ruleset works, but the user should know.
    Warning,
}

/// One validation finding, attached to a rule when it concerns one rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationIssue {
    /// Index into `StatusRules::rules`; `None` for ruleset-level findings.
    pub rule: Option<usize>,
    pub severity: IssueSeverity,
    pub message: String,
}

impl ValidationIssue {
    pub fn error(rule: Option<usize>, message: impl Into<String>) -> Self {
        Self {
            rule,
            severity: IssueSeverity::Error,
            message: message.into(),
        }
    }

    pub fn warning(rule: Option<usize>, message: impl Into<String>) -> Self {
        Self {
            rule,
            severity: IssueSeverity::Warning,
            message: message.into(),
        }
    }

    /// A problem inside a rule's conditions or visual. A disabled rule cannot
    /// fire, so its content problems are warnings: a configuration loaded
    /// with a broken rule disabled stays saveable, and enabling the rule
    /// turns the same finding back into an error.
    pub fn content(rule: usize, enabled: bool, message: impl Into<String>) -> Self {
        if enabled {
            Self::error(Some(rule), message)
        } else {
            Self::warning(Some(rule), format!("Disabled rule: {}", message.into()))
        }
    }

    pub fn is_error(&self) -> bool {
        self.severity == IssueSeverity::Error
    }
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

pub fn has_errors(issues: &[ValidationIssue]) -> bool {
    issues.iter().any(ValidationIssue::is_error)
}

/// Error messages joined for a one-line report; warnings are left out.
pub fn error_summary(issues: &[ValidationIssue]) -> String {
    issues
        .iter()
        .filter(|issue| issue.is_error())
        .map(|issue| issue.message.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}

/// Validate structural limits and parameter ranges, and warn about rules that
/// can never match. The result holds errors and warnings; use [`has_errors`]
/// to decide whether the ruleset may be saved.
pub fn validate(rules: &StatusRules) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let mut ids = HashSet::new();
    let mut enabled_watches: Vec<&ScreenPattern> = Vec::new();
    if rules.rules.len() > MAXIMUM_RULES {
        issues.push(ValidationIssue::error(
            None,
            format!("A ruleset may contain at most {MAXIMUM_RULES} rules"),
        ));
    }
    for (index, rule) in rules.rules.iter().enumerate() {
        let error = |message: String| ValidationIssue::error(Some(index), message);
        let content = |message: String| ValidationIssue::content(index, rule.enabled, message);
        if rule.id.is_empty() || !ids.insert(&rule.id) {
            issues.push(error(format!(
                "Rule {:?} has an empty or duplicate identifier",
                rule.name
            )));
        }
        if rule.name.chars().count() > MAXIMUM_NAME_CHARS {
            issues.push(error(format!(
                "Rule {:?} has a name longer than {MAXIMUM_NAME_CHARS} characters",
                rule.id
            )));
        }
        if rule.any_of.is_empty() {
            issues.push(ValidationIssue::warning(
                Some(index),
                format!("Rule {:?} has no conditions and never matches", rule.name),
            ));
        }
        if rule.any_of.iter().any(|group| group.all_of.is_empty()) {
            issues.push(error(format!(
                "Rule {:?} contains an empty AND group",
                rule.name
            )));
        }
        if rule.any_of.len() > MAXIMUM_GROUPS_PER_RULE
            || rule
                .any_of
                .iter()
                .any(|group| group.all_of.len() > MAXIMUM_CONDITIONS_PER_GROUP)
        {
            issues.push(error(format!(
                "Rule {:?} exceeds the {MAXIMUM_GROUPS_PER_RULE}-group or {MAXIMUM_CONDITIONS_PER_GROUP}-condition limit",
                rule.name
            )));
        }
        if rule
            .any_of
            .iter()
            .map(|group| group.all_of.len())
            .sum::<usize>()
            > MAXIMUM_CONDITIONS_PER_RULE
        {
            issues.push(error(format!(
                "Rule {:?} has more than {MAXIMUM_CONDITIONS_PER_RULE} conditions",
                rule.name
            )));
        }
        for term in rule.any_of.iter().flat_map(|group| &group.all_of) {
            if let Condition::ScreenMatches(pattern) = &term.condition {
                if rule.enabled && !enabled_watches.contains(&pattern) {
                    enabled_watches.push(pattern);
                }
            }
            match &term.condition {
                Condition::ProgressAtLeast(value) | Condition::ProgressBelow(value)
                    if *value > 100 =>
                {
                    issues.push(content(format!(
                        "Rule {:?} has an invalid progress percentage",
                        rule.name
                    )));
                }
                Condition::ProgressBetween { minimum, maximum }
                    if minimum > maximum || *maximum > 100 =>
                {
                    issues.push(content(format!(
                        "Rule {:?} has an invalid progress range",
                        rule.name
                    )));
                }
                Condition::ScreenMatches(pattern)
                    if pattern.pattern.chars().count() > MAXIMUM_PATTERN_CHARS =>
                {
                    issues.push(content(format!(
                        "Rule {:?} has a screen pattern longer than {MAXIMUM_PATTERN_CHARS} characters",
                        rule.name
                    )));
                }
                Condition::ScreenMatches(pattern) if pattern.pattern.trim().is_empty() => {
                    issues.push(content(format!(
                        "Rule {:?} has an empty screen pattern",
                        rule.name
                    )));
                }
                Condition::ScreenMatches(ScreenPattern {
                    region: ScreenRegion::FirstLines(0) | ScreenRegion::LastLines(0),
                    ..
                }) => {
                    issues.push(content(format!(
                        "Rule {:?} restricts the screen pattern to zero lines",
                        rule.name
                    )));
                }
                _ => {}
            }
        }
    }
    if enabled_watches.len() > MAXIMUM_ENABLED_SCREEN_PATTERNS {
        issues.push(ValidationIssue::error(
            None,
            format!("A ruleset may enable at most {MAXIMUM_ENABLED_SCREEN_PATTERNS} distinct screen patterns"),
        ));
    }
    for index in 0..rules.rules.len() {
        if let Some(earlier) = shadowed_by(&rules.rules, index) {
            issues.push(ValidationIssue::warning(
                Some(index),
                format!(
                    "Rule {:?} can never match: rule {:?} above it already covers every situation it describes",
                    rules.rules[index].name, rules.rules[earlier].name
                ),
            ));
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::{error_summary, has_errors, validate, IssueSeverity, ValidationIssue};
    use crate::status_rule_expression::{
        default_rules, AllOf, Condition, ConditionTerm, Rule, ScreenPattern, ScreenRegion,
        StatusRules, StatusVisual, VisualKind, VisualStyle,
    };

    fn rule(id: &str, condition: Condition) -> Rule<Condition, StatusVisual> {
        Rule {
            id: id.to_owned(),
            name: id.to_owned(),
            enabled: true,
            any_of: vec![AllOf {
                all_of: vec![ConditionTerm {
                    condition,
                    negated: false,
                }],
            }],
            then: StatusVisual {
                kind: VisualKind::Hidden,
                style: VisualStyle::default(),
            },
        }
    }

    fn ruleset(rules: Vec<Rule<Condition, StatusVisual>>) -> StatusRules {
        StatusRules {
            defaults_version: 1,
            rules,
            fallback: None,
        }
    }

    #[test]
    fn validation_rejects_invalid_progress_and_duplicate_ids_with_rule_indices() {
        let rules = ruleset(vec![
            rule("one", Condition::ProgressAtLeast(101)),
            rule("one", Condition::Always),
        ]);
        let issues = validate(&rules);
        assert!(has_errors(&issues));
        let duplicate = issues
            .iter()
            .find(|issue| issue.message.contains("duplicate"))
            .unwrap();
        assert_eq!(
            (duplicate.rule, duplicate.severity),
            (Some(1), IssueSeverity::Error)
        );
        let percentage = issues
            .iter()
            .find(|issue| issue.message.contains("percentage"))
            .unwrap();
        assert_eq!(percentage.rule, Some(0));
        assert!(error_summary(&issues).contains("duplicate"));
        assert!(error_summary(&issues).contains("percentage"));
    }

    #[test]
    fn content_problems_in_a_disabled_rule_are_warnings_until_it_is_enabled() {
        let mut rules = ruleset(vec![rule(
            "broken",
            Condition::ProgressBetween {
                minimum: 80,
                maximum: 20,
            },
        )]);
        assert!(has_errors(&validate(&rules)));
        rules.rules[0].enabled = false;
        let issues = validate(&rules);
        assert!(!has_errors(&issues));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].rule, Some(0));
        assert!(issues[0].message.starts_with("Disabled rule: "));
        assert_eq!(error_summary(&issues), "");
    }

    #[test]
    fn screen_patterns_must_select_text_and_be_bounded() {
        let pattern = |pattern: &str, region| {
            Condition::ScreenMatches(ScreenPattern {
                pattern: pattern.to_owned(),
                ignore_case: true,
                region,
            })
        };
        let zero_lines = ruleset(vec![rule(
            "zero",
            pattern("error", ScreenRegion::LastLines(0)),
        )]);
        assert!(validate(&zero_lines)
            .iter()
            .any(|issue| issue.is_error() && issue.message.contains("zero lines")));
        let empty = ruleset(vec![rule("empty", pattern("  ", ScreenRegion::All))]);
        assert!(validate(&empty)
            .iter()
            .any(|issue| issue.is_error() && issue.message.contains("empty screen pattern")));
        let long = ruleset(vec![rule(
            "long",
            pattern(&"a".repeat(257), ScreenRegion::FirstLines(3)),
        )]);
        assert!(validate(&long)
            .iter()
            .any(|issue| issue.is_error() && issue.message.contains("256")));
        let fine = ruleset(vec![rule(
            "fine",
            pattern("error", ScreenRegion::FirstLines(3)),
        )]);
        assert!(validate(&fine).is_empty());
    }

    #[test]
    fn screen_watch_limit_counts_distinct_enabled_patterns() {
        let mut rules = default_rules();
        let example_index = rules
            .rules
            .iter()
            .position(|rule| rule.id == "screen-error-example")
            .unwrap();
        let mut example = rules.rules.remove(example_index);
        example.enabled = true;
        for index in 0..17 {
            let mut rule = example.clone();
            rule.id = format!("screen-{index}");
            rule.any_of[0].all_of[0].condition = Condition::ScreenMatches(ScreenPattern {
                pattern: "shared".to_owned(),
                ignore_case: false,
                region: ScreenRegion::All,
            });
            rules.rules.push(rule);
        }
        assert!(!has_errors(&validate(&rules)));
        for (index, rule) in rules.rules.iter_mut().rev().take(17).enumerate() {
            if let Condition::ScreenMatches(pattern) = &mut rule.any_of[0].all_of[0].condition {
                pattern.pattern = format!("pattern-{index}");
            }
        }
        let issues = validate(&rules);
        assert!(issues.iter().any(|issue| issue.is_error()
            && issue.rule.is_none()
            && issue.message.contains("16 distinct")));
        rules.rules.last_mut().unwrap().enabled = false;
        assert!(!has_errors(&validate(&rules)));
    }

    #[test]
    fn unreachable_and_empty_rules_are_warnings_not_errors() {
        let mut rules = ruleset(vec![
            rule("broad", Condition::Always),
            rule("narrow", Condition::Always),
        ]);
        rules.rules.push(Rule {
            any_of: Vec::new(),
            ..rule("empty", Condition::Always)
        });
        let issues = validate(&rules);
        assert!(!has_errors(&issues));
        let shadow = issues
            .iter()
            .find(|issue| issue.message.contains("can never match"))
            .unwrap();
        assert_eq!(shadow.rule, Some(1));
        assert!(shadow.message.contains("\"broad\""));
        assert!(issues
            .iter()
            .any(|issue| issue.rule == Some(2) && issue.message.contains("no conditions")));
        assert_eq!(
            issues
                .iter()
                .filter(|issue| issue.message.contains("can never match"))
                .count(),
            1
        );
    }

    #[test]
    fn limits_are_enforced_at_ruleset_level() {
        let rules = ruleset(
            (0..65)
                .map(|index| rule(&format!("rule-{index}"), Condition::Always))
                .collect(),
        );
        let issues = validate(&rules);
        assert!(issues
            .iter()
            .any(|issue| issue.rule.is_none() && issue.message.contains("64 rules")));
        let mut named = ruleset(vec![rule("long-name", Condition::Always)]);
        named.rules[0].name = "n".repeat(49);
        assert!(validate(&named)
            .iter()
            .any(|issue| issue.rule == Some(0) && issue.message.contains("48 characters")));
        let mut empty_group = ruleset(vec![rule("empty-group", Condition::Always)]);
        empty_group.rules[0]
            .any_of
            .push(AllOf { all_of: Vec::new() });
        assert!(validate(&empty_group)
            .iter()
            .any(|issue| issue.is_error() && issue.message.contains("empty AND group")));
    }

    #[test]
    fn issue_display_is_the_message() {
        let issue = ValidationIssue::warning(None, "Example");
        assert_eq!(issue.to_string(), "Example");
        assert!(!issue.is_error());
    }
}
