//! Single-threaded task executor for awaited host calls (rust-sdk.md "Awaiting host calls").

use std::future::Future;

/// Starts a task on the instance executor; it advances when completions arrive.
pub fn spawn<F>(_future: F)
where
    F: Future<Output = ()> + 'static,
{
    unimplemented!("rust-sdk.task-spawn")
}
