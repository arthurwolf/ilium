//! Headless installed animation qualification through the shipped client and
//! its sibling helper. This command runs inside the package's native launcher.

use crate::{animation_plugins::PluginCatalogue, execution};
use anyhow::{bail, Context, Result};
use ilium_animation_js::{
    engine::{ArraySpec, CreateState, TypedArrayKind},
    helper::HelperLimits,
    manifest::AnimationMode,
    permissions::Ceiling,
    release,
    runtime::{InstancePreparation, PackageInstance},
    surface::{Data, Format, FrameMeta, NoNativeRenderer, Planes, Shape, Surface},
    TRUSTED_BOOTSTRAP,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::Path,
};

fn regular_bytes(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect installed member {}", path.display()))?;
    if !metadata.file_type().is_file() {
        bail!(
            "installed animation member is not a regular file: {}",
            path.display()
        );
    }
    fs::read(path).with_context(|| format!("read installed member {}", path.display()))
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn array(name: &str, kind: TypedArrayKind, elements: usize) -> ArraySpec {
    ArraySpec {
        name: name.into(),
        kind,
        elements,
    }
}

fn words(bytes: Vec<u8>) -> Result<Vec<u32>> {
    if !bytes.len().is_multiple_of(4) {
        bail!("rendered order plane is not word aligned");
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|word| u32::from_ne_bytes([word[0], word[1], word[2], word[3]]))
        .collect())
}

fn emit(record: serde_json::Value) -> Result<()> {
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, &record)?;
    writeln!(output)?;
    output.flush()?;
    Ok(())
}

