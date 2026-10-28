#![cfg(feature = "testing")]
//! Test-support builders for package specs that the lifecycle-ops rig (`testing::ops`) does not
//! offer yet: a deprecation notice (LIF-319, LIF-320) and a settings schema (LIF-321). The rig's
//! fields are private, so both are carried as tagged content markers that the package builder
//! writes into the manifest (`lifecycle:deprecated:<reason>|<end-of-life>` and
//! `lifecycle:setting-schema:<json>`).

use crate::testing::ops::PackageSpec;

/// Lifecycle builders on [`PackageSpec`] used by the lifecycle-ops tests.
pub trait PackageSpecLifecycleExt: Sized {
    /// Marks the package `deprecated` with a reason and an end-of-life date (`YYYY-MM-DD`), as
    /// the package manifest's `deprecated` object does (LIF-319, LIF-320).
    fn deprecated(self, reason: &str, end_of_life: &str) -> Self;

    /// Declares the package's settings schema as the JSON text of its rows (LIF-321).
    fn with_setting_schema(self, schema_json: &str) -> Self;
}

impl PackageSpecLifecycleExt for PackageSpec {
    fn deprecated(self, reason: &str, end_of_life: &str) -> Self {
        self.content_marker(&format!("lifecycle:deprecated:{reason}|{end_of_life}"))
    }

    fn with_setting_schema(self, schema_json: &str) -> Self {
        self.content_marker(&format!("lifecycle:setting-schema:{schema_json}"))
    }
}
