#![cfg(feature = "v8-runtime")]
//! Real V8 + complete production bootstrap + native transactional Surface.
//! Fixtures mint no permissions and invoke no external effects.
use ilium_animation_js::{
    engine::{
        initialize_engine_for_tests,
        ArraySpec,
        CompletionState,
        CreateState,
        Engine,
        EngineLimits,
        RenderOutput,
        ServiceAuthority,
        ServiceValue,
        TypedArrayKind, // Actual binary completion and activation APIs.
    },
    manifest::AnimationMode,
    package::{Package, PackageLimits},
    surface::{
        ColourSpace, Data, Format, FrameMeta, Mode, NoNativeRenderer, Outcome, Planes, Shape,
        Surface, SurfaceError, Update,
    },
};
use ilium_execution::{QuotaGroup, QuotaLimits, StorageAdmission};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};
const BOOTSTRAP: &str = include_str!("../src/bootstrap.js");
fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(|error| error.into_inner())
}
fn quota() -> QuotaGroup {
    static QUOTA: OnceLock<QuotaGroup> = OnceLock::new();
    let quota = QUOTA
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
    initialize_engine_for_tests(quota.clone(), 1).unwrap();
    quota
}
fn package(source: &str) -> Arc<Package> {
    let manifest = json!({"api_version":1,"id":"production-bootstrap-contract","name":"Contract",
        "version":"1.0.0","entry":"entry.mjs","modes":["live"],
        "settings":{"type":"object","properties":{}},
        "files":[{"path":"entry.mjs","bytes":source.len(),"sha256":format!("{:x}",Sha256::digest(source.as_bytes()))}]});
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in [
        ("entry.mjs", source.as_bytes().to_vec()),
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
    ] {
        zip.start_file(
            path,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    Arc::new(
        Package::from_bytes(
            &zip.finish().unwrap().into_inner(),
            PackageLimits::default(),
        )
        .unwrap(),
    )
}
fn native_authority() -> ServiceAuthority {
    // Native fixture coordinates confer no broker grant.
    ServiceAuthority {
        instance_id: 1,
        plan_generation: 1,
        authorization_epoch: 1,
    } // Bind this no-effect test activation.
} // Accepted-plan JSON supplies no authority.
fn activated_engine(package: Arc<Package>, bootstrap: &str) -> Engine {
    // Bind native activation before create.
    let digest = package.digest().to_owned(); // Preserve actual package identity.
    let mut engine = Engine::new(package, EngineLimits::default(), quota()).unwrap(); // Reuse the original finite root.
    engine.install_bootstrap(bootstrap).unwrap(); // Seal hooks and optional absence.
    engine.load().unwrap(); // Module evaluation remains nonacquiring.
    engine
        .bind_service_authority(&digest, native_authority())
        .unwrap(); // Bind the actual digest and native stamp.
    engine // Return the actual native owner.
} // No external effect or permission is fabricated.
fn engine(package: Arc<Package>, settings: Value) -> Engine {
    let mut engine = activated_engine(package, BOOTSTRAP); // Preserve original cases with explicit activation.
    let plan=engine.plan(&settings,AnimationMode::Live,&json!({"viewport":{"cell_width":2,"cell_height":2},"available":{"pointer":false,"audio":false,"gpu":false,"location":false}})).unwrap();
    assert_eq!(
        engine.start_create(&settings, &plan).unwrap(),
        CreateState::Ready
    );
    assert!(engine.take_requests().unwrap().is_empty());
    engine
}
fn source(render: &str) -> String {
    format!(
        "export function plan(){{return {{}};}} export async function create(){{return {{render(c,f){{{render}}},dispose(){{}}}};}}"
    )
}
fn shape(format: Format, update: Update, cell_rgb: bool) -> Shape {
    Shape {
        cell_width: 2,
        cell_height: 2,
        mode: if format == Format::Mask8 {
            Mode::Cells
        } else {
            Mode::Pixels
        },
        format,
        update,
        cell_rgb,
        colour_space: ColourSpace::Srgb,
    }
}
/// Cover every simultaneous native candidate/seed/packed/output temporary before
/// allocating this tiny fixture. Actual Engine buffers have independent admissions.
struct Owner {
    surface: Surface,
    _admission: StorageAdmission,
}
impl Owner {
    fn new(shape: Shape) -> Self {
        let layout = shape.layout().unwrap();
        let admission = quota()
            .reserve_external_storage(
                layout.canonical_bytes * 3
                    + layout.handoff_bytes * 2
                    + layout.dots * 32
                    + 1024 * 1024,
            )
            .unwrap();
        Self {
            surface: Surface::new(1, 1, shape).unwrap(),
            _admission: admission,
        }
    }
}
fn bytes(data: &Data) -> Vec<u8> {
    match data {
        Data::U8(bytes) => bytes.clone(),
        Data::F32(values) => values
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect(),
    }
}
fn spec(name: &str, kind: TypedArrayKind, elements: usize) -> ArraySpec {
    ArraySpec {
        name: name.into(),
        kind,
        elements,
    }
}
struct Input {
    path: String,
    kind: TypedArrayKind,
    elements: usize,
    bytes: Vec<u8>,
}
fn kind_name(kind: TypedArrayKind) -> &'static str {
    match kind {
        TypedArrayKind::U8 => "u8",
        TypedArrayKind::F32 => "f32",
        TypedArrayKind::U16 => "u16",
        TypedArrayKind::U32 => "u32",
    }
}
fn begin(
    engine: &mut Engine,
    owner: &mut Owner,
    sequence: u64,
    inputs: &[Input],
    extra: Value,
) -> (Value, Vec<ArraySpec>) {
    let seed = owner.surface.begin(sequence).unwrap();
    let layout = seed.shape.layout().unwrap();
    let data_kind = if seed.shape.format == Format::Gray32 {
        TypedArrayKind::F32
    } else {
        TypedArrayKind::U8
    };
    let mut seed_specs = vec![spec("work_data", data_kind, layout.elements)];
    let mut seed_planes = BTreeMap::from([("work_data".into(), bytes(&seed.data))]);
    let mut returned = vec![
        spec("work_data", data_kind, layout.elements),
        spec("data", data_kind, layout.elements),
        spec("work_touch", TypedArrayKind::U8, layout.samples),
        spec("touch", TypedArrayKind::U8, layout.samples),
        spec("work_order", TypedArrayKind::U32, layout.samples),
        spec("order", TypedArrayKind::U32, layout.samples),
    ];
    if let Some(rgb) = seed.cell_rgb {
        seed_specs.push(spec("work_cell_rgb", TypedArrayKind::U8, layout.cells * 3));
        seed_planes.insert("work_cell_rgb".into(), rgb);
        returned.extend([
            spec("work_cell_rgb", TypedArrayKind::U8, layout.cells * 3),
            spec("cell_rgb", TypedArrayKind::U8, layout.cells * 3),
            spec("work_colour_touch", TypedArrayKind::U8, layout.cells),
            spec("colour_touch", TypedArrayKind::U8, layout.cells),
            spec("work_colour_order", TypedArrayKind::U32, layout.cells),
            spec("colour_order", TypedArrayKind::U32, layout.cells),
        ]);
    }
    let input_specs: Vec<Value> = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            let name = format!("input_{index}");
            seed_specs.push(spec(&name, input.kind, input.elements));
            returned.push(spec(&name, input.kind, input.elements));
            seed_planes.insert(name, input.bytes.clone());
            json!({"path":input.path,"kind":kind_name(input.kind),"elements":input.elements})
        })
        .collect();
    engine
        .seed_frame(
            &json!({"frame":{"key":seed.key,"shape":seed.shape,"reset":seed.reset,
        "invalid_rects":seed.invalid_rects,"input_specs":input_specs}}),
            &seed_specs,
            &seed_planes,
        )
        .unwrap();
    let mut context = json!({"time":1.0,"wall":1.0,"delta":0.05,"wall_delta":0.05,"inputs":{},
        "_ilium_frame":{"key":seed.key,"shape":seed.shape}});
    for (key, value) in extra.as_object().unwrap() {
        context[key] = value.clone();
    }
    (context, returned)
}
fn u32s(bytes: &[u8]) -> Vec<u32> {
    assert_eq!(bytes.len() % 4, 0);
    bytes
        .chunks_exact(4)
        .map(|value| u32::from_ne_bytes(value.try_into().unwrap()))
        .collect()
}
fn planes(output: &RenderOutput, shape: Shape) -> Planes {
    let data = if shape.format == Format::Gray32 {
        Data::F32(
            output.planes["data"]
                .chunks_exact(4)
                .map(|value| f32::from_ne_bytes(value.try_into().unwrap()))
                .collect(),
        )
    } else {
        Data::U8(output.planes["data"].clone())
    };
    Planes {
        data,
        touch: output.planes["touch"].clone(),
        order: u32s(&output.planes["order"]),
        cell_rgb: shape.cell_rgb.then(|| output.planes["cell_rgb"].clone()),
        colour_touch: shape
            .cell_rgb
            .then(|| output.planes["colour_touch"].clone()),
        colour_order: shape.cell_rgb.then(|| u32s(&output.planes["colour_order"])),
    }
}
fn finish(
    engine: &mut Engine,
    owner: &mut Owner,
    output: &RenderOutput,
    refuse: bool,
) -> std::result::Result<Outcome, SurfaceError> {
    let meta = FrameMeta::parse(&serde_json::to_vec(&output.metadata).unwrap()).unwrap();
    let planes = planes(output, owner.surface.snapshot().shape());
    let result = owner
        .surface
        .finish_with(meta, planes, &mut NoNativeRenderer, |candidate, _| {
            if refuse {
                // Real admission refusal: the existing platform/isolate owners
                // already occupy part of the shared ledger. Reserve the entire
                // root allowance without allocating any payload.
                let quota = quota();
                assert!(quota
                    .reserve_external_storage(quota.snapshot().limits.worker_bytes)
                    .is_err());
                return Err(SurfaceError::Capacity);
            }
            let layout = candidate.shape().layout()?;
            let _staging = quota()
                .reserve_external_storage(layout.cells * 8 + layout.dots * 8 + 1024)
                .map_err(|_| SurfaceError::Capacity)?;
            let packed = candidate.pack(|value, _, _| value, |value, _, _| value > 0.0)?;
            assert_eq!(packed.masks.len(), candidate.shape().layout()?.cells);
            assert!(packed.owners.iter().all(Option::is_none));
            Ok(())
        });
    engine
        .accept_frame(result.as_ref().is_ok_and(|outcome| outcome.accepted))
        .unwrap();
    result
}
fn masks(owner: &Owner) -> Vec<u8> {
    owner
        .surface
        .snapshot()
        .pack(|value, _, _| value, |value, _, _| value > 0.0)
        .unwrap()
        .masks
}
#[test]
fn all_seven_formats_round_trip_the_real_bootstrap_and_detach_aliases() {
    let _serial = serial();
    let script = source(
        r#"
        if(c.format==='mask8') {f.cells.set_cell(0,0,{mask:1}); globalThis.alias=f.cells.masks;}
        else {const value=c.format==='rgb8'?{r:255,g:255,b:255}:c.format==='rgba8'?{r:255,g:255,b:255,a:255}:c.format==='gray8'?255:1;
              f.pixels.set_pixel(0,0,value);globalThis.alias=f.pixels.data;}
        globalThis.saved_buffer=alias.buffer; globalThis.saved_view=new Uint8Array(saved_buffer);
        f.after_accept(()=>{globalThis.accept_count=(globalThis.accept_count||0)+1;});f.present();
    "#,
    );
    for format in [
        Format::Mask8,
        Format::Mono1,
        Format::Mono8,
        Format::Gray8,
        Format::Gray32,
        Format::Rgb8,
        Format::Rgba8,
    ] {
        let mut engine = engine(package(&script), json!({}));
        let shape = shape(
            format,
            Update::Retain,
            matches!(format, Format::Mask8 | Format::Gray8),
        );
        let mut owner = Owner::new(shape);
        let format = serde_json::to_value(format).unwrap();
        let (context, arrays) = begin(&mut engine, &mut owner, 1, &[], json!({"format":format}));
        let output = engine.render(&context, &arrays).unwrap();
        assert_eq!(output.planes.len(), if shape.cell_rgb { 12 } else { 6 });
        assert_eq!(
            engine
                .evaluate_json("[alias.byteLength,saved_buffer.byteLength,saved_view.byteLength]")
                .unwrap(),
            json!([0, 0, 0])
        );
        assert!(
            finish(&mut engine, &mut owner, &output, false)
                .unwrap()
                .accepted
        );
        assert_eq!(masks(&owner), vec![1, 0, 0, 0]);
        assert_eq!(engine.evaluate_json("accept_count").unwrap(), json!(1));
        assert!(engine.take_requests().unwrap().is_empty());
    }
}
#[test]
fn present_seals_bytes_and_rejected_retain_is_reseeded_from_native_acceptance() {
    let _serial = serial();
    let script = source(
        r#"
        globalThis.alias=f.pixels.data; globalThis.baseline=Array.from(alias);
        f.pixels.set_pixel(0,0,c.value);f.after_accept(()=>{globalThis.accept_count=(globalThis.accept_count||0)+1;});
        if(c.submit){f.present();alias[0]=99;}
    "#,
    );
    let mut engine = engine(package(&script), json!({}));
    let mut owner = Owner::new(shape(Format::Gray8, Update::Retain, false));
    for (sequence, value, submit, refuse, baseline) in [
        (1, 255, true, false, 0),
        (2, 7, true, true, 255),
        (3, 8, false, false, 255),
        (4, 9, true, false, 255),
    ] {
        let (context, arrays) = begin(
            &mut engine,
            &mut owner,
            sequence,
            &[],
            json!({"value":value,"submit":submit}),
        );
        let output = engine.render(&context, &arrays).unwrap();
        assert_eq!(
            engine.evaluate_json("baseline[0]").unwrap(),
            json!(baseline)
        );
        assert_eq!(engine.evaluate_json("alias.byteLength").unwrap(), json!(0));
        if submit {
            assert_eq!(output.planes["data"][0], value);
            assert_eq!(output.planes["work_data"][0], 99);
        }
        let result = finish(&mut engine, &mut owner, &output, refuse);
        if refuse {
            assert_eq!(result, Err(SurfaceError::Capacity));
        } else {
            assert_eq!(result.unwrap().accepted, submit);
        }
        assert_eq!(owner.surface.version(), if sequence == 4 { 2 } else { 1 });
    }
    assert_eq!(engine.evaluate_json("accept_count").unwrap(), json!(2));
    assert_eq!(
        owner.surface.snapshot().data(),
        &Data::U8([vec![9], vec![0; 31]].concat())
    );
}
#[test]
fn drawn_zero_clear_defer_and_explicit_black_remain_distinct() {
    let _serial = serial();
    let script = source(
        r#"
        f.pixels.set_pixel(0,0,0);f.pixels.set_pixel(1,0,255);
        f.pixels.clear_rect({x:1,y:0,width:1,height:1});
        f.defer_rect({unit:'pixels',x:0,y:1,width:1,height:1});
        f.set_cell_rgb(0,0,{r:0,g:0,b:0});f.present();
    "#,
    );
    let mut engine = engine(package(&script), json!({}));
    let mut owner = Owner::new(shape(Format::Gray8, Update::Retain, true));
    let (context, arrays) = begin(&mut engine, &mut owner, 1, &[], json!({}));
    let output = engine.render(&context, &arrays).unwrap();
    finish(&mut engine, &mut owner, &output, false).unwrap();
    assert_eq!(&owner.surface.snapshot().states()[..5], &[2, 1, 1, 1, 0]);
    let packed = owner
        .surface
        .snapshot()
        .pack(|value, _, _| 1.0 - value, |value, _, _| value > 0.0)
        .unwrap();
    assert_eq!(packed.masks[0], 1);
    assert_eq!(packed.rgb[0], Some([0; 3]));
    assert!(!owner.surface.snapshot().invalid_rects().is_empty());
}
#[test]
fn caught_operation_error_refuses_transaction_but_detaches_all_writable_aliases() {
    let _serial = serial();
    let script = source(
        r#"
        globalThis.alias=f.pixels.data;globalThis.saved=alias.buffer;
        try{f.pixels.set_pixel(1000,0,255);}catch{}
        try{f.present();}catch{}
    "#,
    );
    let mut engine = engine(package(&script), json!({}));
    let mut owner = Owner::new(shape(Format::Gray8, Update::Retain, false));
    let (context, arrays) = begin(&mut engine, &mut owner, 1, &[], json!({}));
    let output = engine.render(&context, &arrays).unwrap();
    assert!(output.metadata["error"].is_string());
    assert_eq!(
        engine
            .evaluate_json("[alias.byteLength,saved.byteLength]")
            .unwrap(),
        json!([0, 0])
    );
    assert_eq!(
        finish(&mut engine, &mut owner, &output, false),
        Err(SurfaceError::Callback)
    );
    assert_eq!(owner.surface.version(), 0);
    assert_eq!(masks(&owner), vec![0; 4]);
}
#[test]
fn thrown_render_retires_engine_and_never_commits_submitted_pixels() {
    let _serial = serial();
    let quota = quota();
    let baseline = quota.snapshot();
    let script = source(
        "globalThis.alias=f.pixels.data;globalThis.input_alias=c.inputs.audio.waveform;globalThis.saved_buffers=[alias.buffer,input_alias.buffer];f.pixels.set_pixel(0,0,255);f.present();throw new Error('fixture');", // Retain working and input aliases across the exception.
    );
    {
        let mut engine = engine(package(&script), json!({}));
        let mut owner = Owner::new(shape(Format::Gray8, Update::Retain, false));
        let inputs = [Input {
            path: "audio.waveform".into(),
            kind: TypedArrayKind::F32,
            elements: 2,
            bytes: [0.5_f32, 0.25]
                .iter()
                .flat_map(|value| value.to_ne_bytes())
                .collect(),
        }]; // Add genuine native-selected input storage.
        let (context, arrays) = begin(&mut engine, &mut owner, 1, &inputs, json!({})); // Retain seed custody across render failure.
        assert!(engine.render(&context, &arrays).is_err());
        assert!(engine.is_invalid());
        owner.surface.abort();
        assert_eq!(owner.surface.version(), 0);
        assert_eq!(masks(&owner), vec![0; 4]);
        // A retired isolate cannot be inspected/reused; physical backing release
        // is forced by dropping its actual owner, not inferred from JS assertions.
    }
    assert_eq!(quota.snapshot().worker_threads, baseline.worker_threads);
    assert_eq!(quota.snapshot().worker_bytes, baseline.worker_bytes);
}
#[test]
fn only_declared_binary_inputs_are_injected_and_their_aliases_detach() {
    let _serial = serial();
    let script = source(
        r#"
        globalThis.input_alias=c.inputs.audio.waveform;globalThis.input_buffer=input_alias.buffer;
        globalThis.selection=[typeof c.inputs.audio.bands,typeof c.inputs.pointer,Array.from(input_alias)];
        f.pixels.set_pixel(0,0,input_alias[0]);f.present();
    "#,
    );
    let mut engine = engine(package(&script), json!({}));
    let mut owner = Owner::new(shape(Format::Gray32, Update::Retain, false));
    let inputs = [Input {
        path: "audio.waveform".into(),
        kind: TypedArrayKind::F32,
        elements: 2,
        bytes: [0.5f32, 0.25]
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect(),
    }];
    let (context, arrays) = begin(&mut engine, &mut owner, 1, &inputs, json!({}));
    let output = engine.render(&context, &arrays).unwrap();
    assert_eq!(output.planes.len(), 7);
    assert_eq!(
        engine.evaluate_json("selection").unwrap(),
        json!(["undefined", "undefined", [0.5, 0.25]])
    );
    assert_eq!(
        engine
            .evaluate_json("[input_alias.byteLength,input_buffer.byteLength]")
            .unwrap(),
        json!([0, 0])
    );
    finish(&mut engine, &mut owner, &output, false).unwrap();
    assert_eq!(masks(&owner), vec![1, 0, 0, 0]);
}
#[test]
fn full_colour_and_thirty_two_inputs_use_the_bounded_forty_four_plane_contract() {
    let _serial = serial();
    let script = source(
        r#"
        globalThis.input_alias=c.inputs.audio.sample_31;
        f.pixels.set_pixel(0,0,input_alias[0]);f.set_cell_rgb(0,0,{r:255,g:0,b:0});f.present();
    "#,
    );
    let mut engine = engine(package(&script), json!({}));
    let mut owner = Owner::new(shape(Format::Gray32, Update::Retain, true));
    let inputs: Vec<_> = (0..32)
        .map(|index| Input {
            path: format!("audio.sample_{index}"),
            kind: TypedArrayKind::F32,
            elements: 1,
            bytes: 0.5f32.to_ne_bytes().to_vec(),
        })
        .collect();
    let (context, arrays) = begin(&mut engine, &mut owner, 1, &inputs, json!({}));
    assert_eq!(arrays.len(), 44);
    let output = engine.render(&context, &arrays).unwrap();
    assert_eq!(output.planes.len(), 44);
    assert_eq!(
        engine.evaluate_json("input_alias.byteLength").unwrap(),
        json!(0)
    );
    finish(&mut engine, &mut owner, &output, false).unwrap();
    assert_eq!(
        owner
            .surface
            .snapshot()
            .pack(|value, _, _| value, |value, _, _| value > 0.0)
            .unwrap()
            .rgb[0],
        Some([255, 0, 0])
    );
}
#[test]
fn gray32_alias_and_replace_direct_writes_match_standalone_animation_contract() {
    let _serial = serial();
    let script = source(
        "if(!(f.gray instanceof Float32Array))throw new Error('gray32 alias');globalThis.alias=f.gray;f.gray.fill(0.5);f.present();",
    );
    let mut engine = engine(package(&script), json!({}));
    let mut owner = Owner::new(shape(Format::Gray32, Update::Replace, false));
    let (context, arrays) = begin(&mut engine, &mut owner, 1, &[], json!({}));
    let output = engine.render(&context, &arrays).unwrap();
    finish(&mut engine, &mut owner, &output, false).unwrap();
    assert_eq!(masks(&owner), vec![255; 4]);
    assert_eq!(engine.evaluate_json("alias.byteLength").unwrap(), json!(0));
}
#[test]
#[ignore = "Requires built sibling ilium-animations Beach and Carpet packages; parent runs this qualification explicitly."]
fn actual_beach_and_carpet_archives_render_with_production_bootstrap() {
    let _serial = serial();
    // Isolated qualification snapshots do not relocate the authored projects.
    // An explicit root still reads their actual built archives and keeps every
    // production-bootstrap rendering assertion below unchanged.
    let projects = std::env::var_os("ILIUM_ANIMATION_PROJECT_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("ilium-animations")
        });
    assert!(
        projects.is_absolute(),
        "animation project root must be absolute"
    );
    for id in ["beach", "carpet"] {
        let path = projects
            .join(id)
            .join("dist")
            .join(format!("{id}-1.0.0.iliumanim"));
        let archive = std::fs::read(&path).unwrap();
        let package = Arc::new(Package::from_bytes(&archive, PackageLimits::default()).unwrap());
        assert_eq!(package.manifest().id, id);
        let mut engine = engine(package, json!({}));
        let mut owner = Owner::new(Shape {
            cell_width: 12,
            cell_height: 6,
            ..shape(Format::Gray32, Update::Replace, false)
        });
        let (context, arrays) = begin(
            &mut engine,
            &mut owner,
            1,
            &[],
            json!({"time":3.0,"delta":0.1}),
        );
        let output = engine.render(&context, &arrays).unwrap();
        finish(&mut engine, &mut owner, &output, false).unwrap();
        let Data::F32(data) = owner.surface.snapshot().data() else {
            panic!("real gray32 scene");
        };
        assert!(data
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
        assert!(
            data.iter().any(|value| *value > 0.0),
            "{id} should emit actual scene samples"
        );
        assert!(engine.take_requests().unwrap().is_empty());
    }
}

