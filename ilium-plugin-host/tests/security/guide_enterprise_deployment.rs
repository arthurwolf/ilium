//! guide-enterprise-deployment.md: multi-user isolation (SEC-194 .. SEC-197). The behaviour tests
//! live in reference_admin_policy.rs next to the policy that drives them; this module checks the
//! guide itself states each rule and that the rules agree with the reference page.

use crate::support::*;

/// REQ: SEC-194
#[test]
fn sec_194_guide_lists_every_per_user_store() {
    assert_page_has("guide-enterprise-deployment.md", &["grants, consents, trust, project trust, secrets, audit log and plugin install directory"]);
}

/// REQ: SEC-195
#[test]
fn sec_195_guide_says_the_system_package_directory_is_read_only() {
    assert_page_has(
        "guide-enterprise-deployment.md",
        &[
            "read-only to users",
            "copy into the user's install directory",
        ],
    );
}

/// REQ: SEC-196
#[test]
fn sec_196_guide_says_runtime_directories_are_per_user_and_0700() {
    assert_page_has(
        "guide-enterprise-deployment.md",
        &["mode 0700", "same user's sessions"],
    );
}

/// REQ: SEC-197
#[test]
fn sec_197_guide_forbids_running_as_root() {
    assert_page_has(
        "guide-enterprise-deployment.md",
        &["Do not run Ilium as root", "SEC-056"],
    );
}
