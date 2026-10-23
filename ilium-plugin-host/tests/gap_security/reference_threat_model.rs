//! Threat model page (SEC-706, SEC-714, SEC-715).

use crate::support::*;

/// Every `SEC-nnn` id written in a text, in order of appearance (no duplicates removed).
fn sec_ids_in(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = text;
    while let Some(position) = rest.find("SEC-") {
        let after = &rest[position..];
        let digits: String = after[4..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if digits.len() == 3 {
            ids.push(format!("SEC-{digits}"));
        }
        rest = &after[4..];
    }
    ids
}

/// REQ: SEC-706
#[test]
fn sec_706_component_with_a_disabled_proposal_is_refused_before_compilation() {
    let rig = rig();
    let output = install(
        &rig,
        &pkg(NOTES, "1.0.0").component_with_proposal("threads"),
    );
    assert_ne!(
        output.exit_code, 0,
        "a module using threads must be refused"
    );
    let refusal = output
        .stdout_jsonl
        .iter()
        .find(|line| line["code"] == "manifest.component-invalid")
        .expect("the refusal must carry manifest.component-invalid");
    assert!(
        refusal["detail"].as_str().unwrap_or("").contains("threads"),
        "the detail must name the disabled proposal, got {refusal}"
    );
    assert_eq!(
        rig.compile_count(NOTES),
        0,
        "no compilation may start for a component the validator refused"
    );
}

/// REQ: SEC-714
#[test]
fn sec_714_every_asset_row_names_a_control_whose_sec_id_exists() {
    let threat_model = manual_page("reference-threat-model");
    let policy = manual_page("reference-requirements-security-policy");
    for asset in [
        "user files",
        "terminal content",
        "secrets",
        "tokens",
        "trust store",
        "grants",
        "audit log",
    ] {
        assert!(
            threat_model.to_lowercase().contains(asset),
            "the threat model must list the asset `{asset}`"
        );
    }
    let referenced = sec_ids_in(&threat_model);
    assert!(
        !referenced.is_empty(),
        "the threat model must name the SEC id of each blocking control"
    );
    for sec_id in referenced {
        assert!(
            policy.contains(&format!("| {sec_id} |")),
            "the threat model cites {sec_id}, which the security policy does not define"
        );
    }
}

/// REQ: SEC-715
#[test]
fn sec_715_threat_model_lists_out_of_scope_attackers_under_not_defended() {
    let page = manual_page("reference-threat-model");
    let heading = page
        .find("Not defended")
        .expect("the threat model must have a `Not defended` heading");
    let section = &page[heading..];
    for attacker in [
        "root",
        "malicious kernel",
        "hardware side channel",
        "compromised terminal emulator",
    ] {
        assert!(
            section.to_lowercase().contains(attacker),
            "the `Not defended` section must list the attacker `{attacker}`"
        );
    }
}
