//! api-fs-atomic-and-tail.md: locks, the file picker, revisioned reads, atomic writes and tail.

use std::time::Duration;

use ilium_plugin_api::generated::host::{PickKind, PickSpec, WriteMode};
use ilium_plugin_api::generated::types::{ErrorCode, RequestId};
use ilium_plugin_api::Tier;
use ilium_plugin_host::testing::imports_fixture::ImportsFixture;
use proptest::prelude::*;

use crate::support::*;

const RW: [&str; 2] = ["fs.read:project", "fs.write:project"];

/// The code of a call that may refuse at once or fail at completion.
fn code_either(
    fixture: &mut ImportsFixture,
    result: Result<RequestId, ilium_plugin_api::generated::types::PluginError>,
) -> Option<ErrorCode> {
    match result {
        Err(error) => Some(error.code),
        Ok(request) => outcome_code(&complete(fixture, request)),
    }
}

fn pick_spec(kind: PickKind) -> PickSpec {
    PickSpec {
        kind,
        title: "Choose".to_owned(),
        extensions: vec!["txt".to_owned()],
    }
}

fn rw(dir: &std::path::Path) -> ImportsFixture {
    fixture_in(dir, Tier::T2, &RW)
}

// ---- lock, unlock, take-lock -----------------------------------------------------------------

/// REQ: WIT:ilium:host/fs.lock, WIT:ilium:host/fs.take-lock
#[test]
fn fs_lock_grants_a_lock_id_that_take_lock_returns_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
    let mut fixture = rw(dir.path());
    let request = expect_ok(fixture.fs().lock(project_path("a.txt"), true, 1000));
    assert!(is_ok(&complete(&mut fixture, request)));
    assert!(fixture.fs().take_lock(request).is_some());
    assert!(fixture.fs().take_lock(request).is_none());
}

/// REQ: WIT:ilium:host/fs.lock
#[test]
fn fs_lock_zero_timeout_on_a_held_lock_conflict() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
    let mut fixture = rw(dir.path());
    let first = expect_ok(fixture.fs().lock(project_path("a.txt"), true, 1000));
    complete(&mut fixture, first);
    let second = fixture.fs().lock(project_path("a.txt"), true, 0);
    assert_eq!(code_either(&mut fixture, second), Some(ErrorCode::Conflict));
}

/// REQ: WIT:ilium:host/fs.lock
#[test]
fn fs_lock_wait_longer_than_the_timeout_fails_timed_out() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
    let mut fixture = rw(dir.path());
    let first = expect_ok(fixture.fs().lock(project_path("a.txt"), true, 1000));
    complete(&mut fixture, first);
    let second = expect_ok(fixture.fs().lock(project_path("a.txt"), true, 200));
    fixture.advance(Duration::from_millis(250));
    assert_eq!(
        outcome_code(&complete(&mut fixture, second)),
        Some(ErrorCode::TimedOut)
    );
}

/// REQ: WIT:ilium:host/fs.lock
#[test]
fn fs_lock_two_shared_locks_coexist() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
    let mut fixture = rw(dir.path());
    let first = expect_ok(fixture.fs().lock(project_path("a.txt"), false, 1000));
    assert!(is_ok(&complete(&mut fixture, first)));
    let second = expect_ok(fixture.fs().lock(project_path("a.txt"), false, 0));
    assert!(is_ok(&complete(&mut fixture, second)));
}

/// REQ: WIT:ilium:host/fs.lock
#[test]
fn fs_lock_exclusive_blocks_a_shared_request() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
    let mut fixture = rw(dir.path());
    let first = expect_ok(fixture.fs().lock(project_path("a.txt"), true, 1000));
    complete(&mut fixture, first);
    let second = fixture.fs().lock(project_path("a.txt"), false, 0);
    assert_eq!(code_either(&mut fixture, second), Some(ErrorCode::Conflict));
}

/// REQ: WIT:ilium:host/fs.unlock
#[test]
fn fs_unlock_releases_the_lock_for_the_next_holder() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
    let mut fixture = rw(dir.path());
    let first = expect_ok(fixture.fs().lock(project_path("a.txt"), true, 1000));
    complete(&mut fixture, first);
    let lock = fixture.fs().take_lock(first).expect("lock id");
    fixture.fs().unlock(lock);
    let second = expect_ok(fixture.fs().lock(project_path("a.txt"), true, 0));
    assert!(is_ok(&complete(&mut fixture, second)));
}

