//! Reads durable Text Trigger configuration without coupling it to unrelated
//! settings schemas. Runtime acceptance retains the last valid list.

use std::future::Future;
use std::path::{Path, PathBuf};

use ilium_ipc::{ServerEvent, TextTriggerSettings};

use crate::state::ServerState;

fn read_candidate(path: &Path) -> Result<Option<TextTriggerSettings>, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read durable configuration: {error}"))?;
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
        tokio::task::spawn_blocking(move || read_candidate(&path))
            .await
            .map_err(|error| format!("Text Trigger read task failed: {error}"))?
    })
    .await
}

async fn refresh_with<F, Fut>(state: &ServerState, load: F) -> Result<ServerEvent, String>
where
    F: FnOnce(PathBuf) -> Fut,
    Fut: Future<Output = Result<Option<TextTriggerSettings>, String>>,
{
    // Cover the read too: an older read must not install after a newer result.
    let _transaction = state.text_trigger_settings_transaction.lock().await;
    let path = state
        .text_trigger_config_path
        .get()
        .cloned()
        .ok_or_else(|| "no durable Text Trigger source is configured".to_owned())?;
    let Some(settings) = load(path).await? else {
        return Ok(snapshot(state).await);
    };
    if let Some(message) = crate::text_triggers::validate_settings(&settings) {
        return Err(message);
    }
    let mut current = state.text_trigger_settings.write().await;
    if current.settings == settings {
        return Ok(ServerEvent::TextTriggersChanged {
            settings: current.settings.clone(),
        });
    }
    let revision = current
        .revision
        .checked_add(1)
        .ok_or_else(|| "Text Trigger revision exhausted; retaining accepted rules".to_owned())?;
    current.settings = settings;
    current.revision = revision;
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
        let (sound_requests, sound_task) = crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "durability-test".to_owned(),
            session_cwd: directory.to_path_buf(),
            home_dir: directory.to_path_buf(),
            snapshot_path: directory.join("snapshot.json"),
            socket_path: directory.join("unused.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
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
                Ok(Some(rules("cancelled")))
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
        assert!(refresh_with(&state, |_| async { Ok(Some(rules("new"))) })
            .await
            .is_err());
        assert_eq!(snapshot(&state).await, before);
        assert_eq!(state.text_trigger_settings.read().await.revision, u64::MAX);
        assert!(matches!(
            events.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }
}
