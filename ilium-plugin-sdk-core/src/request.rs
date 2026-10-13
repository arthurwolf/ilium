//! Awaited host calls (rust-sdk.md "Awaiting host calls").

use crate::generated::types::{Outcome, RequestRef};

/// Handle of one in-flight request; completions that an await consumes never reach the handler batch.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingRequest {
    /// The request this handle waits for.
    pub request: RequestRef,
}

impl PendingRequest {
    /// Removes the completion for this request from the batch and returns its outcome, if present.
    pub fn take_from(&self, _batch: &mut Vec<crate::generated::types::RequestResult>) -> Option<Outcome> {
        unimplemented!("rust-sdk.request-await")
    }
}
