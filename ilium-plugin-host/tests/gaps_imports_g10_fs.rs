//! G10 `fs`: a per-instance import over real folders. The `project` scope opens only with an
//! `fs.*:project` grant, two instances never share scopes or request ids, a watch reports
//! `fs-changed` through the runtime queue and stops when the import is dropped (api-fs.md).

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ilium_plugin_api::generated::host::fs::{HostFs, WriteMode};
use ilium_plugin_api::generated::types::errors::ErrorCode;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::generated::types::paths::FsPath;
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
use ilium_plugin_api::grants::Grant;
use ilium_plugin_host::imports::env::ImportEnv;
use ilium_plugin_host::imports::fs::FsImport;

const PATIENCE: Duration = Duration::from_secs(20);

fn path(scope: &str, relative: &str) -> FsPath {
    FsPath {
        scope: scope.to_owned(),
        path: relative.to_owned(),
    }
}

/// An import whose three private scopes live under `base/<name>` and whose project scope is
/// `project` with the given grants.
fn import(name: &str, base: &Path, project: &Path, grants: &[&str]) -> (ImportEnv, FsImport) {
    let env = ImportEnv::unrestricted()
        .with_identity(&format!("test.{name}"), 1)
        .with_grants(grants.iter().map(|grant| (*grant).to_owned()).collect());
    let mut fs_import = FsImport::new(env.clone());
    let root = base.join(name);
    fs_import
        .configure_scope_roots(root.join("package"), root.join("data"), root.join("cache"))
        .expect("scope folders");
    let parsed: Vec<Grant> = grants
        .iter()
        .filter_map(|grant| Grant::parse(grant).ok())
        .collect();
    fs_import
        .configure_project_scope(project.to_path_buf(), Arc::new(Mutex::new(parsed)))
        .expect("project scope");
    (env, fs_import)
}

