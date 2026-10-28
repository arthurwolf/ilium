//! Wire-level tests of [HST-100]..[HST-109]: the envelope, the handshake record, the delta
//! stream records and the framing they travel in. Expected bytes come from reference-host-ipc.md
//! (bincode, fixed-width little-endian integers, enum variants numbered in manual order).

use ilium_ipc::plugin::{
    ClientReplyBody, ClientRequestCall, ContributionDelta, ContributionOp, ControlBody, DeltaKind,
    EnvelopeBody, InstalledPlugin, OutcomeRecord, PluginEnvelope, PluginHello, PluginMessage,
    Rejection, SourcedEvent, Submission, MAX_DELTA_BYTES,
};
use ilium_ipc::{read_frame, write_frame, MAX_FRAME_LEN};
use ilium_ipc::{ClientRequest, ServerEvent, PLUGIN_PROTOCOL_VERSION};
use proptest::prelude::*;

const PLUGIN: &str = "acme.demo";

fn envelope(seq: u64, body: EnvelopeBody) -> PluginEnvelope {
    PluginEnvelope {
        plugin: PLUGIN.to_owned(),
        seq,
        body,
    }
}

/// One body of every variant, in the order of the manual's `EnvelopeBody` listing.
fn every_body() -> Vec<EnvelopeBody> {
    vec![
        EnvelopeBody::Submit(Submission {
            client_id: "c1".into(),
            dedup_key: "k1".into(),
            intent_json: "{\"intent\":\"create-pane\"}".into(),
        }),
        EnvelopeBody::Outcome(OutcomeRecord {
            seq: 3,
            dedup_key: "k1".into(),
            outcome_json: "{\"ok\":true}".into(),
        }),
        EnvelopeBody::Message(PluginMessage {
            topic: "acme.topic".into(),
            payload: vec![1, 2, 3],
        }),
        EnvelopeBody::ClientRequest(ClientRequestCall {
            call_id: 9,
            name: "clipboard.read".into(),
            payload: vec![],
        }),
        EnvelopeBody::ClientReply(ClientReplyBody {
            call_id: 9,
            payload: vec![7],
        }),
        EnvelopeBody::Event(SourcedEvent {
            topic: "tree-changed".into(),
            payload: vec![0],
        }),
        EnvelopeBody::Control(ControlBody::Resync),
    ]
}

fn hello(protocol: u16, contract: &str) -> PluginHello {
    PluginHello {
        plugin_protocol: protocol,
        contract: contract.to_owned(),
        installed: vec![InstalledPlugin {
            id: PLUGIN.to_owned(),
            version: "1.2.3".to_owned(),
            content_hash: [0xAB; 32],
        }],
    }
}

async fn round_trip<T>(value: &T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let (mut near, mut far) = tokio::io::duplex(1 << 20);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        write_frame(&mut near, value).await.expect("write frame");
        read_frame::<T, _>(&mut far).await.expect("read frame")
    })
    .await
    .expect("round trip finished in time")
}

/// REQ: HST-100
#[tokio::test]
async fn hst_100_client_request_plugin_round_trips_every_body() {
    for (index, body) in every_body().into_iter().enumerate() {
        let request = ClientRequest::Plugin(envelope(index as u64 + 1, body));
        assert_eq!(round_trip(&request).await, request);
    }
}

/// REQ: HST-100
#[tokio::test]
async fn hst_100_server_event_plugin_round_trips_every_body() {
    for (index, body) in every_body().into_iter().enumerate() {
        let event = ServerEvent::Plugin(envelope(index as u64 + 1, body));
        assert_eq!(round_trip(&event).await, event);
    }
}

/// REQ: HST-100, HST-107
#[tokio::test]
async fn hst_100_server_event_contributions_round_trips_snapshot_and_increment() {
    let snapshot = ServerEvent::Contributions(ContributionDelta {
        sequence: 1,
        kind: DeltaKind::Snapshot,
        ops: vec![ContributionOp::Put {
            id: "status.a".into(),
            entry: vec![1, 2],
        }],
    });
    let increment = ServerEvent::Contributions(ContributionDelta {
        sequence: 2,
        kind: DeltaKind::Increment,
        ops: vec![
            ContributionOp::Remove {
                id: "status.a".into(),
            },
            ContributionOp::Clear {
                plugin: PLUGIN.into(),
            },
        ],
    });
    assert_eq!(round_trip(&snapshot).await, snapshot);
    assert_eq!(round_trip(&increment).await, increment);
}

/// REQ: HST-100, HST-103
#[tokio::test]
async fn hst_100_server_event_rejected_round_trips_protocol_mismatch() {
    let event = ServerEvent::Rejected(Rejection {
        code: "ipc.protocol-mismatch".into(),
        message: "client and server speak different plugin protocols".into(),
    });
    assert_eq!(round_trip(&event).await, event);
}

