//! The concurrency shim: `std` types normally, `loom` types under the cargo feature `loom`, so the
//! concurrent core can be model-checked (test plan section 2.7). A cargo feature, not
//! `--cfg loom`, because ni-build rejects config and RUSTFLAGS overrides.

#[cfg(feature = "loom")]
pub use loom::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "loom")]
pub use loom::sync::{Arc, Mutex};

#[cfg(not(feature = "loom"))]
pub use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(feature = "loom"))]
pub use std::sync::{Arc, Mutex};

/// Constant-time byte comparison for secrets (reference-threat-model.md SEC-376).
pub fn constant_time_eq(_left: &[u8], _right: &[u8]) -> bool {
    unimplemented!("host::sync::constant_time_eq")
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;

    /// REQ: SEC-376
    #[test]
    fn identical_non_empty_slices_compare_equal() {
        let token = b"lock-token-0123456789abcdef";
        assert!(constant_time_eq(token, token));
    }

    /// REQ: SEC-376
    #[test]
    fn two_empty_slices_compare_equal() {
        assert!(constant_time_eq(&[], &[]));
    }

    /// REQ: SEC-376
    #[test]
    fn empty_and_non_empty_slices_compare_unequal() {
        assert!(!constant_time_eq(&[], b"x"));
        assert!(!constant_time_eq(b"x", &[]));
    }

    /// REQ: SEC-376
    #[test]
    fn single_byte_slices_compare_by_value() {
        assert!(constant_time_eq(&[0x00], &[0x00]));
        assert!(!constant_time_eq(&[0x00], &[0x01]));
        assert!(!constant_time_eq(&[0xFF], &[0x7F]));
    }

    /// REQ: SEC-376
    #[test]
    fn difference_in_first_byte_is_detected() {
        assert!(!constant_time_eq(b"Xbcdef", b"abcdef"));
    }

    /// REQ: SEC-376
    #[test]
    fn difference_in_last_byte_is_detected() {
        assert!(!constant_time_eq(b"abcdeX", b"abcdef"));
    }

    /// REQ: SEC-376
    #[test]
    fn difference_in_middle_byte_is_detected() {
        assert!(!constant_time_eq(b"abXdef", b"abcdef"));
    }

    /// REQ: SEC-376
    #[test]
    fn single_bit_difference_is_detected() {
        let base = [0u8; 64];
        let mut flipped = [0u8; 64];
        flipped[63] = 0b0000_0001;
        assert!(!constant_time_eq(&base, &flipped));
        assert!(!constant_time_eq(&flipped, &base));
    }

    /// REQ: SEC-376
    #[test]
    fn all_zero_and_all_ones_slices_compare_unequal() {
        assert!(constant_time_eq(&[0u8; 32], &[0u8; 32]));
        assert!(!constant_time_eq(&[0u8; 32], &[0xFFu8; 32]));
    }

    /// REQ: SEC-376
    #[test]
    fn prefix_of_longer_token_compares_unequal() {
        assert!(!constant_time_eq(b"secret", b"secret-and-more"));
        assert!(!constant_time_eq(b"secret-and-more", b"secret"));
    }

    /// REQ: SEC-376
    #[test]
    fn different_length_slices_with_equal_leading_bytes_compare_unequal() {
        assert!(!constant_time_eq(b"abc\0", b"abc"));
    }

    /// REQ: SEC-376
    #[test]
    fn comparison_is_symmetric() {
        let pairs: [(&[u8], &[u8]); 4] = [
            (b"token-a", b"token-b"),
            (b"same", b"same"),
            (b"", b"x"),
            (b"longer-value", b"longer-valu"),
        ];
        for (left, right) in pairs {
            assert_eq!(constant_time_eq(left, right), constant_time_eq(right, left));
        }
    }

    /// REQ: SEC-376
    #[test]
    fn high_bit_and_non_ascii_bytes_compare_by_value() {
        let left = [0x80u8, 0xC3, 0xA9, 0xFF];
        let right = [0x80u8, 0xC3, 0xA9, 0xFE];
        assert!(constant_time_eq(&left, &left));
        assert!(!constant_time_eq(&left, &right));
    }
}
