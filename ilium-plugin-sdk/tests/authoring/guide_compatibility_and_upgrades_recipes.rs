//! Upgrade rules run on the fake host (guide-compatibility-and-upgrades.md).

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::DeactivationReason;

use crate::support::{page, Versioned};

/// REQ: AUT-206
#[test]
fn aut_206_known_older_format_byte_is_read_when_the_data_version_matches() {
    let mut host = FakeHost::new();
    host.activate::<Versioned>();
    host.restart_with(vec![Versioned::PREVIOUS_FORMAT, 9]);
    let saved = host
        .deactivate(DeactivationReason::Upgrade)
        .expect("restore bytes");
    assert_eq!(
        saved[0],
        Versioned::FORMAT,
        "the new release writes its own format byte"
    );
    assert_eq!(Versioned::load(&saved).value, 9);
}

/// REQ: AUT-206
#[test]
fn aut_206_unknown_format_byte_gives_defaults() {
    let mut host = FakeHost::new();
    host.activate::<Versioned>();
    host.restart_with(vec![0, 9]);
    let saved = host
        .deactivate(DeactivationReason::Upgrade)
        .expect("restore bytes");
    assert_eq!(Versioned::load(&saved).value, 0);
}

/// REQ: AUT-215
#[test]
fn aut_215_behaviour_does_not_depend_on_the_deactivate_reason() {
    let reasons = [
        DeactivationReason::Shutdown,
        DeactivationReason::Disabled,
        DeactivationReason::Upgrade,
        DeactivationReason::Revoked,
        DeactivationReason::Granted,
    ];
    let mut seen = Vec::new();
    for reason in reasons {
        let mut host = FakeHost::new();
        host.activate::<Versioned>();
        host.restart_with(Versioned { value: 3 }.save());
        seen.push(host.deactivate(reason));
    }
    assert!(
        seen.windows(2).all(|pair| pair[0] == pair[1]),
        "the saved bytes are the same for every reason"
    );
}

/// REQ: AUT-215
#[test]
fn aut_215_restart_with_counts_as_upgrade_and_is_not_a_failure() {
    let mut host = FakeHost::new();
    host.activate::<Versioned>();
    host.restart_with(Versioned { value: 3 }.save());
    assert!(host.failures().is_empty());
    assert!(host.restarts().is_empty());
    assert!(!host.is_disabled());
}

/// REQ: AUT-207
#[test]
fn aut_207_data_in_storage_survives_a_restart_with_other_restore_bytes() {
    let snapshot = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/authoring/fixtures/storage-v1.json"
    );
    let mut host = FakeHost::new()
        .with_data_version(2)
        .with_storage_snapshot(snapshot);
    host.activate::<Versioned>();
    host.restart_with(vec![9, 9, 9]);
    assert_eq!(
        host.snapshot().storage["global"]["notes"],
        b"notes".to_vec()
    );
}

/// REQ: AUT-202
#[test]
fn doc_aut_202_the_table_lists_the_nine_names_that_may_not_change() {
    let text = page("guide-compatibility-and-upgrades");
    for name in [
        "Plugin `id`",
        "Pane kind id",
        "Command id",
        "Settings key",
        "Decoration slot",
        "Topic name",
        "Storage key",
        "Service and method names",
        "Extension point id",
    ] {
        assert!(text.contains(&format!("| {name} |")), "row {name} missing");
    }
}

/// REQ: AUT-201, AUT-210
#[test]
fn doc_aut_201_every_version_rule_cites_the_defining_reuse_code() {
    let text = page("guide-compatibility-and-upgrades");
    assert!(text.contains("`package.version-reused`"));
    assert!(text.contains("refused with `package.version-reused`"));
}
