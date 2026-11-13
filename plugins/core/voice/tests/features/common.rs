//! Shared helpers: hosts for both sides, event builders, readers for the page and for JSONL output.

use std::path::PathBuf;

use ilium_plugin_core_voice::{VoiceClient, VoiceServer};
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    ActionKind, ClientId, ClientRequestEvent, CommandContext, CommandInvokedEvent, CommandOrigin,
    Event, GrabId, Intent, KernelSettingsEvent, KeyEvent, Submission, UiAction, ViewTarget,
};
use serde_json::{json, Value};

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/voice).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The user guide of voice control (src/docs/voice.md), the source of the existing behaviour.
pub fn user_guide() -> String {
    let path = workspace_root().join("src/docs/voice.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("guide {path:?}: {error}"))
}

/// A server-side host with the manifest grants and the plugin activated.
pub fn server_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build();
    host.activate::<VoiceServer>();
    host
}

/// A client-side host with the manifest grants and the plugin activated; voice control is still off.
pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    host.activate::<VoiceClient>();
    host
}

/// Writes one key of the `voice-control` kernel tab and tells the plugin it changed.
pub fn configure(host: &mut FakeHost, key: &str, value: Value) {
    host.kernel_settings()
        .set("voice-control", key, value)
        .expect("kernel setting accepted");
    host.deliver_one(Event::KernelSettingsChanged(KernelSettingsEvent {
        tab: "voice-control".to_string(),
        keys: vec![key.to_string()],
    }));
    host.settle();
}

/// A client host with voice control on.
pub fn enabled_client(input_mode: &str) -> FakeHost {
    let mut host = client_host();
    configure(&mut host, "input_mode", json!(input_mode));
    configure(&mut host, "enabled", json!(true));
    host
}

/// Id of the first F8 grab the plugin holds.
pub fn f8_grab(host: &mut FakeHost) -> u64 {
    host.keys()
        .grabs()
        .first()
        .expect("the plugin holds an F8 grab")
        .id
}

/// `key-pressed` of grab `grab`.
pub fn key_pressed(grab: u64, repeat: bool) -> Event {
    Event::KeyPressed(KeyEvent {
        grab: GrabId(grab),
        pane: None,
        chord: "F8".to_string(),
        repeat,
    })
}

/// `key-released` of grab `grab`.
pub fn key_released(grab: u64) -> Event {
    Event::KeyReleased(KeyEvent {
        grab: GrabId(grab),
        pane: None,
        chord: "F8".to_string(),
        repeat: false,
    })
}

/// The command event the kernel sends when the user runs `command`.
pub fn command_invoked(command: &str) -> Event {
    Event::CommandInvoked(CommandInvokedEvent {
        command: command.to_string(),
        arguments_json: "{}".to_string(),
        origin: CommandOrigin::Keybinding,
        context: CommandContext {
            pane: None,
            view: None,
            buffer: None,
            line: None,
            virtual_row: None,
            menu_item: None,
        },
    })
}

/// `client-request` of kind `kind` with a JSON payload.
pub fn client_request(call: u64, kind: &str, payload: Value) -> Event {
    Event::ClientRequest(ClientRequestEvent {
        call,
        kind: kind.to_string(),
        payload_json: payload.to_string(),
    })
}

/// A button of the popup `popup` activated by the user.
pub fn popup_button(popup: &str, button: &str) -> Event {
    Event::UiAction(UiAction {
        view: ViewTarget::Popup(popup.to_string()),
        key: popup.to_string(),
        action_id: button.to_string(),
        kind: ActionKind::Activated,
        value: None,
    })
}

/// A client attached to the server session.
pub fn client_attached(client: u64) -> Event {
    Event::ClientAttached(ClientId(client))
}

/// Parses JSONL text into one value per non-empty line; every line must be a JSON object.
pub fn jsonl(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: Value =
                serde_json::from_str(line).unwrap_or_else(|error| panic!("line {line:?}: {error}"));
            assert!(value.is_object(), "every JSONL line is an object: {line}");
            value
        })
        .collect()
}

/// The commands of `invoke-command` intents, in submission order.
pub fn invoked_commands(submissions: &[Submission]) -> Vec<String> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::InvokeCommand(spec) => Some(spec.command.clone()),
            _ => None,
        })
        .collect()
}

/// Speaks `text` on an enabled push-to-talk host: press F8, one frame, release, then the transcription job
/// finishes with `text`.
pub fn hear(host: &mut FakeHost, text: &str) {
    let grab = f8_grab(host);
    host.deliver_one(key_pressed(grab, false));
    host.settle();
    let capture = host.audio().captures().last().expect("capture opened").id;
    host.deliver_one(Event::AudioFrames(
        ilium_plugin_sdk::types::AudioFramesEvent {
            capture,
            samples: vec![100, -100, 200],
        },
    ));
    host.deliver_one(key_released(grab));
    host.settle();
    let request = host.jobs().list().last().expect("transcribe job").request;
    host.jobs().finish(request, Ok(text.as_bytes().to_vec()));
    host.settle();
}

/// Adds a terminal pane called `name` to the fake tree.
pub fn add_pane(host: &mut FakeHost, name: &str) -> u64 {
    host.tree().add(ilium_plugin_sdk::testing::TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: ilium_plugin_sdk::types::NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    })
}

/// Contributes a spoken phrase to the `voice-intents` collect point.
pub fn contribute_phrase(
    host: &mut FakeHost,
    id: &str,
    phrase: &str,
    command: &str,
    confirm: bool,
) {
    host.contribute_extension(
        "ilium.core.voice/voice-intents",
        id,
        50,
        json!({ "phrases": [phrase], "command": command, "confirm": confirm }).to_string(),
    );
}

/// The popup key of the confirmation dialog.
pub const CONFIRM_POPUP: &str = "confirm-voice";
