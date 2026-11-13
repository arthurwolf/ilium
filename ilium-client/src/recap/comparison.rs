//! Offline comparison of an agent's current title with its batch short recap.
//!
//! The comparison scores how many title words the recap also states. It is a
//! measurement only: it never changes the title, the recap, or any stored state.
//! Plan section 11 step 7 gates adopting the goal-aware digest for titles on
//! these results, so the function stays pure and free of I/O.

use std::collections::BTreeSet;

/// Words too common to show that a recap restates a title.
const STOP_WORDS: &[&str] = &[
    "and", "are", "for", "from", "has", "have", "into", "its", "not", "the", "this", "that", "via",
    "with",
];

/// Shortest word counted, so articles and single letters never score.
const MINIMUM_WORD_CHARACTERS: usize = 3;

/// Result of comparing one title with one recap.
#[derive(Debug, Clone, PartialEq)]
pub struct RecapTitleComparison {
    /// Counted title words, lowercased and deduplicated.
    pub title_terms: BTreeSet<String>,
    /// Title words that the recap also contains.
    pub shared_terms: BTreeSet<String>,
    /// `shared_terms.len() / title_terms.len()`, or `None` when the title has no counted words.
    pub coverage: Option<f64>,
}

/// Compares `title` with `recap_short` by word overlap.
pub fn compare_title_with_recap(title: &str, recap_short: &str) -> RecapTitleComparison {
    let title_terms = counted_terms(title);
    let recap_terms = counted_terms(recap_short);
    let shared_terms: BTreeSet<String> = title_terms.intersection(&recap_terms).cloned().collect();
    let coverage = if title_terms.is_empty() {
        None
    } else {
        Some(shared_terms.len() as f64 / title_terms.len() as f64)
    };
    RecapTitleComparison {
        title_terms,
        shared_terms,
        coverage,
    }
}

/// Splits text on non-alphanumeric characters and keeps the words that count.
fn counted_terms(text: &str) -> BTreeSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|word| word.chars().count() >= MINIMUM_WORD_CHARACTERS)
        .filter(|word| !STOP_WORDS.contains(&word.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_overlap_scores_one() {
        let comparison = compare_title_with_recap(
            "Token expiry check",
            "The agent reviews the token expiry check in auth.",
        );
        assert_eq!(comparison.coverage, Some(1.0));
        assert_eq!(comparison.shared_terms, comparison.title_terms);
    }

    #[test]
    fn partial_overlap_scores_the_shared_fraction() {
        let comparison = compare_title_with_recap("Fix token expiry", "Token refresh is broken.");
        assert_eq!(comparison.coverage, Some(1.0 / 3.0));
        assert_eq!(
            comparison.shared_terms.into_iter().collect::<Vec<_>>(),
            vec!["token".to_string()]
        );
    }

    #[test]
    fn stop_words_and_short_words_do_not_count() {
        let comparison = compare_title_with_recap("The fix for it", "The fix for it");
        assert_eq!(
            comparison.title_terms.into_iter().collect::<Vec<_>>(),
            vec!["fix".to_string()]
        );
    }

    #[test]
    fn empty_title_has_no_coverage() {
        let comparison = compare_title_with_recap("a an the", "anything at all");
        assert!(comparison.title_terms.is_empty());
        assert_eq!(comparison.coverage, None);
    }

    #[test]
    fn matching_ignores_case_and_punctuation() {
        let comparison = compare_title_with_recap("AUTH, Refactor!", "auth refactor done");
        assert_eq!(comparison.coverage, Some(1.0));
    }
}
