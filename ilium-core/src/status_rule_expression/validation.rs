//! Typed validation of a status ruleset.
//!
//! Core checks structure and parameter ranges. Regex syntax and terminal cell
//! widths are checked by the client adapters, which report through the same
//! [`ValidationIssue`] type so the editor can attach every issue to its rule
//! and, through [`TermPath`], to the offending AND group or condition without
//! parsing the message.

use std::collections::HashSet;
use std::fmt;

use super::{shadowed_by, AllOf, Condition, ScreenPattern, ScreenRegion, StatusRules};

pub const MAXIMUM_RULES: usize = 64;
pub const MAXIMUM_GROUPS_PER_RULE: usize = 8;
pub const MAXIMUM_CONDITIONS_PER_GROUP: usize = 8;
pub const MAXIMUM_CONDITIONS_PER_RULE: usize = 64;
/// Must stay equal to the server's screen-watch budget
/// (`ilium-server` `status_screen_watch::MAX_SCREEN_WATCHES`).
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

/// What a finding is about, independent of its wording. The editor and the
/// load banner branch on this, never on the message text. Load repairs reuse
/// the kind of the problem they repaired; the message says what was changed.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IssueKind {
    /// More rules than [`MAXIMUM_RULES`], or readable rules beyond the limit
    /// left unloaded in the file.
    TooManyRules,
    /// An empty or duplicate rule identifier.
    InvalidIdentifier,
    /// A name longer than [`MAXIMUM_NAME_CHARS`].
    NameTooLong,
    /// A rule without any condition; it never matches (warning).
    NoConditions,
    /// An AND group without a condition.
    EmptyGroup,
    /// More AND groups than [`MAXIMUM_GROUPS_PER_RULE`].
    TooManyGroups,
    /// An AND group with more than [`MAXIMUM_CONDITIONS_PER_GROUP`] conditions.
    GroupTooLarge,
    /// More conditions in one rule than [`MAXIMUM_CONDITIONS_PER_RULE`].
    TooManyConditions,
    /// A progress percentage above 100.
    ProgressOutOfRange,
    /// A progress range whose minimum exceeds its maximum or 100.
    InvalidProgressRange,
    /// A context-window percentage above 100.
    ContextOutOfRange,
    /// A screen pattern longer than [`MAXIMUM_PATTERN_CHARS`].
    ScreenPatternTooLong,
    /// A blank screen pattern.
    ScreenPatternEmpty,
    /// A screen region of zero lines.
    ScreenRegionEmpty,
    /// A screen pattern that does not compile (checked by the client).
    ScreenPatternInvalid,
    /// More distinct enabled screen patterns than the server can watch.
    TooManyScreenPatterns,
    /// A rule an earlier rule always pre-empts (warning).
    Unreachable,
    /// The ruleset has rules but none is enabled, so only the fallback can
    /// show (warning).
    NoRuleEnabled,
    /// A rule's visual does not fit the status slot (checked by the client).
    InvalidVisual,
    /// The fallback visual is unreadable or does not fit the status slot.
    InvalidFallback,
    /// The file was written by a newer format version (load warning).
    NewerFormat,
    /// A rule entry in the file could not be read and was kept as is
    /// (load warning).
    UnreadableRule,
}

/// Where inside a rule a finding sits: an AND group (`term: None`) or one
/// condition of that group. Indices are zero-based into `Rule::any_of` and
/// `AllOf::all_of`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TermPath {
    pub group: usize,
    pub term: Option<usize>,
}

impl TermPath {
    /// The position in the editor's flat one-line form (`to_line`): the
    /// condition itself, or the group's first condition. `None` when the
    /// path does not exist in `groups`.
    pub fn flat_index<C>(&self, groups: &[AllOf<C>]) -> Option<usize> {
        let group = groups.get(self.group)?;
        let within = self.term.unwrap_or(0);
        if within >= group.all_of.len().max(1) {
            return None;
        }
        let before: usize = groups[..self.group]
            .iter()
            .map(|group| group.all_of.len())
            .sum();
        Some(before + within)
    }
}

