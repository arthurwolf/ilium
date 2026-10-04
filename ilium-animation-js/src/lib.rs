//! Worker-local V8 animation adapter. V8 handles never cross thread boundaries.
//! Host services remain subject to package identity, accepted-plan authority,
//! resource admission and immutable presentation ownership.

pub mod clock;

/// Host-owned facade installed before untrusted package evaluation.
pub const TRUSTED_BOOTSTRAP: &str = include_str!("bootstrap.js");

#[cfg(feature = "v8-runtime")]
pub mod engine;
pub mod error;
#[cfg(feature = "v8-runtime")]
pub mod helper;
#[cfg(feature = "native-network")]
pub mod http;
pub mod manifest;
#[cfg(feature = "native-host")]
pub mod native_audio;
#[cfg(feature = "native-host")]
pub mod native_audio_capture;
#[cfg(all(feature = "native-host", feature = "v8-runtime"))]
pub mod native_compute_host;
#[cfg(feature = "native-host")]
pub mod native_draw;
#[cfg(all(feature = "native-host", feature = "v8-runtime"))]
pub mod native_draw_host;
#[cfg(all(
    feature = "v8-runtime",
    feature = "native-host",
    feature = "native-network"
))]
pub(crate) mod native_http_authority;
#[cfg(all(
    feature = "v8-runtime",
    feature = "native-host",
    feature = "native-network"
))]
pub mod native_http_host;
#[cfg(feature = "native-host")]
pub mod native_math;
#[cfg(all(feature = "native-host", feature = "v8-runtime"))]
pub mod native_math_bridge;
#[cfg(feature = "native-host")]
pub mod native_media;
#[cfg(all(
    feature = "v8-runtime",
    feature = "native-host",
    feature = "native-network"
))]
mod native_source_baseline;
pub mod native_source_host;
pub mod native_storage;
pub mod native_video;
pub mod native_video_decoder;
#[cfg(feature = "native-host")]
pub mod native_worlds;
pub mod network;
pub mod package;
pub mod permission_projection;
pub mod permissions;
pub mod plan;
pub mod plan_authorization;
pub mod release;
pub mod replay;
#[cfg(feature = "v8-runtime")]
pub mod runtime;
pub mod settings;
#[cfg(all(feature = "native-host", feature = "native-network"))]
pub mod sources;
pub mod surface;
pub mod trust;

#[cfg(all(test, feature = "v8-runtime"))]
mod v8_dependency_tests {
    #[test]
    fn pinned_engine_executes_typed_array_math_without_node_or_browser_globals() {
        // Rust's test harness has already created sibling threads. Production
        // initializes a protected platform at bootstrap before isolate workers.
        let (_serial, quota) = crate::engine::boundary_tests::fixture_lock();
        let mut engine = crate::engine::boundary_tests::engine(
            "export async function create(){return {render(){},dispose(){}}}",
            crate::engine::boundary_tests::SIMPLE,
            crate::engine::EngineLimits {
                heap_bytes: 16 * 1024 * 1024,
                ..crate::engine::EngineLimits::default()
            },
            quota,
        );
        let value = engine.evaluate_json(r#"(() => {
            if (typeof process !== 'undefined' || typeof require !== 'undefined' || typeof fetch !== 'undefined') throw Error('unexpected ambient OS APIs');
            const pixels = new Float32Array([0.25, 0.5, 0.75]);
            return pixels.reduce((sum,value) => sum+value,0);
        })()"#).unwrap();
        assert_eq!(value, serde_json::json!(1.5));
    }
}

#[cfg(all(feature = "v8-runtime", feature = "native-host"))]
pub mod native_frame_inputs;
