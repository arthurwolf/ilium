//! Boundary coverage for the requestable limit contract (PKG-070 through PKG-074).
use ilium_plugin_api::limits::{LimitKey, Limits, ceiling, clamp, default_of};
use ilium_plugin_api::tier::Tier;

#[test]
fn every_limit_preserves_lower_requests_and_clamps_valid_oversized_requests() {
    for key in LimitKey::ALL {
        for tier in Tier::ALL {
            for requested in [1, ceiling(key, tier), u64::from(u32::MAX)] {
                let limits = Limits([(key.as_key().to_owned(), requested)].into());
                let effective = clamp(&limits, tier).expect("valid request");
                assert_eq!(effective.len(), LimitKey::ALL.len());
                assert_eq!(effective[&key], requested.min(ceiling(key, tier)));
            }
            let limits = Limits([(key.as_key().to_owned(), 0)].into());
            assert_eq!(
                clamp(&limits, tier).expect("zero defaults")[&key],
                default_of(key)
            );
            assert_eq!(ceiling(key, Tier::T2), ceiling(key, Tier::T3));
        }
    }
}

#[test]
fn invalid_integer_range_is_a_schema_error_at_the_limit_value() {
    let limits = Limits([("memory_mib".to_owned(), u64::from(u32::MAX) + 1)].into());
    let diagnostic = clamp(&limits, Tier::T1).expect_err("not a manifest integer");
    assert_eq!(diagnostic.code.as_str(), "manifest.schema");
    assert_eq!(diagnostic.pointer, "/limits/memory_mib");
}

#[test]
fn unknown_keys_have_escaped_json_pointers_even_when_zero() {
    let limits = Limits([("bad~/limit".to_owned(), 0)].into());
    let diagnostic = clamp(&limits, Tier::T3).expect_err("unknown key");
    assert_eq!(diagnostic.code.as_str(), "manifest.unknown-key");
    assert_eq!(diagnostic.pointer, "/limits/bad~0~1limit");
}
