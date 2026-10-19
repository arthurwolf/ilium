//! Twin of the startup-offer test in ilium-client/src/app.rs. The plugin replaces the startup offer dialog
//! with the Setup tab (manual page "Setup tab"), so "no offer is queued" is observed as: no confirm dialog
//! is open and every row reports set up once all targets carry the current blocks.

use crate::common::*;

/// REQ: PORT:ilium-client/src/app.rs::fully_configured_targets_do_not_queue_setup_prompts
#[test]
fn port_app_01_fully_configured_targets_do_not_queue_setup_prompts() {
    let mut host = project_host();
    for target in [CLAUDE_GLOBAL, CODEX_GLOBAL, CLAUDE_PROJECT, CODEX_PROJECT] {
        for block in ["chatroom", "progress"] {
            install(&mut host, block, target).expect("install");
        }
    }
    host.settle();
    for target in [CLAUDE_GLOBAL, CODEX_GLOBAL, CLAUDE_PROJECT, CODEX_PROJECT] {
        for block in ["chatroom", "progress"] {
            assert_eq!(
                status_ok(&mut host, block, target),
                "set-up",
                "{block} {target}"
            );
        }
    }
    assert!(
        host.render_popup(80).trim().is_empty(),
        "no dialog may be pending"
    );
    assert!(host.submitted().is_empty(), "no offer may be submitted");
}
