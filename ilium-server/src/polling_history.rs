//! Worker-local repository reads and secret-free polling history projection.
use std::sync::Arc;

use ilium_ipc::polling::{
    HistoryCursor, PollingAttemptState, PollingHistoryPage, PollingHistoryRecord,
};

use crate::polling_ledger::{Ledger, LedgerCursor, LedgerError, LedgerOwner, LedgerRecord};

/// Call only after bounded I/O admission. Reconnect never repeats recovery.
pub(crate) fn read(
    owner: Arc<LedgerOwner>,
    cursor: Option<HistoryCursor>,
    limit: u16,
) -> Result<PollingHistoryPage, LedgerError> {
    let mut ledger = Ledger::connect(owner)?;
    let page = ledger.history_page(
        cursor.map(|value| LedgerCursor {
            revision: value.revision,
            after_sequence: value.after_sequence,
        }),
        u8::try_from(limit).map_err(|_| LedgerError::Invalid)?,
    )?;
    Ok(PollingHistoryPage {
        revision: page.revision,
        records: page
            .records
            .into_iter()
            .map(record)
            .collect::<Result<_, _>>()?,
        next: page.next.map(|value| HistoryCursor {
            revision: value.revision,
            after_sequence: value.after_sequence,
        }),
    })
}

fn nonnegative(value: i64) -> Result<u64, LedgerError> {
    u64::try_from(value).map_err(|_| LedgerError::Invalid)
}

fn record(value: LedgerRecord) -> Result<PollingHistoryRecord, LedgerError> {
    let state = match value.state.as_str() {
        "prepared" => PollingAttemptState::Prepared,
        "dispatched" => PollingAttemptState::Dispatched,
        "completed" => PollingAttemptState::Completed,
        "cancelled" => PollingAttemptState::Cancelled,
        "unknown" => PollingAttemptState::Unknown,
        _ => return Err(LedgerError::Invalid),
    };
    let reservation_active = matches!(
        state,
        PollingAttemptState::Prepared
            | PollingAttemptState::Dispatched
            | PollingAttemptState::Unknown
    );
    Ok(PollingHistoryRecord {
        sequence: value.sequence,
        created_utc: value.created_utc,
        project_id: value.project_id,
        model: value.model,
        state,
        cost_nanodollars: value.cost_nanodollars.map(nonnegative).transpose()?,
        reserved_nanodollars: if reservation_active {
            nonnegative(value.reserved_nanodollars)?
        } else {
            0
        },
        input_tokens: value.input_tokens.map(nonnegative).transpose()?,
        billing_unknown: state == PollingAttemptState::Unknown,
    })
}
