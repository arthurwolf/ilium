//! Rule statements AUT-623 to AUT-625: the host text catalogue, the launch languages and the
//! pseudo-locale release gate, as the internationalisation guide and the requirement table state them.
use crate::support::{assert_page_line, assert_rule};

/// REQ: AUT-623
#[test]
fn aut_623_project_owns_catalogue_with_english_source_and_all_core_text() {
    assert_rule(
        "AUT-623",
        "MUST",
        &[
            "The Ilium project MUST own the host text catalogue",
            "keep English as its source",
            "including the text of the 30 bundled core plugins",
        ],
    );
}

/// REQ: AUT-623
#[test]
fn aut_623_release_may_ship_missing_text_only_when_english_falls_back() {
    assert_page_line(
        "AUT-623",
        &[
            "a release MUST NOT ship a launch language with a host text missing from its catalogue",
            "unless the missing text falls back to English under the lookup order of [USR-312]",
        ],
    );
}

/// REQ: AUT-624
#[test]
fn aut_624_launch_languages_are_english_german_and_french() {
    assert_rule(
        "AUT-624",
        "MUST",
        &[
            "The host MUST ship its catalogue in these launch languages",
            "English, German and French",
        ],
    );
}

/// REQ: AUT-624
#[test]
fn aut_624_tag_outside_launch_languages_shows_english_for_every_host_text() {
    unimplemented!("ilium-plugin-host::i18n::host_text_for_language_tag")
}

/// REQ: AUT-625
#[test]
fn aut_625_release_candidate_runs_en_xa_check_and_layout_overflow_blocks_release() {
    assert_rule(
        "AUT-625",
        "MUST",
        &[
            "`en-XA` pseudo-locale check",
            "`i18n.layout-overflow` finding MUST block the release",
        ],
    );
}

/// REQ: AUT-625
#[test]
fn aut_625_en_xa_check_covers_every_host_dialog_and_core_plugin_view() {
    unimplemented!("ilium-plugin-testkit::i18n::pseudo_locale_check")
}
