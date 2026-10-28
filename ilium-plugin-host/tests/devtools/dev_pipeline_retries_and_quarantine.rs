//! Retry, quarantine and flaky-test rules DEV-956, DEV-960, DEV-961 and DEV-963
//! (reference-test-harness.md, "Flaky tests and retries"). The documentation tests read the page;
//! the pipeline and runner behaviour is not in the repository yet, so the tests run the small
//! models of this file.

use crate::dev_pipeline_manual::{manual_page, repository_file};

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

/// The pipeline kind of DEV-956.
#[derive(Clone, Copy, PartialEq)]
enum Pipeline {
    Release,
    PullRequest,
}

/// One attempt of a test: its number, the seed it starts with and the state its instance starts with.
struct Attempt {
    number: u32,
    seed: u64,
    instance_state_at_start: u32,
}

/// The result of running a test under a retry policy.
struct RetriedRun {
    attempts: Vec<(u32, bool)>,
    seeds: Vec<u64>,
    states_at_start: Vec<u32>,
}

/// Runs `test` the way DEV-956 states: no retry in a release pipeline, at most one in a pull
/// request, and each attempt on a fresh instance (state 0) with the seed of the first attempt.
fn run_with_retries(pipeline: Pipeline, first_seed: u64, mut test: impl FnMut(&Attempt, &mut u32) -> bool) -> RetriedRun {
    let allowed_attempts = if pipeline == Pipeline::Release { 1 } else { 2 };
    let mut run = RetriedRun { attempts: Vec::new(), seeds: Vec::new(), states_at_start: Vec::new() };
    for number in 1..=allowed_attempts {
        let mut instance_state = 0;
        let attempt = Attempt { number, seed: first_seed, instance_state_at_start: instance_state };
        run.seeds.push(attempt.seed);
        run.states_at_start.push(attempt.instance_state_at_start);
        let passed = test(&attempt, &mut instance_state);
        run.attempts.push((number, passed));
        if passed {
            break;
        }
    }
    run
}

/// A quarantine entry of DEV-960 and DEV-961.
struct Quarantine<'a> {
    test: &'a str,
    owner: &'a str,
    added: &'a str,
    expires: &'a str,
}

/// Validates an entry: it names an owner, expires within 14 days of being added, and is not for a
/// conformance case.
fn validate_quarantine(entry: &Quarantine) -> Result<(), String> {
    if entry.owner.trim().is_empty() {
        return Err("quarantine.owner-missing".to_owned());
    }
    if entry.test.starts_with("conformance.") {
        return Err("quarantine.conformance-case".to_owned());
    }
    let window = day_number(entry.expires) - day_number(entry.added);
    if !(0..=14).contains(&window) {
        return Err(format!("quarantine.expiry-window: {window} days"));
    }
    Ok(())
}

/// The summary of a run: tests that failed and then passed are flaky, not plain passes.
struct RunSummary {
    plain_passes: Vec<String>,
    flaky: Vec<(String, u32)>,
}

fn summarize(results: &[(&str, Vec<bool>)]) -> RunSummary {
    let mut summary = RunSummary { plain_passes: Vec::new(), flaky: Vec::new() };
    for (name, attempts) in results {
        match attempts.as_slice() {
            [true] => summary.plain_passes.push((*name).to_owned()),
            [.., true] => summary.flaky.push(((*name).to_owned(), u32::try_from(attempts.len()).unwrap_or(u32::MAX))),
            _ => {}
        }
    }
    summary
}

/// REQ: DEV-956
#[test]
fn dev_956_the_page_forbids_release_retries_and_allows_one_pull_request_retry() {
    let page = manual_page("reference-test-harness.md");
    assert!(
        page.contains("A release pipeline MUST NOT retry a failing plugin test."),
        "a release pipeline never retries a failing plugin test"
    );
    assert!(
        page.contains("A pull-request pipeline MAY retry it at most 1 time"),
        "a pull-request pipeline may retry at most one time"
    );
}

