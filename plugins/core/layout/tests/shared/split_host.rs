//! Host helpers of the Layout core plugin tests, mounted by both the feature targets and the port targets.
//! Only items that every mounting target uses live here; the rest stays in each target's own `common`.

use std::path::PathBuf;

use ilium_plugin_core_layout::LayoutServer;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    ClientId, CreateSplitSpec, Event, Intent, MessageEvent, PluginId, PluginSide, Submission,
};

pub const PLUGIN_ID: &str = "ilium.core.layout";

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn manifest_path() -> PathBuf {
    crate_dir().join("manifest.json")
}

pub fn server_host() -> FakeHost {
    FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
}

pub fn activated_server() -> FakeHost {
    let mut host = server_host();
    host.activate::<LayoutServer>();
    host
}

pub fn message_from_client(channel: &str, payload: Vec<u8>) -> Event {
    Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_string()),
        from_side: PluginSide::Client,
        from_client: Some(ClientId(1)),
        channel: channel.to_string(),
        payload,
    })
}

pub fn id_bytes(node: u64) -> Vec<u8> {
    node.to_le_bytes().to_vec()
}

pub fn create_splits(submissions: &[Submission]) -> Vec<CreateSplitSpec> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::CreateSplit(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}
