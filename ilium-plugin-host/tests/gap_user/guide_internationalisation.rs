//! Catalogues, language choice, plurals, number formats, text width and pseudo-locale of the guide
//! `guide-internationalisation` (USR-310 .. USR-317).

use std::fs;

use ilium_plugin_host::i18n::{
    choose_ui_language, format_log_time, format_number, load_plugin_catalogue, plural_category,
    LangEnv, Lookup, PluralCategory,
};
use ilium_plugin_host::text::{fit_to_box, terminal_width, wrap_to_width};

use crate::common::*;

/// REQ: USR-310
#[test]
fn usr_310_plugin_catalogue_is_flat_and_invalid_files_are_refused_with_file_and_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let i18n_dir = dir.path().join("i18n");
    fs::create_dir_all(&i18n_dir).expect("i18n dir");

    fs::write(i18n_dir.join("pt-BR.json"), r#"{"greeting":"Olá"}"#).expect("write");
    let loaded = load_plugin_catalogue(&i18n_dir.join("pt-BR.json")).expect("valid catalogue");
    assert_eq!(loaded.tag(), "pt-BR");

    let cases: [(&str, &str); 3] = [
        ("de.json", r#"{"count":5}"#),
        ("fr.json", r#"{"a":"x","a":"y"}"#),
        ("it.json", r#"{"b":"ok","b2":"ok"}"#),
    ];
    let invalid_non_string = cases[0];
    let duplicate = cases[1];
    for (file, body) in [invalid_non_string, duplicate] {
        fs::write(i18n_dir.join(file), body).expect("write");
        let error = match load_plugin_catalogue(&i18n_dir.join(file)) {
            Ok(_) => panic!("{file} must be refused"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("i18n.catalogue-invalid"), "{error}");
        assert!(
            error.contains(file),
            "the error must name the file: {error}"
        );
    }

    let oversize = format!("{{\"big\":\"{}\"}}", "x".repeat(256 * 1024 + 1));
    fs::write(i18n_dir.join("sv.json"), oversize).expect("write");
    let error = match load_plugin_catalogue(&i18n_dir.join("sv.json")) {
        Ok(_) => panic!("an oversize catalogue must be refused"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("i18n.catalogue-invalid"), "{error}");
}

/// REQ: USR-311
#[test]
fn usr_311_ui_language_follows_the_documented_order_and_doctor_prints_its_source() {
    let full = LangEnv::from_pairs(&[
        ("ILIUM_LANG", "pt-BR"),
        ("LC_ALL", "de_DE.UTF-8"),
        ("LC_MESSAGES", "fr_FR.UTF-8"),
        ("LANG", "it_IT.UTF-8"),
    ]);
    let setting_wins = choose_ui_language(&full, Some("ja"));
    assert_eq!(
        (setting_wins.tag.as_str(), setting_wins.source.as_str()),
        ("ja", "[ui] language")
    );
    let ilium_wins = choose_ui_language(&full, None);
    assert_eq!(
        (ilium_wins.tag.as_str(), ilium_wins.source.as_str()),
        ("pt-BR", "ILIUM_LANG")
    );
    let lc_all = choose_ui_language(&LangEnv::from_pairs(&[("LC_ALL", "de_DE.UTF-8")]), None);
    assert_eq!(lc_all.tag, "de");
    let default = choose_ui_language(&LangEnv::from_pairs(&[]), None);
    assert_eq!(default.tag, "en");

    rig().set_env("ILIUM_LANG", "pt-BR");
    let doctor = rig().cli(&["doctor"]);
    assert!(
        doctor.stdout_raw.contains("pt-BR"),
        "doctor must print the chosen tag"
    );
    assert!(
        doctor.stdout_raw.contains("ILIUM_LANG"),
        "doctor must print the source"
    );
}

/// REQ: USR-312
#[test]
fn usr_312_lookup_tries_exact_tag_then_language_then_english_and_falls_back_on_placeholder_mismatch(
) {
    let lookup = Lookup::from_entries(&[
        (
            "en",
            &[
                ("a.msg", "English a"),
                ("b.msg", "{0} bytes"),
                ("c.msg", "English c"),
            ][..],
        ),
        ("pt", &[("a.msg", "pt a"), ("b.msg", "PT {0} bytes")][..]),
        (
            "pt-BR",
            &[("a.msg", "exact a"), ("b.msg", "PT-BR no placeholder")][..],
        ),
    ]);
    assert_eq!(lookup.text("pt-BR", "a.msg"), "exact a", "exact tag wins");
    assert_eq!(
        lookup.text("pt-BR", "c.msg"),
        "English c",
        "unknown in pt-BR falls to en"
    );
    assert_eq!(
        lookup.text("pt", "b.msg"),
        "PT {0} bytes",
        "language without region is tried"
    );
    assert_eq!(
        lookup.text("pt-BR", "b.msg"),
        "{0} bytes",
        "a placeholder-count mismatch falls back to English for that message"
    );
    lookup.text("pt-BR", "b.msg");
    let mismatches = lookup
        .log_codes()
        .iter()
        .filter(|code| code.as_str() == "i18n.placeholder-mismatch")
        .count();
    assert_eq!(mismatches, 1, "the mismatch is logged once per message");
}

/// REQ: USR-313
#[test]
fn usr_313_plural_messages_use_cldr_categories_with_other_required_and_missing_categories_use_other(
) {
    assert_eq!(plural_category("ru", 3), PluralCategory::Few);
    assert_eq!(plural_category("ru", 5), PluralCategory::Many);
    assert_eq!(plural_category("en", 1), PluralCategory::One);
    assert_eq!(plural_category("en", 0), PluralCategory::Other);

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("de.json");
    fs::write(
        &path,
        r#"{"files.count":{"one":"1 Datei","other":"{0} Dateien"}}"#,
    )
    .expect("write");
    let catalogue = load_plugin_catalogue(&path).expect("catalogue with other");
    assert_eq!(
        catalogue.plural_text("de", "files.count", 0),
        "{0} Dateien",
        "missing zero uses other"
    );
    assert_eq!(catalogue.plural_text("de", "files.count", 1), "1 Datei");

    fs::write(&path, r#"{"files.count":{"one":"1 Datei"}}"#).expect("write");
    let error = match load_plugin_catalogue(&path) {
        Ok(_) => panic!("a plural message without other must be refused"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("other"), "{error}");
}

/// REQ: USR-314
#[test]
fn usr_314_numbers_and_dates_follow_the_language_while_logs_and_machine_output_do_not() {
    assert_eq!(format_number(1234.5, "de"), "1.234,5");
    assert_eq!(format_number(1234.5, "en"), "1,234.5");

    let stamp = 1_792_670_400_000_u64;
    let german_log = format_log_time(stamp, "de");
    let english_log = format_log_time(stamp, "en");
    assert_eq!(
        german_log, english_log,
        "log timestamps stay language independent"
    );
    assert_eq!(
        german_log.len(),
        19,
        "YYYY-MM-DD HH:mm:ss expected, got {german_log}"
    );

    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.set_ui_language("en");
    let english_json = rig.cli(&["list", "--json"]).stdout_raw;
    rig.set_ui_language("de");
    let german_json = rig.cli(&["list", "--json"]).stdout_raw;
    assert_eq!(
        german_json, english_json,
        "--json output must not change with the language"
    );
}

/// REQ: USR-315
#[test]
fn usr_315_text_is_measured_in_cells_wide_glyphs_never_split_and_overflow_is_an_ellipsis() {
    let wide = "漢字テキスト";
    assert_eq!(terminal_width(wide), 12, "each CJK glyph is two cells");
    assert_eq!(
        terminal_width("e\u{301}"),
        1,
        "a combining mark adds no cells"
    );

    let wrapped = wrap_to_width("漢字漢字漢", 5);
    for line in &wrapped {
        assert!(terminal_width(line) <= 5, "line overflows the box: {line}");
    }
    assert_eq!(
        wrapped.concat(),
        "漢字漢字漢",
        "no glyph may be lost or split"
    );

    let fitted = fit_to_box("abcdefghijklmnop", 8);
    assert!(
        terminal_width(&fitted) <= 8,
        "overflow not prevented: {fitted}"
    );
    assert!(
        fitted.ends_with('\u{2026}'),
        "a truncated string ends with an ellipsis: {fitted}"
    );
}

/// REQ: USR-316
#[test]
fn usr_316_pseudo_locale_en_xa_expands_text_and_lint_flags_layout_overflow() {
    let rig = rig();
    let pseudo = ilium_plugin_host::i18n::text_for("en-XA", "host.perm.fs.read");
    let english = ilium_plugin_host::i18n::text_for("en", "host.perm.fs.read");
    assert!(
        pseudo.starts_with('[') && pseudo.ends_with(']'),
        "brackets expected: {pseudo}"
    );
    assert!(
        pseudo.chars().count() * 10 >= english.chars().count() * 14,
        "en-XA must expand by about 40 percent: {pseudo} vs {english}"
    );

    let clipping = pkg(NOTES, "1.0.0").view_clipping_at("en-XA");
    let path = rig.write_package(&clipping);
    let lint = rig.cli(&["lint", &path.to_string_lossy()]);
    assert_ne!(
        lint.exit_code, 0,
        "lint must fail a view that clips at en-XA"
    );
    assert!(
        lint.stdout_raw.contains("i18n.layout-overflow")
            || lint.stderr.contains("i18n.layout-overflow")
    );
}

/// REQ: USR-317
#[test]
fn usr_317_plugin_cannot_override_host_permission_wording_or_consent_prompts() {
    let rig = rig();
    let spec = pkg(NOTES, "1.0.0")
        .grant("fs.read")
        .catalogue("en", &[("host.perm.fs.read", "Totally harmless")]);
    install_ok(&rig, &spec);
    let review = rig.last_review().join("\n");
    assert!(
        !review.contains("Totally harmless"),
        "a plugin overrode host wording: {review}"
    );
    let host_text = rig.english_text("host.perm.fs.read");
    assert!(
        review.contains(&host_text),
        "the host catalogue text must be shown"
    );
}

/// REQ: USR-317
#[test]
fn usr_317_plugin_catalogue_override_of_a_second_permission_is_ignored_too() {
    let rig = rig();
    let spec = pkg(NOTES, "1.0.0")
        .grant("fs.write")
        .catalogue("en", &[("host.perm.fs.write", "Nothing to see here")]);
    install_ok(&rig, &spec);
    let review = rig.last_review().join("\n");
    assert!(
        !review.contains("Nothing to see here"),
        "a plugin overrode the host wording of fs.write: {review}"
    );
    let host_text = rig.english_text("host.perm.fs.write");
    assert!(
        review.contains(&host_text),
        "the host catalogue text for fs.write must be shown: {review}"
    );
}

/// REQ: USR-310
#[test]
fn usr_310_invalid_utf8_catalogue_is_refused_naming_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("nl.json");
    fs::write(&path, [b'{', 0xff, 0xfe, b'}']).expect("write");
    let error = match load_plugin_catalogue(&path) {
        Ok(_) => panic!("invalid UTF-8 must be refused"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("i18n.catalogue-invalid"), "{error}");
    assert!(
        error.contains("nl.json"),
        "the error must name the file: {error}"
    );
}

/// REQ: USR-311
#[test]
fn usr_311_lc_messages_beats_lang_when_lc_all_is_absent() {
    let messages_first = choose_ui_language(
        &LangEnv::from_pairs(&[("LC_MESSAGES", "fr_FR.UTF-8"), ("LANG", "it_IT.UTF-8")]),
        None,
    );
    assert_eq!(messages_first.tag, "fr", "LC_MESSAGES must win over LANG");
    let lang_only = choose_ui_language(&LangEnv::from_pairs(&[("LANG", "it_IT.UTF-8")]), None);
    assert_eq!(lang_only.tag, "it", "LANG is the last locale variable tried");
}

/// REQ: USR-312
#[test]
fn usr_312_tag_without_any_table_shows_english_text() {
    let lookup = Lookup::from_entries(&[("en", &[("a.msg", "English a")][..])]);
    assert_eq!(
        lookup.text("sv", "a.msg"),
        "English a",
        "a language tag with no table at all shows English"
    );
}

/// REQ: USR-313
#[test]
fn usr_313_russian_plural_rule_puts_21_in_one_and_11_in_many() {
    assert_eq!(plural_category("ru", 21), PluralCategory::One);
    assert_eq!(plural_category("ru", 11), PluralCategory::Many);
}

/// REQ: USR-314
#[test]
fn usr_314_log_timestamp_has_fixed_separator_positions_in_every_language() {
    let stamp = 1_792_670_400_000_u64;
    for language in ["de", "en", "ja"] {
        let text = format_log_time(stamp, language);
        let bytes = text.as_bytes();
        assert_eq!(
            (bytes[4], bytes[7], bytes[10], bytes[13], bytes[16]),
            (b'-', b'-', b' ', b':', b':'),
            "YYYY-MM-DD HH:mm:ss expected for {language}, got {text}"
        );
    }
}

/// REQ: USR-315
#[test]
fn usr_315_truncated_wide_text_stays_inside_the_box_and_keeps_whole_glyphs() {
    let fitted = fit_to_box("漢字テキスト漢字テキスト", 5);
    assert!(terminal_width(&fitted) <= 5, "overflow: {fitted}");
    assert!(fitted.ends_with('\u{2026}'), "no ellipsis: {fitted}");
    assert!(
        fitted
            .chars()
            .all(|glyph| glyph == '\u{2026}' || "漢字テキスト".contains(glyph)),
        "a glyph was altered: {fitted}"
    );
}

/// REQ: USR-316
#[test]
fn usr_316_a_view_that_fits_at_en_xa_passes_lint() {
    let rig = rig();
    let path = rig.write_package(&pkg(NOTES, "1.0.0"));
    let lint = rig.cli(&["lint", &path.to_string_lossy()]);
    assert_eq!(lint.exit_code, 0, "a fitting view must lint clean: {}", lint.stdout_raw);
}
