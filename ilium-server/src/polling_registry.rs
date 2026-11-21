//! Charged native-I/O lifecycle for this server's polling endpoint hint.
use std::path::PathBuf;

use ilium_execution::{JobCost, Lane, Retained};

use crate::execution::ExecutionClient;
use crate::polling_discovery::Publication;

pub(crate) async fn publish(
    execution: &ExecutionClient,
    directory: PathBuf,
    incarnation: uuid::Uuid,
    socket_path: PathBuf,
) -> Result<Retained<Publication>, String> {
    let storage = directory
        .as_os_str()
        .len()
        .checked_add(socket_path.as_os_str().len())
        .and_then(|bytes| bytes.checked_mul(4))
        .and_then(|bytes| bytes.checked_add(16 * 1024))
        .ok_or_else(|| "polling endpoint allocation overflow".to_string())?;
    let mut publication = execution
        .run(
            Lane::Io,
            JobCost {
                input_bytes: storage,
                result_bytes: storage,
            },
            move |_| {
                crate::polling_discovery::publish(&directory, incarnation, &socket_path)
                    .map_err(|error| error.to_string())
            },
        )
        .await
        .map_err(|error| error.to_string())?;
    publication
        .try_reduce_to_result_storage(storage)
        .map_err(|reason| format!("polling endpoint retention failed: {reason:?}"))?;
    Ok(publication)
}

/// Move the original publication and its charge into the cleanup job. If the
/// caller is cancelled, native removal still owns both until physical completion.
/// A crash may leave a hint; discovery never treats a hint as live authority.
pub(crate) async fn remove(
    execution: &ExecutionClient,
    publication: Retained<Publication>,
) -> Result<(), String> {
    execution
        .run(
            Lane::Io,
            JobCost {
                input_bytes: 4096,
                result_bytes: 4096,
            },
            move |_| {
                let removed = publication
                    .view()
                    .remove()
                    .map_err(|error| error.to_string());
                drop(publication);
                removed
            },
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}
