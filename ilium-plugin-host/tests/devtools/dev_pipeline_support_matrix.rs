//! Support-matrix rules DEV-962, DEV-994 and DEV-999 (reference-abi-major-versions.md and
//! reference-toolchain-support-matrix.md). The matrix table is read from the pages; the release
//! checks that compare dates across the release record, the notes and `ilium version --json` are
//! not in the repository yet; the tests read those places directly and fail while one is missing.

use crate::common::{env, records_of, run, scratch};
use crate::dev_pipeline_manual::{manual_page, repository_file, tables_with_header};

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

/// Days since 1970-01-01 of a proleptic Gregorian date `YYYY-MM-DD`.
fn day_number(date: &str) -> i64 {
    let parts: Vec<i64> = date
        .split('-')
        .map(|part| part.parse().unwrap_or_else(|_| panic!("date {date} is not YYYY-MM-DD")))
        .collect();
    assert_eq!(parts.len(), 3, "date {date} is not YYYY-MM-DD");
    let (year, month, day) = (if parts[1] <= 2 { parts[0] - 1 } else { parts[0] }, parts[1], parts[2]);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    era * 146_097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year - 719_468
}

/// The rows of every real support matrix table of the two manual pages.
fn matrix_rows() -> Vec<Vec<String>> {
    let mut tables = real_support_matrix_tables(&manual_page("reference-abi-major-versions.md"));
    tables.extend(real_support_matrix_tables(&manual_page("reference-toolchain-support-matrix.md")));
    tables.into_iter().flatten().map(|row| row.into_iter().map(|cell| cell.replace('`', "")).collect()).collect()
}

/// DEV-965: the state at a day: `supported` up to the end date, `end-of-life` after it.
fn state_on(day: &str, end_date: &str) -> &'static str {
    if day_number(day) <= day_number(end_date) {
        "supported"
    } else {
        "end-of-life"
    }
}

/// DEV-999: the release check refuses a table in which a published end date moved earlier.
fn check_end_dates(published: &[Vec<String>], proposed: &[Vec<String>]) -> Result<(), String> {
    for old in published {
        let new = proposed
            .iter()
            .find(|row| row[0] == old[0])
            .ok_or_else(|| format!("release.row-removed: {}", old[0]))?;
        if day_number(&new[2]) < day_number(&old[2]) {
            return Err(format!("release.end-date-earlier: {} {} -> {}", old[0], old[2], new[2]));
        }
    }
    Ok(())
}

/// `YYYY-MM-DD` of the day `offset` days after `date`.
fn shifted_date(date: &str, offset: i64) -> String {
    let z = day_number(date) + offset + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn date_after(date: &str) -> String {
    shifted_date(date, 1)
}

fn date_before(date: &str) -> String {
    shifted_date(date, -1)
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
    let rows = matrix_rows();
    let row = rows.first().expect("the support matrix has a row");
    let after_end = date_after(&row[2]);
    assert_eq!(state_on(&after_end, &row[2]), "end-of-life");
    assert_eq!(state_on(&row[2], &row[2]), "supported");
    // After the end date the row is still there and installed plugins keep loading.
    assert!(matrix_rows().iter().any(|kept| kept[0] == row[0]), "the end-of-life row was removed");
    let root = scratch();
    let mut late_env = env(root.path());
    late_env.now = std::time::SystemTime::UNIX_EPOCH
        + std::time::Duration::from_secs(u64::try_from(day_number(&after_end) * 86_400).expect("date after 1970"));
    let listed = run(&late_env, &["list", "--json"]);
    assert_eq!(listed.exit_code, 0, "list after the end date: {:?}", listed.stdout_jsonl);
    assert!(records_of(&listed, "error").is_empty(), "installed plugins stopped loading: {:?}", listed.stdout_jsonl);
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
    let rows = matrix_rows();
    let row = rows.first().expect("the support matrix has a row");
    let minor = row[0].split('.').take(2).collect::<Vec<_>>().join(".");
    let record: serde_json::Value = serde_json::from_str(&repository_file(&format!("release/records/{minor}.json")))
        .expect("the release record is JSON");
    let notes = repository_file("CHANGELOG.md");
    let notes_dates = notes
        .lines()
        .find(|line| line.starts_with("## ") && line.contains(&row[0]))
        .unwrap_or_else(|| panic!("CHANGELOG.md has no release notes heading for {}", row[0]));
    let root = scratch();
    let version = run(&env(root.path()), &["version", "--json"]);
    let version_record = version.stdout_jsonl.first().cloned().unwrap_or_default();
    for (place, released, until) in [
        ("release record", record["released"].as_str().unwrap_or_default().to_owned(), record["security_fixes_until"].as_str().unwrap_or_default().to_owned()),
        ("version output", version_record["released"].as_str().unwrap_or_default().to_owned(), version_record["security_fixes_until"].as_str().unwrap_or_default().to_owned()),
    ] {
        assert_eq!(released, row[1], "release date in the {place}");
        assert_eq!(until, row[2], "end date in the {place}");
    }
    assert!(notes_dates.contains(&row[1]), "release date in the notes heading: {notes_dates}");
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
    let published = matrix_rows();
    assert!(!published.is_empty(), "the support matrix has no row to check");
    assert_eq!(check_end_dates(&published, &published), Ok(()), "an unchanged table passes");
    let mut earlier = published.clone();
    earlier[0][2] = date_before(&published[0][2]);
    let refused = check_end_dates(&published, &earlier).expect_err("an earlier end date must fail the release check");
    assert!(refused.contains("release.end-date-earlier"), "{refused}");
    let mut later = published.clone();
    later[0][2] = date_after(&published[0][2]);
    assert_eq!(check_end_dates(&published, &later), Ok(()), "a later end date passes");
    let mut removed = published.clone();
    removed.remove(0);
    assert!(check_end_dates(&published, &removed).is_err(), "removing a row fails the check");
}
