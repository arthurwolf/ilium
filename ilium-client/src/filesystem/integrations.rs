//! Typed ordered integration mutations and replaceable maintenance admission.
use super::ordered::{OrderedWriter, WriteCompletion, WriteId};
use crate::agent_feature_setup::{AgentFeature, FeatureSetupStatus};
use ilium_execution::{Client, Job, JobContext, JobCost};
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct FeatureTarget {
    pub feature: AgentFeature,
    pub path: PathBuf,
    pub project: Option<PathBuf>,
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Maintenance {
    pub targets: Vec<FeatureTarget>,
    pub automatic: bool,
}
impl Maintenance {
    pub fn checked(&self) -> Result<(), String> {
        if self.targets.capacity() > 256 {
            return Err("Integration target limit exceeded (256); status remains pending".into());
        }
        let mut bytes = 0_usize;
        for target in &self.targets {
            if target.path.capacity() > 64 * 1024
                || target
                    .project
                    .as_ref()
                    .is_some_and(|path| path.capacity() > 64 * 1024)
            {
                return Err("Integration target path exceeds retained limit".into());
            }
            bytes += target.path.capacity() + target.project.as_ref().map_or(0, PathBuf::capacity);
        }
        if bytes > 2 * 1024 * 1024 {
            return Err("Integration snapshot exceeds 2 MiB retained limit".into());
        }
        Ok(())
    }
}
pub(crate) enum IntegrationJob {
    Maintain(Maintenance),
    Install { targets: Vec<FeatureTarget> },
}
pub(crate) struct IntegrationResult {
    pub statuses: Vec<(FeatureTarget, Result<FeatureSetupStatus, String>)>,
    pub failures: Vec<String>,
}
fn bounded_error(error: impl std::fmt::Display) -> String {
    let mut value = error.to_string();
    let mut end = value.len().min(4096);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value.into_boxed_str().into_string()
}
fn install(target: &FeatureTarget) -> Result<(), String> {
    if target
        .project
        .as_ref()
        .is_some_and(|project| !project.is_dir())
    {
        return Err(bounded_error(format!(
            "Project unavailable: {}",
            target
                .project
                .as_ref()
                .map_or(target.path.as_path(), |path| path.as_path())
                .display()
        )));
    }
    // Fresh status precedes mutation, so a future managed contract is never overwritten.
    let status =
        crate::agent_feature_setup::status(&target.path, target.feature).map_err(bounded_error)?;
    if status == FeatureSetupStatus::ManagedFuture {
        return Ok(());
    }
    crate::agent_feature_setup::install(&target.path, target.feature).map_err(bounded_error)?;
    Ok(())
}

impl IntegrationJob {
    pub const COST: JobCost = JobCost {
        input_bytes: 32 * 1024 * 1024,
        result_bytes: 16 * 1024 * 1024,
    };

    fn cost(&self) -> JobCost {
        Self::COST
    }
}
impl Job for IntegrationJob {
    type Output = IntegrationResult;
    type Error = String;
    fn run(self, _context: JobContext) -> Result<Self::Output, String> {
        // An admitted mutation is never cancelled or replayed automatically.
        let mut result = IntegrationResult {
            statuses: Vec::new(),
            failures: Vec::new(),
        };
        match self {
            Self::Maintain(snapshot) => {
                snapshot.checked()?;
                for target in snapshot.targets {
                    let current = crate::agent_feature_setup::status(&target.path, target.feature)
                        .map_err(bounded_error);
                    // Automatic upkeep only writes the global instruction files, which
                    // already cover every project; per-project copies would duplicate
                    // them, and a new project AGENTS.md would hide that project's
                    // CLAUDE.md from Codex. Project files are only installed on request.
                    if snapshot.automatic
                        && target.project.is_none()
                        && !matches!(
                            current,
                            Ok(FeatureSetupStatus::Managed | FeatureSetupStatus::ManagedFuture)
                        )
                    {
                        if let Err(error) = install(&target) {
                            result.failures.push(error);
                        }
                    }
                    let status = crate::agent_feature_setup::status(&target.path, target.feature)
                        .map_err(bounded_error);
                    result.statuses.push((target, status));
                }
            }
            Self::Install { targets } => {
                for target in targets {
                    match install(&target) {
                        Ok(()) => {}
                        Err(error) => result.failures.push(error),
                    }
                    let status = crate::agent_feature_setup::status(&target.path, target.feature)
                        .map_err(bounded_error);
                    if let Err(error) = &status {
                        result
                            .failures
                            .push(format!("Managed instruction readback failed: {error}"));
                    }
                    result.statuses.push((target.clone(), status));
                }
            }
        }
        Ok(result)
    }
}
#[derive(Clone)]
pub(crate) enum IntegrationIntent {
    Maintenance(Maintenance),
    Install {
        targets: Vec<FeatureTarget>,
        prompt: Option<crate::setup_prompt::SetupPromptState>,
    },
}
pub(crate) struct IntegrationFiles {
    client: Client,
    writer: OrderedWriter<IntegrationJob>,
    intents: Vec<(WriteId, IntegrationIntent)>,
    desired: Option<Maintenance>,
    closing: bool,
}
impl IntegrationFiles {
    pub fn new(client: Client, ready: Arc<tokio::sync::Notify>) -> Self {
        Self {
            client: client.clone(),
            writer: OrderedWriter::new(client, ready),
            intents: Vec::new(),
            desired: None,
            closing: false,
        }
    }
    pub fn publication_hold(
        &self,
    ) -> Result<ilium_execution::Retained<()>, ilium_execution::RejectReason> {
        self.client
            .try_reserve_external(JobCost {
                input_bytes: 16 * 1024 * 1024,
                result_bytes: 0,
            })?
            .retain(())
            .map_err(|rejected| rejected.reason)
    }
    pub(crate) fn notification(&self) -> Arc<tokio::sync::Notify> {
        self.writer.notification()
    }
    pub fn pending(&self) -> usize {
        self.writer.pending() + usize::from(self.desired.is_some())
    }
    pub fn want(&mut self, mut snapshot: Maintenance) -> Result<(), String> {
        snapshot.checked()?;
        if self.closing {
            return Err("Integration maintenance admission closed".into());
        }
        if self.intents.iter().any(|(_,intent)|matches!(intent,IntegrationIntent::Maintenance(current) if current.targets==snapshot.targets && (!snapshot.automatic || current.automatic))){return Ok(());}
        if let Some(desired) = &self.desired {
            snapshot.automatic |= desired.automatic;
        }
        self.desired = Some(snapshot);
        Ok(())
    }
    pub fn enqueue(&mut self, intent: IntegrationIntent) -> Result<(), String> {
        if self.closing {
            return Err("Integration writer admission closed".into());
        }
        if self
            .intents
            .iter()
            .any(|(_, current)| same_explicit(current, &intent))
        {
            return Err("This integration operation is already pending".into());
        }
        let job = match &intent {
            IntegrationIntent::Maintenance(snapshot) => {
                snapshot.checked()?;
                IntegrationJob::Maintain(snapshot.clone())
            }
            IntegrationIntent::Install { targets, .. } => {
                Maintenance {
                    targets: targets.clone(),
                    automatic: false,
                }
                .checked()?;
                IntegrationJob::Install {
                    targets: targets.clone(),
                }
            }
        };
        let cost = job.cost();
        let id = self
            .writer
            .enqueue_detailed(cost, job)
            .map_err(|rejected| {
                format!("Integration operation not admitted: {:?}", rejected.failure)
            })?;
        self.intents.push((id, intent));
        Ok(())
    }
    pub fn poll(&mut self) -> Option<(IntegrationIntent, WriteCompletion<IntegrationJob>)> {
        if let Some(completion) = self.writer.poll() {
            let id = match &completion {
                WriteCompletion::Outcome { id, .. }
                | WriteCompletion::Rejected { id, .. }
                | WriteCompletion::Lost { id } => *id,
            };
            let index = self
                .intents
                .iter()
                .position(|(current, _)| *current == id)?;
            let (_, intent) = self.intents.remove(index);
            return Some((intent, completion));
        }
        if let Some(snapshot) = self.desired.take() {
            if self
                .enqueue(IntegrationIntent::Maintenance(snapshot.clone()))
                .is_err()
            {
                self.desired = Some(snapshot);
            }
        }
        None
    }
    pub fn close_admission(&mut self) {
        self.closing = true;
        self.desired = None;
        self.writer.close_admission();
    }
}
fn same_explicit(first: &IntegrationIntent, second: &IntegrationIntent) -> bool {
    match (first, second) {
        (
            IntegrationIntent::Install {
                targets: a,
                prompt: a_prompt,
            },
            IntegrationIntent::Install {
                targets: b,
                prompt: b_prompt,
            },
        ) => {
            a == b
                && match (a_prompt, b_prompt) {
                    (Some(a), Some(b)) => Arc::ptr_eq(&a.identity, &b.identity) && a == b,
                    (None, None) => true,
                    _ => false,
                }
        }
        _ => false,
    }
}