#[test] // Inventory must not claim drawing damage.
fn production_inventory_does_not_claim_unwritten_replace_planes() {
    // Present alone must leave touch and order unchanged.
    let _serial = serial(); // Serialize the shared root.
    let mut engine = engine(package(&source("f.present();")), json!({})); // Use the complete production bootstrap.
    let mut owner = Owner::new(shape(Format::Gray8, Update::Replace, true)); // Include optional colour getter effects.
    let (context, arrays) = begin(&mut engine, &mut owner, 1, &[], json!({})); // Seed every declared working and sealed view.
    let output = engine.render(&context, &arrays).unwrap(); // Run the actual synchronous render.
    for name in [
        "work_touch",
        "touch",
        "work_order",
        "order",
        "work_colour_touch",
        "colour_touch",
        "work_colour_order",
        "colour_order",
    ] {
        // Drawing getter access would mutate these planes.
        assert!(
            output.planes[name].iter().all(|byte| *byte == 0),
            "unexpected drawing discovery mutation in {name}"
        ); // Require no inventory drawing side effects.
    } // Every damage plane stayed untouched.
    assert!(
        finish(&mut engine, &mut owner, &output, false)
            .unwrap()
            .accepted
    ); // Require actual native Surface acceptance.
} // Drawing getters are unnecessary for inventory.

