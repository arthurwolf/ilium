//! Output formats: JSON, YAML and Markdown from one JSON value.
//!
//! YAML is written here rather than through a dependency. Every JSON document
//! is valid YAML 1.2, so strings are always emitted as JSON string literals
//! (double-quoted, escaped) and only structure uses YAML block syntax. That
//! keeps the writer small and removes every quoting ambiguity ("yes", "1e3",
//! leading colons) at the cost of quotes around some plain words.

use serde_json::Value;
use std::fmt::Write as _;

/// A rendered document format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Pretty-printed JSON.
    Json,
    /// One compact JSON document per line.
    Jsonl,
    Yaml,
    Markdown,
}

impl Format {
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "jsonl" | "ndjson" => Some(Self::Jsonl),
            "yaml" | "yml" => Some(Self::Yaml),
            "markdown" | "md" => Some(Self::Markdown),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Jsonl => "jsonl",
            Self::Yaml => "yaml",
            Self::Markdown => "markdown",
        }
    }

    pub const fn content_type(self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::Jsonl => "application/x-ndjson",
            Self::Yaml => "application/yaml",
            Self::Markdown => "text/markdown; charset=utf-8",
        }
    }
}

/// What the value describes, so Markdown can pick a layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `tree_view`: a session with `nodes`.
    Tree,
    /// `node_view`: one node and its subtree.
    Node,
    /// Anything else (lists, results): Markdown falls back to a key list.
    Generic,
}

/// Renders `value` in `format`. The result always ends with a newline.
pub fn render_document(value: &Value, format: Format, shape: Shape) -> String {
    let mut text = match format {
        Format::Json => serde_json::to_string_pretty(value).unwrap_or_default(),
        Format::Jsonl => serde_json::to_string(value).unwrap_or_default(),
        Format::Yaml => yaml(value),
        Format::Markdown => markdown(value, shape),
    };
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn is_plain_key(key: &str) -> bool {
    let mut characters = key.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn scalar(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => serde_json::to_string(text).unwrap_or_default(),
        Value::Array(_) => "[]".to_owned(),
        Value::Object(_) => "{}".to_owned(),
    }
}

fn is_empty_collection(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

/// Writes `value` as a YAML document.
pub fn yaml(value: &Value) -> String {
    let mut out = String::new();
    if matches!(value, Value::Array(_) | Value::Object(_)) && !is_empty_collection(value) {
        yaml_block(value, 0, &mut out);
    } else {
        let _ = writeln!(out, "{}", scalar(value));
    }
    out
}

fn yaml_key(key: &str) -> String {
    if is_plain_key(key) {
        key.to_owned()
    } else {
        serde_json::to_string(key).unwrap_or_default()
    }
}

fn yaml_block(value: &Value, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let key = yaml_key(key);
                if matches!(child, Value::Array(_) | Value::Object(_))
                    && !is_empty_collection(child)
                {
                    let _ = writeln!(out, "{pad}{key}:");
                    // Sequences under a mapping key stay at the key's indent + 2
                    // so every list item lines up with its siblings.
                    yaml_block(child, indent + 2, out);
                } else {
                    let _ = writeln!(out, "{pad}{key}: {}", scalar(child));
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                if matches!(item, Value::Array(_) | Value::Object(_)) && !is_empty_collection(item)
                {
                    let mut nested = String::new();
                    yaml_block(item, indent + 2, &mut nested);
                    // First line of a nested block rides on the dash.
                    let trimmed = nested
                        .strip_prefix(&" ".repeat(indent + 2))
                        .unwrap_or(&nested);
                    let _ = write!(out, "{pad}- {trimmed}");
                } else {
                    let _ = writeln!(out, "{pad}- {}", scalar(item));
                }
            }
        }
        _ => {
            let _ = writeln!(out, "{pad}{}", scalar(value));
        }
    }
}

/// Writes `value` as a Markdown outline for a person or an LLM to read.
pub fn markdown(value: &Value, shape: Shape) -> String {
    let mut out = String::new();
    match shape {
        Shape::Tree => {
            let session = value["session"].as_str().unwrap_or("?");
            let _ = writeln!(
                out,
                "# Session `{session}`\n\n{} panes in {} projects.\n",
                value["pane_count"], value["project_count"]
            );
            if let Some(nodes) = value["nodes"].as_array() {
                for node in nodes {
                    markdown_node(node, 0, &mut out);
                }
            }
        }
        Shape::Node => markdown_node(value, 0, &mut out),
        Shape::Generic => markdown_generic(value, 0, &mut out),
    }
    out
}

