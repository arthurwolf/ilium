#![cfg(feature = "testing")]
//! Test-support builders for package specs that the lifecycle-ops rig (`testing::ops`) does not
//! offer yet: a deprecation notice (LIF-319, LIF-320) and a settings schema (LIF-321). Every body
//! is a skeleton stub; the implementation lands with the rig.

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
    fn deprecated(self, _reason: &str, _end_of_life: &str) -> Self {
        unimplemented!(
            "ilium_plugin_host::lifecycle_ops_support::PackageSpecLifecycleExt::deprecated"
        )
    }

    fn with_setting_schema(self, _schema_json: &str) -> Self {
        unimplemented!(
            "ilium_plugin_host::lifecycle_ops_support::PackageSpecLifecycleExt::with_setting_schema"
        )
    }
}
