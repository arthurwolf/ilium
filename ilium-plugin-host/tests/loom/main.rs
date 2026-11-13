//! Loom models of the scheduler (HST-184, HST-185). Run with `--features loom`.
#![cfg(feature = "loom")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use ilium_plugin_api::ids::PluginId;
use ilium_plugin_host::scheduler::{CallKind, Scheduler};

fn who() -> PluginId {
    PluginId("demo.loom".to_owned())
}

/// REQ: HST-184
#[test]
fn hst_184_two_threads_never_run_calls_of_one_store_at_the_same_time() {
    loom::model(|| {
        let scheduler = Arc::new(Scheduler::new(10));
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let base = Instant::now();
        for _ in 0..2 {
            let running = running.clone();
            let peak = peak.clone();
            scheduler.enqueue(
                &who(),
                CallKind::OnEvents,
                base,
                Box::new(move || {
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    running.fetch_sub(1, Ordering::SeqCst);
                }),
            );
        }
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let scheduler = scheduler.clone();
                loom::thread::spawn(move || scheduler.tick(base))
            })
            .collect();
        for handle in handles {
            handle.join().expect("tick thread");
        }
        assert!(peak.load(Ordering::SeqCst) <= 1);
        assert!(scheduler.max_concurrent_calls_in_store(&who()) <= 1);
    });
}

/// REQ: HST-185
#[test]
fn hst_185_concurrent_ticks_run_each_enqueued_call_exactly_once() {
    loom::model(|| {
        let scheduler = Arc::new(Scheduler::new(10));
        let runs = Arc::new(AtomicUsize::new(0));
        let base = Instant::now();
        let counter = runs.clone();
        scheduler.enqueue(
            &who(),
            CallKind::Render,
            base,
            Box::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let scheduler = scheduler.clone();
                loom::thread::spawn(move || scheduler.tick(base))
            })
            .collect();
        for handle in handles {
            handle.join().expect("tick thread");
        }
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    });
}
