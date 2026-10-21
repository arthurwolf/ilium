//! api-storage.md: `get`, `put`, `remove`, `keys`, `write-batch` and `lease` of
//! `ilium:host/storage`. No grant is needed. Keys are 1 to 256 bytes, a value is at most 1 MiB, a
//! bucket holds 16 MiB by default (key bytes plus value bytes), a batch holds at most 256 entries
//! and 4 MiB, and `lease` gives a key to one caller until the ttl passes.
//!
//! Fixture helpers beyond part 1 are listed in API_GAPS_p2.md.

use std::time::Duration;

use ilium_plugin_api::generated::host::{Bucket, Entry};
use ilium_plugin_api::generated::types::ErrorCode;
use ilium_plugin_host::testing::imports_fixture::ImportsFixture;
use proptest::prelude::*;

use crate::support::*;

const BUCKETS: [Bucket; 4] = [
    Bucket::Global,
    Bucket::Project,
    Bucket::Session,
    Bucket::Shared,
];

fn storage_fixture() -> ImportsFixture {
    fixture_t2(&[])
}

/// A fixture whose buckets hold `mib` MiB.
fn small_bucket_fixture(mib: u64) -> ImportsFixture {
    ImportsFixture::builder()
        .tier(ilium_plugin_api::Tier::T2)
        .limit("storage_bucket_mib", mib)
        .build()
}

fn put_entry(key: &str, value: &[u8]) -> Entry {
    Entry {
        key: key.to_owned(),
        value: Some(value.to_vec()),
    }
}

fn remove_entry(key: &str) -> Entry {
    Entry {
        key: key.to_owned(),
        value: None,
    }
}

fn bytes(count: usize) -> Vec<u8> {
    vec![0xAB; count]
}

fn all_keys(fixture: &mut ImportsFixture, bucket: Bucket) -> Vec<String> {
    fixture.storage().keys(bucket, String::new())
}

// ---- get ---------------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/storage.get
#[test]
fn storage_get_returns_the_stored_value() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Project, "saves".to_owned(), vec![1, 2, 3]),
    );
    assert_eq!(
        fixture.storage().get(Bucket::Project, "saves".to_owned()),
        Some(vec![1, 2, 3])
    );
}

/// REQ: WIT:ilium:host/storage.get
#[test]
fn storage_get_of_a_missing_key_is_none() {
    let mut fixture = storage_fixture();
    assert_eq!(
        fixture.storage().get(Bucket::Global, "missing".to_owned()),
        None
    );
}

/// REQ: WIT:ilium:host/storage.get
#[test]
fn storage_get_of_an_empty_stored_value_is_some_empty() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "empty".to_owned(), Vec::new()),
    );
    assert_eq!(
        fixture.storage().get(Bucket::Global, "empty".to_owned()),
        Some(Vec::new())
    );
}

/// REQ: WIT:ilium:host/storage.get
#[test]
fn storage_get_is_per_bucket() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "k".to_owned(), vec![1]),
    );
    assert_eq!(fixture.storage().get(Bucket::Project, "k".to_owned()), None);
    assert_eq!(fixture.storage().get(Bucket::Session, "k".to_owned()), None);
    assert_eq!(fixture.storage().get(Bucket::Shared, "k".to_owned()), None);
}

/// REQ: WIT:ilium:host/storage.get
#[test]
fn storage_get_does_not_fold_case_or_trim_the_key() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "Key".to_owned(), vec![1]),
    );
    assert_eq!(
        fixture.storage().get(Bucket::Global, "key".to_owned()),
        None
    );
    assert_eq!(
        fixture.storage().get(Bucket::Global, " Key".to_owned()),
        None
    );
    assert_eq!(
        fixture.storage().get(Bucket::Global, "Key".to_owned()),
        Some(vec![1])
    );
}

/// REQ: WIT:ilium:host/storage.get
#[test]
fn storage_get_of_an_empty_or_over_long_key_is_none_not_an_error() {
    let mut fixture = storage_fixture();
    assert_eq!(fixture.storage().get(Bucket::Global, String::new()), None);
    assert_eq!(fixture.storage().get(Bucket::Global, "k".repeat(300)), None);
}

