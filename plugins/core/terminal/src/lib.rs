//! Core plugin `ilium.core.terminal` (manual page core-plugin-terminal.md).
//!
//! The server publishes the rate-limited `screen-changed` topic, renames panes from OSC titles
//! and provides the `transfer` service; the client opens `path:line` links in the editor. Pure
//! parts: `links` (link detection), `screen_diff` (repaint planning), `screen_topic` (rate limit
//! and payload), `transfer` (service requests and paste text).

pub mod links;
pub mod screen_diff;
pub mod screen_topic;
pub mod transfer;

use ilium_plugin_sdk::generated::exports::service_provider::{ServiceCall, ServiceReply};
use ilium_plugin_sdk::generated::host::log::Level;
use ilium_plugin_sdk::generated::host::tree::HostTree;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::host::{HostClock, HostIntentsHost, HostTimers, HostTopics};
use ilium_plugin_sdk::primitives::{HostTerminal, ScreenPart};
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::types::{
    EditorOpenSpec, EditorPlacement, ExternalTarget, FsPath, Intent, NodeId, OpenExternalSpec,
    RenameNodeSpec, RequestRef, RequestResult, Submission, TimerId, TitleOrigin, TitleSource,
};
use ilium_plugin_sdk::{host, ilium_plugin, Handler, Lifecycle, ServiceProvider};

use screen_topic::{Change, Limiter, Offer};
use transfer::{Pending, Request};

/// The server side of `ilium.core.terminal` (core-plugin-terminal.md, world `ilium:server/base@0.1.0`).
///
/// Publishes `ilium.core.terminal/screen-changed` (rate limited per pane), renames panes from
/// OSC titles, and provides the `ilium.core.terminal/transfer` service.
#[ilium_plugin(side = "server", exports = ["service-provider"])]
#[derive(Default)]
pub struct TerminalServer {
    activated: bool,
    limiter: Limiter,
    /// The timer armed for the earliest deferred publication, if any.
    flush_timer: Option<TimerId>,
    /// Transfers waiting for the history of their source pane.
    pending: Vec<Pending>,
}

impl TerminalServer {
    fn publish(change: &Change) {
        let mut api = host();
        if let Err(error) =
            HostTopics::publish(&mut *api, screen_topic::TOPIC.to_string(), change.payload())
        {
            ilium_plugin_sdk::log::log(
                Level::Warn,
                &format!("screen-changed publish failed: {}", error.message),
            );
        }
    }

    fn now() -> u64 {
        let mut api = host();
        HostClock::monotonic_millis(&mut *api)
    }

    /// Arms the one-shot flush timer for `due` unless one is already armed.
    fn arm_flush(&mut self, due: u64, now: u64) {
        if self.flush_timer.is_some() {
            return;
        }
        let wait = due.saturating_sub(now).clamp(50, u64::from(u32::MAX)) as u32;
        let mut api = host();
        match HostTimers::set(&mut *api, wait, None) {
            Ok(timer) => self.flush_timer = Some(timer),
            Err(error) => ilium_plugin_sdk::log::log(
                Level::Warn,
                &format!("screen-changed flush timer failed: {}", error.message),
            ),
        }
    }

    fn on_screen_changed(&mut self, change: Change) {
        let now = Self::now();
        match self.limiter.offer(change, now) {
            Offer::Publish(merged) => Self::publish(&merged),
            Offer::Deferred { due } => self.arm_flush(due, now),
        }
    }

    fn on_timer(&mut self, timer: TimerId) {
        if self.flush_timer != Some(timer) {
            return;
        }
        self.flush_timer = None;
        let now = Self::now();
        let (ready, next_due) = self.limiter.take_due(now);
        for change in &ready {
            Self::publish(change);
        }
        if let Some(due) = next_due {
            self.arm_flush(due, now);
        }
    }

    /// A history read finished: complete the transfer that waited for it.
    fn on_request_completed(&mut self, result: &RequestResult, out: &mut Vec<Submission>) {
        let RequestRef::Call(request) = result.request else {
            return;
        };
        let Some(index) = self
            .pending
            .iter()
            .position(|item| item.request == request.0)
        else {
            return;
        };
        let pending = self.pending.remove(index);
        let history = {
            let mut api = host();
            HostTerminal::take_history(&mut *api, request)
        };
        if let Some(submission) = pending.finish(history.as_deref().unwrap_or("")) {
            out.push(submission);
        }
    }

