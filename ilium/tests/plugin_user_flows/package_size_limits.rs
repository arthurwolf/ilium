//! Package size limits on install (USR-249).

use crate::support::*;

/// REQ: USR-249
#[test]
fn usr_249_a_package_one_byte_over_the_compressed_limit_is_refused_with_no_files_left() {
    let mut rig = UserRig::new();
    let scratch = rig.scratch_dir();
    std::fs::create_dir_all(&scratch).expect("scratch folder");
    let package_path = scratch.join("too-big.pkg");
    let file = std::fs::File::create(&package_path).expect("package file");
    file.set_len(64 * 1024 * 1024 + 1).expect("set size");
    drop(file);
    let before = tree_snapshot(&rig.data_dir());
    let output = rig.cli(&[
        "install",
        package_path.to_str().expect("utf-8 path"),
        "--allow-unsigned",
        "--yes",
    ]);
    assert!(error_codes(&output).contains(&"package.too-large".to_string()));
    assert_eq!(tree_snapshot(&rig.data_dir()), before);
}

/// REQ: USR-249
#[test]
fn usr_249_a_package_exactly_at_the_compressed_limit_is_not_refused_as_too_large() {
    let mut rig = UserRig::new();
    let scratch = rig.scratch_dir();
    std::fs::create_dir_all(&scratch).expect("scratch folder");
    let package_path = scratch.join("at-limit.pkg");
    let file = std::fs::File::create(&package_path).expect("package file");
    file.set_len(64 * 1024 * 1024).expect("set size");
    drop(file);
    let output = rig.cli(&[
        "install",
        package_path.to_str().expect("utf-8 path"),
        "--allow-unsigned",
        "--yes",
    ]);
    assert!(
        !error_codes(&output).contains(&"package.too-large".to_string()),
        "{:?}",
        error_codes(&output)
    );
}
