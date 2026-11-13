//! The pure open-externally policy of `ilium-client/src/open_target.rs`, tested through the
//! plugin's `open_target` module. Disk existence is injected as booleans, as the host will answer.

use std::path::{Path, PathBuf};

use ilium_plugin_core_editor::open_target::{
    classify_path, confine, normalize_lexically, resolve_url, OpenTarget,
};

/// REQ: F-019
#[test]
fn only_http_and_https_urls_are_offered() {
    assert_eq!(resolve_url("javascript:alert(1)"), None);
    assert_eq!(resolve_url("file:///etc/passwd"), None);
    assert_eq!(
        resolve_url("https://example.test"),
        Some(OpenTarget::Url("https://example.test".to_string()))
    );
    assert!(resolve_url("http://example.test").is_some());
}

/// REQ: F-019
#[test]
fn menu_labels_and_file_paths_follow_the_target_kind() {
    let file = OpenTarget::File(PathBuf::from("/p/a.rs"));
    assert_eq!(file.menu_label(), "Open file");
    assert_eq!(file.file_path(), Some(Path::new("/p/a.rs")));
    let directory = OpenTarget::Directory(PathBuf::from("/p"));
    assert_eq!(directory.menu_label(), "Open folder");
    assert_eq!(directory.file_path(), None);
    let url = OpenTarget::Url("https://x".into());
    assert_eq!(url.menu_label(), "Open in browser");
    assert_eq!(url.file_path(), None);
}

/// REQ: F-019
#[test]
fn dot_dot_cannot_escape_the_working_directory_by_prefix_matching() {
    let cwd = Path::new("/work/project");
    assert_eq!(
        confine(Path::new("/work/project/../real.txt"), cwd, None),
        None
    );
    assert_eq!(
        confine(Path::new("/work/project/src/../a.rs"), cwd, None),
        Some(PathBuf::from("/work/project/a.rs"))
    );
}

/// REQ: F-019
#[test]
fn a_path_outside_cwd_and_home_is_never_confined() {
    let cwd = Path::new("/work/project");
    assert_eq!(confine(Path::new("/etc/passwd"), cwd, None), None);
    assert_eq!(
        confine(
            Path::new("/home/u/notes.md"),
            cwd,
            Some(Path::new("/home/u"))
        ),
        Some(PathBuf::from("/home/u/notes.md"))
    );
    assert_eq!(
        confine(Path::new("/home/other/x"), cwd, Some(Path::new("/home/u"))),
        None
    );
}

/// REQ: F-019
#[test]
fn a_working_directory_with_dot_dot_still_confines_like_its_normalized_form() {
    let cwd = Path::new("/work/project/src/..");
    assert_eq!(
        confine(Path::new("/work/project/src/main.rs"), cwd, None),
        Some(PathBuf::from("/work/project/src/main.rs"))
    );
}

/// REQ: F-019
#[test]
fn normalization_collapses_dots_and_never_climbs_above_the_root() {
    assert_eq!(
        normalize_lexically(Path::new("/a/./b/../c")),
        PathBuf::from("/a/c")
    );
    assert_eq!(normalize_lexically(Path::new("/../a")), PathBuf::from("/a"));
    assert_eq!(
        normalize_lexically(Path::new("../a")),
        PathBuf::from("../a")
    );
}

/// REQ: F-019
#[test]
fn classification_prefers_directory_and_refuses_a_missing_path() {
    let path = PathBuf::from("/work/project/docs");
    assert_eq!(
        classify_path(path.clone(), true, false),
        Some(OpenTarget::Directory(path.clone()))
    );
    assert_eq!(
        classify_path(path.clone(), false, true),
        Some(OpenTarget::File(path.clone()))
    );
    assert_eq!(classify_path(path, false, false), None);
}