/// One validation finding, attached to a rule when it concerns one rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationIssue {
    /// Index into `StatusRules::rules`; `None` for ruleset-level findings.
    pub rule: Option<usize>,
    /// The group or condition inside the rule, when the finding has one.
    pub path: Option<TermPath>,
    pub kind: IssueKind,
    pub severity: IssueSeverity,
    pub message: String,
}

impl ValidationIssue {
    pub fn error(kind: IssueKind, rule: Option<usize>, message: impl Into<String>) -> Self {
        Self {
            rule,
            path: None,
            kind,
            severity: IssueSeverity::Error,
            message: message.into(),
        }
    }

    pub fn warning(kind: IssueKind, rule: Option<usize>, message: impl Into<String>) -> Self {
        Self {
            rule,
            path: None,
            kind,
            severity: IssueSeverity::Warning,
            message: message.into(),
        }
    }

    /// A problem inside a rule's conditions or visual. A disabled rule cannot
    /// fire, so its content problems are warnings: a configuration loaded
    /// with a broken rule disabled stays saveable, and enabling the rule
    /// turns the same finding back into an error.
    pub fn content(
        kind: IssueKind,
        rule: usize,
        enabled: bool,
        message: impl Into<String>,
    ) -> Self {
        if enabled {
            Self::error(kind, Some(rule), message)
        } else {
            Self::warning(
                kind,
                Some(rule),
                format!("Disabled rule: {}", message.into()),
            )
        }
    }

