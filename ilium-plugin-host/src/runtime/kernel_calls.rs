//! Service calls the kernel itself makes (key M3 of the workspace cuts): a kernel feature that
//! a plugin now owns asks the plugin's service for a decision and waits for the answer, on the
//! runtime thread, instead of deciding in kernel code.
//!
//! The call takes the path of a plugin's call through `route_services` without the queue: the
//! provider is the plugin of this side whose manifest provides the service, a loaded provider
//! starts with its `service:` trigger, the request and the reply are checked against the
//! installed schema, and `describe` is answered from the schema. The provider sees the caller
//! [`KERNEL_CALLER`].
//!
//! The kernel waits only for an immediate reply: a provider that answers `deferred` ends the
//! call as failed, because the kernel path that asked holds no request to complete later. A
//! provider on the other side is not reached here; the kernel of that side calls it.
//!
//! [`KernelCallError::Unavailable`] tells the kernel to keep its own behaviour (no provider,
//! the provider is disabled, stopped or failed). [`KernelCallError::Failed`] means the provider
//! ran and refused or broke the contract.

use ilium_plugin_api::generated::exports::service_provider::{ServiceCall, ServiceReply};
use ilium_plugin_api::generated::host::log::Level;
use ilium_plugin_api::generated::types::ids::PluginId;
use ilium_plugin_api::{ErrorCode, PluginError};

use super::{lock, HostRuntime, PluginState, RuntimeError};
use crate::instance::{ExportCall, ExportReply};
use crate::schemas::Direction;

/// The caller id a provider sees in a call the kernel made.
pub const KERNEL_CALLER: &str = "ilium.kernel";

/// Why a kernel call of a plugin service gave no reply.
#[derive(Debug, Clone, PartialEq)]
pub enum KernelCallError {
    /// No plugin of this side runs the service: none provides it, its provider is disabled,
    /// stopped, failed or lives on the other side. The kernel keeps its own behaviour.
    Unavailable(PluginError),
    /// The provider ran and failed, refused the call or broke the service contract.
    Failed(PluginError),
}

impl KernelCallError {
    /// The error with its code and message.
    pub fn error(&self) -> &PluginError {
        match self {
            Self::Unavailable(error) | Self::Failed(error) => error,
        }
    }

    /// Whether the kernel should keep its own behaviour because no provider runs.
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

impl std::fmt::Display for KernelCallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let error = self.error();
        write!(formatter, "{:?}: {}", error.code, error.message)
    }
}

fn unavailable(code: ErrorCode, message: &str) -> KernelCallError {
    KernelCallError::Unavailable(PluginError::new(code, message))
}

fn failed(code: ErrorCode, message: String) -> KernelCallError {
    KernelCallError::Failed(PluginError::new(code, message))
}

/// The end of one kernel call: the reply or why there is none, and the runtime errors of the
/// provider (a trap), which the caller logs.
pub type KernelCallEnd = (Result<String, KernelCallError>, Vec<RuntimeError>);

impl HostRuntime {
    /// The plugin of this runtime whose manifest provides `service`.
    pub(super) fn service_provider(&self, service: &str) -> Option<String> {
        self.plugins
            .iter()
            .find(|(_, record)| {
                crate::imports::services::provided_services(&record.manifest)
                    .iter()
                    .any(|name| name == service)
            })
            .map(|(plugin, _)| plugin.clone())
    }

    /// Calls `method` of `service` for the kernel and returns the provider's immediate reply.
    /// Runs on the runtime thread; the caller pumps the runtime afterwards so the intents the
    /// provider submitted while it answered are routed.
    pub fn call_service_as_kernel(
        &mut self,
        service: &str,
        method: &str,
        payload_json: &str,
    ) -> KernelCallEnd {
        let mut failures = Vec::new();
        let Some(provider) = self.service_provider(service) else {
            let end = Err(unavailable(
                ErrorCode::NotFound,
                "no plugin provides that service",
            ));
            return (end, failures);
        };
        if let Some(schema) = lock(&self.services).describe_reply(service, method) {
            return (Ok(schema), failures);
        }
        if self.state(&provider) == Some(PluginState::Loaded) {
            let local = service.rsplit('/').next().unwrap_or(service);
            failures.extend(self.trigger(&format!("service:{service}")));
            failures.extend(self.trigger(&format!("service:{local}")));
        }
        match self.state(&provider) {
            Some(PluginState::Active) => {}
            Some(PluginState::ManifestOnly) => {
                let end = Err(unavailable(
                    ErrorCode::Unsupported,
                    "the provider runs on the other side",
                ));
                return (end, failures);
            }
            _ => {
                let end = Err(unavailable(
                    ErrorCode::Busy,
                    "the service provider is not running",
                ));
                return (end, failures);
            }
        }
        let request_check =
            lock(&self.services).check_schema(service, method, Direction::Request, payload_json);
        if let Err(violation) = request_check {
            let end = Err(failed(
                ErrorCode::InvalidArgument,
                format!("the request does not match the service schema at {violation}"),
            ));
            return (end, failures);
        }
        let call = ServiceCall {
            call: lock(&self.services).next_call(),
            service: service.to_owned(),
            method: method.to_owned(),
            payload_json: payload_json.to_owned(),
            caller: PluginId(KERNEL_CALLER.to_owned()),
        };
        let end = match self.call_export(&provider, ExportCall::ServiceHandle(call)) {
            Ok(ExportReply::ServiceReply(ServiceReply::Done(reply))) => {
                let reply_check =
                    lock(&self.services).check_schema(service, method, Direction::Response, &reply);
                match reply_check {
                    Ok(()) => Ok(reply),
                    Err(violation) => {
                        let message =
                            format!("the reply does not match the service schema at {violation}");
                        self.log.line(
                            &provider,
                            self.side,
                            Level::Error,
                            &format!("service {service} method {method}: {message}"),
                        );
                        Err(failed(ErrorCode::InvalidArgument, message))
                    }
                }
            }
            Ok(ExportReply::ServiceReply(ServiceReply::Deferred)) => Err(failed(
                ErrorCode::Unsupported,
                "the provider deferred its reply; the kernel waits only for an immediate reply"
                    .to_owned(),
            )),
            Ok(_) => Err(failed(
                ErrorCode::Internal,
                "the provider gave no service reply".to_owned(),
            )),
            Err(error) => {
                failures.push(error);
                Err(failed(
                    ErrorCode::Internal,
                    "the service provider failed".to_owned(),
                ))
            }
        };
        (end, failures)
    }
}
