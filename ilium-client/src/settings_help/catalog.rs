use std::sync::OnceLock;

use super::HelpTopic;

const TOPICS_JSON: &str = include_str!("catalog/topics.json");
static TOPICS: OnceLock<Vec<HelpTopic>> = OnceLock::new();

/// Returns the bundled settings-help catalog in stable ID order.
pub fn all() -> &'static [HelpTopic] {
    TOPICS.get_or_init(|| {
        // This is a checked-in, test-covered asset. Invalid JSON is a source
        // defect, not a recoverable runtime condition, so fail at first use.
        serde_json::from_str(TOPICS_JSON)
            .expect("bundled settings help topics must remain valid JSON")
    })
}

pub fn by_id(id: &str) -> Option<&'static HelpTopic> {
    all()
        .binary_search_by(|topic| topic.id.as_str().cmp(id))
        .ok()
        .map(|index| &all()[index])
}
