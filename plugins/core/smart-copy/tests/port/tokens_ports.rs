//! Ports of smart_copy_tokens.rs: which tokens the server offers for a screen row.

use crate::common::{offers, scan};

fn assert_offers(row: &str, tokens: &[&str]) {
    for token in tokens {
        assert!(
            offers(row, token),
            "{token:?} missing from {:?}",
            scan(&[row])
        );
    }
}

/// REQ: PORT:ilium-client/src/smart_copy_tokens.rs::urls_nest_inside_lines_and_drop_trailing_punctuation
#[test]
fn port_tokens_urls_drop_trailing_punctuation() {
    assert_offers(
        "see https://example.com/a?b=1, then (https://x.org/y).",
        &["https://example.com/a?b=1", "https://x.org/y"],
    );
}

/// REQ: PORT:ilium-client/src/smart_copy_tokens.rs::urls_do_not_produce_path_tokens
#[test]
fn port_tokens_urls_do_not_produce_path_tokens() {
    assert!(!offers("https://example.com/a/b.html", "/a/b.html"));
}

/// REQ: PORT:ilium-client/src/smart_copy_tokens.rs::paths_absolute_relative_home_and_bare
#[test]
fn port_tokens_paths_absolute_relative_home_and_bare() {
    assert_offers(
        "/etc/hosts ./src/lib.rs ~/notes/a.txt Makefile",
        &["/etc/hosts", "./src/lib.rs", "~/notes/a.txt", "Makefile"],
    );
    assert!(!offers("and/or", "and/or"));
}

/// REQ: PORT:ilium-client/src/smart_copy_tokens.rs::path_context_resolves_across_ancestors_and_source_directories
#[test]
fn port_tokens_paths_are_offered_as_shown_without_a_filesystem() {
    assert_offers(
        "error in src/lib.rs and ../core/mod.rs",
        &["src/lib.rs", "../core/mod.rs"],
    );
}

/// REQ: PORT:ilium-client/src/smart_copy_tokens.rs::file_locations_nest_path_and_file_name
#[test]
fn port_tokens_file_locations_nest_path_and_file_name() {
    assert_offers(
        "app.rs:6004 src/app.rs:12:3",
        &["app.rs:6004", "src/app.rs:12:3", "src/app.rs"],
    );
}

/// REQ: PORT:ilium-client/src/smart_copy_tokens.rs::hashes_uuids_colors_and_assignments
#[test]
fn port_tokens_hashes_uuids_colors_and_assignments() {
    assert_offers(
        "abc1234def 123e4567-e89b-12d3-a456-426614174000 #ff8800 RUST_LOG=debug",
        &[
            "abc1234def",
            "123e4567-e89b-12d3-a456-426614174000",
            "#ff8800",
            "RUST_LOG=debug",
        ],
    );
    assert!(!offers("1234567", "1234567"));
}

/// REQ: PORT:ilium-client/src/smart_copy_tokens.rs::endpoints_addresses_and_emails
#[test]
fn port_tokens_endpoints_addresses_and_emails() {
    assert_offers(
        "localhost:8080 10.0.0.1 a@b.example",
        &["localhost:8080", "10.0.0.1", "a@b.example"],
    );
}

/// REQ: PORT:ilium-client/src/smart_copy_tokens.rs::version_is_not_found_inside_an_ip_address
#[test]
fn port_tokens_version_is_not_found_inside_an_ip_address() {
    assert_offers("tokio@1.40.2", &["tokio@1.40.2"]);
    assert!(!offers("host 192.168.1.10", "1.10"));
}

/// REQ: PORT:ilium-client/src/smart_copy_tokens.rs::dates_times_measurements_amounts_and_phones
#[test]
fn port_tokens_dates_times_measurements_amounts_and_phones() {
    assert_offers(
        "2026-10-16 12:30:45 5.5 km $12.50 +1 555 123 4567",
        &["2026-10-16", "12:30:45", "5.5 km", "$12.50"],
    );
}

/// REQ: PORT:ilium-client/src/smart_copy_tokens.rs::postal_addresses_single_multi_line_and_foreign
#[test]
fn port_tokens_postal_addresses_single_line() {
    assert_offers(
        "221B Baker Street, London NW1 6XE",
        &["221B Baker Street, London NW1 6XE"],
    );
}

/// REQ: PORT:ilium-client/src/smart_copy_tokens.rs::qualified_names_split_into_nested_segments
#[test]
fn port_tokens_qualified_names_split_into_nested_segments() {
    assert_offers(
        "std::collections::HashMap",
        &["std::collections::HashMap", "std::collections", "HashMap"],
    );
}

/// REQ: PORT:ilium-client/src/smart_copy_tokens.rs::quotes_key_values_and_sentences_nest
#[test]
fn port_tokens_quotes_key_values_and_sentences_nest() {
    assert_offers(
        "name=\"Ada Lovelace\" says hi.",
        &["Ada Lovelace", "name=\"Ada Lovelace\""],
    );
}

/// REQ: PORT:ilium-client/src/smart_copy_tokens.rs::code_identifiers_and_functions
#[test]
fn port_tokens_code_identifiers_and_functions() {
    assert_offers(
        "call parse_input() with MAX_SIZE and camelCase",
        &["parse_input()", "MAX_SIZE", "camelCase"],
    );
}

/// REQ: PORT:ilium-client/src/smart_copy_tokens.rs::styled_runs_group_colour_bold_and_background
#[test]
fn port_tokens_styled_runs_group_colour_bold_and_background() {
    assert_offers(
        "\u{1b}[34mblue run\u{1b}[0m \u{1b}[1mbold run\u{1b}[0m \u{1b}[7;42mback run\u{1b}[0m",
        &["blue run", "bold run", "back run"],
    );
}

/// REQ: PORT:ilium-client/src/smart_copy_tokens.rs::grey_and_default_colours_are_not_styled_runs
#[test]
fn port_tokens_grey_and_default_colours_are_not_styled_runs() {
    assert!(!offers("\u{1b}[90mgrey words\u{1b}[0m plain", "grey words"));
}