/// The normal CLI bootstrap already registered the single process quota.
/// The nested helper consumes and returns a debit from that exact owner.
pub fn probe() -> Result<()> {
    let client = std::env::current_exe().context("installed client executable")?;
    let client_bytes = regular_bytes(&client)?;
    let helper = ilium_platform::animation_sandbox::helper_executable_path(&client)
        .context("platform helper path")?;
    let helper_bytes = regular_bytes(&helper)?;
    let bundled = helper
        .parent()
        .context("installed helper has no parent directory")?;
    let catalogue = PluginCatalogue::discover_default().map_err(anyhow::Error::msg)?;
    let verifier = release::verifier().context("compiled official trust inventory")?;
    let quota = execution::process_quota();
    let baseline = quota.snapshot();
    let mut archives = Vec::with_capacity(release::PACKAGES.len());
    for &(id, filename, expected_digest) in release::PACKAGES {
        let descriptor = catalogue
            .find(id)
            .with_context(|| format!("installed catalogue did not discover {id}"))?;
        let expected_path = bundled.join(filename);
        if descriptor.archive_path != expected_path {
            bail!("{id} resolved outside the installed helper directory");
        }
        let bytes = regular_bytes(&descriptor.archive_path)?;
        if digest(&bytes) != expected_digest {
            bail!("installed {id} differs from compiled official archive digest");
        }
        archives.push((id, expected_digest, bytes));
    }
    emit(json!({"type":"artifact","gate":"installed_catalogue",
        "client_path":client,"client_sha256":digest(&client_bytes),
        "helper_path":helper,"helper_sha256":digest(&helper_bytes),
        "packages":release::PACKAGES.iter().map(|item| item.0).collect::<Vec<_>>(),
        "worker_threads_before":baseline.worker_threads,
        "worker_bytes_before":baseline.worker_bytes}))?;

    for (index, (id, expected_digest, bytes)) in archives.iter().enumerate() {
        let environment =
            json!({"cell_width": 24, "cell_height": 12, "dot_width": 48, "dot_height": 48});
        let verified = PackageInstance::verify(InstancePreparation {
            archive: bytes,
            verifier: &verifier,
            helper_executable: &helper,
            trusted_bootstrap: TRUSTED_BOOTSTRAP,
            settings: &json!({}),
            mode: AnimationMode::Live,
            environment: &environment,
            host_policy: Ceiling {
                permissions: Vec::new(),
            },
            instance_id: (index + 1) as u64,
            limits: HelperLimits::default(),
            quota: quota.clone(),
        })
        .with_context(|| format!("{id} archive verification"))?;
        let (mut instance, review) = verified
            .prepare_without_rights()
            .with_context(|| format!("{id} helper plan"))?;
        let render_result = (|| -> Result<()> {
            if instance.package().manifest().id != *id
                || instance.active_identity().is_some()
                || !review.items().is_empty()
                || !instance.plan().inputs.is_empty()
            {
                bail!("{id} plan or permission review differs from approved default");
            }
            let pending = instance
                .begin_resolution(review, BTreeMap::new())
                .with_context(|| format!("{id} native authorization"))?;
            let resolution = instance
                .finish_resolution(pending)
                .with_context(|| format!("{id} helper creation"))?;
            if !resolution.denied_required.is_empty()
                || resolution.teardown_error.is_some()
                || resolution.creation_error.is_some()
                || resolution.authority_error.is_some()
                || resolution.activation_invalidation.is_some()
                || resolution.creation != Some(CreateState::Ready)
                || !instance
                    .active_identity()
                    .is_some_and(|identity| identity.is_ilium())
                || !instance.requests()?.is_empty()
            {
                bail!("{id} native authorization or helper creation failed");
            }

            let output = instance
                .plan()
                .output
                .as_ref()
                .context("approved output plan")?;
            let shape: Shape = serde_json::from_value(json!({
                "cell_width":24,"cell_height":12,"mode":output.mode,
                "format":output.format,"update":output.update,
                "cell_rgb":output.cell_rgb,
                "colour_space":output.colour_space.as_deref().unwrap_or("srgb")
            }))?;
            if shape.format != Format::Gray32 || shape.cell_rgb {
                bail!("{id} changed its expected native frame shape");
            }
            let layout = shape.layout()?;
            let mut surface = Surface::new((index + 1) as u64, 1, shape)?;
            for sequence in 1..=2 {
                let seed = surface.begin(sequence)?;
                let Data::F32(values) = seed.data else {
                    bail!("{id} changed its seed plane format");
                };
                let mut seed_data = Vec::with_capacity(values.len() * 4);
                for value in values {
                    seed_data.extend_from_slice(&value.to_ne_bytes());
                }
                instance.seed_frame(
                    &json!({"frame":{"key":seed.key,"shape":seed.shape,"reset":seed.reset,
                    "invalid_rects":seed.invalid_rects,"input_specs":[]}}),
                    &[array("work_data", TypedArrayKind::F32, layout.elements)],
                    &BTreeMap::from([("work_data".into(), seed_data)]),
                )?;
                let arrays = [
                    array("work_data", TypedArrayKind::F32, layout.elements),
                    array("data", TypedArrayKind::F32, layout.elements),
                    array("work_touch", TypedArrayKind::U8, layout.samples),
                    array("touch", TypedArrayKind::U8, layout.samples),
                    array("work_order", TypedArrayKind::U32, layout.samples),
                    array("order", TypedArrayKind::U32, layout.samples),
                ];
                let (output, retained_storage) = instance
                    .render(
                        &json!({"time":sequence as f64 * 0.2,"wall":0.0,"delta":0.2,"inputs":{}}),
                        &arrays,
                    )?
                    .into_parts();
                let metadata = FrameMeta::parse(&serde_json::to_vec(&output.metadata)?)?;
                if !metadata.presented || metadata.error.is_some() {
                    bail!("{id} helper did not present an accepted frame");
                }
                let mut planes = output.planes;
                let data = planes.remove("data").context("sealed data plane")?;
                if data.len() != layout.data_bytes {
                    bail!("{id} data plane length differs");
                }
                let pixels: Vec<f32> = data
                    .chunks_exact(4)
                    .map(|word| f32::from_ne_bytes([word[0], word[1], word[2], word[3]]))
                    .collect();
                if pixels
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
                    || !pixels.iter().any(|value| *value > 0.0)
                {
                    bail!("{id} rendered an invalid or blank frame");
                }
                let outcome = surface.finish(
                    metadata,
                    Planes {
                        data: Data::F32(pixels),
                        touch: planes.remove("touch").context("sealed touch plane")?,
                        order: words(planes.remove("order").context("sealed order plane")?)?,
                        cell_rgb: None,
                        colour_touch: None,
                        colour_order: None,
                    },
                    &mut NoNativeRenderer,
                )?;
                if !outcome.accepted {
                    bail!("{id} native surface rejected its helper frame");
                }
                instance.accept_frame(outcome.accepted)?;
                if !instance.requests()?.is_empty() {
                    bail!("{id} requested an undeclared host capability");
                }
                drop(retained_storage);
            }
            Ok(())
        })();
        let stopped = instance.stop();
        let physically_retired = instance.is_physically_retired();
        drop(instance);
        let after = quota.snapshot();
        stopped
            .cancellation
            .with_context(|| format!("{id} physical helper retirement"))?;
        if !physically_retired || stopped.authority_error.is_some() {
            bail!("{id} helper or native authority was not retired");
        }
        if after.worker_threads != baseline.worker_threads
            || after.worker_bytes != baseline.worker_bytes
        {
            bail!("{id} retained a worker debit after helper retirement");
        }
        render_result?;
        emit(
            json!({"type":"artifact","gate":"installed_render","package":id,
            "archive_sha256":expected_digest,"helper_sha256":digest(&helper_bytes),
            "rendered_frames":2,"physical_retirement":true,
            "worker_threads_before":baseline.worker_threads,
            "worker_threads_after":after.worker_threads,
            "worker_bytes_before":baseline.worker_bytes,
            "worker_bytes_after":after.worker_bytes}),
        )?;
    }
    emit(json!({"type":"result","state":"passed",
        "gate":"installed_animation","publication_allowed":false,
        "packages":release::PACKAGES.iter().map(|item| item.0).collect::<Vec<_>>()}))?;
    Ok(())
}
