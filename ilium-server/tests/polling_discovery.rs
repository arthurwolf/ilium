use std::io::Write;

use ilium_server::polling_discovery::discover;

#[test]
fn endpoint_hints_do_not_claim_bad_or_relative_records_are_live() {
    let root = tempfile::tempdir().unwrap();
    let good = uuid::Uuid::new_v4();
    let relative = uuid::Uuid::new_v4();
    let malformed = uuid::Uuid::new_v4();
    let write = |id: uuid::Uuid, content: &[u8]| {
        let path = root.path().join(format!("server-{id}.json"));
        let mut file = ilium_platform::secure_fs::open_private_regular_file(
            &path,
            ilium_platform::secure_fs::PrivateFileMode::CreateNew,
        )
        .unwrap();
        file.write_all(content).unwrap();
    };
    write(good, br#""/run/user/1000/ilium/example.sock""#);
    write(relative, br#""relative.sock""#);
    write(malformed, b"not json");
    let found = discover(root.path()).unwrap();
    assert!(found.incomplete);
    assert_eq!(found.endpoints.len(), 1);
    assert_eq!(found.endpoints[0].incarnation, good);
    assert!(found.endpoints[0].socket_path.is_absolute());
}

#[test]
fn unreadable_registry_is_not_treated_as_no_sessions() {
    let root = tempfile::tempdir().unwrap();
    assert!(discover(&root.path().join("missing")).is_err());
}

#[test]
fn oversized_endpoint_record_is_rejected_as_incomplete() {
    let root = tempfile::tempdir().unwrap();
    let path = root
        .path()
        .join(format!("server-{}.json", uuid::Uuid::new_v4()));
    let mut file = ilium_platform::secure_fs::open_private_regular_file(
        &path,
        ilium_platform::secure_fs::PrivateFileMode::CreateNew,
    )
    .unwrap();
    file.write_all(&vec![b'x'; 4097]).unwrap();
    let found = discover(root.path()).unwrap();
    assert!(found.incomplete);
    assert!(found.endpoints.is_empty());
}

#[test]
fn bounded_discovery_pages_eventually_cover_the_registry() {
    use ilium_server::polling_discovery::DiscoveryCursor;
    use std::collections::HashSet;
    let root = tempfile::tempdir().unwrap();
    let expected: HashSet<_> = (0..5).map(|_| uuid::Uuid::new_v4()).collect();
    for id in &expected {
        let path = root.path().join(format!("server-{id}.json"));
        let mut file = ilium_platform::secure_fs::open_private_regular_file(
            &path,
            ilium_platform::secure_fs::PrivateFileMode::CreateNew,
        )
        .unwrap();
        file.write_all(br#""/run/user/1000/ilium/example.sock""#)
            .unwrap();
    }
    let mut cursor = DiscoveryCursor::open(root.path()).unwrap();
    let mut actual = HashSet::new();
    for _ in 0..10 {
        let Some(page) = cursor.next_page(2).unwrap() else {
            break;
        };
        assert!(page.endpoints.len() <= 2);
        assert!(!page.incomplete);
        actual.extend(page.endpoints.into_iter().map(|entry| entry.incarnation));
    }
    assert_eq!(actual, expected);
}

#[test]
fn malformed_page_entries_remain_visible_as_uncertainty() {
    use ilium_server::polling_discovery::DiscoveryCursor;
    let root = tempfile::tempdir().unwrap();
    let bad = root
        .path()
        .join(format!("server-{}.json", uuid::Uuid::new_v4()));
    let mut file = ilium_platform::secure_fs::open_private_regular_file(
        &bad,
        ilium_platform::secure_fs::PrivateFileMode::CreateNew,
    )
    .unwrap();
    file.write_all(b"broken").unwrap();
    let mut cursor = DiscoveryCursor::open(root.path()).unwrap();
    let first = cursor.next_page(1).unwrap().unwrap();
    assert!(first.incomplete);
    assert!(first.endpoints.is_empty());
    assert!(cursor.next_page(0).is_err());
}

// A session without the optional resource service still needs a discoverable endpoint.
#[test]
fn owned_publication_is_discoverable_and_removes_only_its_own_record() {
    use ilium_server::polling_discovery::publish;
    let root = tempfile::tempdir().unwrap();
    let registry = root.path().join("registry");
    let incarnation = uuid::Uuid::new_v4();
    let socket = root.path().join("synthetic.sock");
    let publication = publish(&registry, incarnation, &socket).unwrap();
    let found = discover(&registry).unwrap();
    assert!(!found.incomplete);
    assert_eq!(found.endpoints.len(), 1);
    assert_eq!(found.endpoints[0].incarnation, incarnation);
    assert_eq!(found.endpoints[0].socket_path, socket);
    publication.remove().unwrap();
    assert!(discover(&registry).unwrap().endpoints.is_empty());
}

// A copied filename must never authorize deleting a replacement owner's record.
#[test]
fn publication_cleanup_preserves_a_replaced_record() {
    use ilium_server::polling_discovery::publish;
    let root = tempfile::tempdir().unwrap();
    let incarnation = uuid::Uuid::new_v4();
    let publication = publish(
        root.path(),
        incarnation,
        &root.path().join("synthetic.sock"),
    )
    .unwrap();
    let path = root.path().join(format!("server-{incarnation}.json"));
    std::fs::remove_file(&path).unwrap();
    let mut replacement = ilium_platform::secure_fs::open_private_regular_file(
        &path,
        ilium_platform::secure_fs::PrivateFileMode::CreateNew,
    )
    .unwrap();
    replacement
        .write_all(br#""/synthetic/replacement.sock""#)
        .unwrap();
    assert!(publication.remove().is_err());
    assert_eq!(
        std::fs::read(&path).unwrap(),
        br#""/synthetic/replacement.sock""#
    );
}

// A readable record alone must not authenticate its serving process. A kernel
// lock held by the publisher must remain observable until publication ends.
#[cfg(target_os = "linux")]
#[test]
fn publication_holds_a_birth_fenced_peer_lease_until_drop() {
    use ilium_platform::{file_lock_owner, process_control};
    let root = tempfile::tempdir().unwrap();
    let incarnation = uuid::Uuid::new_v4();
    let publication = ilium_server::polling_discovery::publish(
        root.path(),
        incarnation,
        &root.path().join("synthetic.sock"),
    )
    .unwrap();
    let path = root.path().join(format!("server-{incarnation}.json"));
    let process_id = std::process::id();
    let identity = process_control::capture_pty_process(process_id).unwrap();
    assert!(
        file_lock_owner::exclusive_lock_owned_by(&path, process_id, &identity).unwrap(),
        "a serving peer needs an actual kernel-owned publication lease"
    );
    drop(publication);
    assert!(!file_lock_owner::exclusive_lock_owned_by(&path, process_id, &identity).unwrap());
}
