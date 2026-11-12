//! The user documentation tables must match the engine: every condition in the
//! registry has exactly one row (key, group, meaning), and the defaults table
//! lists the shipped rules in order. A new `Condition` variant or default rule
//! fails here until `src/docs/custom-status-icons.md` is updated.

use std::collections::BTreeMap;

use ilium_core::status_rule_expression::{
    condition_catalog, default_rules, MAXIMUM_CONDITIONS_PER_GROUP,
    MAXIMUM_ENABLED_SCREEN_PATTERNS, MAXIMUM_GROUPS_PER_RULE, MAXIMUM_NAME_CHARS,
    MAXIMUM_PATTERN_CHARS, MAXIMUM_RULES,
};

const PAGE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../src/docs/custom-status-icons.md"
));

/// The table rows between `<!-- name:start -->` and `<!-- name:end -->`, each
/// split into trimmed cells, without the header and separator rows. `None`
/// when a marker is missing or the markers are out of order.
fn table_rows(page: &str, name: &str) -> Option<Vec<Vec<String>>> {
    let start = format!("<!-- {name}:start -->");
    let end = format!("<!-- {name}:end -->");
    let from = page.find(&start)? + start.len();
    let to = page.find(&end)?;
    if from > to {
        return None;
    }
    Some(
        page[from..to]
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with('|'))
            .skip(2)
            .map(|line| {
                line.trim_start_matches('|')
                    .trim_end_matches('|')
                    .split(" | ")
                    .map(|cell| cell.trim().to_owned())
                    .collect()
            })
            .collect(),
    )
}

#[test]
fn condition_table_matches_the_registry() {
    let mut expected: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    for condition in condition_catalog() {
        expected
            .entry(condition.key())
            .or_insert((condition.category().label(), condition.explain()));
    }
    let rows = table_rows(PAGE, "conditions").expect("condition table markers");
    let mut documented: BTreeMap<String, (String, String)> = BTreeMap::new();
    for row in &rows {
        assert_eq!(row.len(), 3, "condition row {row:?} has three cells");
        let key = row[0].trim_matches('`').to_owned();
        assert!(
            documented
                .insert(key.clone(), (row[1].clone(), row[2].clone()))
                .is_none(),
            "{key} is documented twice"
        );
    }
    for (key, (group, meaning)) in &expected {
        let (doc_group, doc_meaning) = documented
            .get(*key)
            .unwrap_or_else(|| panic!("condition `{key}` is missing from custom-status-icons.md"));
        assert_eq!(doc_group, group, "group of `{key}`");
        assert_eq!(doc_meaning, meaning, "meaning of `{key}`");
    }
    let stale: Vec<&String> = documented
        .keys()
        .filter(|key| !expected.contains_key(key.as_str()))
        .collect();
    assert!(
        stale.is_empty(),
        "documented but not in the registry: {stale:?}"
    );
}

#[test]
fn defaults_table_lists_the_shipped_rules_in_order() {
    let rows = table_rows(PAGE, "defaults").expect("defaults table markers");
    let rules = default_rules().rules;
    assert_eq!(rows.len(), rules.len(), "one row per shipped default rule");
    for (index, (row, rule)) in rows.iter().zip(&rules).enumerate() {
        assert_eq!(row.len(), 4, "defaults row {row:?} has four cells");
        assert_eq!(row[0], (index + 1).to_string(), "order of `{}`", rule.id);
        assert_eq!(row[1].trim_matches('`'), rule.id, "id at position {index}");
        assert_eq!(row[2], rule.name, "name of `{}`", rule.id);
        assert_eq!(
            row[3],
            if rule.enabled { "on" } else { "off" },
            "starting state of `{}`",
            rule.id
        );
    }
}

#[test]
fn the_page_documents_the_limits_the_engine_enforces() {
    for needle in [
        format!("{MAXIMUM_RULES} rules"),
        format!("{MAXIMUM_GROUPS_PER_RULE} AND groups"),
        format!("{MAXIMUM_CONDITIONS_PER_GROUP} conditions per group"),
        format!("{MAXIMUM_NAME_CHARS} characters"),
        format!("{MAXIMUM_ENABLED_SCREEN_PATTERNS} distinct patterns"),
        format!("{MAXIMUM_PATTERN_CHARS} characters"),
    ] {
        assert!(PAGE.contains(&needle), "page should state `{needle}`");
    }
}

#[test]
fn table_parser_reports_missing_or_misordered_markers() {
    assert!(table_rows("no markers here", "conditions").is_none());
    let reversed = "<!-- conditions:end -->\n<!-- conditions:start -->";
    assert!(table_rows(reversed, "conditions").is_none());
    let empty = "<!-- conditions:start -->\n| a | b |\n| --- | --- |\n<!-- conditions:end -->";
    assert_eq!(table_rows(empty, "conditions").unwrap().len(), 0);
}