/// REQ: WIT:ilium:host/fs.unlock
#[test]
fn fs_unlock_of_an_unknown_lock_is_harmless() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw(dir.path());
    fixture
        .fs()
        .unlock(ilium_plugin_api::generated::types::LockId(31_337));
    assert!(!fixture.fs().roots().is_empty());
}

/// REQ: WIT:ilium:host/fs.lock
#[test]
fn fs_lock_without_the_write_grant_not_permitted() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture_in(dir.path(), Tier::T2, &["fs.read:project"]);
    assert_eq!(
        expect_code(fixture.fs().lock(project_path("a.txt"), true, 0)),
        ErrorCode::NotPermitted
    );
}

// ---- pick, take-picked -----------------------------------------------------------------------

/// REQ: WIT:ilium:host/fs.pick, WIT:ilium:host/fs.take-picked
#[test]
fn fs_pick_returns_the_path_the_user_chose_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw(dir.path());
    let request = expect_ok(fixture.fs().pick(pick_spec(PickKind::OpenFile)));
    fixture.picker_choose(vec![project_path("chosen.txt")]);
    assert!(is_ok(&complete(&mut fixture, request)));
    assert_eq!(
        fixture.fs().take_picked(request),
        Some(project_path("chosen.txt"))
    );
    assert!(fixture.fs().take_picked(request).is_none());
}

/// REQ: WIT:ilium:host/fs.pick
#[test]
fn fs_pick_cancelled_by_the_user_completes_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw(dir.path());
    let request = expect_ok(fixture.fs().pick(pick_spec(PickKind::SaveFile)));
    fixture.picker_cancel();
    assert_eq!(
        outcome_code(&complete(&mut fixture, request)),
        Some(ErrorCode::Cancelled)
    );
}

/// REQ: WIT:ilium:host/fs.pick
#[test]
fn fs_pick_while_a_picker_is_already_shown_busy() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw(dir.path());
    expect_ok(fixture.fs().pick(pick_spec(PickKind::OpenFile)));
    assert_eq!(
        expect_code(fixture.fs().pick(pick_spec(PickKind::OpenDirectory))),
        ErrorCode::Busy
    );
}

/// REQ: WIT:ilium:host/fs.pick
#[test]
fn fs_pick_picked_path_is_granted_for_later_use() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("chosen.txt"), b"abc").unwrap();
    let mut fixture = fixture_in(dir.path(), Tier::T2, &[]);
    let request = expect_ok(fixture.fs().pick(pick_spec(PickKind::OpenFile)));
    fixture.picker_choose(vec![project_path("chosen.txt")]);
    complete(&mut fixture, request);
    let picked = fixture.fs().take_picked(request).expect("picked");
    assert_eq!(expect_ok(fixture.fs().read(picked, 0, 10)), b"abc");
}

/// REQ: WIT:ilium:host/fs.take-picked
#[test]
fn fs_take_picked_for_an_unknown_request_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw(dir.path());
    assert!(fixture.fs().take_picked(RequestId(777_777)).is_none());
}

/// REQ: WIT:ilium:host/fs.pick
#[test]
fn fs_pick_empty_title_or_more_than_64_extensions_invalid_argument() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw(dir.path());
    let mut spec = pick_spec(PickKind::OpenFile);
    spec.extensions = (0..65).map(|index| format!("e{index}")).collect();
    assert_eq!(
        expect_code(fixture.fs().pick(spec)),
        ErrorCode::InvalidArgument
    );
}

// ---- read-with-revision, take-revisioned ----------------------------------------------------

/// REQ: WIT:ilium:host/fs.read-with-revision, WIT:ilium:host/fs.take-revisioned
#[test]
fn fs_read_with_revision_returns_bytes_and_a_revision_token_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"content").unwrap();
    let mut fixture = rw(dir.path());
    let request = expect_ok(fixture.fs().read_with_revision(project_path("a.txt")));
    assert!(is_ok(&complete(&mut fixture, request)));
    let revisioned = fixture.fs().take_revisioned(request).expect("revisioned");
    assert_eq!(revisioned.bytes, b"content");
    assert!(!revisioned.revision.is_empty());
    assert!(fixture.fs().take_revisioned(request).is_none());
}

/// REQ: WIT:ilium:host/fs.read-with-revision
#[test]
fn fs_read_with_revision_revision_changes_when_the_file_changes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"one").unwrap();
    let mut fixture = rw(dir.path());
    let first = expect_ok(fixture.fs().read_with_revision(project_path("a.txt")));
    complete(&mut fixture, first);
    let before = fixture.fs().take_revisioned(first).unwrap().revision;
    std::fs::write(dir.path().join("a.txt"), b"two!").unwrap();
    let second = expect_ok(fixture.fs().read_with_revision(project_path("a.txt")));
    complete(&mut fixture, second);
    let after = fixture.fs().take_revisioned(second).unwrap().revision;
    assert_ne!(before, after);
}

