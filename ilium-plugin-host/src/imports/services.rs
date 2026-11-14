//! Host implementation of `ilium:host/services`.
//!
//! This import carries no grant context: a call or reply needs the `services.use` or
//! `services.provide` grant (SVC-001), and the import has no way to prove either, so both are
//! refused with `not-permitted`. Replies to calls that were never accepted do not exist, so
//! `take-reply` always reports none (SVC-010).
//!
//! `BrokeredServices` is the import the runtime gives an instance (contract section 19.3):
//! it knows the grants of its instance and queues calls on the `ServiceBroker` the runtime
//! drains in `pump`, where the provider's `service-provider.handle` export is called.

use ilium_plugin_api::generated::host::services::HostServices;
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::RequestId;

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use super::env::RequestIds;
use crate::schemas::{Direction, SchemaViolation, ServiceSchemas};

/// The method every service with a schema answers itself: its reply is the schema file, and
/// the provider is not called. A schema that lists its own `describe` method turns this off,
/// and that call goes to the provider like any other.
pub const DESCRIBE_METHOD: &str = "describe";

/// Largest payload of a service call or reply: 64 KiB, as for command arguments.
const MAX_PAYLOAD_BYTES: usize = 64 * 1024;
/// Most calls waiting for delivery, deferred calls and unread replies kept at once.
const BROKER_CAPACITY: usize = 4096;

/// A call a plugin made that the runtime has not delivered yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingServiceCall {
    /// The call id the provider sees.
    pub call: u64,
    /// The request id the caller holds.
    pub request: RequestId,
    /// The calling plugin.
    pub caller: String,
    /// The service, `<provider plugin>/<name>`.
    pub service: String,
    /// The method.
    pub method: String,
    /// The JSON payload.
    pub payload_json: String,
}

/// A call whose provider answered `deferred`. The service and method stay with it, so the
/// later reply is checked against the method's result schema (gap G13).
#[derive(Debug, Clone, PartialEq, Eq)]
struct DeferredCall {
    provider: String,
    caller: String,
    request: RequestId,
    service: String,
    method: String,
}

/// A reply the result schema of its method refused. The runtime logs each one at level
/// error for the provider (SVC-009).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedReply {
    /// The provider that gave the reply.
    pub provider: String,
    /// The service the call named.
    pub service: String,
    /// The method the call named.
    pub method: String,
    /// The JSON pointer and what is wrong there.
    pub violation: String,
}

/// The end of a service call, for the caller's `request-completed` event.
#[derive(Debug, Clone, PartialEq)]
pub struct FinishedServiceCall {
    /// The calling plugin.
    pub caller: String,
    /// The request id the caller holds.
    pub request: RequestId,
    /// The failure, or none when a reply waits in `take-reply`.
    pub error: Option<PluginError>,
}

/// The state of the services of one runtime.
#[derive(Debug, Default)]
pub struct ServiceBrokerState {
    pending: VecDeque<PendingServiceCall>,
    deferred: BTreeMap<u64, DeferredCall>,
    replies: BTreeMap<(String, u64), String>,
    finished: VecDeque<FinishedServiceCall>,
    /// The request and call ids of this runtime; shared with every other import.
    ids: RequestIds,
    /// The schema files of the provided services (gap G13). Kept here, beside the deferred
    /// calls, so a deferred reply is checked when the provider sends it.
    schemas: ServiceSchemas,
    /// Replies the schemas refused since the runtime last logged them.
    refused_replies: VecDeque<RefusedReply>,
}

/// The services of one runtime, shared by the imports of its instances and its pump.
pub type ServiceBroker = Arc<Mutex<ServiceBrokerState>>;

