use ilium_core::polling_detection::{
    scheduler::{DecisionQueue, PendingDecision, QueueOffer},
    AgentBinding,
};
use ilium_core::NodeId;

fn candidate(agent: u64, window: u64) -> PendingDecision<u64> {
    PendingDecision {
        binding: AgentBinding {
            session: "synthetic".into(),
            incarnation: "server".into(),
            pane: NodeId(agent),
            generation: 1,
        },
        window_id: window.to_string(),
        payload: window,
    }
}

#[test]
fn global_two_per_agent_one_and_busy_agent_does_not_block_other_agents() {
    let mut queue = DecisionQueue::new(1).unwrap();
    assert_eq!(queue.offer(candidate(1, 11)), QueueOffer::Queued);
    let first = queue.take_ready().unwrap();
    assert_eq!(queue.offer(candidate(1, 12)), QueueOffer::Queued);
    assert_eq!(queue.offer(candidate(2, 21)), QueueOffer::Queued);
    let second = queue.take_ready().unwrap();
    assert_eq!(second.candidate.payload, 21);
    assert_eq!(queue.offer(candidate(3, 31)), QueueOffer::Queued);
    assert!(queue.take_ready().is_none());
    assert!(queue.complete(second.slot));
    let third = queue.take_ready().unwrap();
    assert_eq!(third.candidate.payload, 31);
    assert!(queue.complete(first.slot));
    assert_eq!(queue.take_ready().unwrap().candidate.payload, 12);
    assert!(!queue.complete(first.slot));
}

#[test]
fn latest_pending_window_coalesces_without_losing_its_fair_queue_position() {
    let mut queue = DecisionQueue::new(1).unwrap();
    assert_eq!(queue.offer(candidate(1, 11)), QueueOffer::Queued);
    assert_eq!(queue.offer(candidate(2, 21)), QueueOffer::Queued);
    assert_eq!(queue.offer(candidate(1, 12)), QueueOffer::Coalesced(11));
    assert_eq!(queue.offer(candidate(1, 12)), QueueOffer::Duplicate(12));
    assert_eq!(queue.coalesced(), 1);
    assert_eq!(queue.take_ready().unwrap().candidate.payload, 12);
    assert_eq!(queue.take_ready().unwrap().candidate.payload, 21);
}

#[test]
fn capacity_refusal_does_not_evict_any_of_128_pending_agents() {
    let mut queue = DecisionQueue::new(1).unwrap();
    for agent in 1..=128 {
        assert_eq!(queue.offer(candidate(agent, agent)), QueueOffer::Queued);
    }
    assert_eq!(queue.offer(candidate(129, 129)), QueueOffer::Full(129));
    assert_eq!(queue.dropped_capacity(), 1);
    for agent in 1..=128 {
        let ready = queue.take_ready().unwrap();
        assert_eq!(ready.candidate.payload, agent);
        assert!(queue.complete(ready.slot));
    }
    assert!(queue.take_ready().is_none());
}

#[test]
fn source_closure_drops_pending_but_keeps_physical_active_slots_and_epoch_fences() {
    let mut old = DecisionQueue::new(1).unwrap();
    old.offer(candidate(1, 11));
    let old_request = old.take_ready().unwrap();
    old.offer(candidate(1, 12));
    assert_eq!(old.discard_pending(&candidate(1, 12).binding), Some(12));
    old.offer(candidate(2, 21));
    let second = old.take_ready().unwrap();
    old.offer(candidate(3, 31));
    assert!(old.take_ready().is_none());
    assert!(old.complete(second.slot));
    assert_eq!(old.take_ready().unwrap().candidate.payload, 31);
    let mut replacement = DecisionQueue::new(2).unwrap();
    replacement.offer(candidate(1, 11));
    let new_request = replacement.take_ready().unwrap();
    assert!(!replacement.complete(old_request.slot));
    assert!(replacement.complete(new_request.slot));
}
