//! Fresh reads of the hourly scanner's atomic publication. No model fallback.
use crate::GatewayError;
use serde::Deserialize;

#[derive(Deserialize)]
struct Scan {
    schema_version: u32,
    #[serde(default)]
    benchmark_version: Option<String>,
    recommendations: Recommendations,
    models: Vec<Model>,
}
#[derive(Deserialize)]
struct Recommendations {
    base: Option<String>,
}
#[derive(Deserialize)]
struct Model {
    id: String,
    context_length: u64,
    max_output_tokens: Option<u32>,
    text_completed: u32,
    text_score: u32,
    #[serde(flatten)]
    evidence: serde_json::Map<String, serde_json::Value>,
}

pub(crate) struct RecommendedModel {
    pub id: String,
    pub max_output_tokens: Option<u32>,
}

pub(crate) fn read_base() -> Result<RecommendedModel, GatewayError> {
    let directories = directories::BaseDirs::new().ok_or_else(|| {
        GatewayError::InvalidResponse("cannot resolve model-scanner home directory".into())
    })?;
    let bytes =
        std::fs::read(directories.home_dir().join(".model-scanner/scan.json")).map_err(|_| {
            GatewayError::InvalidResponse("model-scanner publication unavailable".into())
        })?;
    parse_base(&bytes)
}

fn parse_base(bytes: &[u8]) -> Result<RecommendedModel, GatewayError> {
    let scan: Scan = serde_json::from_slice(bytes)
        .map_err(|_| GatewayError::InvalidResponse("invalid model-scanner publication".into()))?;
    let invalid =
        || GatewayError::InvalidResponse("no qualified base model-scanner recommendation".into());
    if !matches!(scan.schema_version, 1..=3)
        || (scan.schema_version == 2 && scan.benchmark_version.as_deref() != Some("2026-10-17.2"))
    {
        return Err(invalid());
    }
    if scan.schema_version == 3 && scan.benchmark_version.as_deref() != Some("2026-10-18.1") {
        return Err(invalid());
    }
    let text_count = if scan.schema_version != 1 { 20 } else { 10 };
    let id = scan.recommendations.base.ok_or_else(invalid)?;
    let mut matches = scan.models.into_iter().filter(|model| model.id == id);
    let model = matches.next().ok_or_else(invalid)?;
    if matches.next().is_some()
        || id.is_empty()
        || model.context_length < 200_000
        || model.text_completed != text_count
        || model.text_score == 0
        || model.text_score > text_count
        || model.max_output_tokens == Some(0)
    {
        return Err(invalid());
    }
    if scan.schema_version == 3 {
        let mut evidence = serde_json::Value::Object(model.evidence.clone());
        evidence["text_score"] = serde_json::json!(model.text_score);
        evidence["context_length"] = serde_json::json!(model.context_length);
        if !has_agentic_evidence(&evidence) {
            return Err(invalid());
        }
    }
    let max_output_tokens = model.max_output_tokens.map(|limit| {
        if id == "dots-studio/dots-3-note-preview:free" {
            limit.min(262_144)
        } else {
            limit
        }
    });
    Ok(RecommendedModel {
        id,
        max_output_tokens,
    })
}

