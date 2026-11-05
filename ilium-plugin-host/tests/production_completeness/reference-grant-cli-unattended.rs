/// REQ: PDY-021
#[test]
fn pdy_021_high_grant_headless_fails_needs_interactive() {
    // Observable: With no client attached and no allow-list, run `ilium plugin grant example.sync shell.exec`: the exit code is 3, the output contains `grant.needs-interactive`, and `ilium plugin grants example.sync --json` does not list `shell.exec`.
    panic!(
        "PDY-021 not implemented: `ilium plugin grant <id> <grant>` for a HIGH grant without an attached client MUST fail with exit code 3 and the code `grant.needs-interactive`, unless a policy allow-list names that plugin and grant."
    );
}

/// REQ: PDY-022
#[test]
fn pdy_022_revoke_headless_writes_epoch_before_exit() {
    // Observable: With no client attached, run the revoke and immediately read the grant store file: the epoch is incremented and the command exited 0.
    panic!(
        "PDY-022 not implemented: `ilium plugin grants <id> --revoke <grant>` MUST work without an attached client and MUST exit 0 only after the epoch bump has been written to the grant store."
    );
}

/// REQ: PDY-023
#[test]
fn pdy_023_grant_commands_print_one_jsonl_result() {
    // Observable: Run `ilium plugin grants example.sync --revoke fs.read --json`: stdout is one JSON line with keys `type`, `plugin_id`, `grant` and `epoch` and nothing else.
    panic!(
        "PDY-023 not implemented: Every grant command MUST print exactly one JSONL result line with the fields `type`, `plugin_id`, `grant` and `epoch`."
    );
}

/// REQ: PDY-024
#[test]
fn pdy_024_headless_first_use_consent_declined() {
    // Observable: Trigger a first-use consent capability in a server with no client attached: the call returns `not-permitted` reason `declined` and the audit log gains one `grant-declined` line with `actor.kind: "policy"`.
    panic!(
        "PDY-024 not implemented: A first-use consent that would prompt in a headless session MUST resolve as `declined`, return `not-permitted` with reason `declined`, and write a `grant-declined` audit record with `actor.kind: 'policy'`."
    );
}

/// REQ: PDY-025
#[test]
fn pdy_025_revoke_unheld_grant_exits_4_no_change() {
    // Observable: Run `ilium plugin grants example.sync --revoke net.fetch` when `net.fetch` is not held: the exit code is 4, the output contains `grant.not-held`, and the epoch in the grant store is unchanged.
    panic!(
        "PDY-025 not implemented: Revoking a grant the plugin does not hold MUST exit 4 with the code `grant.not-held` and MUST change nothing, including the epoch."
    );
}
