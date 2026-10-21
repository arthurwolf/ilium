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

#[cfg(test)]
mod tests {
    use super::{
        display_name, grant_group, review_rows, scroll_indicator, update_review, DeclineCounter,
        GrantChange, GrantGroup, UpdateReview, Widening, WiderRow,
    };
    use ilium_plugin_api::grants::Grant;

    /// Parses a grant string that the test expects to be valid.
    fn grant(text: &str) -> Grant {
        Grant::parse(text).unwrap()
    }

    /// REQ: SEC-104
    #[test]
    fn display_name_keeps_plain_grant_text_unchanged() {
        assert_eq!(display_name("http:api.example.com"), "http:api.example.com");
    }

    /// REQ: SEC-104
    #[test]
    fn display_name_of_empty_text_is_empty() {
        assert_eq!(display_name(""), "");
    }

    /// REQ: SEC-104
    #[test]
    fn display_name_removes_control_bidi_and_zero_width_joiner_characters() {
        let dirty = "fs.read\u{202E}:project\u{200D}\u{0007}";
        assert_eq!(display_name(dirty), "fs.read:project");
    }

    /// REQ: SEC-104
    #[test]
    fn display_name_of_exactly_64_characters_is_not_shortened() {
        let text = "a".repeat(64);
        assert_eq!(display_name(&text), text);
    }

    /// REQ: SEC-104
    #[test]
    fn display_name_longer_than_64_characters_ends_with_ellipsis_at_64() {
        let text = "b".repeat(65);
        let shown = display_name(&text);
        assert_eq!(shown.chars().count(), 64);
        assert!(shown.ends_with('\u{2026}'));
    }

    /// REQ: SEC-106
    #[test]
    fn review_rows_of_no_grants_is_empty() {
        assert!(review_rows(&[]).is_empty());
    }