#[test] // Neither hook may expose sealed planes to guests.
fn private_inventory_and_finish_refuse_script_before_during_and_after_render() {
    // Probe six distinct guest lifecycle windows.
    let _serial = serial(); // Serialize the original native root.
    let script = r#" // Guest-visible names do not confer native phase access.
        globalThis.inventory_errors=[];globalThis.finish_errors=[];globalThis.inventory_phases=[]; // Retain only error and phase observations.
        globalThis.probe=(frame)=>{inventory_phases.push(__ilium_service_phase());try{__ilium_frame_buffers(frame);inventory_errors.push('leaked');}catch(error){inventory_errors.push(error.message);}try{__ilium_finish_frame(frame);finish_errors.push('leaked');}catch(error){finish_errors.push(error.message);}}; // Both hooks must deny before exposing sealed data.
        probe({});export function plan(){return {};} // Probe module phase before WeakMap lookup.
        export async function create(){probe({});return {render(c,f){globalThis.saved_frame=f;probe(f);f.after_accept(()=>probe(f));f.pixels.set_pixel(0,0,7);f.present();probe(f);},dispose(){}};} // Probe create, render, post-present, and acknowledgement.
    "#; // Complete adversarial package.
    let mut engine = engine(package(script), json!({})); // Native code binds preparation authority.
    assert_eq!(engine.evaluate_json("['__ilium_make_frame','__ilium_frame_buffers','__ilium_finish_frame','__ilium_seed_frame'].map(name=>{const d=Object.getOwnPropertyDescriptor(globalThis,name);return [d.writable,d.configurable,d.enumerable];})").unwrap(), json!([[false,false,false],[false,false,false],[false,false,false],[false,false,false]])); // All hooks remain sealed and nonenumerable.
    let mut owner = Owner::new(shape(Format::Gray8, Update::Retain, false)); // No optional colour access is needed.
    let (context, arrays) = begin(&mut engine, &mut owner, 1, &[], json!({})); // Seed real working storage.
    let output = engine.render(&context, &arrays).unwrap(); // Native inventory precedes guest render.
    assert_eq!(output.planes["data"][0], 7); // Denied hook calls cannot corrupt submitted bytes.
    assert!(
        finish(&mut engine, &mut owner, &output, false)
            .unwrap()
            .accepted
    ); // Also execute the acknowledgement probe.
    engine.evaluate_json("probe(saved_frame);null").unwrap(); // Probe again through nonacquiring diagnostics.
    assert_eq!(
        engine.evaluate_json("inventory_errors").unwrap(),
        json!([
            "native_frame_inventory_phase",
            "native_frame_inventory_phase",
            "native_frame_inventory_phase",
            "native_frame_inventory_phase",
            "native_frame_inventory_phase",
            "native_frame_inventory_phase"
        ])
    ); // Every guest inventory call must fail at the phase gate.
    assert_eq!(
        engine.evaluate_json("finish_errors").unwrap(),
        json!([
            "native_frame_finish_phase",
            "native_frame_finish_phase",
            "native_frame_finish_phase",
            "native_frame_finish_phase",
            "native_frame_finish_phase",
            "native_frame_finish_phase"
        ])
    ); // Every guest finish call must fail before exposing planes.
    assert_eq!(
        engine.evaluate_json("inventory_phases").unwrap(),
        json!([0, 1, 0, 0, 0, 0])
    ); // Only create permits acquisition; neither private phase escapes.
    assert!(engine.take_requests().unwrap().is_empty()); // Denials admit no service requests.
} // Hook sealing alone is insufficient authority.