fn lock(broker: &ServiceBroker) -> MutexGuard<'_, ServiceBrokerState> {
    broker
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl ServiceBrokerState {
    /// A broker whose request and call ids come from `ids`, the allocator of the runtime.
    pub fn with_ids(ids: RequestIds) -> Self {
        Self {
            ids,
            ..Self::default()
        }
    }

    /// The calls waiting for delivery, removed from the queue.
    pub fn take_pending(&mut self) -> Vec<PendingServiceCall> {
        self.pending.drain(..).collect()
    }

    /// The calls that ended since the last drain.
    pub fn take_finished(&mut self) -> Vec<FinishedServiceCall> {
        self.finished.drain(..).collect()
    }

    /// Whether the pump has service work.
    pub fn is_idle(&self) -> bool {
        self.pending.is_empty() && self.finished.is_empty()
    }

    /// Queues a call that came from the other side. The runtime's pump delivers it to the
    /// local provider like any other call; `call.caller` names the remote caller. Fails with
    /// `busy` when too many calls wait.
    pub fn push_remote(&mut self, call: PendingServiceCall) -> Result<(), PluginError> {
        if self.pending.len() >= BROKER_CAPACITY {
            return Err(PluginError::new(
                ErrorCode::Busy,
                "too many service calls are waiting",
            ));
        }
        self.pending.push_back(call);
        Ok(())
    }

    /// Installs the schema file of `service`, replacing an earlier one (gap G13).
    pub fn install_schema(&mut self, service: &str, text: &str) -> Result<(), String> {
        self.schemas.install(service, text)
    }

    /// Whether `service` has a schema file.
    pub fn has_schema(&self, service: &str) -> bool {
        self.schemas.has(service)
    }

    /// The schema text of `service`, when one was installed.
    pub fn schema_text(&self, service: &str) -> Option<String> {
        self.schemas.text(service).map(str::to_owned)
    }

    /// The answer the kernel gives to a `describe` call itself: the schema text, when the
    /// service has a schema that does not list its own `describe` method. `None` sends the
    /// call to the provider.
    pub fn describe_reply(&self, service: &str, method: &str) -> Option<String> {
        if method != DESCRIBE_METHOD || self.schemas.has_method(service, DESCRIBE_METHOD) {
            return None;
        }
        self.schema_text(service)
    }

    /// Checks one half of a call against the schema of its method.
    pub fn check_schema(
        &self,
        service: &str,
        method: &str,
        direction: Direction,
        payload_json: &str,
    ) -> Result<(), SchemaViolation> {
        self.schemas.check(service, method, direction, payload_json)
    }

    /// Records a reply the result schema refused, for the runtime to log.
    pub fn record_refused_reply(&mut self, refused: RefusedReply) {
        if self.refused_replies.len() >= BROKER_CAPACITY {
            self.refused_replies.pop_front();
        }
        self.refused_replies.push_back(refused);
    }

    /// The refused replies since the last drain.
    pub fn take_refused_replies(&mut self) -> Vec<RefusedReply> {
        self.refused_replies.drain(..).collect()
    }

    /// Takes the reply a provider gave to the call of `caller` with `request`.
    pub fn take_reply_of(&mut self, caller: &str, request: RequestId) -> Option<String> {
        self.replies.remove(&(caller.to_owned(), request.0))
    }

    /// A fresh request id and call id for a call the runtime makes on behalf of the other
    /// side.
    pub fn fresh_ids(&self) -> (RequestId, u64) {
        (self.ids.next_request(), self.ids.next_call())
    }

    /// Records the reply of a delivered call.
    pub fn complete(&mut self, call: &PendingServiceCall, reply_json: String) {
        self.finish(&call.caller, call.request, Ok(reply_json));
    }

    /// Records that a delivered call failed.
    pub fn fail(&mut self, call: &PendingServiceCall, error: PluginError) {
        self.finish(&call.caller, call.request, Err(error));
    }

    /// Records that the provider will answer a delivered call later.
    pub fn defer(&mut self, call: &PendingServiceCall, provider: &str) {
        if self.deferred.len() >= BROKER_CAPACITY {
            self.fail(
                call,
                PluginError::new(ErrorCode::Busy, "too many deferred service calls"),
            );
            return;
        }
        self.deferred.insert(
            call.call,
            DeferredCall {
                provider: provider.to_owned(),
                caller: call.caller.clone(),
                request: call.request,
                service: call.service.clone(),
                method: call.method.clone(),
            },
        );
    }

    /// Drops what a stopped plugin left: its unread replies, and every deferred call it
    /// would have answered, which fail for their callers.
    pub fn forget_plugin(&mut self, plugin: &str) {
        self.replies.retain(|(caller, _), _| caller != plugin);
        self.pending.retain(|call| call.caller != plugin);
        let orphaned: Vec<u64> = self
            .deferred
            .iter()
            .filter(|(_, call)| call.provider == plugin || call.caller == plugin)
            .map(|(id, _)| *id)
            .collect();
        for id in orphaned {
            if let Some(call) = self.deferred.remove(&id) {
                if call.caller != plugin {
                    self.finish(
                        &call.caller,
                        call.request,
                        Err(PluginError::new(
                            ErrorCode::Cancelled,
                            "the service provider stopped",
                        )),
                    );
                }
            }
        }
    }

    fn finish(&mut self, caller: &str, request: RequestId, outcome: Result<String, PluginError>) {
        let error = match outcome {
            Ok(reply) => {
                if self.replies.len() >= BROKER_CAPACITY {
                    // The oldest unread reply makes room; its caller never took it.
                    self.replies.pop_first();
                }
                self.replies.insert((caller.to_owned(), request.0), reply);
                None
            }
            Err(error) => Some(error),
        };
        self.finished.push_back(FinishedServiceCall {
            caller: caller.to_owned(),
            request,
            error,
        });
    }
}

