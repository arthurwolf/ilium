//! The `ilium.core.diagnostics/report` service as pure state: `warn`, `section` and `build`. The host passes
//! the caller and the clock in; nothing here reads either.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::server_debug::redacted_diagnostic_field_value;
use crate::warnings::Warnings;

/// A report is plain text of at most this many bytes.
pub const MAX_REPORT_BYTES: usize = 256 * 1024;
/// Lines one section may hold, and sections one report may hold.
pub const MAX_SECTION_LINES: usize = 200;
pub const MAX_SECTIONS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Section {
    title: String,
    lines: Vec<String>,
}

/// State of the report service of one side.
#[derive(Debug, Default)]
pub struct ReportService {
    warnings: Warnings,
    sections: Vec<Section>,
}

#[derive(Deserialize)]
struct WarnParams {
    message: String,
    #[serde(default)]
    at_millis: i64,
}

#[derive(Deserialize)]
struct SectionParams {
    title: String,
    #[serde(default)]
    lines: Vec<String>,
}

#[derive(Deserialize, Default)]
struct BuildParams {
    #[serde(default)]
    include_log_tail: bool,
    #[serde(default)]
    log_tail: Vec<String>,
}

impl ReportService {
    pub fn warnings(&self) -> &Warnings {
        &self.warnings
    }

    /// Answers one service call; the error text is the message of an `invalid-argument` reply.
    pub fn handle(
        &mut self,
        caller: &str,
        method: &str,
        payload_json: &str,
    ) -> Result<String, String> {
        let payload: Value = if payload_json.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(payload_json)
                .map_err(|error| format!("invalid payload: {error}"))?
        };
        match method {
            "warn" => {
                let params: WarnParams = serde_json::from_value(payload)
                    .map_err(|error| format!("warn needs a message: {error}"))?;
                self.warnings
                    .push(caller, &params.message, params.at_millis);
                Ok("{}".to_string())
            }
            "section" => {
                let params: SectionParams = serde_json::from_value(payload)
                    .map_err(|error| format!("section needs a title and lines: {error}"))?;
                self.set_section(params);
                Ok("{}".to_string())
            }
            "build" => {
                let params: BuildParams = serde_json::from_value(payload)
                    .map_err(|error| format!("invalid build parameters: {error}"))?;
                Ok(json!({ "text": self.build(&params) }).to_string())
            }
            other => Err(format!("unknown method: {other}")),
        }
    }

    fn set_section(&mut self, params: SectionParams) {
        let lines: Vec<String> = params.lines.into_iter().take(MAX_SECTION_LINES).collect();
        if let Some(existing) = self
            .sections
            .iter_mut()
            .find(|section| section.title == params.title)
        {
            existing.lines = lines;
        } else if self.sections.len() < MAX_SECTIONS {
            self.sections.push(Section {
                title: params.title,
                lines,
            });
        }
    }

    fn build(&self, params: &BuildParams) -> String {
        let mut text = String::from("ILIUM DIAGNOSTICS REPORT\n");
        for section in &self.sections {
            text.push_str(&format!("\n== {} ==\n", single_line(&section.title)));
            for line in &section.lines {
                text.push_str(&redact_line(line));
                text.push('\n');
            }
        }
        text.push_str("\n== Warnings ==\n");
        if self.warnings.is_empty() {
            text.push_str("none\n");
        }
        for warning in self.warnings.entries() {
            text.push_str(&format!(
                "[{}] {}\n",
                single_line(&warning.plugin),
                redact_line(&warning.message)
            ));
        }
        if params.include_log_tail {
            text.push_str("\n== Log tail ==\n");
            for line in &params.log_tail {
                text.push_str(&redact_line(line));
                text.push('\n');
            }
        }
        truncate_to_bytes(text, MAX_REPORT_BYTES)
    }
}

fn single_line(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

/// A `label: value` line whose label names a credential keeps the label only.
fn redact_line(line: &str) -> String {
    let line = single_line(line);
    match line.split_once(':') {
        Some((label, value)) => {
            let redacted = redacted_diagnostic_field_value(label.trim(), value.trim());
            if redacted == value.trim() {
                line
            } else {
                format!("{label}: {redacted}")
            }
        }
        None => line,
    }
}

fn truncate_to_bytes(mut text: String, limit: usize) -> String {
    if text.len() <= limit {
        return text;
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warn_section_and_build_round_trip_into_one_report() {
        let mut service = ReportService::default();
        assert_eq!(
            service.handle("a.b", "warn", r#"{"message":"disk almost full"}"#),
            Ok("{}".into())
        );
        assert_eq!(
            service.handle(
                "a.b",
                "section",
                r#"{"title":"Platform","lines":["os: linux"]}"#
            ),
            Ok("{}".into())
        );
        let reply = service.handle("a.b", "build", "{}").expect("build");
        let text = serde_json::from_str::<Value>(&reply).expect("json")["text"]
            .as_str()
            .expect("text")
            .to_string();
        assert!(text.contains("== Platform ==\nos: linux\n"));
        assert!(text.contains("[a.b] disk almost full"));
        assert!(!text.contains("Log tail"));
    }

    #[test]
    fn secrets_never_reach_a_report() {
        let mut service = ReportService::default();
        service
            .handle(
                "p",
                "section",
                r#"{"title":"Keys","lines":["API key: sk-secret","user: me"]}"#,
            )
            .expect("section");
        let reply = service.handle("p", "build", "{}").expect("build");
        assert!(!reply.contains("sk-secret"));
        assert!(reply.contains("user: me"));
    }

    #[test]
    fn a_report_is_capped_at_256_kib_and_unknown_methods_fail() {
        let mut service = ReportService::default();
        let long = "x".repeat(10_000);
        for index in 0..MAX_SECTIONS {
            let lines: Vec<String> = (0..MAX_SECTION_LINES).map(|_| long.clone()).collect();
            let params = json!({ "title": format!("s{index}"), "lines": lines });
            service
                .handle("p", "section", &params.to_string())
                .expect("section");
        }
        let reply = service.handle("p", "build", "{}").expect("build");
        let text = serde_json::from_str::<Value>(&reply).expect("json")["text"]
            .as_str()
            .expect("text")
            .len();
        assert!(text <= MAX_REPORT_BYTES);
        assert!(service.handle("p", "nope", "{}").is_err());
        assert!(service.handle("p", "warn", "{}").is_err());
    }

    #[test]
    fn a_section_title_replaces_its_previous_lines() {
        let mut service = ReportService::default();
        service
            .handle("p", "section", r#"{"title":"T","lines":["one"]}"#)
            .expect("first");
        service
            .handle("p", "section", r#"{"title":"T","lines":["two"]}"#)
            .expect("second");
        let reply = service
            .handle(
                "p",
                "build",
                r#"{"include_log_tail":true,"log_tail":["tail line"]}"#,
            )
            .expect("build");
        assert!(reply.contains("two") && !reply.contains("one"));
        assert!(reply.contains("tail line"));
    }
}
