//! reference-package-format.md: deterministic archives from `ilium plugin pack` (PKG-825).

use std::time::{Duration, SystemTime};

use ilium_plugin_api::package::{pack_directory, read_archive};

use crate::support::*;

const SIGNED_AT: &str = "2026-10-22T12:00:00Z";

/// REQ: PKG-825
#[test]
fn pkg_825_packing_the_same_tree_twice_with_the_same_signed_at_is_byte_identical() {
    let tree = source_tree("pkg825-identical");
    let first = pack_directory(&tree, SIGNED_AT).expect("the first pack succeeds");
    let second = pack_directory(&tree, SIGNED_AT).expect("the second pack succeeds");
    assert_eq!(first, second, "the two archives must be byte-identical");
}

/// REQ: PKG-825
#[test]
fn pkg_825_file_modification_times_do_not_change_the_archive() {
    let tree = source_tree("pkg825-mtime");
    let first = pack_directory(&tree, SIGNED_AT).expect("the first pack succeeds");
    let touched = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000_000);
    std::fs::File::options()
        .write(true)
        .open(tree.join("plugin.json"))
        .expect("open the fixture manifest")
        .set_modified(touched)
        .expect("set a new modification time");
    let second = pack_directory(&tree, SIGNED_AT).expect("the second pack succeeds");
    assert_eq!(
        first, second,
        "the archive does not record file modification times"
    );
}

/// REQ: PKG-825
#[test]
fn pkg_825_archive_entries_use_fixed_zip_time_mode_0644_and_no_extra_fields() {
    let tree = source_tree("pkg825-entries");
    let bytes = pack_directory(&tree, SIGNED_AT).expect("the pack succeeds");
    let entries = read_archive(&bytes).expect("the archive reads back");
    assert!(!entries.is_empty(), "the archive holds the source tree");
    for entry in entries {
        assert_eq!(
            entry.modified(),
            "1980-01-01 00:00:00",
            "ZIP timestamps are fixed to 1980-01-01 00:00:00 for {}",
            entry.name()
        );
        assert_eq!(entry.mode(), 0o644, "mode is 0644 for {}", entry.name());
        assert_eq!(
            entry.extra_field_len(),
            0,
            "no extra fields for {}",
            entry.name()
        );
    }
}