proptest! {
    /// REQ: WIT:ilium:host/storage.get, WIT:ilium:host/storage.put
    #[test]
    fn storage_put_then_get_round_trips_any_value(value in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let mut fixture = storage_fixture();
        prop_assert!(fixture.storage().put(Bucket::Session, "round".to_owned(), value.clone()).is_ok());
        prop_assert_eq!(fixture.storage().get(Bucket::Session, "round".to_owned()), Some(value));
    }
}

// ---- put ---------------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/storage.put
#[test]
fn storage_put_replaces_the_whole_value() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "k".to_owned(), vec![1, 2, 3, 4]),
    );
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "k".to_owned(), vec![9]),
    );
    assert_eq!(
        fixture.storage().get(Bucket::Global, "k".to_owned()),
        Some(vec![9])
    );
}

/// REQ: WIT:ilium:host/storage.put
#[test]
fn storage_put_works_in_every_bucket() {
    let mut fixture = storage_fixture();
    for bucket in BUCKETS {
        expect_ok(fixture.storage().put(bucket, "k".to_owned(), vec![7]));
        assert_eq!(fixture.storage().get(bucket, "k".to_owned()), Some(vec![7]));
    }
}

/// REQ: WIT:ilium:host/storage.put
#[test]
fn storage_put_key_of_256_bytes_accepted() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "k".repeat(256), vec![1]),
    );
}

/// REQ: WIT:ilium:host/storage.put
#[test]
fn storage_put_key_of_257_bytes_invalid_argument() {
    let mut fixture = storage_fixture();
    let code = expect_code(
        fixture
            .storage()
            .put(Bucket::Global, "k".repeat(257), vec![1]),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/storage.put
#[test]
fn storage_put_key_length_is_counted_in_bytes_not_characters() {
    let mut fixture = storage_fixture();
    // 128 two-byte characters are 256 bytes; 129 are 258 bytes.
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "\u{e9}".repeat(128), vec![1]),
    );
    let code = expect_code(
        fixture
            .storage()
            .put(Bucket::Global, "\u{e9}".repeat(129), vec![1]),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/storage.put
#[test]
fn storage_put_empty_key_invalid_argument() {
    let mut fixture = storage_fixture();
    let code = expect_code(
        fixture
            .storage()
            .put(Bucket::Global, String::new(), vec![1]),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/storage.put
#[test]
fn storage_put_key_of_one_byte_accepted() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "a".to_owned(), vec![1]),
    );
}

/// REQ: WIT:ilium:host/storage.put
#[test]
fn storage_put_value_of_exactly_1_mib_accepted() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "big".to_owned(), bytes(MIB)),
    );
    assert_eq!(
        fixture
            .storage()
            .get(Bucket::Global, "big".to_owned())
            .map(|v| v.len()),
        Some(MIB)
    );
}

/// REQ: WIT:ilium:host/storage.put
#[test]
fn storage_put_value_of_1_mib_plus_one_invalid_argument_never_quota_exceeded() {
    let mut fixture = storage_fixture();
    let code = expect_code(
        fixture
            .storage()
            .put(Bucket::Global, "big".to_owned(), bytes(MIB + 1)),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/storage.put
#[test]
fn storage_put_refused_value_keeps_the_previous_value() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "k".to_owned(), vec![1, 2]),
    );
    let _ = fixture
        .storage()
        .put(Bucket::Global, "k".to_owned(), bytes(MIB + 1));
    assert_eq!(
        fixture.storage().get(Bucket::Global, "k".to_owned()),
        Some(vec![1, 2])
    );
}

/// REQ: WIT:ilium:host/storage.put
#[test]
fn storage_put_counts_key_and_value_bytes_against_the_bucket_quota() {
    let mut fixture = small_bucket_fixture(2);
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "a".to_owned(), bytes(MIB)),
    );
    // 1 + MIB for the first entry; 1 + (MIB - 2) brings the bucket to exactly 2 MiB.
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "b".to_owned(), bytes(MIB - 2)),
    );
}

