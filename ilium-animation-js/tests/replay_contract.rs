//! Public replay boundary checks. The detailed cache/player algorithms live
//! in replay's private test module; no public test constructs a certificate or
//! a terminal-flush proof from arbitrary fields.
mod common;

use ilium_animation_js::replay::{FrozenInputs, InputFamily, ReplayCache, ReplayLimits};
use ilium_execution::{QuotaGroup, QuotaLimits};

fn root() -> QuotaGroup {
    QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 0,
        worker_bytes: 32 * 1024 * 1024,
    })
}

#[test]
fn public_frozen_input_rejects_duplicate_families_and_invalid_recording_names() {
    let quota = root();
    assert!(FrozenInputs::from_host(
        quota.clone(),
        Some("recording"),
        &[InputFamily::Pointer, InputFamily::Pointer],
        [7; 32],
        None,
        "public input fixture",
        &[],
    )
    .is_err());
    assert!(FrozenInputs::from_host(
        quota.clone(),
        Some("bad\nname"),
        &[InputFamily::Pointer],
        [7; 32],
        None,
        "public input fixture",
        &[],
    )
    .is_err());
    let frozen = FrozenInputs::from_host(
        quota.clone(),
        Some("recording"),
        &[InputFamily::Pointer],
        [7; 32],
        None,
        "public input fixture",
        &[],
    )
    .unwrap();
    assert_eq!(frozen.capture_label(), "public input fixture");
    drop(frozen);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn public_replay_cache_retains_original_frame_ceiling() {
    let quota = root();
    let mut limits = ReplayLimits {
        max_frames: 1,
        ..ReplayLimits::default()
    };
    assert!(ReplayCache::new(quota.clone(), limits).is_ok());
    limits.max_frames = 0;
    assert!(ReplayCache::new(quota.clone(), limits).is_err());
    limits.max_frames = 14_401;
    assert!(ReplayCache::new(quota.clone(), limits).is_err());
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn actual_prepared_helper_issues_opaque_source_free_replay_certificate() {
    use ilium_animation_js::{
        engine::CreateState,
        helper::HelperLimits,
        manifest::AnimationMode,
        package::{Package, PackageLimits},
        permissions::Ceiling,
        replay::{ClipSpec, ClipSpecification, FrozenEvidence},
        runtime::{InstancePreparation, PackageInstance},
        surface::{ColourSpace, Format, Mode, Shape, Update},
        trust::TrustVerifier,
        TRUSTED_BOOTSTRAP,
    };
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::{
        collections::BTreeMap,
        io::{Cursor, Write},
    };

    let executable = common::helper_path();
    let source = b"export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{},replay:{seed:3,duration_seconds:1,seamless:false}}} export async function create(){return {render(){},dispose(){}}}";
    let manifest = json!({"api_version":1,"id":"replay-native-contract","name":"Replay native contract","version":"1.0.0","entry":"entry.mjs","modes":["pre_rendered"],"settings":{"type":"object","properties":{}},"files":[{"path":"entry.mjs","bytes":source.len(),"sha256":format!("{:x}",Sha256::digest(source))}]});
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive.start_file("entry.mjs", options).unwrap();
    archive.write_all(source).unwrap();
    archive.start_file("manifest.json", options).unwrap();
    archive
        .write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    let package = Package::from_bytes(&bytes, PackageLimits::default()).unwrap();
    let verifier = TrustVerifier::from_release_inventory(vec![]).unwrap();
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 32,
        worker_bytes: 1024 * 1024 * 1024,
    });
    let settings = json!({});
    let environment = json!({});
    let verified = PackageInstance::verify(InstancePreparation {
        archive: &bytes,
        verifier: &verifier,
        helper_executable: &executable,
        trusted_bootstrap: TRUSTED_BOOTSTRAP,
        settings: &settings,
        mode: AnimationMode::PreRendered,
        environment: &environment,
        host_policy: Ceiling {
            permissions: vec![],
        },
        instance_id: 73,
        limits: HelperLimits::default(),
        quota: quota.clone(),
    })
    .unwrap();
    let (mut instance, review) = verified.prepare_without_rights().unwrap();
    let pending = instance.begin_resolution(review, BTreeMap::new()).unwrap();
    let resolution = instance.finish_resolution(pending).unwrap();
    assert_eq!(
        resolution.accepted_creation(),
        Some(CreateState::Ready),
        "creation={:?}; creation_error={:?}; authority_error={:?}; teardown_error={:?}; denied_required={:?}; activation_invalidated={}",
        resolution.creation,
        resolution.creation_error,
        resolution.authority_error,
        resolution.teardown_error,
        resolution.denied_required,
        resolution.activation_invalidation.is_some(),
    );
    let certification = instance.certify_procedural_replay().unwrap();
    let frozen = FrozenInputs::from_host(
        quota.clone(),
        None,
        &[],
        [5; 32],
        None,
        "native source-free fixture",
        &[],
    )
    .unwrap();
    let evidence =
        FrozenEvidence::from_native(quota.clone(), &verifier.verify(&package), &[]).unwrap();
    let accepted = ClipSpec::from_accepted(
        &quota,
        ClipSpecification {
            package: &package,
            verifier: &verifier,
            plan: instance.plan(),
            settings: &settings,
            shape: Shape {
                cell_width: 1,
                cell_height: 1,
                mode: Mode::Cells,
                format: Format::Mask8,
                update: Update::Replace,
                cell_rgb: false,
                colour_space: ColourSpace::Srgb,
            },
            backend: "native-test",
            api_version: package.manifest().api_version,
            appearance_digest: [6; 32],
            certification,
            frozen,
            source_sequence: None,
            evidence,
        },
    )
    .unwrap();
    assert_eq!(accepted.frame_count(), 2);
    instance.retire_helper().unwrap();
    assert!(instance.is_physically_retired());
    assert!(instance.certify_procedural_replay().is_err());
}
