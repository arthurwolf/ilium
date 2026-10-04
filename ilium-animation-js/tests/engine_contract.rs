#![cfg(feature = "v8-runtime")]
use ilium_animation_js::{
    engine::{
        initialize_engine_for_tests, // Preserve the pinned engine initializer and binary inventory.
        ArraySpec,
        CompletionState,
        CreateState,
        Engine,
        EngineLimits,
        ServiceAuthority,
        ServicePhase, // Exercise actual native boundary types.
        ServiceValue,
        TypedArrayKind,
    },
    manifest::AnimationMode,
    package::{Package, PackageLimits},
};
use ilium_execution::{QuotaGroup, QuotaLimits};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap, // Retain native completion planes with their explicit type inventory.
    io::{Cursor, Write},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

// All instances share one process bootstrap ledger; serialize lifecycle snapshots.
fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn quota() -> QuotaGroup {
    static QUOTA: OnceLock<QuotaGroup> = OnceLock::new();
    let group = QUOTA
        .get_or_init(|| {
            QuotaGroup::new(QuotaLimits {
                clients: 32,
                jobs: 32,
                service_jobs: 32,
                input_bytes: 32 * 1024 * 1024,
                result_bytes: 32 * 1024 * 1024,
                worker_threads: 32,
                worker_bytes: 2048 * 1024 * 1024,
            })
        })
        .clone();
    initialize_engine_for_tests(group.clone(), 1).unwrap();
    group
}
fn package(source: &str, extra: &[(&str, &str)]) -> Arc<Package> {
    let mut files = vec![("entry.mjs", source)];
    files.extend_from_slice(extra);
    let inventory:Vec<_>=files.iter().map(|(path,source)|json!({"path":path,"bytes":source.len(),"sha256":format!("{:x}",Sha256::digest(source.as_bytes()))})).collect();
    let manifest = json!({"api_version":1,"id":"engine-contract","name":"Contract","version":"1.0.0","entry":"entry.mjs","modes":["live"],"settings":{"type":"object","properties":{}},"files":inventory});
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, source) in files {
        zip.start_file(
            path,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        zip.write_all(source.as_bytes()).unwrap();
    }
    zip.start_file(
        "manifest.json",
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    Arc::new(
        Package::from_bytes(
            &zip.finish().unwrap().into_inner(),
            PackageLimits::default(),
        )
        .unwrap(),
    )
}
const BOOTSTRAP: &str = r#"
globalThis.__ilium_host=Object.freeze({http:{request:(payload)=>__ilium_dispatch('http.request',payload)}});
globalThis.__ilium_make_frame=()=>{const pixels=new Float32Array(4);globalThis.retained=pixels;return {gray:pixels,present(){this.submitted=true;}};};
globalThis.__ilium_finish_frame=frame=>({metadata:{submitted:!!frame.submitted},planes:{gray:frame.gray}});
globalThis.__ilium_accept_frame=accepted=>{globalThis.accepted=accepted;};
globalThis.__ilium_take_status=()=>({records:[],dropped:0,reactions:globalThis.reactions||0,then_calls:globalThis.then_calls||0}); // Inspect already stored counters without a checkpoint or service acquisition.
"#;
fn authority() -> ServiceAuthority {
    // Native test coordinates are transport fixtures, never broker grants.
    ServiceAuthority {
        instance_id: 7,
        plan_generation: 11,
        authorization_epoch: 13,
    } // Keep every captured coordinate nonzero and independently testable.
} // End explicit fixture activation coordinates.
fn bound_engine(source: &str, limits: EngineLimits) -> Engine {
    // Use the same original root for platform, isolate, and every escaping service copy.
    let package = package(source, &[]); // Validate the complete immutable package before native activation.
    let digest = package.digest().to_owned(); // Bind the actual package identity, never accepted-plan JSON.
    let mut engine = Engine::new(package, limits, quota()).unwrap(); // Preserve the caller's actual configured service limits.
    engine.install_bootstrap(BOOTSTRAP).unwrap(); // Use a deliberately small transport fixture, not a synthetic native service implementation.
    engine.bind_service_authority(&digest, authority()).unwrap(); // Bind through the real native entrypoint before any create/pump operation.
    engine.load().unwrap(); // Evaluate only the supplied immutable package source.
    engine // Return the original owner-thread isolate.
} // End the explicitly bound test constructor.
fn engine(source: &str) -> Engine {
    // Existing tests inherit the mandatory native activation prerequisite.
    bound_engine(source, EngineLimits::default()) // Keep all default ceilings unchanged.
} // End the existing fixture adapter.
fn completion(metadata: &Value, limits: &EngineLimits) -> ServiceValue {
    // Construct a real original-root immutable completion for JSON-only terminal fixtures.
    ServiceValue::copy_from_host(metadata, &[], &BTreeMap::new(), limits, quota()).unwrap()
    // Admission occurs before the engine receives any result allocation.
} // No native service or permission is implied by this test data constructor.
fn gray_spec() -> [ArraySpec; 1] {
    // Reuse the exact existing four-float frame contract.
    [ArraySpec {
        name: "gray".into(),
        kind: TypedArrayKind::F32,
        elements: 4,
    }] // Frame custody remains separate from all service results.
} // End the bounded frame inventory fixture.
fn create(engine: &mut Engine) -> CreateState {
    engine.start_create(&json!({}), &json!({})).unwrap()
}
#[test]
fn verifies_real_esm_relative_imports_without_node_or_browser_globals() {
    let _serial = serial();
    let mut engine=Engine::new(package("import {value} from './modules/math.mjs'; export function plan(){return {value,os:typeof process,node:typeof require,browser:typeof fetch};}",&[("modules/math.mjs","export const value=42;")]),EngineLimits::default(),quota()).unwrap();
    engine.load().unwrap();
    assert_eq!(
        engine
            .plan(&json!({}), AnimationMode::Live, &json!({}))
            .unwrap(),
        json!({"value":42,"os":"undefined","node":"undefined","browser":"undefined"})
    );
}
#[test]
fn rejects_node_import_and_module_acquisition() {
    let _serial = serial();
    for source in [
        "import 'node:fs';",
        "__ilium_dispatch('http.request',{}); export function plan(){return {};}",
    ] {
        let mut engine =
            Engine::new(package(source, &[]), EngineLimits::default(), quota()).unwrap();
        assert!(engine.load().is_err());
    }
}
#[test]
fn async_create_resolves_only_its_owned_bounded_host_request() {
    let _serial = serial();
    let mut engine = engine(
        "export function plan(){return {fps:20};} export async function create(host){const response=await host.http.request({url:'https://example.org'});return {render(context,frame){frame.gray.fill(response.value);frame.present();},dispose(){}};}",
    );
    assert_eq!(create(&mut engine), CreateState::Pending);
    let requests = engine.take_requests().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "http.request");
    assert!(engine
        .complete_request(requests[0].id + 1, &json!({"value":0.5}))
        .is_err());
    engine
        .complete_request(requests[0].id, &json!({"value":0.5}))
        .unwrap();
    assert_eq!(engine.pump().unwrap(), CreateState::Ready);
    let rendered = engine
        .render(
            &json!({}),
            &[ArraySpec {
                name: "gray".into(),
                kind: TypedArrayKind::F32,
                elements: 4,
            }],
        )
        .unwrap();
    assert_eq!(rendered.metadata, json!({"submitted":true}));
    assert_eq!(rendered.planes["gray"], 0.5_f32.to_ne_bytes().repeat(4));
    engine.accept_frame(true).unwrap();
    assert_eq!(
        engine.evaluate_json("retained.byteLength").unwrap(),
        json!(0)
    );
}
#[test]
fn rejects_wrong_shape_then_detaches_retained_buffer() {
    let _serial = serial();
    let mut engine =
        engine("export async function create(){return {render(c,f){f.present();},dispose(){}};}");
    assert_eq!(create(&mut engine), CreateState::Ready);
    assert!(engine
        .render(
            &json!({}),
            &[ArraySpec {
                name: "gray".into(),
                kind: TypedArrayKind::F32,
                elements: 8
            }]
        )
        .is_err());
    assert!(engine.is_invalid());
}
#[test]
fn watchdog_terminates_infinite_module_and_promise_microtasks() {
    let _serial = serial();
    for source in [
        "while(true){}",
        "Promise.resolve().then(function loop(){Promise.resolve().then(loop);});",
    ] {
        let limits = EngineLimits {
            evaluation_ms: 30,
            ..EngineLimits::default()
        };
        let mut engine = Engine::new(package(source, &[]), limits, quota()).unwrap();
        let start = std::time::Instant::now();
        assert!(engine.load().is_err());
        assert!(engine.is_invalid());
        assert!(start.elapsed() < std::time::Duration::from_secs(3));
    }
}
#[test]
fn cancellation_rejects_future_dispatch_and_completion() {
    let _serial = serial();
    let mut engine = engine(
        "export async function create(host){await host.http.request({});return {render(){},dispose(){}};}",
    );
    assert_eq!(create(&mut engine), CreateState::Pending);
    let id = engine.take_requests().unwrap()[0].id;
    engine.cancel();
    assert!(engine.complete_request(id, &Value::Null).is_err());
    assert!(engine.pump().is_err());
}
#[test]
fn quotas_reject_an_unadmitted_engine_without_creating_an_independent_group() {
    let _serial = serial();
    let original = quota();
    let other = QuotaGroup::new(original.snapshot().limits);
    assert!(Engine::new(
        package("export function plan(){return {};}", &[]),
        EngineLimits::default(),
        other
    )
    .is_err());
}