fn inventory_engine(frame_expression: &str, hook_statement: &str, render: &str) -> Engine {
    // Isolate native collection from production drawing.
    let bootstrap = format!(
        r#" // Trusted collector fixture, not a service dispatcher.
        globalThis.__ilium_host=Object.freeze({{}}); // Mandatory host supplies no effects or grants.
        globalThis.__ilium_make_frame=()=>{{const plane=new Uint8Array(4);globalThis.inventory_plane=plane;return {frame_expression};}}; // Allocate one actual V8 backing buffer.
        globalThis.__ilium_finish_frame=()=>{{globalThis.native_finish_phase=__ilium_service_phase();return {{metadata:{{submitted:true}},planes:{{data:inventory_plane}}}};}}; // Capture native finish phase and return one typed plane.
        globalThis.__ilium_accept_frame=()=>{{}}; // No Surface publication callback in this fixture.
        {hook_statement} // Seal the supplied hook or its absence.
    "#
    ); // Complete collector bootstrap.
    let package_source = "globalThis.inventory_absence_at_load=__ilium_frame_buffers===undefined?!Reflect.defineProperty(globalThis,'__ilium_frame_buffers',{value:()=>({})}):null;".to_owned() + &source(render); // An absent-hook replacement is attempted during module evaluation.
    let mut engine = activated_engine(package(&package_source), &bootstrap); // Preserve actual Engine limits.
    assert_eq!(
        engine.start_create(&json!({}), &json!({})).unwrap(),
        CreateState::Ready
    ); // Native activation precedes create.
    engine // Return the actual ready instance.
} // This fixture proves no native registry integration.