/// REQ: WIT:ilium:host/storage.put
#[test]
fn storage_put_one_byte_past_the_bucket_quota_quota_exceeded() {
    let mut fixture = small_bucket_fixture(2);
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "a".to_owned(), bytes(MIB)),
    );
    let code = expect_code(
        fixture
            .storage()
            .put(Bucket::Global, "b".to_owned(), bytes(MIB - 1)),
    );
    assert_eq!(code, ErrorCode::QuotaExceeded);
    assert_eq!(fixture.storage().get(Bucket::Global, "b".to_owned()), None);
}

/// REQ: WIT:ilium:host/storage.put
#[test]
fn storage_put_replacing_a_value_counts_only_the_difference() {
    let mut fixture = small_bucket_fixture(2);
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "a".to_owned(), bytes(MIB)),
    );
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "b".to_owned(), bytes(MIB - 2)),
    );
    // The bucket is exactly full; the same size again must still be accepted.
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "a".to_owned(), bytes(MIB)),
    );
    // One byte more is the difference that does not fit.
    let code = expect_code(
        fixture
            .storage()
            .put(Bucket::Global, "a".to_owned(), bytes(MIB + 1)),
    );
    assert_eq!(code, ErrorCode::QuotaExceeded);
}

/// REQ: WIT:ilium:host/storage.put
#[test]
fn storage_put_default_bucket_quota_is_16_mib() {
    let mut fixture = storage_fixture();
    // Fifteen keys of one byte with 1 MiB - 1 byte values fill 15 MiB exactly.
    for index in 0..15u8 {
        let key = char::from(b'a' + index).to_string();
        expect_ok(fixture.storage().put(Bucket::Global, key, bytes(MIB - 1)));
    }
    // The sixteenth entry brings the bucket to 16 MiB exactly.
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "z".to_owned(), bytes(MIB - 1)),
    );
    let code = expect_code(
        fixture
            .storage()
            .put(Bucket::Global, "y".to_owned(), vec![1]),
    );
    assert_eq!(code, ErrorCode::QuotaExceeded);
}

/// REQ: WIT:ilium:host/storage.put
#[test]
fn storage_put_quota_is_per_bucket() {
    let mut fixture = small_bucket_fixture(1);
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "a".to_owned(), bytes(MIB - 1)),
    );
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Project, "a".to_owned(), bytes(MIB - 1)),
    );
}

// ---- remove ------------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/storage.remove
#[test]
fn storage_remove_deletes_the_key() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Session, "k".to_owned(), vec![1]),
    );
    fixture.storage().remove(Bucket::Session, "k".to_owned());
    assert_eq!(fixture.storage().get(Bucket::Session, "k".to_owned()), None);
    assert!(all_keys(&mut fixture, Bucket::Session).is_empty());
}

/// REQ: WIT:ilium:host/storage.remove
#[test]
fn storage_remove_of_a_missing_key_does_nothing() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Session, "keep".to_owned(), vec![1]),
    );
    fixture
        .storage()
        .remove(Bucket::Session, "missing".to_owned());
    assert_eq!(
        all_keys(&mut fixture, Bucket::Session),
        vec!["keep".to_owned()]
    );
}

/// REQ: WIT:ilium:host/storage.remove
#[test]
fn storage_remove_touches_only_the_named_bucket() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "k".to_owned(), vec![1]),
    );
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Project, "k".to_owned(), vec![2]),
    );
    fixture.storage().remove(Bucket::Global, "k".to_owned());
    assert_eq!(
        fixture.storage().get(Bucket::Project, "k".to_owned()),
        Some(vec![2])
    );
}