#[test]
fn quota_tracks_actual_watchdog_retirement_and_retained_frame_lifetime() {
    let _serial = serial();
    let quota = quota();
    let baseline = quota.snapshot();
    let mut engine = engine(
        "export async function create(){return {render(c,f){f.gray.fill(0.25);f.present();},dispose(){}};}",
    );
    let running = quota.snapshot();
    assert_eq!(running.worker_threads, baseline.worker_threads + 1);
    assert!(running.worker_bytes > baseline.worker_bytes);
    assert_eq!(create(&mut engine), CreateState::Ready);
    let output = engine
        .render(
            &json!({}),
            &[ArraySpec {
                name: "gray".into(),
                kind: TypedArrayKind::F32,
                elements: 4,
            }],
        )
        .unwrap();
    engine.accept_frame(true).unwrap();
    engine.dispose().unwrap();
    drop(engine);
    let retained = quota.snapshot();
    assert_eq!(retained.worker_threads, baseline.worker_threads);
    assert!(retained.worker_bytes > baseline.worker_bytes);
    assert_eq!(output.planes["gray"], 0.25_f32.to_ne_bytes().repeat(4));
    drop(output);
    assert_eq!(quota.snapshot(), baseline);
}

#[test]
fn caught_backing_store_allocation_failure_still_retires_scene() {
    let _serial = serial();
    let limits = EngineLimits {
        backing_bytes: 1024,
        ..EngineLimits::default()
    };
    let mut engine = Engine::new(
        package(
            "try { new Uint8Array(2048); } catch(error) {} export function plan(){return {};}",
            &[],
        ),
        limits,
        quota(),
    )
    .unwrap();
    assert!(engine.load().is_err());
    assert!(engine.is_invalid());
    assert!(engine
        .plan(&json!({}), AnimationMode::Live, &json!({}))
        .is_err());
}