#[test] // Forty-eight views share one physical buffer.
fn private_inventory_avoids_throwing_getters_and_deduplicates_original_buffers() {
    // Private phases must close before render/checkpoint.
    let _serial = serial(); // Reuse the original finite platform.
    let mut engine = inventory_engine("({get gray(){throw new Error('drawing_getter');},get cell_rgb(){throw new Error('drawing_getter');},get cells(){throw new Error('drawing_getter');},get pixels(){throw new Error('drawing_getter');}})", "globalThis.__ilium_frame_buffers=()=>{globalThis.inventory_phase=__ilium_service_phase();Promise.resolve().then(()=>globalThis.inventory_reaction_phase=__ilium_service_phase());const result={};for(let index=0;index<48;index+=1)result['alias_'+index]=new Uint8Array(inventory_plane.buffer);return result;};", "if(typeof inventory_reaction_phase!=='undefined')throw new Error('early_inventory_checkpoint');globalThis.render_phase=__ilium_service_phase();"); // Getter access throws; counting 48 aliases separately would overflow when output retains the same buffer again.
    let output = engine
        .render(&json!({}), &[spec("data", TypedArrayKind::U8, 4)])
        .unwrap(); // Run actual native inventory and final detachment.
    assert_eq!(output.planes["data"], vec![0; 4]); // Preserve original bytes.
    assert_eq!(engine.evaluate_json("[inventory_phase,render_phase,native_finish_phase,inventory_reaction_phase,inventory_plane.byteLength,inventory_plane.buffer.byteLength]").unwrap(), json!([4,0,5,0,0,0])); // Require phases 4,0,5,0 and detached aliases.
    engine.accept_frame(true).unwrap(); // Acknowledge the completed render.
} // Deduplication does not depend on GC timing.

