use ilium_core::polling_detection::{Admission, PollingScheduler};

#[test]
fn scheduler_coalesces_per_agent_and_enforces_global_concurrency() {
    let mut scheduler = PollingScheduler::new(2, 128);
    assert_eq!(scheduler.offer("a"), Admission::Queued);
    assert_eq!(scheduler.offer("a"), Admission::Coalesced);
    assert_eq!(scheduler.coalesced(), 1);
    assert_eq!(scheduler.offer("b"), Admission::Queued);
    assert_eq!(scheduler.offer("c"), Admission::Queued);
    assert_eq!(scheduler.start_next().as_deref(), Some("a"));
    assert_eq!(scheduler.start_next().as_deref(), Some("b"));
    assert_eq!(scheduler.start_next(), None);
    assert_eq!(scheduler.offer("a"), Admission::Queued);
    assert_eq!(scheduler.offer("a"), Admission::Coalesced);
    assert_eq!(scheduler.start_next(), None);
    scheduler.finish("a");
    assert_eq!(scheduler.start_next().as_deref(), Some("c"));
    scheduler.finish("b");
    assert_eq!(scheduler.start_next().as_deref(), Some("a"));
}

#[test]
fn full_queue_is_bounded_and_disable_drops_pending_work() {
    let mut scheduler = PollingScheduler::new(1, 1);
    assert_eq!(scheduler.offer("a"), Admission::Queued);
    assert_eq!(scheduler.offer("b"), Admission::DroppedCapacity);
    assert_eq!(scheduler.dropped_capacity(), 1);
    scheduler.clear_pending();
    assert_eq!(scheduler.start_next(), None);
}
