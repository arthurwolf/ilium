use super::integrations::{FeatureTarget, IntegrationIntent, Maintenance};
use super::ordered::WriteCompletion;
use crate::agent_feature_setup::{AgentFeature, FeatureSetupStatus};
use crate::app::{AgentSetupTargetStatus, App, Mode};
use ilium_execution::JobOutcome;
use std::{path::Path, sync::Arc};

impl App {
    pub(crate) fn integration_snapshot(&self, automatic: bool) -> Result<Maintenance, String> {
        let mut targets = Vec::new();
        let roots = self.agent_setup_project_roots();
        if roots.len() > 63 || roots.iter().map(|path| path.capacity()).sum::<usize>() > 512 * 1024
        {
            return Err("Integration project inventory exceeds bounded target limit; previous statuses retained".into());
        }
        for feature in AgentFeature::ALL {
            for path in self.automatic_global_agent_setup_files(feature) {
                targets.push(FeatureTarget {
                    feature,
                    path,
                    project: None,
                });
            }
            for project in &roots {
                for name in ["CLAUDE.md", "AGENTS.md"] {
                    targets.push(FeatureTarget {
                        feature,
                        path: project.join(name),
                        project: Some(project.clone()),
                    });
                }
            }
        }
        let snapshot = Maintenance {
            targets: targets.into_boxed_slice().into_vec(),
            automatic,
        };
        snapshot.checked()?;
        Ok(snapshot)
    }
    pub(crate) fn queue_integration_maintenance(&mut self, automatic: bool) -> Result<(), String> {
        let snapshot = self.integration_snapshot(automatic)?;
        self.integration_files
            .as_mut()
            .ok_or_else(|| "Integration workers unavailable".to_owned())?
            .want(snapshot)
    }
    pub(crate) fn enqueue_integration(&mut self, intent: IntegrationIntent) -> Result<(), String> {
        let result = self
            .integration_files
            .as_mut()
            .ok_or_else(|| "Integration writer unavailable".to_owned())?
            .enqueue(intent);
        self.status_message = Some(match &result {
            Ok(()) => "Integration update pending…".into(),
            Err(error) => error.clone(),
        });
        result
    }
    pub(crate) fn collect_integration_files(&mut self) -> bool {
        let Some(mut files) = self.integration_files.take() else {
            return false;
        };
        let mut changed = false;
        while let Some((intent, completion)) = files.poll() {
            changed = true;
            let mut result = None;
            let hold = match completion {
                WriteCompletion::Outcome { outcome, .. } => {
                    Some(Arc::new(outcome.map(|outcome| {
                        result = Some(match outcome {
                            JobOutcome::Finished(result) => result,
                            _ => {
                                Err("Integration publication unconfirmed: worker did not finish"
                                    .into())
                            }
                        });
                    })))
                }
                WriteCompletion::Rejected { rejection, .. } => {
                    result = Some(Err(format!(
                        "Integration not completed: {:?}",
                        rejection.reason
                    )));
                    None
                }
                WriteCompletion::Lost { .. } => {
                    result = Some(Err(
                        "Integration receipt lost; publication unconfirmed".into()
                    ));
                    None
                }
            };
            let Some(result) = result else {
                continue;
            };
            match result {
                Err(error) => {
                    self.status_message = Some(format!(
                        "Integration publication unconfirmed; authored input retained: {error}"
                    ));
                }
                Ok(mut result) => {
                    // Transfer publication under a new bounded resident lease
                    // before releasing peak scratch admission. On contention
                    // the original reservation remains intact.
                    let hold = match files.publication_hold() {
                        Ok(retained) => Some(Arc::new(retained)),
                        Err(_) => hold,
                    };
                    if let Ok(snapshot) = self.integration_snapshot(false) {
                        let current_targets = snapshot.targets;
                        self.agent_setup_statuses.retain(|(feature, path), _| {
                            current_targets
                                .iter()
                                .any(|target| target.feature == *feature && target.path == *path)
                        });
                        for (target, status) in result.statuses.drain(..) {
                            if current_targets.contains(&target) {
                                self.agent_setup_statuses.insert(
                                    (target.feature, target.path),
                                    status
                                        .map(AgentSetupTargetStatus::Available)
                                        .unwrap_or_else(AgentSetupTargetStatus::Unavailable),
                                );
                                self.integration_status_hold = hold.clone();
                            }
                        }
                    }
                    if result.failures.is_empty() {
                        match &intent {
                            IntegrationIntent::Install {
                                prompt: Some(prompt),
                                ..
                            } => {
                                if matches!(&self.mode,Mode::AgentSetupPrompt(current) if Arc::ptr_eq(&current.identity,&prompt.identity) && current.scope==prompt.scope && current.chatroom_selected==prompt.chatroom_selected && current.progress_selected==prompt.progress_selected)
                                {
                                    self.mode = Mode::Normal;
                                }
                                self.status_message = Some("Agent setup updated".into());
                            }
                            IntegrationIntent::Install { .. } => {
                                self.status_message =
                                    Some("Managed agent instructions updated".into())
                            }
                            IntegrationIntent::Maintenance(_) => {}
                        }
                    } else {
                        self.status_message = Some(format!(
                            "Integration setup incomplete: {}",
                            result.failures.join("; ")
                        ));
                    }
                    self.reconcile_cached_agent_setup_prompts();
                    self.reconcile_right_panel_target();
                }
            }
        }
        self.integration_files = Some(files);
        changed
    }
    pub(crate) fn integration_target(
        &self,
        feature: AgentFeature,
        path: &Path,
        project: Option<&Path>,
    ) -> FeatureTarget {
        FeatureTarget {
            feature,
            path: path.to_path_buf(),
            project: project.map(Path::to_path_buf),
        }
    }
    pub(crate) fn cached_setup_needs_install(
        &self,
        feature: AgentFeature,
        path: &Path,
    ) -> Option<bool> {
        match self
            .agent_setup_statuses
            .get(&(feature, path.to_path_buf()))
        {
            Some(AgentSetupTargetStatus::Available(status)) => Some(matches!(
                status,
                FeatureSetupStatus::NotInstalled | FeatureSetupStatus::ManagedStale
            )),
            _ => None,
        }
    }
}
