//! Single-copy terminal payloads with unchanged fixed-integer bincode bytes.
//!
//! Generic Vec decoding deliberately accounts for every intermediate growth
//! buffer. A retained PTY journal instead has an exact byte length on the wire:
//! borrow it, admit its one output allocation, then copy once. The private
//! marker lets the closed bounded decoder recognize only this audited path.
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

pub(crate) const NAME: &str = "ilium_terminal_bytes_one_copy";

struct Bytes<'a>(&'a [u8]);
impl Serialize for Bytes<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self.0)
    }
}

pub(crate) fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_newtype_struct(NAME, &Bytes(bytes))
}

pub(crate) fn deserialize<'de, D: Deserializer<'de>>(decoder: D) -> Result<Vec<u8>, D::Error> {
    decoder.deserialize_newtype_struct(NAME, BytesVisitor)
}

struct BytesVisitor;
impl<'de> de::Visitor<'de> for BytesVisitor {
    type Value = Vec<u8>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a contiguous terminal byte payload")
    }
    fn visit_newtype_struct<D: Deserializer<'de>>(self, decoder: D) -> Result<Vec<u8>, D::Error> {
        decoder.deserialize_bytes(self)
    }
    fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<Vec<u8>, E> {
        Ok(bytes.to_vec())
    }
    fn visit_borrowed_bytes<E: de::Error>(self, bytes: &'de [u8]) -> Result<Vec<u8>, E> {
        self.visit_bytes(bytes)
    }
    fn visit_byte_buf<E: de::Error>(self, bytes: Vec<u8>) -> Result<Vec<u8>, E> {
        Ok(bytes)
    }
    // Preserve ordinary human-readable serde array representations. The
    // bounded IPC path below permits only bincode's borrowed byte callbacks.
    fn visit_seq<A: de::SeqAccess<'de>>(self, sequence: A) -> Result<Vec<u8>, A::Error> {
        Vec::<u8>::deserialize(de::value::SeqAccessDeserializer::new(sequence))
    }
}

#[cfg(test)]
mod tests {
    use crate::{decode_bounded_frame, encode_frame, ServerEvent};
    use ilium_core::NodeId;

    #[test]
    fn terminal_payloads_preserve_legacy_bincode_layout_and_values() {
        let pane_id = NodeId(71);
        let bytes = vec![0, 255, 27, b'[', b'm'];
        let cases = [
            (
                ServerEvent::ScreenUpdate {
                    pane_id,
                    first_sequence: 3,
                    sequence: 9,
                    bytes: bytes.clone(),
                },
                bincode::serialize(&(1_u32, pane_id, 3_u64, 9_u64, &bytes)).unwrap(),
            ),
            (
                ServerEvent::TerminalReplay {
                    pane_id,
                    through_sequence: 9,
                    bytes: bytes.clone(),
                    is_complete: false,
                },
                bincode::serialize(&(7_u32, pane_id, 9_u64, &bytes, false)).unwrap(),
            ),
        ];
        for (event, legacy) in cases {
            assert_eq!(bincode::serialize(&event).unwrap(), legacy);
            assert_eq!(bincode::deserialize::<ServerEvent>(&legacy).unwrap(), event);
            let frame = encode_frame(&event).unwrap();
            assert_eq!(decode_bounded_frame::<ServerEvent>(&frame).unwrap(), event);
        }
    }

    #[test]
    fn maximum_retained_journal_and_reset_prefix_fit_existing_decode_limit() {
        // OutputJournal's unchanged 32 MiB ceiling plus its two-byte reset.
        // Both a complete missing delta and truncated replay must be usable.
        let mut bytes = vec![b'x'; 32 * 1024 * 1024 + 2];
        bytes[..2].copy_from_slice(b"\x1bc");
        let cases = [
            ServerEvent::TerminalReplay {
                pane_id: NodeId(71),
                through_sequence: 91,
                bytes: bytes.clone(),
                is_complete: false,
            },
            ServerEvent::ScreenUpdate {
                pane_id: NodeId(71),
                first_sequence: 3,
                sequence: 91,
                bytes,
            },
        ];
        for event in cases {
            let frame = encode_frame(&event).unwrap();
            let decoded = decode_bounded_frame::<ServerEvent>(&frame).unwrap();
            assert_eq!(decoded, event);
        }
    }
}