#[test]
fn acquisition_is_forbidden_during_plan_and_render_even_if_not_awaited() {
    let _serial = serial();
    let mut planning =
        engine("export function plan(){__ilium_dispatch('http.request',{});return {};}");
    assert!(planning
        .plan(&json!({}), AnimationMode::Live, &json!({}))
        .is_err());
    assert!(planning.is_invalid());
    drop(planning);
    let mut rendering = engine(
        "export async function create(){return {render(c,f){__ilium_dispatch('http.request',{});f.present();}};}",
    );
    assert_eq!(create(&mut rendering), CreateState::Ready);
    assert!(rendering
        .render(
            &json!({}),
            &[ArraySpec {
                name: "gray".into(),
                kind: TypedArrayKind::F32,
                elements: 4,
            }]
        )
        .is_err());
    assert!(rendering.is_invalid());
}

#[test]
fn request_budget_returns_structured_rejection_without_growing_queue() {
    let _serial = serial();
    let limits = EngineLimits {
        pending_requests: 1,
        ..EngineLimits::default()
    };
    let mut engine = bound_engine( // Preserve the existing count-refusal test while binding native authority explicitly.
        "export async function create(host){host.http.request({});globalThis.second=await host.http.request({});return {render(){}};}", limits, // Keep the same one-slot package behavior.
    ); // The shared constructor installs the fixture before loading the package.
    assert_eq!(
        engine.start_create(&json!({}), &json!({})).unwrap(),
        CreateState::Ready
    );
    assert!(!engine.is_invalid());
    assert_eq!(engine.take_requests().unwrap().len(), 1);
    let rejection = engine.evaluate_json("second").unwrap();
    assert_eq!(rejection["ok"], json!(false));
    assert_eq!(rejection["error"]["code"], json!("host_request_rejected"));
    engine.cancel();
    assert!(engine.take_requests().is_err());
}

#[test] // Test actual typed transport.
fn binary_subviews_snapshot_before_mutation_and_results_survive_frame_detachment() {
    // Cover four kinds and aliases.
    let _serial = serial(); // Serialize quota observations.
    let mut engine = engine(
        r#" // Call actual native dispatch.
export async function create(){ // Await separate native delivery.
    const backing=new ArrayBuffer(64), u8=new Uint8Array(backing,1,3), f32=new Float32Array(backing,8,2), u16=new Uint16Array(backing,20,2), u32=new Uint32Array(backing,24,2); // Select exact logical ranges.
    u8.set([3,5,7]); f32.set([1.25,-0.5]); u16.set([258,65535]); u32.set([305419896,4294967295]); // Set native-endian patterns.
    const pending=__ilium_dispatch('compute.submit',{planes:[u8,f32,u16,u32],alias:u8.subarray(1)}); // Test transport admission only.
    new Uint8Array(backing).fill(0); // Mutate all original bytes.
    globalThis.response=await pending; globalThis.reactions=1; // Await the explicit native pump.
    return {render(c,f){f.gray.fill(response.value[1][0]);f.present();}}; // Reuse service data during render.
} // End package fixture.
"#,
    ); // No native registry is simulated.
    assert_eq!(create(&mut engine), CreateState::Pending); // Creation awaits native completion.
    let requests = engine.take_requests().unwrap(); // Read actual immutable snapshots.
    assert_eq!(requests.len(), 1); // Aliases share one request.
    let request = &requests[0]; // Retain charged input custody.
    assert_eq!(request.authority, authority()); // Check the native activation stamp.
    assert_eq!(request.phase, ServicePhase::Create); // Require native preparation phase.
    assert_eq!(request.payload.binary_bytes(), 25); // Include the repeated alias bytes.
    let expected = [
        // Pin exact binary values.
        (TypedArrayKind::U8, 3, vec![3, 5, 7]), // Exclude neighboring backing bytes.
        (
            TypedArrayKind::F32,
            2,
            [1.25_f32.to_ne_bytes(), (-0.5_f32).to_ne_bytes()].concat(),
        ), // Preserve float32 bits.
        (
            TypedArrayKind::U16,
            2,
            [258_u16.to_ne_bytes(), u16::MAX.to_ne_bytes()].concat(),
        ), // Preserve U16 endianness.
        (
            TypedArrayKind::U32,
            2,
            [0x12345678_u32.to_ne_bytes(), u32::MAX.to_ne_bytes()].concat(),
        ), // Preserve U32 endianness.
        (TypedArrayKind::U8, 2, vec![5, 7]),    // Copy the overlapping logical range.
    ]; // Cover every copied plane.
    assert_eq!(request.payload.arrays().len(), expected.len()); // Require the complete inventory.
    for (index, (kind, elements, bytes)) in expected.iter().enumerate() {
        // Check types, shapes, and bytes.
        let spec = &request.payload.arrays()[index]; // Use canonical native names.
        assert_eq!(
            (&spec.name, spec.kind, spec.elements),
            (&format!("b{index}"), *kind, *elements)
        ); // Preserve descriptor identity.
        assert_eq!(&request.payload.planes()[&spec.name], bytes); // Assert mutation isolation.
    } // End request byte audit.
    let metadata = json!({"ok":true,"value":[{"$ilium_binary":"b0"},{"$ilium_binary":"b1"},{"$ilium_binary":"b2"},{"$ilium_binary":"b3"},{"$ilium_binary":"b4"}]}); // Reference actual guarded planes.
    let result = ServiceValue::copy_from_host(
        &metadata,
        request.payload.arrays(),
        request.payload.planes(),
        &EngineLimits::default(),
        quota(),
    )
    .unwrap(); // Admit independent result custody.
    let payload_alias = request.payload.clone(); // Retain an escaped input alias.
    let wire_bytes = request.payload.wire_bytes(); // Capture measured wire cost.
    assert_eq!(
        engine
            .complete_service_request(request.id, authority(), result)
            .unwrap(),
        CompletionState::Delivered
    ); // Copy under native Completion.
    assert_eq!(engine.take_status().unwrap()["reactions"], json!(0)); // Completion executes no callback.
    assert!(engine.evaluate_json("response").is_err()); // Block unrelated checkpoints.
    assert!(!engine.is_invalid()); // Check refusal preserves the engine.
    assert_eq!(engine.pump().unwrap(), CreateState::Ready); // Pump owns the continuation.
    assert_eq!(
        engine
            .evaluate_json("response.value.map(v=>[v.constructor.name,v.byteLength,Array.from(v)])")
            .unwrap(),
        json!([
            ["Uint8Array", 3, [3, 5, 7]],
            ["Float32Array", 8, [1.25, -0.5]],
            ["Uint16Array", 4, [258, 65535]],
            ["Uint32Array", 8, [305419896_u64, 4294967295_u64]],
            ["Uint8Array", 2, [5, 7]]
        ])
    ); // Verify actual JS views and bytes.
    drop(requests); // Release only the queue owner.
    assert_eq!(engine.service_usage(), (1, wire_bytes)); // Escaped input retains admission.
    drop(payload_alias); // Release the final input owner.
    assert_eq!(engine.service_usage(), (0, 0)); // Return both admission dimensions.
    let output = engine.render(&json!({}), &gray_spec()).unwrap(); // Render synchronously.
    assert_eq!(output.planes["gray"], 1.25_f32.to_ne_bytes().repeat(4)); // Service bytes remain usable.
    engine.accept_frame(true).unwrap(); // Acknowledge the frame.
    assert_eq!(
        engine
            .evaluate_json(
                "[retained.byteLength,response.value.map(v=>v.byteLength),response.value[0][1]]"
            )
            .unwrap(),
        json!([0, [3, 8, 4, 8, 2], 5])
    ); // Detach frames; retain service views.
} // End binary round-trip test.