    fn submit(submission: Submission) -> Result<(), String> {
        let mut api = host();
        HostIntentsHost::submit(&mut *api, submission)
            .map(|_| ())
            .map_err(|error| error.message)
    }

    fn read_visible(pane: NodeId) -> Option<ilium_plugin_sdk::primitives::Screen> {
        let mut api = host();
        HostTerminal::read_screen(
            &mut *api,
            pane,
            ScreenPart::Visible,
            transfer::SCREEN_MAX_ROWS,
        )
    }

    fn send_screen(&mut self, source: NodeId, target: NodeId) -> String {
        if self.pending.len() >= transfer::MAX_PENDING {
            return transfer::error_payload("too many transfers are waiting for history");
        }
        let Some(screen) = Self::read_visible(source) else {
            return transfer::error_payload("source pane has no readable screen");
        };
        let visible = transfer::visible_text(&screen.rows);
        let requested = {
            let mut api = host();
            HostTerminal::history_text(&mut *api, source, transfer::HISTORY_MAX_BYTES)
        };
        match requested {
            Ok(request) => {
                let lines = visible.as_deref().map_or(0, |text| text.lines().count());
                self.pending.push(Pending {
                    request: request.0,
                    source,
                    target,
                    revision: screen.revision,
                    screen: visible,
                });
                transfer::sent_payload(true, lines)
            }
            Err(_) => {
                // History is unavailable: the visible screen alone is still worth pasting.
                let Some(text) = visible else {
                    return transfer::error_payload("source screen is blank");
                };
                let lines = text.lines().count();
                match Self::submit(transfer::transfer_submission(
                    source,
                    target,
                    screen.revision,
                    text,
                )) {
                    Ok(()) => transfer::sent_payload(false, lines),
                    Err(message) => transfer::error_payload(&message),
                }
            }
        }
    }

    fn call(&mut self, request: Request) -> String {
        match request {
            Request::SendScreen { source, target } => self.send_screen(source, target),
            Request::ReadScreen { pane } => match Self::read_visible(pane) {
                Some(screen) => transfer::rows_payload(&screen.rows, screen.revision),
                None => transfer::error_payload("pane has no readable screen"),
            },
            Request::SendText { pane, text } => {
                let lines = text.lines().count();
                match Self::submit(Submission::new(transfer::paste(pane, text))) {
                    Ok(()) => transfer::sent_payload(false, lines),
                    Err(message) => transfer::error_payload(&message),
                }
            }
        }
    }
}

impl Lifecycle for TerminalServer {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        let _ = &context;
        self.activated = true;
        Ok(())
    }
}

impl Handler for TerminalServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::ScreenChanged(change) => self.on_screen_changed(Change {
                    pane: change.pane.0,
                    first_row: change.first_row,
                    last_row: change.last_row,
                    revision: change.revision,
                }),
                Event::TimerFired(timer) => self.on_timer(timer),
                Event::RequestCompleted(result) => {
                    self.on_request_completed(&result, &mut submissions)
                }
                Event::PaneExited(exit) => {
                    self.limiter.forget(exit.node.0);
                    ilium_plugin_sdk::log::log(
                        Level::Info,
                        &format!(
                            "pane {} exited status={:?} signal={:?}",
                            exit.node.0, exit.status, exit.signal
                        ),
                    );
                }
                Event::PaneTitleChanged(change) if change.source == TitleOrigin::Osc => {
                    submissions.push(Submission::keyed(
                        Intent::RenameNode(RenameNodeSpec {
                            node: change.node,
                            title: change.title,
                            source: TitleSource::Inferred,
                            expected_revision: None,
                        }),
                        format!("osc-title-{}", change.node.0),
                    ));
                }
                _ => {}
            }
        }
        submissions
    }
}

impl ServiceProvider for TerminalServer {
    fn handle(&mut self, call: ServiceCall) -> ServiceReply {
        if call.service != transfer::SERVICE {
            return ServiceReply::Done(transfer::error_payload("unknown service"));
        }
        let reply = match transfer::from_payload(&call.method, &call.payload_json) {
            Ok(request) => self.call(request),
            Err(message) => transfer::error_payload(&message),
        };
        ServiceReply::Done(reply)
    }
}

/// The text of a `url` link when it is an `http` or `https` URL with something after the
/// scheme; anything else the `url` detector could be handed is not opened.
fn web_url(text: &str) -> Option<&str> {
    let lowered = text.to_ascii_lowercase();
    let rest = lowered
        .strip_prefix("https://")
        .or_else(|| lowered.strip_prefix("http://"))?;
    (!rest.is_empty()).then_some(text)
}

