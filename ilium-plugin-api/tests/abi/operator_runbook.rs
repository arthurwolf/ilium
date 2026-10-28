//! ABI-076: the operator runbook for an ABI incompatibility after a host upgrade.

use crate::support::manual_page;

/// REQ: ABI-076
#[test]
fn doc_abi_076_runbook_gives_triage_pin_roll_back_and_bulk_report_in_order() {
    let page = manual_page("reference-abi-major-versions.md");
    let section = page
        .split("## Operator procedure for ABI incompatibility")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("reference-abi-major-versions.md has the operator procedure section");
    // The requirement row itself names the four steps, so it is excluded from the search.
    let body: String = section
        .lines()
        .filter(|line| !line.trim_start().starts_with("- [ABI-"))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    let positions: Vec<usize> = ["triage", "pin", "roll back", "bulk report"]
        .iter()
        .map(|step| {
            body.find(step)
                .unwrap_or_else(|| panic!("runbook lacks step {step}"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "runbook steps out of order: {positions:?}"
    );
}

/// REQ: ABI-076
#[test]
fn doc_abi_076_runbook_names_the_refusal_code_that_starts_it() {
    let page = manual_page("reference-abi-major-versions.md");
    let section = page
        .split("## Operator procedure for ABI incompatibility")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("reference-abi-major-versions.md has the operator procedure section");
    assert!(
        section.contains("package.api-unsupported"),
        "the runbook must name the refusal code that starts it"
    );
}
