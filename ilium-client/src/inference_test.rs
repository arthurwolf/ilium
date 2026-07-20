//! Safe, explicit provider test used only from Settings. Its fixed prompt is
//! synthetic; it never includes the user's terminal, files, or transcripts.

use std::time::{Duration, Instant};

use ilium_inference::{provider_from_settings, InferenceRequest, InferenceSettings};
use serde::Deserialize;

const TEST_PROMPT: &str = r#"Please title and organize this synthetic work sample: fix login validation, add a regression test, and update the release notes. Return exactly JSON: {"groups":[{"name":"...","panes":["..."]}]} where each group has a concise name and 1-4 concise pane titles."#;
const TEST_MAXIMUM_ATTEMPTS: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceTestResult {
    pub groups: Vec<InferenceTestGroup>,
    pub elapsed: Duration,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceTestGroup {
    pub name: String,
    pub panes: Vec<String>,
}
#[derive(Deserialize)]
struct RawTestResult {
    groups: Vec<RawTestGroup>,
}
#[derive(Deserialize)]
struct RawTestGroup {
    name: String,
    panes: Vec<String>,
}

pub fn run(settings: &InferenceSettings) -> anyhow::Result<InferenceTestResult> {
    let started = Instant::now();
    let provider = provider_from_settings(settings);
    let request = InferenceRequest::json_only(TEST_PROMPT);
    let mut last_parse_error = None;
    for _attempt in 1..=TEST_MAXIMUM_ATTEMPTS {
        let response = provider.complete(&request)?;
        match parse_test_result(&response.text, started.elapsed()) {
            Ok(result) => return Ok(result),
            Err(error) => last_parse_error = Some(error),
        }
    }
    Err(last_parse_error.expect("the non-empty semantic retry loop records an error"))
}

fn parse_test_result(response: &str, elapsed: Duration) -> anyhow::Result<InferenceTestResult> {
    let object = crate::naming::parse_structured_json_object(response, "inference test")?;
    let parsed: RawTestResult = serde_json::from_value(object)?;
    let groups = parsed
        .groups
        .into_iter()
        .filter_map(|group| {
            let name = normalized(&group.name)?;
            let panes = group
                .panes
                .into_iter()
                .filter_map(|pane| normalized(&pane))
                .take(4)
                .collect::<Vec<_>>();
            (!panes.is_empty()).then_some(InferenceTestGroup { name, panes })
        })
        .take(5)
        .collect::<Vec<_>>();
    if groups.is_empty() {
        anyhow::bail!("test response contained no usable groups")
    }
    Ok(InferenceTestResult { groups, elapsed })
}
fn normalized(value: &str) -> Option<String> {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!value.is_empty() && value.chars().count() <= 72).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalization_bounds_labels() {
        assert_eq!(normalized("  Fix   login "), Some("Fix login".to_string()));
        assert_eq!(normalized(" "), None);
    }

    #[test]
    fn parser_accepts_fenced_json_and_trailing_model_text() {
        let response = r#"```json
{"groups":[{"name":"Login fixes","panes":["Validation","Regression tests"]}]}
```
</response-format>"#;

        let parsed = parse_test_result(response, Duration::from_millis(12)).unwrap();

        assert_eq!(parsed.groups[0].name, "Login fixes");
        assert_eq!(parsed.groups[0].panes.len(), 2);
        assert_eq!(parsed.elapsed, Duration::from_millis(12));
    }
}