/// REQ: WIT:ilium:host/fs.read-with-revision
#[test]
fn fs_read_with_revision_file_over_the_read_all_quota_fails_quota_exceeded() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("big.bin"), vec![0u8; 3 * MIB]).unwrap();
    let mut fixture = builder_in(dir.path(), Tier::T2, &RW)
        .limit("fs_read_all_mib", 2)
        .build();
    let result = fixture.fs().read_with_revision(project_path("big.bin"));
    assert_eq!(
        code_either(&mut fixture, result),
        Some(ErrorCode::QuotaExceeded)
    );
}

/// REQ: WIT:ilium:host/fs.read-with-revision
#[test]
fn fs_read_with_revision_missing_file_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw(dir.path());
    let result = fixture.fs().read_with_revision(project_path("missing"));
    assert_eq!(code_either(&mut fixture, result), Some(ErrorCode::NotFound));
}

// ---- write-atomic, take-written --------------------------------------------------------------

/// REQ: WIT:ilium:host/fs.write-atomic, WIT:ilium:host/fs.take-written
#[test]
fn fs_write_atomic_with_the_current_revision_replaces_the_file_and_returns_a_new_one() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"one").unwrap();
    let mut fixture = rw(dir.path());
    let read = expect_ok(fixture.fs().read_with_revision(project_path("a.txt")));
    complete(&mut fixture, read);
    let revision = fixture.fs().take_revisioned(read).unwrap().revision;
    let write = expect_ok(fixture.fs().write_atomic(
        project_path("a.txt"),
        b"two".to_vec(),
        Some(revision.clone()),
    ));
    assert!(is_ok(&complete(&mut fixture, write)));
    let new_revision = fixture.fs().take_written(write).expect("new revision");
    assert_ne!(new_revision, revision);
    assert_eq!(std::fs::read(dir.path().join("a.txt")).unwrap(), b"two");
    assert!(fixture.fs().take_written(write).is_none());
}

/// REQ: WIT:ilium:host/fs.write-atomic
#[test]
fn fs_write_atomic_with_a_stale_revision_conflict_and_the_file_is_untouched() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"one").unwrap();
    let mut fixture = rw(dir.path());
    let read = expect_ok(fixture.fs().read_with_revision(project_path("a.txt")));
    complete(&mut fixture, read);
    let revision = fixture.fs().take_revisioned(read).unwrap().revision;
    std::fs::write(dir.path().join("a.txt"), b"changed elsewhere").unwrap();
    let result = fixture
        .fs()
        .write_atomic(project_path("a.txt"), b"mine".to_vec(), Some(revision));
    assert_eq!(code_either(&mut fixture, result), Some(ErrorCode::Conflict));
    assert_eq!(
        std::fs::read(dir.path().join("a.txt")).unwrap(),
        b"changed elsewhere"
    );
}

/// REQ: WIT:ilium:host/fs.write-atomic
#[test]
fn fs_write_atomic_without_an_expected_revision_creates_or_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw(dir.path());
    let request = expect_ok(fixture.fs().write_atomic(
        project_path("new.txt"),
        b"hi".to_vec(),
        None,
    ));
    assert!(is_ok(&complete(&mut fixture, request)));
    assert_eq!(std::fs::read(dir.path().join("new.txt")).unwrap(), b"hi");
}

/// REQ: WIT:ilium:host/fs.write-atomic
#[test]
fn fs_write_atomic_missing_parent_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw(dir.path());
    let result = fixture
        .fs()
        .write_atomic(project_path("no/dir/a.txt"), b"x".to_vec(), None);
    assert_eq!(code_either(&mut fixture, result), Some(ErrorCode::NotFound));
}

/// REQ: WIT:ilium:host/fs.write-atomic
#[test]
fn fs_write_atomic_over_the_write_quota_invalid_argument() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = builder_in(dir.path(), Tier::T2, &RW)
        .limit("fs_write_mib", 1)
        .build();
    let code = expect_code(fixture.fs().write_atomic(
        project_path("big"),
        vec![0u8; MIB + 1],
        None,
    ));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/fs.write-atomic
