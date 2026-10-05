//! Reads durable Text Trigger configuration without coupling it to unrelated
//! settings schemas. Runtime acceptance retains the last valid list.

use std::future::Future;
use std::io::Read;
use std::path::{Path, PathBuf};

use ilium_ipc::{ServerEvent, TextTriggerSettings};

use crate::state::ServerState;

fn read_candidate(path: &Path) -> Result<Option<TextTriggerSettings>, String> {
    const READ_LIMIT: u64 = 512 * 1024;
    let file = ilium_platform::secure_fs::open_regular_file(path)
        .map_err(|error| format!("could not open regular durable configuration: {error}"))?;
    let mut contents = String::new();
    file.take(READ_LIMIT + 1)
        .read_to_string(&mut contents)
        .map_err(|error| format!("could not read durable configuration: {error}"))?;
    if contents.len() as u64 > READ_LIMIT {
        return Err("durable Text Trigger configuration exceeds the 512 KiB read limit".to_owned());
    }
    let document: toml::Value = toml::from_str(&contents)
        .map_err(|error| format!("could not parse durable configuration: {error}"))?;
    let Some(value) = document.get("text_triggers") else {
        // An absent table is unspecified; only an explicit empty list clears.
        return Ok(None);
    };
    if value.get("triggers").is_none() {
        return Err("[text_triggers] must contain an explicit triggers list".to_owned());
    }
    value
        .clone()
        .try_into()
        .map(Some)
        .map_err(|error| format!("invalid [text_triggers] table: {error}"))
}

pub(crate) async fn snapshot(state: &ServerState) -> ServerEvent {
    ServerEvent::TextTriggersChanged {
        settings: state.text_trigger_settings.read().await.settings.clone(),
    }
}

pub(crate) async fn refresh(state: &ServerState) -> Result<ServerEvent, String> {
    refresh_with(state, |path| async move {
        // The worker owns only the path. Cancelling the awaiting owner cannot
        // leave a detached worker that installs a late result into state.
        let client = crate::text_triggers::execution_client(state)?;
        let reservation = client
            .reserve(
                ilium_execution::Lane::Io,
                ilium_execution::JobCost {
                    input_bytes: 64 * 1024 * 1024,
                    result_bytes: 32 * 1024 * 1024,
                },
            )
            .await
            .map_err(|error| format!("Text Trigger read admission: {error:?}"))?;
        let loaded = client
            .run_reserved(reservation, move |_context| read_candidate(&path))
            .await
            .map_err(|error| format!("Text Trigger read worker failed: {error}"))?;
        let bytes = loaded
            .view()
            .as_ref()
            .map(crate::text_triggers::settings_bytes)
            .unwrap_or(Some(256))
            .filter(|bytes| *bytes <= 16 * 1024 * 1024)
            .ok_or_else(|| {
                "Text Trigger settings exceed the 16 MiB retained allocation limit".to_owned()
            })?;
        let storage = client
            .reserve_storage(bytes.max(256))
            .await
            .map_err(|error| format!("Text Trigger loaded settings admission: {error:?}"))?;
        let (settings, peak_charge) = loaded.into_parts();
        // The actual allocation now has its own storage lease. Releasing the
        // IO result envelope before CPU admission avoids a phase-transition
        // deadlock when several completed reads fill all result credits.
        drop(peak_charge);
        Ok(LoadedCandidate {
            settings,
            storage: Some(storage),
        })
    })
    .await
}

struct LoadedCandidate {
    settings: Option<TextTriggerSettings>,
    storage: Option<std::sync::Arc<ilium_execution::StorageAdmission>>,
}
impl From<Option<TextTriggerSettings>> for LoadedCandidate {
    fn from(settings: Option<TextTriggerSettings>) -> Self {
        Self {
            settings,
            storage: None,
        }
    }
}

async fn refresh_with<F, Fut>(state: &ServerState, load: F) -> Result<ServerEvent, String>
where
    F: FnOnce(PathBuf) -> Fut,
    Fut: Future<Output = Result<LoadedCandidate, String>>,
{
    // Cover the read too: an older read must not install after a newer result.
    let _transaction = state.text_trigger_settings_transaction.lock().await;
    let path = state
        .text_trigger_config_path
        .get()
        .cloned()
        .ok_or_else(|| "no durable Text Trigger source is configured".to_owned())?;
    let mut loaded = load(path).await?;
    let Some(settings) = loaded.settings.take() else {
        return Ok(snapshot(state).await);
    };
    let validated =
        crate::text_triggers::validate_in_worker(state, settings, loaded.storage.take()).await?;
    let mut current = state.text_trigger_settings.write().await;
    if current.settings == validated.settings {
        return Ok(ServerEvent::TextTriggersChanged {
            settings: current.settings.clone(),
        });
    }
    let revision = current
        .revision
        .checked_add(1)
        .ok_or_else(|| "Text Trigger revision exhausted; retaining accepted rules".to_owned())?;
    let crate::text_triggers::AcceptedCandidate {
        settings,
        retention,
        ..
    } = validated;
    current.settings = settings;
    current.revision = revision;
    current.retention = Some(retention);
    let event = ServerEvent::TextTriggersChanged {
        settings: current.settings.clone(),
    };
    // Publish inside the transaction so acceptance and broadcast order agree.
    state.broadcast(event.clone());
    Ok(event)
}

