/// REQ: PDT-039
#[test]
fn pdt_039_claim_names_contract_suite_subject_counts() {
    // Observable: Validate a claim file missing `suite_version` with `ilium plugin verify-conformance`: the exit code is 2 and the output names `suite_version`.
    panic!(
        "PDT-039 not implemented: A conformance claim MUST name the contract `ilium:plugin@0.1.0`, the suite version, the subject, and the pass, skip and fail counts by case id."
    );
}

/// REQ: PDT-040
#[test]
fn pdt_040_invalid_on_retired_or_failed_case() {
    // Observable: Verify a report that counts a retired case id: the exit code is non-zero with `conformance.retired-case`; verify a report with one `fail`: the exit code is non-zero with `conformance.case-failed`.
    panic!(
        "PDT-040 not implemented: A claim MUST be invalid if any counted case has a retired id or if any case failed."
    );
}

/// REQ: PDT-041
#[test]
fn pdt_041_stale_after_ninety_days_of_new_minor() {
    // Observable: Verify a claim made against suite 1.2 when suite 1.3 was released 91 days ago: the output has `status: "stale"`.
    panic!(
        "PDT-041 not implemented: A claim MUST be re-run and re-published within 90 days of a new suite minor, otherwise verification MUST mark it `stale`."
    );
}

/// REQ: PDT-042
#[test]
fn pdt_042_report_includes_subject_sha256() {
    // Observable: Remove the `subject_sha256` field from a report: `ilium plugin verify-conformance` exits 2 naming the field.
    panic!(
        "PDT-042 not implemented: A report MUST include the SHA-256 of the component or host build under test, and the signed report MUST cover that hash."
    );
}

/// REQ: PDT-043
#[test]
fn pdt_043_verify_exit_codes_hash_mismatch_and_malformed() {
    // Observable: Run the command with `--subject` pointing at a different build: the exit code is 3 with `conformance.hash-mismatch`; run it on a non-JSON file: the exit code is 2.
    panic!(
        "PDT-043 not implemented: `ilium plugin verify-conformance <report>` MUST exit 3 when the subject hash does not match the provided artefact and exit 2 on malformed input."
    );
}

/// REQ: PDT-044
#[test]
fn pdt_044_no_ilium_name_beyond_permitted_wording() {
    // Observable: Validate a claim whose `statement` differs from the permitted wording: the template check fails with `conformance.wording-invalid`.
    panic!(
        "PDT-044 not implemented: A claim MUST NOT use the Ilium name or logo beyond the wording the governance page permits, and the published claim template MUST contain only that wording."
    );
}
