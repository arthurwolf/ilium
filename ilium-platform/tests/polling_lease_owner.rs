#![cfg(target_os = "linux")]

use ilium_platform::{
    file_lock::ExclusiveFileLock, file_lock_owner::exclusive_lock_owned_by,
    process_control::capture_pty_process,
};

#[test]
fn peer_lease_proof_requires_the_exact_held_file_and_live_process() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("detector.lock");
    let other = root.path().join("other.lock");
    let held = ExclusiveFileLock::acquire(&path).unwrap();
    let identity = capture_pty_process(std::process::id()).unwrap();
    assert!(exclusive_lock_owned_by(&path, std::process::id(), &identity).unwrap());
    let other_held = ExclusiveFileLock::acquire(&other).unwrap();
    drop(other_held);
    assert!(!exclusive_lock_owned_by(&other, std::process::id(), &identity).unwrap());
    assert!(!exclusive_lock_owned_by(&path, 0, &identity).unwrap());
    drop(held);
    assert!(!exclusive_lock_owned_by(&path, std::process::id(), &identity).unwrap());
}

#[test]
fn replaced_lease_path_and_symlink_do_not_inherit_the_old_lock() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("detector.lock");
    let old = root.path().join("old.lock");
    let alias = root.path().join("alias.lock");
    let held = ExclusiveFileLock::acquire(&path).unwrap();
    let identity = capture_pty_process(std::process::id()).unwrap();
    std::fs::rename(&path, &old).unwrap();
    let replacement = ilium_platform::secure_fs::open_private_regular_file(
        &path,
        ilium_platform::secure_fs::PrivateFileMode::CreateNew,
    )
    .unwrap();
    drop(replacement);
    assert!(!exclusive_lock_owned_by(&path, std::process::id(), &identity).unwrap());
    assert!(exclusive_lock_owned_by(&old, std::process::id(), &identity).unwrap());
    std::os::unix::fs::symlink(&old, &alias).unwrap();
    assert!(exclusive_lock_owned_by(&alias, std::process::id(), &identity).is_err());
    drop(held);
}

#[test]
fn a_live_unrelated_process_and_its_stale_birth_proof_cannot_claim_a_lease() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("detector.lock");
    let _held = ExclusiveFileLock::acquire(&path).unwrap();
    let mut child = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let child_id = child.id();
    let identity = capture_pty_process(child_id);
    // Retain the observation before teardown, but assert only after the owned
    // child is reaped, including when process-proof capture itself fails.
    let unrelated = identity
        .as_ref()
        .ok()
        .map(|proof| exclusive_lock_owned_by(&path, child_id, proof));
    let _ = child.kill();
    child.wait().unwrap();
    let identity = identity.unwrap();
    assert!(!unrelated.unwrap().unwrap());
    assert!(!exclusive_lock_owned_by(&path, child_id, &identity).unwrap());
}