#[test] // Legacy fallback requires trusted hook absence.
fn optional_inventory_absence_is_sealed_before_guest_load() {
    // Guests cannot change native discovery selection.
    let _serial = serial(); // Reuse the original shared root.
    let mut engine = inventory_engine(
        "({gray:plane})",
        "",
        "globalThis.legacy_phase=__ilium_service_phase();",
    ); // Supply the supported legacy gray view.
    assert_eq!(
        engine.evaluate_json("inventory_absence_at_load").unwrap(),
        json!(true)
    ); // Replacement already failed during module evaluation.
    assert_eq!(engine.evaluate_json("(()=>{const d=Object.getOwnPropertyDescriptor(globalThis,'__ilium_frame_buffers');return [d.value===undefined,d.writable,d.configurable,d.enumerable,Reflect.defineProperty(globalThis,'__ilium_frame_buffers',{value:()=>({})}),Reflect.deleteProperty(globalThis,'__ilium_frame_buffers')];})()").unwrap(), json!([true,false,false,false,false,false])); // Absence itself is immutable and nonenumerable.
    let output = engine
        .render(&json!({}), &[spec("data", TypedArrayKind::U8, 4)])
        .unwrap(); // Exercise actual legacy discovery.
    assert_eq!(output.planes["data"], vec![0; 4]); // Preserve legacy binary output.
    assert_eq!(
        engine
            .evaluate_json("[legacy_phase,inventory_plane.byteLength]")
            .unwrap(),
        json!([0, 0])
    ); // Require render phase and physical alias detachment.
    engine.accept_frame(true).unwrap(); // Acknowledge the separate logical output.
} // Invalid present hooks never mean absence.

