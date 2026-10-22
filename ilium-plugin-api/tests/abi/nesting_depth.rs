//! ABI-061 and ABI-062: the nesting-depth bound of decoded WIT values and its refusal code.

use ilium_plugin_api::nesting::{check_nesting_depth, MAX_NESTING_DEPTH};

/// REQ: ABI-061
#[test]
fn abi_061_a_value_nested_32_levels_deep_is_accepted() {
    assert_eq!(MAX_NESTING_DEPTH, 32);
    assert!(check_nesting_depth(MAX_NESTING_DEPTH).is_ok());
}

/// REQ: ABI-061
#[test]
fn abi_061_a_value_nested_33_levels_deep_is_refused() {
    assert!(check_nesting_depth(MAX_NESTING_DEPTH + 1).is_err());
}

/// REQ: ABI-062
#[test]
fn abi_062_the_refusal_carries_the_stable_depth_exceeded_code() {
    let code = check_nesting_depth(MAX_NESTING_DEPTH + 1)
        .err()
        .expect("a 33-level value is refused");
    assert_eq!(code.as_str(), "abi.depth-exceeded");
}
