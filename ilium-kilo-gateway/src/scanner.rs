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
    if !matches!(scan.schema_version, 1 | 2)
        || (scan.schema_version == 2 && scan.benchmark_version.as_deref() != Some("2026-10-17.2"))
    {
        return Err(invalid());
    }
    let text_count = if scan.schema_version == 2 { 20 } else { 10 };
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
}
