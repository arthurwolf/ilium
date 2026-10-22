//! reference-threat-model.md: per-user paths, cross-user sharing and cross-user isolation
//! (SEC-454 .. SEC-456). Doc checks: each test asserts the rule text the manual page must state.

use crate::support::*;

/// REQ: SEC-454
#[test]
fn sec_454_the_threat_model_states_the_per_user_0700_paths_and_the_refuse_to_start_rule() {
    assert_page_has(
        "reference-threat-model.md",
        &[
            "Each OS user MUST have a separate runtime directory, socket, module cache, trust store and state folder",
            "all with mode 0700 owned by that user",
            "A server MUST refuse to start and name the path",
        ],
    );
}

/// REQ: SEC-455
#[test]
fn sec_455_the_threat_model_forbids_cross_user_sharing_and_writable_policy_files() {
    assert_page_has(
        "reference-threat-model.md",
        &[
            "The host MUST NOT share any compiled module, package cache or secret between OS users",
            "even when the plugin and its hash are identical",
            "readable by all users and writable only by root",
        ],
    );
}

/// REQ: SEC-456
#[test]
fn sec_456_the_threat_model_states_that_helpers_cannot_see_other_users_and_peer_stays_per_user() {
    assert_page_has(
        "reference-threat-model.md",
        &[
            "A plugin helper MUST NOT be able to see another OS user's processes, sockets or files through the sandbox",
            "the `peer` scope of messaging MUST stay inside one OS user's server",
        ],
    );
}
