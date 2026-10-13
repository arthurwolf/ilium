//! The contract version and the `ilium:api-version` custom section (reference-abi-evolution.md).

use crate::manifest::Diagnostic;

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
        unimplemented!("api::version::parse")
    }

    /// Decodes the 6-byte payload of `ilium:api-version` (ABI-004); a wrong size is
    /// `package.api-section-invalid` (reference-manifest-schema.md).
    pub fn from_custom_section(bytes: &[u8]) -> Result<ApiVersion, Diagnostic> {
        unimplemented!("api::version::from_custom_section")
    }

    /// The versions this host loads: the current minor and the previous one (ABI-021).
    pub fn host_window() -> std::ops::RangeInclusive<ApiVersion> {
        unimplemented!("api::version::host_window")
    }
}

impl std::fmt::Display for ApiVersion {
    /// `X.Y.Z` (reference-abi-evolution.md).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}