#[test] // Malformed private inventory must never fall back.
fn malformed_private_inventory_is_rejected_without_public_getter_fallback() {
    // Exercise exact native validation branches.
    let _serial = serial(); // Serialize real isolate lifetimes.
    for (hook, expected) in [ // Complete trusted hook inputs follow.
        ("globalThis.__ilium_frame_buffers=null;", "frame inventory hook type"), // Present nonfunction is not absence.
        ("globalThis.__ilium_frame_buffers=()=>null;", "frame inventory must be a nonproxy record"), // Reject null inventory.
        ("globalThis.__ilium_frame_buffers=()=>[];", "frame inventory must be a nonproxy record"), // Reject array inventory.
        ("globalThis.__ilium_frame_buffers=()=>new Date();", "frame inventory must be a nonproxy record"), // Reject native exotic inventory.
        ("globalThis.__ilium_frame_buffers=()=>new Proxy({}, {ownKeys(){throw new Error('proxy_trap');}});", "frame inventory must be a nonproxy record"), // Reject proxies before traps.
        ("globalThis.__ilium_frame_buffers=()=>({[Symbol('hidden')]:inventory_plane});", "frame inventory symbol key"), // Reject symbols rather than omit them.
        ("globalThis.__ilium_frame_buffers=()=>({get hidden(){throw new Error('inventory_getter');}});", "service accessor properties are forbidden"), // Reject accessors without calling getters.
        ("globalThis.__ilium_frame_buffers=()=>Object.defineProperty({},'hidden',{get(){throw new Error('inventory_getter');}});", "service accessor properties are forbidden"), // Inspect nonenumerable descriptors too.
        ("globalThis.__ilium_frame_buffers=()=>({known:inventory_plane,bad:17});", "frame inventory view type"), // Reject a nonview after one valid entry.
        ("globalThis.__ilium_frame_buffers=()=>{const result={};for(let index=0;index<49;index+=1)result['p'+index]=inventory_plane;return result;};", "frame inventory plane count"), // Enforce 48 own keys before buffer retention.
    ] { // Use a fresh engine for every rejection.
        let mut engine = inventory_engine("({get gray(){throw new Error('drawing_getter');},get cell_rgb(){throw new Error('drawing_getter');},get cells(){throw new Error('drawing_getter');},get pixels(){throw new Error('drawing_getter');}})", hook, "throw new Error('render_must_not_run');"); // Fallback or guest render would produce a different error.
        let failure = engine.render(&json!({}), &[spec("data", TypedArrayKind::U8, 4)]).unwrap_err(); // Invoke the actual native collector.
        assert!(failure.to_string().contains(expected), "expected {expected}: {failure}"); // Require the intended refusal branch.
        assert!(engine.is_invalid()); // Malformed inventory retires execution.
    } // Never return partial frame success.
} // Unreached buffers release with their engine; this test does not claim individual detachment before collection.

