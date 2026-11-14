//! Host implementation of `ilium:host/inference`.
//!
//! The import of one plugin instance validates a request, hands it to the [`InferenceBackend`] the
//! process composed (`wiring::wave_two::Backends::inference`) on a worker thread of its own, and
//! queues `request-completed` for the plugin when the backend answers. The plugin then fetches
//! the answer once with `take-completion` or `take-models`.
//!
//! Every pending request belongs to the import of one instance: another plugin cannot take it.
//! Dropping the import (the instance ends) sets the cancel flag, which stops the streaming
//! backend mid-answer and lets waiting workers end without calling it.

use ilium_plugin_api::generated::host::inference::{Completion, CompletionRequest, HostInference};
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::generated::types::Outcome;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::env::{CancelFlag, CancelOnDrop, ImportEnv, WorkerGate};

use ilium_plugin_api::generated::types::errors::ErrorCode;

/// Limit on all message contents together: 4 MiB.
const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
/// Limit on the JSON schema text: 64 KiB.
const MAX_JSON_SCHEMA_BYTES: usize = 64 * 1024;
/// Pending requests (completions and model listings) one plugin may hold.
const PENDING_REQUEST_LIMIT: usize = 64;
/// Backend calls one instance runs at once; later requests wait for a free slot.
const CONCURRENT_CALLS: usize = 4;
/// The grant `complete` and `models` need.
const INFERENCE_GRANT: &str = "inference";

/// Where an instance's completions come from. The server and the client compose one from the
/// user's provider settings (`imports::inference_backend::SettingsInference`); tests use fakes.
pub trait InferenceBackend: Send + Sync {
    /// Runs one completion. A backend that streams checks `cancel` between chunks and returns
    /// `cancelled` once it is set.
    fn complete(
        &self,
        request: &CompletionRequest,
        cancel: &CancelFlag,
    ) -> Result<Completion, PluginError>;

    /// The models the selected provider offers.
    fn models(&self, cancel: &CancelFlag) -> Result<Vec<String>, PluginError>;
}

/// State of one inference request.
#[derive(Debug)]
enum Held {
    /// Accepted, the provider has not answered yet.
    PendingCompletion,
    /// Model list requested, not answered yet.
    PendingModels,
    /// Answered, waiting for one `take-completion`.
    Completed(Completion),
    /// Model list answered, waiting for one `take-models`.
    ModelsListed(Vec<String>),
}

#[derive(Debug, Default)]
struct InferenceState {
    held: HashMap<u64, Held>,
}

