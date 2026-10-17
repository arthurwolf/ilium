//! Agent-detect-lane testing hooks on `FakeHost` (classifying through the activated detector).
//!
//! Plugin-specific scaffolding kept out of the shared harness type. Skeleton bodies.

#![allow(dead_code)]

use ilium_plugin_sdk::generated::exports::detector::{Classification, DetectorScreen};
use ilium_plugin_sdk::testing::FakeHost;

/// Agent-detect-lane hooks. Implemented for `FakeHost` with skeleton bodies.
pub trait AgentDetectHostExt {
    /// Classifies `screen` for the agent `agent` through the activated detector, after hooks ran.
    fn classify(&mut self, _agent: &str, _screen: DetectorScreen) -> Classification {
        unimplemented!("agent-detect.classify")
    }
}

impl AgentDetectHostExt for FakeHost {}
