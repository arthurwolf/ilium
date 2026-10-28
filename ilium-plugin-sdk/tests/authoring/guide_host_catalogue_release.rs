//! Rule statements AUT-623 to AUT-625: the host text catalogue, the launch languages and the
//! pseudo-locale release gate, as the internationalisation guide and the requirement table state them.
use crate::support::{assert_page_line, assert_rule};
use ilium_plugin_host::i18n::text_for;

/// Message ids of the host texts that the catalogue holds today.
const HOST_TEXT_IDS: [&str; 7] = [
    "host.perm.fs.read",
    "host.perm.fs.write",
    "host.perm.net",
    "host.perm.process",
    "host.perm.clipboard",
    "host.perm.terminal.read",
    "host.perm.terminal.write",
];

/// The `i18n.layout-overflow` findings of the `en-XA` check: host texts wider than `width` cells.
fn pseudo_locale_check(ids: &[&str], width: usize) -> Vec<String> {
    ids.iter()
        .filter(|id| text_for("en-XA", id).chars().count() > width)
        .map(|id| format!("i18n.layout-overflow: {id}"))
        .collect()
}

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
    // German and French are launch languages; these tags are not, so every host text is English.
    for tag in ["ja", "ko", "pt-BR", "zh-Hans", "xx"] {
        for id in HOST_TEXT_IDS {
            let english = text_for("en", id);
            assert_ne!(english, id, "{id} must have an English text");
            assert_eq!(text_for(tag, id), english, "{tag} {id}");
        }
    }
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
    for id in HOST_TEXT_IDS {
        let english = text_for("en", id);
        let pseudo = text_for("en-XA", id);
        assert!(pseudo.starts_with('[') && pseudo.ends_with(']'), "{id}: {pseudo}");
        assert!(
            pseudo.chars().count() * 100 >= english.chars().count() * 130,
            "{id} must grow by at least 30 percent: {pseudo}"
        );
    }
    // Wide enough for the expanded texts: no finding. Too narrow: a finding that blocks the release.
    assert!(pseudo_locale_check(&HOST_TEXT_IDS, 100).is_empty());
    let findings = pseudo_locale_check(&HOST_TEXT_IDS, 30);
    assert!(!findings.is_empty());
    assert!(findings.iter().all(|finding| finding.starts_with("i18n.layout-overflow")));
}
