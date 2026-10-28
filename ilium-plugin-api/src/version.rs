//! The contract version and the `ilium:api-version` custom section (reference-abi-evolution.md).

use crate::manifest::{Diagnostic, Stage};
use crate::Code;

/// The umbrella contract identity of this API (reference-abi-evolution.md).
pub const CONTRACT: &str = "ilium:plugin@0.1.0";

/// Name of the custom section every component carries (reference-abi-evolution.md, ABI-004).
pub const API_VERSION_SECTION: &str = "ilium:api-version";

/// ABI-004: major u16 BE, minor u16 BE, patch u16 BE.
pub const API_VERSION_BYTES: [u8; 6] = [0, 0, 0, 1, 0, 0];

/// The API version of this crate (reference-abi-evolution.md).
pub const CURRENT: ApiVersion = ApiVersion {
    major: 0,
    minor: 1,
    patch: 0,
};

/// ABI-021: the refusal codes about the API, in the order the host checks them; a package gets
/// only the first failing one (reference-abi-evolution.md section 4).
pub const REFUSAL_ORDER: [&str; 5] = [
    crate::codes::manifest::SCHEMA,
    crate::codes::package::API_UNSUPPORTED,
    crate::codes::manifest::API_VERSION,
    crate::codes::manifest::IMPORT_NOT_REQUESTED,
    crate::codes::manifest::GRANT_UNKNOWN,
];

/// A contract version `X.Y.Z` (reference-abi-evolution.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApiVersion {
    /// Major number.
    pub major: u16,
    /// Minor number; the host window counts minors (reference-abi-evolution.md section 4).
    pub minor: u16,
    /// Patch number.
    pub patch: u16,
}

impl ApiVersion {
    /// Parses `ilium:plugin@X.Y.Z`; anything else is `manifest.schema` (reference-manifest.md PKG-013).
    pub fn parse(contract: &str) -> Result<ApiVersion, Diagnostic> {
        let invalid = || {
            let mut diagnostic = Diagnostic::error(
                Code::from_static(crate::codes::manifest::SCHEMA),
                "/api",
                "api must be ilium:plugin@X.Y.Z with three unsigned 16-bit decimal numbers and no leading zeroes.",
            );
            diagnostic.stage = Some(Stage(3));
            diagnostic
        };
        let version = contract.strip_prefix("ilium:plugin@").ok_or_else(invalid)?;
        let mut parts = version.split('.');
        let mut number = || -> Result<u16, Diagnostic> {
            let part = parts.next().ok_or_else(invalid)?;
            if part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || !part.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(invalid());
            }
            part.parse().map_err(|_| invalid())
        };
        let result = ApiVersion {
            major: number()?,
            minor: number()?,
            patch: number()?,
        };
        if parts.next().is_some() {
            return Err(invalid());
        }
        Ok(result)
    }

    /// Decodes the 6-byte payload of `ilium:api-version` (ABI-004); a wrong size is
    /// `package.api-section-invalid` (reference-manifest-schema.md).
    pub fn from_custom_section(bytes: &[u8]) -> Result<ApiVersion, Diagnostic> {
        let [major_high, major_low, minor_high, minor_low, patch_high, patch_low] = bytes else {
            let mut diagnostic = Diagnostic::error(
                Code::from_static(crate::codes::package::API_SECTION_INVALID),
                "",
                format!(
                    "ilium:api-version payload is {} bytes; expected exactly 6.",
                    bytes.len()
                ),
            );
            diagnostic.stage = Some(Stage(6));
            return Err(diagnostic);
        };
        Ok(ApiVersion {
            major: u16::from_be_bytes([*major_high, *major_low]),
            minor: u16::from_be_bytes([*minor_high, *minor_low]),
            patch: u16::from_be_bytes([*patch_high, *patch_low]),
        })
    }

    /// The versions this host loads: the current minor and the previous one (ABI-021).
    /// Patch releases do not change interfaces, so every representable patch is included.
    /// At minor zero the window stays in the current major, without wrapping backwards.
    pub fn host_window() -> std::ops::RangeInclusive<ApiVersion> {
        ApiVersion {
            major: CURRENT.major,
            minor: CURRENT.minor.saturating_sub(1),
            patch: 0,
        }..=ApiVersion {
            major: CURRENT.major,
            minor: CURRENT.minor,
            patch: u16::MAX,
        }
    }
}

/// The versions this host loads (ABI-021); the free-function form of [`ApiVersion::host_window`].
pub fn host_window() -> std::ops::RangeInclusive<ApiVersion> {
    ApiVersion::host_window()
}

impl std::fmt::Display for ApiVersion {
    /// `X.Y.Z` (reference-abi-evolution.md).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes;

    fn version(major: u16, minor: u16, patch: u16) -> ApiVersion {
        ApiVersion {
            major,
            minor,
            patch,
        }
    }

    /// Returns the refusal code of a failed parse, and panics when the parse succeeds.
    fn refusal_code(result: Result<ApiVersion, Diagnostic>, input: &str) -> String {
        match result {
            Ok(parsed) => panic!("`{input}` must be refused, but parsed as {parsed}"),
            Err(diagnostic) => diagnostic.code.as_str().to_string(),
        }
    }

    /// REQ: PKG-013
    #[test]
    fn parse_accepts_the_contract_identity() {
        assert_eq!(
            ApiVersion::parse("ilium:plugin@0.1.0"),
            Ok(version(0, 1, 0))
        );
    }

