//! Port twins of the Sessions core plugin (`ilium.core.sessions`): FakeHost twins of the original
//! tests whose behaviour moves behind the plugin API. Lane core-sessions, part ports.

mod agent_recovery;
mod common;
mod crashed_agent_recovery;
mod crashed_agent_recovery_wire;
mod frozen_session;
mod session_recovery;
mod worktree_ui;
