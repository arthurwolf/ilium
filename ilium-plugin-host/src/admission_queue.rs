//! Admission queue of sides refused for memory (API-095).
//!
//! Refused sides wait in the order of refusal. One admission per plugin is allowed at a time,
//! and the queue notice names the plugin while its side waits.

use std::collections::{BTreeSet, VecDeque};

/// The waiting sides of plugins refused for memory, admitted first in first out.
#[derive(Debug, Default)]
pub struct AdmissionQueue {
    /// Waiting `(plugin id, side)` pairs in the order of refusal.
    waiting: VecDeque<(String, String)>,
    /// Plugins that currently hold an admission taken from this queue.
    admitted: BTreeSet<String>,
}

impl AdmissionQueue {
    /// An empty queue.
    pub fn new() -> AdmissionQueue {
        AdmissionQueue::default()
    }

    /// Appends one side of a plugin to the back of the queue after a memory refusal.
    pub fn refuse(&mut self, plugin_id: &str, side: &str) {
        self.waiting
            .push_back((plugin_id.to_string(), side.to_string()));
    }

    /// Admits the first waiting side whose plugin has no admitted side yet, when at least one
    /// slot is free. Returns the admitted plugin id.
    pub fn admit_next(&mut self, free_slots: usize) -> Option<String> {
        if free_slots == 0 {
            return None;
        }
        let position = self
            .waiting
            .iter()
            .position(|(plugin_id, _)| !self.admitted.contains(plugin_id))?;
        let (plugin_id, _side) = self.waiting.remove(position)?;
        self.admitted.insert(plugin_id.clone());
        Some(plugin_id)
    }

    /// Ends the admission a plugin holds (its instance ended), so its next waiting side may be
    /// admitted.
    pub fn release(&mut self, plugin_id: &str) {
        self.admitted.remove(plugin_id);
    }

    /// The notice text shown while a side of this plugin waits for memory, or `None`.
    pub fn notice_for(&self, plugin_id: &str) -> Option<String> {
        let (_, side) = self
            .waiting
            .iter()
            .find(|(waiting_id, _)| waiting_id == plugin_id)?;
        Some(format!(
            "plugin {plugin_id}: the {side} side is inactive for lack of memory and waits in the admission queue"
        ))
    }
}
