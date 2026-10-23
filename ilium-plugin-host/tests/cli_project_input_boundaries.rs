use ilium_plugin_host::cli::project_inputs::{project_input_hash, project_input_hash_with_toolchain};
use std::{collections::BTreeMap, path::Path};

fn write(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn source_lockfile_and_manifest_content_changes_invalidate_hash() {
    let root = tempfile::tempdir().unwrap();
    for name in ["manifest.json", "Cargo.lock", "src/lib.rs"] { write(root.path(), name, b"first"); }
    for name in ["src/lib.rs", "Cargo.lock", "manifest.json"] {
        let before = project_input_hash(root.path()).unwrap();
        write(root.path(), name, b"other");
        let after = project_input_hash(root.path()).unwrap();
        assert_ne!(before, after, "{name}");
        assert_eq!(after.len(), 64);
        assert!(after.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    }
}

#[test]
fn generated_outputs_do_not_change_hash_but_nested_source_names_do() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "manifest.json", b"{}");
    let before = project_input_hash(root.path()).unwrap();
    for name in ["dist/build.json", "target/output", "node_modules/cache", ".ilium/cache/output", ".git/index"] {
        write(root.path(), name, b"generated");
    }
    assert_eq!(project_input_hash(root.path()).unwrap(), before);
    write(root.path(), "src/target/source.rs", b"real input");
    assert_ne!(project_input_hash(root.path()).unwrap(), before);
}

#[test]
fn creation_order_is_irrelevant_and_paths_and_contents_are_framed() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    write(first.path(), "a", b"bc"); write(first.path(), "z", b"end");
    write(second.path(), "z", b"end"); write(second.path(), "a", b"bc");
    assert_eq!(project_input_hash(first.path()).unwrap(), project_input_hash(second.path()).unwrap());
    std::fs::remove_file(second.path().join("a")).unwrap();
    write(second.path(), "ab", b"c");
    assert_ne!(project_input_hash(first.path()).unwrap(), project_input_hash(second.path()).unwrap());
}

#[test]
fn explicit_toolchain_versions_affect_identity() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "src/lib.rs", b"source");
    let mut versions = BTreeMap::from([("rustc".into(), "1.0".into()), ("sdk".into(), "0.1".into())]);
    let before = project_input_hash_with_toolchain(root.path(), &versions).unwrap();
    versions.insert("rustc".into(), "1.1".into());
    assert_ne!(project_input_hash_with_toolchain(root.path(), &versions).unwrap(), before);
    versions.insert("sdk".into(), String::new());
    assert!(project_input_hash_with_toolchain(root.path(), &versions).is_err());
}

#[test]
fn missing_root_and_file_in_place_of_directory_return_errors() {
    let root = tempfile::tempdir().unwrap();
    assert!(project_input_hash(&root.path().join("missing")).is_err());
    write(root.path(), "file", b"input");
    assert!(project_input_hash(&root.path().join("file")).is_err());
}

#[test]
fn ignored_source_is_conservatively_hashed_instead_of_silently_excluded() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), ".gitignore", b"local-source\n");
    write(root.path(), "local-source", b"first");
    let before = project_input_hash(root.path()).unwrap();
    write(root.path(), "local-source", b"other");
    assert_ne!(project_input_hash(root.path()).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn symlink_escape_and_directory_loop_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "secret", b"outside project");
    std::os::unix::fs::symlink(outside.path().join("secret"), root.path().join("escape")).unwrap();
    assert!(project_input_hash(root.path()).unwrap_err().contains("symlink"));
    std::fs::remove_file(root.path().join("escape")).unwrap();
    std::os::unix::fs::symlink(root.path(), root.path().join("loop")).unwrap();
    assert!(project_input_hash(root.path()).unwrap_err().contains("symlink"));
}
