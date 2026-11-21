//! Admitted allocation and I/O boundary for outgoing polling projections.
use std::sync::Arc;

use ilium_execution::{Job, JobContext, JobCost};
use ilium_ipc::polling::{HistoryCursor, PollingRejection, PollingReply, PollingSnapshot};
use tokio::sync::RwLock;

pub(crate) enum ProjectionJob {
    Snapshot(Arc<RwLock<PollingSnapshot>>),
    History {
        owner: Arc<crate::polling_ledger::LedgerOwner>,
        cursor: Option<HistoryCursor>,
        limit: u16,
    },
    Rejected(PollingRejection),
}

impl ProjectionJob {
    pub(crate) fn cost(&self) -> JobCost {
        match self {
            Self::Rejected(_) => JobCost {
                input_bytes: 4096,
                result_bytes: 4096,
            },
            _ => JobCost {
                input_bytes: 32 * 1024 * 1024,
                result_bytes: 2 * 1024 * 1024,
            },
        }
    }
}

impl Job for ProjectionJob {
    type Output = PollingReply;
    type Error = String;

    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        if context.stop_requested() {
            return Err("polling projection was cancelled before execution".into());
        }
        match self {
            Self::Snapshot(snapshot) => Ok(PollingReply::Snapshot(Box::new(
                snapshot.blocking_read().clone(),
            ))),
            Self::Rejected(reason) => Ok(PollingReply::Rejected(reason)),
            Self::History {
                owner,
                cursor,
                limit,
            } => match crate::polling_history::read(owner, cursor, limit) {
                Ok(page) => Ok(PollingReply::History(page)),
                Err(crate::polling_ledger::LedgerError::StaleCursor) => {
                    Ok(PollingReply::Rejected(PollingRejection::StaleCursor))
                }
                Err(error) => Err(error.to_string()),
            },
        }
    }
}
