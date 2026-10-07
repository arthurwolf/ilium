//! Final request custody lives OUTSIDE the cancelable publication future.
//! Queue acceptance and a physical stream-flush barrier are distinct facts.
use crate::{
    connection::{RequestFlushReceipt, RequestSender},
    ipc_preparation::AdmittedRequest,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FlushStatus {
    NotSubmitted,
    Pending,
    Confirmed,
    ReceiptClosed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DrainCause {
    WriterClosed,
    FlushReceiptClosed,
    Deadline,
    PrefixOverflow,
    Aborted,
}
impl std::fmt::Display for DrainCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for DrainCause {}

#[derive(Default)]
pub(crate) struct ShutdownRequests {
    pending: Option<std::vec::IntoIter<AdmittedRequest>>,
    accepted_prefix: usize,
    flush: Option<RequestFlushReceipt>,
}
impl ShutdownRequests {
    #[cfg(test)]
    pub(crate) fn from_batch(requests: Vec<AdmittedRequest>) -> Self {
        Self {
            pending: Some(requests.into_iter()),
            ..Self::default()
        }
    }
    /// Caller adopts only after earlier publication completed. Refusal keeps
    /// both this original tail and the actual supplied batch without copying.
    pub(crate) fn adopt_batch(
        &mut self,
        requests: Vec<AdmittedRequest>,
    ) -> Result<(), Vec<AdmittedRequest>> {
        if !self.pending_originals().is_empty() || self.flush.is_some() {
            return Err(requests);
        }
        self.pending = Some(requests.into_iter());
        Ok(())
    }
    pub(crate) fn pending_originals(&self) -> &[AdmittedRequest] {
        self.pending
            .as_ref()
            .map_or(&[], |pending| pending.as_slice())
    }
    pub(crate) fn accepted_prefix(&self) -> usize {
        self.accepted_prefix
    }
    pub(crate) fn flush_status(&self) -> FlushStatus {
        match &self.flush {
            None => FlushStatus::NotSubmitted,
            Some(flush) if flush.acknowledged() => FlushStatus::Confirmed,
            Some(flush) if flush.failure().is_some() => FlushStatus::ReceiptClosed,
            Some(_) => FlushStatus::Pending,
        }
    }
    /// Whether final cleanup must retain this publication owner in its error.
    pub(crate) fn has_unresolved_publication(&self) -> bool {
        !self.pending_originals().is_empty()
            || match self.flush_status() {
                FlushStatus::NotSubmitted => self.accepted_prefix() != 0,
                FlushStatus::Pending | FlushStatus::ReceiptClosed => true,
                FlushStatus::Confirmed => false,
            }
    }
    pub(crate) async fn publish(&mut self, sender: &RequestSender) -> Result<(), DrainCause> {
        while !self.pending_originals().is_empty() {
            let permit = sender
                .reserve_publication()
                .await
                .map_err(|_| DrainCause::WriterClosed)?;
            let accepted = self
                .accepted_prefix
                .checked_add(1)
                .ok_or(DrainCause::PrefixOverflow)?;
            // No await, cancellation point or reconstruction separates taking
            // this exact admitted original from publishing through its slot.
            let request = self
                .pending
                .as_mut()
                .expect("nonempty original batch")
                .next()
                .expect("observed original head");
            permit.publish(request);
            self.accepted_prefix = accepted;
        }
        Ok(())
    }
    pub(crate) async fn finish(&mut self, sender: &RequestSender) -> Result<(), DrainCause> {
        self.publish(sender).await?;
        if self.flush.is_none() {
            let permit = sender
                .reserve_publication()
                .await
                .map_err(|_| DrainCause::WriterClosed)?;
            self.flush = Some(permit.publish_flush());
        }
        self.flush
            .as_mut()
            .expect("original flush receipt stored")
            .observe()
            .await
            .map_err(|_| DrainCause::FlushReceiptClosed)
    }
    pub(crate) fn into_failure(self, cause: DrainCause) -> ShutdownRequestsError {
        ShutdownRequestsError {
            cause,
            custody: std::sync::Mutex::new(self),
            additional: Vec::new(),
            other: None,
        }
    }
}

/// Typed final failure owns every unpublished original and its SAME guard,
/// plus the accepted-prefix count and actual submitted barrier receiver.
/// No replay operation exists for the prefix, whose socket fate may be partial.
pub(crate) struct ShutdownRequestsError {
    cause: DrainCause,
    custody: std::sync::Mutex<ShutdownRequests>,
    additional: Vec<AdmittedRequest>,
    other: Option<Box<crate::error::ClientError>>,
}
impl ShutdownRequestsError {
    #[cfg(test)]
    pub(crate) fn additional_originals(&self) -> &[AdmittedRequest] {
        &self.additional
    }
    pub(crate) fn with_cleanup(
        mut self,
        additional: Vec<AdmittedRequest>,
        other: Option<crate::error::ClientError>,
    ) -> Self {
        self.additional = additional;
        self.other = other.map(Box::new);
        self
    }
    pub(crate) fn inspect<R>(&self, inspect: impl FnOnce(&ShutdownRequests) -> R) -> R {
        let custody = self
            .custody
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        inspect(&custody)
    }
}
impl std::fmt::Debug for ShutdownRequestsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inspect(|custody| {
            f.debug_struct("ShutdownRequestsError")
                .field("cause", &self.cause)
                .field(
                    "unpublished_originals",
                    &(custody.pending_originals().len() + self.additional.len()),
                )
                .field(
                    "accepted_transport_queue_prefix",
                    &custody.accepted_prefix(),
                )
                .field("actual_flush_status", &custody.flush_status())
                .finish()
        })
    }
}
impl std::fmt::Display for ShutdownRequestsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inspect(|custody| write!(f,
            "request drain {}: {} original requests retained, {} accepted to transport queue, flush {:?}; server acceptance unknown; no automatic prefix replay",
            self.cause, custody.pending_originals().len() + self.additional.len(), custody.accepted_prefix(), custody.flush_status()))
    }
}
impl std::error::Error for ShutdownRequestsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.other
            .as_deref()
            .map(|error| error as &(dyn std::error::Error + 'static))
            .or(Some(&self.cause))
    }
}
