//! Explicit actual-helper tests. The PNG is local synthetic fixture data; all
//! V8 requests, native registries, helper copy/ACK and physical join are real.
use super::*;
use crate::{
    engine::CreateState, helper::HelperLimits, manifest::AnimationMode,
    native_asset_host::NativeAssetHost, native_draw::DrawLimits, native_media::MediaLimits,
    permissions::Ceiling, runtime::InstancePreparation, trust::TrustVerifier, TRUSTED_BOOTSTRAP,
};
use ilium_execution::{
    ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits,
};
use ilium_platform::animation_files::PinnedDirectory;
use ilium_platform::secure_fs::NoFollowDirectory;
use image::{ImageBuffer, ImageFormat, Rgba};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    path::PathBuf,
    sync::Arc,
};

fn quota() -> QuotaGroup {
    QuotaGroup::new(QuotaLimits {
        clients: 8,
        jobs: 16,
        service_jobs: 0,
        input_bytes: 32 * 1024 * 1024,
        result_bytes: 32 * 1024 * 1024,
        worker_threads: 32,
        worker_bytes: 1024 * 1024 * 1024,
    })
}
fn tiny_png() -> Vec<u8> {
    let image = ImageBuffer::from_fn(1, 1, |_, _| Rgba([255_u8, 0, 0, 255]));
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).unwrap();
    output.into_inner()
}
fn archive(script: &str) -> Vec<u8> {
    let manifest = json!({
        "api_version":1,"id":"native-image-qualification","name":"Native image qualification",
        "version":"1.0.0","entry":"entry.mjs","modes":["live"],
        "settings":{"type":"object","properties":{}},
        "files":[{"path":"entry.mjs","bytes":script.len(),
            "sha256":format!("{:x}",Sha256::digest(script.as_bytes()))}]
    });
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("entry.mjs", options).unwrap();
    zip.write_all(script.as_bytes()).unwrap();
    zip.start_file("manifest.json", options).unwrap();
    zip.write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    zip.finish().unwrap().into_inner()
}
fn archive_with_png_asset(script: &str, png: &[u8]) -> Vec<u8> {
    let entry = json!({"path":"entry.mjs","bytes":script.len(),
        "sha256":format!("{:x}",Sha256::digest(script.as_bytes()))});
    let asset = json!({"path":"assets/pixel.png","bytes":png.len(),
        "sha256":format!("{:x}",Sha256::digest(png))});
    let manifest = json!({
        "api_version":1,"id":"native-image-asset-qualification",
        "name":"Native image asset qualification","version":"1.0.0",
        "entry":"entry.mjs","modes":["live"],
        "settings":{"type":"object","properties":{}},
        "assets":[asset],"files":[entry,asset]
    });
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("entry.mjs", options).unwrap();
    zip.write_all(script.as_bytes()).unwrap();
    zip.start_file("assets/pixel.png", options).unwrap();
    zip.write_all(png).unwrap();
    zip.start_file("manifest.json", options).unwrap();
    zip.write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    zip.finish().unwrap().into_inner()
}
fn instance(script: &str, quota: QuotaGroup, mut limits: HelperLimits) -> PackageInstance {
    let helper = PathBuf::from(
        std::env::var_os("ILIUM_ANIMATION_HELPER")
            .expect("explicit qualification requires the matching release helper"),
    );
    assert!(helper.is_absolute());
    let bytes = archive(script);
    let _archive_admission = quota.reserve_external_storage(bytes.len() + 65536).unwrap();
    let verifier = TrustVerifier::from_release_inventory(Vec::new()).unwrap();
    limits.engine.preparation_ms = limits.engine.preparation_ms.max(1000);
    let settings = json!({});
    let environment = json!({"cell_width":1,"cell_height":1,"dot_width":2,"dot_height":4});
    let verified = PackageInstance::verify(InstancePreparation {
        archive: &bytes,
        verifier: &verifier,
        helper_executable: &helper,
        trusted_bootstrap: TRUSTED_BOOTSTRAP,
        settings: &settings,
        mode: AnimationMode::Live,
        environment: &environment,
        host_policy: Ceiling {
            permissions: vec![],
        },
        instance_id: 93,
        limits,
        quota,
    })
    .unwrap();
    let (mut instance, review) = verified.prepare_without_rights().unwrap();
    assert!(review.items().is_empty());
    let pending = instance.begin_resolution(review, BTreeMap::new()).unwrap();
    let result = instance.finish_resolution(pending).unwrap();
    assert!(result.creation_error.is_none());
    assert_eq!(result.creation, Some(CreateState::Pending));
    instance
}
fn script(body: &str) -> String {
    let template = format!(
        "export function plan(){{return {{output:{{mode:'pixels',format:'gray8',update:'replace'}},fps:30,inputs:{{}},permissions:[]}};}} export async function create(host){{const raw=new Uint8Array(__PNG__); {body} return {{render(context,frame){{frame.gray.fill(0);frame.present();}},dispose(){{}}}};}}"
    );
    template.replace("__PNG__", &serde_json::to_string(&tiny_png()).unwrap())
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn actual_helper_reads_packaged_png_decodes_samples_and_retires_native_handles() {
    let quota = quota();
    let png = tiny_png();
    let script = "export function plan(){return {output:{mode:'pixels',format:'gray8',update:'replace'},fps:30,inputs:{},permissions:[]};} export async function create(host){const asset=await host.assets.read({grant:host.assets.bundle,relative_path:'assets/pixel.png',max_bytes:4096}); if(!asset.ok || !(asset.value.bytes instanceof Uint8Array))throw Error('asset_read'); const opened=await host.media.images.decode({bytes:asset.value.bytes,max_pixels:4}); if(!opened.ok)throw Error('image_decode'); const sampled=await host.media.images.sample({image:opened.value,rectangle:{x:0,y:0,width:1,height:1},format:'gray32'}); if(!sampled.ok || Math.abs(sampled.value[0]-0.2126)>0.01)throw Error('image_sample'); host.media.images.close(opened.value); return {render(context,frame){frame.gray.fill(0);frame.present();},dispose(){}};}";
    let bytes = archive_with_png_asset(script, &png);
    let _archive_admission = quota.reserve_external_storage(bytes.len() + 65536).unwrap();
    let verifier = TrustVerifier::from_release_inventory(Vec::new()).unwrap();
    let helper = PathBuf::from(
        std::env::var_os("ILIUM_ANIMATION_HELPER")
            .expect("explicit qualification requires the matching release helper"),
    );
    let mut instance = PackageInstance::verify(InstancePreparation {
        archive: &bytes,
        verifier: &verifier,
        helper_executable: &helper,
        trusted_bootstrap: TRUSTED_BOOTSTRAP,
        settings: &json!({}),
        mode: AnimationMode::Live,
        environment: &json!({"cell_width":1,"cell_height":1,"dot_width":2,"dot_height":4}),
        host_policy: Ceiling {
            permissions: vec![],
        },
        instance_id: 94,
        limits: HelperLimits::default(),
        quota: quota.clone(),
    })
    .unwrap()
    .prepare_without_rights()
    .unwrap()
    .0;
    let disabled = LaneConfig {
        threads: 0,
        queue_slots: 0,
        priority: None,
        resident_bytes_per_thread: 0,
    };
    let execution = Execution::start(
        quota.clone(),
        ExecutionConfig {
            cpu: disabled,
            io: LaneConfig {
                threads: 1,
                queue_slots: 4,
                priority: None,
                resident_bytes_per_thread: 1024 * 1024,
            },
            service: disabled,
        },
    )
    .unwrap();
    let client = execution
        .client(ClientLimits {
            jobs: 8,
            service_jobs: 0,
            input_bytes: 16 * 1024 * 1024,
            result_bytes: 16 * 1024 * 1024,
        })
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let root = Arc::new(
        PinnedDirectory::from_host(Arc::new(
            NoFollowDirectory::open_root(directory.path()).unwrap(),
        ))
        .unwrap(),
    );
    let mut assets = NativeAssetHost::new(&instance, client, quota.clone(), root).unwrap();
    let mut drawing =
        NativeDrawHost::new(quota.clone(), MediaLimits::default(), DrawLimits::default()).unwrap();
    let mut images = NativeImageHost::new(quota.clone()).unwrap();
    let mut methods = Vec::new();
    let mut creation = CreateState::Pending;
    for _ in 0..12 {
        for request in instance.requests().unwrap() {
            methods.push(request.method.clone());
            match request.method.as_str() {
                "assets.read" => {
                    assert!(assets.dispatch(&mut instance, request).unwrap().is_none())
                }
                _ => assert!(images
                    .dispatch(&mut instance, &mut drawing, request)
                    .unwrap()
                    .is_none()),
            }
        }
        creation = instance.pump().unwrap();
        if creation == CreateState::Ready {
            break;
        }
    }
    assert_eq!(creation, CreateState::Ready);
    assert_eq!(
        methods.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "assets.read",
            "media.images.decode",
            "media.images.sample",
            "media.images.close"
        ]
    );
    assert!(images.pending.is_empty() && images.owned.is_empty());
    assert!(instance.stop().cancellation.is_ok());
    assert!(instance.is_physically_retired());
    assets.revoke();
    images.revoke();
    assets.release_terminal_after_helper_retirement();
    images.release_terminal_after_helper_retirement(&mut drawing);
    assert!(assets.is_drained() && images.is_drained());
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn actual_helper_generic_image_decode_resize_float_sample_close_ack_and_retirement() {
    let quota = quota();
    let script = script(
        "const opened=await host.media.images.decode({bytes:raw,max_pixels:4}); if(!opened.ok)throw Error('decode_ack'); const resized=await host.media.images.resize({image:opened.value,width:1,height:1,filter:'nearest'}); if(!resized.ok)throw Error('resize_ack'); const sampled=await host.media.images.sample({image:resized.value,rectangle:{x:0,y:0,width:1,height:1},format:'gray32'}); if(!sampled.ok || !(sampled.value instanceof Float32Array) || Math.abs(sampled.value[0]-0.2126)>0.01)throw Error('float_sample_ack'); host.media.images.close(opened.value); host.media.images.close(resized.value);",
    );
    let mut instance = instance(&script, quota.clone(), HelperLimits::default());
    let mut drawing =
        NativeDrawHost::new(quota.clone(), MediaLimits::default(), DrawLimits::default()).unwrap();
    let mut images = NativeImageHost::new(quota).unwrap();
    let mut methods = Vec::new();
    let mut creation = CreateState::Pending;
    for _ in 0..12 {
        for request in instance.requests().unwrap() {
            methods.push(request.method.clone());
            assert!(images
                .dispatch(&mut instance, &mut drawing, request)
                .unwrap()
                .is_none());
        }
        creation = instance.pump().unwrap();
        if creation == CreateState::Ready && methods.len() == 5 {
            break;
        }
    }
    assert_eq!(creation, CreateState::Ready);
    assert_eq!(
        methods.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "media.images.decode",
            "media.images.resize",
            "media.images.sample",
            "media.images.close",
            "media.images.close",
        ]
    );
    assert!(images.pending.is_empty());
    assert!(
        images.owned.is_empty(),
        "close only retires after helper ACK"
    );
    let stopped = instance.stop();
    assert!(stopped.cancellation.is_ok());
    assert!(instance.is_physically_retired());
    images.revoke();
    images.release_terminal_after_helper_retirement(&mut drawing);
    assert!(images.is_drained());
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn actual_helper_cancelled_generic_decode_never_creates_a_native_handle() {
    let quota = quota();
    let script = script("await host.media.images.decode({bytes:raw,max_pixels:4});");
    let mut instance = instance(&script, quota.clone(), HelperLimits::default());
    let mut drawing =
        NativeDrawHost::new(quota.clone(), MediaLimits::default(), DrawLimits::default()).unwrap();
    let mut images = NativeImageHost::new(quota).unwrap();
    let requests = instance.requests().unwrap();
    assert_eq!(requests.len(), 1);
    let request = requests.into_iter().next().unwrap();
    assert_eq!(request.method, "media.images.decode");
    request.stop_token().stop();
    assert!(images
        .dispatch(&mut instance, &mut drawing, request)
        .is_err());
    assert!(images.pending.is_empty());
    assert!(images.owned.is_empty());
    let stopped = instance.stop();
    assert!(stopped.cancellation.is_ok());
    assert!(instance.is_physically_retired());
    images.revoke();
    images.release_terminal_after_helper_retirement(&mut drawing);
    assert!(images.is_drained());
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn actual_helper_generic_image_copy_refusal_retains_request_result_until_physical_join() {
    let quota = quota();
    let script = script(
        "const opened=await host.media.images.decode({bytes:raw,max_pixels:4}); if(!opened.ok)throw Error('decode_ack'); const resized=await host.media.images.resize({image:opened.value,width:512,height:512,filter:'nearest'}); if(!resized.ok)throw Error('resize_ack'); await host.media.images.sample({image:resized.value,rectangle:{x:0,y:0,width:512,height:512},format:'rgba8'});",
    );
    let mut limits = HelperLimits::default();
    limits.engine.backing_bytes = 256 * 1024;
    let mut instance = instance(&script, quota.clone(), limits);
    let mut drawing =
        NativeDrawHost::new(quota.clone(), MediaLimits::default(), DrawLimits::default()).unwrap();
    let mut images = NativeImageHost::new(quota).unwrap();
    let mut refused = false;
    for _ in 0..8 {
        for request in instance.requests().unwrap() {
            let is_sample = request.method == "media.images.sample";
            let result = images.dispatch(&mut instance, &mut drawing, request);
            if is_sample {
                assert!(
                    result.is_err(),
                    "the helper must refuse the 1 MiB sample copy"
                );
                refused = true;
                break;
            }
            assert!(result.unwrap().is_none());
        }
        if refused {
            break;
        }
        assert_eq!(instance.pump().unwrap(), CreateState::Pending);
    }
    assert!(refused);
    assert!(images.closed);
    assert_eq!(images.pending.len(), 1);
    assert_eq!(images.owned.len(), 2);
    assert!(
        !images.is_drained(),
        "a failed ACK is not physical retirement"
    );
    let stopped = instance.stop();
    assert!(stopped.cancellation.is_ok());
    assert!(instance.is_physically_retired());
    images.revoke();
    images.release_terminal_after_helper_retirement(&mut drawing);
    assert!(images.is_drained());
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn actual_helper_images_borrow_registered_producer_without_transferring_close() {
    let quota = quota();
    let script = script("const opened=await host.media.images.decode({bytes:raw,max_pixels:4}); if(!opened.ok)throw Error('decode_ack'); const resized=await host.media.images.resize({image:opened.value,width:1,height:1,filter:'nearest'}); if(!resized.ok)throw Error('borrow_resize_ack'); const sampled=await host.media.images.sample({image:opened.value,rectangle:{x:0,y:0,width:1,height:1},format:'gray32'}); if(!sampled.ok || Math.abs(sampled.value[0]-0.2126)>0.01)throw Error('borrow_sample_ack'); host.media.images.close(opened.value); host.media.images.close(resized.value);");
    let mut instance = instance(&script, quota.clone(), HelperLimits::default());
    let mut drawing =
        NativeDrawHost::new(quota.clone(), MediaLimits::default(), DrawLimits::default()).unwrap();
    let mut producer = NativeImageHost::new(quota.clone()).unwrap();
    let mut consumer = NativeImageHost::new(quota.clone()).unwrap();
    let mut methods = Vec::new();
    let mut creation = CreateState::Pending;
    for _ in 0..12 {
        for request in instance.requests().unwrap() {
            methods.push(request.method.clone());
            let is_producer_close = request.method == "media.images.close"
                && request
                    .payload
                    .metadata()
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|id| id.parse::<u64>().ok())
                    .is_some_and(|id| producer.owned.contains(&id));
            let owner = if request.method == "media.images.decode" || is_producer_close {
                &mut producer
            } else {
                &mut consumer
            };
            assert!(owner
                .dispatch(&mut instance, &mut drawing, request)
                .unwrap()
                .is_none());
            for id in &producer.owned {
                assert!(
                    consumer.owned_image(ImageHandle::from_id(*id)).is_err(),
                    "borrowing must not give the consumer close custody"
                );
            }
        }
        creation = instance.pump().unwrap();
        if creation == CreateState::Ready && methods.len() == 5 {
            break;
        }
    }
    assert_eq!(creation, CreateState::Ready);
    assert_eq!(
        methods,
        [
            "media.images.decode",
            "media.images.resize",
            "media.images.sample",
            "media.images.close",
            "media.images.close"
        ]
    );
    assert!(producer.owned.is_empty() && consumer.owned.is_empty());
    assert!(producer.pending.is_empty() && consumer.pending.is_empty());
    assert!(instance.stop().cancellation.is_ok());
    assert!(instance.is_physically_retired());
    producer.revoke();
    consumer.revoke();
    producer.release_terminal_after_helper_retirement(&mut drawing);
    consumer.release_terminal_after_helper_retirement(&mut drawing);
    assert!(producer.is_drained() && consumer.is_drained());
    drop(instance);
    drop(producer);
    drop(consumer);
    drop(drawing);
    assert_eq!(quota.snapshot().worker_bytes, 0);
    assert_eq!(quota.snapshot().worker_threads, 0);
}