/// REQ: WIT:ilium:host/storage.remove
#[test]
fn storage_remove_frees_the_quota_it_used() {
    let mut fixture = small_bucket_fixture(1);
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "a".to_owned(), bytes(MIB - 1)),
    );
    assert_eq!(
        expect_code(
            fixture
                .storage()
                .put(Bucket::Global, "b".to_owned(), vec![1])
        ),
        ErrorCode::QuotaExceeded
    );
    fixture.storage().remove(Bucket::Global, "a".to_owned());
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "b".to_owned(), vec![1]),
    );
}

/// REQ: WIT:ilium:host/storage.remove
#[test]
fn storage_remove_of_an_empty_or_over_long_key_does_nothing() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "keep".to_owned(), vec![1]),
    );
    fixture.storage().remove(Bucket::Global, String::new());
    fixture.storage().remove(Bucket::Global, "k".repeat(300));
    assert_eq!(
        all_keys(&mut fixture, Bucket::Global),
        vec!["keep".to_owned()]
    );
    assert_eq!(
        fixture.storage().get(Bucket::Global, "keep".to_owned()),
        Some(vec![1])
    );
}

// ---- keys --------------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/storage.keys
#[test]
fn storage_keys_with_an_empty_prefix_returns_every_key() {
    let mut fixture = storage_fixture();
    for key in ["b", "a", "c"] {
        expect_ok(
            fixture
                .storage()
                .put(Bucket::Global, key.to_owned(), vec![1]),
        );
    }
    assert_eq!(all_keys(&mut fixture, Bucket::Global), vec!["a", "b", "c"]);
}

/// REQ: WIT:ilium:host/storage.keys
#[test]
fn storage_keys_returns_only_keys_that_start_with_the_prefix() {
    let mut fixture = storage_fixture();
    for key in ["pane/1/cursor", "pane/2/cursor", "panel", "other"] {
        expect_ok(
            fixture
                .storage()
                .put(Bucket::Session, key.to_owned(), vec![1]),
        );
    }
    assert_eq!(
        fixture.storage().keys(Bucket::Session, "pane/".to_owned()),
        vec!["pane/1/cursor", "pane/2/cursor"]
    );
}

/// REQ: WIT:ilium:host/storage.keys
#[test]
fn storage_keys_are_sorted_in_byte_order() {
    let mut fixture = storage_fixture();
    for key in ["b", "B", "a", "\u{e9}", "_", "10", "9"] {
        expect_ok(
            fixture
                .storage()
                .put(Bucket::Global, key.to_owned(), vec![1]),
        );
    }
    let mut expected: Vec<String> = ["b", "B", "a", "\u{e9}", "_", "10", "9"]
        .iter()
        .map(|key| (*key).to_owned())
        .collect();
    expected.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    assert_eq!(all_keys(&mut fixture, Bucket::Global), expected);
}

/// REQ: WIT:ilium:host/storage.keys
#[test]
fn storage_keys_prefix_is_case_sensitive() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "Abc".to_owned(), vec![1]),
    );
    assert!(fixture
        .storage()
        .keys(Bucket::Global, "abc".to_owned())
        .is_empty());
    assert_eq!(
        fixture.storage().keys(Bucket::Global, "Ab".to_owned()),
        vec!["Abc"]
    );
}

/// REQ: WIT:ilium:host/storage.keys
#[test]
fn storage_keys_of_an_empty_bucket_is_empty() {
    let mut fixture = storage_fixture();
    assert!(all_keys(&mut fixture, Bucket::Project).is_empty());
}

/// REQ: WIT:ilium:host/storage.keys
#[test]
fn storage_keys_is_per_bucket() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "g".to_owned(), vec![1]),
    );
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Project, "p".to_owned(), vec![1]),
    );
    assert_eq!(all_keys(&mut fixture, Bucket::Global), vec!["g"]);
    assert_eq!(all_keys(&mut fixture, Bucket::Project), vec!["p"]);
}

/// REQ: WIT:ilium:host/storage.keys
#[test]
fn storage_keys_is_not_paged_for_a_large_bucket() {
    let mut fixture = storage_fixture();
    for index in 0..1500 {
        expect_ok(
            fixture
                .storage()
                .put(Bucket::Global, format!("key{index:05}"), Vec::new()),
        );
    }
    assert_eq!(all_keys(&mut fixture, Bucket::Global).len(), 1500);
}

