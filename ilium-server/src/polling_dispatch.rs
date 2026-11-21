//! The durable admission boundary for paid polling decisions.

use crate::polling_ledger::{Completion, Ledger, LedgerError, NewAttempt};
use ilium_core::polling_detection::PollingSettings;

#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error("polling detection is disabled")]
    Disabled,
    #[error("the OpenAI key is missing")]
    MissingKey,
    #[error("the polling ledger is unavailable")]
    LedgerUnavailable,
    #[error("polling settings are invalid")]
    InvalidSettings,
    #[error("the request outcome and billing are unknown")]
    OutcomeUnknown,
    #[error("the dispatch belongs to a previous detector owner")]
    StaleLeadership,
    #[error("polling ledger failure: {0}")]
    Ledger(#[from] LedgerError),
}

/// Owned proof of durable dispatch admission; it borrows no repository lock.
/// Finish every admitted operation, including cancelled or uncertain sends.
pub struct DispatchTicket {
    request_id: String,
    leader_epoch: String,
}

/// Reserve and mark dispatch on the repository's owned I/O worker. Only then
/// send on a provider worker, without retaining a mutable repository borrow.
pub fn begin_dispatch(
    settings: &PollingSettings,
    key: &str,
    ledger: Option<&mut Ledger>,
    attempt: &NewAttempt,
) -> Result<DispatchTicket, DispatchError> {
    if !settings.enabled {
        return Err(DispatchError::Disabled);
    }
    settings
        .validate()
        .map_err(|_| DispatchError::InvalidSettings)?;
    if key.trim().is_empty() {
        return Err(DispatchError::MissingKey);
    }
    let ledger = ledger.ok_or(DispatchError::LedgerUnavailable)?;
    let limit = if settings.unlimited_budget {
        None
    } else {
        Some(
            i64::try_from(
                settings
                    .monthly_limit_microdollars
                    .checked_mul(1000)
                    .ok_or(DispatchError::InvalidSettings)?,
            )
            .map_err(|_| DispatchError::InvalidSettings)?,
        )
    };
    ledger.prepare(attempt, limit)?;
    if let Err(error) = ledger.mark_dispatched(&attempt.request_id) {
        let _ = ledger.cancel_prepared(&attempt.request_id);
        return Err(error.into());
    }
    Ok(DispatchTicket {
        request_id: attempt.request_id.clone(),
        leader_epoch: ledger.leader_epoch().to_owned(),
    })
}

/// Persist a result only through the repository owner that admitted it.
/// Reject stale tickets before any write, including unknown-result transitions.
pub fn finish_dispatch<T>(
    ledger: &mut Ledger,
    ticket: DispatchTicket,
    response: Result<(T, Completion), ()>,
) -> Result<T, DispatchError> {
    if ticket.leader_epoch != ledger.leader_epoch() {
        return Err(DispatchError::StaleLeadership);
    }
    match response {
        Ok((result, completion)) => {
            if let Err(error) = ledger.complete(&ticket.request_id, &completion) {
                let _ = ledger.mark_unknown(&ticket.request_id);
                return Err(error.into());
            }
            Ok(result)
        }
        Err(()) => {
            ledger.mark_unknown(&ticket.request_id)?;
            Err(DispatchError::OutcomeUnknown)
        }
    }
}

/// Synchronous composition for one admitted worker. The concurrent coordinator
/// uses the separate phases. The caller establishes generation and pricing;
/// transport uncertainty conservatively keeps the durable reservation.
pub fn dispatch<T, F>(
    settings: &PollingSettings,
    key: &str,
    mut ledger: Option<&mut Ledger>,
    attempt: &NewAttempt,
    send: F,
) -> Result<T, DispatchError>
where
    F: FnOnce() -> Result<(T, Completion), ()>,
{
    let ticket = begin_dispatch(settings, key, ledger.as_deref_mut(), attempt)?;
    finish_dispatch(
        ledger.ok_or(DispatchError::LedgerUnavailable)?,
        ticket,
        send(),
    )
}
