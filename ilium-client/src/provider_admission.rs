//! Synchronous provider admission inside the existing bounded IO job.
//! Break returns the exact original before body invocation; Continue never retries.
//! A body must finish all provider use before returning, including response drops.
//! Never pass a body that returns a future, stream handle, or detached native owner.
use crate::naming_workers::InferenceConcurrencyLimiter;
use ilium_execution::JobContext;
use ilium_platform::file_lock::ExclusiveFileLock;
use std::io;
use std::ops::ControlFlow;
use std::sync::Arc;
/// Run only from an admitted IO job; this function creates no worker or waiter.
/// The cancellation predicate must only perform bounded, nonblocking observation.
/// Break(WouldBlock) is retryable; Break(Interrupted) is terminal cancellation.
/// Every platform error becomes outer Other, retaining its original error as cause.
pub(crate) fn run<T, R>(
    original: T,
    context: JobContext,
    limiter: &Arc<InferenceConcurrencyLimiter>,
    cancelled: impl Fn(&T) -> bool,
    body: impl FnOnce(T, JobContext) -> R,
) -> ControlFlow<(T, io::Error), R> {
    run_with_probe(
        original,
        context,
        limiter,
        cancelled,
        body,
        ilium_platform::provider_admission::try_acquire_finite_provider_slot, // No production override.
    )
}
pub(crate) fn run_with_probe<T, R>(
    original: T,
    context: JobContext,
    limiter: &Arc<InferenceConcurrencyLimiter>,
    cancelled: impl Fn(&T) -> bool,
    body: impl FnOnce(T, JobContext) -> R,
    probe: impl FnOnce() -> io::Result<Option<ExclusiveFileLock>>,
) -> ControlFlow<(T, io::Error), R> {
    // Only the preflight path can return original ownership.
    if context.stop_requested() || cancelled(&original) {
        return ControlFlow::Break((original, interrupted()));
    }
    let Some(mut process_permit) = limiter.try_acquire() else {
        return ControlFlow::Break((original, busy("process provider slots occupied")));
    };
    let host_permit = match probe() {
        Ok(Some(permit)) => permit,
        Ok(None) => {
            return ControlFlow::Break((original, busy("host provider slots occupied")));
        }
        Err(error) => {
            return ControlFlow::Break((original, io::Error::other(error)));
        }
    };
    if context.stop_requested() || cancelled(&original) {
        return ControlFlow::Break((original, interrupted()));
    } // The next statement crosses the irreversible body-invocation boundary.
    process_permit.mark_started();
    let result = body(original, context);
    drop(host_permit);
    drop(process_permit);
    ControlFlow::Continue(result)
}
fn busy(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::WouldBlock, reason)
}
fn interrupted() -> io::Error {
    io::Error::new(
        io::ErrorKind::Interrupted,
        "provider request cancelled before body",
    )
}