proptest! {
    /// REQ: WIT:ilium:host/storage.keys
    #[test]
    fn storage_keys_with_a_prefix_equals_the_sorted_filter_of_all_keys(
        keys in proptest::collection::btree_set("[a-c/]{1,6}", 0..12),
        prefix in "[a-c/]{0,3}",
    ) {
        let mut fixture = storage_fixture();
        for key in &keys {
            prop_assert!(fixture.storage().put(Bucket::Global, key.clone(), vec![1]).is_ok());
        }
        let mut expected: Vec<String> = keys.iter().filter(|key| key.starts_with(&prefix)).cloned().collect();
        expected.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        prop_assert_eq!(fixture.storage().keys(Bucket::Global, prefix), expected);
    }
}

// ---- write-batch -------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/storage.write-batch
#[test]
fn storage_write_batch_applies_every_entry() {
    let mut fixture = storage_fixture();
    expect_ok(fixture.storage().write_batch(
        Bucket::Project,
        vec![
            put_entry("a", &[1]),
            put_entry("b", &[2]),
            put_entry("c", &[3]),
        ],
    ));
    assert_eq!(all_keys(&mut fixture, Bucket::Project), vec!["a", "b", "c"]);
    assert_eq!(
        fixture.storage().get(Bucket::Project, "b".to_owned()),
        Some(vec![2])
    );
}

/// REQ: WIT:ilium:host/storage.write-batch
#[test]
fn storage_write_batch_entry_with_no_value_removes_the_key() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Project, "gone".to_owned(), vec![1]),
    );
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Project, "stay".to_owned(), vec![1]),
    );
    expect_ok(fixture.storage().write_batch(
        Bucket::Project,
        vec![remove_entry("gone"), put_entry("new", &[5])],
    ));
    assert_eq!(all_keys(&mut fixture, Bucket::Project), vec!["new", "stay"]);
}

/// REQ: WIT:ilium:host/storage.write-batch
#[test]
fn storage_write_batch_empty_list_accepted_and_changes_nothing() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "k".to_owned(), vec![1]),
    );
    expect_ok(fixture.storage().write_batch(Bucket::Global, Vec::new()));
    assert_eq!(all_keys(&mut fixture, Bucket::Global), vec!["k"]);
}

/// REQ: WIT:ilium:host/storage.write-batch
#[test]
fn storage_write_batch_256_entries_accepted() {
    let mut fixture = storage_fixture();
    let entries: Vec<Entry> = (0..256)
        .map(|index| put_entry(&format!("k{index:03}"), &[1]))
        .collect();
    expect_ok(fixture.storage().write_batch(Bucket::Global, entries));
    assert_eq!(all_keys(&mut fixture, Bucket::Global).len(), 256);
}

/// REQ: WIT:ilium:host/storage.write-batch
#[test]
fn storage_write_batch_257_entries_invalid_argument_and_nothing_written() {
    let mut fixture = storage_fixture();
    let entries: Vec<Entry> = (0..257)
        .map(|index| put_entry(&format!("k{index:03}"), &[1]))
        .collect();
    let code = expect_code(fixture.storage().write_batch(Bucket::Global, entries));
    assert_eq!(code, ErrorCode::InvalidArgument);
    assert!(all_keys(&mut fixture, Bucket::Global).is_empty());
}

/// REQ: WIT:ilium:host/storage.write-batch
#[test]
fn storage_write_batch_of_exactly_4_mib_of_key_and_value_bytes_accepted() {
    let mut fixture = storage_fixture();
    // Three entries of 1 + MIB bytes and one of 1 + (MIB - 3) bytes: exactly 4 MiB.
    let entries = vec![
        put_entry("a", &bytes(MIB)),
        put_entry("b", &bytes(MIB)),
        put_entry("c", &bytes(MIB)),
        put_entry("d", &bytes(MIB - 3)),
    ];
    expect_ok(fixture.storage().write_batch(Bucket::Global, entries));
}

