use super::configuration::ProjectRead;
use super::configuration::{ConfigurationChange, ConfigurationWrite};
use super::ordered::{OrderedWriter, WriteCompletion, WriteId};
use ilium_execution::{Client, JobOutcome, JobPoll, Lane, Receipt};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Default)]
pub(crate) enum InferenceSaveState {
    #[default]
    Unsaved,
    Pending {
        operation: Arc<()>,
        budget_dialog: Option<(Arc<()>, u32)>,
    },
    Durable,
}

pub enum AgentSetupDialog {
    Path {
        feature: crate::agent_feature_setup::AgentFeature,
        value: String,
    },
    Suppression(crate::setup_prompt::SetupPromptState),
}
#[derive(Clone)]
pub(crate) struct AnimationPickerSave {
    pub token: Arc<()>,
    pub project_path: PathBuf,
    pub candidate: ilium_ambient::GeoLocation,
    // Last: the admitted candidate allocation is destroyed before its lease.
    pub location_storage: Option<Arc<ilium_execution::StorageAdmission>>,
}
pub(crate) enum ConfigurationIntent {
    ValueDialog {
        token: Arc<()>,
    },
    Inference {
        operation: Arc<()>,
    },
    InferenceValueDialog {
        operation: Arc<()>,
        token: Arc<()>,
    },
    Plain {
        label: &'static str,
        success: Option<&'static str>,
    },
    Session {
        desired: crate::config::SessionSettings,
    },
    SessionValueDialog {
        desired: crate::config::SessionSettings,
        token: Arc<()>,
    },
    AgentSetup {
        desired: crate::config::AgentSetupSettings,
        dialog: Option<AgentSetupDialog>,
    },
    TextTriggers {
        dialog: Option<ilium_ipc::TextTrigger>,
    },
    Onboarding {
        revision: u64,
        dismiss: bool,
    },
    Animation {
        path: PathBuf,
        picker: Option<AnimationPickerSave>,
        value_dialog: Option<Arc<()>>,
        desired: Box<crate::background_animation::AnimationSettings>,
        // Includes optimistic, intent, rollback and presentation copies.
        // Last so the intent's payload dies before the lease.
        location_storage: Option<Arc<ilium_execution::StorageAdmission>>,
    },
    Separators,
}
pub struct ConfigurationFiles {
    writer: OrderedWriter<ConfigurationWrite>,
    client: Client,
    ready: Arc<tokio::sync::Notify>,
    project_read: Option<(PathBuf, Receipt<ProjectRead>)>,
    desired_project: Option<PathBuf>,
    intents: Vec<(WriteId, ConfigurationIntent)>,
}
impl ConfigurationFiles {
    pub fn new(client: Client, ready: Arc<tokio::sync::Notify>) -> Self {
        let wake = Arc::clone(&ready);
        let client = client.with_completion_wake(move || wake.notify_one());
        Self {
            writer: OrderedWriter::new(client.clone(), Arc::clone(&ready)),
            client,
            ready,
            project_read: None,
            desired_project: None,
            intents: Vec::new(),
        }
    }
    /// Upgrade this host's already-admitted draft write to an explicit Enter
    /// confirmation. The original operation and write remain unchanged.
    pub(crate) fn attach_inference_value_dialog(
        &mut self,
        operation: &Arc<()>,
        token: Arc<()>,
    ) -> Result<(), String> {
        let intent = self
            .intents
            .iter_mut()
            .find_map(|(_, intent)| match intent {
                ConfigurationIntent::Inference { operation: current }
                    if Arc::ptr_eq(current, operation) =>
                {
                    Some(intent)
                }
                _ => None,
            })
            .ok_or("Inference save receipt is no longer available; retry after it settles")?;
        *intent = ConfigurationIntent::InferenceValueDialog {
            operation: operation.clone(),
            token,
        };
        Ok(())
    }

