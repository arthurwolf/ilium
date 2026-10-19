//! Twins of last_prompt_banner.rs: wrapping and middle truncation of the last prompt banner.
//! Request/reply shape: see API_GAPS_ports.md.
use crate::common::{ask, Place};
use serde_json::Value;

fn check(place: Place, op: &str, request: &str, expected: &str) {
    let request: Value = serde_json::from_str(request).expect("request literal parses");
    let expected: Value = serde_json::from_str(expected).expect("expected literal parses");
    let reply = ask(place, op, request);
    let fields = expected.as_object().expect("expected is an object");
    for (name, value) in fields {
        assert_eq!(
            &reply[name.as_str()],
            value,
            "field {name} of reply {reply}"
        );
    }
}

/// REQ: PORT:ilium-client/src/last_prompt_banner.rs::a_budget_of_one_keeps_only_the_first_line_marked_as_the_seam
#[test]
fn port_last_prompt_banner_a_budget_of_one_keeps_only_the_first_line_marked_as_the_seam() {
    check(
        Place::Client,
        "banner.layout",
        r####"{"text": "one\ntwo\nthree", "width": 80, "budget": 1}"####,
        r####"{"lines": [["one", true]]}"####,
    );
}

/// REQ: PORT:ilium-client/src/last_prompt_banner.rs::a_long_single_line_word_wraps_instead_of_being_clipped
#[test]
fn port_last_prompt_banner_a_long_single_line_word_wraps_instead_of_being_clipped() {
    check(
        Place::Client,
        "banner.layout",
        r####"{"text": "the quick brown fox jumps over the lazy dog", "width": 10, "wrap_only": true}"####,
        r####"{"lines": ["the quick", "brown fox", "jumps over", "the lazy", "dog"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/last_prompt_banner.rs::a_narrow_width_of_zero_does_not_panic
#[test]
fn port_last_prompt_banner_a_narrow_width_of_zero_does_not_panic() {
    check(
        Place::Client,
        "banner.layout",
        r####"{"text": "abc", "width": 0, "wrap_only": true}"####,
        r####"{"no_panic": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/last_prompt_banner.rs::a_prompt_exactly_at_the_wrap_boundary_stays_on_one_row
#[test]
fn port_last_prompt_banner_a_prompt_exactly_at_the_wrap_boundary_stays_on_one_row() {
    check(
        Place::Client,
        "banner.layout",
        r####"{"text": "12345 12345", "width": 11, "wrap_only": true}"####,
        r####"{"lines": ["12345 12345"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/last_prompt_banner.rs::a_word_wider_than_the_column_hard_breaks_at_grapheme_boundaries
#[test]
fn port_last_prompt_banner_a_word_wider_than_the_column_hard_breaks_at_grapheme_boundaries() {
    check(
        Place::Client,
        "banner.layout",
        r####"{"text": "abcdefghijklmnopqrstuvwxyz", "width": 10, "wrap_only": true}"####,
        r####"{"every_row_width_at_most": 10, "rows_joined_equal_text": true}"####,
    );
}

/// REQ: PORT:ilium-client/src/last_prompt_banner.rs::a_zero_budget_is_treated_as_one
#[test]
fn port_last_prompt_banner_a_zero_budget_is_treated_as_one() {
    check(
        Place::Client,
        "banner.layout",
        r####"{"text": "one\ntwo\nthree", "width": 80, "budget": 0}"####,
        r####"{"lines": [["one", true]]}"####,
    );
}

/// REQ: PORT:ilium-client/src/last_prompt_banner.rs::a_zero_prompt_budget_still_reserves_one_row
#[test]
fn port_last_prompt_banner_a_zero_prompt_budget_still_reserves_one_row() {
    check(
        Place::Client,
        "banner.layout",
        r####"{"reserved_height_of": [0, 4, 20]}"####,
        r####"{"reserved": [1, 4, 20]}"####,
    );
}

/// REQ: PORT:ilium-client/src/last_prompt_banner.rs::blank_lines_inside_a_multiline_prompt_survive_wrapping_as_empty_rows
#[test]
fn port_last_prompt_banner_blank_lines_inside_a_multiline_prompt_survive_wrapping_as_empty_rows() {
    check(
        Place::Client,
        "banner.layout",
        r####"{"text": "first\n\nthird", "width": 80, "wrap_only": true}"####,
        r####"{"lines": ["first", "", "third"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/last_prompt_banner.rs::lone_carriage_returns_split_lines_same_as_line_feeds
#[test]
fn port_last_prompt_banner_lone_carriage_returns_split_lines_same_as_line_feeds() {
    check(
        Place::Client,
        "banner.layout",
        r####"{"text": "first line of a longer prompt\rsecond line\rthird line\rfourth line\rfifth line\rsixth line", "width": 80, "budget": 4}"####,
        r####"{"lines": [["first line of a longer prompt", false], ["second line", true], ["fifth line", true], ["sixth line", false]]}"####,
    );
}

/// REQ: PORT:ilium-client/src/last_prompt_banner.rs::odd_budgets_give_the_extra_kept_line_to_the_head
#[test]
fn port_last_prompt_banner_odd_budgets_give_the_extra_kept_line_to_the_head() {
    check(
        Place::Client,
        "banner.layout",
        r####"{"text": "one\ntwo\nthree\nfour\nfive\nsix\nseven", "width": 80, "budget": 5}"####,
        r####"{"lines": [["one", false], ["two", false], ["three", true], ["six", true], ["seven", false]]}"####,
    );
}

/// REQ: PORT:ilium-client/src/last_prompt_banner.rs::over_budget_keeps_first_two_and_last_two_at_the_default_of_four
#[test]
fn port_last_prompt_banner_over_budget_keeps_first_two_and_last_two_at_the_default_of_four() {
    check(
        Place::Client,
        "banner.layout",
        r####"{"text": "one\ntwo\nthree\nfour\nfive\nsix\nseven", "width": 80, "budget": 4}"####,
        r####"{"lines": [["one", false], ["two", true], ["six", true], ["seven", false]]}"####,
    );
}

/// REQ: PORT:ilium-client/src/last_prompt_banner.rs::small_prompt_slot_retains_the_end_of_the_prompt
#[test]
fn port_last_prompt_banner_small_prompt_slot_retains_the_end_of_the_prompt() {
    check(
        Place::Client,
        "banner.layout",
        r####"{"text": "FIRST\nSECOND\nTHIRD\nLAST", "slot": [40, 2], "budget": 4}"####,
        r####"{"contains": ["FIRST", "LAST"], "excludes": ["SECOND"]}"####,
    );
}

/// REQ: PORT:ilium-client/src/last_prompt_banner.rs::text_at_or_under_budget_keeps_every_line_untouched
#[test]
fn port_last_prompt_banner_text_at_or_under_budget_keeps_every_line_untouched() {
    check(
        Place::Client,
        "banner.layout",
        r####"{"text": "one\ntwo", "width": 80, "budget": 4, "also_empty": ""}"####,
        r####"{"lines": [["one", false], ["two", false]], "empty": []}"####,
    );
}

/// REQ: PORT:ilium-client/src/last_prompt_banner.rs::the_prompt_slot_reserves_the_full_configured_budget
#[test]
fn port_last_prompt_banner_the_prompt_slot_reserves_the_full_configured_budget() {
    check(
        Place::Client,
        "banner.layout",
        r####"{"reserved_height_of": [4]}"####,
        r####"{"reserved": [4]}"####,
    );
}

/// REQ: PORT:ilium-client/src/last_prompt_banner.rs::wrapping_that_still_exceeds_max_lines_is_middle_truncated_over_wrapped_rows
#[test]
fn port_last_prompt_banner_wrapping_that_still_exceeds_max_lines_is_middle_truncated_over_wrapped_rows(
) {
    check(
        Place::Client,
        "banner.layout",
        r####"{"text": "aa bb cc dd ee ff gg hh ii jj", "width": 4, "budget": 4}"####,
        r####"{"wrapped_rows": 10, "lines": [["aa", false], ["bb", true], ["ii", true], ["jj", false]]}"####,
    );
}
