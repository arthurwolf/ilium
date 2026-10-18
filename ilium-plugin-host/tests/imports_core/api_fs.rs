//! api-fs.md: roots, metadata, list-dir, read, read-all, scan, write, create-dir, remove, rename,
//! watch and unwatch, with the `take-*` results for the async calls.

use ilium_plugin_api::generated::host::{EntryKind, WriteMode};
use ilium_plugin_api::generated::types::{ErrorCode, Outcome};
use ilium_plugin_api::Tier;
use proptest::prelude::*;

use crate::support::*;

const RW: [&str; 2] = ["fs.read:project", "fs.write:project"];

fn rw_fixture(
    root: &std::path::Path,
) -> ilium_plugin_host::testing::imports_fixture::ImportsFixture {
    fixture_in(root, Tier::T2, &RW)
}

fn write_file(
    fixture: &mut ilium_plugin_host::testing::imports_fixture::ImportsFixture,
    path: &str,
    data: &[u8],
) {
    let request = expect_ok(fixture.fs().write(
        project_path(path),
        data.to_vec(),
        WriteMode::Replace,
    ));
    assert!(is_ok(&complete(fixture, request)), "write {path}");
}

fn failed_code(outcome: &Outcome) -> Option<ErrorCode> {
    outcome_code(outcome)
}

// ---- roots, metadata -------------------------------------------------------------------------

/// REQ: WIT:ilium:host/fs.roots
#[test]
fn fs_roots_lists_the_project_scope_with_its_writability() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    let roots = fixture.fs().roots();
    let project = roots
        .iter()
        .find(|root| root.scope == "project")
        .expect("project scope");
    assert!(project.writable);
}

/// REQ: WIT:ilium:host/fs.roots
#[test]
fn fs_roots_read_only_grant_marks_the_scope_not_writable() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture_in(dir.path(), Tier::T2, &["fs.read:project"]);
    let roots = fixture.fs().roots();
    let project = roots
        .iter()
        .find(|root| root.scope == "project")
        .expect("project scope");
    assert!(!project.writable);
}

/// REQ: WIT:ilium:host/fs.roots
#[test]
fn fs_roots_without_any_fs_grant_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture_in(dir.path(), Tier::T2, &[]);
    assert!(fixture
        .fs()
        .roots()
        .iter()
        .all(|root| root.scope != "project"));
}

/// REQ: WIT:ilium:host/fs.metadata
#[test]
fn fs_metadata_of_a_file_reports_kind_and_length() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"12345").unwrap();
    let mut fixture = rw_fixture(dir.path());
    let metadata = expect_ok(fixture.fs().metadata(project_path("a.txt")));
    assert_eq!(metadata.kind, EntryKind::File);
    assert_eq!(metadata.length, 5);
}

/// REQ: WIT:ilium:host/fs.metadata
#[test]
fn fs_metadata_of_a_directory_reports_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let mut fixture = rw_fixture(dir.path());
    assert_eq!(
        expect_ok(fixture.fs().metadata(project_path("sub"))).kind,
        EntryKind::Directory
    );
}

/// REQ: WIT:ilium:host/fs.metadata
#[test]
fn fs_metadata_missing_path_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    assert_eq!(
        expect_code(fixture.fs().metadata(project_path("missing"))),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/fs.metadata
#[test]
fn fs_metadata_without_the_read_grant_not_permitted() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
    let mut fixture = fixture_in(dir.path(), Tier::T2, &[]);
    assert_eq!(
        expect_code(fixture.fs().metadata(project_path("a.txt"))),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/fs.metadata
#[test]
fn fs_metadata_path_escaping_the_scope_not_permitted() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    for path in ["../outside", "a/../../outside", "/etc/passwd"] {
        assert_eq!(
            expect_code(fixture.fs().metadata(project_path(path))),
            ErrorCode::NotPermitted,
            "{path}"
        );
    }
}

// ---- list-dir --------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/fs.list-dir
#[test]
fn fs_list_dir_returns_the_entries_with_metadata() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"1").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let mut fixture = rw_fixture(dir.path());
    let mut entries = expect_ok(fixture.fs().list_dir(project_path("."), 100));
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    let names: Vec<_> = entries.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, ["a.txt", "sub"]);
    assert_eq!(entries[1].metadata.kind, EntryKind::Directory);
}