#[test]
fn fs_write_atomic_read_only_scope_not_permitted() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture_in(dir.path(), Tier::T2, &["fs.read:project"]);
    assert_eq!(
        expect_code(
            fixture
                .fs()
                .write_atomic(project_path("a"), b"x".to_vec(), None)
        ),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/fs.take-written
#[test]
fn fs_take_written_for_an_unknown_request_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw(dir.path());
    assert!(fixture.fs().take_written(RequestId(1_234_567)).is_none());
}

// ---- tail ------------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/fs.tail
#[test]
fn fs_tail_from_cursor_zero_returns_the_content_and_the_next_cursor() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("log.txt"), b"line one\n").unwrap();
    let mut fixture = rw(dir.path());
    let chunk = expect_ok(fixture.fs().tail(project_path("log.txt"), 0, 4096));
    assert_eq!(chunk.bytes, b"line one\n");
    assert_eq!(chunk.cursor, 9);
    assert!(!chunk.truncated);
}

/// REQ: WIT:ilium:host/fs.tail
#[test]
fn fs_tail_with_the_returned_cursor_yields_only_appended_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.txt");
    std::fs::write(&path, b"one\n").unwrap();
    let mut fixture = rw(dir.path());
    let first = expect_ok(fixture.fs().tail(project_path("log.txt"), 0, 4096));
    std::fs::write(&path, b"one\ntwo\n").unwrap();
    let second = expect_ok(
        fixture
            .fs()
            .tail(project_path("log.txt"), first.cursor, 4096),
    );
    assert_eq!(second.bytes, b"two\n");
    assert_eq!(second.cursor, 8);
}

/// REQ: WIT:ilium:host/fs.tail
#[test]
fn fs_tail_at_the_end_of_the_file_returns_nothing_new() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("log.txt"), b"abc").unwrap();
    let mut fixture = rw(dir.path());
    let chunk = expect_ok(fixture.fs().tail(project_path("log.txt"), 3, 4096));
    assert!(chunk.bytes.is_empty());
    assert_eq!(chunk.cursor, 3);
}

/// REQ: WIT:ilium:host/fs.tail
#[test]
fn fs_tail_file_shorter_than_the_cursor_is_reported_truncated() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("log.txt"), b"ab").unwrap();
    let mut fixture = rw(dir.path());
    let chunk = expect_ok(fixture.fs().tail(project_path("log.txt"), 100, 4096));
    assert!(chunk.truncated);
    assert!(chunk.cursor <= 2);
}

/// REQ: WIT:ilium:host/fs.tail
#[test]
fn fs_tail_returns_at_most_1_mib_per_call() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("big.log"), vec![b'x'; 2 * MIB]).unwrap();
    let mut fixture = rw(dir.path());
    let chunk = expect_ok(
        fixture
            .fs()
            .tail(project_path("big.log"), 0, 2 * MIB as u32),
    );
    assert_eq!(chunk.bytes.len(), MIB);
    assert_eq!(chunk.cursor, MIB as u64);
}

/// REQ: WIT:ilium:host/fs.tail
#[test]
fn fs_tail_max_bytes_limits_the_chunk() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("log.txt"), b"0123456789").unwrap();
    let mut fixture = rw(dir.path());
    let chunk = expect_ok(fixture.fs().tail(project_path("log.txt"), 2, 5));
    assert_eq!(chunk.bytes, b"23456");
    assert_eq!(chunk.cursor, 7);
}

/// REQ: WIT:ilium:host/fs.tail
#[test]
fn fs_tail_missing_file_not_found_and_no_grant_not_permitted() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = rw(dir.path());
    assert_eq!(
        expect_code(fixture.fs().tail(project_path("missing"), 0, 10)),
        ErrorCode::NotFound
    );
    let mut bare = fixture_in(dir.path(), Tier::T2, &[]);
    assert_eq!(
        expect_code(bare.fs().tail(project_path("missing"), 0, 10)),
        ErrorCode::NotPermitted
    );
}

proptest! {
    // REQ: WIT:ilium:host/fs.tail
    #[test]
    fn fs_tail_chunks_concatenate_to_the_whole_file(
        data in proptest::collection::vec(any::<u8>(), 0..1500),
        step in 1u32..400,
    ) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("t.bin"), &data).unwrap();
        let mut fixture = rw(dir.path());
        let mut cursor = 0u64;
        let mut collected = Vec::new();
        for _ in 0..(data.len() / step as usize + 3) {
            let chunk = expect_ok(fixture.fs().tail(project_path("t.bin"), cursor, step));
            collected.extend_from_slice(&chunk.bytes);
            cursor = chunk.cursor;
        }
        prop_assert_eq!(collected, data);
    }
}

#[allow(dead_code)]
fn _write_mode_is_used(_: WriteMode) {}
