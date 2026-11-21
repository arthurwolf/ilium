//! Sequence-fenced external task observations. Persistence and transport belong to the server.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Stable identity supplied by the producer, scoped to one producer.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ExternalTaskKey {
    pub producer: String,
    pub job: String,
}

/// Producer-supplied presentation, with no implied process ownership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalTaskView {
    pub title: String,
    pub status: String,
}

/// A removal is retained as a tombstone so old updates cannot resurrect a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalTaskObservation {
    pub sequence: u64,
    pub value: Option<ExternalTaskView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalTaskUpdate {
    Applied,
    Replay,
    Stale,
    Conflict,
    Invalid,
    CapacityExceeded,
}

/// A bounded set of sequence-fenced task records, including removal tombstones.
#[derive(Debug, Clone, Default)]
pub struct ExternalTasks {
    records: BTreeMap<ExternalTaskKey, ExternalTaskObservation>,
}

impl ExternalTasks {
    /// Restore a complete ledger, rejecting invalid or duplicate identities.
    pub fn restore(
        entries: Vec<(ExternalTaskKey, ExternalTaskObservation)>,
    ) -> Result<Self, ExternalTaskUpdate> {
        let mut tasks = Self::default();
        for (key, observation) in entries {
            if tasks.records.contains_key(&key) {
                return Err(ExternalTaskUpdate::Conflict);
            }
            let result = tasks.update(key, observation);
            if result != ExternalTaskUpdate::Applied {
                return Err(result);
            }
        }
        Ok(tasks)
    }

    /// Include tombstones in durable snapshots so delayed updates remain fenced.
    pub fn entries(&self) -> impl Iterator<Item = (&ExternalTaskKey, &ExternalTaskObservation)> {
        self.records.iter()
    }

    /// Accept a strictly newer observation; equal sequences are idempotent only for equal values.
    pub fn update(
        &mut self,
        key: ExternalTaskKey,
        observation: ExternalTaskObservation,
    ) -> ExternalTaskUpdate {
        const MAX_RECORDS: usize = 4096;
        fn valid(value: &str, max: usize) -> bool {
            !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
        }
        if !valid(&key.producer, 128)
            || !valid(&key.job, 128)
            || observation.sequence == 0
            || observation
                .value
                .as_ref()
                .is_some_and(|view| !valid(&view.title, 512) || !valid(&view.status, 512))
        {
            return ExternalTaskUpdate::Invalid;
        }
        if let Some(old) = self.records.get(&key) {
            if observation.sequence < old.sequence {
                return ExternalTaskUpdate::Stale;
            }
            if observation.sequence == old.sequence {
                return if *old == observation {
                    ExternalTaskUpdate::Replay
                } else {
                    ExternalTaskUpdate::Conflict
                };
            }
        } else if self.records.len() >= MAX_RECORDS {
            return ExternalTaskUpdate::CapacityExceeded;
        }
        self.records.insert(key, observation);
        ExternalTaskUpdate::Applied
    }

    /// Iterate live tasks; tombstones remain private to the ordering state.
    pub fn live(&self) -> impl Iterator<Item = (&ExternalTaskKey, &ExternalTaskView)> {
        self.records
            .iter()
            .filter_map(|(key, record)| record.value.as_ref().map(|value| (key, value)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removal_fences_stale_updates_and_replays() {
        let mut tasks = ExternalTasks::default();
        let key = ExternalTaskKey {
            producer: "iutube".into(),
            job: "job-1".into(),
        };
        let live = ExternalTaskObservation {
            sequence: 1,
            value: Some(ExternalTaskView {
                title: "Parallax".into(),
                status: "running".into(),
            }),
        };
        assert_eq!(
            tasks.update(key.clone(), live.clone()),
            ExternalTaskUpdate::Applied
        );
        assert_eq!(
            tasks.update(key.clone(), live.clone()),
            ExternalTaskUpdate::Replay
        );
        let removed = ExternalTaskObservation {
            sequence: 2,
            value: None,
        };
        assert_eq!(
            tasks.update(key.clone(), removed.clone()),
            ExternalTaskUpdate::Applied
        );
        assert_eq!(tasks.update(key.clone(), live), ExternalTaskUpdate::Stale);
        assert_eq!(
            tasks.update(key.clone(), removed),
            ExternalTaskUpdate::Replay
        );
        assert_eq!(tasks.live().count(), 0);
        let conflict = ExternalTaskObservation {
            sequence: 2,
            value: Some(ExternalTaskView {
                title: "Parallax".into(),
                status: "running".into(),
            }),
        };
        assert_eq!(tasks.update(key, conflict), ExternalTaskUpdate::Conflict);
    }
}
