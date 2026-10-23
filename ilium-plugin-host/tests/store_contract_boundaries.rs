use ilium_plugin_api::{grants::Grant, side::Side};
use ilium_plugin_host::stores::{
    ConsentStore, GrantEvidence, GrantStore, StoreError, StoreNotice, TrustStore,
};
use serde_json::{Value, json};

const KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_KEY: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
fn grant(text: &str) -> Grant {
    Grant::parse(text).unwrap()
}
fn read(path: &std::path::Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn side_identity_and_epoch_survive_restart_without_cross_side_permissions() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = GrantStore::open(directory.path()).unwrap();
    store
        .set_for_side(
            "example.plugin",
            KEY,
            "T2",
            Side::Server,
            &[grant("tree.read")],
        )
        .unwrap();
    store
        .set_for_side(
            "example.plugin",
            KEY,
            "t2",
            Side::Client,
            &[grant("sound.play")],
        )
        .unwrap();
    let persisted = read(store.path());
    assert_eq!(persisted["plugins"]["example.plugin"]["epoch"], 2);
    assert_eq!(persisted["plugins"]["example.plugin"]["tier"], "t2");
    let mut reopened = GrantStore::open(directory.path()).unwrap();
    assert_eq!(
        reopened.load("example.plugin", KEY, "T2").unwrap(),
        vec![grant("tree.read")]
    );
    assert_eq!(
        reopened
            .load_for_side("example.plugin", KEY, "T2", Side::Client)
            .unwrap(),
        vec![grant("sound.play")]
    );
    assert!(
        reopened
            .load("example.plugin", OTHER_KEY, "T2")
            .unwrap()
            .is_empty()
    );
    reopened
        .set("example.plugin", OTHER_KEY, "T2", &[grant("tree.read")])
        .unwrap();
    assert!(
        reopened
            .load_for_side("example.plugin", OTHER_KEY, "T2", Side::Client)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        read(reopened.path())["plugins"]["example.plugin"]["epoch"],
        3
    );
}

#[test]
fn stale_handles_refuse_lost_updates_and_keep_their_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = GrantStore::open(directory.path()).unwrap();
    let mut stale = GrantStore::open(directory.path()).unwrap();
    first
        .set("example.first", KEY, "T2", &[grant("tree.read")])
        .unwrap();
    let bytes = std::fs::read(first.path()).unwrap();
    assert_eq!(
        stale.set("example.second", KEY, "T2", &[grant("tree.read")]),
        Err(StoreError::Conflict)
    );
    assert_eq!(std::fs::read(first.path()).unwrap(), bytes);
    assert!(stale.load("example.first", KEY, "T2").unwrap().is_empty());
    let mut refreshed = GrantStore::open(directory.path()).unwrap();
    refreshed
        .set("example.second", KEY, "T2", &[grant("tree.read")])
        .unwrap();
    assert_eq!(refreshed.load("example.first", KEY, "T2").unwrap().len(), 1);
}

#[test]
fn high_grants_need_actual_current_epoch_and_side_approval_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = GrantStore::open(directory.path()).unwrap();
    store
        .set(
            "example.plugin",
            KEY,
            "T2",
            &[grant("tree.read"), grant("terminal.write")],
        )
        .unwrap();
    let disk = std::fs::read(store.path()).unwrap();
    let without = GrantStore::open(directory.path()).unwrap();
    assert_eq!(
        without.load("example.plugin", KEY, "T2").unwrap(),
        vec![grant("tree.read")]
    );
    assert!(matches!(
        without.notices(),
        [StoreNotice::UnprovedHighGrant { .. }]
    ));
    assert_eq!(std::fs::read(store.path()).unwrap(), disk);
    let mut evidence = GrantEvidence {
        last_write_sha256: Some(store.sha256().into()),
        ..GrantEvidence::default()
    };
    evidence.approvals.insert((
        "example.plugin".into(),
        KEY.into(),
        "t2".into(),
        "server".into(),
        "terminal.write".into(),
        1,
    ));
    let approved = GrantStore::open_with_evidence(directory.path(), &evidence).unwrap();
    assert_eq!(approved.load("example.plugin", KEY, "T2").unwrap().len(), 2);
    evidence.approvals.clear();
    evidence.approvals.insert((
        "example.plugin".into(),
        KEY.into(),
        "t2".into(),
        "client".into(),
        "terminal.write".into(),
        1,
    ));
    assert_eq!(
        GrantStore::open_with_evidence(directory.path(), &evidence)
            .unwrap()
            .load("example.plugin", KEY, "T2")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn dynamic_high_risk_arguments_use_the_real_catalogue_validator() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = GrantStore::open(directory.path()).unwrap();
    store
        .set("example.plugin", KEY, "T2", &[grant("commands.invoke:*")])
        .unwrap();
    assert!(
        GrantStore::open(directory.path())
            .unwrap()
            .load("example.plugin", KEY, "T2")
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .set(
                "example.plugin",
                KEY,
                "T2",
                &[grant("fs.read:path:/private/**")]
            )
            .is_err()
    );
}