/// REQ: WIT:ilium:host/fs.list-dir
#[test]
fn fs_list_dir_max_entries_limits_the_result() {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..10 {
        std::fs::write(dir.path().join(format!("f{index}")), b"").unwrap();
    }
    let mut fixture = rw_fixture(dir.path());
    assert_eq!(
        expect_ok(fixture.fs().list_dir(project_path("."), 4)).len(),
        4
    );
}

/// REQ: WIT:ilium:host/fs.list-dir
#[test]
fn fs_list_dir_max_entries_4096_accepted_and_4097_invalid_argument() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    expect_ok(fixture.fs().list_dir(project_path("."), 4096));
    assert_eq!(
        expect_code(fixture.fs().list_dir(project_path("."), 4097)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/fs.list-dir
#[test]
fn fs_list_dir_of_a_file_or_missing_directory_fails() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"1").unwrap();
    let mut fixture = rw_fixture(dir.path());
    assert_eq!(
        expect_code(fixture.fs().list_dir(project_path("nope"), 10)),
        ErrorCode::NotFound
    );
    assert_eq!(
        expect_code(fixture.fs().list_dir(project_path("a.txt"), 10)),
        ErrorCode::InvalidArgument
    );
}

// ---- read ------------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/fs.read
#[test]
fn fs_read_returns_the_bytes_from_the_offset() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"0123456789").unwrap();
    let mut fixture = rw_fixture(dir.path());
    assert_eq!(
        expect_ok(fixture.fs().read(project_path("a.txt"), 3, 4)),
        b"3456"
    );
}

/// REQ: WIT:ilium:host/fs.read
#[test]
fn fs_read_past_the_end_returns_the_available_bytes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"0123").unwrap();
    let mut fixture = rw_fixture(dir.path());
    assert_eq!(
        expect_ok(fixture.fs().read(project_path("a.txt"), 2, 100)),
        b"23"
    );
    assert!(expect_ok(fixture.fs().read(project_path("a.txt"), 4, 10)).is_empty());
}

/// REQ: WIT:ilium:host/fs.read
#[test]
fn fs_read_max_bytes_of_exactly_1_mib_accepted_and_one_more_invalid_argument() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.bin"), vec![1u8; 2 * MIB]).unwrap();
    let mut fixture = rw_fixture(dir.path());
    assert_eq!(
        expect_ok(fixture.fs().read(project_path("a.bin"), 0, MIB as u32)).len(),
        MIB
    );
    assert_eq!(
        expect_code(fixture.fs().read(project_path("a.bin"), 0, MIB as u32 + 1)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/fs.read
#[test]
fn fs_read_missing_file_not_found_and_ungranted_scope_not_permitted() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    assert_eq!(
        expect_code(fixture.fs().read(project_path("missing"), 0, 10)),
        ErrorCode::NotFound
    );
    assert_eq!(
        expect_code(fixture.fs().read(scoped_path("home", "x"), 0, 10)),
        ErrorCode::NotPermitted
    );
}

// ---- read-all, take-bytes --------------------------------------------------------------------

/// REQ: WIT:ilium:host/fs.read-all, WIT:ilium:host/fs.take-bytes
#[test]
fn fs_read_all_completes_and_take_bytes_returns_the_content_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"hello world").unwrap();
    let mut fixture = rw_fixture(dir.path());
    let request = expect_ok(fixture.fs().read_all(project_path("a.txt")));
    assert!(is_ok(&complete(&mut fixture, request)));
    assert_eq!(
        fixture.fs().take_bytes(request).as_deref(),
        Some(&b"hello world"[..])
    );
    assert!(fixture.fs().take_bytes(request).is_none(), "taken once");
}

/// REQ: WIT:ilium:host/fs.take-bytes
#[test]
fn fs_take_bytes_before_the_completion_or_for_an_unknown_request_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    assert!(fixture
        .fs()
        .take_bytes(ilium_plugin_api::generated::types::RequestId(987_654))
        .is_none());
}