#[test] // Test getter-free raw traversal.
fn malformed_raw_values_are_structured_refusals_without_property_execution() {
    // Structured refusals preserve usability.
    let _serial = serial(); // Serialize isolate admission.
    let mut engine = engine(
        r#" // Pass original adversarial objects.
export async function create(){ // Rejections require no native work.
    globalThis.traps=0; globalThis.refusals=[]; // Count forbidden side effects.
    const proxy=new Proxy({body:new Uint8Array([1])},{ownKeys(){traps++;throw Error('trap');},get(){traps++;throw Error('trap');},getPrototypeOf(){traps++;throw Error('trap');}}); // Detect proxies before traps.
    const getter=Object.defineProperty({},'body',{get(){traps++;throw Error('getter');}}); // Include hidden accessors.
    const symbol=Object.defineProperty({},Symbol('hidden'),{value:1}), indexed=Object.defineProperty([1],'0',{get(){traps++;throw Error('index');}}); // Reject symbols and indexed getters.
    const expando=Object.defineProperty(new Uint8Array(2),'hidden',{get(){traps++;throw Error('view');}}), subclass=new(class extends Uint8Array{})(2); // Reject expandos and subclasses.
    const detached=new Uint8Array(2); detached.buffer.transfer(); // Keep a genuinely detached view.
    const cycle={}; cycle.self=cycle; let deep={}; for(let i=0;i<33;i++)deep={next:deep}; // Exceed the recursion bound.
    const cases=[proxy,getter,symbol,indexed,expando,subclass,detached,new Uint8Array(new ArrayBuffer(4,{maxByteLength:8})),new Int32Array(1),new Uint8ClampedArray(1),new DataView(new ArrayBuffer(4)),new ArrayBuffer(4),Object.create({}),Object.setPrototypeOf(new Date(),null),Object.assign([1],{extra:2}),[,1],cycle,deep,Array(4097).fill(1),Array.from({length:49},()=>new Uint8Array(1)),undefined,NaN,Infinity,1n,Symbol('value'),()=>1,{'$ilium_binary':'b0'},'\ud800']; // Cover unsupported raw structures.
    for(const value of cases){const pending=__ilium_dispatch('http.request',value), receipt=Object.getOwnPropertyDescriptor(pending,'__ilium_admitted'), result=await pending;refusals.push({ok:result.ok,code:result.error.code,receipt:[receipt.value,receipt.writable,receipt.configurable,receipt.enumerable]});} // Require sealed negative receipts.
    return {render(){}}; // Refusals publish no native work.
} // End adversarial fixture.
"#,
    ); // Exercise real V8 validation.
    assert_eq!(create(&mut engine), CreateState::Ready); // Return structured service errors.
    assert!(!engine.is_invalid()); // Preserve engine usability.
    assert!(engine.take_requests().unwrap().is_empty()); // Reject before native publication.
    assert_eq!(engine.service_usage(), (0, 0)); // Leak no request admission.
    let report = engine
        .evaluate_json("({traps,refusals,shared:typeof SharedArrayBuffer})")
        .unwrap(); // Read completed observations.
    assert_eq!(report["traps"], json!(0)); // Invoke no getter or trap.
    assert_eq!(report["shared"], json!("undefined")); // Preserve the shared-buffer ban.
    assert_eq!(report["refusals"].as_array().unwrap().len(), 28); // Require every adversarial outcome.
    for refused in report["refusals"].as_array().unwrap() {
        // Check Results and receipts.
        assert_eq!(
            (
                refused["ok"].clone(),
                refused["code"].clone(),
                refused["receipt"].clone()
            ),
            (
                json!(false),
                json!("host_request_rejected"),
                json!([false, false, false, false])
            )
        ); // Failures cannot simulate success.
    } // End per-input oracles.
} // End raw validation test.

