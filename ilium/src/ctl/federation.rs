//! Cross-session read coordination. Socket identity scopes every result and cursor.
use std::collections::BTreeMap;

use ilium_ipc::{MonitoringFormat, MonitoringQuery};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::task::JoinSet;

use super::{
    args::{CtlCommand, Target},
    CtlFailure,
};
use crate::session::LiveSessionSocket;

type Cursors = BTreeMap<String, String>;

#[derive(Serialize)]
struct SessionResult {
    session: String,
    socket: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    response: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl SessionResult {
    fn new(socket: &LiveSessionSocket, result: Result<Value, CtlFailure>) -> Self {
        let (response, error) = match result {
            Ok(value) => (Some(value), None),
            Err(error) => (None, Some(error.message)),
        };
        Self {
            session: socket
                .session_name_candidates
                .first()
                .cloned()
                .unwrap_or_default(),
            socket: socket.socket_path.to_string_lossy().into_owned(),
            response,
            error,
        }
    }

    fn item_count(&self) -> usize {
        self.response
            .as_ref()
            .and_then(|response| response.get("events").or_else(|| response.get("digest")))
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
    }
}

fn sockets() -> Result<Vec<LiveSessionSocket>, CtlFailure> {
    let sockets = crate::pane_scan::live_sockets()
        .map_err(|error| CtlFailure::no_session(error.to_string()))?;
    // Keep connection and reply retention finite. Refuse the whole scan rather
    // than silently omitting endpoints beyond the admission boundary.
    if sockets.len() > 1024 {
        return Err(CtlFailure::rejected(
            "all-session read exceeds 1024 live endpoints; select explicit sessions",
        ));
    }
    Ok(sockets)
}

pub(super) async fn monitoring(mut query: MonitoringQuery) -> Result<String, CtlFailure> {
    if query.max_items == 0
        || query.max_items > 4096
        || query.max_chars == 0
        || query.max_chars > 1_000_000
    {
        return Err(CtlFailure::usage("invalid output budget"));
    }
    let cursors: Cursors = query
        .since
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| {
            CtlFailure::usage(
                "all-session --since requires a JSON object mapping socket paths to cursors",
            )
        })?
        .unwrap_or_default();
    let sockets = sockets()?;
    let total = sockets.len();
    let format = query.format;
    let (max_items, max_chars, follow) = (query.max_items, query.max_chars, query.follow);
    // A process retains at most 16 MiB of remote reply text before rendering.
    query.max_chars = max_chars.min((16 * 1024 * 1024) / total.max(1));
    query.format = MonitoringFormat::Json;
    let mut tasks = JoinSet::new();
    for socket in sockets {
        let mut selected = query.clone();
        selected.since = cursors
            .get(socket.socket_path.to_string_lossy().as_ref())
            .cloned();
        tasks.spawn(async move {
            let result = super::monitoring_read_at(&socket.socket_path, selected)
                .await
                .and_then(|text| {
                    serde_json::from_str(&text).map_err(|error| {
                        CtlFailure::rejected(format!("invalid monitoring response: {error}"))
                    })
                });
            let mut page = SessionResult::new(&socket, result);
            qualify_drills(&mut page);
            page
        });
    }
    let mut pages = Vec::new();
    while let Some(result) = tasks.join_next().await {
        let page = result
            .map_err(|error| CtlFailure::rejected(format!("session reader failed: {error}")))?;
        let ready = page.response.is_some();
        pages.push(page);
        if follow && ready {
            while let Some(Ok(page)) = tasks.try_join_next() {
                pages.push(page);
            }
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            break;
        }
    }
    if total > 0 && pages.iter().all(|page| page.response.is_none()) {
        return Err(CtlFailure::no_session(
            serde_json::to_string(&json!({"sessions":pages,"cursor":cursors,"more_sessions":0}))
                .map_err(|error| CtlFailure::rejected(error.to_string()))?,
        ));
    }
    render_pages(pages, cursors, total, max_items, max_chars, format)
}

pub(super) async fn read(
    command: CtlCommand,
    target: Target,
    format: ilium_control::Format,
) -> Result<(), CtlFailure> {
    let sockets = sockets()?;
    let total = sockets.len();
    let any = matches!(&command, CtlCommand::Wait { all: false, .. });
    let mut tasks = JoinSet::new();
    for socket in sockets {
        let (command, target) = (command.clone(), target.clone());
        tasks.spawn(async move {
            let result = super::read::document_at(command, target, socket.clone())
                .await
                .map(|document| document.value);
            SessionResult::new(&socket, result)
        });
    }
    let mut pages = Vec::new();
    while let Some(result) = tasks.join_next().await {
        let page = result
            .map_err(|error| CtlFailure::rejected(format!("session reader failed: {error}")))?;
        let ready = page.response.is_some();
        pages.push(page);
        if any && ready {
            while let Some(Ok(page)) = tasks.try_join_next() {
                pages.push(page);
            }
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            break;
        }
    }
    pages.sort_by(|left, right| left.socket.cmp(&right.socket));
    let failed = total > 0
        && (pages.iter().all(|page| page.response.is_none())
            || matches!(&command, CtlCommand::Wait { all: true, .. })
                && pages.iter().any(|page| page.error.is_some()));
    let value = json!({"more_sessions":total.saturating_sub(pages.len()),"sessions":pages});
    if failed {
        return Err(CtlFailure::rejected(value.to_string()));
    }
    print!(
        "{}",
        ilium_control::render_document(&value, format, ilium_control::Shape::Generic)
    );
    Ok(())
}