fn markdown_node(node: &Value, level: usize, out: &mut String) {
    let pad = "  ".repeat(level);
    let kind = node["kind"].as_str().unwrap_or("node");
    let id = &node["id"];
    let name = node["name"].as_str().unwrap_or("");
    let mut line = format!("{pad}- **{kind} {id}** {name}");
    if let Some(path) = node["path"].as_str() {
        let _ = write!(line, " `{path}`");
    }
    if let Some(pane) = node.get("pane") {
        let status = &pane["status"];
        let describe = match status["kind"].as_str().unwrap_or("?") {
            "agent" => format!(
                "{} {}",
                status["agent"].as_str().unwrap_or("agent"),
                status["state"].as_str().unwrap_or("?")
            ),
            other => other.to_owned(),
        };
        let _ = write!(line, " ({describe})");
        if let Some(cwd) = pane["cwd"].as_str() {
            let _ = write!(line, " in `{cwd}`");
        }
        if let Some(branch) = pane["workspace"]["branch"].as_str() {
            let _ = write!(line, " on branch `{branch}`");
        }
    }
    if node["is_bookmarked"].as_bool() == Some(true) {
        line.push_str(" [bookmarked]");
    }
    let _ = writeln!(out, "{line}");
    if let Some(children) = node["children"].as_array() {
        for child in children {
            markdown_node(child, level + 1, out);
        }
    } else if let Some(ids) = node["child_ids"].as_array() {
        if !ids.is_empty() {
            let list: Vec<String> = ids.iter().map(Value::to_string).collect();
            let _ = writeln!(out, "{pad}  - children not expanded: {}", list.join(", "));
        }
    }
}

fn markdown_generic(value: &Value, level: usize, out: &mut String) {
    let pad = "  ".repeat(level);
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if matches!(child, Value::Object(_) | Value::Array(_))
                    && !is_empty_collection(child)
                {
                    let _ = writeln!(out, "{pad}- **{key}**:");
                    markdown_generic(child, level + 1, out);
                } else {
                    let _ = writeln!(out, "{pad}- **{key}**: {}", plain(child));
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                if matches!(item, Value::Object(_) | Value::Array(_)) && !is_empty_collection(item)
                {
                    let _ = writeln!(out, "{pad}-");
                    markdown_generic(item, level + 1, out);
                } else {
                    let _ = writeln!(out, "{pad}- {}", plain(item));
                }
            }
        }
        other => {
            let _ = writeln!(out, "{pad}{}", plain(other));
        }
    }
}

fn plain(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "none".to_owned(),
        Value::Array(_) => "(empty list)".to_owned(),
        Value::Object(_) => "(empty)".to_owned(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn format_names_round_trip() {
        for name in ["json", "jsonl", "yaml", "markdown"] {
            assert_eq!(Format::parse(name).unwrap().name(), name);
        }
        assert_eq!(Format::parse("md"), Some(Format::Markdown));
        assert_eq!(Format::parse("toml"), None);
    }

    #[test]
    fn yaml_nests_maps_and_lists() {
        let value = json!({"a": 1, "list": [{"x": "y", "n": null}, 2], "empty": [], "q k": "yes"});
        let text = yaml(&value);
        assert!(text.contains("a: 1\n"));
        assert!(text.contains("list:\n  - n: null\n    x: \"y\"\n  - 2\n"));
        assert!(text.contains("empty: []\n"));
        assert!(text.contains("\"q k\": \"yes\"\n"));
    }

    #[test]
    fn yaml_escapes_special_strings() {
        let text = yaml(&json!({"s": "line\nbreak: \"quoted\""}));
        assert_eq!(text, "s: \"line\\nbreak: \\\"quoted\\\"\"\n");
    }

    #[test]
    fn markdown_outlines_a_tree() {
        let value = json!({
            "session": "default", "pane_count": 1, "project_count": 1,
            "nodes": [{"kind": "project", "id": 1, "name": "app", "path": "/x",
                "children": [{"kind": "pane", "id": 2, "name": "sh",
                    "pane": {"status": {"kind": "shell"}, "cwd": "/x"}}]}]
        });
        let text = markdown(&value, Shape::Tree);
        assert!(text.starts_with("# Session `default`"));
        assert!(text.contains("- **project 1** app `/x`\n"));
        assert!(text.contains("  - **pane 2** sh (shell) in `/x`\n"));
    }

    #[test]
    fn rendered_documents_end_with_a_newline() {
        let value = json!({"a": 1});
        for format in [Format::Json, Format::Jsonl, Format::Yaml, Format::Markdown] {
            assert!(render_document(&value, format, Shape::Generic).ends_with('\n'));
        }
    }
}
