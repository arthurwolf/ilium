//! Host implementation of `ilium:host/sessions`.
//!
//! `list-live` reports the calling session only (the one entry marked `is-self`); other sessions
//! are discovered through the socket directory by the layer that owns IPC. `send` validates the
//! request and queues it; the reply is delivered by the target and fetched with `take-reply`.

use ilium_plugin_api::generated::host::sessions::{CrossRequest, HostSessions, LiveSession};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::RequestId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::env::RequestIds;

/// The session name a bare `ilium` owns.
const DEFAULT_SESSION_NAME: &str = "default";

/// Cross-session commands the host accepts. A command outside this set is not declared.
const CROSS_SESSION_COMMANDS: &[&str] = &["pane-count", "list-panes", "focus-pane", "open-pane"];

/// Host side of the `sessions` interface of one plugin instance. Its request ids come from the
/// runtime's allocator and its replies are its own. Cloning shares the replies (a test harness
/// keeps one instance and hands out clones).
#[derive(Debug, Clone, Default)]
pub struct SessionsImport {
    ids: RequestIds,
    /// Replies that arrived for accepted requests; empty until a target answers.
    replies: Arc<Mutex<HashMap<u64, String>>>,
}

impl SessionsImport {
    /// An import drawing request ids from `ids`, the allocator of the runtime.
    pub fn new(ids: RequestIds) -> Self {
        Self {
            ids,
            replies: Arc::default(),
        }
    }

    fn replies(&self) -> MutexGuard<'_, HashMap<u64, String>> {
        // Every writer does one insert or remove, so the map is consistent after a panic.
        self.replies.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records the reply of the target session to an accepted request; the plugin fetches it
    /// once with `take-reply`. The layer that owns IPC calls this when the target answers.
    pub fn deliver_reply(&self, request: RequestId, reply_json: String) {
        self.replies().insert(request.0, reply_json);
    }
}