fn collect_until(env: &ImportEnv, done: impl Fn(&[Event]) -> bool) -> Vec<Event> {
    let started = Instant::now();
    let mut seen: Vec<Event> = Vec::new();
    while started.elapsed() < PATIENCE {
        seen.extend(env.events().drain().into_iter().map(|queued| queued.event));
        if done(&seen) {
            return seen;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    seen
}

fn completed(events: &[Event], request: RequestId) -> Option<Outcome> {
    events.iter().find_map(|event| match event {
        Event::RequestCompleted(result) if result.request == RequestRef::Call(request) => {
            Some(result.outcome.clone())
        }
        _ => None,
    })
}

/// REQ: CAP-018
#[test]
fn the_project_scope_is_closed_without_its_grant_and_open_with_it() {
    let scratch = tempfile::tempdir().expect("scratch");
    let project = scratch.path().join("project");
    std::fs::create_dir_all(&project).expect("project folder");
    std::fs::write(project.join("note.txt"), b"project note").expect("note");
    let (_closed_env, mut closed) = import("closed", scratch.path(), &project, &[]);
    assert!(closed.roots().iter().all(|root| root.scope != "project"));
    let refused = closed.read(path("project", "note.txt"), 0, 64);
    assert_eq!(refused.expect_err("no grant").code, ErrorCode::NotPermitted);
    let (_open_env, mut open) = import("open", scratch.path(), &project, &["fs.read:project"]);
    assert!(open.roots().iter().any(|root| root.scope == "project"));
    let data = open
        .read(path("project", "note.txt"), 0, 64)
        .expect("granted read");
    assert_eq!(data, b"project note");
    let write = open.write(
        path("project", "new.txt"),
        b"x".to_vec(),
        WriteMode::CreateNew,
    );
    assert_eq!(
        write.expect_err("read only grant").code,
        ErrorCode::NotPermitted
    );
}

/// REQ: CAP-018
#[test]
fn a_path_that_leaves_its_scope_is_refused() {
    let scratch = tempfile::tempdir().expect("scratch");
    let project = scratch.path().join("project");
    std::fs::create_dir_all(&project).expect("project folder");
    let (_env, mut fs_import) = import("escape", scratch.path(), &project, &["fs.read:project"]);
    let refused = fs_import.read(path("data", "../../../etc/passwd"), 0, 64);
    assert!(
        matches!(
            refused.map_err(|error| error.code),
            Err(ErrorCode::NotPermitted | ErrorCode::InvalidArgument)
        ),
        "a parent segment must not leave the scope"
    );
}

/// REQ: API-062
#[test]
fn a_write_completes_through_an_event_and_two_instances_keep_their_own_data() {
    let scratch = tempfile::tempdir().expect("scratch");
    let project = scratch.path().join("project");
    std::fs::create_dir_all(&project).expect("project folder");
    let (first_env, mut first) = import("first", scratch.path(), &project, &[]);
    let (_second_env, mut second) = import("second", scratch.path(), &project, &[]);
    let request = first
        .write(
            path("data", "state.txt"),
            b"first data".to_vec(),
            WriteMode::Replace,
        )
        .expect("write starts");
    let events = collect_until(&first_env, |seen| completed(seen, request).is_some());
    assert_eq!(completed(&events, request), Some(Outcome::Ok), "{events:?}");
    assert_eq!(first.completed_outcome(request), Some(Outcome::Ok));
    assert_eq!(
        first
            .read(path("data", "state.txt"), 0, 64)
            .expect("read back"),
        b"first data"
    );
    assert_eq!(
        second
            .read(path("data", "state.txt"), 0, 64)
            .expect_err("not shared")
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        second.completed_outcome(request),
        None,
        "the id means nothing there"
    );
}

/// REQ: API-062
#[test]
fn held_completions_finish_when_they_are_released() {
    let scratch = tempfile::tempdir().expect("scratch");
    let project = scratch.path().join("project");
    std::fs::create_dir_all(&project).expect("project folder");
    let (env, mut fs_import) = import("held", scratch.path(), &project, &[]);
    fs_import.hold_completions(true);
    let request = fs_import
        .write(
            path("data", "late.txt"),
            b"late".to_vec(),
            WriteMode::Replace,
        )
        .expect("write starts");
    assert_eq!(fs_import.completed_outcome(request), None, "held back");
    assert!(env.events().is_empty(), "no event while held");
    fs_import.hold_completions(false);
    let events = collect_until(&env, |seen| completed(seen, request).is_some());
    assert_eq!(completed(&events, request), Some(Outcome::Ok));
}

/// REQ: API-005
#[test]
fn a_watch_reports_changes_as_fs_changed_and_stops_with_the_import() {
    let scratch = tempfile::tempdir().expect("scratch");
    let project = scratch.path().join("project");
    std::fs::create_dir_all(&project).expect("project folder");
    let (env, mut fs_import) = import("watch", scratch.path(), &project, &[]);
    let watch = fs_import
        .watch(path("data", ""), true)
        .expect("watch starts");
    assert_eq!(fs_import.watch_count(), 1);
    assert!(
        env.events().workers() >= 1,
        "a watch keeps the runtime polling"
    );
    let data_folder = scratch.path().join("watch").join("data");
    std::fs::write(data_folder.join("changed.txt"), b"hello").expect("change");
    let events = collect_until(&env, |seen| {
        seen.iter().any(|event| {
            matches!(event, Event::FsChanged(changed)
                if changed.watch == watch && changed.paths.iter().any(|name| name == "changed.txt"))
        })
    });
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::FsChanged(changed)
            if changed.watch == watch && changed.paths.iter().any(|name| name == "changed.txt"))),
        "{events:?}"
    );
    fs_import.unwatch(watch);
    assert_eq!(fs_import.watch_count(), 0);
    let (second_env, mut second) = import("watch-two", scratch.path(), &project, &[]);
    second.watch(path("data", ""), false).expect("second watch");
    drop(second);
    assert_eq!(
        second_env.events().workers(),
        0,
        "dropping the import joined its watch worker"
    );
}

/// REQ: API-005
#[test]
fn the_package_scope_cannot_be_watched_and_a_missing_folder_cannot_either() {
    let scratch = tempfile::tempdir().expect("scratch");
    let project = scratch.path().join("project");
    std::fs::create_dir_all(&project).expect("project folder");
    let (_env, mut fs_import) = import("refuse", scratch.path(), &project, &["fs.watch:project"]);
    assert_eq!(
        fs_import
            .watch(path("package", ""), false)
            .expect_err("package")
            .code,
        ErrorCode::NotPermitted
    );
    assert_eq!(
        fs_import
            .watch(path("data", "absent"), false)
            .expect_err("absent")
            .code,
        ErrorCode::NotFound
    );
    let granted = fs_import.watch(path("project", ""), false);
    assert!(
        granted.is_ok(),
        "fs.watch:project opens the project scope: {granted:?}"
    );
}
