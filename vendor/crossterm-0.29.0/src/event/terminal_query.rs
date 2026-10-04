//! Recognition of replies used by terminal keyboard and image-capability queries.
//!
//! This module does not consume ordinary input. Sources hold an ambiguous
//! prefix only while an explicit query owns the single event reader, and
//! replay that prefix as normal keys if the query ends before a reply does.

use super::{Event, InternalEvent, KeyCode};

pub(super) const MAX_REPLY_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReplyPrefix {
    Incomplete,
    Complete,
    Other,
}

fn prefix_or_other(bytes: &[u8], prefix: &[u8]) -> bool {
    prefix.starts_with(bytes) || bytes.starts_with(prefix)
}

pub(crate) fn reply_prefix(bytes: &[u8]) -> ReplyPrefix {
    if bytes.is_empty() || bytes[0] != b'\x1b' || bytes.len() > MAX_REPLY_BYTES {
        return ReplyPrefix::Other;
    }
    if bytes.len() == 1 {
        return ReplyPrefix::Incomplete;
    }
    match bytes[1] {
        b'_' if prefix_or_other(bytes, b"\x1b_Gi=31;") => {
            if bytes.len() < b"\x1b_Gi=31;".len() {
                return ReplyPrefix::Incomplete;
            }
            if bytes.ends_with(b"\x1b\\") {
                ReplyPrefix::Complete
            } else if bytes.last() == Some(&b'\x1b')
                || bytes[b"\x1b_Gi=31;".len()..]
                    .iter()
                    .all(|byte| (0x20..=0x7e).contains(byte))
            {
                ReplyPrefix::Incomplete
            } else {
                ReplyPrefix::Other
            }
        }
        b']' if prefix_or_other(bytes, b"\x1b]11;") => {
            if bytes.len() < b"\x1b]11;".len() {
                return ReplyPrefix::Incomplete;
            }
            if bytes.last() == Some(&b'\x07') || bytes.ends_with(b"\x1b\\") {
                ReplyPrefix::Complete
            } else if bytes.last() == Some(&b'\x1b')
                || bytes[b"\x1b]11;".len()..]
                    .iter()
                    .all(|byte| (0x20..=0x7e).contains(byte))
            {
                ReplyPrefix::Incomplete
            } else {
                ReplyPrefix::Other
            }
        }
        b'[' => csi_reply_prefix(bytes),
        _ => ReplyPrefix::Other,
    }
}

fn csi_reply_prefix(bytes: &[u8]) -> ReplyPrefix {
    let body = &bytes[2..];
    if body.is_empty() {
        return ReplyPrefix::Incomplete;
    }
    let Some((&last, prior)) = body.split_last() else {
        return ReplyPrefix::Incomplete;
    };
    let final_byte = (0x40..=0x7e).contains(&last);
    let content = if final_byte { prior } else { body };
    let numeric = |part: &[u8]| {
        part.iter()
            .all(|byte| byte.is_ascii_digit() || *byte == b';')
    };
    if body[0] == b'?' {
        if !numeric(&content[1..]) {
            return ReplyPrefix::Other;
        }
        return if !final_byte {
            ReplyPrefix::Incomplete
        } else if last == b'c'
            || (last == b'u' && content.len() > 1 && content[1..].iter().all(u8::is_ascii_digit))
        {
            ReplyPrefix::Complete
        } else {
            ReplyPrefix::Other
        };
    }
    if !numeric(content) {
        return ReplyPrefix::Other;
    }
    if final_byte {
        return match last {
            b'n' if content == b"0" => ReplyPrefix::Complete,
            b't' if content.starts_with(b"6;") => ReplyPrefix::Complete,
            b'R' if content.contains(&b';') => ReplyPrefix::Complete,
            _ => ReplyPrefix::Other,
        };
    }
    if body == b"0"
        || body == b"6"
        || body.starts_with(b"6;")
        || (numeric(body) && body.contains(&b';'))
    {
        ReplyPrefix::Incomplete
    } else {
        ReplyPrefix::Other
    }
}

pub(crate) fn literal_prefix_events(bytes: &[u8]) -> Vec<InternalEvent> {
    bytes
        .iter()
        .map(|byte| {
            let key = if *byte == b'\x1b' {
                KeyCode::Esc
            } else {
                KeyCode::Char(char::from(*byte))
            };
            InternalEvent::Event(Event::Key(key.into()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{reply_prefix, ReplyPrefix};

    #[test]
    fn recognizes_only_complete_query_replies() {
        assert_eq!(reply_prefix(b"\x1b_Gi=31;OK\x1b\\"), ReplyPrefix::Complete);
        assert_eq!(reply_prefix(b"\x1b[?64;4c"), ReplyPrefix::Complete);
        assert_eq!(reply_prefix(b"\x1b[?1u"), ReplyPrefix::Complete);
        assert_eq!(reply_prefix(b"\x1b[?0u"), ReplyPrefix::Complete);
        assert_eq!(reply_prefix(b"\x1b[6;7;14t"), ReplyPrefix::Complete);
        assert_eq!(reply_prefix(b"\x1b[0n"), ReplyPrefix::Complete);
        assert_eq!(reply_prefix(b"\x1b[6;6R"), ReplyPrefix::Complete);
        assert_eq!(
            reply_prefix(b"\x1b]11;rgb:ffff/0000/0000\x07"),
            ReplyPrefix::Complete
        );
        assert_eq!(reply_prefix(b"\x1b_Gi=31;"), ReplyPrefix::Incomplete);
        assert_eq!(reply_prefix(b"\x1b[6;7;"), ReplyPrefix::Incomplete);
        assert_eq!(reply_prefix(b"\x1b[?17"), ReplyPrefix::Incomplete);
        assert_eq!(reply_prefix(b"\x1b[200~"), ReplyPrefix::Other);
        assert_eq!(reply_prefix(b"\x1b[1;5A"), ReplyPrefix::Other);
        assert_eq!(reply_prefix(b"n"), ReplyPrefix::Other);
    }

    #[test]
    fn long_or_invalid_prefix_cannot_hold_the_reader() {
        let mut long = b"\x1b_Gi=31;".to_vec();
        long.extend(std::iter::repeat_n(b'x', 256));
        assert_eq!(reply_prefix(&long), ReplyPrefix::Other);
        assert_eq!(reply_prefix(b"\x1b[?u"), ReplyPrefix::Other);
        assert_eq!(reply_prefix(b"\x1b[?1h"), ReplyPrefix::Other);
    }
}
