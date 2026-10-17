//! `ilium:host/gpu` (api-gpu.md): scene-world kernels.

use ilium_plugin_api::generated::host::gpu::{GpuKernel, HostGpu};
use ilium_plugin_api::{ErrorCode, Side};

use crate::common::*;

fn scene_rig() -> Rig {
    rig_on(Side::Scene, &["gpu.kernels"])
}

/// REQ: WIT:ilium:host/gpu.available
#[test]
fn wit_gpu_available_true_when_a_device_exists() {
    let mut rig = scene_rig();
    rig.kernel.set_gpu_available(true);
    assert!(rig.host.gpu().available());
}

/// REQ: WIT:ilium:host/gpu.available
#[test]
fn wit_gpu_available_false_without_a_device() {
    let mut rig = scene_rig();
    rig.kernel.set_gpu_available(false);
    assert!(!rig.host.gpu().available());
}

/// REQ: WIT:ilium:host/gpu.available
#[test]
fn wit_gpu_available_never_fails_without_the_grant() {
    let mut rig = declined_rig(&["gpu.kernels"]);
    assert!(!rig.host.gpu().available());
}

/// REQ: WIT:ilium:host/gpu.run
#[test]
fn wit_gpu_run_threshold_returns_the_output_pixels() {
    let mut rig = scene_rig();
    rig.kernel.set_gpu_available(true);
    let input = vec![0u8, 255, 0, 255];
    let out = rig
        .host
        .gpu()
        .run(
            GpuKernel::Threshold,
            input,
            2,
            2,
            "{\"level\":128}".to_owned(),
        )
        .expect("runs");
    assert_eq!(out.len(), 4);
}

/// REQ: WIT:ilium:host/gpu.run
#[test]
fn wit_gpu_run_zero_width_invalid_argument() {
    let mut rig = scene_rig();
    rig.kernel.set_gpu_available(true);
    assert_eq!(
        code_of(
            rig.host
                .gpu()
                .run(GpuKernel::Blur, vec![0; 4], 0, 2, "{}".to_owned())
        ),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/gpu.run
#[test]
fn wit_gpu_run_input_size_mismatch_invalid_argument() {
    let mut rig = scene_rig();
    rig.kernel.set_gpu_available(true);
    assert_eq!(
        code_of(
            rig.host
                .gpu()
                .run(GpuKernel::Blur, vec![0; 3], 2, 2, "{}".to_owned())
        ),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/gpu.run
#[test]
fn wit_gpu_run_without_a_device_unsupported() {
    let mut rig = scene_rig();
    rig.kernel.set_gpu_available(false);
    assert_eq!(
        code_of(
            rig.host
                .gpu()
                .run(GpuKernel::Scale, vec![0; 4], 2, 2, "{}".to_owned())
        ),
        ErrorCode::Unsupported
    );
}

/// REQ: WIT:ilium:host/gpu.run
#[test]
fn wit_gpu_run_declined_grant_not_permitted() {
    let mut rig = declined_rig(&["gpu.kernels"]);
    assert_eq!(
        code_of(
            rig.host
                .gpu()
                .run(GpuKernel::Scale, vec![0; 4], 2, 2, "{}".to_owned())
        ),
        ErrorCode::NotPermitted
    );
}
