//! Storage import (api-storage.md): LIF-117..LIF-121 (LIF-115 is in guide_data_migration).

use ilium_plugin_api::ErrorCode;
use proptest::prelude::*;

use super::common::*;

fn started() -> (MigrationRig, MigServer) {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let server = running_v1(&rig, "a", "/p/one", "default");
    (rig, server)
}

fn entries(count: usize, value_len: usize) -> Vec<(String, Option<Vec<u8>>)> {
    (0..count)
        .map(|index| (format!("k{index:04}"), Some(vec![b'v'; value_len])))
        .collect()
}

/// REQ: LIF-117
#[test]
fn lif_117_batch_of_256_entries_is_applied() {
    let (_rig, server) = started();
    server
        .storage(NOTES)
        .write_batch(Bucket::Global, &entries(256, 1))
        .expect("256 entries");
    assert_eq!(dump(&server, Bucket::Global).len(), 256);
}

/// REQ: LIF-117
#[test]
fn lif_117_batch_of_257_entries_is_invalid_argument() {
    let (_rig, server) = started();
    let error = server
        .storage(NOTES)
        .write_batch(Bucket::Global, &entries(257, 1))
        .expect_err("too many");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert!(
        dump(&server, Bucket::Global).is_empty(),
        "a refused batch writes nothing"
    );
}

/// REQ: LIF-117
#[test]
fn lif_117_batch_of_255_entries_is_applied() {
    let (_rig, server) = started();
    server
        .storage(NOTES)
        .write_batch(Bucket::Global, &entries(255, 1))
        .expect("255 entries");
    assert_eq!(dump(&server, Bucket::Global).len(), 255);
}

/// REQ: LIF-117
#[test]
fn lif_117_batch_of_exactly_4_mib_is_applied() {
    let (_rig, server) = started();
    // Four one-byte keys keep every value within its independent 1 MiB cap.
    let batch: Vec<_> = ["a", "b", "c", "d"].into_iter().enumerate().map(|(index,key)| (key.to_owned(), Some(vec![0; MIB as usize - if index == 3 { 4 } else { 0 }]))).collect();
    server
        .storage(NOTES)
        .write_batch(Bucket::Global, &batch)
        .expect("exactly 4 MiB");
}