/// REQ: WIT:ilium:host/storage.write-batch
#[test]
fn storage_write_batch_one_byte_over_4_mib_invalid_argument_and_nothing_written() {
    let mut fixture = storage_fixture();
    let entries = vec![
        put_entry("a", &bytes(MIB)),
        put_entry("b", &bytes(MIB)),
        put_entry("c", &bytes(MIB)),
        put_entry("d", &bytes(MIB - 2)),
    ];
    let code = expect_code(fixture.storage().write_batch(Bucket::Global, entries));
    assert_eq!(code, ErrorCode::InvalidArgument);
    assert!(all_keys(&mut fixture, Bucket::Global).is_empty());
}

/// REQ: WIT:ilium:host/storage.write-batch
#[test]
fn storage_write_batch_whose_last_entry_breaks_the_quota_keeps_every_previous_value() {
    let mut fixture = small_bucket_fixture(1);
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "old".to_owned(), vec![9]),
    );
    let entries = vec![
        put_entry("first", &[1]),
        put_entry("second", &[2]),
        put_entry("last", &bytes(MIB)),
    ];
    let code = expect_code(fixture.storage().write_batch(Bucket::Global, entries));
    assert_eq!(code, ErrorCode::QuotaExceeded);
    assert_eq!(all_keys(&mut fixture, Bucket::Global), vec!["old"]);
    assert_eq!(
        fixture.storage().get(Bucket::Global, "old".to_owned()),
        Some(vec![9])
    );
}

/// REQ: WIT:ilium:host/storage.write-batch
#[test]
fn storage_write_batch_with_an_empty_key_invalid_argument_and_nothing_written() {
    let mut fixture = storage_fixture();
    let code = expect_code(fixture.storage().write_batch(
        Bucket::Global,
        vec![put_entry("ok", &[1]), put_entry("", &[2])],
    ));
    assert_eq!(code, ErrorCode::InvalidArgument);
    assert!(all_keys(&mut fixture, Bucket::Global).is_empty());
}

/// REQ: WIT:ilium:host/storage.write-batch
#[test]
fn storage_write_batch_with_a_value_over_1_mib_invalid_argument_and_nothing_written() {
    let mut fixture = storage_fixture();
    let code = expect_code(fixture.storage().write_batch(
        Bucket::Global,
        vec![put_entry("ok", &[1]), put_entry("big", &bytes(MIB + 1))],
    ));
    assert_eq!(code, ErrorCode::InvalidArgument);
    assert!(all_keys(&mut fixture, Bucket::Global).is_empty());
}

/// REQ: WIT:ilium:host/storage.write-batch
#[test]
fn storage_write_batch_from_a_stale_data_version_conflict() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Global, "k".to_owned(), vec![1]),
    );
    fixture.make_bucket_data_version_newer(Bucket::Global);
    let error = expect_error(
        fixture
            .storage()
            .write_batch(Bucket::Global, vec![put_entry("k", &[2])]),
    );
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(error.message, "stale-data-version");
    assert_eq!(
        fixture.storage().get(Bucket::Global, "k".to_owned()),
        Some(vec![1])
    );
}

/// REQ: WIT:ilium:host/storage.write-batch
#[test]
fn storage_write_batch_later_entries_for_one_key_win() {
    let mut fixture = storage_fixture();
    expect_ok(fixture.storage().write_batch(
        Bucket::Global,
        vec![put_entry("k", &[1]), put_entry("k", &[2])],
    ));
    assert_eq!(
        fixture.storage().get(Bucket::Global, "k".to_owned()),
        Some(vec![2])
    );
}

