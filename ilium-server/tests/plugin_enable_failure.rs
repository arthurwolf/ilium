//! A refused plugin state change must not change the persisted choice.
#![cfg(target_os = "linux")]

use std::{borrow::Cow, path::PathBuf, sync::Arc, time::Duration};

use ilium_ipc::{
    plugin::{ControlBody, EnvelopeBody, PluginEnvelope},
    read_frame, write_frame, ClientRequest, ServerEvent,
};
use ilium_plugin_host::{lifecycle::StartMode, runtime::CataloguePlugin, wiring};
use ilium_server::{
    config::{DetectionConfig, HttpApiConfig, NotificationsConfig, SessionRecoveryConfig},
    plugin_host::PluginStart,
    NoopSoundPlayer, ServerOptions, ServerResources,
};
use ilium_transport::SessionEndpoint;

mod common;

#[tokio::test]
async fn refusing_to_disable_a_plugin_that_failed_to_load_does_not_persist_the_change() {
    assert_refusal_preserves_choice(false).await;
}

#[tokio::test]
async fn refusing_to_enable_a_plugin_with_invalid_contributions_does_not_persist_the_change() {
    assert_refusal_preserves_choice(true).await;
}

async fn assert_refusal_preserves_choice(enabling: bool) {
    let root = PathBuf::from("/data/ilium-plugin-state-tests");
    std::fs::create_dir_all(&root).expect("create test root");
    std::fs::write(root.join(".scratch-reaper"), b"").expect("mark test root");
    let directory = tempfile::tempdir_in(&root).expect("isolated test directory");
    let project = directory.path().join("project");
    std::fs::create_dir(&project).expect("create project");
    let project = project.canonicalize().expect("canonical project");
    let socket = directory.path().join("s.sock");
    let data = directory.path().join("data");
    let options = ServerOptions {
        resource_catalog_path: None,
        session_name: "plugin-state".to_owned(),
        socket_path: socket.clone(),
        snapshot_path: project.join("snapshot.json"),
        ready_log_metadata: None,
        session_cwd: project.clone(),
        home_dir: project,
        detection_config: DetectionConfig::default(),
        notifications_config: NotificationsConfig {
            enabled: false,
            ..Default::default()
        },
        sound_settings: Default::default(),
        sound_config_path: None,
        sound_player: Arc::new(NoopSoundPlayer),
        custom_signatures: Vec::new(),
        session_recovery: SessionRecoveryConfig::RestoreAutomatically,
        session_backups_enabled: false,
        agent_debug_menu_enabled: false,
        http_api: HttpApiConfig { port: 0 },
        progress_monitor_enabled: false,
    };
    let id = "test.broken";
    let plugin = CataloguePlugin {
        id: id.to_owned(),
        manifest: if enabling {
            format!(r#"{{"id":"{id}","version":"0.1.0","contributes":[]}}"#)
        } else {
            format!(r#"{{"id":"{id}","version":"0.1.0","contributes":{{"pane_kinds":[{{"id":"panel","title":"Panel"}}]}}}}"#)
        },
        component: (!enabling).then_some(Cow::Borrowed(b"not a WebAssembly component".as_slice())),
    };
    if enabling {
        ilium_plugin_host::registry::set_plugin_enabled(
            &wiring::installed_path(&data),
            id,
            "0.1.0",
            false,
        )
        .expect("save disabled choice");
    }
    let start = PluginStart {
        catalogue: vec![plugin],
        data_dir: data.clone(),
        mode: StartMode::Normal,
    };
    let task = tokio::spawn(ilium_server::run_with_plugins(
        options,
        ServerResources::new(),
        Some(start),
    ));
    assert!(common::wait_until(|| socket.exists(), Duration::from_secs(20)).await);
    let mut incompatible = SessionEndpoint::from_path(&socket)
        .connect().await.expect("connect incompatible client");
    write_frame(&mut incompatible, &ClientRequest::Attach {
        session: "plugin-state".to_owned(),
        hello: ilium_ipc::plugin::PluginHello {
            plugin_protocol: 2,
            ..Default::default()
        },
    }).await.expect("send incompatible hello");
    let refusal: ServerEvent = tokio::time::timeout(Duration::from_secs(10), read_frame(&mut incompatible))
        .await.expect("incompatible hello reply").expect("read refusal");
    assert!(matches!(refusal, ServerEvent::Rejected(rejection) if rejection.code == "ipc.protocol-mismatch"));
    let mut stream = SessionEndpoint::from_path(&socket)
        .connect()
        .await
        .expect("connect");
    write_frame(
        &mut stream,
        &ClientRequest::Attach {
            session: "plugin-state".to_owned(),
            hello: Default::default(),
        },
    )
    .await
    .expect("attach");
    common::read_initial_state(&mut stream, Duration::from_secs(30)).await;
    write_frame(
        &mut stream,
        &ClientRequest::Plugin(PluginEnvelope {
            plugin: String::new(),
            seq: 1,
            body: EnvelopeBody::Control(ControlBody::SetEnabled {
                plugin: id.to_owned(),
                enabled: enabling,
            }),
        }),
    )
    .await
    .expect("send state change");
    let state = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let event: ServerEvent = read_frame(&mut stream).await.expect("read event");
            if let ServerEvent::Plugin(PluginEnvelope {
                body: EnvelopeBody::Control(ControlBody::SetEnabledResult { request_seq, plugin, state }),
                ..
            }) = event
            {
                if plugin == id {
                    assert_eq!(request_seq, 1, "the direct reply must identify its request");
                    break state;
                }
            }
        }
    })
    .await
    .expect("state reply");
    assert_eq!(state, "refused:plugin.state");
    let (disabled, warning) = wiring::disabled_at_start(&data);
    assert!(warning.is_none());
    assert_eq!(
        disabled.contains(id),
        enabling,
        "a refused change must not persist"
    );
    if !enabling {
        write_frame(&mut stream, &ClientRequest::NewPane {
            request_id: 73,
            parent_group: ilium_core::ROOT_ID,
            kind: ilium_ipc::NewPaneKind::Plugin {
                kind_id: format!("{id}.panel"),
                title: String::new(),
            },
            working_directory: ilium_ipc::NewPaneWorkingDirectory::ProjectRoot,
        }).await.expect("request a failed plugin's pane");
        let result = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let event: ServerEvent = read_frame(&mut stream).await.expect("read event");
                if let ServerEvent::PaneCreateFinished { request_id: 73, result } = event {
                    break result;
                }
            }
        }).await.expect("pane creation reply");
        assert!(result.is_err(), "a failed plugin must not create an undrawable pane");
    }
    task.abort();
}
