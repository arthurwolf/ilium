/// REQ: PDT-027
#[test]
fn pdt_027_wit_capability_limit_or_manifest_change_needs_proposal() {
    // Observable: Open a pull request changing a WIT file without a `Proposal: P<n>` line in its description: the repository check fails with `governance.proposal-missing`.
    panic!(
        "PDT-027 not implemented: A change to a WIT interface, a capability, a limit listed in Errors and limits, or a manifest key MUST be filed as a proposal numbered `P<n>` before any merge."
    );
}

/// REQ: PDT-028
#[test]
fn pdt_028_proposal_states_problem_alternatives_and_effect() {
    // Observable: Validate a proposal file lacking the `Alternatives` section with the proposal template check: it fails naming `Alternatives`.
    panic!(
        "PDT-028 not implemented: A proposal MUST state the problem, the alternatives considered, the effect on existing packages, and, if it deprecates anything, the marking minor and the removal minor."
    );
}

/// REQ: PDT-029
#[test]
fn pdt_029_breaking_proposal_comment_period_at_least_30_days() {
    // Observable: Attempt to mark a breaking proposal `accepted` 20 days after it was opened: the template check fails with `governance.comment-period-open` and the days remaining.
    panic!(
        "PDT-029 not implemented: A proposal containing a breaking change MUST stay open for comment at least 30 days before the owner of the affected requirement prefix decides."
    );
}

/// REQ: PDT-030
#[test]
fn pdt_030_outcome_is_one_of_four_values_and_accepted_creates_decision() {
    // Observable: Set the outcome field to `approved`: the template check fails listing the four allowed values; an `accepted` proposal older than 7 days without a `D<n>` reference fails with `governance.decision-missing`.
    panic!(
        "PDT-030 not implemented: The outcome MUST be exactly one of `accepted`, `rejected`, `withdrawn` or `superseded`, and an accepted proposal MUST create a `D<n>` entry on Design decisions within 7 days."
    );
}

/// REQ: PDT-031
#[test]
fn pdt_031_proposal_must_not_change_contract_in_patch_release() {
    // Observable: Tag a patch release whose changelog references an accepted proposal that changes a WIT interface: the release check fails with `governance.contract-change-in-patch`.
    panic!(
        "PDT-031 not implemented: A proposal MUST NOT change a contract in a patch release."
    );
}

/// REQ: PDT-032
#[test]
fn pdt_032_rejected_proposal_reopen_requires_cited_evidence() {
    // Observable: Change a proposal from `rejected` to open without a `Reopened because` section: the template check fails with `governance.reopen-unjustified`.
    panic!(
        "PDT-032 not implemented: A rejected proposal MUST record its reason and MUST NOT be reopened without new evidence cited in a `Reopened because` section."
    );
}
