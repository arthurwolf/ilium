use std::sync::OnceLock;

use super::HelpTopic;

const INSTRUCTION_TOPICS_JSON: &str = include_str!("catalog/instructions.json");
const TOPICS_JSON: &str = include_str!("catalog/topics.json");
static TOPICS: OnceLock<Vec<HelpTopic>> = OnceLock::new();

/// Returns the bundled settings-help catalog in stable ID order.
pub fn all() -> &'static [HelpTopic] {
    TOPICS.get_or_init(|| {
        // This is a checked-in, test-covered asset. Invalid JSON is a source
        // defect, not a recoverable runtime condition, so fail at first use.
        let mut topics: Vec<HelpTopic> = serde_json::from_str(TOPICS_JSON)
            .expect("bundled settings help topics must remain valid JSON");
        let instructions: Vec<HelpTopic> = serde_json::from_str(INSTRUCTION_TOPICS_JSON)
            .expect("bundled instruction help topics must remain valid JSON");
        topics.extend(instructions);
        topics.sort_by(|left, right| left.id.cmp(&right.id));
        topics
    })
}

pub fn by_id(id: &str) -> Option<&'static HelpTopic> {
    all()
        .binary_search_by(|topic| topic.id.as_str().cmp(id))
        .ok()
        .map(|index| &all()[index])
}