fn error(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

impl HostSessions for SessionsImport {
    fn list_live(&mut self) -> Vec<LiveSession> {
        let project = std::env::current_dir()
            .map(|directory| directory.display().to_string())
            .unwrap_or_else(|_| ".".to_owned());
        vec![LiveSession {
            name: DEFAULT_SESSION_NAME.to_owned(),
            project,
            socket_id: format!("ilium-{}", std::process::id()),
            panes: 0,
            attached_clients: 0,
            is_self: true,
        }]
    }

    fn send(&mut self, request: CrossRequest) -> Result<RequestId, PluginError> {
        let live = self.list_live();
        if !live.iter().any(|session| session.name == request.session) {
            return Err(error(ErrorCode::NotFound, "no live session has that name"));
        }
        if !CROSS_SESSION_COMMANDS.contains(&request.command.as_str()) {
            return Err(error(
                ErrorCode::NotPermitted,
                "the command is not declared in contributes.cross-session-commands",
            ));
        }
        if serde_json::from_str::<serde_json::Value>(&request.arguments_json).is_err() {
            return Err(error(
                ErrorCode::InvalidArgument,
                "arguments-json is not valid JSON",
            ));
        }
        Ok(self.ids.next_request())
    }

    fn take_reply(&mut self, request: RequestId) -> Option<String> {
        self.replies().remove(&request.0)
    }
}

#[cfg(test)]
mod tests {
    use super::SessionsImport;
    use ilium_plugin_api::generated::host::sessions::{CrossRequest, HostSessions, LiveSession};
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::RequestId;

    fn cross_request(session: &str, command: &str, arguments_json: &str) -> CrossRequest {
        CrossRequest {
            session: session.to_owned(),
            command: command.to_owned(),
            arguments_json: arguments_json.to_owned(),
        }
    }

    /// Returns the error code of a `send` that must fail; a success is a test failure.
    fn expect_send_error(result: Result<RequestId, PluginError>) -> ErrorCode {
        match result {
            Ok(request_id) => {
                panic!("send accepted the request as {request_id:?}, expected an error")
            }
            Err(error) => error.code,
        }
    }

    /// REQ: CAP-018
    #[test]
    fn list_live_marks_exactly_one_entry_as_the_calling_session() {
        let mut sessions = SessionsImport::default();
        let live: Vec<LiveSession> = sessions.list_live();
        let self_count = live.iter().filter(|session| session.is_self).count();
        assert_eq!(
            self_count, 1,
            "the calling session always runs, so exactly one entry is_self; got {live:?}"
        );
    }

    /// REQ: CAP-018
    #[test]
    fn list_live_entries_have_non_empty_name_project_and_socket_id() {
        let mut sessions = SessionsImport::default();
        for session in sessions.list_live() {
            assert!(
                !session.name.is_empty(),
                "a live session has a name, bare ilium is \"default\"; got {session:?}"
            );
            assert!(
                !session.project.is_empty(),
                "a live session names its project directory; got {session:?}"
            );
            assert!(
                !session.socket_id.is_empty(),
                "a live session names its socket; got {session:?}"
            );
        }
    }

    /// REQ: EVT-013
    #[test]
    fn send_refuses_an_unknown_session_name_with_not_found() {
        let mut sessions = SessionsImport::default();
        let request = cross_request("no-such-session-name", "pane-count", "{}");
        assert_eq!(
            expect_send_error(sessions.send(request)),
            ErrorCode::NotFound
        );
    }

    /// REQ: EVT-013
    #[test]
    fn send_refuses_an_empty_session_name_with_not_found() {
        let mut sessions = SessionsImport::default();
        let request = cross_request("", "pane-count", "{}");
        assert_eq!(
            expect_send_error(sessions.send(request)),
            ErrorCode::NotFound
        );
    }

    /// REQ: ABI-009
    #[test]
    fn send_refuses_a_command_the_plugin_did_not_declare_with_not_permitted() {
        let mut sessions = SessionsImport::default();
        let request = cross_request("default", "no-such-declared-command", "{}");
        assert_eq!(
            expect_send_error(sessions.send(request)),
            ErrorCode::NotPermitted
        );
    }

    /// REQ: ABI-009
    #[test]
    fn send_refuses_an_empty_command_id_with_not_permitted() {
        let mut sessions = SessionsImport::default();
        let request = cross_request("default", "", "{}");
        assert_eq!(
            expect_send_error(sessions.send(request)),
            ErrorCode::NotPermitted
        );
    }

    /// REQ: ABI-009
    #[test]
    fn send_refuses_arguments_that_are_not_json_with_invalid_argument() {
        let mut sessions = SessionsImport::default();
        let request = cross_request("default", "pane-count", "{not json");
        assert_eq!(
            expect_send_error(sessions.send(request)),
            ErrorCode::InvalidArgument
        );
    }

    /// REQ: ABI-009
    #[test]
    fn send_refuses_empty_arguments_text_with_invalid_argument() {
        let mut sessions = SessionsImport::default();
        let request = cross_request("default", "pane-count", "");
        assert_eq!(
            expect_send_error(sessions.send(request)),
            ErrorCode::InvalidArgument
        );
    }

    /// REQ: RQO-002
    #[test]
    fn send_of_a_valid_request_returns_a_request_id_that_take_reply_reports_as_pending() {
        let mut sessions = SessionsImport::default();
        let request = cross_request("default", "pane-count", "{}");
        let request_id = match sessions.send(request) {
            Ok(request_id) => request_id,
            Err(error) => {
                panic!("a valid request to a live session must be accepted; got {error:?}")
            }
        };
        assert_eq!(
            sessions.take_reply(request_id),
            None,
            "the reply is not there until the target answers, so take-reply gives none"
        );
    }

    /// REQ: RQO-002
    #[test]
    fn each_send_gets_its_own_request_id() {
        let mut sessions = SessionsImport::default();
        let first = sessions.send(cross_request("default", "pane-count", "{}"));
        let second = sessions.send(cross_request("default", "pane-count", "{}"));
        match (first, second) {
            (Ok(first_id), Ok(second_id)) => {
                assert_ne!(
                    first_id, second_id,
                    "two sends in one batch must get two request ids"
                );
            }
            (first, second) => {
                panic!("both valid sends must be accepted; got {first:?} and {second:?}")
            }
        }
    }

    /// REQ: RQO-008
    #[test]
    fn take_reply_for_a_request_that_never_existed_returns_none() {
        let mut sessions = SessionsImport::default();
        assert_eq!(sessions.take_reply(RequestId(0)), None);
    }

    /// REQ: RQO-008
    #[test]
    fn take_reply_at_the_largest_request_id_returns_none() {
        let mut sessions = SessionsImport::default();
        assert_eq!(sessions.take_reply(RequestId(u64::MAX)), None);
    }

    /// REQ: RQO-008
    #[test]
    fn repeated_take_reply_of_an_unknown_request_keeps_returning_none() {
        let mut sessions = SessionsImport::default();
        let first = sessions.take_reply(RequestId(7));
        let second = sessions.take_reply(RequestId(7));
        assert_eq!(
            first, None,
            "the first take of a request with no reply returns none"
        );
        assert_eq!(
            second, None,
            "a second take must not return a reply that was never there"
        );
    }

    #[test]
    fn a_reply_is_taken_once_and_only_by_its_instance() {
        let mut owner = SessionsImport::default();
        let mut stranger = SessionsImport::default();
        let request = owner
            .send(cross_request("default", "pane-count", "{}"))
            .expect("accepted");
        owner.deliver_reply(request, "{\"panes\":3}".to_owned());
        assert_eq!(stranger.take_reply(request), None);
        assert_eq!(owner.take_reply(request).as_deref(), Some("{\"panes\":3}"));
        assert_eq!(owner.take_reply(request), None, "a reply is taken once");
    }
}