/// REQ: DEV-956
#[test]
fn dev_956_every_retry_attempt_starts_fresh_with_the_first_attempts_seed() {
    // A test that leaves state behind when it fails: the retry must not see it.
    let pull_request = run_with_retries(Pipeline::PullRequest, 4242, |attempt, state| {
        let saw_leftover = attempt.instance_state_at_start != 0;
        *state += 1;
        attempt.number > 1 && !saw_leftover
    });
    assert_eq!(pull_request.attempts, vec![(1, false), (2, true)], "one retry is allowed in a pull request");
    assert!(pull_request.seeds.iter().all(|seed| *seed == 4242), "every attempt uses the seed of the first: {:?}", pull_request.seeds);
    assert!(pull_request.states_at_start.iter().all(|state| *state == 0), "every attempt starts a fresh instance");
    let always_failing = run_with_retries(Pipeline::PullRequest, 1, |_, _| false);
    assert_eq!(always_failing.attempts.len(), 2, "at most one retry");
    let release = run_with_retries(Pipeline::Release, 1, |_, _| false);
    assert_eq!(release.attempts.len(), 1, "a release pipeline never retries");
}

/// REQ: DEV-960
#[test]
fn dev_960_the_page_limits_a_quarantine_expiry_to_14_days_after_it_is_added() {
    let page = manual_page("reference-test-harness.md");
    assert!(
        page.contains("A test MAY be quarantined only by a quarantine entry that names an owner and an expiry date"),
        "quarantine needs an owner and an expiry date"
    );
    assert!(
        page.contains("no more than 14 days after the day the entry is added"),
        "the expiry date is at most 14 days after the entry is added"
    );
}

/// REQ: DEV-960
#[test]
fn dev_960_a_quarantine_entry_expiring_after_14_days_is_refused() {
    let entry = |expires| Quarantine { test: "tests.sync_flaky", owner: "team-plugins", added: "2030-03-01", expires };
    assert_eq!(validate_quarantine(&entry("2030-03-15")), Ok(()), "14 days is the longest allowed window");
    let refused = validate_quarantine(&entry("2030-03-16")).expect_err("15 days must be refused");
    assert!(refused.contains("quarantine.expiry-window"), "{refused}");
    assert!(validate_quarantine(&entry("2030-02-20")).is_err(), "an expiry before the day of adding is refused");
    let ownerless = Quarantine { test: "tests.x", owner: "", added: "2030-03-01", expires: "2030-03-05" };
    assert!(validate_quarantine(&ownerless).is_err(), "an entry without an owner is refused");
}

/// REQ: DEV-961
#[test]
fn dev_961_conformance_cases_are_not_quarantinable_and_a_fail_verdict_exits_3() {
    let harness = manual_page("reference-test-harness.md");
    assert!(
        harness.contains("MUST NOT be quarantined"),
        "a conformance case must not be quarantined"
    );
    let conformance = repository_file("docs/plugins-manual/pages/reference-conformance-suite.md");
    assert!(
        conformance.contains("A `fail` verdict makes the exit code 3."),
        "a fail verdict of the conformance suite exits with code 3"
    );
}

/// REQ: DEV-961
#[test]
fn dev_961_a_quarantine_entry_for_a_conformance_case_is_refused() {
    let conformance = Quarantine { test: "conformance.lifecycle.activate", owner: "team", added: "2030-03-01", expires: "2030-03-05" };
    assert_eq!(validate_quarantine(&conformance), Err("quarantine.conformance-case".to_owned()));
    let ordinary = Quarantine { test: "tests.activate", ..conformance };
    assert_eq!(validate_quarantine(&ordinary), Ok(()), "an ordinary test can be quarantined");
}

/// REQ: DEV-963
#[test]
fn dev_963_a_test_that_passes_on_a_retry_is_listed_as_flaky_with_its_attempt_count() {
    let summary = summarize(&[("tests.stable", vec![true]), ("tests.sync", vec![false, true]), ("tests.broken", vec![false, false])]);
    assert_eq!(summary.flaky, vec![("tests.sync".to_owned(), 2)], "flaky entries carry the attempt count");
}

/// REQ: DEV-963
#[test]
fn dev_963_a_retried_test_that_recovers_is_never_reported_as_a_plain_pass() {
    let summary = summarize(&[("tests.stable", vec![true]), ("tests.sync", vec![false, true])]);
    assert_eq!(summary.plain_passes, vec!["tests.stable".to_owned()]);
    assert!(!summary.plain_passes.iter().any(|name| name == "tests.sync"), "a recovered test is not a plain pass");
}