    /// REQ: SEC-106
    #[test]
    fn review_rows_normal_grant_is_not_toxic_and_not_locked() {
        let rows = review_rows(&[grant("tree.read")]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].grant, "tree.read");
        assert!(!rows[0].toxic);
        assert!(!rows[0].is_toxic);
        assert!(!rows[0].locked);
    }

    /// REQ: SEC-106
    #[test]
    fn review_rows_high_risk_grant_is_marked_toxic_in_both_fields() {
        let rows = review_rows(&[grant("process.run:rg")]);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].toxic);
        assert!(rows[0].is_toxic);
    }

    /// REQ: SEC-106
    #[test]
    fn review_rows_orders_high_risk_rows_before_normal_rows() {
        let rows = review_rows(&[grant("tree.read"), grant("process.run:rg")]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].grant, "process.run:rg");
        assert_eq!(rows[1].grant, "tree.read");
        assert!(rows[0].risk_rank > rows[1].risk_rank);
    }

    /// REQ: SEC-106
    #[test]
    fn review_rows_keeps_one_row_per_grant() {
        let grants = vec![
            grant("fs.read:project"),
            grant("http:api.example.com"),
            grant("terminal.history"),
        ];
        let rows = review_rows(&grants);
        assert_eq!(rows.len(), grants.len());
        for row in &rows {
            assert_eq!(row.toxic, row.is_toxic);
        }
    }

    /// REQ: SEC-108
    #[test]
    fn update_review_sorts_new_wider_and_removed_as_the_spec_example() {
        let before = vec![grant("fs.read:data"), grant("http:a.example")];
        let after = vec![grant("fs.read:project"), grant("http:*.example")];
        let review = update_review(&before, &after);
        assert_eq!(
            review.new,
            vec![GrantChange {
                grant: grant("fs.read:project"),
                checked: true,
            }]
        );
        assert_eq!(
            review.wider,
            vec![WiderRow {
                grant: grant("http:*.example"),
                replaces: grant("http:a.example"),
                widening: Widening::Argument,
            }]
        );
        assert_eq!(review.removed, vec![grant("http:a.example")]);
        assert!(review.unchanged.is_empty());
    }

    /// REQ: SEC-108
    #[test]
    fn update_review_of_two_empty_lists_is_empty() {
        let review = update_review(&[], &[]);
        assert_eq!(review, UpdateReview::default());
    }

    /// REQ: SEC-108
    #[test]
    fn update_review_of_identical_lists_only_reports_unchanged_grants() {
        let grants = vec![grant("tree.read"), grant("http:api.example.com")];
        let review = update_review(&grants, &grants);
        assert!(review.new.is_empty());
        assert!(review.wider.is_empty());
        assert!(review.removed.is_empty());
        assert_eq!(review.unchanged, grants);
    }

    /// REQ: SEC-108
    #[test]
    fn update_review_new_high_risk_grant_starts_unchecked() {
        let review = update_review(&[], &[grant("process.run:rg")]);
        assert_eq!(
            review.new,
            vec![GrantChange {
                grant: grant("process.run:rg"),
                checked: false,
            }]
        );
    }

    /// REQ: SEC-108
    #[test]
    fn update_review_never_lists_always_granted_entries_as_removed() {
        let review = update_review(&[grant("fs.read:data")], &[]);
        assert!(review.removed.is_empty());
        assert!(review.new.is_empty());
    }

    /// REQ: SEC-108
    #[test]
    fn update_review_exact_command_id_widened_to_plugin_wildcard() {
        let review = update_review(
            &[grant("commands.invoke:ilium.git/status")],
            &[grant("commands.invoke:ilium.git/*")],
        );
        assert_eq!(
            review.wider,
            vec![WiderRow {
                grant: grant("commands.invoke:ilium.git/*"),
                replaces: grant("commands.invoke:ilium.git/status"),
                widening: Widening::Argument,
            }]
        );
        assert!(review.new.is_empty());
    }

    /// REQ: SEC-108
    #[test]
    fn update_review_path_template_replaced_by_ancestor_is_widened() {
        let review = update_review(
            &[grant("fs.read:path:{home}/a/b")],
            &[grant("fs.read:path:{home}/a")],
        );
        assert_eq!(review.wider.len(), 1);
        assert_eq!(review.wider[0].grant, grant("fs.read:path:{home}/a"));
        assert_eq!(review.wider[0].replaces, grant("fs.read:path:{home}/a/b"));
    }

    /// REQ: SEC-108
    #[test]
    fn update_review_named_scope_becomes_broader_scope_is_widened() {
        let review = update_review(
            &[grant("fs.read:project")],
            &[grant("fs.read:path:{home}/**")],
        );
        assert_eq!(review.wider.len(), 1);
        assert_eq!(review.wider[0].grant, grant("fs.read:path:{home}/**"));
        assert_eq!(review.wider[0].replaces, grant("fs.read:project"));
        assert_eq!(review.removed, vec![grant("fs.read:project")]);
    }

    /// REQ: SEC-108
    #[test]
    fn update_review_read_scope_becoming_write_is_widened() {
        let review = update_review(&[grant("fs.read:project")], &[grant("fs.write:project")]);
        assert_eq!(review.wider.len(), 1);
        assert_eq!(review.wider[0].grant, grant("fs.write:project"));
        assert!(review.new.is_empty());
    }

    /// REQ: SEC-108
    #[test]
    fn update_review_program_becoming_absolute_path_is_new_not_widened() {
        let review = update_review(&[grant("process.run:git")], &[grant("process.run:/usr/bin/git")]);
        assert!(review.wider.is_empty());
        assert_eq!(review.removed, vec![grant("process.run:git")]);
        assert_eq!(
            review.new,
            vec![GrantChange {
                grant: grant("process.run:/usr/bin/git"),
                checked: false,
            }]
        );
    }

    /// REQ: SEC-108
    #[test]
    fn update_review_held_grant_kept_in_both_lists_is_unchanged() {
        let review = update_review(&[grant("tree.read")], &[grant("tree.read"), grant("tree.write")]);
        assert_eq!(review.unchanged, vec![grant("tree.read")]);
        assert_eq!(
            review.new,
            vec![GrantChange {
                grant: grant("tree.write"),
                checked: true,
            }]
        );
    }

    /// REQ: SEC-122
    #[test]
    fn scroll_indicator_formats_position_of_total() {
        assert_eq!(scroll_indicator(3, 12), "3 of 12");
    }

    /// REQ: SEC-122
    #[test]
    fn scroll_indicator_single_item_and_last_item_boundaries() {
        assert_eq!(scroll_indicator(1, 1), "1 of 1");
        assert_eq!(scroll_indicator(12, 12), "12 of 12");
    }

    /// REQ: SEC-110
    #[test]
    fn decline_counter_does_not_silence_a_fresh_target() {
        let counter = DeclineCounter::default();
        assert!(!counter.is_silenced("com.example.plugin", "fs.read:project"));
    }

    /// REQ: SEC-110
    #[test]
    fn decline_counter_silences_after_three_declines() {
        let mut counter = DeclineCounter::default();
        counter.record_decline("com.example.plugin", "http:api.example.com");
        counter.record_decline("com.example.plugin", "http:api.example.com");
        assert!(!counter.is_silenced("com.example.plugin", "http:api.example.com"));
        counter.record_decline("com.example.plugin", "http:api.example.com");
        assert!(counter.is_silenced("com.example.plugin", "http:api.example.com"));
    }

    /// REQ: SEC-110
    #[test]
    fn decline_counter_stays_silenced_after_more_than_three_declines() {
        let mut counter = DeclineCounter::default();
        for _ in 0..4 {
            counter.record_decline("com.example.plugin", "http:api.example.com");
        }
        assert!(counter.is_silenced("com.example.plugin", "http:api.example.com"));
    }

    /// REQ: SEC-110
    #[test]
    fn decline_counter_keys_by_plugin_and_grant() {
        let mut counter = DeclineCounter::default();
        for _ in 0..3 {
            counter.record_decline("com.example.plugin", "http:api.example.com");
        }
        assert!(!counter.is_silenced("com.example.other", "http:api.example.com"));
        assert!(!counter.is_silenced("com.example.plugin", "http:status.example.com"));
    }

    /// REQ: SEC-106
    #[test]
    fn grant_group_data_read_members_are_data_read() {
        for text in [
            "terminal.read-screen",
            "terminal.read-cells",
            "fs.read:project",
            "tree.read",
            "agent.read",
            "journal.read",
        ] {
            assert_eq!(grant_group(text), Some(GrantGroup::DataRead), "{text}");
        }
    }

    /// REQ: SEC-006
    #[test]
    fn grant_group_egress_members_are_egress() {
        for text in [
            "http:api.example.com",
            "inference",
            "notify.desktop",
            "notify.toast",
            "clipboard.write",
            "messages.peer:com.example.logger",
            "services.use:com.example.git/status",
        ] {
            assert_eq!(grant_group(text), Some(GrantGroup::Egress), "{text}");
        }
    }

    /// REQ: SEC-006
    #[test]
    fn grant_group_non_members_and_empty_text_are_none() {
        for text in ["ui.panels", "tree.write", "clipboard.read", ""] {
            assert_eq!(grant_group(text), None, "{text}");
        }
    }
}
