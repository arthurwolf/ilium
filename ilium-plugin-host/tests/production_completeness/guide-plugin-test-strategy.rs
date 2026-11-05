/// REQ: PDT-045
#[test]
fn pdt_045_release_requires_fixture_per_contribution() {
    // Observable: Declare two commands and write a fixture for one: `ilium plugin check --release` prints one `lint.untested-contribution` warning naming the other command.
    panic!(
        "PDT-045 not implemented: A release MUST have at least one fixture per declared command, view and hook, otherwise `ilium plugin check --release` warns with `lint.untested-contribution` naming each uncovered contribution."
    );
}

/// REQ: PDT-046
#[test]
fn pdt_046_every_grant_needs_granted_and_denied_fixtures() {
    // Observable: Request `fs.read` with only a granted fixture: `ilium plugin check --release` prints `lint.grant-denial-untested` naming `fs.read`.
    panic!(
        "PDT-046 not implemented: Every requested grant MUST have one fixture that runs with the grant held and one that expects `capability.denied` with the grant absent."
    );
}

/// REQ: PDT-047
#[test]
fn pdt_047_quarantine_expiry_limited_to_thirty_days() {
    // Observable: Quarantine a case with `expires` 31 days ahead: `ilium plugin test` exits 2 with `test.quarantine-too-long`; run with a quarantine whose date has passed: the run exits non-zero with `test.quarantine-expired`.
    panic!(
        "PDT-047 not implemented: A quarantined case ([DEV-960]) MUST carry an `expires` date at most 30 days ahead, and a run after that date MUST fail with `test.quarantine-expired`."
    );
}

/// REQ: PDT-048
#[test]
fn pdt_048_release_gate_runs_conformance_on_packed_component() {
    // Observable: Run `ilium plugin check --release` after building only a debug component: it fails with `release.conformance-not-packed`.
    panic!(
        "PDT-048 not implemented: The release gate MUST run `ilium plugin test --conformance` against the packed component in dist/, not a debug build."
    );
}

/// REQ: PDT-049
#[test]
fn pdt_049_persistent_flaky_case_fails_release_gate() {
    // Observable: Record three consecutive reports in which case `sync-retry` is `flaky: true` and run `ilium plugin check --release`: it exits non-zero with `test.flaky-persistent` naming `sync-retry`.
    panic!(
        "PDT-049 not implemented: A case flagged flaky ([DEV-963]) in three consecutive runs MUST fail the release gate with `test.flaky-persistent`."
    );
}