/// REQ: LIF-117
#[test]
fn lif_117_batch_one_byte_over_4_mib_is_invalid_argument() {
    let (_rig, server) = started();
    // Four one-byte keys keep every value within its independent 1 MiB cap.
    let batch: Vec<_> = ["a", "b", "c", "d"].into_iter().enumerate().map(|(index,key)| (key.to_owned(), Some(vec![0; MIB as usize - if index == 3 { 3 } else { 0 }]))).collect();
    let error = server
        .storage(NOTES)
        .write_batch(Bucket::Global, &batch)
        .expect_err("over");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: LIF-117
#[test]
fn lif_117_batch_one_byte_under_4_mib_is_applied() {
    let (_rig, server) = started();
    // Four one-byte keys keep every value within its independent 1 MiB cap.
    let batch: Vec<_> = ["a", "b", "c", "d"].into_iter().enumerate().map(|(index,key)| (key.to_owned(), Some(vec![0; MIB as usize - if index == 3 { 5 } else { 0 }]))).collect();
    server
        .storage(NOTES)
        .write_batch(Bucket::Global, &batch)
        .expect("under");
}

/// REQ: LIF-117
#[test]
fn lif_117_quota_breaking_last_entry_keeps_every_previous_value() {
    let (rig, server) = started();
    rig.set_plugin_quota(NOTES, Bucket::Global, 1024);
    server
        .storage(NOTES)
        .put(Bucket::Global, "a", b"old-a")
        .expect("put");
    server
        .storage(NOTES)
        .put(Bucket::Global, "b", b"old-b")
        .expect("put");
    let batch = vec![
        ("a".to_owned(), Some(bytes("new-a"))),
        ("b".to_owned(), Some(bytes("new-b"))),
        ("c".to_owned(), Some(vec![0u8; 4096])),
    ];
    let error = server
        .storage(NOTES)
        .write_batch(Bucket::Global, &batch)
        .expect_err("quota");
    assert_eq!(error.code, ErrorCode::QuotaExceeded);
    let data = dump(&server, Bucket::Global);
    assert_eq!(data.get("a"), Some(&bytes("old-a")));
    assert_eq!(data.get("b"), Some(&bytes("old-b")));
    assert_eq!(data.get("c"), None);
}

/// REQ: LIF-117
#[test]
fn lif_117_batch_removals_and_puts_apply_together() {
    let (_rig, server) = started();
    server
        .storage(NOTES)
        .put(Bucket::Global, "gone", b"x")
        .expect("put");
    server
        .storage(NOTES)
        .write_batch(
            Bucket::Global,
            &[
                ("gone".to_owned(), None),
                ("new".to_owned(), Some(bytes("y"))),
            ],
        )
        .expect("batch");
    let data = dump(&server, Bucket::Global);
    assert_eq!(data.get("gone"), None);
    assert_eq!(data.get("new"), Some(&bytes("y")));
}

proptest! {
    /// REQ: LIF-117
    #[test]
    fn lif_117_entry_count_limit_is_exactly_256(count in 0usize..=400) {
        let (_rig, server) = started();
        let result = server.storage(NOTES).write_batch(Bucket::Global, &entries(count, 1));
        if count <= 256 {
            prop_assert!(result.is_ok());
        } else {
            prop_assert_eq!(result.expect_err("over").code, ErrorCode::InvalidArgument);
        }
    }
}

/// REQ: LIF-118
#[test]
fn lif_118_put_is_flushed_before_return() {
    let (rig, server) = started();
    server
        .storage(NOTES)
        .put(Bucket::Global, "k", b"v")
        .expect("put");
    assert!(
        rig.unflushed_bytes(NOTES) == 0,
        "nothing pending after return"
    );
    rig.power_loss();
    assert_eq!(
        rig.read_after_power_loss(NOTES, Bucket::Global, "k"),
        Some(bytes("v"))
    );
}

/// REQ: LIF-118
#[test]
fn lif_118_remove_is_flushed_before_return() {
    let (rig, server) = started();
    server
        .storage(NOTES)
        .put(Bucket::Global, "k", b"v")
        .expect("put");
    server
        .storage(NOTES)
        .remove(Bucket::Global, "k")
        .expect("remove");
    rig.power_loss();
    assert_eq!(rig.read_after_power_loss(NOTES, Bucket::Global, "k"), None);
}

/// REQ: LIF-118
#[test]
fn lif_118_batch_is_flushed_before_return() {
    let (rig, server) = started();
    server
        .storage(NOTES)
        .write_batch(Bucket::Global, &entries(10, 8))
        .expect("batch");
    rig.power_loss();
    assert_eq!(
        rig.read_after_power_loss(NOTES, Bucket::Global, "k0009"),
        Some(vec![b'v'; 8])
    );
}

/// REQ: LIF-118
#[test]
fn lif_118_crash_in_the_middle_of_a_batch_leaves_the_previous_values() {
    let (rig, server) = started();
    server
        .storage(NOTES)
        .put(Bucket::Global, "k0000", b"old")
        .expect("put");
    rig.crash_during_next_write();
    let _ = server
        .storage(NOTES)
        .write_batch(Bucket::Global, &entries(50, 8));
    let restarted = rig.restart_server(&server);
    restarted.activate(NOTES);
    let data = dump(&restarted, Bucket::Global);
    // LIF-118: a crash before the return leaves the previous value, none of the batch
    assert_eq!(data.len(), 1, "found {:?}", data.keys().collect::<Vec<_>>());
    assert_eq!(data.get("k0000"), Some(&bytes("old")));
}

/// REQ: LIF-119
#[test]
fn lif_119_full_disk_put_is_io_no_space_never_quota_exceeded() {
    let (rig, server) = started();
    rig.fill_disk();
    let error = server
        .storage(NOTES)
        .put(Bucket::Global, "k", &vec![0u8; 64 * 1024])
        .expect_err("disk full");
    assert_eq!(error.code, ErrorCode::Io);
    assert_eq!(error.message, "no-space");
}

/// REQ: LIF-119
#[test]
fn lif_119_full_disk_batch_is_io_no_space() {
    let (rig, server) = started();
    rig.fill_disk();
    let error = server
        .storage(NOTES)
        .write_batch(Bucket::Global, &entries(8, 8192))
        .expect_err("disk full");
    assert_eq!(error.code, ErrorCode::Io);
    assert_eq!(error.message, "no-space");
}

/// REQ: LIF-119
#[test]
fn lif_119_own_quota_is_quota_exceeded_not_io() {
    let (rig, server) = started();
    rig.set_plugin_quota(NOTES, Bucket::Global, 128);
    let error = server
        .storage(NOTES)
        .put(Bucket::Global, "k", &vec![0u8; 1024])
        .expect_err("quota");
    assert_eq!(error.code, ErrorCode::QuotaExceeded);
}

/// REQ: LIF-119
#[test]
fn lif_119_full_disk_keeps_the_previous_value() {
    let (rig, server) = started();
    server
        .storage(NOTES)
        .put(Bucket::Global, "k", b"old")
        .expect("put");
    rig.fill_disk();
    let _ = server
        .storage(NOTES)
        .put(Bucket::Global, "k", &vec![0u8; 64 * 1024]);
    rig.free_disk();
    assert_eq!(
        server.storage(NOTES).get(Bucket::Global, "k"),
        Some(bytes("old"))
    );
}

/// REQ: LIF-120
#[test]
fn lif_120_two_servers_write_the_same_global_key_last_writer_wins() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let first = running_v1(&rig, "a", "/p/one", "alpha");
    let second = running_v1(&rig, "b", "/p/two", "beta");
    first
        .storage(NOTES)
        .put(Bucket::Global, "k", b"first")
        .expect("put");
    second
        .storage(NOTES)
        .put(Bucket::Global, "k", b"second")
        .expect("put");
    assert_eq!(
        first.storage(NOTES).get(Bucket::Global, "k"),
        Some(bytes("second"))
    );
    assert_eq!(
        second.storage(NOTES).get(Bucket::Global, "k"),
        Some(bytes("second"))
    );
}

