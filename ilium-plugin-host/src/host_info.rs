//! Host info of the plugin contract: the effective `limits` record (API-094).

/// One limit of the effective `limits` record: its name, its value and where the value came from.
#[derive(Debug, Clone, PartialEq)]
pub struct LimitEntry {
    /// The limit key, for example `pending_requests`.
    pub name: String,
    /// The effective value.
    pub value: u64,
    /// One of `default`, `manifest` or `user`.
    pub source: String,
}

/// The effective limits as the host info reports them.
pub fn limits_record() -> Vec<LimitEntry> {
    ilium_plugin_api::limits::LimitKey::ALL
        .iter()
        .map(|key| LimitEntry {
            name: key.as_key().to_string(),
            value: ilium_plugin_api::limits::default_of(*key),
            source: "default".to_string(),
        })
        .collect()
}
