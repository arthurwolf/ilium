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
    pub fn take_from(
        &self,
        _batch: &mut Vec<crate::generated::types::RequestResult>,
    ) -> Option<Outcome> {
        unimplemented!("rust-sdk.request-await")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::PendingRequest;
    use crate::generated::types::{Outcome, RequestId, RequestRef, RequestResult};

    fn result(request: RequestRef, outcome: Outcome) -> RequestResult {
        RequestResult { request, outcome }
    }

    fn pending_call(request_id: u64) -> PendingRequest {
        PendingRequest {
            request: RequestRef::Call(RequestId(request_id)),
        }
    }

    #[test]
    fn take_from_returns_outcome_of_matching_call_and_removes_it() {
        let handle = pending_call(5001);
        let mut batch = vec![result(RequestRef::Call(RequestId(5001)), Outcome::Ok)];
        assert_eq!(handle.take_from(&mut batch), Some(Outcome::Ok));
        assert!(batch.is_empty());
    }

    #[test]
    fn take_from_leaves_other_completions_in_order() {
        let handle = pending_call(2);
        let mut batch = vec![
            result(RequestRef::Call(RequestId(1)), Outcome::Ok),
            result(RequestRef::Call(RequestId(2)), Outcome::Ok),
            result(RequestRef::Submission(3), Outcome::Ok),
        ];
        assert_eq!(handle.take_from(&mut batch), Some(Outcome::Ok));
        let remaining: Vec<RequestRef> = batch.iter().map(|entry| entry.request.clone()).collect();
        assert_eq!(
            remaining,
            vec![RequestRef::Call(RequestId(1)), RequestRef::Submission(3)]
        );
    }

    #[test]
    fn take_from_returns_cancelled_outcome_unchanged() {
        let handle = pending_call(77);
        let mut batch = vec![result(RequestRef::Call(RequestId(77)), Outcome::Cancelled)];
        assert_eq!(handle.take_from(&mut batch), Some(Outcome::Cancelled));
        assert!(batch.is_empty());
    }

    #[test]
    fn second_take_of_same_request_returns_none() {
        let handle = pending_call(9);
        let mut batch = vec![result(RequestRef::Call(RequestId(9)), Outcome::Ok)];
        assert_eq!(handle.take_from(&mut batch), Some(Outcome::Ok));
        assert_eq!(handle.take_from(&mut batch), None);
    }

    #[test]
    fn take_from_returns_none_when_completion_is_absent() {
        let handle = pending_call(42);
        let mut batch = vec![result(RequestRef::Call(RequestId(43)), Outcome::Ok)];
        assert_eq!(handle.take_from(&mut batch), None);
        assert_eq!(batch.len(), 1);
    }

    /// Manual: empty input: an empty batch has no completion to take
    #[test]
    fn take_from_empty_batch_returns_none() {
        let handle = pending_call(1);
        let mut batch: Vec<RequestResult> = Vec::new();
        assert_eq!(handle.take_from(&mut batch), None);
    }

    /// Manual: `submission(n)` and `call(id)` are different requests even with the same number
    #[test]
    fn submission_handle_does_not_take_call_with_same_number() {
        let handle = PendingRequest {
            request: RequestRef::Submission(8),
        };
        let mut batch = vec![result(RequestRef::Call(RequestId(8)), Outcome::Ok)];
        assert_eq!(handle.take_from(&mut batch), None);
        assert_eq!(batch.len(), 1);
    }
}