/// REQ: LIF-120
#[test]
fn lif_120_two_servers_write_the_same_project_bucket() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let first = running_v1(&rig, "a", "/p/one", "alpha");
    let second = running_v1(&rig, "b", "/p/one", "beta");
    first
        .storage(NOTES)
        .put(Bucket::Project, "k", b"first")
        .expect("put");
    second
        .storage(NOTES)
        .put(Bucket::Project, "k", b"second")
        .expect("put");
    assert_eq!(
        first.storage(NOTES).get(Bucket::Project, "k"),
        Some(bytes("second"))
    );
}

/// REQ: LIF-120
#[test]
fn lif_120_concurrent_batches_are_atomic_across_processes() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let first = running_v1(&rig, "a", "/p/one", "alpha");
    let second = running_v1(&rig, "b", "/p/two", "beta");
    let batch_a: Vec<(String, Option<Vec<u8>>)> = (0..64)
        .map(|index| (format!("k{index}"), Some(bytes("A"))))
        .collect();
    let batch_b: Vec<(String, Option<Vec<u8>>)> = (0..64)
        .map(|index| (format!("k{index}"), Some(bytes("B"))))
        .collect();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            first
                .storage(NOTES)
                .write_batch(Bucket::Global, &batch_a)
                .expect("A")
        });
        scope.spawn(|| {
            second
                .storage(NOTES)
                .write_batch(Bucket::Global, &batch_b)
                .expect("B")
        });
    });
    let data = dump(&first, Bucket::Global);
    let values: std::collections::BTreeSet<&Vec<u8>> = data.values().collect();
    assert_eq!(values.len(), 1, "one whole batch won, never a mix");
}

/// REQ: LIF-120
#[test]
fn lif_120_there_is_no_compare_and_set() {
    let (_rig, server) = started();
    assert!(!server.storage(NOTES).has_compare_and_set());
}

/// REQ: LIF-121
#[test]
fn lif_121_stale_instance_put_is_conflict_stale_data_version() {
    let rig = rig();
    install_ok(&rig, &notes_v1());
    let current = running_v1(&rig, "a", "/p/one", "alpha");
    let stale = running_v1(&rig, "b", "/p/two", "beta");
    current
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    update_ok(&rig, &notes_v2(Vec::new()), &[]);
    current.upgrade(NOTES);
    let error = stale
        .storage(NOTES)
        .put(Bucket::Global, "g", b"old")
        .expect_err("stale");
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(error.message, "stale-data-version");
}

/// REQ: LIF-121
#[test]
fn lif_121_equal_stamp_writes_are_accepted() {
    let (_rig, server) = started();
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"2")
        .expect("put again");
}

/// REQ: LIF-121
#[test]
fn lif_121_the_stamp_is_not_listed_among_keys() {
    let (_rig, server) = started();
    server
        .storage(NOTES)
        .put(Bucket::Global, "g", b"1")
        .expect("put");
    assert_eq!(
        server.storage(NOTES).keys(Bucket::Global, ""),
        vec!["g".to_owned()]
    );
}