#[test] // Projections must preserve raw guards.
fn wrapper_projection_preserves_raw_proxy_and_accessor_rejection() {
    // Test native reference tables.
    let _serial = serial(); // Serialize original-root ownership.
    let mut engine = engine(
        r#" // Create no native registry entry.
export async function create(){ // Exercise admitted preparation.
    globalThis.traps=0;globalThis.refusals=[];const handle=Object.freeze({close(){throw Error('must not traverse method');}}), pair=[handle,{id:'opaque-fixture',kind:'compute'}]; // Keep local methods untraversed.
    const proxy=new Proxy(handle,{get(){traps++;throw Error('proxy');}}), getter=Object.defineProperty({},'handle',{get(){traps++;throw Error('getter');}}); // Preserve nested adversaries.
    const bad_table=Object.defineProperty([pair],'0',{get(){traps++;throw Error('table');}}); // Reject table accessors.
    for(const [payload,table] of [[{handle:proxy},[pair]],[getter,[pair]],[{handle},bad_table],[{handle},[[handle,{id:'opaque-fixture',kind:'compute',extra:true}]]],[{handle},[pair,pair]]]){const result=await __ilium_dispatch('compute.result',payload,table);refusals.push(result.error.code);} // Malformed requests remain unqueued.
    __ilium_dispatch('compute.result',{handle,input:new Float32Array([2,4])},[pair]); // Native ownership remains unproven.
    return {render(){}}; // Retain the one native request.
} // End projection fixture.
"#,
    ); // Deliver no synthetic success.
    assert_eq!(create(&mut engine), CreateState::Ready); // Require structured refusals.
    assert_eq!(
        engine.evaluate_json("[traps,refusals]").unwrap(),
        json!([
            0,
            [
                "host_request_rejected",
                "host_request_rejected",
                "host_request_rejected",
                "host_request_rejected",
                "host_request_rejected"
            ]
        ])
    ); // Execute no getter or trap.
    let requests = engine.take_requests().unwrap(); // Read the actual queue entry.
    assert_eq!(requests.len(), 1); // Publish no partial projection.
    assert_eq!(
        requests[0].payload.metadata(),
        &json!({"handle":{"id":"opaque-fixture","kind":"compute"},"input":{"$ilium_binary":"b0"}})
    ); // Carry lookup data only.
    assert_eq!(
        requests[0].payload.planes()["b0"],
        [2_f32.to_ne_bytes(), 4_f32.to_ne_bytes()].concat()
    ); // Preserve adjacent binary input.
} // Native ownership checks remain owed.