/// REQ: WIT:ilium:host/fs.read-all
#[test]
fn fs_read_all_missing_file_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    match fixture.fs().read_all(project_path("missing")) {
        Err(error) => assert_eq!(error.code, ErrorCode::NotFound),
        Ok(request) => assert_eq!(
            failed_code(&complete(&mut fixture, request)),
            Some(ErrorCode::NotFound)
        ),
    }
}

/// REQ: WIT:ilium:host/fs.read-all
#[test]
fn fs_read_all_file_over_the_read_all_quota_fails_quota_exceeded() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("big.bin"), vec![0u8; 3 * MIB]).unwrap();
    let mut fixture = builder_in(dir.path(), Tier::T2, &RW)
        .limit("fs_read_all_mib", 2)
        .build();
    let request = expect_ok(fixture.fs().read_all(project_path("big.bin")));
    assert_eq!(
        failed_code(&complete(&mut fixture, request)),
        Some(ErrorCode::QuotaExceeded)
    );
}

/// REQ: WIT:ilium:host/fs.read-all
#[test]
fn fs_read_all_file_exactly_at_the_read_all_quota_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("fit.bin"), vec![0u8; 2 * MIB]).unwrap();
    let mut fixture = builder_in(dir.path(), Tier::T2, &RW)
        .limit("fs_read_all_mib", 2)
        .build();
    let request = expect_ok(fixture.fs().read_all(project_path("fit.bin")));
    assert!(is_ok(&complete(&mut fixture, request)));
}

/// REQ: WIT:ilium:host/fs.read-all
#[test]
fn fs_read_all_without_the_read_grant_not_permitted() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture_in(dir.path(), Tier::T2, &[]);
    assert_eq!(
        expect_code(fixture.fs().read_all(project_path("a.txt"))),
        ErrorCode::NotPermitted
    );
}

// ---- scan, take-entries ----------------------------------------------------------------------

fn scan_tree(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("src/deep")).unwrap();
    std::fs::write(dir.join("src/a.rs"), b"").unwrap();
    std::fs::write(dir.join("src/deep/b.rs"), b"").unwrap();
    std::fs::write(dir.join("src/c.txt"), b"").unwrap();
}

/// REQ: WIT:ilium:host/fs.scan, WIT:ilium:host/fs.take-entries
#[test]
fn fs_scan_returns_the_matching_entries_with_relative_paths() {
    let dir = tempfile::tempdir().unwrap();
    scan_tree(dir.path());
    let mut fixture = rw_fixture(dir.path());
    let request = expect_ok(
        fixture
            .fs()
            .scan(project_path("src"), "**/*.rs".to_owned(), 100),
    );
    assert!(is_ok(&complete(&mut fixture, request)));
    let mut paths: Vec<String> = fixture
        .fs()
        .take_entries(request)
        .expect("entries")
        .into_iter()
        .map(|entry| entry.relative_path)
        .collect();
    paths.sort();
    assert_eq!(paths, ["a.rs", "deep/b.rs"]);
}

/// REQ: WIT:ilium:host/fs.take-entries
#[test]
fn fs_take_entries_returns_the_result_once() {
    let dir = tempfile::tempdir().unwrap();
    scan_tree(dir.path());
    let mut fixture = rw_fixture(dir.path());
    let request = expect_ok(
        fixture
            .fs()
            .scan(project_path("src"), "*.txt".to_owned(), 10),
    );
    complete(&mut fixture, request);
    assert!(fixture.fs().take_entries(request).is_some());
    assert!(fixture.fs().take_entries(request).is_none());
}

/// REQ: WIT:ilium:host/fs.scan
#[test]
fn fs_scan_more_matches_than_max_entries_fails_quota_exceeded() {
    let dir = tempfile::tempdir().unwrap();
    scan_tree(dir.path());
    let mut fixture = rw_fixture(dir.path());
    let request = expect_ok(fixture.fs().scan(project_path("src"), "**/*".to_owned(), 2));
    assert_eq!(
        failed_code(&complete(&mut fixture, request)),
        Some(ErrorCode::QuotaExceeded)
    );
}

