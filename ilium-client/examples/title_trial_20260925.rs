//! One-use, read-only comparison against held-out session titles.
use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ilium_client::naming::PromptCompletionClient;
use ilium_client::session_naming::{infer_pane_title, SessionTitleInput};
use ilium_core::{AgentActivity, AgentClass, NodeId, PaneTitleSource};
use ilium_inference::{InferenceError, InferenceSettings, PaidProxy, TitleStyle};
use mongodb::bson::{Bson, Document};
use serde_json::{json, Value};

struct TrialGenerator<'a> {
    settings: &'a InferenceSettings,
    manual_title: &'a str,
    has_manual_title_in_prompt: Cell<bool>,
}

impl PromptCompletionClient for TrialGenerator<'_> {
    fn complete_prompt(&self, prompt: String) -> Result<String, InferenceError> {
        self.has_manual_title_in_prompt.set(
            self.has_manual_title_in_prompt.get()
                || prompt.replace('\u{a0}', " ").contains(self.manual_title),
        );
        self.settings.complete_prompt(prompt)
    }

    fn title_style(&self) -> TitleStyle {
        self.settings.title_style
    }
}

fn node<'a>(nodes: &'a Value, id: u64) -> anyhow::Result<&'a Value> {
    nodes
        .get(id.to_string().as_str())
        .ok_or_else(|| anyhow::anyhow!("missing tree node {id}"))
}

fn node_name(nodes: &Value, id: u64) -> anyhow::Result<String> {
    Ok(node(nodes, id)?["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing node name {id}"))?
        .to_string())
}

fn parent_id(nodes: &Value, id: u64) -> anyhow::Result<u64> {
    node(nodes, id)?["parent"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("missing parent for {id}"))
}

