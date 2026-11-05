/// REQ: PDT-033
#[test]
fn pdt_033_test_report_junit_one_testcase_per_case() {
    // Observable: Convert a native report with 12 cases: the XML parses, and `count(//testcase)` is 12.
    panic!(
        "PDT-033 not implemented: `ilium plugin test-report <report.json> --format junit --out <path>` MUST write a JUnit XML file with one `testcase` element per case in the native report."
    );
}

/// REQ: PDT-034
#[test]
fn pdt_034_test_report_junit_status_mapping_failure_skipped_flaky() {
    // Observable: Convert a report containing one fail, one skipped and one flaky case: the XML has one `failure`, one `skipped`, and one `testcase` with `<property name="flaky" value="true"/>` and no `failure` child.
    panic!(
        "PDT-034 not implemented: A `fail` or `trap` status MUST map to a `failure` element, `skipped` to a `skipped` element, and a case with `flaky: true` MUST map to a passing testcase carrying a property `flaky` with value `true`."
    );
}

/// REQ: PDT-035
#[test]
fn pdt_035_test_report_junit_no_paths_users_or_timestamps() {
    // Observable: Convert the same native report on two machines with different users and home directories: the two XML files are byte-identical.
    panic!(
        "PDT-035 not implemented: The JUnit file MUST NOT contain absolute paths, user names or wall-clock timestamps, so output from different machines compares equal ([DEV-890])."
    );
}

/// REQ: PDT-036
#[test]
fn pdt_036_test_report_unknown_format_exit_code_2() {
    // Observable: Run `ilium plugin test-report report.json --format tap`: the exit code is 2 and the output contains `usage.invalid-value`.
    panic!(
        "PDT-036 not implemented: An unknown `--format` value MUST fail with exit code 2 and `usage.invalid-value`."
    );
}

/// REQ: PDT-037
#[test]
fn pdt_037_test_report_interrupted_run_complete_false() {
    // Observable: Interrupt a run with SIGINT and convert its report: the XML parses and contains `<property name="complete" value="false"/>`.
    panic!(
        "PDT-037 not implemented: A native report from an interrupted run (exit 130) MUST still convert to a well-formed JUnit file carrying the testsuite property `complete` with value `false`."
    );
}

/// REQ: PDT-038
#[test]
fn pdt_038_test_report_conformance_uses_case_id_as_name() {
    // Observable: Convert a `test --conformance` report: every `testcase` `name` attribute equals a conformance case id of the form `C-<n>`.
    panic!(
        "PDT-038 not implemented: The conformance suite report MUST convert with the same command and formats, using the case id as the `testcase` name."
    );
}
