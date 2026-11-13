//! What happens to a pane's worktree when the pane closes: the three dispositions, the default
//! policy of the Git settings tab, and how a refusal from the server is classified.
//!
//! Pure rules, moved from `ilium/src/close_pane.rs` and `GitClosePolicy` in the client config.

/// The prefix the removal path puts on a refusal whose outcome is not known.
pub const UNCERTAIN_REASON_PREFIX: &str = "REMOVAL OUTCOME UNCERTAIN";

/// What closing a pane does to its worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Keep,
    Remove,
    RemoveAndBranch,
}

impl Disposition {
    pub const ALL: [Self; 3] = [Self::Keep, Self::Remove, Self::RemoveAndBranch];

    /// The label used on the command line and in JSON answers.
    pub fn label(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Remove => "remove",
            Self::RemoveAndBranch => "remove-and-branch",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.label() == label)
    }

    /// Whether the worktree directory is deleted.
    pub fn removes_worktree(self) -> bool {
        !matches!(self, Self::Keep)
    }

    /// Whether the creation branch is deleted too (only when it is safe).
    pub fn removes_branch(self) -> bool {
        matches!(self, Self::RemoveAndBranch)
    }
}

/// The default close policy of the Git settings tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClosePolicy {
    #[default]
    Keep,
    OfferRemovalWhenSafe,
}

impl ClosePolicy {
    pub const ALL: [Self; 2] = [Self::Keep, Self::OfferRemovalWhenSafe];

    pub fn label(self) -> &'static str {
        match self {
            Self::Keep => "Keep",
            Self::OfferRemovalWhenSafe => "Offer removal when safe",
        }
    }

    /// The stored value of the setting.
    pub fn value(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::OfferRemovalWhenSafe => "offer_removal_when_safe",
        }
    }

    pub fn from_value(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.value() == value)
    }

    /// Step to the next or previous value, wrapping.
    pub fn stepped(self, direction: i32) -> Self {
        let position = Self::ALL.iter().position(|item| *item == self).unwrap_or(0) as i32;
        let count = Self::ALL.len() as i32;
        Self::ALL[(position + direction).rem_euclid(count) as usize]
    }

    /// The disposition pre-selected in the close dialog when the worktree is safe to remove.
    pub fn preselected(self, safe_to_remove: bool) -> Disposition {
        match (self, safe_to_remove) {
            (Self::OfferRemovalWhenSafe, true) => Disposition::Remove,
            _ => Disposition::Keep,
        }
    }
}

/// The server's answer to a close request, reduced to what the caller must act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Closed,
    Removed,
    Blocked(Vec<String>),
    Uncertain(Vec<String>),
}

/// A refusal with the uncertain prefix may have removed the worktree; any other is a clean block.
pub fn classify_refusal(reasons: Vec<String>) -> Outcome {
    if reasons
        .iter()
        .any(|reason| reason.starts_with(UNCERTAIN_REASON_PREFIX))
    {
        Outcome::Uncertain(reasons)
    } else {
        Outcome::Blocked(reasons)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_round_trip() {
        for item in Disposition::ALL {
            assert_eq!(Disposition::from_label(item.label()), Some(item));
        }
        assert_eq!(Disposition::from_label("delete"), None);
    }

    #[test]
    fn only_keep_leaves_the_worktree() {
        assert!(!Disposition::Keep.removes_worktree());
        assert!(Disposition::Remove.removes_worktree());
        assert!(Disposition::RemoveAndBranch.removes_branch());
        assert!(!Disposition::Remove.removes_branch());
    }

    #[test]
    fn policy_steps_wrap_and_default_is_keep() {
        assert_eq!(ClosePolicy::default(), ClosePolicy::Keep);
        assert_eq!(
            ClosePolicy::Keep.stepped(1),
            ClosePolicy::OfferRemovalWhenSafe
        );
        assert_eq!(
            ClosePolicy::Keep.stepped(-1),
            ClosePolicy::OfferRemovalWhenSafe
        );
        assert_eq!(
            ClosePolicy::OfferRemovalWhenSafe.stepped(1),
            ClosePolicy::Keep
        );
        assert_eq!(
            ClosePolicy::from_value("offer_removal_when_safe"),
            Some(ClosePolicy::OfferRemovalWhenSafe)
        );
    }

    #[test]
    fn removal_is_preselected_only_when_offered_and_safe() {
        assert_eq!(ClosePolicy::Keep.preselected(true), Disposition::Keep);
        assert_eq!(
            ClosePolicy::OfferRemovalWhenSafe.preselected(false),
            Disposition::Keep
        );
        assert_eq!(
            ClosePolicy::OfferRemovalWhenSafe.preselected(true),
            Disposition::Remove
        );
    }

    #[test]
    fn uncertain_prefix_is_uncertain_not_blocked() {
        let reasons = vec![format!("{UNCERTAIN_REASON_PREFIX}: git reported an error")];
        assert!(matches!(classify_refusal(reasons), Outcome::Uncertain(_)));
        assert!(matches!(
            classify_refusal(vec!["worktree has uncommitted changes".into()]),
            Outcome::Blocked(_)
        ));
    }
}