#[test] // Native inventory cannot acquire services.
fn frame_inventory_cannot_acquire_or_leave_usable_engine_authority() {
    // Ignored refusal still invalidates the transaction.
    let _serial = serial(); // Use the actual native bridge.
    let mut engine = inventory_engine("({gray:plane})", "globalThis.__ilium_frame_buffers=()=>{void __ilium_dispatch('http.request',{});return {data:inventory_plane};};", ""); // Bypass SDK guards to exercise native phase enforcement.
    let failure = engine
        .render(&json!({}), &[spec("data", TypedArrayKind::U8, 4)])
        .unwrap_err(); // Acquisition attempt cannot publish a frame.
    assert!(failure.to_string().contains("FrameInventory"), "{failure}"); // Require the actual native phase refusal.
    assert!(engine.is_invalid()); // The poisoned engine cannot resume.
    assert_eq!(engine.service_usage(), (0, 0)); // Refuse before request admission.
} // No synthetic broker permission participates.

#[test] // Service storage is independent of frame storage.
fn completion_has_no_checkpoint_and_service_bytes_survive_frame_detachment() {
    // Exercise actual Engine and production SDK transport.
    let _serial = serial(); // Use the original root for both copies.
    let script = r#" // Fixture data does not qualify HTTP or broker effects.
        export function plan(){return {};} // No synthetic permission plan.
        export async function create(host){const response=await host.http.request({url:'https://example.invalid/fixture',response:'bytes'});if(!response.ok)throw new Error(response.error.code);globalThis.service_bytes=response.value.body;host.status.log('info','service_resumed');return {render(c,f){globalThis.frame_alias=f.pixels.data;f.pixels.set_pixel(0,0,service_bytes[0]);f.present();},dispose(){}};} // Observe continuation only after an authorized pump.
    "#; // Complete async-create/synchronous-render package.
    let mut engine = activated_engine(package(script), BOOTSTRAP); // Bind native authority before create.
    assert_eq!(
        engine.start_create(&json!({}), &json!({})).unwrap(),
        CreateState::Pending
    ); // Require one native-backed pending request.
    let requests = engine.take_requests().unwrap(); // Use the real admitted correlation.
    assert_eq!(requests.len(), 1); // Exactly one native flight.
    assert_eq!(requests[0].method, "http.request"); // Require the actual SDK method spelling.
    let producer_admission = quota().reserve_external_storage(4096).unwrap(); // Admit fixture source custody before allocation.
    let native_bytes = BTreeMap::from([("b0".to_owned(), vec![7_u8, 11, 13, 17])]); // Allocate only under the original producer guard.
    let result = ServiceValue::copy_from_host(&json!({"ok":true,"value":{"status":200,"headers":{},"body":{"$ilium_binary":"b0"},"final_url":"https://example.invalid/fixture"}}), &[spec("b0", TypedArrayKind::U8, 4)], &native_bytes, &EngineLimits::default(), quota()).unwrap(); // Use a typed Result envelope and original-root copy.
    assert_eq!(
        engine
            .complete_service_request(requests[0].id, native_authority(), result)
            .unwrap(),
        CompletionState::Delivered
    ); // Require native copy/settlement success before reactions.
    assert_eq!(
        engine.take_status().unwrap(),
        json!({"records":[],"dropped":0})
    ); // No continuation log can exist before the pump.
    assert!(engine.take_requests().unwrap().is_empty()); // Completion acquires no follow-up work.
    assert_eq!(engine.pump().unwrap(), CreateState::Ready); // Only this separate pump resumes create.
    let status = engine.take_status().unwrap(); // Observe the authorized continuation.
    assert_eq!(status["records"].as_array().unwrap().len(), 1); // Require exactly one continuation log.
    assert_eq!(status["records"][0]["message"], json!("service_resumed")); // Read the production status hook.
    let mut owner = Owner::new(shape(Format::Gray8, Update::Retain, false)); // Allocate independent native frame storage.
    let (context, arrays) = begin(&mut engine, &mut owner, 1, &[], json!({})); // Seed no service result as a frame plane.
    let output = engine.render(&context, &arrays).unwrap(); // Draw from retained service bytes.
    assert_eq!(output.planes["data"][0], 7); // Require their actual submitted pixel value.
    assert_eq!(
        engine
            .evaluate_json(
                "[Array.from(service_bytes),service_bytes.byteLength,frame_alias.byteLength]"
            )
            .unwrap(),
        json!([[7, 11, 13, 17], 4, 0])
    ); // Frame detaches while service bytes stay live.
    assert_eq!(native_bytes["b0"], vec![7, 11, 13, 17]); // Preserve the native producer source.
    assert!(
        finish(&mut engine, &mut owner, &output, false)
            .unwrap()
            .accepted
    ); // Require downstream native Surface acceptance.
    drop(native_bytes); // Destroy producer bytes before their guard.
    drop(producer_admission); // Release custody after physical source destruction.
} // Actual HTTP authorization, redirects, registries, and network issue remain owner acceptance gates.
