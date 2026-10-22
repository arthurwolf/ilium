//! guide-enterprise-deployment.md: emergency disable of third-party plugins and the explicit
//! re-enable (SEC-457 .. SEC-458). Doc checks: each test asserts the rule text the manual page must state.

use crate::support::*;

/// REQ: SEC-457
#[test]
fn sec_457_the_enterprise_guide_states_the_one_step_third_party_disable_keeping_t1_running() {
    assert_page_has(
        "guide-enterprise-deployment.md",
        &[
            "An operator MUST be able to disable all third-party plugins at once",
            "`[plugins] disable_third_party = true`",
            "`ilium plugin disable --all-third-party --reason <text>`",
            "Bundled T1 plugins keep running",
        ],
    );
}

/// REQ: SEC-458
#[test]
fn sec_458_the_enterprise_guide_states_the_audited_disable_and_the_explicit_re_enable() {
    assert_page_has(
        "guide-enterprise-deployment.md",
        &[
            "Each emergency disable and each re-enable MUST write one audit record with the actor, the reason and the plugin count",
            "Re-enabling MUST be a separate explicit step (`ilium plugin enable --all-third-party`)",
            "it MUST NOT happen on its own",
        ],
    );
}
