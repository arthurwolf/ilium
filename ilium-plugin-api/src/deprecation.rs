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
pub fn notice_is_long_enough(feature: &MarkedFeature) -> bool {
    feature.removal_minor >= feature.marked_in_minor.saturating_add(2)
}

/// True while the host minor is below the removal minor, so the feature still works (ABI-053).
pub fn feature_available(feature: &MarkedFeature, host_minor: u32) -> bool {
    host_minor < feature.removal_minor
}

/// One record per distinct marked feature the plugin uses; unmarked features give none (ABI-053).
pub fn deprecation_records(
    plugin_id: &str,
    used_features: &[&str],
    marked: &[MarkedFeature],
    host_minor: u32,
) -> Vec<DeprecationRecord> {
    let mut records: Vec<DeprecationRecord> = Vec::new();
    for used in used_features {
        let Some(feature) = marked.iter().find(|feature| feature.name == *used) else {
            continue;
        };
        if !feature_available(feature, host_minor) || records.iter().any(|record| record.feature == *used) {
            continue;
        }
        records.push(DeprecationRecord {
            plugin_id: plugin_id.to_owned(),
            feature: feature.name.clone(),
            removal_minor: feature.removal_minor,
        });
    }
    records
}

/// The section 4 refusal code a package gets for a feature the host no longer offers at
/// `host_minor`, or `None` while the feature still works (ABI-054).
pub fn refusal_for_feature(feature: &MarkedFeature, host_minor: u32) -> Option<&'static str> {
    if feature_available(feature, host_minor) {
        None
    } else {
        Some("package.api-unsupported")
    }
}
