//! Case isolation and ordering of `ilium plugin test` (DEV-919). Page: reference-test-harness.

use std::collections::BTreeMap;

use crate::common::*;
use crate::{project_with_fixtures, rows_with};

/// The status of every fixture named in a run, keyed by fixture name.
fn statuses(output: &ilium_plugin_host::cli::CliOutput) -> BTreeMap<String, String> {
    rows_with(output, "fixture")
        .into_iter()
        .map(|row| {
            (
                row["fixture"].as_str().expect("fixture name").to_owned(),
                row["status"].as_str().expect("fixture status").to_owned(),
            )
        })
        .collect()
}

/// REQ: DEV-919
#[test]
fn cases_run_in_sorted_order_and_a_seeded_shuffle_gives_identical_statuses() {
    let (_root, env, _dir) = project_with_fixtures(&["c-case", "a-case", "b-case"]);
    let sorted = run_in_project(&env, "test", &[]);
    assert_eq!(sorted.exit_code, 0, "{:?}", sorted.stdout_jsonl);
    let order: Vec<String> = rows_with(&sorted, "fixture")
        .iter()
        .map(|row| row["fixture"].as_str().expect("fixture name").to_owned())
        .collect();
    assert_eq!(
        order,
        vec!["a-case", "b-case", "c-case"],
        "sorted fixture name order by default"
    );

    let first_shuffle = run_in_project(&env, "test", &["--shuffle", "7"]);
    let second_shuffle = run_in_project(&env, "test", &["--shuffle", "7"]);
    let seed_rows = rows_with(&first_shuffle, "seed");
    assert_eq!(
        seed_rows.first().and_then(|row| row["seed"].as_u64()),
        Some(7),
        "the seed is printed"
    );
    assert_eq!(
        statuses(&first_shuffle),
        statuses(&second_shuffle),
        "a case does not observe another case's state"
    );
    assert_eq!(
        statuses(&first_shuffle),
        statuses(&sorted),
        "the permutation does not change any status"
    );
}