#[cfg(test)]
mod durability_tests {
    use super::*;
    use ilium_ipc::TextTrigger;
    use std::sync::Arc;
    use tokio::sync::oneshot;
    use tokio::time::{timeout, Duration};
    const WAIT: Duration = Duration::from_secs(5);

    struct Task<T>(tokio::task::JoinHandle<T>);
    impl<T> Drop for Task<T> {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    fn state_at(directory: &Path) -> (Arc<ServerState>, Task<()>) {
        let (sound_requests, sound_task) = crate::sounds::spawn(
            Arc::new(crate::NoopSoundPlayer),
            crate::execution::test_general_client(),
        );
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "durability-test".to_owned(),
            session_cwd: directory.to_path_buf(),
            home_dir: directory.to_path_buf(),
            snapshot_path: directory.join("snapshot.json"),
            socket_path: directory.join("unused.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: false,
        }));
        state
            .text_trigger_config_path
            .set(directory.join("config.toml"))
            .unwrap();
        (state, Task(sound_task))
    }
    fn rules(message: &str) -> TextTriggerSettings {
        TextTriggerSettings {
            triggers: vec![TextTrigger {
                id: "authored-stable-id".to_owned(),
                regexp: "ready$".to_owned(),
                message: message.to_owned(),
                ..TextTrigger::default()
            }],
        }
    }
    fn document(settings: &TextTriggerSettings) -> String {
        let mut root = toml::value::Table::new();
        root.insert(
            "text_triggers".to_owned(),
            toml::Value::try_from(settings).unwrap(),
        );
        toml::to_string(&toml::Value::Table(root)).unwrap()
    }

    #[tokio::test]
    async fn regex_validation_uses_a_real_cpu_worker_and_acceptance_keeps_its_storage() {
        let directory = tempfile::tempdir().unwrap();
        let (state, _sound) = state_at(directory.path());
        assert!(state
            .execution
            .set(crate::execution::ServerExecution::start().unwrap())
            .is_ok());
        let original = rules("literal accepted message");
        let candidate = crate::text_triggers::validate_in_worker(&state, original.clone(), None)
            .await
            .unwrap();
        assert_ne!(candidate.validation_thread, std::thread::current().id());
        assert_eq!(candidate.settings, original);
        assert!(std::sync::Arc::strong_count(&candidate.retention) >= 1);
        std::fs::write(directory.path().join("config.toml"), document(&original)).unwrap();
        refresh(&state).await.unwrap();
        assert!(state.text_trigger_settings.read().await.retention.is_some());
        let mut invalid = rules("bad");
        invalid.triggers[0].regexp = "[".to_owned();
        std::fs::write(directory.path().join("config.toml"), document(&invalid)).unwrap();
        assert!(refresh(&state).await.is_err());
        assert_eq!(state.text_trigger_settings.read().await.settings, original);
        assert_eq!(state.text_trigger_settings.read().await.revision, 1);
    }

    #[tokio::test]
    async fn oversized_durable_read_retains_the_last_accepted_rules_and_revision() {
        let directory = tempfile::tempdir().unwrap();
        let (state, _sound) = state_at(directory.path());
        let path = directory.path().join("config.toml");
        let original = rules("retained after overload");
        std::fs::write(&path, document(&original)).unwrap();
        refresh(&state).await.unwrap();
        let before = snapshot(&state).await;
        std::fs::write(&path, vec![b' '; 512 * 1024 + 1]).unwrap();
        let error = refresh(&state).await.unwrap_err();
        assert!(error.contains("512 KiB"), "{error}");
        assert_eq!(snapshot(&state).await, before);
        assert_eq!(state.text_trigger_settings.read().await.revision, 1);
    }

    #[tokio::test]
    async fn text_trigger_invalid_partial_and_absent_sources_retain_last_valid_without_rewriting() {
        let directory = tempfile::tempdir().unwrap();
        let (state, _sound) = state_at(directory.path());
        let path = directory.path().join("config.toml");
        let original = rules("retained");
        std::fs::write(&path, document(&original)).unwrap();
        let accepted = refresh(&state).await.unwrap();
        let mut bad_regex = original.clone();
        bad_regex.triggers[0].regexp = "[".to_owned();
        let mut bad_id = original.clone();
        bad_id.triggers[0].id.clear();
        let mut duplicates = original.clone();
        duplicates.triggers.push(original.triggers[0].clone());
        let mut multiline = original.clone();
        multiline.triggers[0].message = "one\ntwo".to_owned();
        for contents in [
            "not valid [ toml".to_owned(),
            "[text_triggers]".to_owned(),
            "[text_triggers]\ntriggers = 12".to_owned(),
            document(&bad_regex),
            document(&bad_id),
            document(&duplicates),
            document(&multiline),
        ] {
            std::fs::write(&path, &contents).unwrap();
            assert!(refresh(&state).await.is_err());
            assert_eq!(snapshot(&state).await, accepted);
            assert_eq!(state.text_trigger_settings.read().await.revision, 1);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
        }
        std::fs::remove_file(&path).unwrap();
        assert!(refresh(&state).await.is_err());
        assert_eq!(snapshot(&state).await, accepted);
        std::fs::write(&path, "[sound]\nsource = 'system_beep'").unwrap();
        assert_eq!(refresh(&state).await.unwrap(), accepted);
        std::fs::write(
            &path,
            format!(
                "{}\n[detection]\nworking_poll_seconds = 'bad'",
                document(&original)
            ),
        )
        .unwrap();
        assert_eq!(refresh(&state).await.unwrap(), accepted);
        assert_eq!(state.text_trigger_settings.read().await.revision, 1);
    }

    #[tokio::test]
    async fn text_trigger_explicit_empty_survives_fresh_state_and_noop_preserves_revision() {
        let directory = tempfile::tempdir().unwrap();
        let (state, _sound) = state_at(directory.path());
        let path = directory.path().join("config.toml");
        std::fs::write(&path, document(&rules("old"))).unwrap();
        refresh(&state).await.unwrap();
        std::fs::write(&path, document(&TextTriggerSettings::default())).unwrap();
        let empty = refresh(&state).await.unwrap();
        assert_eq!(state.text_trigger_settings.read().await.revision, 2);
        assert_eq!(refresh(&state).await.unwrap(), empty);
        assert_eq!(state.text_trigger_settings.read().await.revision, 2);
        let (fresh, _fresh_sound) = state_at(directory.path());
        assert_eq!(refresh(&fresh).await.unwrap(), empty);
        assert_eq!(fresh.text_trigger_settings.read().await.revision, 0);
    }

    #[tokio::test]
    async fn text_trigger_cancelled_reload_cannot_install_and_releases_owner() {
        let directory = tempfile::tempdir().unwrap();
        let (state, _sound) = state_at(directory.path());
        let (entered_tx, entered_rx) = oneshot::channel();
        let (_release_tx, release_rx) = oneshot::channel::<()>();
        let owned = Arc::clone(&state);
        let mut task = Task(tokio::spawn(async move {
            refresh_with(&owned, |_| async move {
                entered_tx.send(()).unwrap();
                let _ = release_rx.await;
                Ok(Some(rules("cancelled")).into())
            })
            .await
        }));
        timeout(WAIT, entered_rx).await.unwrap().unwrap();
        assert!(state.text_trigger_settings_transaction.try_lock().is_err());
        task.0.abort();
        assert!(timeout(WAIT, &mut task.0)
            .await
            .unwrap()
            .unwrap_err()
            .is_cancelled());
        assert!(state.text_trigger_settings_transaction.try_lock().is_ok());
        assert_eq!(
            state.text_trigger_settings.read().await.settings,
            TextTriggerSettings::default()
        );
    }

    #[tokio::test]
    async fn text_trigger_revision_exhaustion_refuses_change_without_broadcast() {
        let directory = tempfile::tempdir().unwrap();
        let (state, _sound) = state_at(directory.path());
        state.text_trigger_settings.write().await.revision = u64::MAX;
        let before = snapshot(&state).await;
        let mut events = state.events.subscribe();
        assert!(
            refresh_with(&state, |_| async { Ok(Some(rules("new")).into()) })
                .await
                .is_err()
        );
        assert_eq!(snapshot(&state).await, before);
        assert_eq!(state.text_trigger_settings.read().await.revision, u64::MAX);
        assert!(matches!(
            events.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }
}

#[cfg(test)]
mod legacy_delay_tests {
    use super::read_candidate;

    #[test]
    fn a_rule_stored_before_the_delay_setting_loads_with_sixty_seconds() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        std::fs::write(
            &path,
            "[text_triggers]\n[[text_triggers.triggers]]\nid = \"old\"\nenabled = true\nregexp = \"ready$\"\nmessage = \"go\"\ntarget = \"both\"\nsample_text = \"\"\n\n[[text_triggers.triggers]]\nid = \"new\"\nregexp = \"x\"\nmessage = \"y\"\ndelay_seconds = 5\n",
        )
        .unwrap();
        let settings = read_candidate(&path).unwrap().unwrap();
        assert_eq!(settings.triggers[0].delay_seconds, 60);
        assert_eq!(settings.triggers[1].delay_seconds, 5);
    }
}
