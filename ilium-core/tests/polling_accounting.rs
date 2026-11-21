use ilium_core::polling_detection::{estimate_cost_nanodollars, TokenRates, TokenUsage};

#[test]
fn pricing_subtracts_cache_from_total_without_double_counting() {
    let usage = TokenUsage {
        input: 1000,
        cache_read: 300,
        cache_write: 100,
    };
    let rates = TokenRates {
        uncached_nanos_per_million: 100_000_000,
        cache_read_nanos_per_million: 10_000_000,
        cache_write_nanos_per_million: 125_000_000,
    };
    assert_eq!(estimate_cost_nanodollars(usage, rates), Some(75_500));
}

#[test]
fn impossible_cache_counts_and_arithmetic_overflow_are_unknown() {
    let invalid = TokenUsage {
        input: 10,
        cache_read: 11,
        cache_write: 0,
    };
    let rates = TokenRates {
        uncached_nanos_per_million: 100_000_000,
        cache_read_nanos_per_million: 10_000_000,
        cache_write_nanos_per_million: 125_000_000,
    };
    assert_eq!(estimate_cost_nanodollars(invalid, rates), None);
    let huge = TokenUsage {
        input: u64::MAX,
        cache_read: 0,
        cache_write: 0,
    };
    assert_eq!(estimate_cost_nanodollars(huge, rates), None);
}
