//! Host imports of client-side plugins that the server backs (`ClientRequest::PluginCall`).
//!
//! The journal lives in the server, beside the session snapshot, so a client-side plugin writes
//! and reads the same entries as the server-side plugins, and they survive a restart of the
//! client and of the server. The call runs [`JournalImport`] over the server's [`JournalBook`]:
//! the entry's `source` is the calling plugin, and the 4 KiB, host-kind and rate rules are the
//! ones a server-side plugin meets.
//!
//! A client-side call reads only the plugin's own entries: the server does not know which
//! grants the client half holds, so it never widens a read to `journal.read`.

use ilium_ipc::plugin::{ClientReplyBody, EnvelopeBody, PluginCall, PluginEnvelope};
use ilium_ipc::ServerEvent;
use ilium_plugin_api::generated::host::journal::HostJournal;
use ilium_plugin_api::generated::types::{EpochMs, ErrorCode, NodeId, PluginError};
use ilium_plugin_host::imports::journal::{JournalBook, JournalImport};
use serde_json::{json, Value};

/// The interface name of `ilium:host/journal` in a [`PluginCall`].
const JOURNAL_INTERFACE: &str = "journal";

/// The reply envelope for a call: `payload` is the JSON of the result.
pub(super) fn reply_event(call: &PluginCall, seq: u64, payload: Value) -> ServerEvent {
    ServerEvent::Plugin(PluginEnvelope {
        plugin: call.plugin.clone(),
        seq,
        body: EnvelopeBody::ClientReply(ClientReplyBody {
            call_id: call.call_id,
            payload: payload.to_string().into_bytes(),
        }),
    })
}

/// `{"err": plugin-error}` for a call that was refused before it ran.
pub(super) fn refusal(code: ErrorCode, message: &str) -> Value {
    json!({ "err": PluginError::new(code, message) })
}

/// Runs one call against the server's journal and returns the reply payload.
pub(super) fn answer(book: &JournalBook, call: &PluginCall) -> Value {
    if call.args_json.len() > ilium_ipc::plugin::MAX_PLUGIN_CALL_ARGS_BYTES {
        return refusal(
            ErrorCode::InvalidArgument,
            "the call arguments are too large",
        );
    }
    if call.interface != JOURNAL_INTERFACE {
        return refusal(
            ErrorCode::Unsupported,
            &format!("the server backs no import {}", call.interface),
        );
    }
    let Ok(args) = serde_json::from_slice::<Value>(&call.args_json) else {
        return refusal(
            ErrorCode::InvalidArgument,
            "the call arguments are not JSON",
        );
    };
    let pane = args.get("pane").and_then(Value::as_u64).map(NodeId);
    let mut journal = JournalImport::new(book.clone(), &call.plugin);
    match call.function.as_str() {
        "log-event" => {
            let (Some(kind), Some(payload_json)) = (
                args.get("kind").and_then(Value::as_str),
                args.get("payload-json").and_then(Value::as_str),
            ) else {
                return refusal(
                    ErrorCode::InvalidArgument,
                    "log-event needs kind and payload-json",
                );
            };
            match journal.log_event(pane, kind.to_owned(), payload_json.to_owned()) {
                Ok(()) => json!({ "ok": null }),
                Err(error) => json!({ "err": error }),
            }
        }
        "read" => {
            let since = EpochMs(args.get("since").and_then(Value::as_u64).unwrap_or(0));
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .map_or(u32::MAX, |limit| u32::try_from(limit).unwrap_or(u32::MAX));
            json!({ "ok": journal.read(pane, since, limit) })
        }
        other => refusal(
            ErrorCode::Unsupported,
            &format!("journal has no function {other}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{answer, reply_event};
    use ilium_ipc::plugin::{EnvelopeBody, PluginCall};
    use ilium_ipc::ServerEvent;
    use ilium_plugin_host::imports::journal::JournalBook;
    use serde_json::{json, Value};

    fn call(plugin: &str, function: &str, args: Value) -> PluginCall {
        PluginCall {
            plugin: plugin.to_owned(),
            call_id: 9,
            interface: "journal".to_owned(),
            function: function.to_owned(),
            args_json: args.to_string().into_bytes(),
        }
    }

    #[test]
    fn a_client_entry_lands_in_the_server_journal_under_the_calling_plugin() {
        let book = JournalBook::new();
        let written = answer(
            &book,
            &call(
                "com.example.notes",
                "log-event",
                json!({"pane": 4, "kind": "note", "payload-json": "{\"n\":1}"}),
            ),
        );
        assert_eq!(written, json!({"ok": null}));
        assert_eq!(book.len(), 1);
        let read = answer(
            &book,
            &call(
                "com.example.notes",
                "read",
                json!({"since": 0, "limit": 10}),
            ),
        );
        let entries = read["ok"].as_array().expect("a list of entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["source"], "com.example.notes");
        assert_eq!(entries[0]["kind"], "note");
        assert_eq!(entries[0]["pane"], 4);
    }

    #[test]
    fn another_plugin_reads_none_of_those_entries() {
        let book = JournalBook::new();
        answer(
            &book,
            &call(
                "com.example.notes",
                "log-event",
                json!({"kind": "note", "payload-json": "{}"}),
            ),
        );
        let read = answer(
            &book,
            &call(
                "com.example.other",
                "read",
                json!({"since": 0, "limit": 10}),
            ),
        );
        assert_eq!(read, json!({"ok": []}));
    }

    #[test]
    fn host_kinds_unknown_functions_and_bad_arguments_are_refused() {
        let book = JournalBook::new();
        let host_kind = answer(
            &book,
            &call(
                "com.example.notes",
                "log-event",
                json!({"kind": "pane-created", "payload-json": "{}"}),
            ),
        );
        assert!(host_kind.get("err").is_some(), "{host_kind}");
        let unknown = answer(&book, &call("com.example.notes", "erase", json!({})));
        assert!(unknown.get("err").is_some(), "{unknown}");
        let missing = answer(&book, &call("com.example.notes", "log-event", json!({})));
        assert!(missing.get("err").is_some(), "{missing}");
        assert!(book.is_empty());
    }

    #[test]
    fn the_reply_quotes_the_call_and_goes_to_the_calling_plugin() {
        let request = call("com.example.notes", "read", json!({}));
        let ServerEvent::Plugin(envelope) = reply_event(&request, 3, json!({"ok": []})) else {
            panic!("a plugin envelope");
        };
        assert_eq!(envelope.plugin, "com.example.notes");
        let EnvelopeBody::ClientReply(reply) = envelope.body else {
            panic!("a client reply");
        };
        assert_eq!(reply.call_id, 9);
        assert_eq!(reply.payload, br#"{"ok":[]}"#.to_vec());
    }
}