/// REQ: HST-100
#[tokio::test]
async fn hst_100_plugin_frame_is_length_prefix_then_bincode_body() {
    let request = ClientRequest::Plugin(envelope(1, EnvelopeBody::Control(ControlBody::Resync)));
    let mut framed: Vec<u8> = Vec::new();
    write_frame(&mut framed, &request)
        .await
        .expect("write frame");
    let body = bincode::serialize(&request).expect("bincode body");
    let declared = u32::from_le_bytes(framed[..4].try_into().expect("four prefix bytes"));
    // The prefix carries a schema tag in its top five bits (see `frame_length_word`
    // in ilium-ipc/src/framing.rs); the low 27 bits hold the payload length.
    const SCHEMA_TAG_MASK: u32 = 0xF800_0000;
    const SCHEMA_TAG: u32 = 0xC800_0000;
    assert_eq!(declared & SCHEMA_TAG_MASK, SCHEMA_TAG);
    assert_eq!((declared & !SCHEMA_TAG_MASK) as usize, body.len());
    assert_eq!(&framed[4..], body.as_slice());
}

/// REQ: HST-100
#[test]
fn hst_100_frame_limit_stays_64_mib() {
    assert_eq!(MAX_FRAME_LEN, 64 * 1024 * 1024);
}

/// REQ: HST-100
#[tokio::test]
async fn hst_100_truncated_plugin_frame_is_refused() {
    let request = ClientRequest::Plugin(envelope(1, every_body().remove(0)));
    let mut framed: Vec<u8> = Vec::new();
    write_frame(&mut framed, &request)
        .await
        .expect("write frame");
    framed.truncate(framed.len() - 3);
    let mut reader: &[u8] = framed.as_slice();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        read_frame::<ClientRequest, _>(&mut reader),
    )
    .await
    .expect("read finished in time");
    assert!(result.is_err());
}

proptest! {
    // REQ: HST-100
    #[test]
    fn hst_100_envelope_bincode_round_trip_is_identity(
        plugin in "[a-z]{1,8}\\.[a-z]{1,8}",
        seq in 1u64..u64::MAX,
        topic in "[a-z.]{1,16}",
        payload in proptest::collection::vec(any::<u8>(), 0..64),
    ) {
        let original = PluginEnvelope {
            plugin,
            seq,
            body: EnvelopeBody::Message(PluginMessage { topic, payload }),
        };
        let bytes = bincode::serialize(&original).expect("encode");
        let decoded: PluginEnvelope = bincode::deserialize(&bytes).expect("decode");
        prop_assert_eq!(decoded, original);
    }
}

/// REQ: HST-101
#[test]
fn hst_101_envelope_body_has_exactly_the_seven_manual_variants() {
    // An exhaustive match without a wildcard: adding a hook-call or guest-call variant breaks
    // compilation of this test.
    fn name(body: &EnvelopeBody) -> &'static str {
        match body {
            EnvelopeBody::Submit(_) => "Submit",
            EnvelopeBody::Outcome(_) => "Outcome",
            EnvelopeBody::Message(_) => "Message",
            EnvelopeBody::ClientRequest(_) => "ClientRequest",
            EnvelopeBody::ClientReply(_) => "ClientReply",
            EnvelopeBody::Event(_) => "Event",
            EnvelopeBody::Control(_) => "Control",
        }
    }
    let names: Vec<&str> = every_body().iter().map(name).collect();
    assert_eq!(
        names,
        [
            "Submit",
            "Outcome",
            "Message",
            "ClientRequest",
            "ClientReply",
            "Event",
            "Control"
        ]
    );
    assert!(names
        .iter()
        .all(|variant| !variant.contains("Hook") && !variant.contains("Guest")));
}

/// REQ: HST-101
#[test]
fn hst_101_envelope_body_variant_indices_follow_manual_order() {
    for (expected, body) in every_body().into_iter().enumerate() {
        let bytes = bincode::serialize(&body).expect("encode");
        let index = u32::from_le_bytes(bytes[..4].try_into().expect("four index bytes"));
        assert_eq!(index as usize, expected);
    }
}

/// REQ: HST-101
#[test]
fn hst_101_envelope_body_index_seven_is_refused_unknown_variant() {
    let mut bytes = 7u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[0u8; 64]);
    let error = bincode::deserialize::<EnvelopeBody>(&bytes).expect_err("index 7 is unknown");
    // serde's derived enum decoder names the variant-index range; a short-input error would not.
    assert!(error.to_string().contains("variant index"), "{error}");
}

/// REQ: HST-103
#[tokio::test]
async fn hst_103_attach_carries_hello() {
    let request = ClientRequest::Attach {
        session: "default".into(),
        hello: hello(1, "0.1.0"),
    };
    let decoded = round_trip(&request).await;
    match decoded {
        ClientRequest::Attach {
            session,
            hello: received,
        } => {
            assert_eq!(session, "default");
            assert_eq!(received, hello(1, "0.1.0"));
        }
        other => panic!("expected Attach, got {other:?}"),
    }
}

