//! Headless installed animation qualification through the shipped client and
//! its sibling helper. This command runs inside the package's native launcher.

use crate::{
    animation_plugins::PluginCatalogue,
    execution,
    filesystem::plugin_permissions::{
        PermissionCompletion, PersistenceStamp, PluginPermissionFiles,
    },
};
use anyhow::{bail, Context, Result};
use ilium_animation_js::{
    engine::{ArraySpec, CreateState, TypedArrayKind},
    helper::HelperLimits,
    manifest::AnimationMode,
    package::PackageLimits,
    permissions::{Ceiling, PlanReview},
    release,
    runtime::{InstancePreparation, PackageInstance, VerifiedPreparation},
    surface::{Data, Format, FrameMeta, NoNativeRenderer, Planes, Shape, Surface},
    TRUSTED_BOOTSTRAP,
};
use ilium_execution::{Client, ClientLimits, QuotaGroup, StorageAdmission};
use ilium_platform::{animation_files::PinnedDirectory, secure_fs::NoFollowDirectory};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    path::Path,
    sync::Arc,
    time::Duration,
};

async fn prepare_with_native_ledger(
    verified: VerifiedPreparation,
    client: &Client,
    root: Arc<PinnedDirectory>,
) -> Result<(PackageInstance, PlanReview)> {
    let notification = Arc::new(tokio::sync::Notify::new());
    let wake = Arc::clone(&notification);
    let mut files = PluginPermissionFiles::new(
        client,
        root,
        Arc::clone(&notification),
        Arc::new(move || wake.notify_one()),
    )
    .map_err(anyhow::Error::msg)?;
    let stamp = PersistenceStamp {
        selection_revision: verified.instance_id(),
        instance_id: verified.instance_id(),
        plan_revision: 1,
        authorization_epoch: verified.authorization_epoch(),
    };
    let fence = files.bind(verified.principal(), stamp)?;
    files.request_load(&fence)?;
    let loaded = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(completion) = files.collect() {
                return match completion {
                    PermissionCompletion::Loaded(loaded) => Ok(loaded),
                    PermissionCompletion::Failed(error) => Err(anyhow::Error::new(error)),
                    PermissionCompletion::Written(_) => anyhow::bail!("unexpected ledger write"),
                };
            }
            notification.notified().await;
        }
    })
    .await
    .context("native permission ledger load deadline")??;
    let snapshot = loaded
        .view()
        .as_ref()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    if !snapshot.is_current() || snapshot.stamp() != stamp {
        bail!("native permission ledger load lost its selection binding");
    }
    // None comes only from this actual missing-state load, never an assumed
    // empty ledger. The broker consumes remembered bytes before helper launch.
    let prepared = verified.prepare(snapshot.bytes())?;
    files.close_admission();
    if !files.is_physically_settled() {
        bail!("native permission ledger load is not physically settled");
    }
    Ok(prepared)
}

fn open_installed_file(path: &Path) -> Result<fs::File> {
    let parent = path
        .parent()
        .context("installed member has no parent directory")?;
    let leaf = path
        .file_name()
        .context("installed member has no filename")?;
    let directory = NoFollowDirectory::open_root(parent)
        .with_context(|| format!("open installed directory {}", parent.display()))?;
    directory
        .open_regular(leaf)
        .with_context(|| format!("open installed regular file {}", path.display()))
}

