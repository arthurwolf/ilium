//! Error explanations and the Details page contacts of the page `user-plugin-store`
//! (USR-347, USR-349).

use ilium_plugin_host::errors::{error_explanation, user_visible_error_codes};

use crate::common::*;

/// True when the text is exactly one sentence that ends with a full stop.
fn is_one_sentence(text: &str) -> bool {
    let trimmed = text.trim();
    let Some(body) = trimmed.strip_suffix('.') else {
        return false;
    };
    !body.is_empty() && !body.chars().any(|c| matches!(c, '.' | '!' | '?'))
}

/// REQ: USR-347
#[test]
fn usr_347_every_user_visible_code_has_a_one_sentence_cause_and_next_step() {
    let codes = user_visible_error_codes();
    assert!(
        !codes.is_empty(),
        "the user-visible code table must not be empty"
    );
    for code in codes {
        let explanation =
            error_explanation(code).unwrap_or_else(|| panic!("{code} has no catalogue entry"));
        assert!(
            is_one_sentence(&explanation.cause),
            "cause of {code} is not one sentence"
        );
        assert!(
            is_one_sentence(&explanation.next_step),
            "next step of {code} is not one sentence"
        );
    }
}

/// REQ: USR-347
#[test]
fn usr_347_explain_exits_0_for_a_known_code_and_2_for_an_unknown_one() {
    let rig = rig();
    let known = rig.cli(&["explain", "package.too-large"]);
    assert_eq!(known.exit_code, 0, "{}", known.stderr);
    assert!(
        !known.stdout_raw.trim().is_empty(),
        "the explanation must be printed"
    );
    let unknown = rig.cli(&["explain", "no.such-code"]);
    assert_eq!(unknown.exit_code, 2, "an unknown code must exit 2");
}

/// REQ: USR-349
#[test]
fn usr_349_details_page_shows_contacts_as_text_without_opening_a_link() {
    let rig = rig();
    let spec = pkg(NOTES, "1.0.0")
        .contact("https://example.com/help")
        .security_contact("security@example.com");
    install_ok(&rig, &spec);
    let server = rig.start_server("s1");
    let details = server.open_details_page(NOTES);
    assert!(
        details.text().contains("security@example.com"),
        "security contact missing"
    );
    assert!(
        details.text().contains("https://example.com/help"),
        "contact missing"
    );
    assert!(
        rig.links_opened().is_empty(),
        "no link may open without the consent prompt"
    );
}

/// REQ: USR-349
#[test]
fn usr_349_report_prints_contacts_version_and_fingerprint_and_makes_no_request() {
    let rig = rig();
    let spec = pkg(NOTES, "1.0.0")
        .contact("https://example.com/help")
        .security_contact("security@example.com");
    install_ok(&rig, &spec);
    rig.block_network();
    let report = rig.cli(&["report", NOTES]);
    assert_eq!(report.exit_code, 0, "{}", report.stderr);
    let fingerprint = rig.key_fingerprint(NOTES);
    assert!(
        report.stdout_raw.contains("security@example.com"),
        "{}",
        report.stdout_raw
    );
    assert!(
        report.stdout_raw.contains("1.0.0"),
        "version missing: {}",
        report.stdout_raw
    );
    assert!(
        report.stdout_raw.contains(&fingerprint),
        "fingerprint missing"
    );
    assert_eq!(rig.network_attempts(), 0, "report must send nothing");
}
