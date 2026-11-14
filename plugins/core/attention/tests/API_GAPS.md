# API gaps of the attention feature tests

Items the tests call that the skeleton lacks, or manual details the tests had to assume.

- None yet: the crate is a skeleton and has no feature tests.

- Port status: `src/item.rs` (kinds, briefing, stable FNV-1a fingerprint; 5 kernel unit tests) and `src/parse.rs` (lenient reading of the LLM attention list; 7 kernel unit tests) are ported with their tests. The queue, instant and turbo evaluators, settings, briefing, panel and the `next`, `panel` and `overview` commands are not: the commands need a focus intent and `MarkAttention`/`AckAttention` (G3), the briefing needs the `inference` import (G10), the panel needs paging and text entry (G8), and the activity topic does not cross sides (G4). The manifest now declares the commands, keybindings, panel and settings of the page; the plugin does not handle the commands yet.

## Re-check of the foundation against the tree (2026-11-14): what still blocks the moves

- Keys `i`, `I`, `O` focus a pane: wanted `Intent::InvokeCommand("kernel.focus-node", {"node"})` routed to `ClientEffect::KernelCommand` (`ilium-client/src/plugin_host/effects.rs`, absent), `Intent::MarkAttention` and `Intent::AckAttention` routed to `ClientEffect::Attention`.
- The queue is evaluated every frame by `app_attention.rs` from kernel pane state; the plugin receives no `PaneFocused`, `AgentActivityChanged` or `AttentionChanged` events (G19 feed `runtime/events_feed.rs` absent; the server feed turns only `TreeSnapshot` and `PaneNodeChanged` into events).
- Briefing: the `inference` import has no backing provider (`ilium-plugin-host/src/imports/inference.rs` needs the `inference` service of a provider plugin; no core plugin provides it yet).
- `attention/dialog.rs`: text entry and scrolling now exist in panels, but not in popups (see the tree gaps).
