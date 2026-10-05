//! Real helper process and original broker activation for pure task yields.
#![cfg(all(feature = "v8-runtime", feature = "native-host"))]
use ilium_animation_js::{
    engine::CreateState,
    helper::HelperLimits,
    manifest::AnimationMode,
    native_task_host::NativeTaskHost,
    permissions::Ceiling,
    runtime::{InstancePreparation, PackageInstance},
    trust::TrustVerifier,
    TRUSTED_BOOTSTRAP,
};
use ilium_execution::{QuotaGroup, QuotaLimits};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn quota() -> QuotaGroup {
    QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 32,
        worker_bytes: 1024 * 1024 * 1024,
    })
}

fn accepted(
    source: &str,
    instance_id: u64,
    quota: QuotaGroup,
    mode: AnimationMode,
) -> (PackageInstance, CreateState) {
    let executable = std::env::var("ILIUM_ANIMATION_HELPER")
        .expect("real built ILIUM_ANIMATION_HELPER required for native task qualification");
    let manifest = json!({"api_version":1,"id":"native-task-helper-contract",
        "name":"Native task helper contract","version":"1.0.0","entry":"entry.mjs",
        "modes":[if mode == AnimationMode::PreRendered {"pre_rendered"} else {"live"}],
        "settings":{"type":"object","properties":{}},
        "files":[{"path":"entry.mjs","bytes":source.len(),
            "sha256":format!("{:x}",Sha256::digest(source.as_bytes()))}]});
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive.start_file("entry.mjs", options).unwrap();
    archive.write_all(source.as_bytes()).unwrap();
    archive.start_file("manifest.json", options).unwrap();
    archive
        .write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    let verifier = TrustVerifier::from_release_inventory(vec![]).unwrap();
    let settings = json!({});
    let environment = json!({});
    let verified = PackageInstance::verify(InstancePreparation {
        archive: &bytes,
        verifier: &verifier,
        helper_executable: Path::new(&executable),
        trusted_bootstrap: TRUSTED_BOOTSTRAP,
        settings: &settings,
        mode,
        environment: &environment,
        host_policy: Ceiling {
            permissions: vec![],
        },
        instance_id,
        limits: HelperLimits::default(),
        quota,
    })
    .unwrap();
    let (mut instance, review) = verified.prepare_without_rights().unwrap();
    let pending = instance.begin_resolution(review, BTreeMap::new()).unwrap();
    let resolution = instance.finish_resolution(pending).unwrap();
    let creation = resolution
        .accepted_creation()
        .expect("original activation accepted");
    (instance, creation)
}

#[test]
fn helper_two_sequential_yields_are_acked_before_replay_certification() {
    let source = r#"export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{},replay:{seed:3,duration_seconds:1,seamless:false}}}
        export async function create(host){
          for(let i=0;i<2;i++){const result=await host.tasks.yield();if(!result.ok)throw Error(result.error.code)}
          return {render(){},dispose(){}};
        }"#;
    let quota = quota();
    let (mut instance, creation) = accepted(source, 401, quota.clone(), AnimationMode::PreRendered);
    assert_eq!(creation, CreateState::Pending);
    let mut tasks = NativeTaskHost::new(quota.clone(), instance.engine_limits().clone()).unwrap();
    for index in 0..2 {
        let requests = instance.requests().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "tasks.yield");
        tasks
            .dispatch(&mut instance, requests.into_iter().next().unwrap())
            .unwrap();
        std::thread::sleep(Duration::from_millis(2));
        assert!(tasks.on_due(&mut instance, Instant::now()).unwrap());
        assert_eq!(
            instance.pump().unwrap(),
            if index == 0 {
                CreateState::Pending
            } else {
                CreateState::Ready
            }
        );
    }
    assert!(instance.requests().unwrap().is_empty());
    instance.certify_procedural_replay().unwrap();
    tasks.revoke();
    assert!(tasks.is_drained());
    instance.retire_helper().unwrap();
    assert!(instance.is_physically_retired());
}

