//! The namespaced code constants of `ilium_plugin_api::codes` against the manual tables, and
//! the refusal order of ABI-021. Doc lints except the registry test.

use ilium_plugin_api::codes;
use ilium_plugin_api::version::REFUSAL_ORDER;

use crate::support::{manual_page, manual_pages};

/// The first backticked cell of a table row, when the row starts with one.
fn first_code_cell(line: &str) -> Option<(&str, Vec<&str>)> {
    let cells: Vec<&str> = line.split('|').map(str::trim).collect();
    let first = cells.get(1)?;
    let code = first.strip_prefix('`')?.strip_suffix('`')?;
    code.contains('.').then(|| (code, cells[2..].to_vec()))
}

/// REQ: ABI-021, ABI-023
#[test]
fn doc_every_catalogue_code_has_a_constant() {
    let page = manual_page("reference-manifest-schema.md");
    let mut missing = Vec::new();
    for line in page.lines() {
        let Some((code, rest)) = first_code_cell(line) else {
            continue;
        };
        // Catalogue rows carry a numeric stage; the retired-code table does not.
        let has_stage = rest
            .first()
            .is_some_and(|stage| !stage.is_empty() && stage.chars().all(|c| c.is_ascii_digit()));
        if has_stage && !codes::ALL.contains(&code) {
            missing.push(code.to_owned());
        }
    }
    assert!(missing.is_empty(), "codes without a constant: {missing:#?}");
}

/// REQ: ABI-008, ABI-027
#[test]
fn doc_every_trap_code_has_a_constant() {
    let page = manual_page("reference-errors-and-limits.md");
    let missing: Vec<String> = page
        .lines()
        .filter_map(first_code_cell)
        .map(|(code, _)| code)
        .filter(|code| code.starts_with("trap."))
        .filter(|code| !codes::ALL.contains(code))
        .map(str::to_owned)
        .collect();
    assert!(missing.is_empty(), "{missing:#?}");
}

/// REQ: ABI-021, ABI-023, ABI-024
#[test]
fn doc_every_code_constant_is_named_in_the_manual() {
    let manual: String = manual_pages().into_iter().map(|(_, text)| text).collect();
    let unknown: Vec<&str> = codes::ALL
        .iter()
        .copied()
        .filter(|code| !manual.contains(&format!("`{code}`")))
        .collect();
    assert!(unknown.is_empty(), "{unknown:#?}");
}

/// REQ: ABI-021
#[test]
fn doc_codes_are_unique() {
    let mut seen = std::collections::BTreeSet::new();
    for code in codes::ALL {
        assert!(seen.insert(*code), "{code} twice");
    }
}

/// REQ: ABI-021
#[test]
fn doc_abi_021_refusal_order_follows_the_abi_evolution_table() {
    let page = manual_page("reference-abi-evolution.md");
    let section = page
        .split("## 4. The window and the refusal codes")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("section 4 of reference-abi-evolution.md");
    let in_page: Vec<&str> = section
        .lines()
        .filter(|line| line.starts_with('|'))
        .filter_map(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            let cell = cells.get(2)?;
            let code = cell.strip_prefix('`')?;
            code.split('`').next()
        })
        .collect();
    assert_eq!(in_page, REFUSAL_ORDER.to_vec());
}

/// REQ: ABI-024
#[test]
fn abi_024_wasi_forbidden_code_is_in_the_registry() {
    assert_eq!(codes::component::WASI_FORBIDDEN, "component.wasi-forbidden");
    assert!(ilium_plugin_api::error::registry()
        .iter()
        .any(|info| info.code == codes::component::WASI_FORBIDDEN));
}

/// REQ: ABI-023
#[test]
fn abi_023_capability_not_requested_is_registered() {
    assert_eq!(
        codes::host::CAPABILITY_NOT_REQUESTED,
        "host.capability-not-requested"
    );
    assert!(ilium_plugin_api::error::registry()
        .iter()
        .any(|info| info.code == codes::host::CAPABILITY_NOT_REQUESTED));
}

/// REQ: ABI-023
#[test]
fn abi_023_every_constant_is_in_the_registry() {
    let registry = ilium_plugin_api::error::registry();
    for code in codes::ALL {
        assert!(
            registry.iter().any(|info| info.code == *code),
            "{code} not registered"
        );
    }
}
