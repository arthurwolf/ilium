//! Resolve model references to text captured from the terminal, never model-authored text.

use serde_json::{json, Value};

const MAX_RECORD_BYTES: usize = 64 * 1024;
const MAX_PARTS: usize = 128;
const MAX_CANDIDATES: usize = 2048;

pub(crate) struct ScreenSnapshot {
    rows: Vec<String>,
}

impl ScreenSnapshot {
    pub(crate) fn new(rows: Vec<String>) -> Self {
        Self { rows }
    }

    pub(crate) fn prompt(&self) -> String {
        let lines: Vec<_> = self.rows.iter().enumerate().map(|(index, text)| {
            let words: Vec<_> = words(text).iter().enumerate().map(|(word, &(start, end))| {
                json!({"id": format!("w{}", word + 1), "text": &text[start..end]})
            }).collect();
            json!({"id": index + 1, "text": text, "words": words})
        }).collect();
        serde_json::to_string(&lines).unwrap_or_default()
    }

    pub(crate) fn resolve(&self, response: &str) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        response.lines().filter(|line| line.len() <= MAX_RECORD_BYTES)
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter_map(|record| self.resolve_record(&record))
            .filter(|text| seen.insert(text.clone()))
            .take(MAX_CANDIDATES).collect()
    }

    fn resolve_record(&self, record: &Value) -> Option<String> {
        let object = record.as_object()?;
        if object.len() != 3 { return None; }
        valid_label(object.get("label")?.as_str()?, 80)?;
        valid_label(object.get("kind")?.as_str()?, 40)?;
        let parts = object.get("parts")?.as_array()?;
        if parts.is_empty() || parts.len() > MAX_PARTS { return None; }
        let mut spans = Vec::new();
        for part in parts {
            let part = part.as_object()?;
            if let Some(lines) = part.get("lines") {
                if part.len() != 1 { return None; }
                let lines = lines.as_array()?;
                if lines.is_empty() || lines.len() > MAX_PARTS { return None; }
                for line in lines {
                    let row = self.row(line.as_u64()?)?;
                    spans.push((row, 0, self.rows[row].trim_end().len()));
                }
            } else {
                if part.keys().any(|key| !matches!(key.as_str(), "line" | "from" | "through")) { return None; }
                let row = self.row(part.get("line")?.as_u64()?)?;
                let text = &self.rows[row];
                let from = match part.get("from") { Some(value) => Some(value.as_str()?), None => None };
                let through = match part.get("through") { Some(value) => Some(value.as_str()?), None => None };
                if from.is_none() && through.is_none() {
                    spans.push((row, 0, text.trim_end().len()));
                } else {
                    let words = words(text);
                    let first = match from { Some(id) => word_index(id, words.len())?, None => 0 };
                    let last = match through { Some(id) => word_index(id, words.len())?, None => words.len().checked_sub(1)? };
                    if first > last { return None; }
                    spans.push((row, words.get(first)?.0, words.get(last)?.1));
                }
            }
            if spans.len() > MAX_PARTS * MAX_PARTS { return None; }
        }
        spans.sort_unstable();
        spans.dedup();
        let text = spans.into_iter().map(|(row, start, end)| &self.rows[row][start..end]).collect::<Vec<_>>().join("\n");
        (!text.trim().is_empty()).then_some(text)
    }

    fn row(&self, line: u64) -> Option<usize> {
        let row = usize::try_from(line.checked_sub(1)?).ok()?;
        (row < self.rows.len()).then_some(row)
    }
}

fn valid_label(value: &str, limit: usize) -> Option<()> {
    (!value.trim().is_empty() && value.chars().count() <= limit && !value.chars().any(|c| c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))).then_some(())
}

fn word_index(id: &str, count: usize) -> Option<usize> {
    let index = id.strip_prefix('w')?.parse::<usize>().ok()?.checked_sub(1)?;
    (index < count && format!("w{}", index + 1) == id).then_some(index)
}

fn words(text: &str) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let mut start = None;
    for (index, character) in text.char_indices() {
        if character.is_whitespace() {
            if let Some(first) = start.take() { result.push((first, index)); }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    if let Some(first) = start { result.push((first, text.len())); }
    result
}