fn digest_installed_file(path: &Path) -> Result<String> {
    let mut file = open_installed_file(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 32 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("read installed member {}", path.display()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{digest:x}"))
}

struct AdmittedArchive {
    bytes: Vec<u8>,
    _admission: StorageAdmission,
}

fn read_admitted_archive(
    mut archive: impl Read,
    expected_size: usize,
    quota: &QuotaGroup,
) -> Result<AdmittedArchive> {
    let maximum = usize::try_from(PackageLimits::default().archive_bytes)
        .context("animation archive size limit")?;
    if expected_size > maximum {
        bail!("installed animation archive exceeds {maximum} bytes");
    }
    let reserved_size = expected_size
        .checked_add(1)
        .context("installed animation archive read size overflow")?;
    let admission = quota
        .reserve_external_storage(reserved_size)
        .map_err(|error| anyhow::anyhow!("installed archive admission: {error:?}"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(reserved_size)
        .context("installed animation archive allocation")?;
    archive
        .take(reserved_size as u64)
        .read_to_end(&mut bytes)
        .context("read installed animation archive")?;
    if bytes.len() != expected_size {
        bail!("installed animation archive changed size during read");
    }
    Ok(AdmittedArchive {
        bytes,
        _admission: admission,
    })
}

fn admitted_archive_from_path(path: &Path, quota: &QuotaGroup) -> Result<AdmittedArchive> {
    let archive = open_installed_file(path)?;
    let metadata = archive
        .metadata()
        .with_context(|| format!("inspect opened animation archive {}", path.display()))?;
    let expected_size = usize::try_from(metadata.len()).context("installed archive size")?;
    if expected_size > usize::try_from(PackageLimits::default().archive_bytes)? {
        bail!("installed animation archive exceeds 32 MiB");
    }
    read_admitted_archive(archive, expected_size, quota)
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
pub async fn probe() -> Result<()> {
    let bank = execution::ClientExecution::start_async().await?;
    let result = probe_with_bank(&bank).await;
    let shutdown = bank.shutdown().await;
    match (result, shutdown) {
        (Err(error), Err(shutdown)) => {
            Err(error.context(format!("probe bank shutdown: {shutdown}")))
        }
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error).context("probe bank shutdown"),
        (Ok(()), Ok(())) => Ok(()),
    }
}

#[cfg(test)]
mod archive_admission_tests {
    use super::*;
    use ilium_execution::QuotaLimits;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    fn quota(worker_bytes: usize) -> QuotaGroup {
        QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 0,
            worker_bytes,
        })
    }

    struct ReadCounter(Arc<AtomicUsize>);
    impl Read for ReadCounter {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(0)
        }
    }

    #[test]
    fn installed_archive_admission_precedes_read_and_covers_buffer_lifetime() {
        let quota = quota(4);
        let reads = Arc::new(AtomicUsize::new(0));
        let result = read_admitted_archive(ReadCounter(Arc::clone(&reads)), 4, &quota);
        assert!(
            result.is_err(),
            "size-plus-one reservation should exceed quota"
        );
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);

        let quota = quota(5);
        let admitted = read_admitted_archive(io::Cursor::new(b"pack"), 4, &quota)
            .expect("archive read is admitted");
        assert_eq!(admitted.bytes.as_slice(), b"pack");
        assert_eq!(quota.snapshot().worker_bytes, 5);
        drop(admitted);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn changed_or_oversized_installed_archive_releases_admission() {
        let quota = quota(16);
        assert!(read_admitted_archive(io::Cursor::new(b"larger"), 4, &quota).is_err());
        assert_eq!(quota.snapshot().worker_bytes, 0);

        let maximum = usize::try_from(PackageLimits::default().archive_bytes).unwrap();
        assert!(read_admitted_archive(io::Cursor::new([]), maximum + 1, &quota).is_err());
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn installed_archive_path_rejects_non_files_and_keeps_file_admission() {
        let directory = tempfile::tempdir().expect("temporary archive directory");
        let quota = quota(5);
        assert!(admitted_archive_from_path(directory.path(), &quota).is_err());
        assert_eq!(quota.snapshot().worker_bytes, 0);

        let path = directory.path().join("package.iliumanim");
        fs::write(&path, b"pack").expect("write temporary archive");
        assert_eq!(
            digest_installed_file(&path).expect("stream installed-file digest"),
            digest(b"pack")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let link = directory.path().join("linked-package.iliumanim");
            symlink(&path, &link).expect("create symlink fixture");
            assert!(admitted_archive_from_path(&link, &quota).is_err());
            assert!(digest_installed_file(&link).is_err());
            assert_eq!(quota.snapshot().worker_bytes, 0);
        }
        let archive = admitted_archive_from_path(&path, &quota).expect("admit archive file");
        assert_eq!(archive.bytes.as_slice(), b"pack");
        assert_eq!(quota.snapshot().worker_bytes, 5);
        drop(archive);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn multiple_installed_archive_leases_remain_charged_together() {
        let quota = quota(10);
        let first = read_admitted_archive(io::Cursor::new(b"one!"), 4, &quota)
            .expect("first archive is admitted");
        let second = read_admitted_archive(io::Cursor::new(b"two!"), 4, &quota)
            .expect("second archive is admitted");
        assert_eq!(quota.snapshot().worker_bytes, 10);

        drop(first);
        assert_eq!(quota.snapshot().worker_bytes, 5);
        drop(second);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}

async fn probe_with_bank(bank: &execution::ClientExecution) -> Result<()> {
    let client = std::env::current_exe().context("installed client executable")?;
    let client_digest = digest_installed_file(&client)?;
    let helper = ilium_platform::animation_sandbox::helper_executable_path(&client)
        .context("platform helper path")?;
    let helper_digest = digest_installed_file(&helper)?;
    let bundled = helper
        .parent()
        .context("installed helper has no parent directory")?;
    let catalogue = PluginCatalogue::discover_default().map_err(anyhow::Error::msg)?;
    let verifier = release::verifier().context("compiled official trust inventory")?;
    let quota = execution::process_quota();
    let permission_client = bank
        .client(ClientLimits {
            jobs: 2,
            service_jobs: 0,
            input_bytes: 8 * 1024 * 1024,
            result_bytes: 32 * 1024 * 1024,
        })
        .map_err(|error| anyhow::anyhow!("permission client admission: {error:?}"))?;
    let _root_storage = quota
        .reserve_external_storage(64 * 1024)
        .map_err(|error| anyhow::anyhow!("permission root admission: {error:?}"))?;
    let directories =
        directories::ProjectDirs::from("", "", "ilium").context("Ilium permission directory")?;
    let permission_path = directories.config_dir().join("animation-permissions");
    ilium_platform::secure_fs::create_private_directory(&permission_path)?;
    let permission_root = Arc::new(PinnedDirectory::from_host(Arc::new(
        NoFollowDirectory::open_root(&permission_path)?,
    ))?);
    let mut archives = Vec::with_capacity(release::PACKAGES.len());
    for &(id, filename, expected_digest) in release::PACKAGES {
        let descriptor = catalogue
            .find(id)
            .with_context(|| format!("installed catalogue did not discover {id}"))?;
        let expected_path = bundled.join(filename);
        if descriptor.archive_path != expected_path {
            bail!("{id} resolved outside the installed helper directory");
        }
        let archive = admitted_archive_from_path(&descriptor.archive_path, &quota)?;
        if digest(&archive.bytes) != expected_digest {
            bail!("installed {id} differs from compiled official archive digest");
        }
        archives.push((id, expected_digest, archive));
    }
    let baseline = quota.snapshot();
    emit(json!({"type":"artifact","gate":"installed_catalogue",
        "client_path":client,"client_sha256":client_digest,
        "helper_path":helper,"helper_sha256":helper_digest,
        "packages":release::PACKAGES.iter().map(|item| item.0).collect::<Vec<_>>(),
        "worker_threads_before":baseline.worker_threads,
        "worker_bytes_before":baseline.worker_bytes}))?;

    for (index, (id, expected_digest, archive)) in archives.iter().enumerate() {
        let environment =
            json!({"cell_width": 24, "cell_height": 12, "dot_width": 48, "dot_height": 48});
        let verified = PackageInstance::verify(InstancePreparation {
            archive: &archive.bytes,
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
        let (mut instance, review) =
            prepare_with_native_ledger(verified, &permission_client, Arc::clone(&permission_root))
                .await
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
                        &json!({"time":sequence as f64 * 0.2,"wall":0.0,"delta":0.2,"inputs":{},
                            "_ilium_frame":{"key":seed.key,"shape":seed.shape}}),
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
            "archive_sha256":expected_digest,"helper_sha256":helper_digest,
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
