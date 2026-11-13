//! Twin of `ilium-server/tests/smoke.rs::voice_submission_unblocks_a_real_pty_reader_with_enter`. The real PTY
//! reader is host behaviour; the plugin's part is the `send-input` intent: the text with `submitted` delivery,
//! which the host turns into the text plus Enter. See API_GAPS_svc01.md (item 17).

use ilium_plugin_sdk::types::{Delivery, Intent};

use crate::common::{add_pane, enabled_client, hear};
use crate::host_ext::*;

/// REQ: PORT:ilium-server/tests/smoke.rs::voice_submission_unblocks_a_real_pty_reader_with_enter
#[test]
fn port_045_a_spoken_message_is_submitted_to_the_pane_with_enter_delivery() {
    let mut host = enabled_client("push_to_talk");
    let pane = add_pane(&mut host, "echo");
    host.realtime().set_active_agent("echo");
    hear(&mut host, "message from voice");
    let sends: Vec<_> = host
        .submitted()
        .into_iter()
        .filter_map(|submission| match submission.intent {
            Intent::SendInput(spec) => Some(spec),
            _ => None,
        })
        .collect();
    assert_eq!(sends.len(), 1, "{sends:?}");
    assert_eq!(sends[0].pane.0, pane);
    assert_eq!(sends[0].text, "message from voice");
    assert_eq!(sends[0].delivery, Delivery::Submitted);
}