fn project_context(
    nodes: &Value,
    pane_id: u64,
) -> anyhow::Result<(String, PathBuf, String, Vec<String>)> {
    let parent = parent_id(nodes, pane_id)?;
    let mut ancestor = parent;
    let mut names = Vec::new();
    let (project_name, project_path) = loop {
        let entry = node(nodes, ancestor)?;
        names.push(node_name(nodes, ancestor)?);
        if let Some(path) = entry["kind"]["Container"]["kind"]["Project"]["path"].as_str() {
            break (node_name(nodes, ancestor)?, PathBuf::from(path));
        }
        ancestor = parent_id(nodes, ancestor)?;
    };
    names.reverse();
    let siblings = node(nodes, parent)?["kind"]["Container"]["children"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing siblings for {pane_id}"))?;
    let target_index = siblings
        .iter()
        .position(|id| id.as_u64() == Some(pane_id))
        .unwrap_or(0);
    let start = target_index
        .saturating_sub(20)
        .min(siblings.len().saturating_sub(41));
    let mut nearby = Vec::new();
    for id in siblings.iter().skip(start) {
        let Some(id) = id.as_u64() else { continue };
        if id == pane_id {
            continue;
        }
        let entry = node(nodes, id)?;
        let long = node_name(nodes, id)?;
        if let Some(short) = entry["short_name"]
            .as_str()
            .filter(|short| *short != long.as_str())
        {
            nearby.push(format!("short: {short}; long: {long}"));
        } else {
            nearby.push(long);
        }
        if nearby.len() == 40 {
            break;
        }
    }
    Ok((project_name, project_path, names.join(" > "), nearby))
}

async fn load_proxies(settings: &mut InferenceSettings) -> anyhow::Result<()> {
    if !settings.kilo_gateway.paid_proxies_enabled {
        return Ok(());
    }
    let database_settings = &settings.kilo_gateway.proxy_database;
    let fields = &database_settings.structure;
    let client = mongodb::Client::with_uri_str(&database_settings.uri).await?;
    let collection = client
        .database(&database_settings.database)
        .collection::<Document>(&database_settings.collection);
    let mut filter = Document::new();
    filter.insert(&fields.enabled, Bson::Boolean(true));
    let mut cursor = collection.find(filter).await?;
    while cursor.advance().await? {
        let document: Document = cursor.deserialize_current()?;
        let port = match document.get(&fields.port) {
            Some(Bson::Int32(value)) => u16::try_from(*value)?,
            Some(Bson::Int64(value)) => u16::try_from(*value)?,
            _ => anyhow::bail!("invalid paid proxy port"),
        };
        settings.kilo_gateway.paid_proxies.push(PaidProxy {
            ip: document.get_str(&fields.ip)?.to_owned(),
            port,
            protocol: document.get_str(&fields.protocol)?.to_owned(),
            username: document.get_str(&fields.username).unwrap_or("").to_owned(),
            password: document.get_str(&fields.password).unwrap_or("").to_owned(),
        });
    }
    anyhow::ensure!(
        !settings.kilo_gateway.paid_proxies.is_empty(),
        "no paid proxies available"
    );
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(
        (2..=3).contains(&arguments.len()),
        "usage: title_trial_20260925 --pane-ids=ID,ID --style=both|labeling|summarization [--existing-summaries=PATH]"
    );
    let pane_ids = arguments[0]
        .strip_prefix("--pane-ids=")
        .ok_or_else(|| anyhow::anyhow!("first argument must be --pane-ids"))?
        .split(',')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()?;
    anyhow::ensure!(!pane_ids.is_empty(), "at least one pane id is required");
    let styles = match arguments[1].as_str() {
        "--style=both" => vec![TitleStyle::Summarization, TitleStyle::Labeling],
        "--style=labeling" => vec![TitleStyle::Labeling],
        "--style=summarization" => vec![TitleStyle::Summarization],
        _ => anyhow::bail!("second argument must be --style=both|labeling|summarization"),
    };
    let mut existing_summaries = HashMap::new();
    if let Some(argument) = arguments.get(2) {
        let path = argument
            .strip_prefix("--existing-summaries=")
            .ok_or_else(|| anyhow::anyhow!("third argument must be --existing-summaries"))?;
        for line in std::fs::read_to_string(path)?.lines() {
            let row: Value = serde_json::from_str(line)?;
            if row["type"] == "result" && row["style"] == "summarization" {
                if let (Some(pane_id), Some(title)) =
                    (row["pane_id"].as_u64(), row["long"].as_str())
                {
                    existing_summaries.insert(pane_id, title.to_owned());
                }
            }
        }
    }
    let snapshot: Value = serde_json::from_slice(&std::fs::read(
        "/home/arthur/dev/ai/ilium/.ilium/sessions/default.json",
    )?)?;
    let nodes = &snapshot["tree"]["nodes"];
    let config_dir = directories::ProjectDirs::from("", "", "ilium")
        .ok_or_else(|| anyhow::anyhow!("no Ilium config directory"))?
        .config_dir()
        .to_path_buf();
    let mut settings = ilium_client::config::load(&config_dir)?.inference;
    load_proxies(&mut settings).await?;
    let home = Path::new("/home/arthur");

    for &pane_id in &pane_ids {
        let result = (|| -> anyhow::Result<()> {
            let manual_title = node_name(nodes, pane_id)?.replace('\u{a0}', " ");
            let pane = snapshot["panes"]
                .as_array()
                .and_then(|panes| {
                    panes
                        .iter()
                        .find(|pane| pane["node_id"].as_u64() == Some(pane_id))
                })
                .ok_or_else(|| anyhow::anyhow!("missing pane {pane_id}"))?;
            let command = pane["kind"]["Terminal"]["Command"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("pane {pane_id} is not a resumed agent"))?;
            let class = if command.starts_with("codex ") {
                AgentClass::Codex
            } else if command.starts_with("claude ") {
                AgentClass::Claude
            } else {
                anyhow::bail!("unknown agent for pane {pane_id}")
            };
            let session_id = command
                .split('\'')
                .nth(1)
                .ok_or_else(|| anyhow::anyhow!("missing session id for pane {pane_id}"))?
                .to_owned();
            let (project_name, project_path, parent_group, nearby_titles) =
                project_context(nodes, pane_id)?;
            let input = SessionTitleInput {
                pane_id: NodeId(pane_id),
                project_name,
                project_path,
                agent_class: class,
                session_id,
                process_id: None,
                current_title: existing_summaries
                    .get(&pane_id)
                    .cloned()
                    .unwrap_or_else(|| "Coding Session".to_owned()),
                current_short_title: None,
                current_icon: None,
                title_source: PaneTitleSource::Automatic,
                activity: AgentActivity::Done,
                has_persistent_goal: false,
                terminal_screen: String::new(),
                parent_group,
                nearby_titles,
            };
            for &style in &styles {
                settings.title_style = style;
                let generator = TrialGenerator {
                    settings: &settings,
                    manual_title: &manual_title,
                    has_manual_title_in_prompt: Cell::new(false),
                };
                let outcome = infer_pane_title(&generator, home, &input);
                let style_name = if style == TitleStyle::Labeling {
                    "labeling"
                } else {
                    "summarization"
                };
                match outcome {
                    Ok(title) => println!(
                        "{}",
                        json!({"type":"result","pane_id":pane_id,"style":style_name,"manual_title":manual_title,"short":title.short,"long":title.long,"manual_title_in_prompt":generator.has_manual_title_in_prompt.get()})
                    ),
                    Err(error) => println!(
                        "{}",
                        json!({"type":"error","pane_id":pane_id,"style":style_name,"message":error.to_string()})
                    ),
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            println!(
                "{}",
                json!({"type":"error","pane_id":pane_id,"message":error.to_string()})
            );
        }
    }
    println!(
        "{}",
        json!({"type":"summary","selected_panes":pane_ids.len(),"styles":styles.len()})
    );
    Ok(())
}