#[test] // Pin original admission limits.
fn original_pending_limits_include_completed_but_escaped_input_owners() {
    // Escaped custody outlives resolution.
    let _serial = serial(); // Serialize quota owners.
    assert_eq!(
        (
            EngineLimits::default().pending_requests,
            EngineLimits::default().pending_bytes
        ),
        (32, 4 * 1024 * 1024)
    ); // Preserve frozen defaults.
    let mut count_engine = engine("export async function create(){const pending=Array.from({length:33},()=>__ilium_dispatch('http.request',{}));globalThis.last=await pending[32];return {render(){}};}"); // Request one excessive slot.
    assert_eq!(create(&mut count_engine), CreateState::Ready); // Refuse request thirty-three.
    assert_eq!(count_engine.take_requests().unwrap().len(), 32); // Admit exactly thirty-two requests.
    assert_eq!(
        count_engine.evaluate_json("last.error.code").unwrap(),
        json!("host_request_rejected")
    ); // Return a structured limit error.
    drop(count_engine); // Retire the first isolate.
    let limits = EngineLimits {
        pending_bytes: 1600,
        ..EngineLimits::default()
    }; // Tighten aggregate bytes for testing.
    let mut engine = bound_engine("export async function create(){const first=__ilium_dispatch('http.request',{body:new Uint8Array(600)});first.then(()=>{globalThis.reactions=1;globalThis.after=__ilium_dispatch('http.request',{body:new Uint8Array(600)});});globalThis.second=await __ilium_dispatch('http.request',{body:new Uint8Array(600)});return {render(){}};}", limits.clone()); // Exceed aggregate retained bytes.
    assert_eq!(create(&mut engine), CreateState::Ready); // Keep request and create independent.
    let request = engine.take_requests().unwrap().pop().unwrap(); // Retain one native input owner.
    let payload_alias = request.payload.clone(); // Retain its payload-only alias.
    assert_eq!(engine.service_usage(), (1, request.payload.wire_bytes())); // Charge complete measured wire cost.
    assert!(request.payload.wire_bytes() <= limits.pending_bytes); // Respect the receiver's own bound.
    assert_eq!(
        engine.evaluate_json("second.error.code").unwrap(),
        json!("host_request_rejected")
    ); // Refuse aggregate-byte overflow.
    assert_eq!(
        engine
            .complete_service_request(request.id, authority(), completion(&Value::Null, &limits))
            .unwrap(),
        CompletionState::Delivered
    ); // Resolve while input aliases survive.
    assert!(engine.take_requests().unwrap().is_empty()); // Completion cannot dispatch.
    assert_eq!(engine.take_status().unwrap()["reactions"], json!(0)); // Diagnostics perform no checkpoint.
    drop(request); // Payload custody retains the lease.
    assert_eq!(engine.pump().unwrap(), CreateState::Ready); // Pump attempts another request.
    assert!(engine.take_requests().unwrap().is_empty()); // Retained bytes prevent replacement.
    assert_eq!(
        engine
            .evaluate_json("Object.getOwnPropertyDescriptor(after,'__ilium_admitted').value")
            .unwrap(),
        json!(false)
    ); // Check the negative native receipt.
    assert_eq!(engine.service_usage().0, 1); // Resolution cannot release aliases.
    drop(payload_alias); // Release the last input owner.
    assert_eq!(engine.service_usage(), (0, 0)); // Return count and bytes once.
} // End admission lifetime test.

#[test] // Retain input debit beyond Engine.
fn retained_native_request_alias_keeps_original_root_storage_after_engine_drop() {
    // Cancellation does not prove exit.
    let _serial = serial(); // Serialize original-root snapshots.
    let quota = quota();
    let before = quota.snapshot(); // Reuse the initialized platform root.
    let mut engine = engine("export async function create(){__ilium_dispatch('http.request',{body:new Uint8Array(65536)});return {render(){}};}"); // Admit measurable real input.
    assert_eq!(create(&mut engine), CreateState::Ready); // Finish create independently.
    let request = engine.take_requests().unwrap().pop().unwrap();
    let stop = request.stop_token();
    let payload = request.payload.clone(); // Separate request, stop, and payload.
    engine.cancel(); // Signal native cancellation.
    assert!(stop.is_stopped() && request.is_cancelled()); // All stop aliases observe cancellation.
    drop(engine);
    drop(request); // Retain payload beyond Engine drop.
    let retained = quota.snapshot(); // Observe remaining actual custody.
    assert_eq!(retained.worker_threads, before.worker_threads); // Retire the original watchdog.
    assert!(retained.worker_bytes >= before.worker_bytes + 65536); // Retain debit on the original root.
    assert_eq!(payload.planes()["b0"].len(), 65536); // Preserve escaped readable bytes.
    drop(payload); // Drop the final allocation owner.
    assert_eq!(quota.snapshot(), before); // Return original-root storage last.
} // End escaped-storage test.

#[test] // Check native result publication gates.
fn native_result_preflight_preserves_pending_request_and_structured_failure() {
    // Preserve pending current requests.
    let _serial = serial(); // Serialize original-root admission.
    let limits = EngineLimits {
        pending_bytes: 1024,
        ..EngineLimits::default()
    }; // Tighten the receiving limit.
    let mut engine = bound_engine("export async function create(){globalThis.response=await __ilium_dispatch('http.request',{});globalThis.reactions=1;return {render(){}};}", limits.clone()); // Retain a real native resolver.
    assert_eq!(create(&mut engine), CreateState::Pending); // Keep creation pending.
    let request = engine.take_requests().unwrap().pop().unwrap(); // Preserve original correlation.
    let stale = ServiceAuthority {
        authorization_epoch: authority().authorization_epoch + 1,
        ..authority()
    }; // Change only the native epoch.
    assert!(engine
        .complete_service_request(request.id, stale, completion(&Value::Null, &limits))
        .is_err()); // Reject stale publication.
    assert!(engine.cancel_request(request.id, stale).is_err()); // Reject stale cancellation.
    let foreign = QuotaGroup::new(quota().snapshot().limits); // Create a deliberate foreign root.
    let foreign_result =
        ServiceValue::copy_from_host(&Value::Null, &[], &BTreeMap::new(), &limits, foreign)
            .unwrap(); // Equal limits are insufficient.
    assert!(engine
        .complete_service_request(request.id, authority(), foreign_result)
        .is_err()); // Reject foreign custody.
    let arrays = [ArraySpec {
        name: "b0".into(),
        kind: TypedArrayKind::U8,
        elements: 2048,
    }];
    let planes = BTreeMap::from([("b0".into(), vec![9; 2048])]); // Prepare larger producer data.
    let result = ServiceValue::copy_from_host(
        &json!({"$ilium_binary":"b0"}),
        &arrays,
        &planes,
        &EngineLimits::default(),
        quota(),
    )
    .unwrap(); // Admit it on the original root.
    assert!(engine
        .complete_service_request(request.id, authority(), result)
        .is_err()); // Recheck actual receiver limits.
    assert!(!engine.is_invalid()); // Preserve the current resolver.
    assert_eq!(engine.take_status().unwrap()["reactions"], json!(0)); // Refusals execute no package code.
    assert_eq!(
        engine
            .complete_service_request(
                request.id + 1,
                authority(),
                completion(&Value::Null, &limits)
            )
            .unwrap(),
        CompletionState::Unknown
    ); // Unknown IDs are nonfatal.
    let error =
        json!({"ok":false,"error":{"code":"permission_denied","message":"native fixture refusal"}}); // Publish a real Result refusal.
    assert_eq!(
        engine
            .complete_service_request(request.id, authority(), completion(&error, &limits))
            .unwrap(),
        CompletionState::Delivered
    ); // Use the same completion boundary.
    assert_eq!(engine.take_status().unwrap()["reactions"], json!(0)); // Error delivery has no checkpoint.
    assert_eq!(engine.pump().unwrap(), CreateState::Ready); // Pump resumes package code.
    assert_eq!(engine.evaluate_json("response").unwrap(), error); // Preserve the complete error.
    assert_eq!(
        engine
            .complete_service_request(request.id, authority(), completion(&Value::Null, &limits))
            .unwrap(),
        CompletionState::Unknown
    ); // Reject duplicate publication.
} // Production broker checks remain owed.

