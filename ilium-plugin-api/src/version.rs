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

impl std::fmt::Display for ApiVersion {
    /// `X.Y.Z` (reference-abi-evolution.md).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}
