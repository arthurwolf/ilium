//! Port twins of the recap tests that exist in the monolith (lane core-services, part ports).
//! Each twin asserts the same observable behaviour through the plugin's channels: the `recap`
//! command output, the job input, the model answer and the host snapshot.

#[path = "../features/common.rs"]
mod common;
#[path = "../host_ext.rs"]
mod host_ext;
mod support;

mod client_pipeline;
mod custody;
mod evidence;
mod fence;
mod infer;
mod model;
mod pointers;
mod recap_isolated;
mod server_input;