#[test] // Preflight native output shapes.
fn native_completion_shape_validation_is_exact_and_overflow_checked() {
    // Reject unbacked binary references.
    let _serial = serial(); // Serialize native quota observations.
    let limits = EngineLimits::default();
    let quota = quota();
    let before = quota.snapshot(); // Preserve original root and limits.
    let spec = ArraySpec {
        name: "b0".into(),
        kind: TypedArrayKind::U32,
        elements: 1,
    };
    let planes = BTreeMap::from([("b0".into(), vec![0; 4])]); // Supply one U32 plane.
    for metadata in [
        json!({"$ilium_binary":"b1"}),
        json!([{"$ilium_binary":"b0"},{"$ilium_binary":"b0"}]),
        json!({"$ilium_binary":"b0","extra":1}),
        json!({"constructor":1}),
        Value::Null,
    ] {
        // Cover malformed reference graphs.
        assert!(ServiceValue::copy_from_host(
            &metadata,
            std::slice::from_ref(&spec),
            &planes,
            &limits,
            quota.clone()
        )
        .is_err()); // Refuse before native copying.
    } // End marker graph cases.
    let overflow = ArraySpec {
        elements: usize::MAX,
        ..spec.clone()
    }; // Force checked shape overflow.
    assert!(ServiceValue::copy_from_host(
        &json!({"$ilium_binary":"b0"}),
        &[overflow],
        &planes,
        &limits,
        quota.clone()
    )
    .is_err()); // Refuse before allocation.
    let noncanonical = ArraySpec {
        name: "other".into(),
        ..spec
    }; // Require canonical bN names.
    assert!(ServiceValue::copy_from_host(
        &json!({"$ilium_binary":"other"}),
        &[noncanonical],
        &BTreeMap::from([("other".into(), vec![0; 4])]),
        &limits,
        quota.clone()
    )
    .is_err()); // Reject arbitrary plane names.
    assert_eq!(quota.snapshot(), before); // Leave the original ledger unchanged.
} // End native shape test.

#[test] // Shield inherited thenable getters.
fn completion_shields_then_getters_and_only_pump_can_acquire_followup_work() {
    // Keep copy ACK checkpoint-free.
    let _serial = serial(); // Serialize V8 ownership.
    for typed in [false, true] {
        // Cover root arrays and views.
        let mut engine = engine("globalThis.then_calls=0;Object.defineProperty(Array.prototype,'then',{get(){globalThis.then_calls++;throw Error('array then');}});Object.defineProperty(Object.getPrototypeOf(Uint8Array.prototype),'then',{get(){globalThis.then_calls++;throw Error('view then');}});export async function create(){__ilium_dispatch('http.request',{}).then(value=>{globalThis.response=value;globalThis.reactions=1;globalThis.next=__ilium_dispatch('http.request',{});});return {render(){}};}"); // Poison inherited then accessors.
        assert_eq!(create(&mut engine), CreateState::Ready); // Finish create before Async work.
        let request = engine.take_requests().unwrap().pop().unwrap(); // Capture the first real request.
        let result = if typed {
            ServiceValue::copy_from_host(
                &json!({"$ilium_binary":"b0"}),
                &[ArraySpec {
                    name: "b0".into(),
                    kind: TypedArrayKind::U8,
                    elements: 2,
                }],
                &BTreeMap::from([("b0".into(), vec![7, 9])]),
                &EngineLimits::default(),
                quota(),
            )
            .unwrap()
        } else {
            completion(&json!([7, 9]), &EngineLimits::default())
        }; // Construct actual native result data.
        assert_eq!(
            engine
                .complete_service_request(request.id, authority(), result)
                .unwrap(),
            CompletionState::Delivered
        ); // Resolve under native shielding.
        let status = engine.take_status().unwrap(); // Inspect without a checkpoint.
        assert_eq!(
            (status["then_calls"].clone(), status["reactions"].clone()),
            (json!(0), json!(0))
        ); // Execute no getter or continuation.
        assert!(engine.take_requests().unwrap().is_empty()); // Admit no follow-up before pump.
        assert_eq!(engine.pump().unwrap(), CreateState::Ready); // Pump under native Async phase.
        let followup = engine.take_requests().unwrap(); // Read the actual follow-up request.
        assert_eq!(followup.len(), 1);
        assert_eq!(followup[0].phase, ServicePhase::Async); // Stamp acquisition as native Async.
        assert_eq!(
            engine
                .evaluate_json(
                    "[then_calls||0,Array.from(response),Object.hasOwn(response,'then')]"
                )
                .unwrap(),
            json!([0, [7, 9], false])
        ); // Remove the temporary then shield.
    } // Cover both protected root kinds.
} // End settlement isolation test.