fn error(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

fn lock(state: &Mutex<InferenceState>) -> MutexGuard<'_, InferenceState> {
    // Every writer does one insert or remove, so the state is consistent after a panic.
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a worker hands back to the state of its import.
enum Answer {
    Completion(Result<Completion, PluginError>),
    Models(Result<Vec<String>, PluginError>),
}

/// Host side of the `inference` interface of one plugin instance: its pending requests are its
/// own and go away with it. Cloning shares them (a test harness keeps one instance and hands out
/// clones); the workers stop when the last clone drops.
#[derive(Clone)]
pub struct InferenceImport {
    env: ImportEnv,
    backend: Option<Arc<dyn InferenceBackend>>,
    state: Arc<Mutex<InferenceState>>,
    gate: WorkerGate,
    life: Arc<CancelOnDrop>,
}

impl std::fmt::Debug for InferenceImport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InferenceImport")
            .field("plugin", &self.env.plugin())
            .field("has_backend", &self.backend.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for InferenceImport {
    fn default() -> Self {
        Self::new(ImportEnv::unrestricted(), None)
    }
}

impl InferenceImport {
    /// The import of the instance `env` describes, answered by `backend`. Without a backend every
    /// accepted request completes with `unsupported`.
    pub fn new(env: ImportEnv, backend: Option<Arc<dyn InferenceBackend>>) -> Self {
        Self {
            env,
            backend,
            state: Arc::default(),
            gate: WorkerGate::new(CONCURRENT_CALLS),
            life: Arc::new(CancelOnDrop::default()),
        }
    }

    /// Whether this instance holds the `inference` grant.
    fn check_grant(&self) -> Result<(), PluginError> {
        if self.env.holds(INFERENCE_GRANT) {
            Ok(())
        } else {
            Err(error(ErrorCode::NotPermitted, "inference is not granted"))
        }
    }

    /// Records a new pending entry, or `busy` at the 64-request limit.
    fn hold_pending(&self, entry: Held) -> Result<RequestId, PluginError> {
        let mut state = lock(&self.state);
        if state.held.len() >= PENDING_REQUEST_LIMIT {
            return Err(error(
                ErrorCode::Busy,
                "64 inference requests are already pending",
            ));
        }
        let id = self.env.next_request();
        state.held.insert(id.0, entry);
        Ok(id)
    }

    /// Runs `work` on a worker of this instance and completes `id` with its answer.
    fn dispatch(
        &self,
        id: RequestId,
        work: impl FnOnce(&dyn InferenceBackend, &CancelFlag) -> Answer + Send + 'static,
    ) {
        let state = Arc::clone(&self.state);
        let sink = self.env.sink();
        let guard = sink.worker();
        let gate = self.gate.clone();
        let cancel = self.life.flag();
        let backend = self.backend.clone();
        let spawned = std::thread::Builder::new()
            .name("plugin-inference".to_owned())
            .spawn(move || {
                let _guard = guard;
                let answer = match &backend {
                    None => Answer::Completion(Err(error(
                        ErrorCode::Unsupported,
                        "no inference provider is configured on this side",
                    ))),
                    Some(backend) => {
                        let Some(_slot) = gate.enter(&cancel) else {
                            return;
                        };
                        if cancel.is_set() {
                            return;
                        }
                        work(backend.as_ref(), &cancel)
                    }
                };
                // An ended instance takes nothing; its requests are dropped with it.
                if cancel.is_set() {
                    return;
                }
                let outcome = {
                    let mut held = lock(&state);
                    match answer {
                        Answer::Completion(Ok(completion)) => {
                            held.held.insert(id.0, Held::Completed(completion));
                            Outcome::Ok
                        }
                        Answer::Models(Ok(models)) => {
                            held.held.insert(id.0, Held::ModelsListed(models));
                            Outcome::Ok
                        }
                        Answer::Completion(Err(failure)) | Answer::Models(Err(failure)) => {
                            held.held.remove(&id.0);
                            if failure.code == ErrorCode::Cancelled {
                                Outcome::Cancelled
                            } else {
                                Outcome::Failed(failure)
                            }
                        }
                    }
                };
                sink.send(super::env::completion_event(id, outcome));
            });
        if spawned.is_err() {
            lock(&self.state).held.remove(&id.0);
            self.env.complete(
                id,
                Outcome::Failed(error(
                    ErrorCode::Internal,
                    "the inference worker did not start",
                )),
            );
        }
    }
}

impl HostInference for InferenceImport {
    fn complete(&mut self, request: CompletionRequest) -> Result<RequestId, PluginError> {
        self.check_grant()?;
        if request.messages.is_empty() {
            return Err(error(
                ErrorCode::InvalidArgument,
                "messages must not be empty",
            ));
        }
        let total_bytes: usize = request
            .messages
            .iter()
            .map(|message| message.content.len())
            .sum();
        if total_bytes > MAX_MESSAGE_BYTES {
            return Err(error(ErrorCode::InvalidArgument, "messages exceed 4 MiB"));
        }
        if let Some(schema) = &request.json_schema {
            if schema.len() > MAX_JSON_SCHEMA_BYTES {
                return Err(error(
                    ErrorCode::InvalidArgument,
                    "json schema exceeds 64 KiB",
                ));
            }
            if serde_json::from_str::<serde_json::Value>(schema).is_err() {
                return Err(error(
                    ErrorCode::InvalidArgument,
                    "json schema is not valid JSON",
                ));
            }
        }
        let id = self.hold_pending(Held::PendingCompletion)?;
        self.dispatch(id, move |backend, cancel| {
            Answer::Completion(backend.complete(&request, cancel).and_then(|completion| {
                if completion.text.len() > MAX_MESSAGE_BYTES {
                    Err(error(
                        ErrorCode::Internal,
                        "the provider answered with more than 4 MiB",
                    ))
                } else {
                    Ok(completion)
                }
            }))
        });
        Ok(id)
    }

    fn take_completion(&mut self, request: RequestId) -> Option<Completion> {
        let mut state = lock(&self.state);
        match state.held.get(&request.0) {
            Some(Held::Completed(_)) => match state.held.remove(&request.0) {
                Some(Held::Completed(completion)) => Some(completion),
                _ => None,
            },
            _ => None,
        }
    }

    fn models(&mut self) -> Result<RequestId, PluginError> {
        self.check_grant()?;
        let id = self.hold_pending(Held::PendingModels)?;
        self.dispatch(id, |backend, cancel| Answer::Models(backend.models(cancel)));
        Ok(id)
    }

    fn take_models(&mut self, request: RequestId) -> Option<Vec<String>> {
        let mut state = lock(&self.state);
        match state.held.get(&request.0) {
            Some(Held::ModelsListed(_)) => match state.held.remove(&request.0) {
                Some(Held::ModelsListed(models)) => Some(models),
                _ => None,
            },
            _ => None,
        }
    }

    /// Provider calls go to the backend the process composed, so no `inference-provider` call is
    /// ever open on this import and a well-formed reply names an unknown call.
    fn provider_reply(
        &mut self,
        _call: u64,
        outcome: Result<Completion, PluginError>,
    ) -> Result<(), PluginError> {
        if let Ok(completion) = &outcome {
            if completion.text.len() > MAX_MESSAGE_BYTES {
                return Err(error(
                    ErrorCode::InvalidArgument,
                    "reply text exceeds 4 MiB",
                ));
            }
        }
        Err(error(
            ErrorCode::NotFound,
            "no open provider call has this id",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{InferenceBackend, InferenceImport};
    use crate::imports::env::{CancelFlag, ImportEnv};
    use crate::runtime::pending_events::PendingEvents;
    use ilium_plugin_api::generated::host::inference::{
        ChatMessage, Completion, CompletionRequest, HostInference,
    };
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::events::Event;
    use ilium_plugin_api::generated::types::ids::RequestId;
    use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// Limit on all messages together, in content bytes (API page "Limits": 4 MiB).
    const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
    /// Limit on the `json_schema` text (API page "Limits": 64 KiB).
    const MAX_JSON_SCHEMA_BYTES: usize = 64 * 1024;

    /// Answers with the last user message reversed, or waits for cancel when told to block.
    #[derive(Default)]
    struct FakeBackend {
        block: bool,
        calls: AtomicUsize,
    }

    impl InferenceBackend for FakeBackend {
        fn complete(
            &self,
            request: &CompletionRequest,
            cancel: &CancelFlag,
        ) -> Result<Completion, PluginError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.block {
                loop {
                    if cancel.is_set() {
                        return Err(PluginError::new(ErrorCode::Cancelled, "cancelled"));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            let last = request
                .messages
                .last()
                .map(|message| message.content.chars().rev().collect())
                .unwrap_or_default();
            Ok(Completion {
                text: last,
                model: "fake-model".to_owned(),
                input_tokens: 1,
                output_tokens: 1,
            })
        }

        fn models(&self, _cancel: &CancelFlag) -> Result<Vec<String>, PluginError> {
            Ok(vec!["fake-model".to_owned()])
        }
    }

    fn import_with(backend: Arc<FakeBackend>, events: &PendingEvents) -> InferenceImport {
        InferenceImport::new(
            ImportEnv::unrestricted().with_events(events.clone()),
            Some(backend as Arc<dyn InferenceBackend>),
        )
    }

    /// Waits for `request-completed` of `request` and returns its outcome.
    fn completion_of(events: &PendingEvents, request: RequestId) -> Outcome {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            for queued in events.drain() {
                if let Event::RequestCompleted(result) = queued.event {
                    if result.request == RequestRef::Call(request) {
                        return result.outcome;
                    }
                }
            }
            assert!(Instant::now() < deadline, "no completion for {request:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn user_message(content: String) -> ChatMessage {
        ChatMessage {
            role: "user".to_string(),
            content,
        }
    }

    fn request_with(messages: Vec<ChatMessage>, json_schema: Option<String>) -> CompletionRequest {
        CompletionRequest {
            messages,
            max_output_tokens: None,
            json_schema,
            purpose: "unit-test".to_string(),
        }
    }

    /// A valid one-message request whose schema, if any, is provided by the caller.
    fn simple_request(json_schema: Option<String>) -> CompletionRequest {
        request_with(vec![user_message("hello".to_string())], json_schema)
    }

    /// Builds a JSON object text of exactly `total_bytes` bytes, padded inside a string value.
    fn json_schema_of_exact_length(total_bytes: usize) -> String {
        let prefix = r#"{"description":""#;
        let suffix = r#""}"#;
        let padding = total_bytes - prefix.len() - suffix.len();
        format!("{prefix}{}{suffix}", "a".repeat(padding))
    }

    fn expect_error(result: Result<RequestId, PluginError>, expected: ErrorCode) {
        match result {
            Ok(request_id) => panic!("expected {expected:?}, got request {request_id:?}"),
            Err(error) => assert_eq!(error.code, expected),
        }
    }

    fn expect_not_invalid_argument(result: Result<RequestId, PluginError>) {
        if let Err(error) = result {
            assert_ne!(
                error.code,
                ErrorCode::InvalidArgument,
                "valid input refused as invalid-argument: {}",
                error.message
            );
        }
    }

    fn reply_with_text(text: String) -> Completion {
        Completion {
            text,
            model: "unit-test-model".to_string(),
            input_tokens: 1,
            output_tokens: 1,
        }
    }

    /// REQ: API-005
    #[test]
    fn complete_returns_request_id_for_valid_request() {
        let mut import = InferenceImport::default();
        let result = import.complete(simple_request(None));
        assert!(
            result.is_ok(),
            "valid request must return a request id, got {result:?}"
        );
    }

    /// REQ: API-005
    #[test]
    fn a_completion_is_answered_by_the_backend_and_taken_once() {
        let events = PendingEvents::new();
        let backend = Arc::new(FakeBackend::default());
        let mut import = import_with(Arc::clone(&backend), &events);
        let request = import
            .complete(simple_request(None))
            .expect("accepted request");
        assert_eq!(completion_of(&events, request), Outcome::Ok);
        let completion = import.take_completion(request).expect("answer");
        assert_eq!(completion.text, "olleh");
        assert_eq!(completion.model, "fake-model");
        assert_eq!(import.take_completion(request), None);
        assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    }

    /// REQ: API-005
    #[test]
    fn models_are_answered_by_the_backend() {
        let events = PendingEvents::new();
        let mut import = import_with(Arc::new(FakeBackend::default()), &events);
        let request = import.models().expect("accepted listing");
        assert_eq!(completion_of(&events, request), Outcome::Ok);
        assert_eq!(
            import.take_models(request),
            Some(vec!["fake-model".to_owned()])
        );
    }

    #[test]
    fn without_a_backend_a_request_fails_as_unsupported() {
        let events = PendingEvents::new();
        let mut import =
            InferenceImport::new(ImportEnv::unrestricted().with_events(events.clone()), None);
        let request = import
            .complete(simple_request(None))
            .expect("accepted request");
        match completion_of(&events, request) {
            Outcome::Failed(failure) => assert_eq!(failure.code, ErrorCode::Unsupported),
            other => panic!("expected failed(unsupported), got {other:?}"),
        }
        assert_eq!(import.take_completion(request), None);
    }

    #[test]
    fn a_plugin_without_the_grant_is_refused() {
        let mut import = InferenceImport::new(
            ImportEnv::unrestricted().with_grants(vec!["http:example.com".to_owned()]),
            Some(Arc::new(FakeBackend::default()) as Arc<dyn InferenceBackend>),
        );
        expect_error(
            import.complete(simple_request(None)),
            ErrorCode::NotPermitted,
        );
        expect_error(import.models(), ErrorCode::NotPermitted);
    }

    #[test]
    fn another_instance_cannot_take_an_answer() {
        let events = PendingEvents::new();
        let backend = Arc::new(FakeBackend::default());
        let mut first = import_with(Arc::clone(&backend), &events);
        let mut second = import_with(backend, &events);
        let request = first.complete(simple_request(None)).expect("accepted");
        assert_eq!(completion_of(&events, request), Outcome::Ok);
        assert_eq!(second.take_completion(request), None);
        assert!(first.take_completion(request).is_some());
    }

    #[test]
    fn dropping_the_instance_cancels_a_streaming_answer() {
        let events = PendingEvents::new();
        let backend = Arc::new(FakeBackend {
            block: true,
            calls: AtomicUsize::new(0),
        });
        let mut import = import_with(Arc::clone(&backend), &events);
        import.complete(simple_request(None)).expect("accepted");
        let deadline = Instant::now() + Duration::from_secs(5);
        while backend.calls.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < deadline, "the backend was never called");
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(import);
        let deadline = Instant::now() + Duration::from_secs(5);
        while events.workers() > 0 {
            assert!(Instant::now() < deadline, "the worker did not stop");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(events
            .drain()
            .iter()
            .all(|queued| !matches!(queued.event, Event::RequestCompleted(_))));
    }

    /// REQ: API-061
    #[test]
    fn complete_rejects_empty_messages_with_invalid_argument() {
        let mut import = InferenceImport::default();
        let result = import.complete(request_with(Vec::new(), None));
        expect_error(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn complete_rejects_messages_over_4_mib_with_invalid_argument() {
        let mut import = InferenceImport::default();
        let messages = vec![
            user_message("a".repeat(MAX_MESSAGE_BYTES)),
            user_message("b".to_string()),
        ];
        let result = import.complete(request_with(messages, None));
        expect_error(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn complete_accepts_messages_at_exactly_4_mib() {
        let mut import = InferenceImport::default();
        let messages = vec![user_message("a".repeat(MAX_MESSAGE_BYTES))];
        let result = import.complete(request_with(messages, None));
        expect_not_invalid_argument(result);
    }

    /// REQ: API-062
    #[test]
    fn complete_rejects_json_schema_over_64_kib_with_invalid_argument() {
        let mut import = InferenceImport::default();
        let schema = json_schema_of_exact_length(MAX_JSON_SCHEMA_BYTES + 1);
        let result = import.complete(simple_request(Some(schema)));
        expect_error(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn complete_accepts_json_schema_at_exactly_64_kib() {
        let mut import = InferenceImport::default();
        let schema = json_schema_of_exact_length(MAX_JSON_SCHEMA_BYTES);
        let result = import.complete(simple_request(Some(schema)));
        expect_not_invalid_argument(result);
    }

    /// REQ: API-061
    #[test]
    fn complete_rejects_json_schema_that_is_not_json_with_invalid_argument() {
        let mut import = InferenceImport::default();
        let result = import.complete(simple_request(Some("{not json".to_string())));
        expect_error(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn complete_accepts_valid_json_schema() {
        let mut import = InferenceImport::default();
        let schema = r#"{"type":"object","properties":{"tags":{"type":"array"}}}"#.to_string();
        let result = import.complete(simple_request(Some(schema)));
        expect_not_invalid_argument(result);
    }

    /// REQ: API-073
    #[test]
    fn complete_returns_busy_for_65th_pending_request() {
        let events = PendingEvents::new();
        let mut import = import_with(
            Arc::new(FakeBackend {
                block: true,
                calls: AtomicUsize::new(0),
            }),
            &events,
        );
        for accepted_index in 0..64 {
            let result = import.complete(simple_request(None));
            assert!(
                result.is_ok(),
                "request {accepted_index} of 64 must be accepted, got {result:?}"
            );
        }
        expect_error(import.complete(simple_request(None)), ErrorCode::Busy);
    }

    /// REQ: API-005
    #[test]
    fn take_completion_returns_none_for_unknown_request() {
        let mut import = InferenceImport::default();
        assert_eq!(import.take_completion(RequestId(0)), None);
    }

    /// REQ: API-005
    #[test]
    fn take_completion_returns_none_for_largest_request_id() {
        let mut import = InferenceImport::default();
        assert_eq!(import.take_completion(RequestId(u64::MAX)), None);
    }

    /// REQ: API-005
    #[test]
    fn models_returns_request_id_without_invalid_argument() {
        let mut import = InferenceImport::default();
        expect_not_invalid_argument(import.models());
    }

    /// REQ: API-005
    #[test]
    fn take_models_returns_none_for_unknown_request() {
        let mut import = InferenceImport::default();
        assert_eq!(import.take_models(RequestId(0)), None);
    }

    /// REQ: API-002
    #[test]
    fn provider_reply_returns_not_found_for_unknown_call() {
        let mut import = InferenceImport::default();
        let outcome = Err(PluginError {
            code: ErrorCode::Network,
            message: "provider unreachable".to_string(),
            retry_after_ms: None,
        });
        let result = import.provider_reply(999, outcome);
        match result {
            Ok(()) => panic!("unknown call must not be accepted"),
            Err(error) => assert_eq!(error.code, ErrorCode::NotFound),
        }
    }

    /// REQ: API-002
    #[test]
    fn provider_reply_returns_not_found_for_call_zero_with_completion() {
        let mut import = InferenceImport::default();
        let result = import.provider_reply(0, Ok(reply_with_text("ok".to_string())));
        match result {
            Ok(()) => panic!("unknown call must not be accepted"),
            Err(error) => assert_eq!(error.code, ErrorCode::NotFound),
        }
    }

    /// REQ: API-061
    #[test]
    fn provider_reply_rejects_reply_text_over_4_mib_with_invalid_argument() {
        let mut import = InferenceImport::default();
        let oversized_text = "a".repeat(MAX_MESSAGE_BYTES + 1);
        let result = import.provider_reply(1, Ok(reply_with_text(oversized_text)));
        match result {
            Ok(()) => panic!("reply text over 4 MiB must be refused"),
            Err(error) => assert_eq!(error.code, ErrorCode::InvalidArgument),
        }
    }

    /// REQ: API-062
    #[test]
    fn provider_reply_accepts_reply_text_at_exactly_4_mib() {
        let mut import = InferenceImport::default();
        let text = "a".repeat(MAX_MESSAGE_BYTES);
        let result = import.provider_reply(1, Ok(reply_with_text(text)));
        if let Err(error) = result {
            assert_ne!(
                error.code,
                ErrorCode::InvalidArgument,
                "reply text at exactly 4 MiB refused as invalid-argument"
            );
        }
    }
}
