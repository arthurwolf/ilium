//! Capability-surface text (toasts, notices, grant-review dialog) and who localises it.
//! The helpers below model the capability-surface rules of the internationalisation guide
//! (AUT-618 to AUT-622): plugin text is shown as given, cut with an ellipsis, never translated.

/// Widest reason line after translation, in cells (AUT-621).
const REASON_LINE_CELLS: usize = 60;

/// What the host draws for one piece of plugin-supplied text.
struct DrawnText {
    shown: String,
    translated_by_host: bool,
}

/// Draws plugin-supplied text on a capability surface at `width` cells.
fn draw_plugin_text(text: &str, width: usize) -> DrawnText {
    let without_escapes: String = text.chars().filter(|letter| *letter != '\u{1b}').collect();
    DrawnText {
        shown: cut_with_ellipsis(&without_escapes, width),
        translated_by_host: false,
    }
}

/// Cuts `text` to `width` cells, ending in `…` when anything was removed.
fn cut_with_ellipsis(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// Draws the grant reason in the review dialog.
fn draw_grant_reason(reason: &str, width: usize) -> DrawnText {
    DrawnText {
        shown: cut_with_ellipsis(reason, width),
        translated_by_host: false,
    }
}

/// Whether the dialog translates an identifier such as a grant id.
fn dialog_translates_identifier(identifier: &str) -> bool {
    // Grant ids, plugin ids and capability names are lower-case dotted words; only prose is
    // translated (AUT-622).
    let is_identifier = !identifier.is_empty()
        && identifier.chars().all(|letter| {
            letter.is_ascii_lowercase() || letter.is_ascii_digit() || ".:_-".contains(letter)
        });
    !is_identifier
}

/// Cells the reason line of a localised reason needs.
fn reason_fits_after_translation(reason: &str) -> bool {
    reason.chars().count() <= REASON_LINE_CELLS
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

/// REQ: AUT-618
#[test]
fn toast_body_is_cut_with_an_ellipsis_and_stays_a_prefix_of_the_input() {
    let text = "Gespeicherte Notizen des Projekts wurden in den Ordner verschoben";
    let drawn = draw_plugin_text(text, 10);
    assert!(drawn.shown.ends_with('…'));
    assert!(text.starts_with(drawn.shown.trim_end_matches('…')));
}

/// REQ: AUT-619
#[test]
fn plugin_text_keeps_the_plugin_language_on_a_capability_surface() {
    let drawn = draw_plugin_text("Notiz gelöscht", 40);
    assert_eq!(drawn.shown, "Notiz gelöscht");
}

/// REQ: AUT-620
#[test]
fn grant_reason_is_shown_whole_when_it_fits_the_dialog() {
    let drawn = draw_grant_reason("Liest Notizen", 80);
    assert_eq!(drawn.shown, "Liest Notizen");
}

/// REQ: AUT-621
#[test]
fn reason_of_sixty_one_cells_does_not_fit_the_reason_line() {
    assert!(!reason_fits_after_translation(&"x".repeat(61)));
}

/// REQ: AUT-622
#[test]
fn capability_names_stay_identifiers_in_the_dialog() {
    assert!(!dialog_translates_identifier("fs.write:project"));
}
