//! The envelope mapping of the messages that cross between the server runtime and the
//! client runtimes (gap G4). The host crate shapes them as [`RemoteWire`]; this maps that
//! shape to and from the plugin envelope bodies of `ilium-ipc`.

use ilium_ipc::plugin::{ClientReplyBody, ClientRequestCall, EnvelopeBody, SourcedEvent};
use ilium_plugin_host::runtime::cross_side::RemoteWire;

/// The envelope body that carries `wire`.
pub(super) fn body_of(wire: RemoteWire) -> EnvelopeBody {
    match wire {
        RemoteWire::Event { topic, payload } => {
            EnvelopeBody::Event(SourcedEvent { topic, payload })
        }
        RemoteWire::Request {
            call_id,
            name,
            payload,
        } => EnvelopeBody::ClientRequest(ClientRequestCall {
            call_id,
            name,
            payload,
        }),
        RemoteWire::Reply { call_id, payload } => {
            EnvelopeBody::ClientReply(ClientReplyBody { call_id, payload })
        }
    }
}

/// The wire shape an envelope body carries, or `None` for a body that is not a crossing.
pub(super) fn wire_of(body: EnvelopeBody) -> Option<RemoteWire> {
    match body {
        EnvelopeBody::Event(SourcedEvent { topic, payload }) => {
            Some(RemoteWire::Event { topic, payload })
        }
        EnvelopeBody::ClientRequest(ClientRequestCall {
            call_id,
            name,
            payload,
        }) => Some(RemoteWire::Request {
            call_id,
            name,
            payload,
        }),
        EnvelopeBody::ClientReply(ClientReplyBody { call_id, payload }) => {
            Some(RemoteWire::Reply { call_id, payload })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{body_of, wire_of};
    use ilium_plugin_host::runtime::cross_side::RemoteWire;

    #[test]
    fn every_wire_shape_round_trips_through_an_envelope_body() {
        let shapes = [
            RemoteWire::Event {
                topic: "a/b".to_owned(),
                payload: b"{}".to_vec(),
            },
            RemoteWire::Request {
                call_id: 3,
                name: "ilium.service-call".to_owned(),
                payload: b"{}".to_vec(),
            },
            RemoteWire::Reply {
                call_id: 3,
                payload: b"{}".to_vec(),
            },
        ];
        for shape in shapes {
            assert_eq!(wire_of(body_of(shape.clone())), Some(shape));
        }
    }
}
