//! Single-threaded task executor for awaited host calls (rust-sdk.md "Awaiting host calls").

use std::future::Future;

/// Starts a task on the instance executor; it advances when completions arrive.
pub fn spawn<F>(_future: F)
where
    F: Future<Output = ()> + 'static,
{
    unimplemented!("rust-sdk.task-spawn")
}

#[cfg(test)]
mod spec_tests {
    use super::spawn;
    use std::future::pending;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// REQ: rust-sdk.md "Calls, requests and errors" (a spawned task advances only when its awaited completion arrives)
    #[test]
    fn spawned_task_does_not_advance_before_its_awaited_call_completes() {
        let body_reached = Arc::new(AtomicBool::new(false));
        let body_flag = Arc::clone(&body_reached);
        spawn(async move {
            pending::<()>().await;
            body_flag.store(true, Ordering::SeqCst);
        });
        assert!(!body_reached.load(Ordering::SeqCst));
    }

    /// REQ: rust-sdk.md "Calls, requests and errors" (slow calls are awaited without blocking the caller of spawn)
    #[test]
    fn spawn_returns_while_the_task_waits_on_a_slow_host_call() {
        spawn(async {
            pending::<()>().await;
        });
    }

    /// REQ: rust-sdk.md "Calls, requests and errors" (one single-threaded executor serves every spawned task)
    #[test]
    fn several_spawned_tasks_wait_independently_on_one_executor() {
        let first_done = Arc::new(AtomicBool::new(false));
        let second_done = Arc::new(AtomicBool::new(false));
        let first_flag = Arc::clone(&first_done);
        let second_flag = Arc::clone(&second_done);
        spawn(async move {
            pending::<()>().await;
            first_flag.store(true, Ordering::SeqCst);
        });
        spawn(async move {
            pending::<()>().await;
            second_flag.store(true, Ordering::SeqCst);
        });
        assert!(!first_done.load(Ordering::SeqCst));
        assert!(!second_done.load(Ordering::SeqCst));
    }

    /// REQ: rust-sdk.md "Calls, requests and errors" (empty input: a task with no awaited call is still a valid task)
    #[test]
    fn spawning_a_task_without_any_await_is_accepted() {
        spawn(async {});
    }
}
