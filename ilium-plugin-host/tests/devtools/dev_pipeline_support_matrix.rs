//! Support-matrix rules DEV-962, DEV-994 and DEV-999 (reference-abi-major-versions.md and
//! reference-toolchain-support-matrix.md). The matrix table is read from the pages; the release
//! checks that compare dates across the release record, the notes and `ilium version --json` are
//! not in the repository yet and stay unimplemented.

use crate::dev_pipeline_manual::{manual_page, tables_with_header};

const SUPPORT_STATES: [&str; 2] = ["supported", "end-of-life"];

/// The host-minor support matrix table of a page, skipping the illustrative example table.
fn real_support_matrix_tables(page_text: &str) -> Vec<Vec<Vec<String>>> {
    tables_with_header(page_text, "Host minor")
        .into_iter()
        .filter(|table| !table.preceding_line.contains("illustrative only"))
        .filter(|table| table.header == ["Host minor", "Released", "Security fixes until", "Support"])
        .map(|table| table.rows)
        .collect()
}

/// REQ: DEV-962
#[test]
fn dev_962_the_matrix_lists_each_minor_with_release_date_end_date_and_state() {
    let mut matrices = real_support_matrix_tables(&manual_page("reference-abi-major-versions.md"));
    matrices.extend(real_support_matrix_tables(&manual_page(
        "reference-toolchain-support-matrix.md",
    )));
    assert!(
        !matrices.is_empty(),
        "a support matrix table with a `Host minor` header must exist; the only table in the ABI page is the illustrative example"
    );
    for rows in &matrices {
        for row in rows {
            assert_eq!(row.len(), 4, "a matrix row has minor, release date, end date and state: {row:?}");
            assert!(
                SUPPORT_STATES.contains(&row[3].trim_matches('`')),
                "the state of a row is supported or end-of-life: {row:?}"
            );
        }
    }
}

/// REQ: DEV-962
#[test]
fn dev_962_an_end_of_life_minor_keeps_its_row_and_its_installed_plugins_keep_loading() {
    unimplemented!("ilium-plugin-host::support::SupportMatrix::end_of_life_row_and_plugin_load");
}

/// REQ: DEV-994
#[test]
fn dev_994_the_toolchain_page_holds_the_one_row_per_minor_support_matrix() {
    let page = manual_page("reference-toolchain-support-matrix.md");
    assert!(
        page.contains("is the table in this page, one row per host minor release"),
        "the page states that its own table is the support matrix"
    );
    assert!(
        !real_support_matrix_tables(&page).is_empty(),
        "the toolchain support matrix page has no host-minor table with a `Host minor` header"
    );
}

/// REQ: DEV-994
#[test]
fn dev_994_release_date_and_end_date_match_in_record_notes_row_and_version_output() {
    unimplemented!("ilium-plugin-host::release::record::minor_dates_match_four_places");
}

/// REQ: DEV-999
#[test]
fn dev_999_the_page_forbids_moving_a_published_end_date_earlier_and_names_later_dates() {
    let page = manual_page("reference-toolchain-support-matrix.md");
    assert!(
        page.contains("A security-fix end date that is already published in the table MUST NOT be moved earlier."),
        "a published end date is never moved earlier"
    );
    assert!(
        page.contains("A later end date MUST be recorded in the same table row and named in the release notes of the next patch release of that minor"),
        "a later end date is recorded in the row and named in the next patch notes"
    );
}

/// REQ: DEV-999
#[test]
fn dev_999_the_release_check_fails_when_a_published_end_date_is_moved_earlier() {
    unimplemented!("ilium-plugin-host::release::check::published_end_date_not_earlier");
}