    pub(crate) fn notification(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.ready)
    }
    pub fn pending(&self) -> usize {
        self.writer.pending()
            + usize::from(self.project_read.is_some())
            + usize::from(self.desired_project.is_some())
    }
    pub fn close_admission(&mut self) {
        self.writer.close_admission();
        self.desired_project = None;
        if let Some((_, receipt)) = &self.project_read {
            receipt.cancel();
        }
    }
    pub fn request_project(&mut self, path: PathBuf) -> Result<(), String> {
        if path.capacity() > 64 * 1024 {
            return Err("Project path exceeds retained limit".into());
        }
        if self
            .project_read
            .as_ref()
            .is_some_and(|(current, _)| *current == path)
            && self.desired_project.is_none()
        {
            return Ok(());
        }
        self.desired_project = Some(path);
        if let Some((current, receipt)) = &self.project_read {
            if self.desired_project.as_ref() != Some(current) {
                receipt.cancel();
            }
        }
        self.ready.notify_one();
        Ok(())
    }
    pub fn poll_project(
        &mut self,
    ) -> Option<(
        PathBuf,
        Result<crate::background_animation::AnimationSettings, String>,
    )> {
        if let Some((path, receipt)) = &mut self.project_read {
            let mut result = None;
            match receipt.try_take() {
                JobPoll::Pending => return None,
                JobPoll::Ready(outcome) => {
                    let _held = outcome.map(|outcome| {
                        result = Some(match outcome {
                            JobOutcome::Finished(result) => result,
                            _ => Err("Project read did not complete".into()),
                        })
                    });
                }
                JobPoll::Lost | JobPoll::Taken => {
                    result = Some(Err("Project read receipt lost".into()))
                }
            }
            let path = path.clone();
            self.project_read = None;
            if self.desired_project.is_none() || self.desired_project.as_ref() == Some(&path) {
                self.desired_project = None;
                return result.map(|result| (path, result));
            }
        }
        let path = self.desired_project.as_ref()?.clone();
        let job = ProjectRead { path: path.clone() };
        match self
            .client
            .try_submit(Lane::Io, ConfigurationWrite::COST, job)
        {
            Ok(receipt) => {
                self.desired_project = None;
                self.project_read = Some((path, receipt));
            }
            Err(rejected)
                if matches!(
                    rejected.reason,
                    ilium_execution::RejectReason::Busy
                        | ilium_execution::RejectReason::QueueFull
                        | ilium_execution::RejectReason::JobLimit
                        | ilium_execution::RejectReason::InputBytes
                        | ilium_execution::RejectReason::ResultBytes
                ) => {}
            Err(rejected) => {
                self.desired_project = None;
                return Some((
                    path,
                    Err(format!("Project load refused: {:?}", rejected.reason)),
                ));
            }
        }
        None
    }
    pub(crate) fn enqueue(
        &mut self,
        directory: PathBuf,
        change: ConfigurationChange,
        intent: ConfigurationIntent,
    ) -> Result<(), String> {
        if directory.capacity() > 64 * 1024 {
            return Err("Configuration path exceeds byte limit".into());
        }
        if let ConfigurationIntent::Animation {
            path,
            picker: Some(save),
            ..
        } = &intent
        {
            if path.capacity() > 64 * 1024
                || save.project_path.capacity() > 64 * 1024
                || save.candidate.label.capacity() > 8192
            {
                return Err("Animation picker receipt exceeds retained limit".into());
            }
        }
        let change = change.normalize()?;
        change.checked_bytes()?;
        let id = self
            .writer
            .enqueue_detailed(
                ConfigurationWrite::COST,
                ConfigurationWrite { directory, change },
            )
            .map_err(|rejected| {
                format!(
                    "Configuration write admission: {:?}; change is unsaved",
                    rejected.failure
                )
            })?;
        self.intents.push((id, intent));
        Ok(())
    }
    pub(crate) fn poll(
        &mut self,
    ) -> Option<(
        Option<ConfigurationIntent>,
        WriteCompletion<ConfigurationWrite>,
    )> {
        let completion = self.writer.poll()?;
        let id = match &completion {
            WriteCompletion::Outcome { id, .. }
            | WriteCompletion::Rejected { id, .. }
            | WriteCompletion::Lost { id } => *id,
        };
        let intent = self
            .intents
            .iter()
            .position(|(pending, _)| *pending == id)
            .map(|index| self.intents.remove(index).1);
        Some((intent, completion))
    }
}