proptest! {
    /// REQ: WIT:ilium:host/storage.write-batch
    #[test]
    fn storage_write_batch_matches_applying_the_entries_one_by_one(
        entries in proptest::collection::vec(("[a-d]", proptest::option::of(proptest::collection::vec(any::<u8>(), 0..8))), 0..20),
    ) {
        let mut batched = storage_fixture();
        let mut single = storage_fixture();
        let batch: Vec<Entry> = entries
            .iter()
            .map(|(key, value)| Entry { key: key.clone(), value: value.clone() })
            .collect();
        prop_assert!(batched.storage().write_batch(Bucket::Global, batch).is_ok());
        for (key, value) in &entries {
            match value {
                Some(value) => { prop_assert!(single.storage().put(Bucket::Global, key.clone(), value.clone()).is_ok()); }
                None => { prop_assert!(single.storage().remove(Bucket::Global, key.clone()).is_ok()); }
            }
        }
        let batched_keys = all_keys(&mut batched, Bucket::Global);
        prop_assert_eq!(&batched_keys, &all_keys(&mut single, Bucket::Global));
        for key in batched_keys {
            prop_assert_eq!(
                batched.storage().get(Bucket::Global, key.clone()),
                single.storage().get(Bucket::Global, key)
            );
        }
    }
}

// ---- lease -------------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/storage.lease
#[test]
fn storage_lease_first_caller_gets_true() {
    let mut fixture = storage_fixture();
    assert!(fixture.storage().lease("index".to_owned(), 60_000));
}

/// REQ: WIT:ilium:host/storage.lease
#[test]
fn storage_lease_second_caller_gets_false_while_the_ttl_is_open() {
    let mut fixture = storage_fixture();
    assert!(fixture.storage().lease("index".to_owned(), 60_000));
    assert!(!fixture.other_caller_lease("index", 60_000));
}

/// REQ: WIT:ilium:host/storage.lease
#[test]
fn storage_lease_just_before_the_ttl_ends_the_other_caller_still_gets_false() {
    let mut fixture = storage_fixture();
    assert!(fixture.storage().lease("index".to_owned(), 1000));
    fixture.advance(Duration::from_millis(999));
    assert!(!fixture.other_caller_lease("index", 1000));
}

/// REQ: WIT:ilium:host/storage.lease
#[test]
fn storage_lease_after_the_ttl_passes_the_next_caller_gets_true() {
    let mut fixture = storage_fixture();
    assert!(fixture.storage().lease("index".to_owned(), 1000));
    fixture.advance(Duration::from_millis(1001));
    assert!(fixture.other_caller_lease("index", 1000));
}

/// REQ: WIT:ilium:host/storage.lease
#[test]
fn storage_lease_is_per_key() {
    let mut fixture = storage_fixture();
    assert!(fixture.storage().lease("one".to_owned(), 60_000));
    assert!(fixture.other_caller_lease("two", 60_000));
}

/// REQ: WIT:ilium:host/storage.lease
#[test]
fn storage_lease_of_a_one_millisecond_ttl_is_taken_and_then_free_again() {
    let mut fixture = storage_fixture();
    assert!(fixture.storage().lease("k".to_owned(), 1));
    fixture.advance(Duration::from_millis(2));
    assert!(fixture.other_caller_lease("k", 1));
}

/// REQ: WIT:ilium:host/storage.lease
#[test]
fn storage_lease_of_the_maximum_ttl_of_one_hour_stays_taken_for_59_minutes() {
    let mut fixture = storage_fixture();
    assert!(fixture.storage().lease("k".to_owned(), 3_600_000));
    fixture.advance(Duration::from_secs(59 * 60));
    assert!(!fixture.other_caller_lease("k", 3_600_000));
    fixture.advance(Duration::from_secs(61));
    assert!(fixture.other_caller_lease("k", 3_600_000));
}

/// REQ: WIT:ilium:host/storage.lease
#[test]
fn storage_lease_does_not_touch_the_stored_shared_value() {
    let mut fixture = storage_fixture();
    expect_ok(
        fixture
            .storage()
            .put(Bucket::Shared, "index".to_owned(), vec![4]),
    );
    assert!(fixture.storage().lease("index".to_owned(), 1000));
    assert_eq!(
        fixture.storage().get(Bucket::Shared, "index".to_owned()),
        Some(vec![4])
    );
}
