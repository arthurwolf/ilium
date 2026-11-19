//! Bounded, process-fenced final replies for monitoring drill-downs.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

use ilium_core::NodeId;
use ilium_execution::{JobCost, Lane};
use ilium_ipc::{MonitoringEventKind, MonitoringQuery};

use crate::pane::{CacheWaitSource, PaneResource};
use crate::state::ServerState;

/// Read only candidate finished panes; no screen or complete transcript dump.
pub(crate) async fn final_replies(
    state: &Arc<ServerState>,
    query: &MonitoringQuery,
) -> HashMap<(NodeId, u64), String> {
    let candidates = {
        let log = state
            .events
            .monitoring
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let after = query
            .since
            .as_deref()
            .and_then(|cursor| {
                crate::monitoring_query::parse_cursor(cursor, &log.epoch().to_string()).ok()
            })
            .unwrap_or(0);
        let mut latest = HashMap::new();
        for event in log.read_after(after).unwrap_or_default() {
            if let Some(pane) = event.pane_id {
                if !matches!(
                    event.kind,
                    MonitoringEventKind::Progress | MonitoringEventKind::ResourceChanged
                ) {
                    latest.insert(pane, event);
                }
            }
        }
        let mut candidates: Vec<_> = latest
            .into_iter()
            .filter(|(_, event)| event.kind == MonitoringEventKind::Done)
            .collect();
        candidates.sort_by_key(|(_, event)| event.cursor);
        candidates.truncate(query.max_items.min(4096));
        candidates
    };
    let mut replies = HashMap::new();
    for (pane, event) in candidates {
        if let Some(text) = read_reply(state, pane, false).await {
            replies.insert((pane, event.cursor), text);
        }
    }
    replies
}

pub(crate) async fn read_reply(
    state: &Arc<ServerState>,
    pane: NodeId,
    latest: bool,
) -> Option<String> {
    let source = {
        let panes = state.panes.read().await;
        let PaneResource::Terminal(runtime) = panes.get(&pane)? else {
            return None;
        };
        runtime.cache_wait_source()?
    };
    let execution = state.execution.get()?.client.clone();
    // The input declaration includes JSON expansion of the bounded tail.
    let result = execution
        .run(
            Lane::Io,
            JobCost {
                input_bytes: 64 * 1024 * 1024,
                result_bytes: 16 * 1024,
            },
            move |context: ilium_execution::JobContext| {
                let text = if context.stop_requested() {
                    None
                } else {
                    read_current_tail(&source, latest).ok().flatten()
                };
                Ok::<_, std::io::Error>((source, text))
            },
        )
        .await
        .ok()?;
    let (source, text) = result.view();
    let panes = state.panes.read().await;
    let PaneResource::Terminal(runtime) = panes.get(&pane)? else {
        return None;
    };
    if !runtime.cache_wait_source_is_current(source) {
        return None;
    }
    text.clone()
}

fn read_current_tail(source: &CacheWaitSource, latest: bool) -> std::io::Result<Option<String>> {
    use ilium_platform::{file_identity, process_control, process_info, secure_fs};
    const TAIL_BYTES: u64 = 1024 * 1024;
    const RECORD_BYTES: usize = 256 * 1024;
    if !process_control::captured_process_is_current(&source.process_identity)? {
        return Ok(None);
    }
    let mut file = secure_fs::open_regular_file(&source.transcript_path)?;
    if !process_info::process_has_open_file(source.process.process_id, &file, 4096)? {
        return Ok(None);
    }
    let identity = file_identity::identity(&file)?;
    let length = file.metadata()?.len();
    let offset = length.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.by_ref().take(TAIL_BYTES).read_to_end(&mut bytes)?;
    if bytes.last() != Some(&b'\n') {
        return Ok(None);
    }
    let mut records = bytes.split(|byte| *byte == b'\n');
    if offset != 0 {
        records.next();
    }
    let mut output = ilium_agent_session::final_output::FinalOutput::default();
    for record in records.filter(|record| !record.is_empty()) {
        if record.len() > RECORD_BYTES {
            return Ok(None);
        }
        let Ok(entry) = serde_json::from_slice::<serde_json::Value>(record) else {
            return Ok(None);
        };
        if source.process.class == ilium_core::AgentClass::Claude {
            if entry["sessionId"]
                .as_str()
                .is_some_and(|id| id != source.session_id)
            {
                return Ok(None);
            }
            if entry["isSidechain"].as_bool() == Some(true) {
                continue;
            }
        }
        output.observe(&source.process.class, &entry);
    }
    let reopened = secure_fs::open_regular_file(&source.transcript_path)?;
    if file_identity::identity(&reopened)? != identity
        || file.metadata()?.len() != length
        || !process_control::captured_process_is_current(&source.process_identity)?
        || !process_info::process_has_open_file(source.process.process_id, &file, 4096)?
    {
        return Ok(None);
    }
    Ok(if latest {
        output.latest_text()
    } else {
        output.text()
    }
    .map(str::to_owned))
}
