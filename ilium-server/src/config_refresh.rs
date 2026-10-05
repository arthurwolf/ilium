//! A bounded native read supplies both fingerprint and validated settings.
use crate::config::NotificationsConfig;
use crate::error::{ConfigLoadError, ServerError};
use ilium_execution::{Job, JobContext};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;

pub(crate) struct RefreshJob {
    pub(crate) path: PathBuf,
    pub(crate) observed_fingerprint: Option<u64>,
}
pub(crate) struct Refresh {
    pub(crate) fingerprint: u64,
    pub(crate) settings: ilium_sound::SoundSettings,
    pub(crate) notifications: NotificationsConfig,
    pub(crate) backups_enabled: bool,
}
impl Job for RefreshJob {
    type Output = Option<Refresh>;
    type Error = ServerError;
    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        if context.stop_requested() {
            return Ok(None);
        }
        let contents = match crate::config::read_contents(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(ServerError::ConfigLoad {
                    path: self.path,
                    source: Box::new(ConfigLoadError::Read(source)),
                })
            }
        };
        let mut hasher = DefaultHasher::new();
        contents.as_bytes().hash(&mut hasher);
        let fingerprint = hasher.finish();
        if self.observed_fingerprint == Some(fingerprint) || context.stop_requested() {
            return Ok(None);
        }
        let config = crate::config::parse_contents(&self.path, &contents)?;
        // Unused tables and signatures retire on this native worker.
        Ok(Some(Refresh {
            fingerprint,
            settings: config.sound,
            notifications: config.notifications,
            backups_enabled: config.session_backups_enabled,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::{JobCost, Lane};

    #[tokio::test]
    async fn read_fingerprint_and_settings_describe_the_same_input() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let bytes =
            "[sound.events]\napproval_required = true\n[session]\nbackups_enabled = false\n";
        std::fs::write(&path, bytes).unwrap();
        let execution = crate::execution::ServerExecution::start().unwrap();
        let prepared = execution
            .client
            .run(
                Lane::Io,
                JobCost {
                    input_bytes: 64 * 1024 * 1024,
                    result_bytes: 4 * 1024 * 1024,
                },
                RefreshJob {
                    path: path.clone(),
                    observed_fingerprint: None,
                },
            )
            .await
            .unwrap();
        let refresh = prepared.view().as_ref().unwrap();
        let mut hasher = DefaultHasher::new();
        bytes.as_bytes().hash(&mut hasher);
        assert_eq!(refresh.fingerprint, hasher.finish());
        assert!(refresh.settings.events.approval_required);
        assert!(!refresh.backups_enabled);
        let fingerprint = refresh.fingerprint;
        drop(prepared);
        let unchanged = execution
            .client
            .run(
                Lane::Io,
                JobCost {
                    input_bytes: 64 * 1024 * 1024,
                    result_bytes: 4 * 1024 * 1024,
                },
                RefreshJob {
                    path,
                    observed_fingerprint: Some(fingerprint),
                },
            )
            .await
            .unwrap();
        assert!(unchanged.view().is_none());
    }

    #[tokio::test]
    async fn missing_invalid_and_oversized_input_do_not_publish_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let execution = crate::execution::ServerExecution::start().unwrap();
        let cost = JobCost {
            input_bytes: 64 * 1024 * 1024,
            result_bytes: 4 * 1024 * 1024,
        };
        let missing = execution
            .client
            .run(
                Lane::Io,
                cost,
                RefreshJob {
                    path: path.clone(),
                    observed_fingerprint: Some(42),
                },
            )
            .await
            .unwrap();
        assert!(missing.view().is_none());
        drop(missing);
        std::fs::write(&path, "[sound\n").unwrap();
        let invalid = execution
            .client
            .run(
                Lane::Io,
                cost,
                RefreshJob {
                    path: path.clone(),
                    observed_fingerprint: Some(42),
                },
            )
            .await;
        assert!(matches!(
            invalid,
            Err(crate::execution::ExecutionError::Failed(_))
        ));
        drop(invalid);
        let bytes = format!("#{}", "x".repeat(crate::config::MAX_CONFIG_BYTES));
        std::fs::write(&path, bytes).unwrap();
        let overflow = execution
            .client
            .run(
                Lane::Io,
                cost,
                RefreshJob {
                    path,
                    observed_fingerprint: Some(42),
                },
            )
            .await;
        assert!(matches!(
            overflow,
            Err(crate::execution::ExecutionError::Failed(_))
        ));
    }
}