/// REQ: WIT:ilium:host/fs.scan
#[test]
fn fs_scan_exactly_max_entries_matches_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    scan_tree(dir.path());
    let mut fixture = rw_fixture(dir.path());
    let request = expect_ok(
        fixture
            .fs()
            .scan(project_path("src"), "**/*.rs".to_owned(), 2),
    );
    assert!(is_ok(&complete(&mut fixture, request)));
}

/// REQ: WIT:ilium:host/fs.scan
#[test]
fn fs_scan_max_entries_65536_accepted_and_65537_invalid_argument() {
    let dir = tempfile::tempdir().unwrap();
    scan_tree(dir.path());
    let mut fixture = rw_fixture(dir.path());
    expect_ok(
        fixture
            .fs()
            .scan(project_path("src"), "*".to_owned(), 65_536),
    );
    assert_eq!(
        expect_code(
            fixture
                .fs()
                .scan(project_path("src"), "*".to_owned(), 65_537)
        ),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/fs.scan
#[test]
fn fs_scan_without_the_read_grant_not_permitted() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture_in(dir.path(), Tier::T2, &[]);
    assert_eq!(
        expect_code(fixture.fs().scan(project_path("."), "*".to_owned(), 10)),
        ErrorCode::NotPermitted
    );
}

// ---- write -----------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/fs.write
#[test]
fn fs_write_replace_creates_and_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    write_file(&mut fixture, "a.txt", b"first");
    write_file(&mut fixture, "a.txt", b"2");
    assert_eq!(std::fs::read(dir.path().join("a.txt")).unwrap(), b"2");
}

/// REQ: WIT:ilium:host/fs.write
#[test]
fn fs_write_append_adds_to_the_end() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    write_file(&mut fixture, "a.txt", b"ab");
    let request = expect_ok(fixture.fs().write(
        project_path("a.txt"),
        b"cd".to_vec(),
        WriteMode::Append,
    ));
    complete(&mut fixture, request);
    assert_eq!(std::fs::read(dir.path().join("a.txt")).unwrap(), b"abcd");
}

/// REQ: WIT:ilium:host/fs.write
#[test]
fn fs_write_create_new_onto_an_existing_file_conflict() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"old").unwrap();
    let mut fixture = rw_fixture(dir.path());
    match fixture
        .fs()
        .write(project_path("a.txt"), b"new".to_vec(), WriteMode::CreateNew)
    {
        Err(error) => assert_eq!(error.code, ErrorCode::Conflict),
        Ok(request) => assert_eq!(
            failed_code(&complete(&mut fixture, request)),
            Some(ErrorCode::Conflict)
        ),
    }
    assert_eq!(std::fs::read(dir.path().join("a.txt")).unwrap(), b"old");
}

/// REQ: WIT:ilium:host/fs.write
#[test]
fn fs_write_missing_parent_directory_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    match fixture.fs().write(
        project_path("no/such/a.txt"),
        b"x".to_vec(),
        WriteMode::Replace,
    ) {
        Err(error) => assert_eq!(error.code, ErrorCode::NotFound),
        Ok(request) => assert_eq!(
            failed_code(&complete(&mut fixture, request)),
            Some(ErrorCode::NotFound)
        ),
    }
}

/// REQ: WIT:ilium:host/fs.write
#[test]
fn fs_write_over_the_write_quota_invalid_argument_and_exactly_at_it_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = builder_in(dir.path(), Tier::T2, &RW)
        .limit("fs_write_mib", 1)
        .build();
    let at_limit = expect_ok(fixture.fs().write(
        project_path("fit.bin"),
        vec![0u8; MIB],
        WriteMode::Replace,
    ));
    assert!(is_ok(&complete(&mut fixture, at_limit)));
    let code = expect_code(fixture.fs().write(
        project_path("big.bin"),
        vec![0u8; MIB + 1],
        WriteMode::Replace,
    ));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/fs.write
#[test]
fn fs_write_to_a_read_only_scope_not_permitted() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture_in(dir.path(), Tier::T2, &["fs.read:project"]);
    let code = expect_code(fixture.fs().write(
        project_path("a.txt"),
        b"x".to_vec(),
        WriteMode::Replace,
    ));
    assert_eq!(code, ErrorCode::NotPermitted);
    assert!(!dir.path().join("a.txt").exists());
}

