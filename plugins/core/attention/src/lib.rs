//! Core plugin `ilium.core.attention` (manual page core-plugin-attention.md).
//!
//! The attention queue as a plugin. The client side keeps the queue: it derives the instant items
//! from the agent panes, merges the model's answers, remembers visits, draws the panel and the
//! overview, reports the count in the chrome and takes the user to the next agent that needs
//! them. The server side runs the `analyse` job: it reads the recent screen of each changed agent
//! and asks the host inference call to classify it.

pub mod analyse;
pub mod client;
pub mod instant;
pub mod item;
pub mod parse;
pub mod queue;
pub mod refresh;
pub mod runner;
pub mod settings;
pub mod state;
pub mod turbo;
pub mod view;

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::generated::types::layout::Area;
use ilium_plugin_sdk::generated::types::ui::{UiTree, ViewTarget};
use ilium_plugin_sdk::host::HostJobs;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::{
    ErrorCode, Intent, MessageSpec, MessageTarget, Outcome, PluginError, PluginId, PluginSide,
    RequestId, RequestRef,
};
use ilium_plugin_sdk::{host, ilium_plugin, Handler, HostApi, JobRunner, Lifecycle, Views};

use crate::turbo::TurboLimiter;

/// The server side of `ilium.core.attention` (core-plugin-attention.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["job-runner"])]
pub struct AttentionServer {
    /// The concurrency window of the model calls; it outlives a job so a throttled provider keeps
    /// its smaller window for the next one.
    limiter: TurboLimiter,
    /// The `analyse` job in flight, if any; one at a time.
    job: Option<RequestId>,
}

impl Default for AttentionServer {
    fn default() -> Self {
        Self {
            limiter: TurboLimiter::new(turbo::DEFAULT_CONCURRENCY),
            job: None,
        }
    }
}

impl AttentionServer {
    fn to_client(channel: &str, payload: Vec<u8>) -> Submission {
        Submission::new(Intent::Message(MessageSpec {
            to: MessageTarget {
                plugin: PluginId(client::PLUGIN_ID.to_string()),
                side: PluginSide::Client,
                client: None,
            },
            channel: channel.to_string(),
            payload,
        }))
    }

    fn failed(message: &str) -> Submission {
        Self::to_client(
            "failed",
            serde_json::json!({ "message": message })
                .to_string()
                .into_bytes(),
        )
    }

    fn start(&mut self, payload: &[u8], out: &mut Vec<Submission>) {
        if self.job.is_some() {
            out.push(Self::failed("an analysis is already running"));
            return;
        }
        let mut handle = host();
        let api: &mut dyn HostApi = &mut *handle;
        match HostJobs::start(api, "analyse".to_string(), payload.to_vec()) {
            Ok(job) => self.job = Some(job),
            Err(error) => out.push(Self::failed(&error.message)),
        }
    }

    fn finish(&mut self, job: RequestId, outcome: Outcome, out: &mut Vec<Submission>) {
        if self.job != Some(job) {
            return;
        }
        self.job = None;
        let mut handle = host();
        let api: &mut dyn HostApi = &mut *handle;
        match outcome {
            Outcome::Failed(error) | Outcome::Refused(error) => {
                let message = if error.code == ErrorCode::Cancelled {
                    "The analysis was cancelled.".to_string()
                } else {
                    error.message
                };
                out.push(Self::failed(&message));
            }
            other if !other.is_ok() => out.push(Self::failed("The analysis did not finish.")),
            _ => match HostJobs::take_output(api, job) {
                Some(bytes) => out.push(Self::to_client("reply", bytes)),
                None => out.push(Self::failed("The analysis returned no result.")),
            },
        }
    }
}

impl Lifecycle for AttentionServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for AttentionServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut out = Vec::new();
        for event in batch {
            match event {
                Event::Message(message)
                    if message.from_side == PluginSide::Client && message.channel == "analyse" =>
                {
                    self.start(&message.payload, &mut out);
                }
                Event::RequestCompleted(result) => {
                    if let RequestRef::Call(request) = result.request {
                        self.finish(request, result.outcome, &mut out);
                    }
                }
                _ => {}
            }
        }
        out
    }
}

impl JobRunner for AttentionServer {
    fn run(&mut self, kind: String, input: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        match kind.as_str() {
            "analyse" => runner::run_analyse(&mut self.limiter, &input),
            other => Err(PluginError::invalid_argument(format!(
                "unknown job kind: {other}"
            ))),
        }
    }
}

/// The client side of `ilium.core.attention` (core-plugin-attention.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct AttentionClient {
    client: client::Client,
}

impl Lifecycle for AttentionClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        self.client.activate();
        Ok(())
    }
}

impl Handler for AttentionClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        self.client.on_events(batch)
    }
}

impl Views for AttentionClient {
    fn render(&mut self, view: ViewTarget, area: Area) -> UiTree {
        self.client.render(&view, area)
    }
}

#[cfg(test)]
mod spec_tests {
    use serde_json::Value;

    const MANIFEST_TEXT: &str = include_str!("../manifest.json");

    fn manifest() -> Value {
        serde_json::from_str(MANIFEST_TEXT).expect("manifest.json is valid JSON")
    }

    fn string_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// REQ: COR-050
    #[test]
    fn manifest_identifies_the_attention_plugin() {
        let manifest = manifest();
        assert_eq!(manifest["id"].as_str(), Some("ilium.core.attention"));
        assert_eq!(manifest["api"].as_str(), Some("ilium:plugin@0.1.0"));
        assert_eq!(manifest["version"].as_str(), Some("0.1.0"));
        assert_eq!(manifest["name"].as_str(), Some("Attention queue"));
    }

    /// REQ: COR-050
    #[test]
    fn manifest_declares_both_sides_with_their_worlds() {
        let manifest = manifest();
        assert_eq!(manifest["server"]["entry"].as_str(), Some("server.wasm"));
        assert_eq!(
            manifest["server"]["world"].as_str(),
            Some("ilium:server/base@0.1.0")
        );
        assert_eq!(manifest["client"]["entry"].as_str(), Some("client.wasm"));
        assert_eq!(
            manifest["client"]["world"].as_str(),
            Some("ilium:client/base@0.1.0")
        );
    }

    /// REQ: COR-050
    #[test]
    fn manifest_activates_on_startup() {
        let manifest = manifest();
        assert_eq!(string_list(&manifest["activation"]), vec!["startup"]);
    }

    /// REQ: COR-050
    #[test]
    fn manifest_grants_its_extension_points() {
        let manifest = manifest();
        let mut granted = string_list(&manifest["capabilities"]["server"]);
        granted.extend(string_list(&manifest["capabilities"]["client"]));
        for point_id in ["item-sources", "before-jump"] {
            let grant = format!("extensions.provide:ilium.core.attention/{point_id}");
            assert!(granted.contains(&grant), "missing grant {grant}");
        }
    }
}
