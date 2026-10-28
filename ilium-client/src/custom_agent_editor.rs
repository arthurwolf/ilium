//! Editable custom-agent draft. Environment rows stay ordered and distinct.

use ilium_core::custom_agent::{
    validate_profiles, CustomAgentProfile, EnvironmentEntry, ProfileValidationError,
};
use ilium_core::BuiltinAgentProvider;

#[derive(Debug, Clone)]
pub struct CustomAgentEditor {
    pub draft: CustomAgentProfile,
    /// Values remain masked until the user explicitly reveals them.
    pub reveal_values: bool,
}

/// The ordered controls of a draft, independent of terminal geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorRow {
    Name,
    Icon,
    Provider,
    EnvironmentKey(usize),
    EnvironmentValue(usize),
    RemoveEnvironment(usize),
    AddEnvironment,
    RevealValues,
    Save,
    Cancel,
}

impl CustomAgentEditor {
    pub fn rows(&self) -> Vec<EditorRow> {
        let mut rows = vec![EditorRow::Name, EditorRow::Icon, EditorRow::Provider];
        for index in 0..self.draft.environment.len() {
            rows.extend([
                EditorRow::EnvironmentKey(index),
                EditorRow::EnvironmentValue(index),
                EditorRow::RemoveEnvironment(index),
            ]);
        }
        rows.extend([
            EditorRow::AddEnvironment,
            EditorRow::RevealValues,
            EditorRow::Save,
            EditorRow::Cancel,
        ]);
        rows
    }

    /// The value returned for painting never exposes a masked value.
    pub fn displayed_value(&self, index: usize) -> Option<String> {
        let entry = self.draft.environment.get(index)?;
        Some(if self.reveal_values {
            entry.value.clone()
        } else {
            "•".repeat(entry.value.chars().count().min(16))
        })
    }
    pub fn new() -> Self {
        Self {
            draft: CustomAgentProfile {
                id: uuid::Uuid::new_v4().to_string(),
                name: String::new(),
                icon: "✦".into(),
                base_provider: BuiltinAgentProvider::Claude,
                environment: Vec::new(),
            },
            reveal_values: false,
        }
    }

    pub fn edit(profile: &CustomAgentProfile) -> Self {
        Self {
            draft: profile.clone(),
            reveal_values: false,
        }
    }

    pub fn add_environment_row(&mut self) {
        self.draft.environment.push(EnvironmentEntry {
            key: String::new(),
            value: String::new(),
        });
    }

    pub fn remove_environment_row(&mut self, index: usize) -> bool {
        if index >= self.draft.environment.len() {
            return false;
        }
        self.draft.environment.remove(index);
        true
    }

    /// Validate the draft against all other profiles without changing the draft.
    pub fn validate(&self, existing: &[CustomAgentProfile]) -> Result<(), ProfileValidationError> {
        let mut profiles: Vec<_> = existing
            .iter()
            .filter(|profile| profile.id != self.draft.id)
            .cloned()
            .collect();
        profiles.push(self.draft.clone());
        validate_profiles(&profiles)
    }
}

impl Default for CustomAgentEditor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_retains_literal_values_and_duplicate_rows_until_validation() {
        let mut editor = CustomAgentEditor::new();
        let id = editor.draft.id.clone();
        editor.draft.name = "Example".into();
        editor.add_environment_row();
        editor.draft.environment[0] = EnvironmentEntry {
            key: "TOKEN".into(),
            value: "'literal'\n☃".into(),
        };
        editor.add_environment_row();
        editor.draft.environment[1] = editor.draft.environment[0].clone();
        assert_eq!(
            editor.validate(&[]),
            Err(ProfileValidationError::DuplicateKey { row: 2 })
        );
        assert!(editor.remove_environment_row(1));
        assert_eq!(editor.validate(&[]), Ok(()));
        assert_eq!(editor.draft.id, id);
        assert_eq!(editor.draft.environment[0].value, "'literal'\n☃");
        assert!(!editor.reveal_values);
        assert!(!format!("{editor:?}").contains("literal"));
    }

    #[test]
    fn rows_expand_without_limit_and_secret_values_are_masked_by_default() {
        let mut editor = CustomAgentEditor::new();
        for _ in 0..20 {
            editor.add_environment_row();
        }
        editor.draft.environment[19].value = "private-sentinel".into();
        assert!(editor.rows().contains(&EditorRow::EnvironmentValue(19)));
        assert!(!editor.displayed_value(19).unwrap().contains("private"));
        editor.reveal_values = true;
        assert_eq!(
            editor.displayed_value(19).as_deref(),
            Some("private-sentinel")
        );
        assert!(editor.remove_environment_row(0));
        assert!(editor.rows().contains(&EditorRow::EnvironmentValue(18)));
    }
}