/// The service names a manifest offers: `contributes.services.provides[].name`.
pub fn provided_services(manifest: &serde_json::Value) -> Vec<String> {
    manifest
        .get("contributes")
        .and_then(|contributes| contributes.get("services"))
        .and_then(|services| services.get("provides"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|service| service.get("name").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect()
}

/// The services a manifest provides that have no schema file in `broker`. Calls to them run
/// unchecked; the runtime says so once per load (gap G13).
pub fn services_without_schema(
    manifest: &serde_json::Value,
    broker: &ServiceBrokerState,
) -> Vec<String> {
    provided_services(manifest)
        .into_iter()
        .filter(|service| !broker.has_schema(service))
        .collect()
}

/// `ilium:host/services` of one instance over the runtime's broker.
#[derive(Debug)]
pub struct BrokeredServices {
    broker: ServiceBroker,
    plugin: String,
    granted: Vec<String>,
}

impl BrokeredServices {
    /// The services import of `plugin`. `granted` is what the instance holds.
    pub fn new(broker: ServiceBroker, plugin: &str, granted: &[String]) -> Self {
        Self {
            broker,
            plugin: plugin.to_owned(),
            granted: granted.to_vec(),
        }
    }

    /// Whether the instance holds `<family>:<service>`. The grant names one service.
    fn holds(&self, family: &str, service: &str) -> bool {
        self.granted.iter().any(|grant| {
            grant
                .split_once(':')
                .is_some_and(|(held, name)| held == family && name == service)
        })
    }
}

impl HostServices for BrokeredServices {
    fn call(
        &mut self,
        service: String,
        method: String,
        payload_json: String,
    ) -> Result<RequestId, PluginError> {
        if !self.holds("services.use", &service) {
            return Err(not_permitted("services.use"));
        }
        if payload_json.len() > MAX_PAYLOAD_BYTES {
            return Err(PluginError::new(
                ErrorCode::InvalidArgument,
                "the payload is larger than 64 KiB",
            ));
        }
        let mut broker = lock(&self.broker);
        if broker.pending.len() >= BROKER_CAPACITY {
            return Err(PluginError::new(
                ErrorCode::Busy,
                "too many service calls are waiting",
            ));
        }
        let (request, call) = broker.fresh_ids();
        broker.pending.push_back(PendingServiceCall {
            call,
            request,
            caller: self.plugin.clone(),
            service,
            method,
            payload_json,
        });
        Ok(request)
    }

    fn take_reply(&mut self, request: RequestId) -> Option<String> {
        lock(&self.broker)
            .replies
            .remove(&(self.plugin.clone(), request.0))
    }

    fn reply(
        &mut self,
        call: u64,
        outcome: Result<String, PluginError>,
    ) -> Result<(), PluginError> {
        if !self
            .granted
            .iter()
            .any(|grant| grant.starts_with("services.provide:"))
        {
            return Err(not_permitted("services.provide"));
        }
        let mut broker = lock(&self.broker);
        // Only the provider that deferred a call may answer it.
        let owned = broker
            .deferred
            .get(&call)
            .is_some_and(|deferred| deferred.provider == self.plugin);
        if !owned {
            return Err(PluginError::new(
                ErrorCode::NotFound,
                "no deferred call has that id",
            ));
        }
        let Some(deferred) = broker.deferred.remove(&call) else {
            return Ok(());
        };
        // A reply the method's result schema refuses is refused here with invalid-argument,
        // and the caller's request ends as failed(invalid-argument) (gap G13, SVC-009).
        if let Ok(reply_json) = &outcome {
            let checked = broker
                .check_schema(
                    &deferred.service,
                    &deferred.method,
                    Direction::Response,
                    reply_json,
                )
                .map_err(|violation| {
                    format!("the reply does not match the service schema at {violation}")
                });
            if let Err(message) = checked {
                let error = PluginError::new(ErrorCode::InvalidArgument, message.clone());
                broker.finish(&deferred.caller, deferred.request, Err(error.clone()));
                broker.record_refused_reply(RefusedReply {
                    provider: deferred.provider,
                    service: deferred.service,
                    method: deferred.method,
                    violation: message,
                });
                return Err(error);
            }
        }
        broker.finish(&deferred.caller, deferred.request, outcome);
        Ok(())
    }
}

/// Host side of the `services` interface.
#[derive(Debug)]
pub struct ServicesImport;

fn not_permitted(grant: &str) -> PluginError {
    PluginError {
        code: ErrorCode::NotPermitted,
        message: format!("the {grant} grant is required"),
        retry_after_ms: None,
    }
}

impl HostServices for ServicesImport {
    fn call(
        &mut self,
        _service: String,
        _method: String,
        _payload_json: String,
    ) -> Result<RequestId, PluginError> {
        Err(not_permitted("services.use"))
    }

    fn take_reply(&mut self, _request: RequestId) -> Option<String> {
        None
    }

    fn reply(
        &mut self,
        _call: u64,
        _outcome: Result<String, PluginError>,
    ) -> Result<(), PluginError> {
        Err(not_permitted("services.provide"))
    }
}

#[cfg(test)]
mod broker_tests {
    use super::{BrokeredServices, ServiceBroker};
    use ilium_plugin_api::generated::host::services::HostServices;
    use ilium_plugin_api::generated::types::errors::ErrorCode;

    fn grants(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_call_needs_the_use_grant_and_its_reply_reaches_only_the_caller() {
        let broker = ServiceBroker::default();
        let mut caller =
            BrokeredServices::new(broker.clone(), "a.caller", &grants(&["services.use:p/s"]));
        let mut other = BrokeredServices::new(
            broker.clone(),
            "a.other",
            &grants(&["services.use:p/other"]),
        );
        let refused = other.call("p/s".to_owned(), "m".to_owned(), "{}".to_owned());
        assert_eq!(
            refused.map_err(|error| error.code),
            Err(ErrorCode::NotPermitted)
        );

        let request = caller
            .call("p/s".to_owned(), "m".to_owned(), "{}".to_owned())
            .expect("the caller holds services.use");
        let pending = super::lock(&broker).take_pending();
        assert_eq!(pending.len(), 1);
        super::lock(&broker).complete(&pending[0], "\"done\"".to_owned());
        assert_eq!(other.take_reply(request), None);
        assert_eq!(caller.take_reply(request), Some("\"done\"".to_owned()));
        assert_eq!(caller.take_reply(request), None);
        assert_eq!(super::lock(&broker).take_finished().len(), 1);
    }

    #[test]
    fn only_the_provider_that_deferred_a_call_answers_it() {
        let broker = ServiceBroker::default();
        let mut caller = BrokeredServices::new(
            broker.clone(),
            "a.caller",
            &grants(&["services.use:a.provider/s"]),
        );
        let all = grants(&["services.provide:a.provider/s"]);
        let mut provider = BrokeredServices::new(broker.clone(), "a.provider", &all);
        let mut stranger = BrokeredServices::new(broker.clone(), "a.stranger", &all);
        let request = caller
            .call("a.provider/s".to_owned(), "m".to_owned(), "{}".to_owned())
            .expect("the caller holds services.use");
        let pending = super::lock(&broker).take_pending();
        super::lock(&broker).defer(&pending[0], "a.provider");
        let call = pending[0].call;
        assert_eq!(
            stranger
                .reply(call, Ok("1".to_owned()))
                .map_err(|error| error.code),
            Err(ErrorCode::NotFound)
        );
        assert_eq!(provider.reply(call, Ok("1".to_owned())), Ok(()));
        assert_eq!(caller.take_reply(request), Some("1".to_owned()));
        assert_eq!(
            provider
                .reply(call, Ok("2".to_owned()))
                .map_err(|error| error.code),
            Err(ErrorCode::NotFound)
        );
    }

    const SCHEMA: &str = r#"{"lookup": {
        "params": {"type": "object", "required": ["term"]},
        "result": {"type": "object", "required": ["found"]}
    }}"#;

    /// A caller and a provider of `a.provider/s` over one broker with `SCHEMA` installed, and
    /// one call of `lookup` the provider deferred: (caller, provider, request, call).
    fn deferred_lookup() -> (
        ServiceBroker,
        BrokeredServices,
        BrokeredServices,
        ilium_plugin_api::generated::types::ids::RequestId,
        u64,
    ) {
        let broker = ServiceBroker::default();
        super::lock(&broker)
            .install_schema("a.provider/s", SCHEMA)
            .expect("the schema installs");
        let mut caller = BrokeredServices::new(
            broker.clone(),
            "a.caller",
            &grants(&["services.use:a.provider/s"]),
        );
        let provider = BrokeredServices::new(
            broker.clone(),
            "a.provider",
            &grants(&["services.provide:a.provider/s"]),
        );
        let request = caller
            .call(
                "a.provider/s".to_owned(),
                "lookup".to_owned(),
                r#"{"term":"x"}"#.to_owned(),
            )
            .expect("the caller holds services.use");
        let pending = super::lock(&broker).take_pending();
        super::lock(&broker).defer(&pending[0], "a.provider");
        let call = pending[0].call;
        (broker, caller, provider, request, call)
    }

    /// REQ: SVC-009
    #[test]
    fn a_deferred_reply_that_fails_the_result_schema_is_refused_and_fails_the_caller() {
        let (broker, mut caller, mut provider, request, call) = deferred_lookup();
        let refused = provider
            .reply(call, Ok(r#"{"definition":"none"}"#.to_owned()))
            .expect_err("the reply lacks /found");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        assert!(refused.message.contains("/found"), "{}", refused.message);
        assert_eq!(caller.take_reply(request), None);
        let finished = super::lock(&broker).take_finished();
        assert_eq!(finished.len(), 1);
        assert_eq!(
            finished[0].error.as_ref().map(|error| error.code),
            Some(ErrorCode::InvalidArgument)
        );
        let logged = super::lock(&broker).take_refused_replies();
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].provider, "a.provider");
        assert_eq!(logged[0].method, "lookup");
        // The call is answered; a second reply finds nothing.
        assert_eq!(
            provider
                .reply(call, Ok(r#"{"found":true}"#.to_owned()))
                .map_err(|error| error.code),
            Err(ErrorCode::NotFound)
        );
    }

    #[test]
    fn a_deferred_reply_that_matches_or_is_an_error_reaches_the_caller() {
        let (_, mut caller, mut provider, request, call) = deferred_lookup();
        assert_eq!(
            provider.reply(call, Ok(r#"{"found":true}"#.to_owned())),
            Ok(())
        );
        assert_eq!(
            caller.take_reply(request),
            Some(r#"{"found":true}"#.to_owned())
        );

        let (_, mut caller, mut provider, request, call) = deferred_lookup();
        assert_eq!(
            provider.reply(call, Ok(r#"{"error":"no entry"}"#.to_owned())),
            Ok(())
        );
        assert_eq!(
            caller.take_reply(request),
            Some(r#"{"error":"no entry"}"#.to_owned())
        );
    }

    #[test]
    fn describe_is_answered_from_the_schema_unless_the_schema_lists_it() {
        let broker = ServiceBroker::default();
        let mut state = super::lock(&broker);
        assert_eq!(state.describe_reply("a.provider/s", "describe"), None);
        state
            .install_schema("a.provider/s", SCHEMA)
            .expect("installs");
        assert_eq!(
            state.describe_reply("a.provider/s", "describe"),
            Some(SCHEMA.to_owned())
        );
        assert_eq!(state.describe_reply("a.provider/s", "lookup"), None);
        state
            .install_schema("a.provider/own", r#"{"describe": {"result": {}}}"#)
            .expect("installs");
        assert_eq!(state.describe_reply("a.provider/own", "describe"), None);
    }

    #[test]
    fn services_without_schema_names_only_the_unchecked_ones() {
        let broker = ServiceBroker::default();
        super::lock(&broker)
            .install_schema("a.provider/s", SCHEMA)
            .expect("installs");
        let manifest = serde_json::json!({"contributes": {"services": {"provides": [
            {"name": "a.provider/s", "version": "0.1.0", "schema": "schemas/s.json"},
            {"name": "a.provider/bare", "version": "0.1.0"}
        ]}}});
        assert_eq!(
            super::services_without_schema(&manifest, &super::lock(&broker)),
            vec!["a.provider/bare".to_owned()]
        );
    }
}

#[cfg(test)]
mod tests {
    use super::ServicesImport;
    use ilium_plugin_api::generated::host::services::HostServices;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::RequestId;

    const SUGGEST_SERVICE: &str = "com.example.titles/suggest";
    const SUGGEST_METHOD: &str = "suggest";
    const SHORT_PAYLOAD: &str = r#"{"text":"Fix the parser"}"#;

    /// Returns the code of a plugin error, and fails the test if the call succeeded.
    fn code_of<T: std::fmt::Debug>(result: Result<T, PluginError>) -> ErrorCode {
        match result {
            Ok(value) => panic!("expected a plugin error, got {value:?}"),
            Err(error) => error.code,
        }
    }

    fn error_outcome(code: ErrorCode) -> Result<String, PluginError> {
        Err(PluginError {
            code,
            message: "provider failed".to_string(),
            retry_after_ms: None,
        })
    }

    /// REQ: SVC-001
    #[test]
    fn call_without_services_use_grant_is_not_permitted() {
        let mut services = ServicesImport;
        let code = code_of(services.call(
            SUGGEST_SERVICE.to_string(),
            SUGGEST_METHOD.to_string(),
            SHORT_PAYLOAD.to_string(),
        ));
        assert_eq!(code, ErrorCode::NotPermitted);
    }

    /// REQ: SVC-001
    #[test]
    fn call_with_empty_service_name_is_not_permitted_without_grant() {
        let mut services = ServicesImport;
        let code = code_of(services.call(
            String::new(),
            SUGGEST_METHOD.to_string(),
            SHORT_PAYLOAD.to_string(),
        ));
        assert_eq!(code, ErrorCode::NotPermitted);
    }

    /// REQ: SVC-001
    #[test]
    fn call_with_empty_method_and_empty_payload_is_not_permitted_without_grant() {
        let mut services = ServicesImport;
        let code =
            code_of(services.call(SUGGEST_SERVICE.to_string(), String::new(), String::new()));
        assert_eq!(code, ErrorCode::NotPermitted);
    }

    /// REQ: SVC-001
    #[test]
    fn call_with_longest_legal_service_name_is_not_permitted_without_grant() {
        // Plugin-defined names carry a 1 to 64 character name after the slash.
        let longest_name = format!("com.example.titles/{}", "a".repeat(64));
        let mut services = ServicesImport;
        let code = code_of(services.call(
            longest_name,
            SUGGEST_METHOD.to_string(),
            SHORT_PAYLOAD.to_string(),
        ));
        assert_eq!(code, ErrorCode::NotPermitted);
    }

    /// REQ: SVC-001
    #[test]
    fn reply_without_services_provide_grant_is_not_permitted() {
        let mut services = ServicesImport;
        let code = code_of(services.reply(1, Ok(r#"{"title":"Fix the parser"}"#.to_string())));
        assert_eq!(code, ErrorCode::NotPermitted);
    }

    /// REQ: SVC-001
    #[test]
    fn reply_with_error_outcome_without_provide_grant_is_not_permitted() {
        let mut services = ServicesImport;
        let code = code_of(services.reply(1, error_outcome(ErrorCode::Cancelled)));
        assert_eq!(code, ErrorCode::NotPermitted);
    }

    /// REQ: SVC-001
    #[test]
    fn reply_boundary_call_numbers_without_grant_are_not_permitted() {
        let mut services = ServicesImport;
        let zero_call = code_of(services.reply(0, Ok(String::new())));
        let largest_call = code_of(services.reply(u64::MAX, Ok(String::new())));
        assert_eq!(zero_call, ErrorCode::NotPermitted);
        assert_eq!(largest_call, ErrorCode::NotPermitted);
    }

    /// REQ: SVC-010
    #[test]
    fn take_reply_for_unknown_request_is_none() {
        let mut services = ServicesImport;
        assert_eq!(services.take_reply(RequestId(1)), None);
    }

    /// REQ: SVC-010
    #[test]
    fn take_reply_for_boundary_request_ids_is_none() {
        let mut services = ServicesImport;
        assert_eq!(services.take_reply(RequestId(0)), None);
        assert_eq!(services.take_reply(RequestId(u64::MAX)), None);
    }

    /// REQ: SVC-010
    #[test]
    fn take_reply_for_same_unknown_request_twice_stays_none() {
        let mut services = ServicesImport;
        assert_eq!(services.take_reply(RequestId(7)), None);
        assert_eq!(services.take_reply(RequestId(7)), None);
    }
}