#[test]
fn live_helper_declares_mode_without_replacing_physical_date_or_random() {
    let source = r#"const date = Object.getOwnPropertyDescriptor(globalThis, "Date");
        const math = Object.getOwnPropertyDescriptor(globalThis, "Math");
        const random = Object.getOwnPropertyDescriptor(Math, "random");
        // Replay installs non-writable replacements; live keeps V8's physical globals.
        const liveNow = Date.now(), randomValue = Math.random();
        if (!date.writable || !date.configurable || !math.writable ||
            !math.configurable || !random.writable || !random.configurable ||
            !Number.isFinite(liveNow) || Math.abs(liveNow - __HOST_NOW_MS__) > 600000 ||
            randomValue < 0 || randomValue >= 1) throw Error("live_ambient_replaced");
        export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{}}}
        export async function create(host){
          const yielded=await host.tasks.yield();
          if(!yielded.ok)throw Error(yielded.error.code);
          return {render(){},dispose(){}};
        }"#;
    let host_now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let source = source.replace("__HOST_NOW_MS__", &host_now_ms.to_string());
    let quota = quota();
    let (mut instance, creation) = accepted(&source, 411, quota.clone(), AnimationMode::Live);
    assert_eq!(creation, CreateState::Pending);
    let mut tasks = NativeTaskHost::new(quota, instance.engine_limits().clone()).unwrap();
    let requests = instance.requests().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "tasks.yield");
    tasks
        .dispatch(&mut instance, requests.into_iter().next().unwrap())
        .unwrap();
    std::thread::sleep(Duration::from_millis(2));
    assert!(tasks.on_due(&mut instance, Instant::now()).unwrap());
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    assert!(instance.requests().unwrap().is_empty());
    tasks.revoke();
    instance.retire_helper().unwrap();
    assert!(instance.is_physically_retired());
}

#[test]
fn fire_and_forget_yield_after_ready_cannot_certify_or_dispatch() {
    let source = r#"export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{},replay:{seed:3,duration_seconds:1,seamless:false}}}
        export async function create(host){void host.tasks.yield();return {render(){},dispose(){}};}"#;
    let quota = quota();
    let (mut instance, creation) = accepted(source, 402, quota.clone(), AnimationMode::PreRendered);
    assert_eq!(creation, CreateState::Ready);
    let requests = instance.requests().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "tasks.yield");
    let mut tasks = NativeTaskHost::new(quota, instance.engine_limits().clone()).unwrap();
    assert!(tasks
        .dispatch(&mut instance, requests.into_iter().next().unwrap())
        .is_err());
    assert!(instance.certify_procedural_replay().is_err());
    tasks.revoke();
    instance.retire_helper().unwrap();
}

#[test]
fn live_poll_has_one_native_next_and_close_requires_its_ack() {
    let source = r#"export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{}}}
        export async function create(host){
          let handle;
          const opened=await host.tasks.poll({interval_ms:16,deadline_ms:1000},async()=>{handle.close()});
          if(!opened.ok)throw Error(opened.error.code);
          handle=opened.value;
          return {render(){},dispose(){}};
        }"#;
    let quota = quota();
    let (mut instance, creation) = accepted(source, 403, quota.clone(), AnimationMode::Live);
    assert_eq!(creation, CreateState::Pending);
    let mut tasks = NativeTaskHost::new(quota, instance.engine_limits().clone()).unwrap();
    let requests = instance.requests().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "tasks.poll.open");
    tasks
        .dispatch(&mut instance, requests.into_iter().next().unwrap())
        .unwrap();
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    let requests = instance.requests().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "tasks.poll.next");
    tasks
        .dispatch(&mut instance, requests.into_iter().next().unwrap())
        .unwrap();
    let before_tick = tasks
        .next_due()
        .unwrap()
        .checked_sub(Duration::from_millis(1))
        .unwrap();
    assert!(!tasks.on_due(&mut instance, before_tick).unwrap());
    std::thread::sleep(Duration::from_millis(18));
    assert!(tasks.on_due(&mut instance, Instant::now()).unwrap());
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    let requests = instance.requests().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "tasks.poll.close");
    tasks
        .dispatch(&mut instance, requests.into_iter().next().unwrap())
        .unwrap();
    assert!(tasks.next_due().is_some());
    assert!(tasks.on_due(&mut instance, Instant::now()).unwrap());
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    assert!(instance.requests().unwrap().is_empty());
    assert!(tasks.next_due().is_none());
    assert_eq!(
        tasks.snapshots(&mut instance).unwrap().metadata(),
        &json!([])
    );
    tasks.revoke();
    instance.retire_helper().unwrap();
    assert!(instance.is_physically_retired());
}