    /// Attach the finding to one AND group (`term: None`) or condition.
    pub fn at(mut self, group: usize, term: Option<usize>) -> Self {
        self.path = Some(TermPath { group, term });
        self
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

/// The range or size problem of one condition, if any.
fn condition_problem(condition: &Condition, rule_name: &str) -> Option<(IssueKind, String)> {
    match condition {
        Condition::ProgressAtLeast(value) | Condition::ProgressBelow(value) if *value > 100 => {
            Some((
                IssueKind::ProgressOutOfRange,
                format!("Rule {rule_name:?} has an invalid progress percentage"),
            ))
        }
        Condition::ContextAbovePercent(value) if *value > 100 => Some((
            IssueKind::ContextOutOfRange,
            format!("Rule {rule_name:?} has an invalid context-window percentage"),
        )),
        Condition::ProgressBetween { minimum, maximum } if minimum > maximum || *maximum > 100 => {
            Some((
                IssueKind::InvalidProgressRange,
                format!("Rule {rule_name:?} has an invalid progress range"),
            ))
        }
        Condition::ScreenMatches(pattern)
            if pattern.pattern.chars().count() > MAXIMUM_PATTERN_CHARS =>
        {
            Some((
                IssueKind::ScreenPatternTooLong,
                format!(
                    "Rule {rule_name:?} has a screen pattern longer than {MAXIMUM_PATTERN_CHARS} characters"
                ),
            ))
        }
        Condition::ScreenMatches(pattern) if pattern.pattern.trim().is_empty() => Some((
            IssueKind::ScreenPatternEmpty,
            format!("Rule {rule_name:?} has an empty screen pattern"),
        )),
        Condition::ScreenMatches(ScreenPattern {
            region: ScreenRegion::FirstLines(0) | ScreenRegion::LastLines(0),
            ..
        }) => Some((
            IssueKind::ScreenRegionEmpty,
            format!("Rule {rule_name:?} restricts the screen pattern to zero lines"),
        )),
        _ => None,
    }
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
            IssueKind::TooManyRules,
            None,
            format!("A ruleset may contain at most {MAXIMUM_RULES} rules"),
        ));
    }
    for (index, rule) in rules.rules.iter().enumerate() {
        let error = |kind, message: String| ValidationIssue::error(kind, Some(index), message);
        if rule.id.is_empty() || !ids.insert(&rule.id) {
            issues.push(error(
                IssueKind::InvalidIdentifier,
                format!("Rule {:?} has an empty or duplicate identifier", rule.name),
            ));
        }
        if rule.name.chars().count() > MAXIMUM_NAME_CHARS {
            issues.push(error(
                IssueKind::NameTooLong,
                format!(
                    "Rule {:?} has a name longer than {MAXIMUM_NAME_CHARS} characters",
                    rule.id
                ),
            ));
        }
        if rule.any_of.is_empty() {
            issues.push(ValidationIssue::warning(
                IssueKind::NoConditions,
                Some(index),
                format!("Rule {:?} has no conditions and never matches", rule.name),
            ));
        }
        if rule.any_of.len() > MAXIMUM_GROUPS_PER_RULE {
            issues.push(error(
                IssueKind::TooManyGroups,
                format!(
                    "Rule {:?} exceeds the {MAXIMUM_GROUPS_PER_RULE}-group limit",
                    rule.name
                ),
            ));
        }
        if rule
            .any_of
            .iter()
            .map(|group| group.all_of.len())
            .sum::<usize>()
            > MAXIMUM_CONDITIONS_PER_RULE
        {
            issues.push(error(
                IssueKind::TooManyConditions,
                format!(
                    "Rule {:?} has more than {MAXIMUM_CONDITIONS_PER_RULE} conditions",
                    rule.name
                ),
            ));
        }
        for (group_index, group) in rule.any_of.iter().enumerate() {
            if group.all_of.is_empty() {
                issues.push(
                    error(
                        IssueKind::EmptyGroup,
                        format!("Rule {:?} contains an empty AND group", rule.name),
                    )
                    .at(group_index, None),
                );
            }
            if group.all_of.len() > MAXIMUM_CONDITIONS_PER_GROUP {
                issues.push(
                    error(
                        IssueKind::GroupTooLarge,
                        format!(
                            "Rule {:?} exceeds the {MAXIMUM_CONDITIONS_PER_GROUP}-condition limit of an AND group",
                            rule.name
                        ),
                    )
                    .at(group_index, None),
                );
            }
            for (term_index, term) in group.all_of.iter().enumerate() {
                if let Condition::ScreenMatches(pattern) = &term.condition {
                    if rule.enabled && !enabled_watches.contains(&pattern) {
                        enabled_watches.push(pattern);
                    }
                }
                if let Some((kind, message)) = condition_problem(&term.condition, &rule.name) {
                    issues.push(
                        ValidationIssue::content(kind, index, rule.enabled, message)
                            .at(group_index, Some(term_index)),
                    );
                }
            }
        }
    }
    if enabled_watches.len() > MAXIMUM_ENABLED_SCREEN_PATTERNS {
        issues.push(ValidationIssue::error(
            IssueKind::TooManyScreenPatterns,
            None,
            format!("A ruleset may enable at most {MAXIMUM_ENABLED_SCREEN_PATTERNS} distinct screen patterns"),
        ));
    }
    if !rules.rules.is_empty() && rules.rules.iter().all(|rule| !rule.enabled) {
        issues.push(ValidationIssue::warning(
            IssueKind::NoRuleEnabled,
            None,
            "Every rule is disabled, so panes show only the fallback icon",
        ));
    }
    for index in 0..rules.rules.len() {
        if let Some(earlier) = shadowed_by(&rules.rules, index) {
            issues.push(ValidationIssue::warning(
                IssueKind::Unreachable,
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
    use super::{
        error_summary, has_errors, validate, IssueKind, IssueSeverity, TermPath, ValidationIssue,
    };
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
            version: 1,
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
    fn context_percentage_accepts_the_boundaries_and_rejects_above_100() {
        for valid in [0, 100] {
            let issues = validate(&ruleset(vec![rule(
                "context",
                Condition::ContextAbovePercent(valid),
            )]));
            assert!(!has_errors(&issues), "{valid}% must be valid: {issues:?}");
        }
        let issues = validate(&ruleset(vec![rule(
            "context",
            Condition::ContextAbovePercent(101),
        )]));
        let issue = issues
            .iter()
            .find(|issue| issue.kind == IssueKind::ContextOutOfRange)
            .expect("101% is out of range");
        assert_eq!(
            (issue.rule, issue.severity),
            (Some(0), IssueSeverity::Error)
        );
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
        // The rule's own finding, plus the ruleset-level "nothing enabled".
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].rule, Some(0));
        assert_eq!(issues[0].kind, IssueKind::InvalidProgressRange);
        assert_eq!(
            issues[0].path,
            Some(TermPath {
                group: 0,
                term: Some(0)
            })
        );
        assert!(issues[0].message.starts_with("Disabled rule: "));
        assert_eq!(issues[1].kind, IssueKind::NoRuleEnabled);
        assert_eq!(issues[1].rule, None);
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
        let issue = ValidationIssue::warning(IssueKind::NoConditions, None, "Example");
        assert_eq!(issue.to_string(), "Example");
        assert!(!issue.is_error());
    }

    #[test]
    fn issues_carry_a_kind_and_the_path_of_the_offending_term() {
        let mut rules = ruleset(vec![rule("first", Condition::Always)]);
        rules.rules[0].any_of.push(AllOf {
            all_of: vec![
                ConditionTerm {
                    condition: Condition::Always,
                    negated: false,
                },
                ConditionTerm {
                    condition: Condition::ProgressBelow(150),
                    negated: false,
                },
            ],
        });
        rules.rules[0].any_of.push(AllOf { all_of: Vec::new() });
        let issues = validate(&rules);
        let range = issues
            .iter()
            .find(|issue| issue.kind == IssueKind::ProgressOutOfRange)
            .expect("range issue");
        assert_eq!(
            (range.rule, range.severity),
            (Some(0), IssueSeverity::Error)
        );
        let path = range.path.expect("term path");
        assert_eq!(
            path,
            TermPath {
                group: 1,
                term: Some(1)
            }
        );
        // Flat editor line: [always] OR [always AND progress<150] OR [].
        assert_eq!(path.flat_index(&rules.rules[0].any_of), Some(2));
        let empty = issues
            .iter()
            .find(|issue| issue.kind == IssueKind::EmptyGroup)
            .expect("empty group issue");
        assert_eq!(
            empty.path,
            Some(TermPath {
                group: 2,
                term: None
            })
        );
        // An empty group has no term of its own; its flat position is where
        // its first condition would go.
        assert_eq!(
            empty.path.unwrap().flat_index(&rules.rules[0].any_of),
            Some(3)
        );
        assert_eq!(
            TermPath {
                group: 9,
                term: None
            }
            .flat_index(&rules.rules[0].any_of),
            None
        );
        assert_eq!(
            TermPath {
                group: 0,
                term: Some(1)
            }
            .flat_index(&rules.rules[0].any_of),
            None
        );
    }

    #[test]
    fn group_and_rule_limits_have_distinct_kinds() {
        let mut rules = ruleset(vec![rule("wide", Condition::Always)]);
        rules.rules[0].any_of[0].all_of = (0..9)
            .map(|_| ConditionTerm {
                condition: Condition::Always,
                negated: false,
            })
            .collect();
        let issues = validate(&rules);
        let wide = issues
            .iter()
            .find(|issue| issue.kind == IssueKind::GroupTooLarge)
            .expect("group limit");
        assert_eq!(
            wide.path,
            Some(TermPath {
                group: 0,
                term: None
            })
        );
        assert!(!issues
            .iter()
            .any(|issue| issue.kind == IssueKind::TooManyGroups));
        rules.rules[0].any_of = (0..9)
            .map(|_| AllOf {
                all_of: vec![ConditionTerm {
                    condition: Condition::Always,
                    negated: false,
                }],
            })
            .collect();
        assert!(validate(&rules)
            .iter()
            .any(|issue| issue.kind == IssueKind::TooManyGroups && issue.is_error()));
    }

    #[test]
    fn a_ruleset_with_every_rule_disabled_gets_one_warning() {
        assert!(!validate(&ruleset(Vec::new()))
            .iter()
            .any(|issue| issue.kind == IssueKind::NoRuleEnabled));
        let mut rules = ruleset(vec![
            rule("one", Condition::Always),
            rule("two", Condition::Parked),
        ]);
        assert!(!validate(&rules)
            .iter()
            .any(|issue| issue.kind == IssueKind::NoRuleEnabled));
        rules.rules[0].enabled = false;
        assert!(!validate(&rules)
            .iter()
            .any(|issue| issue.kind == IssueKind::NoRuleEnabled));
        rules.rules[1].enabled = false;
        let issues = validate(&rules);
        let warnings: Vec<_> = issues
            .iter()
            .filter(|issue| issue.kind == IssueKind::NoRuleEnabled)
            .collect();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].severity, IssueSeverity::Warning);
        assert!(!has_errors(&issues));
    }
}
