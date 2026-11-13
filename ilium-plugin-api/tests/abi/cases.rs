//! Case counts and case order: ABI-002 and ABI-019. Pure data over the generated types and
//! the golden snapshot; these pass against the skeleton.

use std::collections::BTreeMap;

use ilium_plugin_api::generated::types::{
    ActionKind, ActionValue, DeactivationReason, ErrorCode, Event, Intent, Outcome, Widget,
};

use crate::support::{crate_dir, definitions, read, wit_files};

/// REQ: ABI-002
#[test]
fn abi_002_event_has_56_cases() {
    assert_eq!(Event::CASE_NAMES.len(), 56);
}

/// REQ: ABI-002
#[test]
fn abi_002_intent_has_42_cases() {
    assert_eq!(Intent::CASE_NAMES.len(), 42);
}

/// REQ: ABI-002
#[test]
fn abi_002_outcome_has_7_cases() {
    assert_eq!(Outcome::CASE_NAMES.len(), 7);
}

/// REQ: ABI-002
#[test]
fn abi_002_widget_has_51_cases() {
    assert_eq!(Widget::CASE_NAMES.len(), 51);
}

/// REQ: ABI-002
#[test]
fn abi_002_action_kind_has_17_cases() {
    assert_eq!(ActionKind::CASE_NAMES.len(), 17);
}

/// REQ: ABI-002
#[test]
fn abi_002_action_value_has_12_cases() {
    assert_eq!(ActionValue::CASE_NAMES.len(), 12);
}

/// REQ: ABI-002
#[test]
fn abi_002_error_code_has_15_cases() {
    assert_eq!(ErrorCode::CASE_NAMES.len(), 15);
}

/// REQ: ABI-002
#[test]
fn abi_002_deactivation_reason_has_6_cases() {
    assert_eq!(DeactivationReason::CASE_NAMES.len(), 6);
}

/// REQ: ABI-002
#[test]
fn abi_002_generated_case_order_matches_wit() {
    let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (_, text) in wit_files() {
        for definition in definitions(&text) {
            by_name.insert(definition.name, definition.members);
        }
    }
    let generated: [(&str, &[&str]); 8] = [
        ("event", Event::CASE_NAMES),
        ("intent", Intent::CASE_NAMES),
        ("outcome", Outcome::CASE_NAMES),
        ("widget", Widget::CASE_NAMES),
        ("action-kind", ActionKind::CASE_NAMES),
        ("action-value", ActionValue::CASE_NAMES),
        ("error-code", ErrorCode::CASE_NAMES),
        ("deactivation-reason", DeactivationReason::CASE_NAMES),
    ];
    for (name, cases) in generated {
        let wit_cases = by_name
            .get(name)
            .unwrap_or_else(|| panic!("{name} not in wit/"));
        assert_eq!(cases, wit_cases.as_slice(), "{name}");
    }
}

/// One golden line: `<file> <interface> <kind> <name>: <members>`.
fn golden() -> BTreeMap<(String, String, String), (String, Vec<String>)> {
    let text = read(&crate_dir().join("tests/abi/golden/case_order.txt"));
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let (head, members) = line
                .split_once(": ")
                .unwrap_or((line.trim_end_matches(':'), ""));
            let parts: Vec<&str> = head.split(' ').collect();
            assert_eq!(parts.len(), 4, "golden line {line}");
            let members = members
                .split(',')
                .filter(|member| !member.is_empty())
                .map(str::to_owned)
                .collect();
            (
                (
                    parts[0].to_owned(),
                    parts[1].to_owned(),
                    parts[3].to_owned(),
                ),
                (parts[2].to_owned(), members),
            )
        })
        .collect()
}

fn current() -> BTreeMap<(String, String, String), (String, Vec<String>)> {
    let mut found = BTreeMap::new();
    for (relative, text) in wit_files() {
        for definition in definitions(&text) {
            found.insert(
                (relative.clone(), definition.interface, definition.name),
                (definition.kind, definition.members),
            );
        }
    }
    found
}

/// REQ: ABI-019
#[test]
fn doc_abi_019_no_frozen_definition_is_removed_or_changes_kind() {
    let current = current();
    for (key, (kind, _)) in golden() {
        let Some((current_kind, _)) = current.get(&key) else {
            panic!("{key:?} was removed after the freeze");
        };
        assert_eq!(current_kind, &kind, "{key:?} changed kind");
    }
}

/// REQ: ABI-019
#[test]
fn doc_abi_019_cases_are_only_appended() {
    let current = current();
    for (key, (kind, members)) in golden() {
        if kind == "record" {
            continue;
        }
        let Some((_, current_members)) = current.get(&key) else {
            continue;
        };
        assert!(
            current_members.len() >= members.len()
                && current_members[..members.len()] == members[..],
            "{key:?}: frozen cases {members:?} are not a prefix of {current_members:?}"
        );
    }
}

/// REQ: ABI-019
#[test]
fn doc_abi_019_records_keep_their_fields() {
    let current = current();
    for (key, (kind, members)) in golden() {
        if kind != "record" {
            continue;
        }
        let Some((_, current_members)) = current.get(&key) else {
            continue;
        };
        assert_eq!(current_members, &members, "{key:?}: record fields changed");
    }
}

/// REQ: ABI-019
#[test]
fn doc_abi_019_golden_covers_every_current_definition_of_0_1_0() {
    let golden = golden();
    let missing: Vec<_> = current()
        .into_keys()
        .filter(|key| !golden.contains_key(key))
        .collect();
    // A definition added after the freeze belongs to a later minor; in 0.1.0 there is none.
    assert!(
        missing.is_empty(),
        "not in golden/case_order.txt: {missing:#?}"
    );
}
