//! Notice and removal of host features (reference-abi-evolution.md, "Notice and removal";
//! ABI-052..054). Pure policy: the host calls these at load time and reads no release notes.

/// A host feature that a release has marked for removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkedFeature {
    /// The feature name as plugins use it, for example `host.legacy-clipboard`.
    pub name: String,
    /// The minor version of the release that marked the feature.
    pub marked_in_minor: u32,
    /// The minor version in which the feature stops working.
    pub removal_minor: u32,
}

/// The `deprecated` record of ABI-053: one per plugin and feature, written at load time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeprecationRecord {
    /// The plugin that uses the feature.
    pub plugin_id: String,
    /// The feature the plugin uses.
    pub feature: String,
    /// The minor in which the feature stops working.
    pub removal_minor: u32,
}

/// True when the removal minor is at least two minors after the marking release (ABI-052).
pub fn notice_is_long_enough(_feature: &MarkedFeature) -> bool {
    unimplemented!("api::deprecation::notice_is_long_enough")
}

/// True while the host minor is below the removal minor, so the feature still works (ABI-053).
pub fn feature_available(_feature: &MarkedFeature, _host_minor: u32) -> bool {
    unimplemented!("api::deprecation::feature_available")
}

/// One record per distinct marked feature the plugin uses; unmarked features give none (ABI-053).
pub fn deprecation_records(
    _plugin_id: &str,
    _used_features: &[&str],
    _marked: &[MarkedFeature],
    _host_minor: u32,
) -> Vec<DeprecationRecord> {
    unimplemented!("api::deprecation::deprecation_records")
}

/// The section 4 refusal code a package gets for a feature the host no longer offers at
/// `host_minor`, or `None` while the feature still works (ABI-054).
pub fn refusal_for_feature(_feature: &MarkedFeature, _host_minor: u32) -> Option<&'static str> {
    unimplemented!("api::deprecation::refusal_for_feature")
}
