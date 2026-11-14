//! Wire tests for the kernel-routing seam commit: the appended plugin protocol records
//! (CLI control records, forwarded intents, the pane signal request, the plugin pane kind) and
//! the protocol version bump from 1 to 2. Variant order is the bincode index, so every new
//! variant is checked at its appended index.

use ilium_core::NodeId;
use ilium_ipc::plugin::{
    CliStream, ControlBody, EnvelopeBody, ForwardTarget, ForwardedIntent, PluginEnvelope,
    PluginHello, PLUGIN_PROTOCOL_VERSION,
};
use ilium_ipc::{ClientRequest, NewPaneKind, PaneSignal, ServerEvent};
use std::path::PathBuf;

fn round_trip<T>(value: &T) -> Vec<u8>
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let bytes = bincode::serialize(value).expect("encode");
    let decoded: T = bincode::deserialize(&bytes).expect("decode");
    assert_eq!(&decoded, value);
    bytes
}

fn index_of(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[..4].try_into().expect("variant index"))
}

#[test]
fn cli_control_records_round_trip_at_appended_indices() {
    let bodies = [
        (
            ControlBody::CliInvoke {
                invocation_id: 7,
                plugin: "ilium.core.fleet".into(),
                command: "status".into(),
                args: vec!["--json".into()],
                stdin: vec![1, 2, 3],
                cwd: PathBuf::from("/work"),
                columns: 120,
            },
            6u32,
        ),
        (
            ControlBody::CliOutput {
                invocation_id: 7,
                stream: CliStream::Stderr,
                data: b"oops".to_vec(),
            },
            7,
        ),
        (
            ControlBody::CliExit {
                invocation_id: 7,
                code: -1,
            },
            8,
        ),
        (ControlBody::CliCancel { invocation_id: 7 }, 9),
        (
            ControlBody::SettingsChanged {
                plugin: "ilium.core.terminal".into(),
                keys: vec!["font-size".into()],
            },
            10,
        ),
    ];
    for (body, expected_index) in bodies {
        let bytes = round_trip(&body);
        assert_eq!(index_of(&bytes), expected_index, "{body:?}");
    }
}

#[test]
fn forwarded_intent_round_trips_at_index_seven_inside_a_request_and_an_event() {
    let intent = ForwardedIntent {
        plugin: "ilium.core.keys".into(),
        target: ForwardTarget::AllClients,
        intent_json: br#"{"intent":"focus-node"}"#.to_vec(),
    };
    let body = EnvelopeBody::ForwardedIntent(intent);
    assert_eq!(index_of(&round_trip(&body)), 7);
    let envelope = PluginEnvelope {
        plugin: "ilium.core.keys".into(),
        seq: 4,
        body,
    };
    round_trip(&ClientRequest::Plugin(envelope.clone()));
    round_trip(&ServerEvent::Plugin(envelope));
}

#[test]
fn signal_pane_process_and_plugin_pane_kind_round_trip() {
    for signal in [
        PaneSignal::Interrupt,
        PaneSignal::Terminate,
        PaneSignal::Kill,
        PaneSignal::Hangup,
    ] {
        round_trip(&ClientRequest::SignalPaneProcess {
            pane_id: NodeId(4),
            signal,
        });
    }
    let kind = NewPaneKind::Plugin {
        kind_id: "ilium.core.fleet.board".into(),
        title: "Fleet".into(),
    };
    assert_eq!(index_of(&round_trip(&kind)), 6);
}

#[test]
fn protocol_version_is_two() {
    assert_eq!(PLUGIN_PROTOCOL_VERSION, 2);
}

#[test]
fn a_version_one_hello_differs_from_the_current_protocol() {
    // The server compares `plugin_protocol` with the constant (HST-103); a client built before
    // the append sends 1 and is refused with `ipc.protocol-mismatch`.
    let old_client = PluginHello {
        plugin_protocol: 1,
        contract: "0.1.0".into(),
        installed: Vec::new(),
    };
    assert_ne!(old_client.plugin_protocol, PLUGIN_PROTOCOL_VERSION);
    round_trip(&old_client);
}
