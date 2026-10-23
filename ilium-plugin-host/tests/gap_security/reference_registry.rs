//! Registry accounts page (SEC-723).

use crate::support::*;

/// REQ: SEC-723
#[test]
fn sec_723_publishing_without_a_second_factor_is_refused() {
    let rig = rig();
    let output = rig.cli(&["registry", "publish", NOTES, "--account", "acme-publisher"]);
    assert_ne!(output.exit_code, 0, "publish must require a second factor");
    assert!(
        error_codes(&output)
            .iter()
            .any(|code| code == "account.second-factor-required"),
        "expected account.second-factor-required, got {:?}",
        error_codes(&output)
    );
}

/// REQ: SEC-723
#[test]
fn sec_723_a_publisher_id_supports_at_least_two_maintainers_who_each_get_notices() {
    let rig = rig();
    rig.cli(&[
        "registry",
        "maintainers",
        "add",
        "acme-publisher",
        "second@example.test",
    ]);
    let listing = rig.cli(&["registry", "maintainers", "list", "acme-publisher"]);
    let maintainers = listing
        .stdout_jsonl
        .iter()
        .filter(|line| line["type"] == "maintainer")
        .count();
    assert!(
        maintainers >= 2,
        "a publisher needs at least two maintainers, found {maintainers}"
    );
    rig.cli(&[
        "registry",
        "publish",
        NOTES,
        "--account",
        "acme-publisher",
        "--totp",
        "123456",
    ]);
    let notified = rig.registry_notices_sent("acme-publisher");
    assert!(
        notified >= 2,
        "every maintainer must get a notice on publish, got {notified} notices"
    );
}
