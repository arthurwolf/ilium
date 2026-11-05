/// REQ: PDY-013
#[test]
fn pdy_013_grant_records_carry_actor_kind_grant_previous_state_epoch() {
    // Observable: Revoke `fs.read` with `ilium plugin grants example.sync --revoke fs.read` and read the last audit line: event is `grant-revoked`, `actor.kind` is `cli`, `grant` is `fs.read`, `previous_state` is `held` and `epoch` equals the new epoch.
    panic!(
        "PDY-013 not implemented: `grant-given` and `grant-revoked` records MUST carry `actor.kind` (one of `user-tui`, `cli`, `policy`, `expiry`), `grant`, `previous_state` and `epoch`."
    );
}

/// REQ: PDY-014
#[test]
fn pdy_014_revoke_all_writes_per_grant_records_and_summary_with_count() {
    // Observable: Run `ilium plugin grants example.sync --revoke-all` on a plugin holding three grants: the audit log gains three `grant-revoked` lines and one `revoke-all` line with `count: 3`.
    panic!(
        "PDY-014 not implemented: Revoke all MUST write one `grant-revoked` record per grant plus exactly one `revoke-all` summary record carrying the count."
    );
}

/// REQ: PDY-015
#[test]
fn pdy_015_grant_expired_record_written_once_per_expiry() {
    // Observable: Let a grant expire and make five calls afterwards: exactly one `grant-expired` line exists for it, with `expired_at` equal to the stored expiry and `detected_at` at or after it.
    panic!(
        "PDY-015 not implemented: The audit catalogue MUST include `grant-expired` with `plugin_id`, `grant`, `expired_at` and `detected_at`, written once per expiry."
    );
}

/// REQ: PDY-016
#[test]
fn pdy_016_policy_denied_install_writes_record_per_attempt() {
    // Observable: Install a plugin requesting a policy-denied grant twice: the audit log contains exactly two `grant-policy-denied` lines, each with the grant name.
    panic!(
        "PDY-016 not implemented: A grant refused by policy at install time MUST write one `grant-policy-denied` record per install attempt, naming the grant."
    );
}

/// REQ: PDY-017
#[test]
fn pdy_017_declined_row_writes_grant_declined_record() {
    // Observable: Decline the `net.fetch` row during install: the audit log contains one `grant-declined` line with `grant: "net.fetch"`.
    panic!(
        "PDY-017 not implemented: Declining a row in the review dialog MUST write one `grant-declined` record with the grant name."
    );
}

/// REQ: PDY-018
#[test]
fn pdy_018_refused_calls_summarised_per_plugin_grant_per_minute() {
    // Observable: Make 500 refused `clipboard.read` calls within 10 seconds: the audit log contains exactly one `grant-refused` line for that plugin and grant with `count: 500`.
    panic!(
        "PDY-018 not implemented: Refused runtime calls for grants that are not HIGH MUST be summarised into at most one `grant-refused` record per plugin and grant per minute, carrying `count`; HIGH grants keep their per-call records."
    );
}

/// REQ: PDY-019
#[test]
fn pdy_019_cli_grant_change_records_invoking_uid_in_actor_uid() {
    // Observable: Run `ilium plugin grants example.sync --revoke fs.read` from a shell without a controlling terminal: the `grant-revoked` line has `actor.kind: "cli"` and `actor.uid` equal to the shell's uid.
    panic!(
        "PDY-019 not implemented: A grant change made by the CLI in a non-interactive session MUST record the invoking uid in `actor.uid`."
    );
}

/// REQ: PDY-020
#[test]
fn pdy_020_grant_audit_records_reachable_via_plugin_audit_json() {
    // Observable: Run `ilium plugin audit --event grant-revoked --plugin example.sync --json` after one revoke: the output is exactly one JSON line whose `event` is `grant-revoked`.
    panic!(
        "PDY-020 not implemented: Every grant audit record MUST be reachable with `ilium plugin audit --event <name> --plugin <id> --json`, printing one JSONL line per record."
    );
}