/// The client side of `ilium.core.terminal` (core-plugin-terminal.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct TerminalClient {
    activated: bool,
}

impl Lifecycle for TerminalClient {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        let _ = &context;
        self.activated = true;
        Ok(())
    }
}

impl TerminalClient {
    /// The working directory of the pane a link was clicked in, when the tree knows it.
    fn pane_cwd(pane: NodeId) -> Option<String> {
        let mut api = host();
        HostTree::get(&mut *api, pane).and_then(|view| view.cwd)
    }

    /// The editor submission of a file target.
    fn open_file(target: links::FileTarget) -> Submission {
        Submission::new(Intent::EditorOpen(EditorOpenSpec {
            path: FsPath {
                scope: target.scope.to_string(),
                path: target.path,
            },
            line: target.line,
            placement: EditorPlacement::Split,
        }))
    }
}

impl Handler for TerminalClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::LinkActivated(link) = event else {
                continue;
            };
            let submission = match link.detector_id.as_str() {
                "path-line" => {
                    let Some((path, line)) = links::parse_path_line(&link.text) else {
                        continue;
                    };
                    let cwd = Self::pane_cwd(link.node);
                    let Some(target) = links::file_target(path, Some(line), cwd.as_deref()) else {
                        continue;
                    };
                    Self::open_file(target)
                }
                "path" => {
                    let cwd = Self::pane_cwd(link.node);
                    let Some(target) = links::file_target_of_text(&link.text, cwd.as_deref())
                    else {
                        continue;
                    };
                    Self::open_file(target)
                }
                "url" => {
                    let Some(url) = web_url(&link.text) else {
                        continue;
                    };
                    Submission::new(Intent::OpenExternal(OpenExternalSpec {
                        target: ExternalTarget::Url(url.to_string()),
                    }))
                }
                _ => continue,
            };
            submissions.push(submission);
        }
        submissions
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use serde_json::Value;

    const MANIFEST_TEXT: &str = include_str!("../manifest.json");

    fn manifest() -> Value {
        serde_json::from_str(MANIFEST_TEXT).expect("manifest.json is valid JSON")
    }

    fn string_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("manifest value is an array")
            .iter()
            .map(|entry| entry.as_str().expect("array entry is a string").to_string())
            .collect()
    }

    fn extension_point(manifest: &Value, point_id: &str) -> Value {
        manifest["contributes"]["extension_points"]
            .as_array()
            .expect("extension_points is an array")
            .iter()
            .find(|point| point["id"] == point_id)
            .cloned()
            .expect("extension point is declared in the manifest")
    }

    fn detector(manifest: &Value, id: &str) -> Value {
        manifest["contributes"]["link_detectors"]
            .as_array()
            .expect("link_detectors is an array")
            .iter()
            .find(|detector| detector["id"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("detector {id} is declared"))
    }

    fn path_line_regex() -> regex::Regex {
        let manifest = manifest();
        let detector = &detector(&manifest, "path-line");
        let pattern = detector["regex"]
            .as_str()
            .expect("path-line regex is a string");
        regex::Regex::new(pattern).expect("path-line regex compiles")
    }

    /// REQ: COR-050
    #[test]
    fn manifest_identity_matches_the_manual() {
        let manifest = manifest();
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.terminal");
        assert_eq!(manifest["name"], "Terminal");
        assert_eq!(manifest["version"], "0.1.0");
        assert_eq!(manifest["publisher"], "Ilium");
        assert_eq!(manifest["license"], "MIT");
    }

    /// REQ: COR-050
    #[test]
    fn each_side_has_an_entry_a_world_and_a_struct() {
        let manifest = manifest();
        assert_eq!(manifest["server"]["entry"], "server.wasm");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(manifest["client"]["entry"], "client.wasm");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
        let _server: TerminalServer = TerminalServer::default();
        let _client: TerminalClient = TerminalClient::default();
    }

    /// REQ: COR-050
    #[test]
    fn activation_triggers_are_startup_and_screen_changed() {
        let mut activation = string_list(&manifest()["activation"]);
        activation.sort();
        assert_eq!(activation, vec!["event:screen-changed", "startup"]);
    }

    /// REQ: COR-033
    #[test]
    fn client_side_holds_the_write_grant_for_the_terminal_settings_tab() {
        let manifest = manifest();
        let client_caps = string_list(&manifest["capabilities"]["client"]);
        assert!(client_caps
            .iter()
            .any(|cap| cap == "settings.write-kernel:terminal"));
    }

    /// REQ: COR-050
    #[test]
    fn server_side_grants_terminal_pane_and_tree_capabilities() {
        let manifest = manifest();
        let server_caps = string_list(&manifest["capabilities"]["server"]);
        for expected in [
            "terminal.read-screen",
            "terminal.read-cells",
            "terminal.history",
            "terminal.write",
            "panes.read",
            "panes.signal",
            "tree.write",
        ] {
            assert!(
                server_caps.iter().any(|cap| cap == expected),
                "missing {expected}"
            );
        }
    }

    /// REQ: EXT-052
    #[test]
    fn extension_point_limits_match_the_manual_table() {
        let manifest = manifest();
        let before_send = extension_point(&manifest, "before-send-input");
        assert_eq!(before_send["deadline_ms"], 20);
        assert_eq!(before_send["max_handlers"], 8);
        let open_link = extension_point(&manifest, "open-link");
        assert_eq!(open_link["deadline_ms"], 20);
        assert_eq!(open_link["max_handlers"], 8);
        let link_actions = extension_point(&manifest, "link-actions");
        assert_eq!(link_actions["max_entries"], 64);
    }

    /// REQ: SEC-293
    #[test]
    fn extension_point_risk_marks_follow_the_corrected_table() {
        let manifest = manifest();
        assert_eq!(
            extension_point(&manifest, "before-send-input")["risk"],
            "high"
        );
        assert_eq!(extension_point(&manifest, "open-link")["risk"], "high");
        assert_eq!(
            extension_point(&manifest, "link-actions")["risk"],
            "standard"
        );
    }

    /// REQ: EXT-052
    #[test]
    fn open_link_is_provided_from_the_client_side_not_the_server() {
        let manifest = manifest();
        let server_caps = string_list(&manifest["capabilities"]["server"]);
        let client_caps = string_list(&manifest["capabilities"]["client"]);
        assert!(!server_caps
            .iter()
            .any(|cap| cap.starts_with("extensions.provide:ilium.core.terminal/open-link")));
        assert!(client_caps
            .iter()
            .any(|cap| cap == "extensions.provide:ilium.core.terminal/open-link"));
    }

    #[test]
    fn path_line_detector_matches_a_path_with_a_line_suffix() {
        let manifest = manifest();
        let detector = detector(&manifest, "path-line");
        assert_eq!(detector["command"], "link-open");
        assert!(path_line_regex().is_match("src/server/routes.rs:42"));
    }

    #[test]
    fn link_open_is_a_declared_client_command_and_the_url_detector_comes_first() {
        let manifest = manifest();
        let commands = manifest["contributes"]["commands"]
            .as_array()
            .expect("commands is an array");
        assert!(commands
            .iter()
            .any(|command| command["id"] == "link-open" && command["side"] == "client"));
        // Detectors are tried in manifest order: a `host:8080` inside a URL must not open as
        // a path.
        assert_eq!(manifest["contributes"]["link_detectors"][0]["id"], "url");
        assert!(string_list(&manifest["capabilities"]["client"])
            .iter()
            .any(|grant| grant == "open-external"));
    }

    #[test]
    fn only_web_urls_open_externally() {
        assert_eq!(
            web_url("https://example.org:8080/a"),
            Some("https://example.org:8080/a")
        );
        assert_eq!(web_url("HTTP://example.org"), Some("HTTP://example.org"));
        assert_eq!(web_url("https://"), None);
        assert_eq!(web_url("a.rs:1"), None);
        assert_eq!(web_url("file:///etc/passwd"), None);
    }

    #[test]
    fn a_url_link_submits_open_external() {
        let mut client = TerminalClient::default();
        let submissions = client.on_events(vec![Event::LinkActivated(
            ilium_plugin_sdk::types::LinkActivatedEvent {
                node: NodeId(3),
                detector_id: "url".to_string(),
                text: "https://example.org/x".to_string(),
            },
        )]);
        assert_eq!(submissions.len(), 1);
        assert_eq!(
            submissions[0].intent,
            Intent::OpenExternal(OpenExternalSpec {
                target: ExternalTarget::Url("https://example.org/x".to_string()),
            })
        );
    }

    #[test]
    fn path_line_detector_rejects_empty_text_and_paths_without_a_line_number() {
        let regex = path_line_regex();
        assert!(!regex.is_match(""));
        assert!(!regex.is_match("README.md"));
        assert!(!regex.is_match("src/server/routes.rs:"));
    }
}