#[test] // Cancel requests independently.
fn cancellation_aliases_retain_inputs_and_completion_runs_no_checkpoint() {
    // Notification does not prove exit.
    let _serial = serial(); // Serialize original-root owners.
    let mut engine = engine("globalThis.responses=[];export async function create(){for(let i=0;i<3;i++)__ilium_dispatch('http.request',{body:new Uint8Array([i])}).then(value=>{responses.push(value);globalThis.reactions=responses.length;});return {render(){}};}"); // Queue three real native requests.
    assert_eq!(create(&mut engine), CreateState::Ready);
    let requests = engine.take_requests().unwrap(); // Finish create independently.
    assert_eq!(requests.len(), 3); // Keep all requests admitted.
    assert_eq!(
        engine.cancel_request(requests[0].id, authority()).unwrap(),
        CompletionState::Cancelled
    ); // Cancel one exact correlation.
    requests[1].stop_token().stop();
    engine.expire_service_requests().unwrap(); // Signal a separate stop alias.
    assert!(
        requests[0].is_cancelled() && requests[1].is_cancelled() && !requests[2].is_cancelled()
    ); // Preserve unrelated request lifetime.
    let cancelled = engine.take_cancelled_requests();
    assert_eq!(cancelled.len(), 2); // Retain native terminal records.
    assert_eq!(
        [cancelled[0].id, cancelled[1].id],
        [requests[0].id, requests[1].id]
    ); // Preserve correlation and ordering.
    assert_eq!(
        engine.cancel_request(requests[0].id, authority()).unwrap(),
        CompletionState::Unknown
    ); // Duplicate cancellation is harmless.
    assert_eq!(
        engine
            .complete_service_request(
                requests[2].id,
                authority(),
                completion(
                    &json!({"ok":true,"value":"survived"}),
                    &EngineLimits::default()
                )
            )
            .unwrap(),
        CompletionState::Delivered
    ); // Deliver unrelated current work.
    assert_eq!(engine.take_status().unwrap()["reactions"], json!(0)); // Execute no terminal checkpoint.
    assert_eq!(engine.service_usage().0, 3); // Retain all actual input owners.
    assert_eq!(engine.pump().unwrap(), CreateState::Ready); // Pump the queued continuations.
    assert_eq!(
        engine
            .evaluate_json("responses.map(r=>r.ok?r.value:r.error.code)")
            .unwrap(),
        json!(["cancelled", "cancelled", "survived"])
    ); // Return structured terminal Results.
    drop(cancelled);
    drop(requests); // Release every native request owner.
    assert_eq!(engine.service_usage(), (0, 0)); // Return admission after all aliases.
} // End cancellation custody test.

#[test] // Distinguish request/create deadlines.
fn ready_instance_request_deadline_expires_without_retiring_unrelated_scene() {
    // Expire one ready-scene request.
    let _serial = serial(); // Serialize original-root ownership.
    let limits = EngineLimits {
        preparation_ms: 150,
        ..EngineLimits::default()
    }; // Tighten the fixture deadline.
    let mut engine = bound_engine("export async function create(){__ilium_dispatch('http.request',{body:new Uint8Array([1,2])}).then(value=>{globalThis.response=value;globalThis.reactions=1;});return {render(){}};}", limits.clone()); // Keep request lifetime independent.
    assert_eq!(create(&mut engine), CreateState::Ready);
    let request = engine.take_requests().unwrap().pop().unwrap();
    let payload_alias = request.payload.clone(); // Retain escaped original input.
    let remaining = request.remaining_ms();
    assert!(remaining <= limits.preparation_ms); // Queue time cannot refresh expiry.
    std::thread::sleep(std::time::Duration::from_millis(
        remaining.saturating_add(10),
    )); // Wait the bounded original deadline.
    engine.expire_service_requests().unwrap(); // Expire without a checkpoint.
    assert!(request.is_cancelled() && request.stop_token().is_stopped()); // Signal every cancellation alias.
    assert_eq!(request.remaining_ms(), 0); // Preserve expired time.
    assert!(!engine.is_invalid());
    assert_eq!(engine.take_status().unwrap()["reactions"], json!(0)); // Preserve the ready scene.
    let terminal = engine.take_cancelled_requests();
    assert_eq!(terminal.len(), 1);
    assert_eq!(terminal[0].id, request.id); // Retain exact terminal correlation.
    assert_eq!(
        engine
            .complete_service_request(request.id, authority(), completion(&Value::Null, &limits))
            .unwrap(),
        CompletionState::Unknown
    ); // Refuse post-expiry publication.
    assert_eq!(engine.pump().unwrap(), CreateState::Ready); // Pump the timeout continuation.
    assert_eq!(
        engine.evaluate_json("response.error.code").unwrap(),
        json!("timeout")
    ); // Return a structured timeout.
    drop(terminal);
    drop(request);
    assert_eq!(engine.service_usage().0, 1); // Escaped input retains admission.
    drop(payload_alias);
    assert_eq!(engine.service_usage(), (0, 0)); // Release only the final alias.
} // End deadline custody test.
