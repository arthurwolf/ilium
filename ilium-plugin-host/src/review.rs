//! Grant review rows, update reviews and the decline counter
//! (reference-grant-review-and-consent.md SEC-100..SEC-110).

use std::collections::HashMap;

use ilium_plugin_api::grants::{self, Grant};

/// The display name of a grant string, as the review dialog shows it.
pub fn display_name(grant: &str) -> String {
    // Display-only sanitization (SEC-104); never alter the stored identifier.
    let clean: String = grant.chars().filter(|c| !c.is_control() &&
        !matches!(*c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')).collect();
    if clean.chars().count() > 64 {
        format!("{}…", clean.chars().take(63).collect::<String>())
    } else {
        clean
    }
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
pub fn review_rows(grants: &[Grant]) -> Vec<Row> {
    let mut rows: Vec<_> = unique_visible(grants)
        .into_iter()
        .map(|grant| {
            let high = grants::is_high_risk(grant.as_str());
            Row {
                grant: grant.as_str().to_owned(),
                details: grant_details(&grant),
                toxic: high,
                is_toxic: high,
                risk_rank: u32::from(high),
                locked: false,
            }
        })
        .collect();
    rows.sort_by(|a, b| b.risk_rank.cmp(&a.risk_rank).then(a.grant.cmp(&b.grant)));
    rows
}

fn grant_details(grant: &Grant) -> String {
    let argument = grant.argument().unwrap_or("");
    let description = match grant.family() {
        "tree.read" => "Read the session tree".to_owned(),
        "agent.read" => "Read agent state".to_owned(),
        "terminal.read-screen" | "terminal.read-cells" => "Read terminal text".to_owned(),
        "terminal.history" => "Read scrollback".to_owned(),
        "process.run" => format!("Run {argument}"),
        "http" => format!("Send data to {argument}"),
        "notify.toast" => "Show toasts".to_owned(),
        "sound.play" => "Play sounds".to_owned(),
        "fs.read" => format!("Read files in {argument}"),
        "fs.write" => format!("Write files in {argument}"),
        _ => format!("Allow {}", grant.as_str()),
    };
    display_name(&description)
}

fn always_granted(grant: &Grant) -> bool {
    matches!(
        (grant.family(), grant.argument()),
        ("fs.read" | "fs.watch", Some("package" | "data" | "cache"))
            | ("fs.write", Some("data" | "cache"))
    )
}

fn unique_visible(grants: &[Grant]) -> Vec<Grant> {
    let mut result = Vec::new();
    for grant in grants {
        if !always_granted(grant) && !result.contains(grant) {
            result.push(grant.clone());
        }
    }
    result
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
pub fn update_review(before: &[Grant], after: &[Grant]) -> UpdateReview {
    let before = unique_visible(before);
    let after = unique_visible(after);
    let mut review = UpdateReview {
        removed: before
            .iter()
            .filter(|g| !after.contains(g))
            .cloned()
            .collect(),
        ..UpdateReview::default()
    };
    for grant in after {
        if before.contains(&grant) {
            review.unchanged.push(grant);
            continue;
        }
        let replaces = review.removed.iter().find(|old| widens(old, &grant));
        if let Some(old) = replaces {
            review.wider.push(WiderRow {
                grant,
                replaces: old.clone(),
                widening: Widening::Argument,
            });
        } else {
            let checked = !grants::is_high_risk(grant.as_str());
            review.new.push(GrantChange { grant, checked });
        }
    }
    review
}

fn widens(old: &Grant, new: &Grant) -> bool {
    // A newly high-risk capability is always a new, unchecked request (SEC-108).
    if !grants::is_high_risk(old.as_str()) && grants::is_high_risk(new.as_str()) {
        return false;
    }
    match new.family() {
        "commands.invoke" if old.family() == new.family() => {
            let old_arg = old.argument().unwrap_or("");
            let new_arg = new.argument().unwrap_or("");
            ((!old_arg.contains('*') && new_arg.ends_with("/*"))
                || (old_arg.ends_with("/*") && new_arg == "*"))
                && new.covers(old)
        }
        "http" if old.family() == new.family() => {
            old.argument()
                .is_some_and(|arg| !arg.contains('*') && !arg.starts_with("setting:"))
                && new.argument().is_some_and(|arg| arg.starts_with("*."))
                && new.covers(old)
        }
        "fs.read" | "fs.write" | "fs.watch" => {
            let same_family = old.family() == new.family();
            let read_to_write = old.family() == "fs.read" && new.family() == "fs.write";
            if !same_family && !read_to_write {
                return false;
            }
            match (old.argument(), new.argument()) {
                (Some(a), Some(b)) if a.starts_with("path:") && b.starts_with("path:") => {
                    same_family && new.covers(old) || read_to_write && a == b
                }
                (Some("project"), Some("path:{home}/**")) => true,
                (Some(a), Some(b)) => read_to_write && a == b,
                _ => false,
            }
        }
        _ => false,
    }
}

/// The scroll indicator of a long review (`3 of 12`).
pub fn scroll_indicator(position: usize, total: usize) -> String {
    if total == 0 || position >= total {
        String::new()
    } else {
        format!("{position} of {total}")
    }
}

/// Counts declines per plugin and grant and silences repeated consent prompts (SEC-110).
#[derive(Debug, Clone, Default)]
pub struct DeclineCounter {
    declines: HashMap<(String, String), u8>,
}

impl DeclineCounter {
    /// Records one decline of `grant` by `plugin`'s prompt.
    pub fn record_decline(&mut self, plugin: &str, grant: &str) {
        let count = self
            .declines
            .entry((plugin.into(), grant.into()))
            .or_default();
        *count = count.saturating_add(1).min(3);
    }

    /// Whether further prompts of `plugin` for `grant` are silenced.
    pub fn is_silenced(&self, plugin: &str, grant: &str) -> bool {
        self.declines
            .get(&(plugin.into(), grant.into()))
            .copied()
            .unwrap_or(0)
            >= 3
    }

    /// Resets the answer after a Settings change or withdrawn request (SEC-110).
    pub fn reset(&mut self, plugin: &str, grant: &str) {
        self.declines.remove(&(plugin.into(), grant.into()));
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
pub fn grant_group(grant: &str) -> Option<GrantGroup> {
    let grant = Grant::parse(grant).ok()?;
    // SEC-006 is a fixed set, not all file reads or all grants containing "read".
    match grant.family() {
        "terminal.read-screen"
        | "terminal.read-cells"
        | "tree.read"
        | "agent.read"
        | "journal.read" => Some(GrantGroup::DataRead),
        "fs.read" if grant.argument() == Some("project") => Some(GrantGroup::DataRead),
        "http" | "inference" | "notify.toast" | "notify.desktop" | "clipboard.write"
        | "messages.peer" | "services.use" => Some(GrantGroup::Egress),
        _ => None,
    }
}

/// Whether the currently checked grants of ONE side form the SEC-006 combination.
/// Call separately for each side; combining different sides would invent a warning.
pub fn has_toxic_combination(checked: &[Grant]) -> bool {
    let has_group = |group| {
        checked
            .iter()
            .any(|grant| grant_group(grant.as_str()) == Some(group))
    };
    has_group(GrantGroup::DataRead) && has_group(GrantGroup::Egress)
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
        let review = update_review(
            &[grant("process.run:git")],
            &[grant("process.run:/usr/bin/git")],
        );
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
        let review = update_review(
            &[grant("tree.read")],
            &[grant("tree.read"), grant("tree.write")],
        );
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