/// REQ: HST-103
#[tokio::test]
async fn hst_103_attach_interactive_carries_hello() {
    let request = ClientRequest::AttachInteractive {
        session: "default".into(),
        hello: hello(1, "0.1.0"),
    };
    let decoded = round_trip(&request).await;
    match decoded {
        ClientRequest::AttachInteractive {
            session,
            hello: received,
        } => {
            assert_eq!(session, "default");
            assert_eq!(received.installed[0].content_hash, [0xAB; 32]);
            assert_eq!(received.installed[0].version, "1.2.3");
        }
        other => panic!("expected AttachInteractive, got {other:?}"),
    }
}

proptest! {
    // REQ: HST-103
    #[test]
    fn hst_103_hello_bincode_round_trip_is_identity(
        protocol in any::<u16>(),
        contract in "[0-9]{1,2}\\.[0-9]{1,2}\\.[0-9]{1,2}",
        version in "[0-9]\\.[0-9]\\.[0-9]",
        hash in proptest::array::uniform32(any::<u8>()),
    ) {
        let original = PluginHello {
            plugin_protocol: protocol,
            contract,
            installed: vec![InstalledPlugin { id: PLUGIN.into(), version, content_hash: hash }],
        };
        let decoded: PluginHello =
            bincode::deserialize(&bincode::serialize(&original).expect("encode")).expect("decode");
        prop_assert_eq!(decoded, original);
    }
}

/// REQ: HST-106
#[test]
fn hst_106_protocol_constant_is_u16_equal_to_one() {
    let constant: u16 = PLUGIN_PROTOCOL_VERSION;
    assert_eq!(constant, 1);
}

/// REQ: HST-106
#[test]
fn hst_106_envelope_layout_golden_bytes() {
    // plugin: u64 length + bytes; seq: u64; body: u32 variant index (Control = 6) then the
    // ControlBody variant index (Subscribe = 0, Resync = 1).
    let bytes = bincode::serialize(&envelope(1, EnvelopeBody::Control(ControlBody::Resync)))
        .expect("encode");
    let mut expected = Vec::new();
    expected.extend_from_slice(&9u64.to_le_bytes());
    expected.extend_from_slice(PLUGIN.as_bytes());
    expected.extend_from_slice(&1u64.to_le_bytes());
    expected.extend_from_slice(&6u32.to_le_bytes());
    expected.extend_from_slice(&1u32.to_le_bytes());
    assert_eq!(bytes, expected);
}

/// REQ: HST-106
#[test]
fn hst_106_hello_layout_golden_bytes() {
    let bytes = bincode::serialize(&PluginHello {
        plugin_protocol: 1,
        contract: "0.1.0".into(),
        installed: vec![],
    })
    .expect("encode");
    let mut expected = Vec::new();
    expected.extend_from_slice(&1u16.to_le_bytes());
    expected.extend_from_slice(&5u64.to_le_bytes());
    expected.extend_from_slice(b"0.1.0");
    expected.extend_from_slice(&0u64.to_le_bytes());
    assert_eq!(bytes, expected);
}

/// REQ: HST-107
#[test]
fn hst_107_delta_kind_snapshot_is_variant_zero_and_increment_one() {
    let snapshot = bincode::serialize(&DeltaKind::Snapshot).expect("encode");
    let increment = bincode::serialize(&DeltaKind::Increment).expect("encode");
    assert_eq!(snapshot, 0u32.to_le_bytes());
    assert_eq!(increment, 1u32.to_le_bytes());
}

/// REQ: HST-107
#[test]
fn hst_107_contribution_op_variants_follow_manual_order() {
    let ops = [
        ContributionOp::Put {
            id: "a".into(),
            entry: vec![],
        },
        ContributionOp::Remove { id: "a".into() },
        ContributionOp::Clear {
            plugin: PLUGIN.into(),
        },
    ];
    for (expected, op) in ops.iter().enumerate() {
        let bytes = bincode::serialize(op).expect("encode");
        assert_eq!(
            u32::from_le_bytes(bytes[..4].try_into().expect("index")) as usize,
            expected
        );
    }
}

proptest! {
    // REQ: HST-107, HST-108
    #[test]
    fn hst_107_delta_bincode_round_trip_keeps_sequence_and_ops(
        sequence in 1u64..u64::MAX,
        is_snapshot in any::<bool>(),
        ids in proptest::collection::vec("[a-z.]{1,12}", 0..8),
    ) {
        let original = ContributionDelta {
            sequence,
            kind: if is_snapshot { DeltaKind::Snapshot } else { DeltaKind::Increment },
            ops: ids.into_iter().map(|id| ContributionOp::Remove { id }).collect(),
        };
        let decoded: ContributionDelta =
            bincode::deserialize(&bincode::serialize(&original).expect("encode")).expect("decode");
        prop_assert_eq!(decoded, original);
    }
}

/// REQ: HST-109
#[test]
fn hst_109_delta_cap_constant_is_4_mib() {
    assert_eq!(MAX_DELTA_BYTES, 4 * 1024 * 1024);
}

/// REQ: HST-109
#[test]
fn hst_109_delta_cap_is_below_the_frame_limit() {
    assert!((MAX_DELTA_BYTES as u64) < u64::from(MAX_FRAME_LEN));
}
