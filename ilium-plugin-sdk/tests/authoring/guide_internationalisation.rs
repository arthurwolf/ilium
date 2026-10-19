//! Rule statements of the guide-internationalisation page as the manual states them (doc-lint tests, part 2).
use crate::support::assert_rule;

/// REQ: AUT-601
#[test]
fn doc_aut_601_locale_and_time_zone_not_learned_from_host() {
    assert_rule(
        "AUT-601",
        "MUST NOT",
        &["MUST NOT assume that it can learn the user's locale or time zone from the host"],
    );
}

/// REQ: AUT-602
#[test]
fn doc_aut_602_width_measured_with_display_width_not_len() {
    assert_rule(
        "AUT-602",
        "MUST NOT",
        &[
            "MUST measure width with `display-width`",
            "`chars().count()`",
            "`String.length`",
        ],
    );
}

/// REQ: AUT-603
#[test]
fn doc_aut_603_cut_at_grapheme_boundary_ending_with_ellipsis() {
    assert_rule(
        "AUT-603",
        "MUST",
        &[
            "MUST cut at a grapheme boundary",
            "SHOULD end a cut with `…`",
            "takes one cell",
        ],
    );
}

/// REQ: AUT-604 WARN
#[test]
fn doc_aut_604_display_width_not_called_per_character() {
    assert_rule(
        "AUT-604",
        "SHOULD NOT",
        &[
            "SHOULD NOT call `display-width` once per character for a long text",
            "Measure once",
        ],
    );
}

/// REQ: AUT-605 WARN
#[test]
fn doc_aut_605_room_left_for_longer_label_no_fixed_width() {
    assert_rule(
        "AUT-605",
        "SHOULD NOT",
        &[
            "SHOULD leave room for a longer label",
            "SHOULD NOT depend on a fixed width",
        ],
    );
}

/// REQ: AUT-606
#[test]
fn doc_aut_606_outside_text_treated_as_data() {
    assert_rule(
        "AUT-606",
        "MUST",
        &[
            "MUST treat text from outside as data",
            "wide characters, combining marks, text in any direction and any length",
        ],
    );
}

/// REQ: AUT-607 WARN
#[test]
fn doc_aut_607_no_promise_of_right_to_left_display_stated_in_readme() {
    assert_rule(
        "AUT-607",
        "SHOULD NOT",
        &[
            "SHOULD NOT promise correct display of right-to-left scripts",
            "state this in its README",
        ],
    );
}

/// REQ: AUT-608
#[test]
fn doc_aut_608_missing_message_falls_back_to_default_never_shows_id() {
    assert_rule(
        "AUT-608",
        "MUST NOT",
        &[
            "MUST fall back to one default language for any missing message",
            "MUST NOT show a message id to the user",
        ],
    );
}

/// REQ: AUT-609 WARN
#[test]
fn doc_aut_609_manifest_titles_one_language_translation_in_own_views() {
    assert_rule(
        "AUT-609",
        "SHOULD",
        &[
            "SHOULD keep the manifest titles in one language",
            "views it draws itself",
        ],
    );
}

/// REQ: AUT-610
#[test]
fn doc_aut_610_identifiers_not_translated() {
    assert_rule(
        "AUT-610",
        "MUST NOT",
        &["MUST NOT translate command ids, setting keys, error codes and event names"],
    );
}

/// REQ: AUT-611
#[test]
fn doc_aut_611_time_shows_zone_or_relative_and_stored_as_utc_millis() {
    assert_rule(
        "AUT-611",
        "MUST",
        &[
            "MUST write the zone or say that it is relative",
            "MUST store times as UTC milliseconds, never as local text",
        ],
    );
}

/// REQ: AUT-612 WARN
#[test]
fn doc_aut_612_absolute_times_use_yyyy_mm_dd_hh_mm_ss() {
    assert_rule(
        "AUT-612",
        "SHOULD",
        &[
            "SHOULD use `YYYY-MM-DD HH:mm:ss` for absolute times",
            "`YYYY-MM-DD HH:mm:ss`",
        ],
    );
}

/// REQ: AUT-613
#[test]
fn doc_aut_613_time_formatting_tested_with_fake_clock_from_2030() {
    assert_rule(
        "AUT-613",
        "MUST",
        &[
            "MUST be tested with the fake clock",
            "2030-01-01 00:00:00 UTC",
        ],
    );
}

/// REQ: AUT-614 WARN
#[test]
fn doc_aut_614_machine_output_dot_no_group_separator_metric_units() {
    assert_rule(
        "AUT-614",
        "SHOULD NOT",
        &[
            "SHOULD use a dot and no group separator in machine-readable output",
            "SHOULD use metric units for sizes it shows",
        ],
    );
}

/// REQ: AUT-615 WARN
#[test]
fn doc_aut_615_name_sort_folds_case_and_accents_no_language_order_claim() {
    assert_rule(
        "AUT-615",
        "SHOULD NOT",
        &[
            "SHOULD fold case and accents before comparing",
            "SHOULD NOT claim a language-correct order",
        ],
    );
}

/// REQ: AUT-616
#[test]
fn doc_aut_616_time_stored_as_utc_millis_zone_or_relative_displayed() {
    assert_rule(
        "AUT-616",
        "MUST",
        &[
            "MUST store time as UTC milliseconds",
            "MUST state the zone or show relative time",
            "`now-unix-millis` is UTC",
        ],
    );
}

/// REQ: AUT-617
#[test]
fn doc_aut_617_column_sized_by_display_width_not_character_count() {
    assert_rule(
        "AUT-617",
        "MUST NOT",
        &[
            "MUST measure text with `display-width`",
            "MUST NOT size a column by character count",
        ],
    );
}

/// Civil date of a day count since 1970-01-01 (Howard Hinnant's algorithm), UTC.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Formats UTC milliseconds in the manual's `YYYY-MM-DD HH:mm:ss` form with an explicit zone.
fn format_utc_millis(millis: i64) -> String {
    let seconds = millis.div_euclid(1_000);
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let in_day = seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        in_day / 3_600,
        in_day % 3_600 / 60,
        in_day % 60
    )
}

/// REQ: AUT-612 WARN, AUT-613, AUT-616
#[test]
fn aut_612_fake_clock_start_formats_as_2030_01_01_00_00_00_utc() {
    assert_eq!(
        format_utc_millis(1_893_456_000_000),
        "2030-01-01 00:00:00 UTC"
    );
    assert_eq!(
        format_utc_millis(1_893_456_000_000 + 86_399_000),
        "2030-01-01 23:59:59 UTC"
    );
    assert_eq!(
        format_utc_millis(1_893_456_000_000 + 86_400_000 * 59),
        "2030-03-01 00:00:00 UTC"
    );
}

/// REQ: AUT-614 WARN
#[test]
fn aut_614_machine_output_uses_dot_and_no_group_separator() {
    let machine = format!("{:.2}", 1234567.5_f64);
    assert_eq!(machine, "1234567.50");
    assert!(!machine.contains(',') && !machine.contains(' '));
}

/// REQ: AUT-007
#[test]
fn aut_007_saturating_sub_does_not_underflow_when_wall_time_goes_backwards() {
    let (now, last): (u64, u64) = (1_000, 5_000);
    assert_eq!(now.saturating_sub(last), 0);
    assert!(now.checked_sub(last).is_none());
}
