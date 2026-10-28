//! Agent-detect-lane testing hooks on `FakeHost` (classifying through the activated detector).
//!
//! Plugin-specific scaffolding kept out of the shared harness type.

use ilium_plugin_core_agent_detect::AgentDetectServer;
use ilium_plugin_sdk::generated::exports::detector::{Classification, DetectorScreen};
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::Detector;

/// Agent-detect-lane hooks. Implemented for `FakeHost`.
pub trait AgentDetectHostExt {
    /// Classifies `screen` for the agent `agent` through the activated detector, after hooks ran.
    fn classify(&mut self, agent: &str, screen: DetectorScreen) -> Classification;
}

impl AgentDetectHostExt for FakeHost {
    /// Runs the `detector.classify` export of the activated server plugin; contributed activity rules are
    /// settled into the plugin first so the call sees them.
    fn classify(&mut self, agent: &str, screen: DetectorScreen) -> Classification {
        self.settle();
        self.instance_mut::<AgentDetectServer>()
            .classify(agent.to_string(), screen)
    }
}
