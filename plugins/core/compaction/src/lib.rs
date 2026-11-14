//! Core plugin `ilium.core.compaction` (manual page core-plugin-compaction.md).
//!
//! Context compaction as a plugin: the server side runs the `compact` summariser job and the `scan` job of the
//! optimizer (transcript statistics, replay and the trigger recommendation); the client side draws the optimizer
//! panel. Both the Optimization and the Remote compaction settings tabs are contributed by the manifest.

use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::generated::types::layout::Area;
use ilium_plugin_sdk::generated::types::ui::UiTree;
use ilium_plugin_sdk::generated::types::ui::ViewTarget;
use ilium_plugin_sdk::host::HostJobs;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::{
    ErrorCode, Intent, MessageSpec, MessageTarget, NodeId, Outcome, PluginError, PluginId,
    PluginSide, RequestId, RequestRef,
};
use ilium_plugin_sdk::{host, ilium_plugin, Handler, HostApi, JobRunner, Lifecycle, Views};

pub mod analysis;
mod optimize;
mod panel;
mod redact;
pub mod remote;
mod remote_settings;
mod report;
mod runner;
mod scan;

/// The server side of `ilium.core.compaction` (core-plugin-compaction.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["job-runner"])]
#[derive(Default)]
pub struct CompactionServer {
    /// The pane the user focused last; it decides which technique row the job runner uses.
    focused: Option<NodeId>,
    /// The `scan` job in flight, if any; one scan at a time.
    scan_job: Option<RequestId>,
}

impl CompactionServer {
    fn to_client(channel: &str, payload: serde_json::Value) -> Submission {
        Submission::new(Intent::Message(MessageSpec {
            to: MessageTarget {
                plugin: PluginId(panel::PLUGIN_ID.to_string()),
                side: PluginSide::Client,
                client: None,
            },
            channel: channel.to_string(),
            payload: payload.to_string().into_bytes(),
        }))
    }

    fn start_scan(&mut self, payload: &[u8], out: &mut Vec<Submission>) {
        if self.scan_job.is_some() {
            return;
        }
        let mut handle = host();
        let api: &mut dyn HostApi = &mut *handle;
        match HostJobs::start(api, "scan".to_string(), payload.to_vec()) {
            Ok(job) => self.scan_job = Some(job),
            Err(error) => out.push(Self::to_client(
                "failed",
                serde_json::json!({ "message": error.message }),
            )),
        }
    }

    fn finish_scan(&mut self, job: RequestId, outcome: Outcome, out: &mut Vec<Submission>) {
        if self.scan_job != Some(job) {
            return;
        }
        self.scan_job = None;
        let mut handle = host();
        let api: &mut dyn HostApi = &mut *handle;
        match outcome {
            Outcome::Failed(error) | Outcome::Refused(error) => {
                let message = if error.code == ErrorCode::Cancelled {
                    "The scan was cancelled.".to_string()
                } else {
                    error.message
                };
                out.push(Self::to_client(
                    "failed",
                    serde_json::json!({ "message": message }),
                ));
            }
            other if !other.is_ok() => out.push(Self::to_client(
                "failed",
                serde_json::json!({ "message": "The scan did not finish." }),
            )),
            _ => match HostJobs::take_output(api, job) {
                Some(bytes) => out.push(Submission::new(Intent::Message(MessageSpec {
                    to: MessageTarget {
                        plugin: PluginId(panel::PLUGIN_ID.to_string()),
                        side: PluginSide::Client,
                        client: None,
                    },
                    channel: "report".to_string(),
                    payload: bytes,
                }))),
                None => out.push(Self::to_client(
                    "failed",
                    serde_json::json!({ "message": "The scan returned no result." }),
                )),
            },
        }
    }
}

impl Lifecycle for CompactionServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for CompactionServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut out = Vec::new();
        for event in batch {
            match event {
                Event::PaneFocused(pane) => self.focused = pane,
                Event::Message(message)
                    if message.from_side == PluginSide::Client && message.channel == "scan" =>
                {
                    self.start_scan(&message.payload, &mut out);
                }
                Event::RequestCompleted(result) => {
                    if let RequestRef::Call(request) = result.request {
                        self.finish_scan(request, result.outcome, &mut out);
                    }
                }
                _ => {}
            }
        }
        out
    }
}

impl JobRunner for CompactionServer {
    fn run(&mut self, kind: String, input: Vec<u8>) -> Result<Vec<u8>, PluginError> {
        match kind.as_str() {
            "compact" => runner::run_compact(&input, self.focused),
            "scan" => optimize::run_scan_job(&input),
            other => Err(PluginError::invalid_argument(format!(
                "unknown job kind: {other}"
            ))),
        }
    }
}

/// The client side of `ilium.core.compaction` (core-plugin-compaction.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct CompactionClient;

impl Lifecycle for CompactionClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for CompactionClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        batch.into_iter().flat_map(panel::on_event).collect()
    }
}

impl Views for CompactionClient {
    fn render(&mut self, view: ViewTarget, area: Area) -> UiTree {
        panel::render(&view, area)
    }
}

#[cfg(test)]
mod spec_tests {
    use serde_json::{json, Value};

