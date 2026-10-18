//! Grant review rows, update reviews and the decline counter
//! (reference-grant-review-and-consent.md SEC-100..SEC-110).

use ilium_plugin_api::grants::Grant;

/// The display name of a grant string, as the review dialog shows it.
pub fn display_name(_grant: &str) -> String {
    unimplemented!("host::review::display_name")
}

/// One row of the grant review dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The grant string.
    pub grant: String,
    /// The explanation shown under the grant.
    pub details: String,
    /// Whether the grant is high-risk (shown with the warning style, unchecked by default).
    pub toxic: bool,
    /// Same as `toxic`; kept for the rows that spell it this way.
    pub is_toxic: bool,
    /// The risk rank used to order rows (higher first).
    pub risk_rank: u32,
    /// Whether the row cannot be toggled (always-granted entries).
    pub locked: bool,
}

/// The review rows of a grant list, ordered as the dialog shows them.
pub fn review_rows(_grants: &[Grant]) -> Vec<Row> {
    unimplemented!("host::review::review_rows")
}

/// How an update widened a grant (SEC-108 widening table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Widening {
    /// The argument now covers more than before.
    Argument,
}

/// A grant that is new in an update review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantChange {
    /// The grant.
    pub grant: Grant,
    /// Whether it is checked by default (normal grants only).
    pub checked: bool,
}

/// A grant that got wider in an update review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WiderRow {
    /// The widened grant.
    pub grant: Grant,
    /// The grant it replaces.
    pub replaces: Grant,
    /// How it got wider.
    pub widening: Widening,
}

/// The three groups of an update review (SEC-108).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateReview {
    /// Grants not held before.
    pub new: Vec<GrantChange>,
    /// Grants whose argument now covers more.
    pub wider: Vec<WiderRow>,
    /// Grants the new version no longer lists.
    pub removed: Vec<Grant>,
    /// Grants kept as they were (not shown).
    pub unchanged: Vec<Grant>,
}

/// The update review from the grants held before to the grants the new version lists.
pub fn update_review(_before: &[Grant], _after: &[Grant]) -> UpdateReview {
    unimplemented!("host::review::update_review")
}

/// The scroll indicator of a long review (`3 of 12`).
pub fn scroll_indicator(_position: usize, _total: usize) -> String {
    unimplemented!("host::review::scroll_indicator")
}

/// Counts declines per plugin and grant and silences repeated consent prompts (SEC-110).
#[derive(Debug, Clone, Default)]
pub struct DeclineCounter;

impl DeclineCounter {
    /// Records one decline of `grant` by `plugin`'s prompt.
    pub fn record_decline(&mut self, _plugin: &str, _grant: &str) {
        unimplemented!("host::review::DeclineCounter::record_decline")
    }

    /// Whether further prompts of `plugin` for `grant` are silenced.
    pub fn is_silenced(&self, _plugin: &str, _grant: &str) -> bool {
        unimplemented!("host::review::DeclineCounter::is_silenced")
    }
}

/// The group a grant family belongs to in the review dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GrantGroup {
    /// Reads of local data.
    DataRead,
    /// Network egress.
    Egress,
}

/// The review group of a grant string, if it belongs to one.
pub fn grant_group(_grant: &str) -> Option<GrantGroup> {
    unimplemented!("host::review::grant_group")
}
