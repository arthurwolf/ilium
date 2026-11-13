//! Dedup keys: ABI-010 and ABI-034.

use ilium_plugin_api::dedup::{
    is_reserved, DedupKey, DEDUP_KEY_MAX_CHARS, DEDUP_KEY_PUNCTUATION, DEDUP_WINDOW_MS,
};
use ilium_plugin_api::ErrorCode;
use proptest::prelude::*;

/// REQ: ABI-010
#[test]
fn abi_010_key_constants() {
    assert_eq!(DEDUP_KEY_MAX_CHARS, 128);
    assert_eq!(DEDUP_KEY_PUNCTUATION, "._:/-");
}

/// REQ: ABI-010
#[test]
fn abi_010_typical_key_is_accepted() {
    let key = DedupKey::parse("job/42:retry-1.a_b").expect("valid key");
    assert_eq!(key.as_str(), "job/42:retry-1.a_b");
}

/// REQ: ABI-010
#[test]
fn abi_010_one_character_key_is_accepted() {
    assert!(DedupKey::parse("a").is_ok());
}

/// REQ: ABI-010
#[test]
fn abi_010_128_character_key_is_accepted() {
    assert!(DedupKey::parse(&"k".repeat(128)).is_ok());
}

/// REQ: ABI-010
#[test]
fn abi_010_129_character_key_is_invalid_argument() {
    let error = DedupKey::parse(&"k".repeat(129)).expect_err("too long");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: ABI-010
#[test]
fn abi_010_empty_key_is_invalid_argument() {
    let error = DedupKey::parse("").expect_err("empty");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: ABI-010
#[test]
fn abi_010_space_is_refused() {
    let error = DedupKey::parse("a b").expect_err("space refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: ABI-010
#[test]
fn abi_010_non_ascii_letter_is_refused() {
    let error = DedupKey::parse("cle-é").expect_err("non-ascii refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// REQ: ABI-034
#[test]
fn abi_034_window_is_600_000_ms() {
    assert_eq!(DEDUP_WINDOW_MS, 600_000);
}

/// REQ: ABI-034
#[test]
fn abi_034_key_is_reserved_at_599_999_ms() {
    assert!(is_reserved(1_000, 1_000 + 599_999));
}

/// REQ: ABI-034
#[test]
fn abi_034_key_is_free_at_600_000_ms() {
    assert!(!is_reserved(1_000, 1_000 + 600_000));
}

/// REQ: ABI-034
#[test]
fn abi_034_key_is_reserved_at_acceptance() {
    assert!(is_reserved(5_000, 5_000));
}

/// REQ: ABI-034
#[test]
fn abi_034_wall_clock_going_back_keeps_the_key_reserved() {
    assert!(is_reserved(10_000, 9_000));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    // REQ: ABI-010
    #[test]
    fn abi_010_keys_of_the_alphabet_are_accepted(key in "[A-Za-z0-9._:/-]{1,128}") {
        prop_assert!(DedupKey::parse(&key).is_ok());
    }

    // REQ: ABI-010
    #[test]
    fn abi_010_keys_with_another_character_are_refused(
        prefix in "[A-Za-z0-9]{0,20}",
        bad in "[ !#$%&*+,;<=>?@\\[\\]^`{|}~\"']",
        suffix in "[A-Za-z0-9]{0,20}",
    ) {
        let key = format!("{prefix}{bad}{suffix}");
        let error = DedupKey::parse(&key).expect_err("character outside the alphabet");
        prop_assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    // REQ: ABI-034
    #[test]
    fn abi_034_reserved_exactly_inside_the_window(accepted in 0u64..1u64 << 50, elapsed in 0u64..2_000_000) {
        prop_assert_eq!(is_reserved(accepted, accepted + elapsed), elapsed < 600_000);
    }
}
