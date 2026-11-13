//! Dedup keys for keyed submissions (rust-sdk.md "Values, submissions and outcomes").

/// Joins the parts into a dedup key of at most 128 bytes that the host de-duplicates for 10 minutes.
pub fn key_for(_parts: &[&str]) -> String {
    unimplemented!("rust-sdk.dedup-key")
}
