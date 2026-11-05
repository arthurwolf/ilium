/// REQ: PDW-008
#[test]
fn pdw_008_submit_prints_payload_and_confirms_before_sending() {
    // Observable: Run `ilium plugin report example.sync` with network capture on: no request is made; run it with `--submit` and answer `n`: the payload was printed and still no request is made.
    panic!(
        "PDW-008 not implemented: `ilium plugin report <id> --submit` MUST print the exact JSON payload and require explicit confirmation before any request is sent; without `--submit` the command MUST print the contacts and send nothing."
    );
}

/// REQ: PDW-009
#[test]
fn pdw_009_payload_contains_only_allowed_fields() {
    // Observable: Submit a report and inspect the captured request body: its keys are exactly `plugin_id`, `version`, `sha256`, `rule_id`, `category` and `text`, and `text` longer than 4000 characters is refused before sending with `report.text-too-long`.
    panic!(
        "PDW-009 not implemented: The payload MUST contain only the plugin id, version, package `sha256`, one `policy.json` rule id, a category and a free text of at most 4000 characters, and MUST NOT contain the host name, user name, file paths or the list of other installed plugins."
    );
}

/// REQ: PDW-010
#[test]
fn pdw_010_unknown_rule_id_refused_with_exit_code_2() {
    // Observable: Run `ilium plugin report example.sync --submit --rule no-such-rule`: the exit code is 2, the output contains `report.rule-unknown` and no request is sent.
    panic!(
        "PDW-010 not implemented: The client MUST refuse a rule id not present in the cached `policy.json` with `report.rule-unknown` and exit code 2."
    );
}

/// REQ: PDW-011
#[test]
fn pdw_011_report_id_stored_and_status_lookup_prints_decision() {
    // Observable: Submit a report, then run `ilium plugin report --status <id>`: the output is one JSON line with `report_id` and `decision` set to `pending` or the recorded value.
    panic!(
        "PDW-011 not implemented: The intake MUST answer with a report id, the client MUST store it locally, and `ilium plugin report --status <report-id>` MUST fetch and print the recorded decision or `pending`."
    );
}

/// REQ: PDW-012
#[test]
fn pdw_012_rate_limit_returns_429_with_retry_after() {
    // Observable: Submit 11 reports with one token in a day: the 11th receives HTTP 429 with `Retry-After`, and the client prints `report.rate-limited` with the number of seconds.
    panic!(
        "PDW-012 not implemented: The registry MUST accept at most 10 reports per client token per day and MUST answer excess reports with HTTP 429 and a `Retry-After` header, which the client MUST print as `report.rate-limited` with the wait time."
    );
}

/// REQ: PDW-013
#[test]
fn pdw_013_vulnerability_report_redirected_to_security_contacts() {
    // Observable: Run `ilium plugin report example.sync --category vulnerability --submit`: the output prints both security contacts and no abuse report request is sent.
    panic!(
        "PDW-013 not implemented: A report with category `vulnerability` MUST be redirected to the plugin's `security_contact` and the registry security contact, and MUST NOT be filed as an abuse report."
    );
}

/// REQ: PDW-014
#[test]
fn pdw_014_reporter_identity_absent_from_transparency_log() {
    // Observable: Fetch the transparency log entry for a decided report: it contains the report id and decision but no token, email or client identifier.
    panic!(
        "PDW-014 not implemented: The reporter identity MUST NOT appear in any public document, including the transparency log entry for the decision ([PUB-045])."
    );
}
