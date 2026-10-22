//! Admission queue of sides refused for memory (API-095).
//!
//! Refused sides wait in the order of refusal. One admission per plugin is allowed at a time,
//! and the queue notice names the plugin while its side waits.

/// The waiting sides of plugins refused for memory, admitted first in first out.
#[derive(Debug, Default)]
pub struct AdmissionQueue;

impl AdmissionQueue {
    /// An empty queue.
    pub fn new() -> AdmissionQueue {
        unimplemented!("ilium_plugin_host::admission_queue::AdmissionQueue::new")
    }

    /// Appends one side of a plugin to the back of the queue after a memory refusal.
    pub fn refuse(&mut self, _plugin_id: &str, _side: &str) {
        unimplemented!("ilium_plugin_host::admission_queue::AdmissionQueue::refuse")
    }

    /// Admits the first waiting side whose plugin has no admitted side yet, when at least one
    /// slot is free. Returns the admitted plugin id.
    pub fn admit_next(&mut self, _free_slots: usize) -> Option<String> {
        unimplemented!("ilium_plugin_host::admission_queue::AdmissionQueue::admit_next")
    }

    /// The notice text shown while a side of this plugin waits for memory, or `None`.
    pub fn notice_for(&self, _plugin_id: &str) -> Option<String> {
        unimplemented!("ilium_plugin_host::admission_queue::AdmissionQueue::notice_for")
    }
}
