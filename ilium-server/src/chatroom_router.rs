//! Routes explicit `@name` references in newly appended project chat records
//! to the matching live agent pane.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ilium_ipc::PromptSubmissionSource;

use crate::ipc::handlers::{submit_terminal_text, write_scheduled_key_input};
use crate::pane::PaneResource;
use crate::state::ServerState;

const POLL_INTERVAL: Duration = Duration::from_millis(250);

pub(crate) fn spawn(state: Arc<ServerState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(run(state))
}

async fn run(state: Arc<ServerState>) {
    let path = state.session_cwd.join("CHATROOM.md");
    let mut offset = std::fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    loop {
        if let Ok(metadata) = std::fs::metadata(&path) {
            if metadata.len() < offset {
                offset = 0;
            }
            if metadata.len() > offset {
                match read_new_records(&path, offset) {
                    Ok((next_offset, records)) => {
                        offset = next_offset;
                        for record in records {
                            route_record(&state, &record.author, &record.content).await;
                        }
                    }
                    Err(error) => tracing::warn!(%error, "chatroom reference scan failed"),
                }
            }
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    author: String,
    content: String,
}

fn read_new_records(path: &Path, offset: u64) -> Result<(u64, Vec<Record>), std::io::Error> {
    let bytes = std::fs::read(path)?;
    let start = usize::try_from(offset)
        .unwrap_or(bytes.len())
        .min(bytes.len());
    let tail = &bytes[start..];
    let complete = tail
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let text = std::str::from_utf8(&tail[..complete]).unwrap_or("");
    let records = text.lines().filter_map(parse_record).collect();
    Ok((offset + complete as u64, records))
}

fn parse_record(line: &str) -> Option<Record> {
    let mut fields = line.strip_prefix("- ")?.splitn(3, " | ");
    let _timestamp = fields.next()?;
    let author = unescape(fields.next()?)?;
    let content = unescape(fields.next()?)?;
    Some(Record { author, content })
}

fn unescape(value: &str) -> Option<String> {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            match characters.next()? {
                'n' => output.push('\n'),
                'r' => output.push('\r'),
                't' => output.push('\t'),
                '\\' => output.push('\\'),
                other => output.push(other),
            }
        } else {
            output.push(character);
        }
    }
    Some(output)
}

async fn route_record(state: &Arc<ServerState>, author: &str, content: &str) {
    let references = referenced_names(content);
    if references.is_empty() {
        return;
    }
    let panes = state.panes.read().await;
    let tree = state.tree.read().await;
    let targets = tree
        .panes()
        .filter_map(|node| {
            let name = node.name.strip_prefix('@').unwrap_or(&node.name);
            if !references.iter().any(|reference| reference == name) {
                return None;
            }
            let pane_id = node.id;
            if !matches!(panes.get(&pane_id), Some(PaneResource::Terminal(_))) {
                return None;
            }
            Some((pane_id, node.name.clone()))
        })
        .collect::<Vec<_>>();
    drop(tree);
    drop(panes);
    for (pane_id, target_name) in targets {
        let source_session = source_session_id(state, author).await;
        let called_name = target_name.strip_prefix('@').unwrap_or(&target_name);
        let message = format!(
            "new event: the agent {author} with session {} sent you (called @{called_name}) the following message in the chatroom:\n{content}\n",
            source_session.as_deref().unwrap_or("unknown")
        );
        for line in notification_lines(&message) {
            let result = if line.is_empty() {
                write_scheduled_key_input(
                    state,
                    pane_id,
                    b"\r",
                    Some(PromptSubmissionSource::ScheduledInput),
                )
                .await
            } else {
                submit_terminal_text(state, pane_id, line, PromptSubmissionSource::ScheduledInput)
                    .await
            };
            if let Err(error) = result {
                tracing::warn!(?pane_id, %error, "chatroom reference delivery failed");
                break;
            }
        }
    }
}

fn notification_lines(message: &str) -> impl Iterator<Item = &str> {
    message.split('\n')
}

async fn source_session_id(state: &Arc<ServerState>, author: &str) -> Option<String> {
    let expected = author.strip_prefix('@').unwrap_or(author);
    let tree = state.tree.read().await;
    let panes = state.panes.read().await;
    let source = tree.panes().find_map(|node| {
        let name = node.name.strip_prefix('@').unwrap_or(&node.name);
        if name != expected {
            return None;
        }
        match panes.get(&node.id) {
            Some(PaneResource::Terminal(runtime)) => runtime.session_id.clone(),
            _ => None,
        }
    });
    source
}

fn referenced_names(content: &str) -> Vec<String> {
    content
        .split_whitespace()
        .filter_map(|word| word.strip_prefix('@'))
        .map(|word| {
            word.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != '-' && character != '_'
            })
        })
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_escaped_chat_record_and_multiple_references() {
        let record = parse_record("- t | sender | hello\\n@compiler @reviewer").unwrap();
        assert_eq!(record.content, "hello\n@compiler @reviewer");
        assert_eq!(
            referenced_names(&record.content),
            vec!["compiler", "reviewer"]
        );
    }

    #[test]
    fn notification_lines_preserve_each_enter_boundary() {
        let lines = notification_lines("header\n/goal\n").collect::<Vec<_>>();
        assert_eq!(lines, vec!["header", "/goal", ""]);
    }
}
