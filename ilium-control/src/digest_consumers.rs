//! Bounded, delivery-specific acknowledgements for monitoring pages.

use ilium_core::NodeId;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

#[derive(Default)]
pub struct DigestConsumers {
    consumers: HashMap<String, Consumer>,
    next_delivery: u64,
}

#[derive(Default)]
struct Consumer {
    acknowledged: HashMap<NodeId, u64>,
    contexts: HashMap<NodeId, u64>,
    pending: BTreeMap<u64, Delivery>,
    committed: VecDeque<u64>,
}

#[derive(PartialEq, Eq)]
struct Delivery {
    cursor: u64,
    evidence: Vec<(NodeId, u64)>,
    contexts: Vec<(NodeId, u64)>,
}

impl DigestConsumers {
    pub fn evidence(&self, consumer: &str) -> HashMap<NodeId, u64> {
        self.consumers
            .get(consumer)
            .map(|state| state.acknowledged.clone())
            .unwrap_or_default()
    }

    pub fn contexts(&self, consumer: &str) -> HashMap<NodeId, u64> {
        self.consumers
            .get(consumer)
            .map(|state| state.contexts.clone())
            .unwrap_or_default()
    }

    /// A cursor alone cannot identify a priority-sorted page. Retain an exact
    /// delivery receipt; omitted federation pages can never be committed by
    /// acknowledging another page with the same safe sequence.
    pub fn stage(
        &mut self,
        consumer: &str,
        cursor: u64,
        delivered: &[(NodeId, u64)],
        contexts: &[(NodeId, u64)],
    ) -> Result<u64, &'static str> {
        if consumer.is_empty() || consumer.len() > 128 || consumer.chars().any(char::is_control) {
            return Err("invalid consumer name");
        }
        if self.consumers.len() >= 256 && !self.consumers.contains_key(consumer) {
            return Err("monitoring consumer capacity reached");
        }
        let delivery = Delivery {
            cursor,
            evidence: delivered.to_vec(),
            contexts: contexts.to_vec(),
        };
        let state = self.consumers.entry(consumer.to_owned()).or_default();
        if let Some((id, _)) = state.pending.iter().find(|(_, page)| **page == delivery) {
            return Ok(*id);
        }
        let nodes: HashSet<_> = state
            .acknowledged
            .keys()
            .chain(state.contexts.keys())
            .chain(delivered.iter().map(|(pane, _)| pane))
            .chain(contexts.iter().map(|(pane, _)| pane))
            .chain(state.pending.values().flat_map(|page| {
                page.evidence
                    .iter()
                    .chain(&page.contexts)
                    .map(|(pane, _)| pane)
            }))
            .collect();
        if nodes.len() > 4096 {
            return Err("monitoring evidence capacity reached");
        }
        self.next_delivery = self
            .next_delivery
            .checked_add(1)
            .ok_or("delivery identity exhausted")?;
        // A reader may discard pages due to its aggregate budget. Eviction
        // invalidates that receipt explicitly, without committing its evidence.
        if state.pending.len() >= 16 {
            state.pending.pop_first();
        }
        state.pending.insert(self.next_delivery, delivery);
        Ok(self.next_delivery)
    }

    pub fn acknowledge(
        &mut self,
        consumer: &str,
        cursor: u64,
        delivery: Option<u64>,
    ) -> Result<(), &'static str> {
        let Some(id) = delivery else {
            return Ok(());
        };
        let state = self
            .consumers
            .get_mut(consumer)
            .ok_or("unknown delivery receipt; read again")?;
        if state.committed.contains(&id) {
            return Ok(());
        }
        let page = state
            .pending
            .get(&id)
            .ok_or("expired delivery receipt; read again")?;
        if page.cursor != cursor {
            return Err("delivery receipt does not match cursor");
        }
        let page = state
            .pending
            .remove(&id)
            .ok_or("unknown delivery receipt")?;
        state.acknowledged.extend(page.evidence);
        state.contexts.extend(page.contexts);
        if state.committed.len() >= 16 {
            state.committed.pop_front();
        }
        state.committed.push_back(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acknowledging_one_delivery_does_not_commit_another_with_the_same_sequence() {
        let mut consumers = DigestConsumers::default();
        let discarded = consumers
            .stage("manager", 0, &[(NodeId(3), 42)], &[])
            .unwrap();
        let shown = consumers
            .stage("manager", 0, &[(NodeId(2), 43)], &[])
            .unwrap();
        assert_ne!(discarded, shown);
        consumers.acknowledge("manager", 0, Some(shown)).unwrap();
        assert_eq!(consumers.evidence("manager").len(), 1);
        assert_eq!(consumers.evidence("manager").get(&NodeId(2)), Some(&43));
        assert!(consumers.evidence("another").is_empty());
        consumers.acknowledge("manager", 0, Some(shown)).unwrap();
    }

    #[test]
    fn repeated_reads_reuse_receipts_and_evicted_pages_never_commit() {
        let mut consumers = DigestConsumers::default();
        let first = consumers
            .stage("manager", 0, &[(NodeId(3), 42)], &[])
            .unwrap();
        for _ in 0..300 {
            assert_eq!(
                consumers
                    .stage("manager", 0, &[(NodeId(3), 42)], &[])
                    .unwrap(),
                first
            );
        }
        for cursor in 1..20 {
            consumers
                .stage("manager", cursor, &[], &[(NodeId(3), cursor)])
                .unwrap();
        }
        assert!(consumers.acknowledge("manager", 0, Some(first)).is_err());
        assert!(consumers.evidence("manager").is_empty());
    }
}