/// REQ: WIT:ilium:host/fs.write
#[test]
fn fs_write_path_escaping_the_scope_not_permitted() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    let code = expect_code(fixture.fs().write(
        project_path("../escape.txt"),
        b"x".to_vec(),
        WriteMode::Replace,
    ));
    assert_eq!(code, ErrorCode::NotPermitted);
}

// ---- create-dir, remove, rename --------------------------------------------------------------

/// REQ: WIT:ilium:host/fs.create-dir
#[test]
fn fs_create_dir_makes_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    expect_ok(fixture.fs().create_dir(project_path("made")));
    assert!(dir.path().join("made").is_dir());
}

/// REQ: WIT:ilium:host/fs.create-dir
#[test]
fn fs_create_dir_missing_parent_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    assert_eq!(
        expect_code(fixture.fs().create_dir(project_path("a/b"))),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/fs.create-dir
#[test]
fn fs_create_dir_read_only_scope_not_permitted() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture_in(dir.path(), Tier::T2, &["fs.read:project"]);
    assert_eq!(
        expect_code(fixture.fs().create_dir(project_path("x"))),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/fs.remove
#[test]
fn fs_remove_deletes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
    let mut fixture = rw_fixture(dir.path());
    let request = expect_ok(fixture.fs().remove(project_path("a.txt")));
    assert!(is_ok(&complete(&mut fixture, request)));
    assert!(!dir.path().join("a.txt").exists());
}

/// REQ: WIT:ilium:host/fs.remove
#[test]
fn fs_remove_missing_path_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    match fixture.fs().remove(project_path("missing")) {
        Err(error) => assert_eq!(error.code, ErrorCode::NotFound),
        Ok(request) => assert_eq!(
            failed_code(&complete(&mut fixture, request)),
            Some(ErrorCode::NotFound)
        ),
    }
}

/// REQ: WIT:ilium:host/fs.remove
#[test]
fn fs_remove_read_only_scope_not_permitted() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
    let mut fixture = fixture_in(dir.path(), Tier::T2, &["fs.read:project"]);
    assert_eq!(
        expect_code(fixture.fs().remove(project_path("a.txt"))),
        ErrorCode::NotPermitted
    );
    assert!(dir.path().join("a.txt").exists());
}

/// REQ: WIT:ilium:host/fs.rename
#[test]
fn fs_rename_moves_the_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
    let mut fixture = rw_fixture(dir.path());
    let request = expect_ok(
        fixture
            .fs()
            .rename(project_path("a.txt"), project_path("b.txt")),
    );
    assert!(is_ok(&complete(&mut fixture, request)));
    assert!(dir.path().join("b.txt").exists() && !dir.path().join("a.txt").exists());
}

/// REQ: WIT:ilium:host/fs.rename
#[test]
fn fs_rename_across_scopes_invalid_argument() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
    let mut fixture = fixture_in(
        dir.path(),
        Tier::T2,
        &["fs.read:project", "fs.write:project", "fs.write:data"],
    );
    let code = expect_code(
        fixture
            .fs()
            .rename(project_path("a.txt"), scoped_path("data", "a.txt")),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/fs.rename
#[test]
fn fs_rename_missing_source_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    match fixture
        .fs()
        .rename(project_path("missing"), project_path("b"))
    {
        Err(error) => assert_eq!(error.code, ErrorCode::NotFound),
        Ok(request) => assert_eq!(
            failed_code(&complete(&mut fixture, request)),
            Some(ErrorCode::NotFound)
        ),
    }
}

/// REQ: WIT:ilium:host/fs.rename
#[test]
fn fs_rename_target_escaping_the_scope_not_permitted() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
    let mut fixture = rw_fixture(dir.path());
    assert_eq!(
        expect_code(
            fixture
                .fs()
                .rename(project_path("a.txt"), project_path("../out.txt"))
        ),
        ErrorCode::NotPermitted
    );
}

// ---- watch, unwatch --------------------------------------------------------------------------