#[test]
fn original_revoke_withholds_a_queued_yield_before_result_copy() {
    let source = r#"export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{},replay:{seed:3,duration_seconds:1,seamless:false}}}
        export async function create(host){await host.tasks.yield();return {render(){},dispose(){}};}"#;
    let quota = quota();
    let (mut instance, creation) = accepted(source, 404, quota.clone(), AnimationMode::PreRendered);
    assert_eq!(creation, CreateState::Pending);
    let mut tasks = NativeTaskHost::new(quota.clone(), instance.engine_limits().clone()).unwrap();
    let requests = instance.requests().unwrap();
    assert_eq!(requests.len(), 1);
    tasks
        .dispatch(&mut instance, requests.into_iter().next().unwrap())
        .unwrap();
    let stopped = instance.stop();
    assert!(stopped.authority_error.is_none());
    assert!(stopped.cancellation.is_ok());
    let after_stop = quota.snapshot().worker_bytes;
    std::thread::sleep(Duration::from_millis(2));
    assert!(tasks.on_due(&mut instance, Instant::now()).is_err());
    assert_eq!(quota.snapshot().worker_bytes, after_stop);
    tasks.revoke();
    assert!(tasks.is_drained());
    assert!(instance.is_physically_retired());
}

#[test]
fn original_activation_revoke_blocks_issue_and_later_result_copy() {
    let source = r#"export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{},replay:{seed:3,duration_seconds:1,seamless:false}}}
        export async function create(host){await host.tasks.yield();return {render(){},dispose(){}};}"#;
    let quota = quota();
    let (mut before_issue, creation) =
        accepted(source, 407, quota.clone(), AnimationMode::PreRendered);
    assert_eq!(creation, CreateState::Pending);
    let request = before_issue.requests().unwrap().pop().unwrap();
    assert!(before_issue.revoke_activation().unwrap().is_some());
    let mut tasks =
        NativeTaskHost::new(quota.clone(), before_issue.engine_limits().clone()).unwrap();
    assert!(tasks.dispatch(&mut before_issue, request).is_err());
    tasks.revoke();
    before_issue.retire_helper().unwrap();

    let (mut before_copy, creation) =
        accepted(source, 408, quota.clone(), AnimationMode::PreRendered);
    assert_eq!(creation, CreateState::Pending);
    let request = before_copy.requests().unwrap().pop().unwrap();
    let mut tasks = NativeTaskHost::new(quota, before_copy.engine_limits().clone()).unwrap();
    tasks.dispatch(&mut before_copy, request).unwrap();
    assert!(before_copy.revoke_activation().unwrap().is_some());
    std::thread::sleep(Duration::from_millis(2));
    assert!(tasks.on_due(&mut before_copy, Instant::now()).is_err());
    tasks.revoke();
    assert!(tasks.is_drained());
    before_copy.retire_helper().unwrap();
}

#[test]
fn cancellation_during_awaited_callback_keeps_one_next_and_joins_helper() {
    let source = r#"export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{}}}
        export async function create(host){
          const opened=await host.tasks.poll({interval_ms:16,deadline_ms:1000},async()=>{
            await host.tasks.yield();
          });
          if(!opened.ok)throw Error(opened.error.code);
          return {render(){},dispose(){}};
        }"#;
    let quota = quota();
    let (mut instance, creation) = accepted(source, 405, quota.clone(), AnimationMode::Live);
    assert_eq!(creation, CreateState::Pending);
    let mut tasks = NativeTaskHost::new(quota.clone(), instance.engine_limits().clone()).unwrap();
    let open = instance.requests().unwrap();
    tasks
        .dispatch(&mut instance, open.into_iter().next().unwrap())
        .unwrap();
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    let next = instance.requests().unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].method, "tasks.poll.next");
    tasks
        .dispatch(&mut instance, next.into_iter().next().unwrap())
        .unwrap();
    std::thread::sleep(Duration::from_millis(18));
    assert!(tasks.on_due(&mut instance, Instant::now()).unwrap());
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    let callback_yield = instance.requests().unwrap();
    assert_eq!(callback_yield.len(), 1);
    assert_eq!(callback_yield[0].method, "tasks.yield");
    // The awaited callback has not issued another next while its yield waits.
    tasks
        .dispatch(&mut instance, callback_yield.into_iter().next().unwrap())
        .unwrap();
    assert!(instance.requests().unwrap().is_empty());
    let stopped = instance.stop();
    assert!(stopped.authority_error.is_none());
    assert!(stopped.cancellation.is_ok());
    tasks.revoke();
    assert!(tasks.is_drained());
    assert!(instance.is_physically_retired());
}