#[test]
fn invalid_id_side_duplicate_and_capacity_never_mutate_disk() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = GrantStore::open(directory.path()).unwrap();
    let before = std::fs::read(store.path()).unwrap();
    assert!(
        store
            .set("../escape", KEY, "T2", &[grant("tree.read")])
            .is_err()
    );
    assert!(
        store
            .set("example.plugin", "key-a", "T2", &[grant("tree.read")])
            .is_err()
    );
    assert!(
        store
            .set_for_side(
                "example.plugin",
                KEY,
                "T2",
                Side::Client,
                &[grant("panes.signal")]
            )
            .is_err()
    );
    assert!(
        store
            .set(
                "example.plugin",
                KEY,
                "T2",
                &[grant("tree.read"), grant("tree.read")]
            )
            .is_err()
    );
    assert!(
        store
            .set("example.plugin", KEY, "T2", &vec![grant("tree.read"); 257])
            .is_err()
    );
    assert_eq!(std::fs::read(store.path()).unwrap(), before);
}

#[test]
fn future_version_is_untouched_and_corrupt_duplicate_keys_are_quarantined_losslessly() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("grants.json");
    let future = b"{\"version\":999,\"plugins\":{}}";
    std::fs::write(&path, future).unwrap();
    assert!(matches!(
        GrantStore::open(directory.path()),
        Err(StoreError::UnsupportedVersion(999))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), future);
    let corrupt = b"{\"version\":1,\"version\":1,\"plugins\":{}}";
    std::fs::write(&path, corrupt).unwrap();
    let store = GrantStore::open(directory.path()).unwrap();
    let [StoreNotice::Unreadable(quarantine)] = store.notices() else {
        panic!("expected actual recovery notice")
    };
    assert_eq!(std::fs::read(quarantine).unwrap(), corrupt);
    assert_eq!(read(&path), json!({"version":1,"plugins":{}}));
}

#[test]
fn consent_denials_persist_per_target_and_reset_explicitly() {
    let directory = tempfile::tempdir().unwrap();
    let target = "http-hosts:api.example.com";
    let mut store = ConsentStore::open(directory.path()).unwrap();
    for _ in 0..3 {
        store.record("example.plugin", target, false).unwrap();
    }
    store.record("example.other", target, true).unwrap();
    let mut reopened = ConsentStore::open(directory.path()).unwrap();
    assert_eq!(reopened.decline_count("example.plugin", target).unwrap(), 3);
    assert_eq!(
        reopened
            .decline_count("example.plugin", "http-hosts:other.example.com")
            .unwrap(),
        0
    );
    assert!(!reopened.decisions("example.plugin")[0].allowed);
    reopened.reset("example.plugin", target).unwrap();
    assert_eq!(reopened.decline_count("example.plugin", target).unwrap(), 0);
    reopened.record("example.plugin", target, true).unwrap();
    assert!(reopened.decisions("example.plugin")[0].allowed);
    reopened.forget("example.plugin").unwrap();
    assert!(reopened.decisions("example.plugin").is_empty());
    assert_eq!(reopened.decisions("example.other").len(), 1);
}

#[test]
fn unknown_row_metadata_survives_mutation_and_missing_revoke_creates_no_rows() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = GrantStore::open(directory.path()).unwrap();
    store
        .set("example.plugin", KEY, "T2", &[grant("tree.read")])
        .unwrap();
    let mut value = read(store.path());
    value["host-metadata"] = json!({"preserve":[1,2,3]});
    value["plugins"]["example.plugin"]["decision-note"] = json!("actual retained user note");
    std::fs::write(store.path(), serde_json::to_vec(&value).unwrap()).unwrap();
    let mut store = GrantStore::open(directory.path()).unwrap();
    store.revoke("example.absent", "tree.read").unwrap();
    store
        .set(
            "example.plugin",
            KEY,
            "T2",
            &[grant("tree.read"), grant("sound.play")],
        )
        .unwrap();
    let value = read(store.path());
    assert!(value["plugins"].get("example.absent").is_none());
    assert_eq!(value["host-metadata"], json!({"preserve":[1,2,3]}));
    assert_eq!(
        value["plugins"]["example.plugin"]["decision-note"],
        "actual retained user note"
    );
}

