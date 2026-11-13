//! The wrap-up instructions a worktree agent receives (moved from `ilium-prompts`).
//!
//! The kernel registered this text as the template `agent/worktree-wrap-up`. The plugin owns the
//! text now and offers it to `ilium.core.agent-setup` as an instruction block once the extension
//! hook handler exists (see `tests/API_GAPS.md`).

/// Extension point of `ilium.core.agent-setup` that collects instruction blocks.
pub const INSTRUCTION_BLOCKS_POINT: &str = "ilium.core.agent-setup/instruction-blocks";

/// The wrap-up instructions, verbatim.
pub const INSTRUCTIONS: &str = include_str!("../templates/worktree-wrap-up.hbs");

/// The close-pane choices the instructions name, in the order they are listed.
pub const CLOSE_PANE_MODES: [&str; 3] = ["remove", "keep", "remove-and-branch"];
