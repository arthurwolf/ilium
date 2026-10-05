//! Explicit matching-helper tests. Fixture archive and data root are isolated;
//! helper RPC, bounded copy, native ACK and physical helper retirement are real.
use super::*;
use crate::{
    engine::CreateState,
    helper::HelperLimits,
    manifest::AnimationMode,
    permissions::{Capability, Ceiling, Right, Scope, Selection, UserChoice},
    runtime::InstancePreparation,
    trust::TrustVerifier,
    TRUSTED_BOOTSTRAP,
};
use ilium_execution::{ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaLimits};
use ilium_platform::secure_fs::NoFollowDirectory;
use std::{
    io::{Cursor, Write},
    path::PathBuf,
    sync::mpsc,
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
fn client(quota: &QuotaGroup) -> (Execution, Client, mpsc::Receiver<()>) {
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
    let (sender, receiver) = mpsc::sync_channel(16);
    let client = execution
        .client(ClientLimits {
            jobs: 8,
            service_jobs: 0,
            input_bytes: 16 * 1024 * 1024,
            result_bytes: 16 * 1024 * 1024,
        })
        .unwrap()
        .with_completion_wake(move || {
            let _ = sender.try_send(());
        });
    (execution, client, receiver)
}
fn state_root() -> (tempfile::TempDir, Arc<PinnedDirectory>) {
    let directory = tempfile::tempdir().unwrap();
    let root = Arc::new(
        PinnedDirectory::from_host(Arc::new(
            NoFollowDirectory::open_root(directory.path()).unwrap(),
        ))
        .unwrap(),
    );
    (directory, root)
}
fn archive(script: &str, asset: &[u8], state: bool, disk: bool, disk_write: bool) -> Vec<u8> {
    let entry = json!({"path":"entry.mjs","bytes":script.len(),
        "sha256":format!("{:x}",Sha256::digest(script.as_bytes()))});
    let bundled = json!({"path":"assets/default.bin","bytes":asset.len(),
        "sha256":format!("{:x}",Sha256::digest(asset))});
    let mut capabilities = if state {
        vec![json!({"id":"state.persist","scope":"session"})]
    } else {
        vec![]
    };
    if disk {
        capabilities.push(if disk_write {
            json!({"id":"disk.write","scope":{"selection":"folder","access":"write","slot":"pictures"}})
        } else {
            json!({"id":"disk.read","scope":{"selection":"folder","access":"read","slot":"pictures"}})
        });
    }
    let manifest = json!({"api_version":1,"id":"native-asset-qualification",
        "name":"Native asset qualification","version":"1.0.0","entry":"entry.mjs",
        "modes":["live"],"settings":{"type":"object","properties":{}},
        "capabilities":capabilities,"assets":[bundled.clone()],"files":[entry,bundled]});
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("entry.mjs", options).unwrap();
    zip.write_all(script.as_bytes()).unwrap();
    zip.start_file("assets/default.bin", options).unwrap();
    zip.write_all(asset).unwrap();
    zip.start_file("manifest.json", options).unwrap();
    zip.write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    zip.finish().unwrap().into_inner()
}
fn instance(
    script: &str,
    asset: &[u8],
    quota: QuotaGroup,
    limits: HelperLimits,
    state: bool,
    disk: Option<Arc<SelectedStorage>>,
) -> PackageInstance {
    instance_with_disk_write(script, asset, quota, limits, state, disk, false)
}
fn instance_with_disk_write(
    script: &str,
    asset: &[u8],
    quota: QuotaGroup,
    limits: HelperLimits,
    state: bool,
    disk: Option<Arc<SelectedStorage>>,
    disk_write: bool,
) -> PackageInstance {
    let helper = PathBuf::from(
        std::env::var_os("ILIUM_ANIMATION_HELPER")
            .expect("explicit qualification requires the matching release helper"),
    );
    assert!(helper.is_absolute());
    let bytes = archive(script, asset, state, disk.is_some(), disk_write);
    let _archive_admission = quota.reserve_external_storage(bytes.len() + 65536).unwrap();
    let verifier = TrustVerifier::from_release_inventory(Vec::new()).unwrap();
    let verified = PackageInstance::verify(InstancePreparation {
        archive: &bytes,
        verifier: &verifier,
        helper_executable: &helper,
        trusted_bootstrap: TRUSTED_BOOTSTRAP,
        settings: &json!({}),
        mode: AnimationMode::Live,
        environment: &json!({"cell_width":1,"cell_height":1,"dot_width":2,"dot_height":4}),
        host_policy: Ceiling {
            permissions: {
                let mut rights = if state {
                    vec![Right {
                        id: Capability::StatePersist,
                        scope: Scope::Namespace {
                            name: "session".into(),
                        },
                    }]
                } else {
                    vec![]
                };
                if disk.is_some() {
                    rights.push(Right {
                        id: if disk_write {
                            Capability::DiskWrite
                        } else {
                            Capability::DiskRead
                        },
                        scope: Scope::Disk {
                            slot: "pictures".into(),
                            selection: Selection::Folder,
                        },
                    });
                }
                rights
            },
        },
        instance_id: 113,
        limits,
        quota,
    })
    .unwrap();
    let (mut instance, review) = if state || disk.is_some() {
        verified.prepare(None).unwrap()
    } else {
        verified.prepare_without_rights().unwrap()
    };
    let review = if let Some(selected) = disk {
        let mut resources = BTreeMap::new();
        resources.insert("pictures".to_owned(), selected);
        instance
            .review_selected_resources(113, 2, resources, BTreeMap::new())
            .unwrap()
    } else {
        review
    };
    let answers = review
        .items()
        .iter()
        .map(|item| (item.request.request_id.clone(), UserChoice::AllowSession))
        .collect::<BTreeMap<_, _>>();
    let pending = instance.begin_resolution(review, answers).unwrap();
    let result = instance.finish_resolution(pending).unwrap();
    assert!(result.creation_error.is_none());
    assert_eq!(result.creation, Some(CreateState::Pending));
    instance
}
fn script(body: &str) -> String {
    format!("export function plan(){{return {{output:{{mode:'pixels',format:'gray8',update:'replace'}},fps:30,inputs:{{}},permissions:[]}};}} export async function create(host){{{body} return {{render(context,frame){{frame.gray.fill(0);frame.present();}},dispose(){{}}}};}}")
}
fn state_script(body: &str) -> String {
    format!("export function plan(){{return {{output:{{mode:'pixels',format:'gray8',update:'replace'}},fps:30,inputs:{{}},permissions:[{{request_id:'state',id:'state.persist',scope:'session',required:true,reason:'Fixture state'}}]}};}} export async function create(host){{{body} return {{render(context,frame){{frame.gray.fill(0);frame.present();}},dispose(){{}}}};}}")
}
fn disk_script(body: &str) -> String {
    format!("export function plan(){{return {{output:{{mode:'pixels',format:'gray8',update:'replace'}},fps:30,inputs:{{}},permissions:[{{request_id:'pictures',id:'disk.read',scope:{{selection:'folder',access:'read',slot:'pictures'}},required:true,reason:'Fixture picture'}}]}};}} export async function create(host){{{body} return {{render(context,frame){{frame.gray.fill(0);frame.present();}},dispose(){{}}}};}}")
}
fn disk_write_script(body: &str) -> String {
    format!("export function plan(){{return {{output:{{mode:'pixels',format:'gray8',update:'replace'}},fps:30,inputs:{{}},permissions:[{{request_id:'pictures',id:'disk.write',scope:{{selection:'folder',access:'write',slot:'pictures'}},required:true,reason:'Fixture output'}}]}};}} export async function create(host){{{body} return {{render(context,frame){{frame.gray.fill(0);frame.present();}},dispose(){{}}}};}}")
}
struct BlockIo {
    started: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
}
impl Job for BlockIo {
    type Output = ();
    type Error = String;
    fn run(self, _context: JobContext) -> std::result::Result<(), String> {
        self.started.send(()).map_err(|error| error.to_string())?;
        self.release.recv().map_err(|error| error.to_string())
    }
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn actual_helper_bundle_memory_cache_typed_bytes_ack_and_retirement() {
    let quota = quota();
    let (_execution, client, _receiver) = client(&quota);
    let (_directory, root) = state_root();
    let body = "const asset=await host.assets.read({grant:host.assets.bundle,relative_path:'assets/default.bin',max_bytes:16}); if(!asset.ok || !(asset.value.bytes instanceof Uint8Array) || asset.value.bytes[0]!==7)throw Error('bundle'); const listing=await host.assets.list({grant:host.assets.bundle,max_entries:8}); if(!listing.ok || listing.value.length!==1)throw Error('list'); const put=await host.cache.put({key:'frame',bytes:new Uint8Array([3,4]),ttl_seconds:60}); if(!put.ok)throw Error('put'); const got=await host.cache.get({key:'frame',max_bytes:4}); if(!got.ok || !(got.value instanceof Uint8Array) || got.value[1]!==4)throw Error('get'); const removed=await host.cache.remove('frame'); if(!removed.ok || !removed.value)throw Error('remove');";
    let mut instance = instance(
        &script(body),
        &[7, 8],
        quota.clone(),
        HelperLimits::default(),
        false,
        None,
    );
    let mut assets = NativeAssetHost::new(&instance, client, quota, root).unwrap();
    let mut methods = Vec::new();
    let mut creation = CreateState::Pending;
    for _ in 0..16 {
        for request in instance.requests().unwrap() {
            methods.push(request.method.clone());
            assert!(assets.dispatch(&mut instance, request).unwrap().is_none());
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
            "assets.list",
            "cache.put",
            "cache.get",
            "cache.remove"
        ]
    );
    assert!(assets.pending.is_empty());
    let stopped = instance.stop();
    assert!(stopped.cancellation.is_ok());
    assert!(instance.is_physically_retired());
    assets.revoke();
    assets.release_terminal_after_helper_retirement();
    assert!(assets.is_drained());
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn actual_helper_bundle_copy_refusal_retains_original_until_physical_join() {
    let quota = quota();
    let (_execution, client, _receiver) = client(&quota);
    let (_directory, root) = state_root();
    let body = "await host.assets.read({grant:host.assets.bundle,relative_path:'assets/default.bin',max_bytes:1048576});";
    let mut limits = HelperLimits::default();
    limits.engine.backing_bytes = 256 * 1024;
    let mut instance = instance(
        &script(body),
        &vec![5u8; 1024 * 1024],
        quota.clone(),
        limits,
        false,
        None,
    );
    let mut assets = NativeAssetHost::new(&instance, client, quota, root).unwrap();
    let mut refused = false;
    for _ in 0..8 {
        let mut requests = instance.requests().unwrap().into_iter();
        if let Some(request) = requests.next() {
            assert_eq!(request.method, "assets.read");
            assert!(assets.dispatch(&mut instance, request).is_err());
            refused = true;
        }
        drop(requests);
        if refused {
            break;
        }
        assert_eq!(instance.pump().unwrap(), CreateState::Pending);
    }
    assert!(refused);
    assert!(assets.closed);
    assert_eq!(assets.pending.len(), 1);
    assert!(!assets.is_drained());
    let stopped = instance.stop();
    assert!(stopped.cancellation.is_ok());
    assert!(instance.is_physically_retired());
    assets.revoke();
    assets.release_terminal_after_helper_retirement();
    assert!(assets.is_drained());
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn actual_helper_persistent_cache_state_grant_finite_ack_and_remove() {
    let quota = quota();
    let (_execution, client, receiver) = client(&quota);
    let (_directory, root) = state_root();
    let body = "const put=await host.cache.put({key:'saved',bytes:new Uint8Array([11,12]),persistent:true,ttl_seconds:60}); if(!put.ok)throw Error('persistent_put'); const got=await host.cache.get({key:'saved',max_bytes:16}); if(!got.ok || !(got.value instanceof Uint8Array) || got.value[1]!==12)throw Error('persistent_get'); const removed=await host.cache.remove('saved'); if(!removed.ok || !removed.value)throw Error('persistent_remove');";
    let mut instance = instance(
        &state_script(body),
        &[1],
        quota.clone(),
        HelperLimits::default(),
        true,
        None,
    );
    let mut assets = NativeAssetHost::new(&instance, client, quota, root).unwrap();
    assert_eq!(assets.state_request_id.as_deref(), Some("state"));
    let mut methods = Vec::new();
    let mut creation = CreateState::Pending;
    for _ in 0..16 {
        for request in instance.requests().unwrap() {
            methods.push(request.method.clone());
            assert!(assets.dispatch(&mut instance, request).unwrap().is_none());
            if !assets.persistent_pending.is_empty() {
                receiver
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                assets.on_completion_wake(&mut instance).unwrap();
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
        ["cache.put", "cache.get", "cache.remove"]
    );
    assert!(assets.persistent_pending.is_empty());
    assert!(assets
        .persistent
        .get("saved", 16, &StorageCancellation::default())
        .unwrap()
        .is_none());
    let stopped = instance.stop();
    assert!(stopped.cancellation.is_ok());
    assert!(instance.is_physically_retired());
    assets.revoke();
    assets.release_terminal_after_helper_retirement();
    assert!(assets.is_drained());
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn actual_helper_retirement_retains_forced_lost_selected_receipt_until_terminal_wake() {
    let quota = quota();
    let (_execution, client, receiver) = client(&quota);
    let (started_sender, started_receiver) = mpsc::sync_channel(1);
    let (release_sender, release_receiver) = mpsc::sync_channel(1);
    let mut blocker = client
        .try_reserve(
            Lane::Io,
            JobCost {
                input_bytes: 128,
                result_bytes: 128,
            },
        )
        .unwrap()
        .submit(BlockIo {
            started: started_sender,
            release: release_receiver,
        })
        .unwrap();
    started_receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let (_state_directory, state_root) = state_root();
    let selected_directory = tempfile::tempdir().unwrap();
    std::fs::write(selected_directory.path().join("item"), b"pinned").unwrap();
    let selected = Arc::new(
        SelectedStorage::pin_user_path(
            selected_directory.path(),
            Selection::Folder,
            "pictures".into(),
            false,
            quota.clone(),
        )
        .unwrap(),
    );
    let body = "const grant=host.permissions.get('pictures'); if(!grant || !grant.handle)throw Error('selected_grant'); const read=await host.assets.read({grant:grant.handle,relative_path:'item',max_bytes:16}); if(!read.ok)throw Error('selected_read');";
    let mut instance = instance(
        &disk_script(body),
        &[1],
        quota.clone(),
        HelperLimits::default(),
        false,
        Some(selected),
    );
    let mut assets = NativeAssetHost::new(&instance, client, quota, state_root).unwrap();
    let mut issued = false;
    for _ in 0..8 {
        for request in instance.requests().unwrap() {
            assert_eq!(request.method, "assets.read");
            assert!(assets.dispatch(&mut instance, request).unwrap().is_none());
            issued = true;
        }
        if issued {
            break;
        }
        assert_eq!(instance.pump().unwrap(), CreateState::Pending);
    }
    assert!(issued);
    assert_eq!(assets.selected_pending.len(), 1);
    let id = *assets.selected_pending.keys().next().unwrap();
    // Inject the exact Lost branch while the real receipt is demonstrably
    // queued behind the blocker. A disconnected worker channel cannot be
    // induced through the public execution API without corrupting its owner.
    assert!(assets.apply_selected_poll(id, JobPoll::Lost).is_err());
    assert!(assets.closed);
    assert!(assets.selected_pending[&id].receipt.is_some());
    let stopped = instance.stop();
    assert!(stopped.cancellation.is_ok());
    assert!(instance.is_physically_retired());
    assets.revoke();
    assets.release_terminal_after_helper_retirement();
    assert_eq!(
        assets.selected_pending.len(),
        1,
        "helper exit must not erase the original queued finite receipt"
    );
    assert!(!assets.is_drained());
    release_sender.send(()).unwrap();
    let mut blocker_terminal = false;
    for _ in 0..4 {
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        if !blocker_terminal && matches!(blocker.try_take(), JobPoll::Ready(_)) {
            blocker_terminal = true;
        }
        assets.on_completion_wake(&mut instance).unwrap();
        assets.release_terminal_after_helper_retirement();
        if assets.is_drained() {
            break;
        }
    }
    assert!(blocker_terminal);
    assert!(
        assets.is_drained(),
        "selected finite receipt must physically settle"
    );
}

#[test]
#[ignore = "run explicitly with matching ILIUM_ANIMATION_HELPER and delegated sandbox"]
fn actual_helper_selected_write_copy_refusal_keeps_committed_result_and_original_ticket() {
    let quota = quota();
    let (_execution, client, receiver) = client(&quota);
    let (started_sender, started_receiver) = mpsc::sync_channel(1);
    let (release_sender, release_receiver) = mpsc::sync_channel(1);
    let mut blocker = client
        .try_reserve(
            Lane::Io,
            JobCost {
                input_bytes: 128,
                result_bytes: 128,
            },
        )
        .unwrap()
        .submit(BlockIo {
            started: started_sender,
            release: release_receiver,
        })
        .unwrap();
    started_receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let (_state_directory, state_root) = state_root();
    let selected_directory = tempfile::tempdir().unwrap();
    let selected = Arc::new(
        SelectedStorage::pin_user_path(
            selected_directory.path(),
            Selection::Folder,
            "pictures".into(),
            true,
            quota.clone(),
        )
        .unwrap(),
    );
    let body = "const hold=new Uint8Array(65536); const grant=host.permissions.get('pictures'); if(!grant || !grant.handle)throw Error('selected_grant'); const written=await host.assets.write({grant:grant.handle,relative_path:'committed.bin',bytes:new Uint8Array([7,8,9]),overwrite:true}); if(hold[0]!==0 || !written.ok)throw Error('selected_write');";
    let mut limits = HelperLimits::default();
    limits.engine.backing_bytes = 128 * 1024;
    let mut instance = instance_with_disk_write(
        &disk_write_script(body),
        &[1],
        quota.clone(),
        limits,
        false,
        Some(selected),
        true,
    );
    let mut assets = NativeAssetHost::new(&instance, client, quota.clone(), state_root).unwrap();
    let mut issued = false;
    for _ in 0..8 {
        for request in instance.requests().unwrap() {
            assert_eq!(request.method, "assets.write");
            assert!(assets.dispatch(&mut instance, request).unwrap().is_none());
            issued = true;
        }
        if issued {
            break;
        }
        assert_eq!(instance.pump().unwrap(), CreateState::Pending);
    }
    assert!(issued);
    let id = *assets.selected_pending.keys().next().unwrap();
    // The real write is still queued. Replace only its test completion with
    // a valid, pre-admitted binary descriptor whose V8 copy exceeds remaining
    // backing once the guest's live hold is counted. The original operation,
    // bytes, finite receipt and ticket are unchanged.
    let mut planes = BTreeMap::new();
    planes.insert("b0".to_owned(), vec![0u8; 128 * 1024]);
    let value = ServiceValue::copy_from_host(
        &json!({"ok":true,"value":{"sha256":"forced_copy_refusal","test_plane":{"$ilium_binary":"b0"}}}),
        &[ArraySpec {
            name: "b0".into(),
            kind: TypedArrayKind::U8,
            elements: 128 * 1024,
        }],
        &planes,
        instance.engine_limits(),
        quota.clone(),
    )
    .unwrap();
    assets.selected_pending.get_mut(&id).unwrap().value = Some(value);
    release_sender.send(()).unwrap();
    for _ in 0..2 {
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
    }
    assert!(matches!(blocker.try_take(), JobPoll::Ready(_)));
    assert_eq!(
        std::fs::read(selected_directory.path().join("committed.bin")).unwrap(),
        [7, 8, 9]
    );
    assert!(assets.on_completion_wake(&mut instance).is_err());
    assert!(assets.closed);
    let record = &assets.selected_pending[&id];
    assert!(record.receipt.is_none());
    assert!(matches!(
        record.output.as_ref(),
        Some(Ok(SelectedOutput::Write(_)))
    ));
    assert!(record.value.is_some());
    assert!(!assets.is_drained());
    assert_eq!(
        std::fs::read(selected_directory.path().join("committed.bin")).unwrap(),
        [7, 8, 9]
    );
    let stopped = instance.stop();
    assert!(stopped.cancellation.is_ok());
    assert!(instance.is_physically_retired());
    assets.revoke();
    assets.release_terminal_after_helper_retirement();
    assert!(assets.is_drained());
}
