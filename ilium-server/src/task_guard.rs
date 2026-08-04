//! Small ownership guard tying a spawned background task's lifetime to
//! whatever scope holds the guard, so a cancelled or panicking owner --
//! not only a clean shutdown path -- is guaranteed to stop it too.
//!
//! `tokio::task::JoinHandle`'s own `Drop` intentionally does not cancel its
//! task (that is what makes "fire and forget" possible at all): it merely
//! detaches the handle, leaving the spawned task running for as long as it
//! wants to keep running. Every long-lived task `crate::run` spawns (the
//! detection loop, scheduled-input executor, snapshot writer, sound actor,
//! optional config watcher) is otherwise only ever aborted by `run`'s own
//! post-`select!` cleanup code -- code that runs only when that `select!`
//! resolves on its own. If `run`'s future is itself cancelled from the
//! outside instead (e.g. a caller holding `run`'s own `JoinHandle` calls
//! `.abort()` on it -- exactly what `ilium-server/tests/common/mod.rs`'s
//! `TestServer::drop` does whenever a test panics or times out before
//! reaching a clean `KillSession`), that cleanup code never runs, and a
//! bare `JoinHandle` local variable would just be silently detached rather
//! than stopped -- leaking every one of those tasks, and transitively the
//! `Arc<ServerState>` (tree, PTYs, connection registry) they keep alive,
//! for the rest of the process.
//!
//! Wrapping each such handle in [`AbortOnDropHandle`] closes that gap for
//! free: dropping a suspended `async fn`'s future also drops every live
//! local variable at its current suspension point, so cancelling `run`'s
//! own task now cancels every child task it owns too, the same way RAII
//! already guarantees for any other resource this crate holds.

use tokio::task::JoinHandle;

/// Owns a spawned task's [`JoinHandle`] and aborts it on drop, in addition
/// to (not instead of) any explicit `.abort()` call already on a normal
/// shutdown path -- `JoinHandle::abort` is a harmless no-op on a task that
/// is already finished or already aborted, so the two never conflict.
/// The handle is optional only so [`AbortOnDropHandle::join`] can take it
/// out: awaiting a task the guard still owns would abort it on drop the moment
/// the await completed.
pub struct AbortOnDropHandle<T>(Option<JoinHandle<T>>);

impl<T> AbortOnDropHandle<T> {
    pub fn new(handle: JoinHandle<T>) -> Self {
        Self(Some(handle))
    }

    /// Requests cancellation without waiting for it to take effect. Safe to
    /// call more than once, including implicitly once more via `Drop`.
    pub fn abort(&self) {
        if let Some(handle) = &self.0 {
            handle.abort();
        }
    }

    /// Waits for the task to finish, reporting how it ended. Consumes the
    /// guard, because a joined handle has nothing left to abort.
    pub async fn join(mut self) -> Result<T, tokio::task::JoinError> {
        let Some(handle) = self.0.take() else {
            unreachable!("the handle is only taken here, and this consumes the guard");
        };
        handle.await
    }
}

impl<T> Drop for AbortOnDropHandle<T> {
    fn drop(&mut self) {
        if let Some(handle) = &self.0 {
            handle.abort();
        }
    }
}
