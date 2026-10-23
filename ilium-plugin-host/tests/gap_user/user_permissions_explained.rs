//! Message catalogues and English fallback of the page `user-permissions-explained`
//! (USR-259, USR-260).

use crate::common::*;

/// REQ: USR-259
#[test]
fn usr_259_review_dialog_and_error_text_come_from_the_loaded_message_catalogue() {
    let rig = rig();
    rig.load_catalogue(
        "xx-TEST",
        &[
            ("host.perm.fs.read", "Test wording for reading files"),
            ("package.bad-signature", "TEST: signature refused"),
        ],
    );
    rig.set_ui_language("xx-TEST");
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    assert!(
        rig.last_review()
            .join("\n")
            .contains("Test wording for reading files"),
        "the review dialog must use the catalogue text"
    );
    let failed = install(&rig, &pkg(NOTES, "1.0.0").bad_signature());
    assert!(
        failed.stderr.contains("TEST: signature refused"),
        "the error must use the catalogue text: {}",
        failed.stderr
    );
}

/// REQ: USR-260
#[test]
fn usr_260_missing_message_falls_back_to_english_and_broken_catalogue_gives_english_recovery() {
    let rig = rig();
    rig.load_catalogue("de", &[("host.perm.fs.read", "Dateien lesen")]);
    rig.set_ui_language("de");
    install_ok(&rig, &pkg(NOTES, "1.0.0").grant("fs.read"));
    let review = rig.last_review().join("\n");
    assert!(
        review.contains("Dateien lesen"),
        "translated message expected: {review}"
    );
    let english_bad_signature = rig.english_text("package.bad-signature");
    let failed = install(&rig, &pkg(NOTES, "1.0.0").bad_signature());
    assert!(
        failed.stderr.contains(&english_bad_signature),
        "a message without German text must fall back to English: {}",
        failed.stderr
    );

    rig.load_catalogue_raw("de", b"{ this is not json");
    let server = rig.start_server("s1");
    let english_recovery_title = rig.english_text("recovery.title");
    assert!(
        server
            .render_recovery_page()
            .contains(&english_recovery_title),
        "the recovery page must fall back to English when its catalogue is broken"
    );
}