#[test]
fn live_poll_deadline_terminal_ack_suppresses_due_callback() {
    let source = r#"export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{}}}
        export async function create(host){
          const opened=await host.tasks.poll({interval_ms:16,deadline_ms:100},async()=>{
            await host.tasks.yield();
          });
          if(!opened.ok)throw Error(opened.error.code);
          return {render(){},dispose(){}};
        }"#;
    let quota = quota();
    let (mut instance, creation) = accepted(source, 406, quota.clone(), AnimationMode::Live);
    assert_eq!(creation, CreateState::Pending);
    let mut tasks = NativeTaskHost::new(quota, instance.engine_limits().clone()).unwrap();
    let open = instance.requests().unwrap();
    tasks
        .dispatch(&mut instance, open.into_iter().next().unwrap())
        .unwrap();
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    let next = instance.requests().unwrap();
    assert_eq!(next.len(), 1);
    tasks
        .dispatch(&mut instance, next.into_iter().next().unwrap())
        .unwrap();
    std::thread::sleep(Duration::from_millis(120));
    assert!(tasks.on_due(&mut instance, Instant::now()).unwrap());
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    assert!(instance.requests().unwrap().is_empty());
    assert!(tasks.next_due().is_none());
    tasks.revoke();
    instance.retire_helper().unwrap();
}

#[test]
fn expired_awaiting_callbacks_free_live_slots_beyond_thirty_two_and_ack_late_close() {
    let source = r#"export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{}}}
        export async function create(host){
          const handles=[];
          for(let index=0;index<35;index++){
            const opened=await host.tasks.poll({interval_ms:16,deadline_ms:64},
              async()=>{await new Promise(()=>{})});
            if(!opened.ok)throw Error(opened.error.code);
            handles.push(opened.value);
            const gate=await host.tasks.yield();
            if(!gate.ok)throw Error(gate.error.code);
          }
          handles[0].close();
          return {render(){},dispose(){}};
        }"#;
    let quota = quota();
    let (mut instance, creation) = accepted(source, 409, quota.clone(), AnimationMode::Live);
    assert_eq!(creation, CreateState::Pending);
    let mut tasks = NativeTaskHost::new(quota, instance.engine_limits().clone()).unwrap();
    for index in 0..35 {
        let open = instance.requests().unwrap();
        assert_eq!(open.len(), 1, "open {index}");
        assert_eq!(open[0].method, "tasks.poll.open");
        tasks
            .dispatch(&mut instance, open.into_iter().next().unwrap())
            .unwrap();
        assert_eq!(instance.pump().unwrap(), CreateState::Pending);
        let mut queued = instance.requests().unwrap();
        assert_eq!(queued.len(), 2, "next plus delayed create gate {index}");
        let next_index = queued
            .iter()
            .position(|item| item.method == "tasks.poll.next")
            .unwrap();
        let next = queued.remove(next_index);
        let gate = queued.pop().unwrap();
        assert_eq!(gate.method, "tasks.yield");
        tasks.dispatch(&mut instance, next).unwrap();
        std::thread::sleep(Duration::from_millis(18));
        assert!(tasks.on_due(&mut instance, Instant::now()).unwrap());
        assert_eq!(instance.pump().unwrap(), CreateState::Pending);
        assert!(
            instance.requests().unwrap().is_empty(),
            "awaited callback has no next"
        );
        std::thread::sleep(Duration::from_millis(55));
        assert!(tasks.on_due(&mut instance, Instant::now()).unwrap());
        assert_eq!(instance.pump().unwrap(), CreateState::Pending);
        assert!(
            tasks.next_due().is_none(),
            "expired callback owns no live timer slot"
        );
        assert_eq!(
            tasks
                .snapshots(&mut instance)
                .unwrap()
                .metadata()
                .as_array()
                .unwrap()
                .len(),
            index + 1
        );
        tasks.dispatch(&mut instance, gate).unwrap();
        std::thread::sleep(Duration::from_millis(2));
        assert!(tasks.on_due(&mut instance, Instant::now()).unwrap());
        assert_eq!(
            instance.pump().unwrap(),
            if index == 34 {
                CreateState::Ready
            } else {
                CreateState::Pending
            }
        );
    }
    let late_close = instance.requests().unwrap();
    assert_eq!(late_close.len(), 1);
    assert_eq!(late_close[0].method, "tasks.poll.close");
    tasks
        .dispatch(&mut instance, late_close.into_iter().next().unwrap())
        .unwrap();
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    assert!(instance.requests().unwrap().is_empty());
    assert_eq!(
        tasks
            .snapshots(&mut instance)
            .unwrap()
            .metadata()
            .as_array()
            .unwrap()
            .len(),
        34
    );
    tasks.revoke();
    assert!(tasks.is_drained());
    instance.retire_helper().unwrap();
    assert!(instance.is_physically_retired());
}

