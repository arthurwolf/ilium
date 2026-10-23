//! Pure review contracts; no simulated dialog or consent execution.
use ilium_plugin_api::grants::Grant;
use ilium_plugin_host::review::{self, DeclineCounter, GrantGroup};

fn grants(values: &[&str]) -> Vec<Grant> {
    values.iter().map(|value| Grant::parse(value).expect("valid fixture")).collect()
}

#[test]
fn toxic_groups_use_the_fixed_read_list_and_checked_members() {
    assert_eq!(review::grant_group("fs.read:project"), Some(GrantGroup::DataRead));
    assert_eq!(review::grant_group("fs.read:data"), None);
    assert_eq!(review::grant_group("terminal.history"), None);
    assert_eq!(review::grant_group("unknown.read"), None);
    assert!(review::has_toxic_combination(&grants(&["tree.read", "http:example.com"])));
    assert!(!review::has_toxic_combination(&grants(&["tree.read"])));
    assert!(!review::has_toxic_combination(&grants(&["http:example.com"])));
}

#[test]
fn individual_risk_does_not_change_when_combined() {
    let rows = review::review_rows(&grants(&["tree.read", "http:example.com", "terminal.history"]));
    assert_eq!(rows[0].grant, "terminal.history");
    assert!(rows[0].toxic);
    assert!(rows.iter().filter(|r| r.grant != "terminal.history").all(|r| !r.toxic));
}

#[test]
fn update_uses_the_normative_new_wider_removed_example() {
    let result = review::update_review(&grants(&["fs.read:data", "http:a.example"]),
        &grants(&["fs.read:project", "http:*.example"]));
    assert_eq!(result.new.len(), 1);
    assert_eq!(result.new[0].grant.as_str(), "fs.read:project");
    assert!(result.new[0].checked);
    assert_eq!(result.wider.len(), 1);
    assert_eq!(result.wider[0].grant.as_str(), "http:*.example");
    assert_eq!(result.removed, grants(&["http:a.example"]));
}

#[test]
fn port_changes_and_program_paths_are_new_not_wider() {
    let result = review::update_review(&grants(&["http:a.example", "process.run:rg"]),
        &grants(&["http:a.example:8443", "process.run:/usr/bin/rg"]));
    assert_eq!(result.new.len(), 2);
    assert!(result.wider.is_empty());
    assert_eq!(result.removed.len(), 2);
}

#[test]
fn repeated_declarations_and_private_scopes_do_not_add_review_rows() {
    let values = grants(&["fs.read:package", "fs.write:data", "tree.read", "tree.read"]);
    assert_eq!(review::review_rows(&values).len(), 1);
    assert_eq!(review::update_review(&[], &values).new.len(), 1);
}

#[test]
fn decline_silencing_is_per_plugin_and_target_and_can_be_reset() {
    let mut counter = DeclineCounter::default();
    for _ in 0..2 { counter.record_decline("a", "tree.read"); }
    assert!(!counter.is_silenced("a", "tree.read"));
    counter.record_decline("a", "tree.read");
    assert!(counter.is_silenced("a", "tree.read"));
    assert!(!counter.is_silenced("b", "tree.read"));
    assert!(!counter.is_silenced("a", "agent.read"));
    counter.reset("a", "tree.read");
    assert!(!counter.is_silenced("a", "tree.read"));
}

#[test]
fn names_are_sanitized_without_changing_plain_names() {
    assert_eq!(review::display_name("Git Helper"), "Git Helper");
    assert_eq!(review::display_name("a\u{202e}b\u{200d}c\n"), "abc");
    let long = review::display_name(&"é".repeat(65));
    assert_eq!(long.chars().count(), 64);
    assert!(long.ends_with('…'));
}

#[test]
fn newly_high_risk_requests_are_new_and_unchecked() {
    let result = review::update_review(&grants(&["commands.invoke:example.tool/*"]),
        &grants(&["commands.invoke:*"]));
    assert!(result.wider.is_empty());
    assert_eq!(result.new.len(), 1);
    assert!(!result.new[0].checked);
}
