//! Consent records, revocation and the review dialog of `reference-grant-review-and-consent`
//! (USR-318, USR-319, USR-321).

use crate::common::*;

/// REQ: USR-318
#[test]
fn usr_318_every_consent_decision_is_stored_with_its_fields_and_listed_oldest_first() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    let granted = rig.cli(&["grant", NOTES, "fs.read", "--yes"]);
    assert_eq!(granted.exit_code, 0, "grant failed: {}", granted.stderr);
    let revoked = rig.cli(&["revoke", NOTES, "fs.read"]);
    assert_eq!(revoked.exit_code, 0, "revoke failed: {}", revoked.stderr);

    let listing = rig.cli(&["consents", NOTES]);
    assert_eq!(listing.exit_code, 0, "consents failed: {}", listing.stderr);
    let records = listing.stdout_jsonl;
    assert!(
        records.len() >= 3,
        "dialog, cli grant and revoke expected: {records:?}"
    );
    for record in &records {
        for key in ["plugin", "version", "grant", "decision", "time", "surface"] {
            assert!(record.get(key).is_some(), "record lacks {key}: {record}");
        }
        let time = record["time"].as_str().expect("time is a string");
        assert_eq!(time.len(), 19, "time must be YYYY-MM-DD HH:mm:ss: {time}");
    }
    assert!(records.iter().any(|record| record["surface"] == "dialog"));
    assert!(records.iter().any(|record| record["surface"] == "cli"));
    let times: Vec<String> = records
        .iter()
        .map(|record| record["time"].as_str().expect("time").to_string())
        .collect();
    let mut sorted = times.clone();
    sorted.sort();
    assert_eq!(times, sorted, "consent records must be listed oldest first");
}

/// REQ: USR-319
#[test]
fn usr_319_dialog_consent_is_revocable_in_two_key_presses_without_reason_or_restart() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    let server = rig.start_server("s1");
    let outcome = server.revoke_via_capabilities_tab(NOTES, "fs.read");
    assert!(
        outcome.keypresses <= 2,
        "at most 2 key presses, got {}",
        outcome.keypresses
    );
    assert!(
        outcome.prompts.is_empty(),
        "a normal revoke must not ask anything: {:?}",
        outcome.prompts
    );
    assert!(
        !outcome.restart_required,
        "revoking must not require a restart"
    );
    assert!(!server.granted(NOTES).contains(&"fs.read".to_string()));
}

/// REQ: USR-321
#[test]
fn usr_321_review_dialog_shows_full_wording_and_keeps_confirm_disabled_until_scrolled_to_end() {
    let rig = rig();
    let mut spec = pkg(NOTES, "1.0.0");
    for grant in [
        "fs.read",
        "fs.write",
        "http:example.invalid",
        "clipboard",
        "notify",
        "audio",
        "location",
    ] {
        spec = spec.grant(grant);
    }
    let dialog = rig.open_review_dialog(&spec, 60, 12);
    let below_fold = dialog.rows_below_fold();
    assert!(below_fold > 0, "the list must overflow the 12-row dialog");
    assert!(
        !dialog.confirm_enabled(),
        "Confirm must stay disabled above the end of the list"
    );
    dialog.scroll_to_end();
    assert!(
        dialog.confirm_enabled(),
        "Confirm must enable once the list is at its end"
    );
    assert!(
        dialog.all_text().contains(&dialog.full_wording("fs.read")),
        "the plain wording must be shown untruncated"
    );
}

/// REQ: USR-318
#[test]
fn usr_318_consent_records_carry_the_plugin_id_version_and_grant_string() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    let granted = rig.cli(&["grant", NOTES, "fs.read", "--yes"]);
    assert_eq!(granted.exit_code, 0, "grant failed: {}", granted.stderr);

    let listing = rig.cli(&["consents", NOTES]);
    assert_eq!(listing.exit_code, 0, "consents failed: {}", listing.stderr);
    let records = listing.stdout_jsonl;
    assert!(!records.is_empty(), "the grant must leave a consent record");
    for record in &records {
        assert_eq!(record["plugin"], NOTES, "wrong plugin id: {record}");
        assert_eq!(record["version"], "1.0.0", "wrong version: {record}");
        assert_eq!(record["grant"], "fs.read", "wrong grant string: {record}");
    }
}

/// REQ: USR-319
#[test]
fn usr_319_dialog_revoke_is_recorded_as_a_revoke_from_the_dialog_surface() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    let server = rig.start_server("s1");
    let _outcome = server.revoke_via_capabilities_tab(NOTES, "fs.read");

    let listing = rig.cli(&["consents", NOTES]);
    let records = listing.stdout_jsonl;
    assert!(
        records
            .iter()
            .any(|record| record["decision"] == "revoke" && record["surface"] == "dialog"),
        "the dialog revoke must be recorded: {records:?}"
    );
}

/// REQ: USR-321
#[test]
fn usr_321_a_single_screen_of_rows_enables_confirm_without_scrolling() {
    let rig = rig();
    let spec = pkg(NOTES, "1.0.0").grant("fs.read");
    let dialog = rig.open_review_dialog(&spec, 60, 12);
    assert_eq!(
        dialog.rows_below_fold(),
        0,
        "one row must fit the 12-row dialog"
    );
    assert!(
        dialog.confirm_enabled(),
        "Confirm must not wait for a scroll when no more than one screen of rows exists"
    );
}