/// Check measured agentic completion and derived totals before trusting a selection.
fn has_agentic_evidence(model: &serde_json::Value) -> bool {
    let Some(answers) = model["agentic"].as_array() else {
        return false;
    };
    let mut ids = std::collections::HashSet::new();
    let graded = answers.iter().all(|answer| {
        answer["question_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty() && ids.insert(id))
            && matches!(answer["status"].as_str(), Some("correct" | "incorrect"))
    });
    let correct = answers
        .iter()
        .filter(|answer| answer["status"].as_str() == Some("correct"))
        .count() as u64;
    let scores = [
        ("text_score", 20),
        ("visual_score", 10),
        ("long_context_score", 5),
        ("agentic_score", 15),
    ];
    let totals_valid = scores
        .iter()
        .all(|(key, limit)| model[*key].as_u64().is_some_and(|score| score <= *limit));
    answers.len() == 15
        && answers.iter().any(|answer| {
            answer["status"].as_str() == Some("correct")
                && answer["tool_calls"].as_u64().is_some_and(|count| count > 0)
        })
        && graded
        && correct > 0
        && model["agentic_completed"].as_u64() == Some(15)
        && model["agentic_score"].as_u64() == Some(correct)
        && totals_valid
        && model["general_score"].as_u64()
            == Some(
                model["text_score"].as_u64().unwrap_or(0)
                    + model["visual_score"].as_u64().unwrap_or(0),
            )
        && model["score"].as_u64()
            == Some(
                scores
                    .iter()
                    .map(|(key, _)| model[*key].as_u64().unwrap_or(0))
                    .sum(),
            )
        && model["score_max"].as_u64()
            == Some(if model["context_length"].as_u64().unwrap_or(0) >= 210000 {
                50
            } else {
                45
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_current_publication_and_no_qualified_model() {
        let publication = |id: &str, context: u64| {
            serde_json::json!({"schema_version":1,"recommendations":{"base":id},"models":[{"id":id,"context_length":context,"max_output_tokens":460800,"text_completed":10,"text_score":10}]}).to_string()
        };
        let first =
            parse_base(publication("dots-studio/dots-3-note-preview:free", 200_000).as_bytes())
                .unwrap();
        assert_eq!(first.max_output_tokens, Some(262_144));
        assert_eq!(
            parse_base(publication("changed", 200_000).as_bytes())
                .unwrap()
                .id,
            "changed"
        );
        assert!(parse_base(publication("short", 100).as_bytes()).is_err());
        assert!(
            parse_base(br#"{"schema_version":1,"recommendations":{"base":null},"models":[]}"#)
                .is_err()
        );
    }
    #[test]
    fn expanded_base_requires_versioned_twenty_question_suite() {
        let mut scan = serde_json::json!({"schema_version":2,"benchmark_version":"2026-10-17.2","recommendations":{"base":"expanded"},"models":[{"id":"expanded","context_length":200000,"max_output_tokens":null,"text_completed":20,"text_score":18}]});
        assert_eq!(
            parse_base(scan.to_string().as_bytes()).unwrap().id,
            "expanded"
        );
        scan["models"][0]["text_completed"] = serde_json::json!(10);
        assert!(parse_base(scan.to_string().as_bytes()).is_err());
        scan["benchmark_version"] = serde_json::json!("unknown");
        assert!(parse_base(scan.to_string().as_bytes()).is_err());
    }
    #[test]
    fn agentic_recommendation_requires_graded_answers_and_consistent_totals() {
        let answers: Vec<_> = (0..15).map(|index| serde_json::json!({"question_id":format!("agentic-{index}"),"status":"correct","tool_calls":1})).collect();
        let mut scan = serde_json::json!({"schema_version":3,"benchmark_version":"2026-10-18.1","recommendations":{"base":"measured"},"models":[{"id":"measured","context_length":1000000,"max_output_tokens":4096,"text_completed":20,"text_score":20,"visual_completed":10,"visual_score":10,"long_context_score":0,"general_score":30,"score":45,"score_max":50,"agentic":answers,"agentic_score":15,"agentic_completed":15}]});
        assert!(parse_base(scan.to_string().as_bytes()).is_ok());
        for answer in scan["models"][0]["agentic"].as_array_mut().unwrap() {
            answer["tool_calls"] = serde_json::json!(0);
        }
        assert!(parse_base(scan.to_string().as_bytes()).is_err());
        scan["models"][0]["agentic"][1]["tool_calls"] = serde_json::json!(1);
        scan["models"][0]["agentic_completed"] = serde_json::json!(14);
        assert!(parse_base(scan.to_string().as_bytes()).is_err());
        scan["models"][0]["agentic_completed"] = serde_json::json!(15);
        scan["models"][0]["agentic"][0]["status"] = serde_json::json!("unavailable");
        assert!(parse_base(scan.to_string().as_bytes()).is_err());
        scan["models"][0]["agentic"][0]["status"] = serde_json::json!("incorrect");
        assert!(parse_base(scan.to_string().as_bytes()).is_err());
        scan["models"][0]["agentic_score"] = serde_json::json!(14);
        scan["models"][0]["score"] = serde_json::json!(44);
        assert!(parse_base(scan.to_string().as_bytes()).is_ok());
        scan["models"][0]["score"] = serde_json::json!(50);
        assert!(parse_base(scan.to_string().as_bytes()).is_err());
        scan["benchmark_version"] = serde_json::json!("unknown");
        assert!(parse_base(scan.to_string().as_bytes()).is_err());
    }
}
