//! One admitted I/O catalogue job with retained output and completion wake.
use super::{package_directories, PluginCatalogue};
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, Retained,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Notify;

struct CatalogueJob {
    directories: Vec<PathBuf>,
}
impl Job for CatalogueJob {
    type Output = PluginCatalogue;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        if context.stop_requested() {
            return Err("Animation catalogue discovery cancelled".into());
        }
        let catalogue =
            PluginCatalogue::discover_with_stop(&self.directories, || context.stop_requested());
        if context.stop_requested() {
            return Err("Animation catalogue discovery cancelled".into());
        }
        Ok(catalogue)
    }
}

pub struct CataloguePreparation {
    client: Client,
    notification: Arc<Notify>,
    receipt: Option<Receipt<CatalogueJob>>,
    pub status: Option<String>,
}

impl CataloguePreparation {
    pub fn new(client: Client) -> Self {
        let notification = Arc::new(Notify::new());
        let wake = notification.clone();
        Self {
            client: client.with_completion_wake(move || wake.notify_one()),
            notification,
            receipt: None,
            status: None,
        }
    }
    pub fn notification(&self) -> Arc<Notify> {
        self.notification.clone()
    }
    pub fn is_pending(&self) -> bool {
        self.receipt.is_some()
    }
    pub fn request(&mut self) -> Result<(), String> {
        if self.is_pending() {
            return Ok(());
        }
        let reservation = self
            .client
            .try_reserve(
                Lane::Io,
                JobCost {
                    // ZIP central-directory preflight caps entries/metadata; only a
                    // <=256 KiB manifest is decompressed at a time, never assets.
                    input_bytes: 4 * 1024 * 1024,
                    // Discovery caps serialized metadata to 1 MiB. This holds the
                    // parsed JSON tree, owned display strings, paths and retained result.
                    result_bytes: 64 * 1024 * 1024,
                },
            )
            .map_err(|reason| format!("Animation catalogue admission refused: {reason:?}"))?;
        let job = CatalogueJob {
            directories: package_directories()?,
        };
        self.receipt = Some(reservation.submit(job).map_err(|rejected| {
            format!(
                "Animation catalogue submission refused: {:?}",
                rejected.reason
            )
        })?);
        self.status = Some("Loading animation packages…".into());
        Ok(())
    }
    pub fn collect(&mut self) -> Option<Result<Retained<PluginCatalogue>, String>> {
        let mut receipt = self.receipt.take()?;
        match receipt.try_take() {
            JobPoll::Pending => {
                self.receipt = Some(receipt);
                None
            }
            JobPoll::Ready(outcome) => {
                let error = match outcome.view() {
                    JobOutcome::Finished(Ok(_)) => None,
                    JobOutcome::Finished(Err(error)) => Some(error.clone()),
                    JobOutcome::NotStarted { .. } => {
                        Some("Animation discovery cancelled before execution".into())
                    }
                    JobOutcome::Panicked => Some("Animation discovery failed unexpectedly".into()),
                };
                self.status = error.clone();
                Some(match error {
                    Some(error) => Err(error),
                    None => Ok(outcome.map(|outcome| match outcome {
                        JobOutcome::Finished(Ok(value)) => value,
                        _ => unreachable!("successful owned catalogue outcome was checked"),
                    })),
                })
            }
            JobPoll::Lost | JobPoll::Taken => {
                self.status = Some("Animation catalogue completion unavailable".into());
                Some(Err("Animation catalogue completion unavailable".into()))
            }
        }
    }
}
impl Drop for CataloguePreparation {
    fn drop(&mut self) {
        if let Some(receipt) = &self.receipt {
            receipt.cancel();
        }
    }
}