    const MANIFEST_JSON: &str = include_str!("../manifest.json");
    const OPTIMIZATION_TAB_GRANT: &str = "settings.write-kernel:optimization";
    const REMOTE_COMPACTION_TAB_GRANT: &str = "settings.write-kernel:remote-compaction";

    fn load_manifest() -> Value {
        serde_json::from_str(MANIFEST_JSON).expect("manifest.json must parse as JSON")
    }

    /// Counts the sides of a manifest that request the given capability grant.
    fn grant_holder_count(manifest: &Value, grant: &str) -> usize {
        ["server", "client"]
            .iter()
            .map(|side| {
                manifest["capabilities"][*side]
                    .as_array()
                    .map_or(0, |grants| {
                        grants
                            .iter()
                            .filter(|entry| entry.as_str() == Some(grant))
                            .count()
                    })
            })
            .sum()
    }

    /// Counts every kernel-tab write grant (`settings.write-kernel:<tab>`) across both sides.
    fn kernel_tab_grant_count(manifest: &Value) -> usize {
        ["server", "client"]
            .iter()
            .map(|side| {
                manifest["capabilities"][*side]
                    .as_array()
                    .map_or(0, |grants| {
                        grants
                            .iter()
                            .filter(|entry| {
                                entry.as_str().is_some_and(|grant| {
                                    grant.starts_with("settings.write-kernel:")
                                })
                            })
                            .count()
                    })
            })
            .sum()
    }

    /// REQ: COR-050
    #[test]
    fn manifest_identifies_the_compaction_plugin() {
        let manifest = load_manifest();
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.compaction");
        assert_eq!(manifest["publisher"], "Ilium");
    }

    /// REQ: COR-050
    #[test]
    fn manifest_declares_each_side_with_its_world_and_export() {
        let manifest = load_manifest();
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(manifest["server"]["exports"], json!(["job-runner"]));
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
        assert_eq!(manifest["client"]["exports"], json!(["views"]));
    }

    /// REQ: COR-050
    #[test]
    fn manifest_lists_the_three_activation_triggers() {
        let manifest = load_manifest();
        assert_eq!(
            manifest["activation"],
            json!([
                "event:agent-activity-changed",
                "service:compact",
                "view:ilium.core.compaction/optimizer"
            ])
        );
    }

    /// REQ: COR-050
    #[test]
    fn manifest_declares_compact_job_command_and_panel() {
        let manifest = load_manifest();
        let compact_job = &manifest["contributes"]["jobs"][0];
        assert_eq!(compact_job["kind"], "compact");
        assert_eq!(compact_job["deadline_ms"], 60000);
        assert_eq!(compact_job["memory_mib"], 256);
        assert_eq!(manifest["contributes"]["commands"][0]["id"], "compact");
        assert_eq!(manifest["contributes"]["panels"][0]["id"], "optimizer");
        assert_eq!(manifest["contributes"]["panels"][0]["modal"], false);
    }

    /// REQ: COR-050
    #[test]
    fn manifest_declares_techniques_and_before_compact_extension_points() {
        let manifest = load_manifest();
        let points = manifest["contributes"]["extension_points"]
            .as_array()
            .expect("extension_points must be an array");
        let techniques = points
            .iter()
            .find(|point| point["id"] == "techniques")
            .expect("the techniques extension point must be declared");
        assert_eq!(techniques["kind"], "collect");
        assert_eq!(techniques["max_entries"], 64);
        assert_eq!(
            techniques["entry_schema"]["required"],
            json!(["id", "title", "service"])
        );
        let before_compact = points
            .iter()
            .find(|point| point["id"] == "before-compact")
            .expect("the before-compact extension point must be declared");
        assert_eq!(before_compact["kind"], "hook");
        assert_eq!(before_compact["mode"], "veto");
        assert_eq!(before_compact["deadline_ms"], 20);
    }

    /// REQ: COR-030
    #[test]
    fn each_kernel_tab_it_owns_has_exactly_one_holder() {
        let manifest = load_manifest();
        assert_eq!(grant_holder_count(&manifest, OPTIMIZATION_TAB_GRANT), 1);
        assert_eq!(
            grant_holder_count(&manifest, REMOTE_COMPACTION_TAB_GRANT),
            1
        );
    }

    /// REQ: COR-030
    #[test]
    fn plugin_writes_no_kernel_tab_beyond_its_two_owned_tabs() {
        let manifest = load_manifest();
        assert_eq!(kernel_tab_grant_count(&manifest), 2);
    }

    /// REQ: COR-030
    #[test]
    fn second_holder_of_a_kernel_tab_is_detected_as_a_duplicate() {
        let duplicate_owner = json!({
            "capabilities": {
                "server": [OPTIMIZATION_TAB_GRANT],
                "client": [OPTIMIZATION_TAB_GRANT]
            }
        });
        assert_eq!(
            grant_holder_count(&duplicate_owner, OPTIMIZATION_TAB_GRANT),
            2
        );
    }

    /// REQ: COR-030
    #[test]
    fn manifest_without_capabilities_holds_no_kernel_tab() {
        let empty_manifest = json!({});
        assert_eq!(
            grant_holder_count(&empty_manifest, OPTIMIZATION_TAB_GRANT),
            0
        );
        assert_eq!(kernel_tab_grant_count(&empty_manifest), 0);
    }
}