/// REQ: WIT:ilium:host/fs.watch, WIT:ilium:host/fs.unwatch
#[test]
fn fs_watch_delivers_a_change_event_until_unwatch() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture_in(
        dir.path(),
        Tier::T2,
        &["fs.read:project", "fs.write:project", "fs.watch:project"],
    );
    let watch = expect_ok(fixture.fs().watch(project_path("."), true));
    write_file(&mut fixture, "seen.txt", b"1");
    assert!(
        fixture.wait_fs_event(watch, WAIT),
        "an event arrives for the watch"
    );
    fixture.fs().unwatch(watch);
    write_file(&mut fixture, "unseen.txt", b"1");
    assert!(!fixture.wait_fs_event(watch, std::time::Duration::from_millis(500)));
}

/// REQ: WIT:ilium:host/fs.watch
#[test]
fn fs_watch_without_the_watch_grant_not_permitted() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    assert_eq!(
        expect_code(fixture.fs().watch(project_path("."), false)),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/fs.watch
#[test]
fn fs_watch_64_watches_accepted_and_the_65th_quota_exceeded() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture_in(
        dir.path(),
        Tier::T2,
        &["fs.read:project", "fs.watch:project"],
    );
    for _ in 0..64 {
        expect_ok(fixture.fs().watch(project_path("."), false));
    }
    assert_eq!(
        expect_code(fixture.fs().watch(project_path("."), false)),
        ErrorCode::QuotaExceeded
    );
}

/// REQ: WIT:ilium:host/fs.unwatch
#[test]
fn fs_unwatch_frees_a_slot_for_a_new_watch() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture_in(
        dir.path(),
        Tier::T2,
        &["fs.read:project", "fs.watch:project"],
    );
    let mut watches = Vec::new();
    for _ in 0..64 {
        watches.push(expect_ok(fixture.fs().watch(project_path("."), false)));
    }
    fixture.fs().unwatch(watches[0]);
    expect_ok(fixture.fs().watch(project_path("."), false));
}

/// REQ: WIT:ilium:host/fs.unwatch
#[test]
fn fs_unwatch_of_an_unknown_watch_is_harmless() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw_fixture(dir.path());
    fixture
        .fs()
        .unwatch(ilium_plugin_api::generated::types::WatchId(424_242));
    assert!(fixture
        .fs()
        .roots()
        .iter()
        .any(|root| root.scope == "project"));
}

/// REQ: WIT:ilium:host/fs.watch
#[test]
fn fs_watch_missing_path_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture_in(
        dir.path(),
        Tier::T2,
        &["fs.read:project", "fs.watch:project"],
    );
    assert_eq!(
        expect_code(fixture.fs().watch(project_path("missing"), false)),
        ErrorCode::NotFound
    );
}

// ---- pending request cap ---------------------------------------------------------------------

/// REQ: WIT:ilium:host/fs.read-all
#[test]
fn fs_read_all_65th_pending_request_busy() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
    let mut fixture = rw_fixture(dir.path());
    fixture.hold_async_completions(true);
    for _ in 0..64 {
        expect_ok(fixture.fs().read_all(project_path("a.txt")));
    }
    assert_eq!(
        expect_code(fixture.fs().read_all(project_path("a.txt"))),
        ErrorCode::Busy
    );
}

proptest! {
    // REQ: WIT:ilium:host/fs.write, WIT:ilium:host/fs.read
    #[test]
    fn fs_write_then_read_round_trips_the_bytes(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let dir = tempfile::tempdir().unwrap();
        let mut fixture = rw_fixture(dir.path());
        write_file(&mut fixture, "round.bin", &data);
        let back = expect_ok(fixture.fs().read(project_path("round.bin"), 0, 4096));
        prop_assert_eq!(back, data);
    }

    // REQ: WIT:ilium:host/fs.metadata
    #[test]
    fn fs_metadata_length_matches_the_written_size(length in 0usize..4096) {
        let dir = tempfile::tempdir().unwrap();
        let mut fixture = rw_fixture(dir.path());
        write_file(&mut fixture, "size.bin", &vec![0u8; length]);
        prop_assert_eq!(expect_ok(fixture.fs().metadata(project_path("size.bin"))).length, length as u64);
    }
}
