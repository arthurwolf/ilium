/// REQ: PDY-001
#[test]
fn pdy_001_grant_until_accepts_only_rfc3339_utc_and_refuses_past_or_far_future() {
    // Observable: Run `ilium plugin grant example.sync fs.read --until 2020-01-01T00:00:00Z`: the exit code is 2, the output contains `grant.expiry-invalid`, and the grant store is unchanged.
    panic!(
        "PDY-001 not implemented: `ilium plugin grant --until <timestamp>` MUST accept only RFC 3339 UTC timestamps and MUST refuse a time in the past or more than 365 days ahead with exit code 2 and the code `grant.expiry-invalid`."
    );
}

/// REQ: PDY-002
#[test]
fn pdy_002_host_compares_expiry_at_start_of_every_capability_call() {
    // Observable: Set an expiry 2 seconds ahead, stop the host timers in the harness, and make a call 3 seconds later: the call returns `not-permitted` with reason `revoked`.
    panic!(
        "PDY-002 not implemented: The host MUST compare the expiry against a monotonic-corrected wall clock at the start of every capability call, not only on a timer."
    );
}

/// REQ: PDY-003
#[test]
fn pdy_003_in_flight_call_returns_revoked_and_bumps_epoch_once() {
    // Observable: Start a long `fs.read` stream 1 second before expiry: the stream ends with `not-permitted` reason `revoked` and `ilium plugin grants example.sync --json` shows the epoch incremented by exactly 1.
    panic!(
        "PDY-003 not implemented: A call that started before the expiry and is still running when it passes MUST return `not-permitted` with reason `revoked`, and the grant epoch MUST bump once."
    );
}

/// REQ: PDY-004
#[test]
fn pdy_004_expired_grant_stays_expired_when_clock_moves_backwards() {
    // Observable: Let a grant expire, move the system clock back one day in the harness, and call the capability: the call returns `not-permitted` with reason `revoked`.
    panic!(
        "PDY-004 not implemented: If the system clock moves backwards, a grant that has already expired MUST stay expired."
    );
}

/// REQ: PDY-005
#[test]
fn pdy_005_expired_grant_shown_for_30_days_then_removed_and_regrant_reopens_review() {
    // Observable: After expiry, `ilium plugin grants example.sync --json` lists the grant with `state: "expired"`; after 31 days of simulated time the entry is absent, and `ilium plugin grant example.sync fs.read` opens the review dialog.
    panic!(
        "PDY-005 not implemented: An expired grant MUST be shown as `expired` in the Plugins screen and in `ilium plugin grants` for 30 days and then removed from the store; re-granting MUST require the review dialog again."
    );
}

/// REQ: PDY-006
#[test]
fn pdy_006_expiry_reactivates_plugin_with_reduced_grant_set() {
    // Observable: Let one of two grants expire: the plugin's activate export runs once more and its reported grant list contains only the remaining grant.
    panic!(
        "PDY-006 not implemented: On expiry the host MUST re-activate the plugin with the reduced grant set exactly as it does for a revoke."
    );
}