fn shell_path(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

fn qualify_drills(page: &mut SessionResult) {
    let Some(response) = page.response.as_mut() else {
        return;
    };
    let prefix = format!("ilium ctl --socket {}", shell_path(&page.socket));
    if let Some(lines) = response.get_mut("digest").and_then(Value::as_array_mut) {
        for line in lines {
            if let Some(text) = line.as_str() {
                *line = Value::String(text.replace("ilium ctl pane ", &format!("{prefix} pane ")));
            }
        }
    }
    if let Some(events) = response.get_mut("events").and_then(Value::as_array_mut) {
        for event in events {
            if let Some(pane) = event["pane_id"].as_u64() {
                event["drill"] = json!(format!("{prefix} pane {pane} --brief"));
            }
        }
    }
}

fn render_pages(
    mut pages: Vec<SessionResult>,
    mut cursors: Cursors,
    total: usize,
    max_items: usize,
    max_chars: usize,
    format: MonitoringFormat,
) -> Result<String, CtlFailure> {
    pages.sort_by_key(|page| {
        let priority = if page.error.is_some() {
            0
        } else {
            page.response
                .as_ref()
                .and_then(|value| value["digest"].as_array())
                .and_then(|lines| lines.first())
                .and_then(Value::as_str)
                .map_or(3, |line| {
                    if line.starts_with("needs-you") {
                        0
                    } else if line.starts_with("failed-or-stuck") {
                        1
                    } else {
                        2
                    }
                })
        };
        (priority, page.socket.clone())
    });
    let mut selected = Vec::new();
    let mut items = 0;
    for page in pages {
        let count = page.item_count();
        if items + count > max_items {
            continue;
        }
        let old = cursors.get(&page.socket).cloned();
        let cursor = page
            .response
            .as_ref()
            .and_then(|value| value["cursor"].as_str())
            .map(str::to_owned);
        if let Some(cursor) = cursor {
            cursors.insert(page.socket.clone(), cursor);
        }
        let socket = page.socket.clone();
        selected.push(page);
        let text = render(
            &selected,
            &cursors,
            total.saturating_sub(selected.len()),
            format,
        );
        if text.chars().count() <= max_chars {
            items += count;
        } else {
            selected.pop();
            match old {
                Some(cursor) => {
                    cursors.insert(socket, cursor);
                }
                None => {
                    cursors.remove(&socket);
                }
            }
        }
    }
    let text = render(
        &selected,
        &cursors,
        total.saturating_sub(selected.len()),
        format,
    );
    if text.chars().count() > max_chars || total > 0 && selected.is_empty() {
        return Err(CtlFailure::usage(
            "output budget is too small to report a session, its cursor and overflow",
        ));
    }
    Ok(text)
}

fn render(
    pages: &[SessionResult],
    cursors: &Cursors,
    omitted: usize,
    format: MonitoringFormat,
) -> String {
    if format == MonitoringFormat::Json {
        return json!({"sessions":pages,"cursor":cursors,"more_sessions":omitted}).to_string()
            + "\n";
    }
    let mut text = String::new();
    for page in pages {
        // JSON-quoted names cannot inject terminal controls or spoof a header.
        text.push_str(&format!(
            "session={} socket={}\n",
            json!(page.session),
            json!(page.socket)
        ));
        if let Some(error) = &page.error {
            text.push_str(&format!("error={}\n", json!(error)));
        }
        if let Some(response) = &page.response {
            if let Some(lines) = response["digest"].as_array() {
                for line in lines {
                    if let Some(line) = line.as_str() {
                        text.push_str(line);
                        text.push('\n');
                    }
                }
                text.push_str(&format!(
                    "working={} more={}\n",
                    response["working"], response["more"]
                ));
            } else if let Some(events) = response["events"].as_array() {
                for event in events {
                    let kind = event["kind"].as_str().unwrap_or("unknown");
                    let severity = match kind {
                        "error" | "stuck" => "error",
                        "needs_input" => "warning",
                        _ => "info",
                    };
                    let pane = event["pane_id"]
                        .as_u64()
                        .map_or_else(|| "-".into(), |pane| pane.to_string());
                    let context = [event["project"].as_str(), event["title"].as_str()]
                        .into_iter()
                        .flatten()
                        .map(clean_text)
                        .collect::<Vec<_>>()
                        .join("/");
                    let summary = clean_text(event["summary"].as_str().unwrap_or(""));
                    let drill = event["drill"].as_str().unwrap_or("ilium ctl sessions");
                    text.push_str(&format!(
                        "{severity} {pane} {context} {kind} {summary} -> {drill}\n"
                    ));
                }
                text.push_str(&format!("more={}\n", response["more"]));
            }
        }
    }
    text.push_str(&format!(
        "+{omitted} more sessions (cursor {})\n",
        json!(cursors)
    ));
    text
}

fn clean_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(120)
        .collect()
}
