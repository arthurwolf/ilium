//! Lossless normalization for one client event-loop turn's outbound IPC.
//!
//! The socket writer owns the bounded queue and applies backpressure. This
//! module only removes adjacent state updates whose earlier value can never be
//! observed meaningfully, preserving every key, click, command, and ordering
//! boundary.

use ilium_ipc::ClientRequest;

/// Collapses redundant adjacent resize/focus state while preserving order.
pub fn coalesce(requests: Vec<ClientRequest>) -> Vec<ClientRequest> {
    let mut coalesced = Vec::with_capacity(requests.len());
    for request in requests {
        match (coalesced.last_mut(), request) {
            (
                Some(ClientRequest::ResizePane {
                    pane_id: previous_pane_id,
                    rows: previous_rows,
                    cols: previous_cols,
                }),
                ClientRequest::ResizePane {
                    pane_id,
                    rows,
                    cols,
                },
            ) if *previous_pane_id == pane_id => {
                *previous_rows = rows;
                *previous_cols = cols;
            }
            (
                Some(ClientRequest::SetPaneFocus {
                    pane_id: previous_pane_id,
                    focused: previous_focused,
                }),
                ClientRequest::SetPaneFocus { pane_id, focused },
            ) if *previous_pane_id == pane_id && *previous_focused == focused => {}
            (_, request) => coalesced.push(request),
        }
    }
    coalesced
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::NodeId;

    #[test]
    fn adjacent_resizes_keep_only_the_latest_geometry() {
        let pane_id = NodeId(7);
        let requests = coalesce(vec![
            ClientRequest::ResizePane {
                pane_id,
                rows: 20,
                cols: 80,
            },
            ClientRequest::ResizePane {
                pane_id,
                rows: 30,
                cols: 120,
            },
        ]);
        assert!(matches!(
            requests.as_slice(),
            [ClientRequest::ResizePane {
                pane_id: NodeId(7),
                rows: 30,
                cols: 120
            }]
        ));
    }

    #[test]
    fn repeated_focus_state_coalesces_without_erasing_transitions() {
        let pane_id = NodeId(9);
        let requests = coalesce(vec![
            ClientRequest::SetPaneFocus {
                pane_id,
                focused: true,
            },
            ClientRequest::SetPaneFocus {
                pane_id,
                focused: true,
            },
            ClientRequest::SetPaneFocus {
                pane_id,
                focused: false,
            },
        ]);
        assert_eq!(requests.len(), 2);
        assert!(matches!(
            requests[0],
            ClientRequest::SetPaneFocus { focused: true, .. }
        ));
        assert!(matches!(
            requests[1],
            ClientRequest::SetPaneFocus { focused: false, .. }
        ));
    }

    #[test]
    fn key_input_is_never_coalesced_or_reordered() {
        let pane_id = NodeId(11);
        let requests = coalesce(vec![
            ClientRequest::ResizePane {
                pane_id,
                rows: 20,
                cols: 80,
            },
            ClientRequest::KeyInput {
                pane_id,
                bytes: b"x".to_vec(),
            },
            ClientRequest::ResizePane {
                pane_id,
                rows: 21,
                cols: 81,
            },
        ]);
        assert_eq!(requests.len(), 3);
        assert!(matches!(requests[1], ClientRequest::KeyInput { .. }));
    }
}