    /// REQ: PKG-013
    #[test]
    fn parse_of_contract_constant_equals_current_version() {
        assert_eq!(ApiVersion::parse(CONTRACT), Ok(CURRENT));
    }

    /// REQ: PKG-013
    #[test]
    fn parse_accepts_any_well_formed_version_including_numeric_bounds() {
        assert_eq!(
            ApiVersion::parse("ilium:plugin@0.3.12"),
            Ok(version(0, 3, 12))
        );
        assert_eq!(
            ApiVersion::parse("ilium:plugin@65535.65535.65535"),
            Ok(version(65535, 65535, 65535))
        );
    }

    /// REQ: PKG-013
    #[test]
    fn parse_refuses_wrong_scheme_or_missing_prefix_with_manifest_schema() {
        let inputs = [
            "ilium:plugins@0.1.0",
            "other:plugin@0.1.0",
            "ilium:plugin/0.1.0",
            "0.1.0",
        ];
        for input in inputs {
            assert_eq!(
                refusal_code(ApiVersion::parse(input), input),
                codes::manifest::SCHEMA,
                "`{input}` is not an `ilium:plugin@X.Y.Z` string"
            );
        }
    }

    /// REQ: PKG-013
    #[test]
    fn parse_refuses_malformed_numbers_with_manifest_schema() {
        let inputs = [
            "ilium:plugin@0.1",
            "ilium:plugin@0.1.0.0",
            "ilium:plugin@0.1.x",
            "ilium:plugin@v0.1.0",
            "ilium:plugin@-1.0.0",
            "ilium:plugin@ 0.1.0",
            "ilium:plugin@0.1.0 ",
            "ilium:plugin@0.1.0-rc.1",
            "ilium:plugin@65536.0.0",
        ];
        for input in inputs {
            assert_eq!(
                refusal_code(ApiVersion::parse(input), input),
                codes::manifest::SCHEMA,
                "`{input}` is not a well-formed X.Y.Z version"
            );
        }
    }

    /// REQ: PKG-013
    #[test]
    fn parse_refuses_empty_input_with_manifest_schema() {
        assert_eq!(
            refusal_code(ApiVersion::parse(""), ""),
            codes::manifest::SCHEMA
        );
    }

    /// REQ: ABI-004
    #[test]
    fn section_bytes_0_0_0_1_0_0_decode_to_version_0_1_0() {
        assert_eq!(
            ApiVersion::from_custom_section(&[0, 0, 0, 1, 0, 0]),
            Ok(version(0, 1, 0))
        );
    }

    /// REQ: ABI-004
    #[test]
    fn section_bytes_constant_round_trips_to_current_version() {
        assert_eq!(
            ApiVersion::from_custom_section(&API_VERSION_BYTES),
            Ok(CURRENT)
        );
    }

    /// REQ: PKG-139
    #[test]
    fn section_fields_are_big_endian_major_minor_patch() {
        assert_eq!(
            ApiVersion::from_custom_section(&[0, 2, 0, 3, 0, 4]),
            Ok(version(2, 3, 4))
        );
        assert_eq!(
            ApiVersion::from_custom_section(&[1, 0, 0, 0, 0, 0]),
            Ok(version(256, 0, 0))
        );
    }

    /// REQ: PKG-139
    #[test]
    fn section_all_bytes_set_decodes_to_maximum_version() {
        assert_eq!(
            ApiVersion::from_custom_section(&[0xFF; 6]),
            Ok(version(65535, 65535, 65535))
        );
    }

    /// REQ: PKG-139
    #[test]
    fn section_of_wrong_length_is_refused_as_api_section_invalid() {
        let inputs: [&[u8]; 4] = [
            &[],
            &[0, 0, 0, 1, 0],
            &[0, 0, 0, 1, 0, 0, 0],
            &[0, 0, 0, 1, 0, 0, 0, 0],
        ];
        for input in inputs {
            assert_eq!(
                refusal_code(ApiVersion::from_custom_section(input), "section bytes"),
                codes::package::API_SECTION_INVALID,
                "a section of {} bytes must be refused",
                input.len()
            );
        }
    }

    /// REQ: ABI-021
    #[test]
    fn host_window_contains_current_and_previous_minor() {
        let window = ApiVersion::host_window();
        assert!(window.contains(&CURRENT), "the current 0.1.0 is loaded");
        assert!(
            window.contains(&version(0, 0, 0)),
            "the previous minor 0.0.x is loaded"
        );
    }

    /// REQ: ABI-021
    #[test]
    fn host_window_counts_patches_within_the_current_minor() {
        let window = ApiVersion::host_window();
        assert!(
            window.contains(&version(0, 1, 9)),
            "the window counts minors, so any patch of 0.1 is loaded"
        );
    }

    /// REQ: ABI-021
    #[test]
    fn host_window_excludes_the_next_minor_and_later_majors() {
        let window = ApiVersion::host_window();
        assert!(
            !window.contains(&version(0, 2, 0)),
            "0.2.0 is outside the window"
        );
        assert!(
            !window.contains(&version(0, 3, 0)),
            "0.3.0 is outside the window"
        );
        assert!(
            !window.contains(&version(1, 0, 0)),
            "1.0.0 is outside the window"
        );
    }

    /// REQ: ABI-021
    #[test]
    fn refusal_order_matches_the_documented_check_order() {
        assert_eq!(
            REFUSAL_ORDER,
            [
                "manifest.schema",
                "package.api-unsupported",
                "manifest.api-version",
                "manifest.import-not-requested",
                "manifest.grant-unknown",
            ]
        );
    }
}