#[test]
fn late_callback_settlement_gets_terminal_next_ack_after_deadline() {
    let source = r#"export function plan(){return {fps:2,output:{mode:'cells',format:'mask8',update:'replace'},inputs:{}}}
        export async function create(host){
          const opened=await host.tasks.poll({interval_ms:16,deadline_ms:64},
            async()=>{const yielded=await host.tasks.yield();
              if(!yielded.ok)throw Error(yielded.error.code)});
          if(!opened.ok)throw Error(opened.error.code);
          return {render(){},dispose(){}};
        }"#;
    let quota = quota();
    let (mut instance, creation) = accepted(source, 410, quota.clone(), AnimationMode::Live);
    assert_eq!(creation, CreateState::Pending);
    let mut tasks = NativeTaskHost::new(quota, instance.engine_limits().clone()).unwrap();
    let open = instance.requests().unwrap();
    tasks
        .dispatch(&mut instance, open.into_iter().next().unwrap())
        .unwrap();
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    let next = instance.requests().unwrap();
    assert_eq!(next.len(), 1);
    tasks
        .dispatch(&mut instance, next.into_iter().next().unwrap())
        .unwrap();
    std::thread::sleep(Duration::from_millis(18));
    assert!(tasks.on_due(&mut instance, Instant::now()).unwrap());
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    let pending_callback = instance.requests().unwrap();
    assert_eq!(pending_callback.len(), 1);
    assert_eq!(pending_callback[0].method, "tasks.yield");
    // Hold the actual request in the actor until after the native poll expires.
    std::thread::sleep(Duration::from_millis(55));
    assert!(tasks.on_due(&mut instance, Instant::now()).unwrap());
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    assert!(tasks.next_due().is_none());
    assert_eq!(
        tasks
            .snapshots(&mut instance)
            .unwrap()
            .metadata()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    tasks
        .dispatch(&mut instance, pending_callback.into_iter().next().unwrap())
        .unwrap();
    std::thread::sleep(Duration::from_millis(2));
    assert!(tasks.on_due(&mut instance, Instant::now()).unwrap());
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    let late_next = instance.requests().unwrap();
    assert_eq!(late_next.len(), 1);
    assert_eq!(late_next[0].method, "tasks.poll.next");
    tasks
        .dispatch(&mut instance, late_next.into_iter().next().unwrap())
        .unwrap();
    assert_eq!(instance.pump().unwrap(), CreateState::Ready);
    assert!(instance.requests().unwrap().is_empty());
    assert_eq!(
        tasks.snapshots(&mut instance).unwrap().metadata(),
        &json!([])
    );
    tasks.revoke();
    instance.retire_helper().unwrap();
    assert!(instance.is_physically_retired());
}
