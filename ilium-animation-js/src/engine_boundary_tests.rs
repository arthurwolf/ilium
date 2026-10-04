use super::*; // Test the actual private native boundary rather than a JavaScript simulation of it.
use std::{
    io::{Cursor, Write},
    rc::Weak,
    sync::MutexGuard,
}; // Keep V8 global handles owner-thread local and share one Rust-harness platform root.
pub(crate) fn fixture_lock() -> (MutexGuard<'static, ()>, QuotaGroup) {
    super::inventory_contracts::fixture_lock()
}
#[derive(Clone)] // Only a weak observer is stored inside V8, so global handles drop before their isolate on unwind.
struct BufferObserver(Weak<RefCell<Vec<v8::Global<v8::ArrayBuffer>>>>); // Test-only observation does not expose sealed planes to package JavaScript.
pub(super) fn observe_buffer(scope: &mut v8::PinScope, buffer: v8::Local<v8::ArrayBuffer>) {
    // Called only after the production retain_buffer deduplication check.
    if scope
        .get_slot::<Rc<RefCell<Bridge>>>()
        .is_some_and(|bridge| bridge.borrow().phase == Phase::Seed)
    {
        return;
    } // Observe frame discovery only; seed ownership has its independent native detachment inventory.
    let Some(observer) = scope.get_slot::<BufferObserver>().cloned() else {
        return;
    }; // Normal production and other tests have no observer slot.
    let Some(records) = observer.0.upgrade() else {
        return;
    }; // A dropped test observer cannot prolong native buffer custody.
    assert!(records.borrow().len() < 48);
    records.borrow_mut().push(v8::Global::new(scope, buffer)); // Retain at most the original inventory limit for direct native post-failure inspection.
} // No JavaScript callback or user-visible V8 hook runs from this instrumentation.
fn observe(engine: &mut Engine) -> Rc<RefCell<Vec<v8::Global<v8::ArrayBuffer>>>> {
    // Declare the returned owner after the Engine so it drops first on assertion failure.
    let records = Rc::new(RefCell::new(Vec::new()));
    engine
        .isolate
        .as_mut()
        .unwrap()
        .set_slot(BufferObserver(Rc::downgrade(&records)));
    records // Original allocator custody remains associated with every retained backing store.
} // The isolate stores no strong reference to these test-owned globals.
fn detached(
    engine: &mut Engine,
    records: &Rc<RefCell<Vec<v8::Global<v8::ArrayBuffer>>>>,
) -> Vec<bool> {
    // Inspect genuine V8 buffer state even after public engine retirement.
    let isolate = engine.isolate.as_mut().unwrap();
    v8::scope!(let scope, isolate); // Opening native handles does not execute JavaScript or revive the retired engine.
    records
        .borrow()
        .iter()
        .map(|record| v8::Local::new(scope, record).was_detached())
        .collect() // Read only the intrinsic native detached bit.
} // This is a test-only observation, not a production API for reentering retired V8.
fn package(source: &str) -> Arc<Package> {
    // Construct a complete tiny immutable package under the supplied test pattern.
    use sha2::{Digest, Sha256}; // Hash the actual fixture module bytes declared in its manifest.
    let manifest = serde_json::json!({"api_version":1,"id":"engine-boundary-unit","name":"Boundary","version":"1.0.0","entry":"entry.mjs","modes":["live"],"settings":{"type":"object","properties":{}},"files":[{"path":"entry.mjs","bytes":source.len(),"sha256":format!("{:x}",Sha256::digest(source.as_bytes()))}]}); // Use the exact supplied Manifest schema; no capability, trust, or native identity is fabricated.
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new())); // Use the same actual stored-ZIP loader path as the original engine contracts.
    for (name, bytes) in [
        ("entry.mjs", source.as_bytes().to_vec()),
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
    ] {
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        zip.write_all(&bytes).unwrap();
    } // Supply complete content and inventory together.
    Arc::new(
        Package::from_bytes(
            &zip.finish().unwrap().into_inner(),
            crate::package::PackageLimits::default(),
        )
        .unwrap(),
    ) // Validate the exact archive before native engine construction.
} // End complete fixture package.
fn authority() -> ServiceAuthority {
    ServiceAuthority {
        instance_id: 1,
        plan_generation: 2,
        authorization_epoch: 3,
    }
} // Native engine-only fixture stamp; broker authority is tested separately with actual channels.
pub(crate) fn engine(
    source: &str,
    bootstrap: &str,
    limits: EngineLimits,
    quota: QuotaGroup,
) -> Engine {
    // Bind native engine authority explicitly in every acquiring fixture.
    let package = package(source);
    let digest = package.digest().to_owned();
    let mut engine = Engine::new(package, limits, quota).unwrap(); // Preserve the supplied root rather than creating a fallback.
    engine.install_bootstrap(bootstrap).unwrap();
    engine.load().unwrap();
    engine.bind_service_authority(&digest, authority()).unwrap();
    engine // Script-readable accepted-plan JSON remains irrelevant.
} // No fixture dispatcher stands in for real native services here.
pub(crate) const SIMPLE: &str = r#"globalThis.__ilium_host={}; // A raw engine fixture does not fabricate a service facade.
__ilium_make_frame=()=>{globalThis.data=new Float32Array(4);return {gray:data,present(){}}}; // Legacy facade supplies one actual native-allocated buffer.
__ilium_finish_frame=()=>({metadata:{phase:__ilium_service_phase()},planes:{gray:data}}); // Preserve the real finish hook and output type validation.
__ilium_accept_frame=()=>{};"#; // End complete legacy bootstrap.
fn gray_spec() -> [ArraySpec; 1] {
    [ArraySpec {
        name: "gray".into(),
        kind: TypedArrayKind::F32,
        elements: 4,
    }]
} // Keep the original four-sample fixture shape.
#[test] // Getter discovery would throw, while private inventory must succeed without invoking it.
fn boundary_inventory_is_private_getter_free_deduplicated_and_phase_is_restored() {
    // Observe actual intrinsic buffer identities and lifecycle phases.
    let (_serial, quota) = fixture_lock(); // Serialize the shared V8 platform and root-accounting fixture.
    let bootstrap = format!("{SIMPLE}\n__ilium_make_frame=()=>{{globalThis.data=new Float32Array(4);return {{get gray(){{throw Error('DRAWING_GETTER_EXECUTED')}},present(){{}}}}}};\n__ilium_frame_buffers=()=>{{if(__ilium_service_phase()!==4)throw Error('inventory_phase');return {{first:data,second:new Uint8Array(data.buffer)}}}};"); // The private inventory includes two views of one real backing buffer.
    let mut engine = engine("export async function create(){return {render(c,f){globalThis.render_phase=__ilium_service_phase();f.present()},dispose(){}}}", &bootstrap, EngineLimits::default(), quota);
    let records = observe(&mut engine); // Test globals drop before the isolate even on failure.
    assert_eq!(
        engine
            .start_create(&serde_json::json!({}), &serde_json::json!({}))
            .unwrap(),
        CreateState::Ready
    ); // No service operation is required to create this rendering fixture.
    let output = engine.render(&serde_json::json!({}), &gray_spec()).unwrap(); // Public drawing getters must remain untouched by inventory discovery.
    assert_eq!(records.borrow().len(), 1);
    assert_eq!(detached(&mut engine, &records), vec![true]); // Distinct-view aliases must deduplicate to the same actually detached native buffer.
    assert_eq!(output.metadata["phase"], serde_json::json!(5));
    assert_eq!(
        engine.evaluate_json("render_phase").unwrap(),
        serde_json::json!(0)
    ); // Native finish is phase 5, while package render cannot inherit phase 4 or 5.
    assert_eq!(engine.bridge.borrow().phase, Phase::Idle);
    engine.accept_frame(true).unwrap(); // Native state is restored after successful handoff without another acquiring checkpoint.
} // This test is RED with legacy drawing-property discovery or an unclosed inventory phase.
#[test] // Present malformed inventory must fail without executing a public drawing getter as fallback.
fn boundary_bad_inventory_never_falls_back_and_partial_inventory_detaches() {
    // Cover wrong values, hidden accessors, symbols, and whole-inventory overflow.
    let (_serial, quota) = fixture_lock(); // Use only the existing original-root fixture.
    for (inventory, reason, retained) in [("({first:data,bad:null})", "frame inventory view type", 1), ("Object.defineProperty({first:data},'bad',{get(){throw Error('PRIVATE_GETTER_EXECUTED')}})", "service accessor properties", 1), ("({[Symbol('bad')]:data})", "frame inventory symbol key", 0), ("Object.fromEntries(Array.from({length:49},(_,index)=>['p'+index,data]))", "frame inventory plane count", 0)] { // Each counterexample is independently constructed and bounded.
        let bootstrap = format!("{SIMPLE}\n__ilium_make_frame=()=>{{globalThis.data=new Float32Array(4);return {{get gray(){{throw Error('DRAWING_GETTER_EXECUTED')}},present(){{}}}}}};\n__ilium_frame_buffers=()=>{inventory};"); // A fallback would produce a different error instead of the specific native inventory rejection.
        let mut engine = engine("export async function create(){return {render(){throw Error('RENDER_MUST_NOT_RUN')},dispose(){}}}", &bootstrap, EngineLimits::default(), quota.clone()); let records = observe(&mut engine); // Keep actual collected buffers visible to native test code after failure.
        assert_eq!(engine.start_create(&serde_json::json!({}), &serde_json::json!({})).unwrap(), CreateState::Ready); // The fixture fails only at the intended inventory boundary.
        let error = engine.render(&serde_json::json!({}), &gray_spec()).unwrap_err().to_string(); assert!(error.contains(reason), "{error}"); // Never accept fallback, render execution, or a misleading generic getter error.
        assert_eq!(records.borrow().len(), retained); assert!(detached(&mut engine, &records).iter().all(|value| *value)); // Buffers discovered before a later malformed entry are detached by the actual finally path.
        assert!(engine.is_invalid()); assert_eq!(engine.bridge.borrow().phase, Phase::Idle); // Failure closes the native inventory phase instead of leaving a callable private-data window.
    } // Each failed isolate is physically owned and dropped before the next case.
} // No absent-hook fallback may be triggered by malformed present data.
#[test] // Original absence is immutable and only that absence selects the legacy path.
fn boundary_optional_inventory_absence_is_sealed_before_package_evaluation() {
    // A guest cannot install a fake empty inventory to evade cleanup.
    let (_serial, quota) = fixture_lock();
    let mut engine = engine("globalThis.replaced=false;try{Object.defineProperty(globalThis,'__ilium_frame_buffers',{value:()=>({})});replaced=true}catch{} export async function create(){return {render(c,f){f.present()},dispose(){}}}", SIMPLE, EngineLimits::default(), quota);
    let records = observe(&mut engine); // The package attempts substitution before creation.
    assert_eq!(engine.evaluate_json("(()=>{const d=Object.getOwnPropertyDescriptor(globalThis,'__ilium_frame_buffers');return [replaced,typeof d.value,d.writable,d.configurable,d.enumerable]})()").unwrap(), serde_json::json!([false,"undefined",false,false,false])); // Sealed undefined is a concrete original absence, not permission for later package registration.
    assert_eq!(
        engine
            .start_create(&serde_json::json!({}), &serde_json::json!({}))
            .unwrap(),
        CreateState::Ready
    );
    engine.render(&serde_json::json!({}), &gray_spec()).unwrap(); // The explicit legacy fixture remains supported.
    assert_eq!(records.borrow().len(), 1);
    assert_eq!(detached(&mut engine, &records), vec![true]); // Legacy support must still detach the genuine buffer.
} // Production bootstrap uses its sealed trusted hook instead of this fallback.
#[test] // Native observation distinguishes actual detachment from merely dropping the isolate later.
fn boundary_production_render_throw_detaches_all_private_working_and_sealed_buffers() {
    // Use the complete production bootstrap, not a fake buffer-length field.
    let (_serial, quota) = fixture_lock();
    let mut engine = engine("export async function create(){return {render(c,f){f.gray[0]=0.5;f.present();throw Error('expected_render_failure')},dispose(){}}}", crate::TRUSTED_BOOTSTRAP, EngineLimits::default(), quota);
    let records = observe(&mut engine); // The observer reads only native buffer bits after failure.
    assert_eq!(
        engine
            .start_create(&serde_json::json!({}), &serde_json::json!({}))
            .unwrap(),
        CreateState::Ready
    ); // Fail only after a real frame has been made and inventoried.
    let key =
        serde_json::json!({"instance_id":"1","revision":"1","base_version":"0","sequence":"1"});
    let shape = serde_json::json!({"cell_width":1,"cell_height":1,"mode":"pixels","format":"gray32","update":"replace","cell_rgb":false,"colour_space":"srgb"}); // Preserve the exact native surface schema and string identities.
    engine.seed_frame(&serde_json::json!({"frame":{"key":key,"shape":shape,"reset":true,"invalid_rects":[],"input_specs":[]}}), &[ArraySpec { name:"work_data".into(),kind:TypedArrayKind::F32,elements:8 }], &BTreeMap::from([("work_data".into(),vec![0;32])])).unwrap(); // Seed one native-selected original-root working buffer before render.
    let arrays = [
        ("work_data", TypedArrayKind::F32),
        ("data", TypedArrayKind::F32),
        ("work_touch", TypedArrayKind::U8),
        ("touch", TypedArrayKind::U8),
        ("work_order", TypedArrayKind::U32),
        ("order", TypedArrayKind::U32),
    ]
    .into_iter()
    .map(|(name, kind)| ArraySpec {
        name: name.into(),
        kind,
        elements: 8,
    })
    .collect::<Vec<_>>(); // Every private plane must be retained before the throwing callback.
    assert!(engine
        .render(
            &serde_json::json!({"_ilium_frame":{"key":key,"shape":shape},"inputs":{}}),
            &arrays
        )
        .is_err());
    assert!(engine.is_invalid()); // The submitted frame cannot be published after a package exception.
    assert_eq!(records.borrow().len(), 6);
    assert_eq!(detached(&mut engine, &records), vec![true; 6]); // Force physical detachment of every distinct private working/sealed allocation before Engine destruction.
    assert_eq!(engine.bridge.borrow().phase, Phase::Idle); // Neither private inventory nor private finish capability survives the failure.
} // Existing surface tests separately force no canonical/pixel/provenance commit.
#[test] // A missed deadline must cancel only its own resolver while escaped immutable input remains admitted.
fn boundary_deadline_and_cancel_aliases_retain_original_quota_and_do_not_execute_reactions() {
    // Advance a private fixture deadline deterministically instead of racing wall-clock sleeps.
    let (_serial, quota) = fixture_lock();
    let baseline = quota.snapshot();
    let limits = EngineLimits {
        pending_requests: 1,
        ..EngineLimits::default()
    }; // Tighten one fixture dimension without raising any quota.
    let mut engine = engine("export async function create(){globalThis.answer=await __ilium_dispatch('fixture.bytes',{data:new Uint8Array([1,2,3])});return {render(){},dispose(){}}}", SIMPLE, limits.clone(), quota.clone()); // Dispatch actual typed input through the native callback.
    assert_eq!(
        engine
            .start_create(&serde_json::json!({}), &serde_json::json!({}))
            .unwrap(),
        CreateState::Pending
    );
    let requests = engine.take_requests().unwrap();
    let id = requests[0].id;
    drop(requests); // Leave only the native pending request owner before setting its test deadline.
    let alias = {
        let mut bridge = engine.bridge.borrow_mut();
        let pending = bridge.pending.get_mut(&id).unwrap();
        Arc::get_mut(&mut pending.request.inner).unwrap().deadline =
            Instant::now() - Duration::from_secs(1);
        pending.request.clone()
    }; // Mutate only this private test fixture's monotonic deadline, never production timeouts.
    let result =
        ServiceValue::copy_from_host(&Value::Null, &[], &BTreeMap::new(), &limits, quota.clone())
            .unwrap(); // Real completion source admission is independently held through the terminal decision.
    assert_eq!(
        engine
            .complete_service_request(id, authority(), result)
            .unwrap(),
        CompletionState::TimedOut
    );
    assert!(engine.bridge.borrow().service_reactions_pending); // Timeout settlement queues a result but never runs JavaScript.
    let cancelled = engine.take_cancelled_requests();
    assert_eq!(cancelled.len(), 1);
    assert!(alias.is_cancelled());
    assert_eq!(engine.service_usage().0, 1); // Terminal inventory and escaped aliases still retain the same physical request lease.
    assert_eq!(alias.payload.planes()["b0"], [1, 2, 3]);
    assert_eq!(engine.pump().unwrap(), CreateState::Ready); // Only an explicit authorized pump observes the complete timeout result.
    assert_eq!(
        engine.evaluate_json("answer.error.code").unwrap(),
        serde_json::json!("timeout")
    );
    engine.cancel();
    drop(engine); // Logical retirement cannot erase surviving input custody.
    drop(cancelled);
    assert!(quota.snapshot().worker_bytes > baseline.worker_bytes);
    assert_eq!(alias.payload.planes()["b0"], [1, 2, 3]); // The immutable allocation survives the engine and cancellation inventory.
    drop(alias);
    assert_eq!(quota.snapshot(), baseline); // Only the actual last request owner releases its original-root storage.
} // Native job execution and its own receipt remain separate from this engine input test.
#[test] // Getter, proxy, prototype and malformed-view cases must not invoke user conversion hooks.
fn boundary_native_raw_traversal_rejects_malformed_data_before_queue_publication() {
    // These are native V8 rejection tests, not a facade's shallow validation.
    let (_serial, quota) = fixture_lock();
    let source = r#"export async function create(){ // Complete adversarial package setup.
        globalThis.hooks=0; const bad=[()=>new Proxy({},{ownKeys(){hooks++;throw Error('proxy')}}),()=>Object.defineProperty({},'x',{get(){hooks++;return 1}}),()=>Object.assign(Object.create({foreign:true}),{x:1}),()=>new Uint8ClampedArray(1),()=>new DataView(new ArrayBuffer(4)),()=>new ArrayBuffer(4),()=>{const a=[];a[1]=1;return a},()=>({[Symbol('x')]:1}),()=>({$ilium_binary:'b0'}),()=>{const x={};x.self=x;return x},()=>new Float32Array(new ArrayBuffer(16,{maxByteLength:32})),()=>Object.defineProperty(new Uint8Array(1),'extra',{value:1}),()=>({x:NaN}),()=>Array.from({length:49},()=>new Uint8Array(1))]; // Construct each malformed object without dispatching a native service.
        globalThis.rejections=[]; for(const make of bad){const result=await __ilium_dispatch('fixture.invalid',make());rejections.push(result.ok===false && typeof result.error.message==='string')} // Structured rejection must preserve engine usability and never create pending work.
        return {render(){},dispose(){}}; // End the adversarial fixture without any actual native effects.
    }"#; // Every fixture has bounded size and a closed explicit failure oracle.
    let mut engine = engine(source, SIMPLE, EngineLimits::default(), quota);
    assert_eq!(
        engine
            .start_create(&serde_json::json!({}), &serde_json::json!({}))
            .unwrap(),
        CreateState::Ready
    ); // Immediate structured rejections may settle during the authorized creation checkpoint.
    assert_eq!(
        engine
            .evaluate_json("[hooks,rejections.length,rejections.every(Boolean)]")
            .unwrap(),
        serde_json::json!([0, 14, true])
    ); // Proxies and accessors are rejected before any user hook or numeric conversion.
    assert!(engine.take_requests().unwrap().is_empty());
    assert_eq!(engine.service_usage(), (0, 0));
    assert!(!engine.is_invalid()); // A malformed individual request does not consume quota or poison unrelated work.
} // Known typed-array kinds and borrowed subviews are covered independently by positive binary tests.
fn own_number(engine: &mut Engine, name: &str) -> f64 {
    // Read a known primitive test counter natively without running an evaluation or microtask checkpoint.
    let isolate = engine.isolate.as_mut().unwrap();
    v8::scope!(let scope,isolate);
    let context = v8::Local::new(scope, engine.context.as_ref().unwrap());
    let scope = &mut v8::ContextScope::new(scope, context); // Reopen only the owner-thread context and already existing global.
    let key = v8::String::new(scope, name).unwrap();
    let global = context.global(scope);
    let value = wire_own_value(scope, global, key.into()).unwrap();
    assert!(value.is_number());
    value.number_value(scope).unwrap() // Own data descriptors cannot invoke an inherited getter or package conversion hook.
} // This helper is test-only; public evaluate_json intentionally refuses pending native completion reactions.
#[test] // Root typed values must defeat inherited thenable getters during protected Promise resolution.
fn boundary_completion_is_inert_even_with_poisoned_then_and_runs_only_on_later_pump() {
    // Native result copying and JS continuation execution are distinct boundaries.
    let (_serial, quota) = fixture_lock();
    let mut engine=engine("export async function create(){globalThis.then_hits=0;globalThis.continuations=0;const pending=__ilium_dispatch('fixture.result',{});Object.defineProperty(Object.prototype,'then',{configurable:true,get(){then_hits++;throw Error('then_getter')}});globalThis.received=await pending;continuations++;delete Object.prototype.then;return {render(){},dispose(){}}}",SIMPLE,EngineLimits::default(),quota.clone()); // A inherited root typed-array then getter would run synchronously without the native shield.
    assert_eq!(
        engine
            .start_create(&serde_json::json!({}), &serde_json::json!({}))
            .unwrap(),
        CreateState::Pending
    );
    let request = engine.take_requests().unwrap().remove(0); // Capture an actual native resolver and immutable request stamp.
    let arrays = [ArraySpec {
        name: "b0".into(),
        kind: TypedArrayKind::F32,
        elements: 2,
    }];
    let planes = BTreeMap::from([(
        "b0".into(),
        [0.25_f32, 0.5]
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect(),
    )]); // Construct an independently admitted native float result, not a JSON array.
    let value = ServiceValue::copy_from_host(
        &serde_json::json!({"$ilium_binary":"b0"}),
        &arrays,
        &planes,
        &EngineLimits::default(),
        quota,
    )
    .unwrap(); // The guarded source overlaps with the newly allocated V8 destination during completion.
    assert_eq!(
        engine
            .complete_service_request(request.id, authority(), value)
            .unwrap(),
        CompletionState::Delivered
    );
    assert_eq!(own_number(&mut engine, "then_hits"), 0.0);
    assert_eq!(own_number(&mut engine, "continuations"), 0.0); // Neither inherited property code nor the awaiting continuation may run inside delivery.
    assert!(engine.bridge.borrow().service_reactions_pending);
    assert!(engine.evaluate_json("continuations").is_err()); // An unrelated evaluation cannot sneak in the pending microtask checkpoint.
    assert_eq!(engine.pump().unwrap(), CreateState::Ready);
    assert_eq!(engine.evaluate_json("[then_hits,continuations,received instanceof Float32Array,Array.from(received),Object.hasOwn(received,'then')]").unwrap(),serde_json::json!([0,1,true,[0.25,0.5],false]));
    // The temporary shield is removed before the real result becomes visible to package code.
} // The returned buffer can later be used as a normal strict binary input without a synthetic own then property.
