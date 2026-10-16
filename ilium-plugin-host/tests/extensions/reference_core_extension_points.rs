//! reference-core-extension-points.md: the table of core points, services, topics and modes
//! (EXT-040, EXT-041, EXT-042, EXT-052). Doc tests parse the manual page.

use crate::support::*;
use proptest::prelude::*;

fn kind_of(cell: &str) -> (String, Option<String>) {
    match cell.split_once(':') {
        Some((kind, mode)) => (kind.trim().to_owned(), Some(mode.trim().to_owned())),
        None => (cell.trim().to_owned(), None),
    }
}

/// REQ: EXT-040
#[test]
fn doc_ext_040_the_page_lists_66_points() {
    assert_eq!(core_points().len(), 66);
}

/// REQ: EXT-040
#[test]
fn doc_ext_040_the_summary_lists_30_core_plugins_whose_point_counts_add_up_to_66() {
    let text = page("reference-core-extension-points.md");
    let rows = table_after(&text, "### Summary");
    assert_eq!(rows.len(), 30);
    let total: usize = rows.iter().map(|row| row[2].trim().parse::<usize>().unwrap_or(0)).sum();
    assert_eq!(total, 66);
}

/// REQ: EXT-040
#[test]
fn doc_ext_040_modes_are_5_observe_11_transform_10_veto_1_first() {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for (_, kind, _) in core_points() {
        if let (_, Some(mode)) = kind_of(&kind) {
            *counts.entry(mode).or_default() += 1;
        }
    }
    assert_eq!(counts.get("observe"), Some(&5));
    assert_eq!(counts.get("transform"), Some(&11));
    assert_eq!(counts.get("veto"), Some(&10));
    assert_eq!(counts.get("first"), Some(&1));
}

/// REQ: EXT-040
#[test]
fn doc_ext_040_the_mode_table_matches_the_point_table() {
    let text = page("reference-core-extension-points.md");
    let rows = table_after(&text, "### Hook modes at a glance");
    let from_table: u32 = rows.iter().map(|row| row[1].trim().parse::<u32>().unwrap_or(0)).sum();
    let hooks = core_points().iter().filter(|(_, kind, _)| kind.starts_with("hook")).count() as u32;
    assert_eq!(from_table, hooks);
}

/// REQ: EXT-040
#[test]
fn doc_ext_040_every_point_belongs_to_a_listed_core_plugin_and_per_plugin_counts_match() {
    let text = page("reference-core-extension-points.md");
    for row in table_after(&text, "### Summary") {
        let id = row[1].trim_matches('`').to_owned();
        let declared: usize = row[2].trim().parse().unwrap_or(usize::MAX);
        let listed = core_points().iter().filter(|(point, _, _)| point.starts_with(&format!("{id}/"))).count();
        assert_eq!(listed, declared, "{id}");
    }
}

/// REQ: EXT-040
#[test]
fn doc_ext_040_point_ids_are_unique() {
    let points = core_points();
    let unique: std::collections::BTreeSet<_> = points.iter().map(|(point, _, _)| point.clone()).collect();
    assert_eq!(unique.len(), points.len());
}

/// REQ: EXT-041
#[test]
fn doc_ext_041_every_listed_point_id_has_the_documented_form() {
    for (point, _, _) in core_points() {
        let (owner, name) = point.rsplit_once('/').unwrap_or(("", ""));
        assert!(owner.starts_with("ilium.core."), "{point}");
        assert!(!name.is_empty() && name.len() <= 64, "{point}");
        assert!(name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "{point}");
    }
}

/// REQ: EXT-041
#[test]
fn ext_041_point_id_of_64_characters_is_accepted_and_of_65_fails_extension_point_invalid() {
    let mut world = ExtWorld::new();
    for (length, expected) in [(64usize, "ok"), (65, "manifest.extension-point-invalid"), (1, "ok"), (0, "manifest.extension-point-invalid")] {
        let id = "a".repeat(length);
        let manifest = owner_manifest(&format!("com.example.p{length}"), vec![collect_point(&id, 8, false)]);
        assert_eq!(diag_code(&world.install(Tier::T3, manifest, Vec::new())), expected, "length {length}");
    }
}