#[test]
fn refusal_metadata_persists_without_fabricating_trusted_key_rows() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = TrustStore::open(directory.path()).unwrap();
    assert_eq!(store.state(KEY), "unknown");
    store.decline(KEY).unwrap();
    assert_eq!(
        TrustStore::open(directory.path()).unwrap().state(KEY),
        "trust.declined"
    );
    assert!(!directory.path().join("trust.json").exists());
    assert!(store.decline("not-a-fingerprint").is_err());
}

#[test]
fn startup_hash_mismatch_is_an_explicit_notice_not_an_invented_audit_record() {
    let directory = tempfile::tempdir().unwrap();
    GrantStore::open(directory.path()).unwrap();
    let evidence = GrantEvidence {
        last_write_sha256: Some("0".repeat(64)),
        ..GrantEvidence::default()
    };
    let store = GrantStore::open_with_evidence(directory.path(), &evidence).unwrap();
    assert!(store.notices().contains(&StoreNotice::ExternalEdit));
    assert!(
        ilium_plugin_host::stores::AuditLog::open(directory.path())
            .unwrap()
            .entries()
            .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn private_files_and_directory_and_symlink_refusal() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = tempfile::tempdir().unwrap();
    let store = GrantStore::open(directory.path()).unwrap();
    assert_eq!(
        std::fs::metadata(store.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(directory.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let target = directory.path().join("outside.json");
    std::fs::write(&target, b"preserve outside data").unwrap();
    symlink(&target, directory.path().join("consents.json")).unwrap();
    assert!(ConsentStore::open(directory.path()).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"preserve outside data");
}

#[test]
fn full_per_side_grant_capacity_is_usable_without_lowering_the_gate() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = GrantStore::open(directory.path()).unwrap();
    let mut grants: Vec<_> = (0..256)
        .map(|index| grant(&format!("topics.subscribe:topic-{index}")))
        .collect();
    store.set("example.plugin", KEY, "T2", &grants).unwrap();
    assert_eq!(
        GrantStore::open(directory.path())
            .unwrap()
            .load("example.plugin", KEY, "T2")
            .unwrap()
            .len(),
        256
    );
    let bytes = std::fs::read(store.path()).unwrap();
    grants.push(grant("topics.subscribe:topic-256"));
    assert!(store.set("example.plugin", KEY, "T2", &grants).is_err());
    assert_eq!(std::fs::read(store.path()).unwrap(), bytes);
}

#[test]
fn reader_returns_actual_retained_rotation_lines_in_sequence_order() {
    use ilium_plugin_host::audit::{
        AuditActor, AuditContext, AuditLog, AuditRecord, AuditRetention,
    };
    let directory = tempfile::tempdir().unwrap();
    let mut writer = AuditLog::open_with_context(
        directory.path().join("audit/audit.jsonl"),
        Some(AuditContext {
            session: "actual-test-session".into(),
            actor: AuditActor {
                kind: "kernel".into(),
                pane: None,
                plugin: None,
            },
        }),
        AuditRetention {
            max_bytes: 1,
            keep_days: 365,
        },
    )
    .unwrap();
    for at in 0..3 {
        writer
            .append_at(
                &AuditRecord {
                    at_unix_millis: at,
                    plugin: String::new(),
                    action: "safe-mode".into(),
                    detail_json: json!({"result":"ok"}).to_string(),
                },
                at,
            )
            .unwrap();
    }
    writer.verify().unwrap();
    let entries = ilium_plugin_host::stores::AuditLog::open(directory.path())
        .unwrap()
        .entries();
    assert_eq!(entries.len(), 3);
    for (index, entry) in entries.iter().enumerate() {
        assert_eq!(entry.kind, "safe-mode");
        assert!(entry.plugin.is_none());
        assert_eq!(
            serde_json::from_str::<Value>(&entry.line).unwrap()["seq"],
            (index + 1) as u64
        );
    }
}

#[test]
fn repeated_corruption_preserves_each_original_without_quarantine_overwrite() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("consents.json");
    std::fs::write(&path, b"first original broken document").unwrap();
    let first = ConsentStore::open(directory.path()).unwrap();
    let [StoreNotice::Unreadable(first_path)] = first.notices() else {
        panic!("missing first recovery")
    };
    std::fs::write(&path, b"second original broken document").unwrap();
    let second = ConsentStore::open(directory.path()).unwrap();
    let [StoreNotice::Unreadable(second_path)] = second.notices() else {
        panic!("missing second recovery")
    };
    assert_ne!(first_path, second_path);
    assert_eq!(
        std::fs::read(first_path).unwrap(),
        b"first original broken document"
    );
    assert_eq!(
        std::fs::read(second_path).unwrap(),
        b"second original broken document"
    );
}
