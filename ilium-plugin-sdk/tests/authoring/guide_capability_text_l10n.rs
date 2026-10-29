//! Capability-surface text (toasts, notices, grant-review dialog) and who localises it.
//! The renderer is a red-phase skeleton: it fails until the host text pipeline exists.

/// What the host draws for one piece of plugin-supplied text.
struct DrawnText {
    shown: String,
    translated_by_host: bool,
}

/// Draws plugin-supplied text on a capability surface at `width` cells.
fn draw_plugin_text(_text: &str, _width: usize) -> DrawnText {
    unimplemented!("capability-surface text pipeline")
}

/// Draws the grant reason in the review dialog.
fn draw_grant_reason(_reason: &str, _width: usize) -> DrawnText {
    unimplemented!("grant-review dialog reason line")
}

/// Whether the dialog translates an identifier such as a grant id.
fn dialog_translates_identifier(_identifier: &str) -> bool {
    unimplemented!("grant-review dialog identifiers")
}

/// Cells the reason line of a localised reason needs.
fn reason_fits_after_translation(_reason: &str) -> bool {
    unimplemented!("grant reason length check")
}

/// REQ: AUT-618
#[test]
fn toast_body_is_shown_unchanged_apart_from_escape_removal() {
    let drawn = draw_plugin_text("Gespeichert \u{1b}[31mrot", 40);
    assert_eq!(drawn.shown, "Gespeichert [31mrot");
    assert!(!drawn.translated_by_host);
}

/// REQ: AUT-619
#[test]
fn host_fixed_words_are_not_the_plugin_language() {
    let drawn = draw_plugin_text("Notiz gelöscht", 40);
    assert!(!drawn.shown.contains("Show toasts"));
}

/// REQ: AUT-620
#[test]
fn grant_reason_is_verbatim_and_cut_with_an_ellipsis() {
    let drawn = draw_grant_reason(
        "Lit les notes du projet pour les afficher dans la barre",
        20,
    );
    assert!(drawn.shown.ends_with('…'));
    assert!(!drawn.translated_by_host);
}

/// REQ: AUT-621
#[test]
fn translated_reason_of_sixty_cells_fits() {
    assert!(reason_fits_after_translation(
        "Zeigt die Notizen des Projekts in der Statusleiste an"
    ));
}

/// REQ: AUT-622
#[test]
fn identifiers_in_the_dialog_are_never_translated() {
    assert!(!dialog_translates_identifier("notify.toast"));
}
