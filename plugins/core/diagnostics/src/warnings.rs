//! The bounded warnings list of the diagnostics panel: plugins report a warning, the newest 64 are kept.

use serde::{Deserialize, Serialize};

/// Most warnings kept; the oldest is dropped first.
pub const MAX_WARNINGS: usize = 64;

/// One reported warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warning {
    pub plugin: String,
    pub message: String,
    pub at_millis: i64,
}

/// Bounded newest-last list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Warnings {
    entries: Vec<Warning>,
}

impl Warnings {
    pub fn push(&mut self, plugin: &str, message: &str, at_millis: i64) {
        self.entries.push(Warning {
            plugin: plugin.to_string(),
            message: message.to_string(),
            at_millis,
        });
        if self.entries.len() > MAX_WARNINGS {
            let excess = self.entries.len() - MAX_WARNINGS;
            self.entries.drain(..excess);
        }
    }

    pub fn entries(&self) -> &[Warning] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_keeps_the_newest_sixty_four() {
        let mut warnings = Warnings::default();
        for index in 0..70 {
            warnings.push("p", &format!("warning {index}"), index);
        }
        assert_eq!(warnings.len(), MAX_WARNINGS);
        assert_eq!(warnings.entries()[0].message, "warning 6");
        assert_eq!(warnings.entries()[MAX_WARNINGS - 1].message, "warning 69");
    }

    #[test]
    fn clear_empties_the_list() {
        let mut warnings = Warnings::default();
        warnings.push("p", "m", 1);
        warnings.clear();
        assert!(warnings.is_empty());
    }
}
