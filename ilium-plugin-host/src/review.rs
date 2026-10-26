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
    } else { clean }
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
    let mut rows: Vec<_> = unique_visible(grants).into_iter().map(|grant| {
        let high = grants::is_high_risk(grant.as_str());
        Row { grant: grant.as_str().to_owned(), details: grant_details(&grant),
            toxic: high, is_toxic: high, risk_rank: u32::from(high), locked: false }
    }).collect();
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
    matches!((grant.family(), grant.argument()),
        ("fs.read" | "fs.watch", Some("package" | "data" | "cache")) |
        ("fs.write", Some("data" | "cache")))
}

fn unique_visible(grants: &[Grant]) -> Vec<Grant> {
    let mut result = Vec::new();
    for grant in grants {
        if !always_granted(grant) && !result.contains(grant) { result.push(grant.clone()); }
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
        removed: before.iter().filter(|g| !after.contains(g)).cloned().collect(),
        ..UpdateReview::default()
    };
    for grant in after {
        if before.contains(&grant) { review.unchanged.push(grant); continue; }
        let replaces = review.removed.iter().find(|old| widens(old, &grant));
        if let Some(old) = replaces {
            review.wider.push(WiderRow { grant, replaces: old.clone(), widening: Widening::Argument });
        } else {
            let checked = !grants::is_high_risk(grant.as_str());
            review.new.push(GrantChange { grant, checked });
        }
    }
    review
}

fn widens(old: &Grant, new: &Grant) -> bool {
    // A newly high-risk capability is always a new, unchecked request (SEC-108).
    if !grants::is_high_risk(old.as_str()) && grants::is_high_risk(new.as_str()) { return false; }
    match new.family() {
        "commands.invoke" if old.family() == new.family() => {
            let old_arg = old.argument().unwrap_or("");
            let new_arg = new.argument().unwrap_or("");
            ((!old_arg.contains('*') && new_arg.ends_with("/*")) ||
                (old_arg.ends_with("/*") && new_arg == "*")) && new.covers(old)
        }
        "http" if old.family() == new.family() => {
            old.argument().is_some_and(|arg| !arg.contains('*') && !arg.starts_with("setting:")) &&
                new.argument().is_some_and(|arg| arg.starts_with("*.")) && new.covers(old)
        }
        "fs.read" | "fs.write" | "fs.watch" => {
            let same_family = old.family() == new.family();
            let read_to_write = old.family() == "fs.read" && new.family() == "fs.write";
            if !same_family && !read_to_write { return false; }
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
    if total == 0 || position >= total { String::new() } else { format!("{position} of {total}") }
}

/// Counts declines per plugin and grant and silences repeated consent prompts (SEC-110).
#[derive(Debug, Clone, Default)]
pub struct DeclineCounter {
    declines: HashMap<(String, String), u8>,
}

impl DeclineCounter {
    /// Records one decline of `grant` by `plugin`'s prompt.
    pub fn record_decline(&mut self, plugin: &str, grant: &str) {
        let count = self.declines.entry((plugin.into(), grant.into())).or_default();
        *count = count.saturating_add(1).min(3);
    }

    /// Whether further prompts of `plugin` for `grant` are silenced.
    pub fn is_silenced(&self, plugin: &str, grant: &str) -> bool {
        self.declines.get(&(plugin.into(), grant.into())).copied().unwrap_or(0) >= 3
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
        "terminal.read-screen" | "terminal.read-cells" | "tree.read" | "agent.read" |
        "journal.read" => Some(GrantGroup::DataRead),
        "fs.read" if grant.argument() == Some("project") => Some(GrantGroup::DataRead),
        "http" | "inference" | "notify.toast" | "notify.desktop" |
        "clipboard.write" | "messages.peer" | "services.use" => Some(GrantGroup::Egress),
        _ => None,
    }
}

/// Whether the currently checked grants of ONE side form the SEC-006 combination.
/// Call separately for each side; combining different sides would invent a warning.
pub fn has_toxic_combination(checked: &[Grant]) -> bool {
    let has_group = |group| checked.iter().any(|grant| grant_group(grant.as_str()) == Some(group));
    has_group(GrantGroup::DataRead) && has_group(GrantGroup::Egress)
}
