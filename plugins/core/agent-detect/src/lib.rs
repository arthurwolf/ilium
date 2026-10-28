//! Core plugin `ilium.core.agent-detect` (manual page core-plugin-agent-detect.md).
//!
//! Skeleton of the tests-first phase: one struct per side declared in `manifest.json`, each
//! implementing the lifecycle, the handler and the exports its manifest lists. Every body is
//! `unimplemented!` until the implementation phase moves the feature here.

use ilium_plugin_sdk::generated::exports::detector::Classification;
use ilium_plugin_sdk::generated::exports::detector::DetectorProcess;
use ilium_plugin_sdk::generated::exports::detector::DetectorScreen;
use ilium_plugin_sdk::generated::types::events::Event;
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::types::Activation;
use ilium_plugin_sdk::types::PluginError;
use ilium_plugin_sdk::{ilium_plugin, Detector, Handler, Lifecycle};

/// The server side of `ilium.core.agent-detect` (core-plugin-agent-detect.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["detector"])]
#[derive(Default)]
pub struct AgentDetectServer;

impl Lifecycle for AgentDetectServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.agent-detect: server lifecycle.activate")
    }
}

impl Handler for AgentDetectServer {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.agent-detect: server handler.on_events")
    }
}

impl Detector for AgentDetectServer {
    fn identify(&mut self, _processes: Vec<DetectorProcess>) -> Option<String> {
        unimplemented!("ilium.core.agent-detect: server detector.identify")
    }

    fn classify(&mut self, _signature: String, _screen: DetectorScreen) -> Classification {
        unimplemented!("ilium.core.agent-detect: server detector.classify")
    }
}

/// The client side of `ilium.core.agent-detect` (core-plugin-agent-detect.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = [])]
#[derive(Default)]
pub struct AgentDetectClient;

impl Lifecycle for AgentDetectClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        unimplemented!("ilium.core.agent-detect: client lifecycle.activate")
    }
}

impl Handler for AgentDetectClient {
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        unimplemented!("ilium.core.agent-detect: client handler.on_events")
    }
}
