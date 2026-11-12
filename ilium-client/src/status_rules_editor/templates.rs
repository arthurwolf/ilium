//! Starting points offered by "New rule", so a user never faces an empty
//! form. Every template except "Blank" is a copy of a shipped default rule
//! (`ilium_core::status_rule_expression::default_rules`), so its conditions
//! and icon stay in step with the defaults instead of being written twice.

use ilium_core::status_rule_expression::{
    default_rules, AllOf, Condition, ConditionTerm, Rule, StatusVisual, VisualKind, VisualStyle,
};

/// One entry of the template picker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleTemplate {
    pub title: &'static str,
    /// One sentence shown under the list while the template is highlighted.
    pub description: &'static str,
    /// The default rule this template copies; `None` for the blank rule.
    default_id: Option<&'static str>,
}

/// Picker order: the most common needs first, the blank rule last.
const TEMPLATES: [RuleTemplate; 4] = [
    RuleTemplate {
        title: "Agent needs approval",
        description: "The agent stopped and waits for you to approve a command.",
        default_id: Some("approval"),
    },
    RuleTemplate {
        title: "Goal paused",
        description: "The agent's goal is paused until you resume it.",
        default_id: Some("goal-paused"),
    },
    RuleTemplate {
        title: "Task failed",
        description: "The pane's progress monitor reported a failed task.",
        default_id: Some("task-failed"),
    },
    RuleTemplate {
        title: "Blank",
        description: "One condition (a goal is set) and a dot; change both.",
        default_id: None,
    },
];

pub fn templates() -> &'static [RuleTemplate] {
    &TEMPLATES
}

impl RuleTemplate {
    /// The rule this template creates, with the given identifier. The name
    /// is the template's title, which the user can rename afterwards.
    pub fn build(&self, id: String) -> Rule<Condition, StatusVisual> {
        let copied = self.default_id.and_then(|default_id| {
            default_rules()
                .rules
                .into_iter()
                .find(|rule| rule.id == default_id)
        });
        match copied {
            Some(rule) => Rule {
                id,
                name: self.title.to_owned(),
                enabled: true,
                ..rule
            },
            None => blank_rule(id, self.title),
        }
    }
}

/// A rule with one condition rather than none, so it never matches every
/// pane and hides the rules below it by accident.
fn blank_rule(id: String, name: &str) -> Rule<Condition, StatusVisual> {
    Rule {
        id,
        name: name.to_owned(),
        enabled: true,
        any_of: vec![AllOf {
            all_of: vec![ConditionTerm {
                condition: Condition::GoalSet,
                negated: false,
            }],
        }],
        then: StatusVisual {
            kind: VisualKind::Glyph("●".to_owned()),
            style: VisualStyle::default(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::templates;
    use ilium_core::status_rule_expression::{default_rules, Condition};

    #[test]
    fn every_template_copies_an_existing_default_or_is_blank() {
        let defaults = default_rules();
        for template in templates() {
            let rule = template.build("custom-9".into());
            assert_eq!(rule.id, "custom-9");
            assert_eq!(rule.name, template.title);
            assert!(rule.enabled);
            assert!(!rule.any_of.is_empty(), "{} has conditions", template.title);
            match template.default_id {
                Some(default_id) => {
                    let source = defaults
                        .rules
                        .iter()
                        .find(|rule| rule.id == default_id)
                        .unwrap_or_else(|| panic!("default rule {default_id} exists"));
                    assert_eq!(rule.any_of, source.any_of);
                    assert_eq!(rule.then, source.then);
                }
                None => assert_eq!(
                    rule.any_of[0].all_of[0].condition,
                    Condition::GoalSet,
                    "the blank rule still has one condition"
                ),
            }
        }
        assert_eq!(
            templates().last().map(|template| template.title),
            Some("Blank")
        );
    }
}