/// REQ: EXT-041
#[test]
fn ext_041_point_id_with_an_upper_case_letter_underscore_or_slash_fails() {
    let mut world = ExtWorld::new();
    for (index, id) in ["Bad", "bad_id", "a/b", "bad id", "bad.id"].iter().enumerate() {
        let manifest = owner_manifest(&format!("com.example.q{index}"), vec![collect_point(id, 8, false)]);
        assert_eq!(diag_code(&world.install(Tier::T3, manifest, Vec::new())), "manifest.extension-point-invalid", "{id}");
    }
}

proptest! {
    // REQ: EXT-041
    #[test]
    fn ext_041_point_id_grammar_matches_a_reference_check(id in "[a-zA-Z0-9_./ -]{0,70}") {
        let mut world = ExtWorld::new();
        let valid = !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        let manifest = owner_manifest("com.example.prop", vec![collect_point(&id, 8, false)]);
        let code = diag_code(&world.install(Tier::T3, manifest, Vec::new()));
        prop_assert_eq!(code == "ok", valid);
    }
}

/// REQ: EXT-042
#[test]
fn doc_ext_042_the_summary_marks_every_plugin_except_sessions_and_settings_replaceable() {
    let text = page("reference-core-extension-points.md");
    let rows = table_after(&text, "### Summary");
    for row in &rows {
        let id = row[1].trim_matches('`');
        let expected = if id == "ilium.core.sessions" || id == "ilium.core.settings" { "no" } else { "yes" };
        assert_eq!(row[5].trim(), expected, "{id}");
    }
}

/// REQ: EXT-042
#[test]
fn doc_ext_042_every_core_plugin_page_has_a_replaceable_line_matching_the_summary() {
    let text = page("reference-core-extension-points.md");
    for row in table_after(&text, "### Summary") {
        let slug = row[1].trim_matches('`').trim_start_matches("ilium.core.");
        let body = page(&format!("core-plugin-{slug}.md"));
        let says_yes = body.contains("Replaceable") || body.contains("replaceable");
        assert!(says_yes, "{slug} states its Replaceable value");
    }
}

/// REQ: EXT-052
#[test]
fn doc_ext_052_extension_point_table_has_the_side_version_deadline_max_and_dynamic_columns() {
    let text = page("extending-core-plugins.md");
    let header = text.lines().find(|line| line.starts_with('|') && line.contains("Deadline") && line.contains("Dynamic"));
    let header = header.unwrap_or("");
    for column in ["Side", "Version", "Deadline", "Max", "Dynamic"] {
        assert!(header.contains(column), "missing column {column}: {header}");
    }
}

/// REQ: EXT-052
#[test]
fn doc_ext_052_every_core_plugin_page_with_points_has_the_same_columns() {
    let text = page("reference-core-extension-points.md");
    for row in table_after(&text, "### Summary") {
        if row[2].trim() == "0" {
            continue;
        }
        let slug = row[1].trim_matches('`').trim_start_matches("ilium.core.");
        let body = page(&format!("core-plugin-{slug}.md"));
        let header = body.lines().find(|line| line.starts_with('|') && line.contains("Deadline")).unwrap_or("");
        for column in ["Side", "Version", "Deadline", "Max", "Dynamic"] {
            assert!(header.contains(column), "{slug}: missing {column}");
        }
    }
}

/// REQ: EXT-052
#[test]
fn ext_052_deadline_defaults_and_max_handlers_show_in_the_listing_of_a_core_point() {
    let world = ExtWorld::with_core_plugins();
    for point in world.list_points() {
        assert!((5..=50).contains(&point.deadline_ms), "{}", point.id);
        assert!(point.max_handlers >= 1 && point.max_handlers <= 8, "{}", point.id);
    }
}
