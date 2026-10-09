"use strict"; // Test-only Bun harness using supported VM built-ins; production imports no runtime APIs.
const { readFileSync } = require("node:fs"), { resolve } = require("node:path"), { createContext, runInContext } = require("node:vm"); // Read exact proposed source into an isolated JS realm.
const { test } = require("bun:test"), assert = require("node:assert/strict"), { types } = require("node:util"); // Built-in testing and cross-realm Promise detection.
const { createHash } = require("node:crypto");
const source = readFileSync(resolve(__dirname, "../src/bootstrap.js"), "utf8"); // No downloaded or regenerated bootstrap fixture.
const get_buffer = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype), "buffer").get; // Ignore script-shadowed buffer properties.
function environment(dispatch = () => ({ ok: true, value: null }), admission = () => true) { // Facade fixture; no native registry, authority, quota, worker or checkpoint.
    let phase = 2; const context = createContext({}); const env = { context, evaluate: (code) => runInContext(code, context, { timeout: 1000 }), phase: (value) => { phase = value; }, native: (value, action) => { const previous = phase; phase = value; try { return action(); } finally { phase = previous; } } }; // Only the embedding test controls phase.
    const native_dispatch = (method, payload, references) => { const admitted = admission(method, payload, references); let pending; try { pending = admitted ? dispatch(method, payload, references, env) : { ok: false, error: { code: "fixture_refused", message: "The fixture did not admit this request." } }; } catch (error) { throw error; } return Object.defineProperty(Promise.resolve(pending), "__ilium_admitted", { value: admitted }); }; // Preserve raw options and seal the exact admission receipt.
    Object.defineProperties(context, { __ilium_dispatch: { value: native_dispatch }, __ilium_service_phase: { value: () => phase }, __ilium_service_wire_version: { value: 1 }, __ilium_geography_project: { value: () => { throw Error("fixture native geography hook not configured"); } }, __ilium_astronomy_observe: { value: () => { throw Error("fixture native astronomy hook not configured"); } }, __ilium_text_measure: { value: () => { throw Error("fixture native text hook not configured"); } } }); runInContext(source, context, { timeout: 1000 }); return env; // Native admission and pumping are tested separately.
} // Construct native JSON in the target realm below.
function host_seed(env, metadata) { return env.native(3, () => env.evaluate(`__ilium_seed_frame(${metadata},{});`)); } // Model a native seed; grant no script authority.
function fixture_result(env, expression) { return env.evaluate(`({__proto__:null,ok:true,value:(${expression})})`); } // Preserve realm-local binary result leaves.
async function settle_controls() { for (let index = 0; index < 12; index += 1) await Promise.resolve(); } // Drain fixture Promises only; no native checkpoint claim.
function descriptor(format = "gray8", update = "retain", cell_rgb = false, base = "0", sequence = "1") { return { key: { instance_id: "1", revision: "1", base_version: base, sequence }, shape: { cell_width: 3, cell_height: 2, mode: format === "mask8" ? "cells" : "pixels", format, update, cell_rgb, colour_space: "srgb" }, reset: base === "0", invalid_rects: [], input_specs: [] }; } // Same field names as Rust FrameSeed/Shape.
function seed(env, frame, values = null, extra = "") { // Native fixture creates exact binary buffers in the embedding realm.
    const s = frame.shape, width = s.mode === "cells" ? 3 : 6, height = s.mode === "cells" ? 2 : 8, channels = s.format === "rgb8" ? 3 : s.format === "rgba8" ? 4 : 1, count = (s.format === "mono1" ? Math.ceil(width / 8) : width * channels) * height; // Independently derive ArraySpec counts.
    const kind = s.format === "gray32" ? "Float32Array" : "Uint8Array", data = values ? `new ${kind}(${JSON.stringify(values)})` : `new ${kind}(${count})`; // Tiny fixture literals only; production injection stays binary.
    env.native(3, () => env.evaluate(`globalThis.native_descriptor = ${JSON.stringify(frame)}; __ilium_seed_frame({frame:native_descriptor}, {work_data:${data}${s.cell_rgb ? ",work_cell_rgb:new Uint8Array(18)" : ""}${extra}});`)); // Seed baseline/input arrays in phase 3 only.
} // The harness does not give bootstrap access to a filesystem or timers.
function make(env) { env.phase(0); return env.evaluate("globalThis.frame = __ilium_make_frame({_ilium_frame:{key:native_descriptor.key,shape:native_descriptor.shape},time:0,wall:0,delta:0,inputs:{}}); frame;"); } // Use the injected frame.context.
function finish(env) { return env.native(5, () => env.context.__ilium_finish_frame(env.context.frame)); } // Only phase 5 exposes the complete handoff inventory.
function detach(output) { const buffers = [...new Set(Object.values(output.planes).map((plane) => Reflect.apply(get_buffer, plane, [])))]; structuredClone(null, { transfer: buffers }); for (const buffer of buffers) assert.equal(buffer.byteLength, 0); } // Physical detachment, not a mocked length assignment.
function acknowledge(env, output, accepted) { detach(output); try { env.context.__ilium_accept_frame(accepted); } finally { env.phase(2); } } // Detach before acknowledgement; restore async fixture phase.
function shipped_entry(id) { // Read a pinned repository fixture, not a production ZIP validator or sandbox.
    const filename = `${id}-1.0.0.iliumanim`, archive = readFileSync(resolve(__dirname,"../assets/packages",filename));
    const inventory = readFileSync(resolve(__dirname,"../src/release.rs"),"utf8");
    const identity = inventory.match(new RegExp(`\\(\\s*"${id}",\\s*"${id}-1\\.0\\.0\\.iliumanim",\\s*"([a-f0-9]{64})",\\s*\\)`));
    assert.ok(identity, "fixture archive requires a compiled release identity");
    assert.equal(createHash("sha256").update(archive).digest("hex"), identity[1]);
    let offset = 0;
    for (let count = 0; count < 256 && offset + 30 <= archive.length; count += 1) {
        if (archive.readUInt32LE(offset) !== 0x04034b50) break;
        assert.equal(archive.readUInt16LE(offset + 6), 0x800); assert.equal(archive.readUInt16LE(offset + 8), 0);
        const size = archive.readUInt32LE(offset + 18), name_bytes = archive.readUInt16LE(offset + 26);
        assert.equal(size, archive.readUInt32LE(offset + 22));
        const data = offset + 30 + name_bytes + archive.readUInt16LE(offset + 28), end = data + size;
        assert.ok(end <= archive.length && size <= 16 * 1024 * 1024);
        const name = archive.subarray(offset + 30, offset + 30 + name_bytes).toString("utf8");
        if (name === "entry.mjs") return archive.subarray(data,end).toString("utf8");
        offset = end;
    }
    assert.fail("pinned stored fixture requires entry.mjs");
}
test("all seven formats expose only admitted exact planes", () => { // Validate interface layout and selected scalar storage.
    for (const format of ["mask8", "mono1", "mono8", "gray8", "gray32", "rgb8", "rgba8"]) { const env = environment(), d = descriptor(format, "replace"); seed(env, d); make(env); const value = ["rgb8", "rgba8"].includes(format) ? "{r:255,g:0,b:0,a:255}" : ["mask8", "mono1", "mono8", "gray32"].includes(format) ? "1" : "255"; env.evaluate(format === "mask8" ? "frame.cells.set_cell(0,0,{mask:1});frame.present();" : `frame.pixels.set_pixel(0,0,${format === "rgb8" ? "{r:255,g:0,b:0}" : value});frame.present();`); const output = finish(env); assert.deepEqual(Object.keys(output.planes).sort(), ["data", "order", "touch", "work_data", "work_order", "work_touch"].sort()); assert.equal(output.planes.touch[0], 1); assert.equal(output.planes.order[0], 1); assert.equal(types.isFloat32Array(output.planes.data), format === "gray32"); assert.equal(types.isUint32Array(output.planes.order), true); assert.equal(output.planes.touch.length, format === "mask8" ? 6 : 48); assert.equal(output.planes.data.length, format === "mask8" ? 6 : format === "mono1" ? 8 : format === "rgb8" ? 144 : format === "rgba8" ? 192 : 48); acknowledge(env, output, true); } // No hidden RGBA or optional colour allocation.
}); // Shape agreement is necessary but does not establish V8 native acceptance.
test("present seals bytes and after_accept requires physical detachment", () => { // Hold an alias across present and handoff.
    const env = environment(); seed(env, descriptor()); make(env); env.evaluate("globalThis.accepted_count=0;globalThis.alias=frame.pixels.data;alias[0]=7;frame.damage_rect({unit:'pixels',x:0,y:0,width:1,height:1});frame.after_accept(()=>{accepted_count+=1;});frame.present();alias[0]=99;"); // Deliberate write after present.
    const output = finish(env); assert.equal(output.planes.data[0], 7); assert.equal(output.planes.work_data[0], 99); assert.throws(() => env.context.__ilium_accept_frame(true), /buffers_not_detached/); assert.equal(env.context.accepted_count, 0); // Copying alone is insufficient.
    acknowledge(env, output, true); assert.equal(env.context.accepted_count, 1); assert.equal(env.context.alias.byteLength, 0); assert.throws(() => env.evaluate("alias.fill(1)"), /detached/); assert.throws(() => env.context.__ilium_accept_frame(true), /missing_handoff/); // One logical acknowledgement only.
}); // Actual terminal emission is never asserted by this callback.
test("rejection never advances the next retained drawing baseline", () => { // Native retains version 12 and value 4 after rejection.
    const env = environment(), baseline = Array(48).fill(0); baseline[0] = 4; seed(env, descriptor("gray8", "retain", false, "12", "20"), baseline); make(env); env.evaluate("globalThis.accepted_count=0;frame.pixels.set_pixel(0,0,9);frame.after_accept(()=>{accepted_count+=1;});frame.present();"); // Attempt a new drawing value.
    const output = finish(env); assert.equal(output.planes.data[0], 9); acknowledge(env, output, false); assert.equal(env.context.accepted_count, 0); // The Rust rejection does not run bookkeeping.
    seed(env, descriptor("gray8", "retain", false, "12", "21"), baseline); make(env); assert.equal(env.evaluate("frame.pixels.data[0]"), 4); env.evaluate("frame.present()"); const next = finish(env); assert.equal(next.metadata.key.base_version, "12"); assert.equal(next.planes.touch[0], 0); acknowledge(env, next, true); // A retained no-op uses a fresh native seed.
}); // No JS canonical mirror can silently preserve rejected pixels.
test("empty replacement and no-present are different submissions", () => { // Reusing old data cannot defeat implicit replacement clear.
    const env = environment(); seed(env, descriptor("gray8", "replace"), Array(48).fill(255)); make(env); const none = finish(env); assert.equal(none.metadata.presented, false); acknowledge(env, none, false); // No-present leaves canonical state to the native owner.
    seed(env, descriptor("gray8", "replace", false, "0", "2"), Array(48).fill(255)); make(env); env.evaluate("frame.present()"); const empty = finish(env); assert.equal(empty.metadata.presented, true); assert.ok(Array.from(empty.planes.data).every((v) => v === 0)); assert.ok(Array.from(empty.planes.touch).every((v) => v === 0)); acknowledge(env, empty, true); // Rust interprets zero touches in replace as full known-empty clearing.
}); // No-present never becomes a cached blank frame accidentally.
test("duplicate present and caught bounds errors poison the frame", () => { // Script catch blocks cannot make malformed transactions acceptable.
    for (const code of ["frame.present();try{frame.present();}catch{}", "try{frame.pixels.set_pixel(6,0,1);}catch{}", "try{frame.pixels.write_rect({x:0,y:0,width:1,height:1,format:'gray8',data:new Uint8Array(2)});}catch{}"]) { const env = environment(); seed(env, descriptor()); make(env); env.evaluate(code); const output = finish(env); assert.equal(output.metadata.error, "frame_operation_failed"); acknowledge(env, output, false); } // Invalid type/length/bounds cannot partially publish.
}); // The native Surface rejects metadata.error before applying any planes.
test("packed rows, direct damage, defer, and command order share one protocol", () => { // Validate real local encoders, with no dispatch per dot.
    let requests = 0; const env = environment(async () => { requests += 1; return { ok: true, value: null }; }); seed(env, descriptor("mono1", "retain")); make(env); env.evaluate("frame.pixels.write_rows({x:0,y:0,width:5,height:1,format:'mono1',rows:[new Uint8Array([168])]});__ilium_host.draw.blit({handle:'prepared',source:{x:0,y:0,width:1,height:1},target:{x:0,y:0,width:1,height:1}});frame.pixels.set_pixel(0,0,0);frame.defer_rect({unit:'pixels',x:1,y:1,width:1,height:1});frame.present();"); // Scalar overwrite occurs after the native blit request.
    const output = finish(env); assert.equal(output.planes.data[0], 40); assert.equal(output.metadata.commands[0].order, 2); assert.equal(output.planes.order[0], 3); assert.equal(output.planes.touch[7], 3); assert.equal(requests, 0); acknowledge(env, output, true); // Native can apply correct order without receiving per-dot RPC.
    const bad = environment(); seed(bad, descriptor("mono1")); make(bad); bad.evaluate("try{frame.pixels.write_rows({x:0,y:0,width:5,height:1,format:'mono1',rows:[new Uint8Array([1])]});}catch{}"); const failed = finish(bad); assert.ok(failed.metadata.error); acknowledge(bad, failed, false); // Nonzero unused row bits are malformed.
}); // Logical defer remains distinct from known-empty clear.
test("colour presence and zero-area metadata edits stay separate", () => { // Black cannot be the palette sentinel.
    const env = environment(); seed(env, descriptor("mask8", "retain", true)); make(env); env.evaluate("frame.cells.set_cell(0,0,{mask:255,rgb:{r:0,g:0,b:0}});frame.set_cell_rgb(1,0,{r:0,g:0,b:0});frame.cells.clear_rect({x:2,y:0,width:0,height:1});frame.present();"); // One pixel/mask edit and one colour-only edit.
    const output = finish(env); assert.equal(output.planes.colour_touch[0], 1); assert.equal(output.planes.colour_touch[1], 1); assert.equal(output.planes.touch[1], 0); assert.equal(output.planes.colour_touch[2], 0); assert.equal(output.planes.cell_rgb.length, 18); assert.equal(output.planes.colour_order.length, 6); acknowledge(env, output, true); // Metadata-only colour changes preserve sample validity.
}); // Native independently enforces implicit colour clearing on clear/defer.
test("binary inputs of every engine kind are copied views and detached with the frame", () => { // No JSON array of large input or baseline pixels.
    const env = environment(), d = descriptor(); d.input_specs = [{ path: "occlusion.cells", kind: "u8", elements: 6 }, { path: "audio.bands", kind: "f32", elements: 4 }, { path: "audio.indices", kind: "u16", elements: 2 }, { path: "audio.sequence", kind: "u32", elements: 1 }]; // Host-selected subscribed projections.
    seed(env, d, null, ",input_0:new Uint8Array(6),input_1:new Float32Array([0.25,0.5,0.75,1]),input_2:new Uint16Array([2,3]),input_3:new Uint32Array([9])"); make(env); assert.equal(env.evaluate("frame.context.inputs.audio.bands[2]"), 0.75); env.evaluate("globalThis.input_alias=frame.context.inputs.occlusion.cells;input_alias[0]=1;frame.present();"); // Changing this copy cannot change a native compositor mask.
    const output = finish(env); assert.equal(types.isUint16Array(output.planes.input_2), true); assert.equal(types.isUint32Array(output.planes.input_3), true); acknowledge(env, output, true); assert.equal(env.context.input_alias.byteLength, 0); // All borrowed input buffers participate in physical handoff.
}); // Permission/final-mask enforcement remains native and is not simulated here.
test("wrong seed/base and oversized direct views are rejected", () => { // The hook checks identities before allocating a new facade.
    const env = environment(); seed(env, descriptor()); assert.throws(() => env.evaluate("__ilium_make_frame({_ilium_frame:{key:{...native_descriptor.key,base_version:'9'},shape:native_descriptor.shape},inputs:{}})"), /seed_context_mismatch/); make(env); acknowledge(env, finish(env), false); // The original valid seed remains usable after a failed make.
    const bad = environment(), d = descriptor(); assert.throws(() => bad.native(3, () => bad.evaluate(`__ilium_seed_frame({frame:${JSON.stringify(d)}},{work_data:new Uint8Array(new ArrayBuffer(49),1,48)})`)), /oversized_or_offset_view/); // Logical length cannot hide excess backing storage.
}); // Native repeats these checks before reading buffer memory.
test("RPC returns structured results and bounds unresolved requests", async () => { // Exercise the actual Promise bridge and local pending counters.
    const gate = Promise.withResolvers(); let count = 0; const env = environment(async (method, payload) => { count += 1; await gate.promise; return { ok: true, value: { method, payload } }; }); // Controlled completion fixture.
    const pending = env.evaluate("Promise.all(Array.from({length:65},()=>__ilium_host.inputs.poll({})))"); assert.equal(count, 64); gate.resolve(); const results = await pending; assert.equal(results[64].error.code, "host_request_rejected"); assert.equal(results[0].value.method, "inputs.poll"); // Refuse the 65th unresolved request.
    seed(env, descriptor()); make(env); const denied = await env.evaluate("__ilium_host.inputs.poll({})"); assert.equal(denied.error.code, "render_phase"); assert.equal(count, 64); acknowledge(env, finish(env), false); // Render performs no dispatch.
    const recovered = await env.evaluate("__ilium_host.inputs.poll({})"); assert.equal(recovered.ok, true); assert.equal(count, 65); // Settlement releases the local pending slot.
}); // Real native dispatch/delivery authority is tested by the parent's D2 integration.
test("permissions are informational, unique, and native hooks are sealed", () => { // Snapshot contents cannot create a native channel.
    const env = environment(); host_seed(env, "{host:{permissions:[{request_id:'a',id:'network.http',scope:{origins:['https://a.example']}},{request_id:'b',id:'network.http',scope:{origins:['https://b.example']}}],package:{id:'test'},bundle:'bundle'},frame:null}"); // Seed metadata before create.
    assert.equal(env.evaluate("__ilium_host.permissions.has('network.http')"), false); assert.equal(env.evaluate("__ilium_host.permissions.has('a')"), false); assert.throws(() => env.evaluate("__ilium_host.assets.bundle"), /native_bundle_handle_unavailable/); // Request descriptions and string bundles grant no rights.
    host_seed(env, "{host:{permissions:[{request_id:'a',id:'network.http',scope:{origins:['https://a.example']},epoch:7}]},frame:null}"); assert.equal(env.evaluate("__ilium_host.permissions.has('a')"), true); assert.equal(env.evaluate("Object.isFrozen(__ilium_host.permissions.get('a'))"), true); // Native grant observations remain informational.
    for (const name of ["__ilium_host", "__ilium_seed_frame", "__ilium_make_frame", "__ilium_frame_buffers", "__ilium_finish_frame", "__ilium_accept_frame"]) { assert.equal(env.evaluate(`Object.getOwnPropertyDescriptor(globalThis,'${name}').writable`), false); assert.equal(env.evaluate(`Object.getOwnPropertyDescriptor(globalThis,'${name}').configurable`), false); assert.equal(env.evaluate(`Object.getOwnPropertyDescriptor(globalThis,'${name}').enumerable`), false); } // Hooks resist replacement, deletion and enumeration.
    assert.throws(() => env.evaluate("'use strict'; __ilium_make_frame = ()=>null"), /read only|readonly|Cannot assign/); // Sealing is an enforced property descriptor.
}); // Callable global hooks still require native key/phase validation; sealing is not authority.
test("native rejects async render; async after_accept is a retirement error", async () => { // The frozen finish signature cannot inspect instance.render's return value itself.
    const env = environment(); seed(env, descriptor()); make(env); const returned = env.evaluate("(async()=>{frame.pixels.set_pixel(0,0,1);frame.present();})()"); assert.equal(types.isPromise(returned), true); const output = finish(env); acknowledge(env, output, false); await returned; // Native Promise detection forces rejection and cleanup.
    const bookkeeping = environment(); seed(bookkeeping, descriptor()); make(bookkeeping); bookkeeping.evaluate("frame.after_accept(async()=>{});frame.present()"); const accepted = finish(bookkeeping); detach(accepted); assert.throws(() => bookkeeping.context.__ilium_accept_frame(true), /async_after_accept/); // Native must retire after a post-publication bookkeeping failure.
}); // The harness does not mislabel this as a resumable V8 timeout test.
test("prototype changes cannot redirect handoff or reuse an old frame drawing facade", () => { // Reproduce package mutations after trusted installation.
    const env = environment(); env.evaluate("globalThis.to_json_calls=0;Object.prototype.toJSON=function(){to_json_calls+=1;throw Error('untrusted_to_json');};globalThis.Set=undefined;globalThis.String=undefined;Math.ceil=()=>{throw Error('untrusted_math');};"); // Captured intrinsics must remain usable.
    assert.throws(() => env.evaluate("'use strict';Array.prototype.sort=()=>[]"), /read only|readonly|Cannot assign/); seed(env, descriptor()); make(env); env.evaluate("globalThis.old_frame=frame;frame.present()"); const first = finish(env); assert.doesNotThrow(() => JSON.stringify(first.metadata)); assert.equal(env.context.to_json_calls, 0); acknowledge(env, first, true); // Only metadata is JSON; binary planes are never serialized as objects.
    seed(env, descriptor("gray8", "retain", false, "0", "2")); make(env); assert.throws(() => env.evaluate("old_frame.draw.line({points:[[0,0],[1,1]],value:1})"), /closed_frame/); env.evaluate("frame.present()"); const next = finish(env); assert.equal(next.metadata.error, null); assert.equal(next.metadata.commands.length, 0); acknowledge(env, next, true); // An old facade cannot enqueue work into the new transaction.
}); // Native authority is still enforced independently of trusted facade hardening.
test("after_accept cannot reenter acknowledgement", () => { // Count actual callback invocations after one physical handoff.
    const env = environment(); seed(env, descriptor()); make(env); env.evaluate("globalThis.calls=0;frame.after_accept(()=>{calls+=1;try{__ilium_accept_frame(true);}catch{}});frame.present()"); const output = finish(env); acknowledge(env, output, true); assert.equal(env.context.calls, 1); // Recursive hook calls cannot replay accepted bookkeeping.
}); // Native remains the sole owner of the publication decision.

test("gray32 direct alias and synchronous render diagnostics preserve the package API", () => {
    let requests = 0;
    const env = environment(async () => { requests += 1; return {ok:true,value:null}; });
    seed(env, descriptor("gray32", "replace")); make(env);
    env.evaluate("if(!(frame.gray instanceof Float32Array))throw Error('gray32_alias');frame.gray[0]=0.75;__ilium_host.status.log('info','native chess status');__ilium_host.status.progress(25,'simulation');frame.present();");
    const output = finish(env); assert.equal(output.planes.data[0], 0.75); assert.equal(output.planes.touch[0], 1);
    assert.equal(requests, 0); assert.equal(Object.hasOwn(output.metadata,"diagnostics"), false);
    const diagnostics = env.context.__ilium_take_status();
    assert.equal(diagnostics.records.length, 2); assert.equal(diagnostics.records[0].message, "native chess status");
    assert.equal(diagnostics.records[1].percent, 25); assert.equal(diagnostics.dropped, 0);
    assert.equal(env.context.__ilium_take_status().records.length, 0); acknowledge(env, output, true);
    assert.equal(env.evaluate("Object.getOwnPropertyDescriptor(globalThis,'__ilium_take_status').writable"), false);
    env.evaluate("for(let i=0;i<80;i+=1)__ilium_host.status.log('debug','bounded');");
    const bounded = env.context.__ilium_take_status(); assert.equal(bounded.records.length, 64); assert.equal(bounded.dropped, 16);
    assert.throws(() => env.evaluate("__ilium_host.status.log('info','x'.repeat(4097))"), /status_message/);
});

test("actual source and shipped Beach/Carpet modules render all eleven choices against the production facade", async () => {
    const project_root = process.env.ILIUM_ANIMATION_PROJECT_ROOT ?? resolve(__dirname,"../../../ilium-animations");
    for (const id of ["beach","carpet"]) {
        const result = await Bun.build({entrypoints:[resolve(project_root,id,"src/index.mts")],target:"browser",format:"esm",minify:false,write:false});
        assert.equal(result.success,true, JSON.stringify(result.logs));
        const source_bundle = await result.outputs[0].text(), shipped_bundle = shipped_entry(id), baselines = new Map();
        const footer = /export\s*\{\s*create\s*,\s*plan\s*\}\s*;?\s*$/;
        assert.ok(footer.test(source_bundle) && footer.test(shipped_bundle),"controlled bundles must export exactly create and plan");
        const settings = id === "beach" ? [{style:"classic"},{style:"rich"}] : Array.from({length:9},(_,mode)=>({mode}));
        const cases = settings.flatMap(configuration => [{configuration,bundled:source_bundle,label:"source"},{configuration,bundled:shipped_bundle,label:"shipped"}]);
        for (const {configuration,bundled,label} of cases) {
            const requests = [], tv = id === "carpet" && configuration.mode === 4;
            const env = environment((method, payload, references, realm) => {
                requests.push(method); assert.equal(method, "sources.chess.open");
                assert.equal(payload.game_id, "tv"); assert.equal(payload.max_hz, 1);
                return fixture_result(realm, "{id:'synthetic_tv_source',kind:'sources.chess',revision:1,status:{state:'ready'},latest:{revision:1,available:true,game_id:'synthetic-tv',fen:'4k3/8/8/8/8/8/8/R3K3 w - - 0 1',moves:[],white:'fixture-white',black:'fixture-black',state:'current_tv_position_history_unavailable',white_seconds:120,black_seconds:115}}");
            }); // Synthetic native descriptor tests the real facade; no live-provider or broker claim.
            env.evaluate(bundled.replace(footer,"globalThis.animation={create,plan};")); env.phase(1);
            env.context.fixture_settings = configuration;
            await env.evaluate("globalThis.accepted_plan=animation.plan(fixture_settings,'live',{});globalThis.pending_instance=animation.create(__ilium_host,fixture_settings,accepted_plan);");
            env.context.instance = await env.context.pending_instance;
            assert.deepEqual(requests, tv ? ["sources.chess.open"] : []);
            const d = descriptor("gray32","replace",false,"0",String(configuration.mode === undefined ? (configuration.style==="classic"?1:2) : configuration.mode+1));
            seed(env,d); make(env);
            env.evaluate("{const returned=instance.render({time:0.5,wall:0.5,delta:0.025,wall_delta:0.025,inputs:{}},frame);if(returned!==undefined)throw Error('render_must_be_sync');}");
            const output=finish(env); assert.equal(output.metadata.presented,true); assert.equal(output.metadata.error,null);
            assert.ok(Array.from(output.planes.data).every(value=>Number.isFinite(value)&&value>=0&&value<=1));
            assert.ok(Array.from(output.planes.touch).every(value=>value===1));
            const pixels = Array.from(output.planes.data), key = JSON.stringify(configuration);
            if (label === "source") baselines.set(key,pixels); else assert.deepEqual(pixels,baselines.get(key),`${id} ${key} shipped raster differs from source`);
            assert.deepEqual(requests, tv ? ["sources.chess.open"] : []); // Cached render performs no service acquisition.
            acknowledge(env,output,true); const status = env.context.__ilium_take_status();
            if (tv) assert.ok(status.records.some(record => record.message === "Lichess TV: synthetic-tv"));
            env.evaluate("instance.dispose();");
        }
    }
});

test("binary RPC preserves caller structure and structured terminal errors", async () => { // Preserve the raw tree for separate native validation.
    let observed, references, response; const env = environment((method, payload, table) => { assert.equal(method, "http.request"); observed = payload; references = table; return response; }); response = fixture_result(env, "{status:200,headers:{},body:new Uint8Array([8,9]),final_url:'https://example.org/'}"); // Realm-local binary response.
    env.evaluate("globalThis.getter_calls=0;globalThis.bytes=new Uint8Array([1,2,3,4]);globalThis.options={url:'https://example.org/',response:'bytes',body:bytes.subarray(1,3),get hidden(){getter_calls+=1;throw Error('getter');}};"); const result = await env.evaluate("__ilium_host.http.request(options)"); assert.equal(result.ok, true); assert.strictEqual(observed, env.context.options); assert.strictEqual(observed.body, env.context.options.body); assert.equal(observed.body.byteOffset, 1); assert.equal(env.context.getter_calls, 0); assert.equal(references, undefined); assert.deepEqual(Array.from(result.value.body), [8,9]); // No JSON or payload substitution.
    env.evaluate("globalThis.proxy_options=new Proxy(options,{});"); await env.evaluate("__ilium_host.http.request(proxy_options)"); assert.strictEqual(observed, env.context.proxy_options); assert.equal(env.context.getter_calls, 0); // Native ingress must reject the unchanged proxy.
    for (const code of ["cancelled", "timeout", "stale_authority", "budget"]) { response = { ok: false, error: { code, message: `native ${code}`, retry_after_ms: 7 } }; const failed = await env.evaluate("__ilium_host.http.request(options)"); assert.equal(failed.ok, false); assert.equal(failed.error.code, code); assert.equal(failed.error.message, `native ${code}`); assert.equal(failed.error.retry_after_ms, 7); assert.equal(Object.isFrozen(failed.error), true); } // Preserve native terminal error classifications.
    response = { ok: false, error: { code: "missing_message" } }; assert.equal((await env.evaluate("__ilium_host.http.request(options)")).error.code, "host_error"); response = Promise.reject(new Error("fixture failure")); assert.equal((await env.evaluate("__ilium_host.http.request(options)")).error.code, "host_error"); // Malformed and rejected promises become Result errors.
}); // Permissive fixtures prove no native authority.
test("native phases guard seed and private frame inventory while binary results outlive frame handoff", async () => { // Rust tests own actual phase and finally-cleanup proof.
    const env = environment((method, payload, references, realm) => fixture_result(realm, "new Uint8Array([4,5,6])")); const bytes = await env.evaluate("__ilium_host.cache.get({key:'binary',max_bytes:3})"); assert.equal(bytes.ok, true); assert.throws(() => env.evaluate("__ilium_seed_frame({frame:null},{})"), /native_seed_phase/); assert.throws(() => env.evaluate("__ilium_finish_frame({})"), /native_frame_finish_phase/); // Calling a hook cannot enter its native phase.
    const d = descriptor("gray8", "replace"); d.input_specs = [{ path: "audio.bands", kind: "f32", elements: 2 }]; seed(env, d, null, ",input_0:new Float32Array([0.25,0.75])"); make(env); assert.throws(() => env.evaluate("__ilium_frame_buffers(frame)"), /native_frame_inventory_phase/); assert.throws(() => env.evaluate("__ilium_finish_frame(frame)"), /native_frame_finish_phase/); // Script cannot inspect sealed planes through either hook.
    const inventory = env.native(4, () => env.context.__ilium_frame_buffers(env.context.frame)); assert.equal(Object.getPrototypeOf(inventory), null); assert.equal(Object.isFrozen(inventory), true); assert.equal(Object.keys(inventory).length, 7); assert.ok(Object.values(inventory).every(ArrayBuffer.isView)); assert.ok(Array.from(inventory.work_touch).every((value) => value === 0)); assert.equal(env.context.__ilium_service_phase(), 0); // Inventory invokes no damage getter; restore render phase.
    assert.throws(() => env.evaluate("globalThis.draw_alias=frame.pixels.data;globalThis.input_alias=frame.context.inputs.audio.bands;draw_alias[0]=12;try{__ilium_finish_frame(frame);throw Error('finish_exposed');}catch(error){if(error.message!=='native_frame_finish_phase')throw error;}frame.present();throw Error('render_failure');"), /render_failure/); assert.throws(() => env.evaluate("__ilium_frame_buffers(frame)"), /native_frame_inventory_phase/); assert.throws(() => env.evaluate("__ilium_finish_frame(frame)"), /native_frame_finish_phase/); assert.equal(inventory.data[0], 12); env.context.draw_alias[0] = 99; assert.equal(inventory.data[0], 12); // After present, only working bytes remain script-writable.
    const output = finish(env); assert.strictEqual(output.planes, inventory); acknowledge(env, output, false); assert.equal(env.context.draw_alias.byteLength, 0); assert.equal(env.context.input_alias.byteLength, 0); assert.ok(Object.values(inventory).every((view) => view.byteLength === 0)); assert.deepEqual(Array.from(bytes.value), [4,5,6]); assert.throws(() => env.evaluate("__ilium_frame_buffers(frame)"), /native_frame_inventory_phase/); assert.throws(() => env.evaluate("__ilium_finish_frame(frame)"), /native_frame_finish_phase/); // Detach frame aliases; retain independent service bytes.
    env.native(4, () => assert.throws(() => env.context.__ilium_frame_buffers({}), /foreign_frame/)); assert.equal(env.context.__ilium_service_phase(), 2); // Restore fixture phase even on inventory error.
}); // Rust tests cover malformed inventory, absence and deduplication.
test("asset, image and world wrappers preserve identity and expose only their SDK lifecycle", async () => { // Fixture descriptors confer no native registry authority.
    const calls = []; let allow_control = true, wrong_world = false; const close_gate = Promise.withResolvers(); const env = environment((method, payload, references, realm) => { calls.push({ method, payload, references }); if (method === "assets.read") return fixture_result(realm, "{bytes:new Uint8Array([7]),sha256:'a'.repeat(64),name:'one'}"); if (method === "media.images.decode") return fixture_result(realm, "{id:'image_a',width:2,height:2,format:'rgba8',sha256:'b'.repeat(64)}"); if (method === "worlds.open") return fixture_result(realm, "{id:'world_a',revision:1,status:{state:'ready'},identity:'native_world_digest'}"); if (method === "worlds.region") return fixture_result(realm, `{origin:[0,0,0],size:[1,1,2],palette:[{name:'minecraft:stone',properties:{}},{name:'minecraft:air',properties:{}}],blocks:new Uint16Array([0,1]),identity:'${wrong_world ? "wrong" : "native_world_digest"}'}`); if (method === "media.images.close") return close_gate.promise; if (method === "worlds.close") return fixture_result(realm, "{id:'world_a',revision:4,status:{state:'closed'}}"); throw Error("unexpected fixture method"); }, (method) => allow_control || !method.endsWith(".close")); // Exact response schemas and a controlled close ACK.
    host_seed(env, "{host:{bundle:{id:'asset_a'},permissions:[]},services:[{id:'world_a',kind:'worlds',revision:3,status:{state:'ready'}}],frame:null}"); env.evaluate("globalThis.asset=__ilium_host.assets.bundle;"); const read = await env.evaluate("__ilium_host.assets.read({grant:asset,relative_path:'one',max_bytes:1})"); assert.equal(read.ok, true); assert.deepEqual(Array.from(read.value.bytes), [7]); assert.deepEqual(Object.keys(env.context.asset), ["id"]); assert.equal(typeof env.context.asset.close, "undefined"); assert.strictEqual(calls[0].payload.grant, env.context.asset); assert.ok(calls[0].references.some(([handle, data]) => handle === env.context.asset && data.kind === "asset")); // Keep original options and opaque asset identity.
    const image = await env.evaluate("__ilium_host.media.images.decode({bytes:new Uint8Array([1]),max_pixels:4})"), world = await env.evaluate("__ilium_host.worlds.open({id:'world'})"); assert.equal(image.ok, true); assert.equal(world.ok, true); env.context.image = image.value; env.context.world = world.value; assert.deepEqual(Object.keys(image.value).sort(), ["format","height","id","sha256","width"]); assert.equal(typeof image.value.close, "undefined"); assert.equal(world.value.identity, "native_world_digest"); assert.throws(() => env.evaluate("'use strict';world.identity='forged'"), /read only|readonly|Cannot assign/); // New status may omit but cannot replace world identity.
    const before = calls.length; assert.equal(world.value.status().state, "ready"); const forged = await env.evaluate("__ilium_host.assets.read({grant:{id:'asset_a'},relative_path:'one',max_bytes:1})"); assert.equal(forged.ok, false); assert.equal(calls.length, before); const region = await env.evaluate("__ilium_host.worlds.region({world,x:0,y:0,z:0,width:1,height:1,depth:2,max_bytes:4})"); assert.equal(region.ok, true); assert.equal(types.isUint16Array(region.value.blocks), true); wrong_world = true; assert.equal((await env.evaluate("__ilium_host.worlds.region({world,x:0,y:0,z:0,width:1,height:1,depth:1,max_bytes:2})")).ok, false); // Status performs no RPC; region identity must match.
    allow_control = false; assert.throws(() => env.evaluate("__ilium_host.media.images.close(image)"), /host_request_rejected/); await settle_controls(); allow_control = true; env.evaluate("__ilium_host.media.images.close(image);__ilium_host.media.images.close(image)"); assert.equal(calls.filter((entry) => entry.method === "media.images.close").length, 1); close_gate.resolve({ ok: true, value: null }); await settle_controls(); env.evaluate("__ilium_host.media.images.close(image)"); assert.equal(calls.filter((entry) => entry.method === "media.images.close").length, 1); // Refusal throws; admitted close is pending-idempotent.
    env.evaluate("world.close()"); await settle_controls(); assert.equal(world.value.status().state, "closed"); const closed_count = calls.length; env.evaluate("world.close()"); assert.equal(calls.length, closed_count); // Only native closure changes observed lifecycle state.
}); // Native image, asset and world authority remain unqualified.
test("stream and compute handles serialize reads and cache only an admitted retrieval", async () => { // The math artifact separately uses the real bank and MathHandle.
    const stream_gate = Promise.withResolvers(), compute_gate = Promise.withResolvers(); let stream_reads = 0, compute_reads = 0, submissions = 0, allow_result = false; const env = environment((method, payload, references, realm) => { if (method === "http.stream") return fixture_result(realm, "{id:'stream_a',revision:1,status:{state:'ready'}}"); if (method === "http.stream.next") { stream_reads += 1; return stream_reads === 1 ? stream_gate.promise : { ok: true, value: null }; } if (method === "compute.submit") { submissions += 1; assert.strictEqual(payload.input, realm.context.input); return fixture_result(realm, "{id:'compute_a',revision:1,status:{state:'preparing'}}"); } if (method === "compute.result") { compute_reads += 1; return compute_gate.promise; } if (method === "http.close") return fixture_result(realm, "{id:'stream_a',revision:2,status:{state:'closed'}}"); if (method === "compute.close") return fixture_result(realm, "{id:'compute_a',revision:2,status:{state:'closed'}}"); throw Error("unexpected fixture method"); }, (method) => method !== "compute.result" || allow_result); // Fixture arrays prove no native CPU execution.
    env.evaluate("globalThis.input=new Float32Array([0,1,2,3]).subarray(1,3)"); const stream = await env.evaluate("__ilium_host.http.stream({url:'https://example.org/'})"), compute = await env.evaluate("__ilium_host.compute.submit({kernel:'fft',input,parameters:{},max_bytes:64,timeout_ms:10})"); assert.equal(stream.ok, true); assert.equal(compute.ok, true); env.context.stream = stream.value; env.context.job = compute.value; const first = env.evaluate("stream.next()"); assert.equal((await env.evaluate("stream.next()")).error.code, "read_pending"); assert.equal(stream_reads, 1); stream_gate.resolve(fixture_result(env, "{data:'record',event:'event',id:'1'}")); assert.equal((await first).value.data, "record"); assert.equal((await env.evaluate("stream.next()")).value, null); assert.equal((await env.evaluate("stream.next()")).value, null); assert.equal(stream_reads, 2); // Cache EOF only after the outstanding read completes.
    assert.equal((await env.evaluate("job.result()")).error.code, "fixture_refused"); assert.equal(compute_reads, 0); allow_result = true; const left = env.evaluate("job.result()"), right = env.evaluate("job.result()"); assert.equal(compute_reads, 1); compute_gate.resolve(fixture_result(env, "new Float32Array([0.25,0.75])")); const left_result = await left, right_result = await right; assert.equal(left_result.ok, true); assert.strictEqual(left_result.value, right_result.value); assert.strictEqual((await env.evaluate("job.result()")).value, left_result.value); assert.equal(compute_reads, 1); assert.equal(submissions, 1); assert.equal(types.isFloat32Array(left_result.value), true); // Retry refusal; reuse one admitted result retrieval.
    assert.equal(compute.value.status().state, "preparing"); host_seed(env, "{services:[{id:'compute_a',kind:'compute',revision:2,status:{state:'ready'},latest:{}},{id:'stream_a',kind:'http.stream',revision:2,status:{state:'ready'},latest:{}}],frame:null}"); assert.equal((await env.evaluate("job.result()")).error.code, "invalid_result"); assert.equal((await env.evaluate("stream.next()")).error.code, "invalid_result"); assert.equal(compute_reads, 1); assert.equal(stream_reads, 2); host_seed(env, "{services:[],frame:null}"); env.evaluate("stream.close();job.close()"); await settle_controls(); assert.equal(stream.value.status().state, "closed"); assert.equal(compute.value.status().state, "closed"); assert.equal((await env.evaluate("stream.next()")).error.code, "closed_handle"); assert.equal((await env.evaluate("job.result()")).error.code, "closed_handle"); assert.deepEqual(Array.from(left_result.value), [0.25,0.75]); // Delivered aliases survive independent handle closure.
}); // No native deadline, join or bank-release proof here.
test("video and source observations are native snapshots and void pause has an admission contract", async () => { // Synchronous video/source observers perform no RPC.
    const calls = []; let allow_pause = false; const pause_gate = Promise.withResolvers(); const env = environment((method, payload, references, realm) => { calls.push(method); if (method === "media.video.open") return fixture_result(realm, "{id:'video_a',revision:1,status:{state:'ready'},latest:{image:{id:'video_image',width:1,height:1,format:'rgba8',sha256:'c'.repeat(64)},time_seconds:1,revision:1}}"); if (method === "sources.series.open") return fixture_result(realm, "{id:'series_a',revision:1,status:{state:'ready'},latest:{revision:1,available:true,provider:'iss',points:[],verification:'not_applicable',attribution:'fixture'}}"); if (method === "media.video.pause") return pause_gate.promise; if (method === "media.video.seek") return fixture_result(realm, "{id:'video_a',revision:2,status:{state:'ready'},latest:null}"); if (method === "media.video.close") return fixture_result(realm, "{id:'video_a',revision:3,status:{state:'closed'}}"); if (method === "sources.series.close") return fixture_result(realm, "{id:'series_a',revision:3,status:{state:'closed'}}"); throw Error("unexpected fixture method"); }, (method) => method !== "media.video.pause" || allow_pause); // Use only explicit native observations.
    const video = await env.evaluate("__ilium_host.media.video.open({url:'https://example.org/v',max_pixels:1,max_fps:1})"), series = await env.evaluate("__ilium_host.sources.series.open({provider:'iss',max_samples:2,interval_ms:1000})"); assert.equal(video.ok, true); assert.equal(series.ok, true); env.context.video = video.value; env.context.series = series.value; const before = calls.length; assert.equal(video.value.latest().value.image.id, "video_image"); assert.equal(series.value.latest().value.revision, 1); assert.equal(video.value.status().state, "ready"); assert.equal(series.value.status().state, "ready"); assert.equal(calls.length, before); // Status and latest remain synchronous.
    assert.throws(() => env.evaluate("video.pause(true)"), /host_request_rejected/); await settle_controls(); allow_pause = true; env.evaluate("video.pause(true)"); assert.throws(() => env.evaluate("video.pause(false)"), /handle_control_pending/); assert.equal(video.value.status().state, "ready"); pause_gate.resolve({ ok: true, value: null }); await settle_controls(); assert.equal(video.value.status().state, "ready"); assert.equal(video.value.latest().value.time_seconds, 1); // Null ACK cannot invent paused or closed status.
    assert.equal((await env.evaluate("video.seek(-1)")).error.code, "invalid_request"); assert.equal((await env.evaluate("video.seek(2)")).ok, true); assert.equal(video.value.latest().value, null); host_seed(env, "{services:[{id:'video_a',kind:'media.video',revision:1,status:{state:'ready'},latest:null},{id:'series_a',kind:'sources.series',revision:2,status:{state:'ready'},latest:{revision:2,available:false,points:[]}}],frame:null}"); assert.equal(series.value.latest().value.revision, 2); const observed = calls.length; seed(env, descriptor()); make(env); assert.equal(series.value.latest().value.available, false); assert.equal(video.value.status().state, "ready"); assert.equal(calls.length, observed); acknowledge(env, finish(env), false); // Apply newer seeded observations during render without RPC.
    host_seed(env, "{services:[{id:'video_a',kind:'media.video',revision:3,status:{state:'ready'},latest:{image:{id:'invalid',width:0,height:1,format:'rgba8',sha256:'a'.repeat(64)},time_seconds:2,revision:3}}],frame:null}"); const before_invalid = calls.length; assert.equal((await env.evaluate("video.seek(3)")).error.code, "invalid_result"); assert.equal(calls.length, before_invalid); host_seed(env, "{services:[],frame:null}"); env.evaluate("video.close();series.close()"); await settle_controls(); assert.equal(video.value.status().state, "closed"); assert.equal(series.value.status().state, "closed"); assert.equal(video.value.latest().error.code, "closed_handle"); assert.equal(series.value.latest().error.code, "closed_handle"); // Native closed revisions retire facade caches.
}); // Native decoder/feed ownership remains external.
test("HTTP polling returns ServiceHandle and serializes structured callbacks", async () => { // One outstanding pull; no fixture timer.
    const pull = Promise.withResolvers(); let reads = 0; const env = environment((method, payload, references, realm) => { if (method === "http.poll") return fixture_result(realm, "{id:'poll_a',revision:1,status:{state:'ready'}}"); if (method === "http.poll.next") { reads += 1; return reads === 1 ? pull.promise : { ok: true, value: null }; } if (method === "http.close") return fixture_result(realm, "{id:'poll_a',revision:2,status:{state:'closed'}}"); throw Error("unexpected fixture method"); }); // Outer null is EOF; inner errors notify the callback.
    const opened = await env.evaluate("globalThis.notifications=[];__ilium_host.http.poll({url:'https://example.org/',response:'json',interval_ms:1000},function(value){'use strict';if(this!==undefined)throw Error('private_this');notifications.push(value);})"); assert.equal(opened.ok, true); assert.equal(typeof opened.value.close, "function"); assert.equal(typeof opened.value.status, "function"); assert.equal(reads, 1); pull.resolve({ ok: true, value: { ok: false, error: { code: "rate_limited", message: "Native provider floor", retry_after_ms: 1000 } } }); await settle_controls(); assert.equal(env.context.notifications.length, 1); assert.equal(env.context.notifications[0].error.code, "rate_limited"); assert.equal(env.context.notifications[0].error.retry_after_ms, 1000); assert.equal(reads, 2); assert.equal(opened.value.status().state, "ready"); // No private callback this; EOF is not native closure.
    opened.value.close(); await settle_controls(); assert.equal(opened.value.status().state, "closed"); assert.equal((await env.evaluate("__ilium_host.tasks.poll({interval_ms:1,deadline_ms:1},()=>Promise.resolve())")).ok, false); assert.equal((await env.evaluate("__ilium_host.presentation.subscribe(null)")).error.code, "invalid_callback"); // Async presentation rejects invalid callbacks without native acquisition.
}); // Actual Delivery and emission authority remain unqualified.
test("failed nested handle publication rolls back local custody and newest closure skips unpublished images", async () => { // Check the fixed 64-slot local wrapper bound.
    let sequence = 0, malformed = true; const env = environment((method, payload, references, realm) => { sequence += 1; if (method === "media.images.decode") return fixture_result(realm, `{id:'kept_${sequence}',width:1,height:1,format:'rgba8',sha256:'d'.repeat(64)}`); if (method === "sources.weather.open") return fixture_result(realm, `{id:'weather_${sequence}',revision:1,status:{state:'ready'},latest:{revision:1,available:true,layers:[{name:'first',tiles:[{image:{id:'nested_${sequence}',width:1,height:1,format:'rgba8',sha256:'e'.repeat(64)}}],bounds:{west:0,south:0,east:1,north:1},epoch_ms:0},{name:'second',tiles:[{image:{id:'bad_${sequence}',width:${malformed ? 0 : 1},height:1,format:'rgba8',sha256:'f'.repeat(64)}}],bounds:{west:0,south:0,east:1,north:1},epoch_ms:0}],attribution:'fixture'}}`); throw Error("unexpected fixture method"); }); // Fail after registering one unpublished nested image.
    const kept = await env.evaluate("__ilium_host.media.images.decode({bytes:new Uint8Array([1]),max_pixels:1})"); for (let index = 0; index < 70; index += 1) { const failed = await env.evaluate("__ilium_host.sources.weather.open({layers:[]})"); assert.equal(failed.error.code, "invalid_result"); } malformed = false; const recovered = await env.evaluate("__ilium_host.sources.weather.open({layers:[]})"); assert.equal(recovered.ok, true); assert.equal(recovered.value.latest().value.layers.length, 2); assert.equal(kept.value.width, 1); // Rollback new custody; preserve published wrappers.
    let images = 0; const closed = environment((method, payload, references, realm) => { if (method === "media.images.decode") { images += 1; return fixture_result(realm, `{id:'image_${images}',width:1,height:1,format:'rgba8',sha256:'a'.repeat(64)}`); } return fixture_result(realm, "{id:'closed_source',revision:1,status:{state:'ready'},latest:{revision:1,available:true,layers:[{name:'never',tiles:[{image:{id:'unpublished',width:1,height:1,format:'rgba8',sha256:'b'.repeat(64)}}],bounds:{},epoch_ms:0}],attribution:'fixture'}}"); }); for (let index = 0; index < 63; index += 1) assert.equal((await closed.evaluate("__ilium_host.media.images.decode({bytes:new Uint8Array([1]),max_pixels:1})")).ok, true); // Retain 63 live wrappers.
    host_seed(closed, "{services:[{id:'closed_source',kind:'sources.weather',revision:2,status:{state:'closed'}}],frame:null}"); const terminal = await closed.evaluate("__ilium_host.sources.weather.open({layers:[]})"); assert.equal(terminal.ok, true); assert.equal(terminal.value.status().state, "closed"); assert.equal(terminal.value.latest().error.code, "closed_handle"); assert.equal((await closed.evaluate("__ilium_host.media.images.decode({bytes:new Uint8Array([1]),max_pixels:1})")).ok, true); assert.equal((await closed.evaluate("__ilium_host.media.images.decode({bytes:new Uint8Array([1]),max_pixels:1})")).error.code, "handle_limit"); // Newest closure skips stale images and releases its slot.
}); // Native failed-publication cleanup remains separately owned.
test("prototype mutation cannot expose private inventory, handle arrays or async envelopes", async () => { // Protect captured intrinsics and private state.
    const env = environment((method, payload, references, realm) => fixture_result(realm, "{id:'prototype_source',revision:1,status:{state:'ready'},latest:null}")); env.evaluate("globalThis.frame=undefined;globalThis.leaked=0;for(const name of ['returned','frame','work_cell_rgb','cell_rgb'])Object.defineProperty(Object.prototype,name,{set(){leaked+=1;throw Error('private_state');},configurable:true});Object.defineProperty(Object.prototype,'then',{get(){leaked+=1;return undefined;},configurable:true});Object.defineProperty(Object.prototype,'return',{get(){leaked+=1;throw Error('iterator_close');},configurable:true});"); // Inherited setters/getters must not obtain private objects.
    assert.throws(() => env.evaluate("'use strict';Array.prototype.constructor=function(){throw Error('species')}"), /read only|readonly|Cannot assign/); assert.throws(() => env.evaluate("'use strict';Object.getPrototypeOf([][Symbol.iterator]()).next=()=>({done:true})"), /read only|readonly|Cannot assign/); assert.throws(() => host_seed(env, "{services:[{id:'bad',kind:'not_native',revision:1,status:{state:'ready'}}],frame:null}"), /native_snapshot_kind/); host_seed(env, "{services:[],frame:null}"); const opened = await env.evaluate("__ilium_host.sources.series.open({provider:'iss',max_samples:1,interval_ms:1000})"); assert.equal(opened.ok, true); seed(env, descriptor("mask8", "retain", true)); make(env); const inventory = env.native(4, () => env.context.__ilium_frame_buffers(env.context.frame)); assert.equal(Object.keys(inventory).length, 12); assert.equal(env.context.leaked, 0); env.evaluate("frame.present()"); acknowledge(env, finish(env), true); assert.equal(env.context.leaked, 0); // Retain complete planes without exposing private state.
}); // Rust tests separately qualify native ingress and authority.

test("context random matches native mix64, survives frames and guards retained callbacks", () => {
    const env = environment(); seed(env, descriptor()); make(env);
    const expected = [0.8833108082136426, 0.5665615751722809, 0.5911897341980794];
    assert.deepEqual(Array.from(env.evaluate("[frame.context.random.next(),frame.context.random.next(),frame.context.random.next()]")), expected);
    assert.equal(env.evaluate("frame.context.random.integer(4,4)"), 4);
    assert.throws(() => env.evaluate("frame.context.random.integer(3,2)"), /random_integer_bounds/);
    env.evaluate("globalThis.retained_random=frame.context.random;frame.present();");
    acknowledge(env, finish(env), true);
    assert.throws(() => env.evaluate("retained_random.next()"), /closed_frame/);
    seed(env, descriptor("gray8", "retain", false, "1", "2")); make(env);
    assert.equal(env.evaluate("frame.context.random.next()"), 0.43145581774497377);
    env.evaluate("frame.present()"); acknowledge(env, finish(env), true);
});
test("occlusion helpers derive pixels from cells and expire at handoff", () => {
    const env = environment(), d = descriptor(); d.input_specs=[{path:"occlusion.cells",kind:"u8",elements:6}];
    seed(env,d,null,",input_0:new Uint8Array([1,0,0,0,0,0])");make(env);
    assert.equal(env.evaluate("frame.context.inputs.occlusion.is_cell_blocked(0,0)"),true);
    assert.equal(env.evaluate("frame.context.inputs.occlusion.is_pixel_blocked(1,3)"),true);
    assert.equal(env.evaluate("frame.context.inputs.occlusion.is_pixel_blocked(2,3)"),false);
    assert.equal(env.evaluate("frame.context.inputs.occlusion.is_cell_blocked(-1,0)"),false);
    assert.throws(()=>env.evaluate("frame.context.inputs.occlusion.is_pixel_blocked(0.5,0)"),/occlusion_coordinates/);
    env.evaluate("globalThis.retained_mask=frame.context.inputs.occlusion;frame.present()");acknowledge(env,finish(env),true);
    assert.throws(()=>env.evaluate("retained_mask.is_cell_blocked(0,0)"),/closed_frame/);
});
test("occlusion helpers derive cells from requested pixel plane only", () => {
    const env = environment(), d = descriptor(); d.input_specs=[{path:"occlusion.pixels",kind:"u8",elements:48}];
    seed(env,d,null,",input_0:Uint8Array.from({length:48},(_,i)=>i===47?1:0)");make(env);
    assert.equal(env.evaluate("frame.context.inputs.occlusion.is_cell_blocked(2,1)"),true);
    assert.equal(env.evaluate("frame.context.inputs.occlusion.is_cell_blocked(1,1)"),false);
    assert.equal(env.evaluate("frame.context.inputs.occlusion.is_pixel_blocked(5,7)"),true);
    env.evaluate("frame.present()");acknowledge(env,finish(env),true);
});

test("permission observations expose actual epoch and scope without inventing asset handles", () => {
    const env = environment();
    host_seed(env, "{frame:null,host:{permissions:[{request_id:'pointer',id:'input.pointer',scope:{kind:'animation_viewport'},epoch:7}]}}");
    const grant = env.evaluate("__ilium_host.permissions.get('pointer')");
    assert.equal(grant.epoch, 7); assert.equal(grant.scope.kind, 'animation_viewport');
    assert.equal(grant.handle, undefined); assert.equal(Object.isFrozen(grant), true);
    assert.equal(env.evaluate("__ilium_host.permissions.has('input.pointer')"), true);
    host_seed(env, "{frame:null,host:{permissions:[{request_id:'pointer',id:'input.pointer',scope:{kind:'animation_viewport'}}]}}");
    assert.equal(env.evaluate("__ilium_host.permissions.has('pointer')"), false);
    host_seed(env, "{frame:null,host:{permissions:[{request_id:'disk',id:'disk.read',scope:{kind:'disk',slot:'selected',selection:'file'},epoch:7,binding:{opaque_identity:'unregistered'}}]}}");
    assert.equal(env.evaluate("__ilium_host.permissions.get('disk').handle"), undefined);
});


test("world-frame preparation preserves genuine source and refuses forged or mismatched resources", async () => {
    let requests=0,wrong=false;
    const env=environment((method,payload,references,realm)=>{
        if(method==='worlds.open')return fixture_result(realm,"{id:'world_a',kind:'worlds',revision:1,status:{state:'ready'},identity:'"+'a'.repeat(64)+"'}");
        if(method==='worlds.frame'){
            requests++;assert.strictEqual(payload.world,env.context.world);
            assert.ok(references.some(([handle,data])=>handle===env.context.world&&data.kind==='worlds'));
            return fixture_result(realm,"{id:'frame_a',kind:'worlds.frame',revision:1,status:{state:'ready'},world_id:'world_a',source_identity:'"+(wrong?'b':'a').repeat(64)+"',width:6,height:8}");
        }
        if(method==='worlds.frame.close')return fixture_result(realm,"{id:'frame_a',kind:'worlds.frame',revision:2,status:{state:'closed'}}");
        throw Error('unexpected method '+method);
    });
    env.context.world=(await env.evaluate("__ilium_host.worlds.open({id:'fixture'})")).value;
    const resource=await env.evaluate("__ilium_host.worlds.frame({world,width:3,height:2,time:0,wall:0})");
    assert.equal(resource.ok,true);assert.equal(resource.value.world_id,'world_a');assert.equal(resource.value.width,6);assert.equal(resource.value.height,8);assert.equal(Object.isFrozen(resource.value),true);
    assert.equal(resource.value.status().state,'ready');assert.equal(requests,1);
    const forged=await env.evaluate("__ilium_host.worlds.frame({world:{id:'world_a',identity:'"+'a'.repeat(64)+"'},width:3,height:2,time:0,wall:0})");
    assert.equal(forged.ok,false);assert.equal(requests,1);
    resource.value.close();await settle_controls();assert.equal(resource.value.status().state,'closed');
    wrong=true;assert.equal((await env.evaluate("__ilium_host.worlds.frame({world,width:3,height:2,time:0,wall:0})")).ok,false);
});
test("world-frame dimensions and source metadata are immutable native observations", async () => {
    let shape={width:6,height:8};
    const env=environment((method,_payload,_refs,realm)=>{
        if(method==='worlds.open')return fixture_result(realm,"{id:'world_a',revision:1,status:{state:'ready'},identity:'"+'a'.repeat(64)+"'}");
        if(method==='worlds.frame')return fixture_result(realm,"{id:'frame_a',kind:'worlds.frame',revision:1,status:{state:'ready'},world_id:'world_a',source_identity:'"+'a'.repeat(64)+"',width:"+shape.width+",height:"+shape.height+"}");
        throw Error('unexpected method');
    });
    env.context.world=(await env.evaluate("__ilium_host.worlds.open({id:'fixture'})")).value;
    const prepared=await env.evaluate("__ilium_host.worlds.frame({world,width:3,height:2,time:0,wall:0})");assert.equal(prepared.ok,true);
    env.context.prepared=prepared.value;assert.throws(()=>env.evaluate("'use strict';prepared.width=8"),/read only|readonly|Cannot assign/);
    shape={width:7,height:8};assert.equal((await env.evaluate("__ilium_host.worlds.frame({world,width:3,height:2,time:0,wall:0})")).ok,false);
    host_seed(env,"{frame:null,services:[{id:'frame_a',kind:'worlds.frame',revision:2,status:{state:'closed'}}]}");
    assert.equal(prepared.value.status().state,'closed');
});

test("world-frame preparation rejects malformed native metadata without publishing a wrapper", async () => {
    let replacement={};
    const env=environment((method,_payload,_references,realm)=>{
        if(method==='worlds.open')return fixture_result(realm,"{id:'world_a',revision:1,status:{state:'ready'},identity:'"+'a'.repeat(64)+"'}");
        if(method==='worlds.frame')return fixture_result(realm,JSON.stringify({id:'frame_a',kind:'worlds.frame',revision:1,status:{state:'ready'},world_id:'world_a',source_identity:'a'.repeat(64),width:6,height:8,...replacement}));
        throw Error('unexpected fixture method');
    });
    env.context.world=(await env.evaluate("__ilium_host.worlds.open({id:'fixture'})")).value;
    for(const malformed of [{width:0},{width:482},{height:404},{height:7},{source_identity:'guest-label'},{world_id:'world_b'},{kind:'image'},{status:{state:'guest-ready'}}]){
        replacement=malformed;
        const result=await env.evaluate("__ilium_host.worlds.frame({world,width:3,height:2,time:0,wall:0})");
        assert.equal(result.ok,false,JSON.stringify(malformed));
    }
    replacement={};assert.equal((await env.evaluate("__ilium_host.worlds.frame({world,width:3,height:2,time:0,wall:0})")).ok,true);
});

test("world-frame preparation propagates native refusal and performs no synchronous render acquisition", async () => {
    let requests=0;
    const env=environment((method,_payload,_references,realm)=>{
        if(method==='worlds.open')return fixture_result(realm,"{id:'world_a',revision:1,status:{state:'ready'},identity:'"+'a'.repeat(64)+"'}");
        requests++;return realm.evaluate("({ok:false,error:{code:'source_preparing',message:'Original source has no complete raster yet.'}})");
    });
    env.context.world=(await env.evaluate("__ilium_host.worlds.open({id:'fixture'})")).value;
    const refused=await env.evaluate("__ilium_host.worlds.frame({world,width:3,height:2,time:0,wall:0})");
    assert.equal(refused.ok,false);assert.equal(refused.error.code,'source_preparing');assert.equal(requests,1);
    seed(env,descriptor('gray32','replace'));make(env);
    const during_render=await env.evaluate("__ilium_host.worlds.frame({world,width:3,height:2,time:0,wall:0})");
    assert.equal(during_render.ok,false);assert.equal(requests,1);
    env.evaluate('frame.present()');acknowledge(env,finish(env),true);
});

test("native world-frame presentation encodes the same blit command in all seven formats", async () => {
    for(const format of ['mask8','mono1','mono8','gray8','gray32','rgb8','rgba8']){
        let requests=0;
        const env=environment((method,_payload,_references,realm)=>{
            requests++;
            if(method==='worlds.open')return fixture_result(realm,"{id:'world_a',revision:1,status:{state:'ready'},identity:'"+'a'.repeat(64)+"'}");
            if(method==='worlds.frame')return fixture_result(realm,"{id:'frame_a',kind:'worlds.frame',revision:1,status:{state:'ready'},world_id:'world_a',source_identity:'"+'a'.repeat(64)+"',width:6,height:8}");
            throw Error('unexpected fixture method');
        });
        env.context.world=(await env.evaluate("__ilium_host.worlds.open({id:'fixture'})")).value;
        env.context.prepared=(await env.evaluate("__ilium_host.worlds.frame({world,width:3,height:2,time:0,wall:0})")).value;
        seed(env,descriptor(format,'replace'));make(env);
        const result=env.evaluate("__ilium_host.presentation.blit_source({frame,source:world,world_frame:prepared,rectangle:{unit:'cells',x:0,y:0,width:3,height:2}})");
        assert.equal(result.ok,true);assert.equal(requests,2);
        env.evaluate('frame.present()');const output=finish(env);
        assert.equal(output.metadata.error,null);assert.equal(output.metadata.commands.length,1);
        const command=output.metadata.commands[0];assert.equal(command.kind,'blit');assert.equal(command.handle,'frame_a');assert.equal(command.blend,'overwrite');
        assert.equal(command.source.width,6);assert.equal(command.source.height,8);assert.equal(command.target.width,format==='mask8'?3:6);assert.equal(command.target.height,format==='mask8'?2:8);
        acknowledge(env,output,true);
    }
});

test("forged world-frame presentation rejects and poisons the complete frame transaction", async () => {
    const env=environment((method,_payload,_references,realm)=>{
        if(method==='worlds.open')return fixture_result(realm,"{id:'world_a',revision:1,status:{state:'ready'},identity:'"+'a'.repeat(64)+"'}");
        throw Error('unexpected fixture method');
    });
    env.context.world=(await env.evaluate("__ilium_host.worlds.open({id:'fixture'})")).value;
    seed(env,descriptor('gray32','replace'));make(env);
    assert.equal(env.evaluate("__ilium_host.presentation.blit_source({frame,source:world,world_frame:{id:'frame_a',world_id:'world_a'},rectangle:{unit:'cells',x:0,y:0,width:3,height:2}})").ok,false);
    assert.throws(()=>env.evaluate('frame.present()'),/closed_frame/);
    const output=finish(env);assert.equal(output.metadata.error,'frame_operation_failed');assert.equal(output.metadata.commands.length,0);acknowledge(env,output,false);
});

const opened_descriptor = "{id:'receipt_1',kind:'presentation',revision:1,status:{state:'ready'}}";
const valid_receipt = "{frame_id:'frame_1',source_identity:'source_1',composition_revision:3,emitted_dots:5,owners:[{token:'dot_1',dots:2},{token:'dot_2',dots:1}]}";
async function drain_fixture() { for (let index = 0; index < 48; index += 1) await Promise.resolve(); }
test("subscription rejects invalid callback through the declared async Result contract", async () => {
    let requests = 0;
    const env = environment(() => { requests += 1; throw Error('unexpected'); });
    const result = await env.evaluate("__ilium_host.presentation.subscribe(null)");
    assert.equal(result.ok, false); assert.equal(result.error.code, 'invalid_callback'); assert.equal(requests, 0);
});
test("native subscription refusal does not fabricate a handle or callback", async () => {
    const methods = []; let callbacks = 0;
    const env = environment(method => { methods.push(method); return {ok:false,error:{code:'unavailable',message:'native adapter unavailable'}}; });
    env.context.callback = () => { callbacks += 1; };
    const result = await env.evaluate("__ilium_host.presentation.subscribe(callback)");
    assert.equal(result.ok, false); assert.deepEqual(methods, ['presentation.subscribe']); assert.equal(callbacks, 0);
});
test("one native receipt pull and awaited callback preserve bounded serialization", async () => {
    const methods = []; let release_receipt, release_callback, observed;
    const env = environment((method, payload, references, realm) => {
        methods.push(method);
        if (method === 'presentation.subscribe') return fixture_result(realm, opened_descriptor);
        if (method === 'presentation.next' && methods.filter(x => x === method).length === 1) return new Promise(resolve => { release_receipt = () => resolve(fixture_result(realm, valid_receipt)); });
        if (method === 'presentation.next') return fixture_result(realm, 'null');
        throw Error('unexpected');
    });
    env.context.callback = receipt => { observed = receipt; return new Promise(resolve => { release_callback = resolve; }); };
    const opened = await env.evaluate("__ilium_host.presentation.subscribe(callback)");
    assert.equal(opened.ok, true); assert.equal(opened.value.id, 'receipt_1');
    assert.deepEqual(methods, ['presentation.subscribe', 'presentation.next']);
    release_receipt(); await drain_fixture();
    assert.equal(observed.emitted_dots, 5); assert.equal(observed.owners[0].dots, 2);
    assert.equal(Object.isFrozen(observed), true); assert.equal(Object.isFrozen(observed.owners), true); assert.equal(Object.isFrozen(observed.owners[0]), true);
    assert.equal(methods.filter(x => x === 'presentation.next').length, 1);
    release_callback(); await drain_fixture();
    assert.equal(methods.filter(x => x === 'presentation.next').length, 2);
    assert.equal(opened.value.status().state, 'closed');
});
test("closing subscription prevents late callback without local closure fabrication", async () => {
    const methods = []; let release_receipt, release_close, calls = 0;
    const env = environment((method, payload, references, realm) => {
        methods.push(method);
        if (method === 'presentation.subscribe') return fixture_result(realm, opened_descriptor);
        if (method === 'presentation.next') return new Promise(resolve => { release_receipt = () => resolve(fixture_result(realm, valid_receipt)); });
        if (method === 'presentation.close') return new Promise(resolve => { release_close = () => resolve(fixture_result(realm, "{id:'receipt_1',kind:'presentation',revision:2,status:{state:'closed'}}")); });
        throw Error('unexpected');
    });
    env.context.callback = () => { calls += 1; };
    const opened = await env.evaluate("__ilium_host.presentation.subscribe(callback)");
    opened.value.close(); opened.value.close();
    assert.equal(opened.value.status().state, 'ready');
    release_receipt(); await drain_fixture(); assert.equal(calls, 0);
    assert.equal(methods.filter(x => x === 'presentation.close').length, 1);
    release_close(); await drain_fixture(); assert.equal(opened.value.status().state, 'closed');
});
test("callback failure closes the original native subscription", async () => {
    const methods = []; let callbacks = 0;
    const env = environment((method, payload, references, realm) => {
        methods.push(method);
        if (method === 'presentation.subscribe') return fixture_result(realm, opened_descriptor);
        if (method === 'presentation.next') return fixture_result(realm, valid_receipt);
        if (method === 'presentation.close') return fixture_result(realm, "{id:'receipt_1',kind:'presentation',revision:2,status:{state:'closed'}}");
        throw Error('unexpected');
    });
    env.context.callback = async () => { callbacks += 1; throw Error('script callback failed'); };
    const opened = await env.evaluate("__ilium_host.presentation.subscribe(callback)"); await drain_fixture();
    assert.equal(callbacks, 1); assert.deepEqual(methods, ['presentation.subscribe','presentation.next','presentation.close']);
    assert.equal(opened.value.status().state, 'closed');
});
test("malformed and accessor receipts never invoke callbacks and close native custody", async () => {
    const bad = ["{...valid,extra:true}", "{...valid,frame_id:''}", "{...valid,composition_revision:1.5}", "{...valid,emitted_dots:1048577}", "{...valid,owners:[{token:'a',dots:4},{token:'b',dots:4}]}", "{...valid,owners:[{token:'a',dots:1},{token:'a',dots:1}]}", "{...valid,owners:[{token:'a',dots:0}]}", "{...valid,owners:[{token:'a',dots:1,extra:true}]}", "{...valid,get frame_id(){getter_calls+=1;return 'forged'}}", "{...valid,owners:Array(1)}"];
    for (const expression of bad) {
        const methods = []; let callbacks = 0;
        const env = environment((method, payload, references, realm) => {
            methods.push(method);
            if (method === 'presentation.subscribe') return fixture_result(realm, opened_descriptor);
            if (method === 'presentation.next') return fixture_result(realm, expression);
            if (method === 'presentation.close') return fixture_result(realm, "{id:'receipt_1',kind:'presentation',revision:2,status:{state:'closed'}}");
            throw Error('unexpected');
        });
        env.evaluate("globalThis.getter_calls=0;globalThis.valid="+valid_receipt);
        env.context.callback = () => { callbacks += 1; };
        const opened = await env.evaluate("__ilium_host.presentation.subscribe(callback)"); await drain_fixture();
        assert.equal(callbacks, 0, expression); assert.equal(env.context.getter_calls, 0, expression);
        assert.deepEqual(methods, ['presentation.subscribe','presentation.next','presentation.close'], expression);
        assert.equal(opened.value.status().state, 'closed', expression);
    }
});

test("native seed closure retires subscription before a late receipt resolves", async () => {
    let deliver; let callbacks = 0; const methods = [];
    const env = environment((method, payload, references, realm) => {
        methods.push(method);
        if (method === 'presentation.subscribe') return fixture_result(realm, opened_descriptor);
        if (method === 'presentation.next') return new Promise(resolve => { deliver = () => resolve(fixture_result(realm, valid_receipt)); });
        throw Error('unexpected');
    });
    env.context.callback = () => { callbacks += 1; };
    const opened = await env.evaluate("__ilium_host.presentation.subscribe(callback)");
    host_seed(env, "{services:[{id:'receipt_1',kind:'presentation',revision:2,status:{state:'closed'}}],frame:null}");
    assert.equal(opened.value.status().state, 'closed');
    deliver(); await drain_fixture(); assert.equal(callbacks, 0);
    assert.deepEqual(methods, ['presentation.subscribe','presentation.next']);
});
test("receipt delivered during acknowledgement cannot invoke script or admit close", async () => {
    let deliver; let callbacks = 0; const methods = [];
    const env = environment((method, payload, references, realm) => {
        methods.push(method);
        if (method === 'presentation.subscribe') return fixture_result(realm, opened_descriptor);
        if (method === 'presentation.next') return new Promise(resolve => { deliver = () => resolve(fixture_result(realm, valid_receipt)); });
        throw Error('unexpected');
    });
    env.context.callback = () => { callbacks += 1; };
    const opened = await env.evaluate("__ilium_host.presentation.subscribe(callback)");
    env.phase(4); deliver(); await drain_fixture();
    assert.equal(callbacks, 0); assert.equal(opened.value.status().state, 'ready');
    assert.deepEqual(methods, ['presentation.subscribe','presentation.next']);
});

test("packed full-size source-owner receipt becomes one immutable public callback", async () => {
    let callbacks = 0, next = 0;
    const env = environment((method, payload, references, realm) => {
        if (method === 'presentation.subscribe') return fixture_result(realm, opened_descriptor);
        if (method === 'presentation.next') {
            next += 1;
            if (next > 1) return fixture_result(realm, "null");
            return fixture_result(realm, `{
                frame_id:'native_frame', source_identity:'native_source',
                composition_revision:9, emitted_dots:8192,
                owners:{token_prefix:'source-owner-18446744073709500000-',
                    ids:Uint32Array.from({length:8192},(_,i)=>i+1),
                    dots:new Uint32Array(8192).fill(1)}
            }`);
        }
        if (method === 'presentation.close') return fixture_result(realm,
            "{id:'receipt_1',kind:'presentation',revision:2,status:{state:'closed'}}");
        throw Error('unexpected native method');
    });
    env.context.callback = receipt => {
        callbacks += 1;
        assert.equal(receipt.owners.length, 8192);
        assert.equal(receipt.owners[0].token, 'source-owner-18446744073709500000-1');
        assert.equal(receipt.owners[8191].token, 'source-owner-18446744073709500000-8192');
        assert.equal(receipt.owners[8191].dots, 1);
        assert.equal(receipt.emitted_dots, 8192);
        assert(Object.isFrozen(receipt));
        assert(Object.isFrozen(receipt.owners));
        assert(Object.isFrozen(receipt.owners[8191]));
    };
    const opened = await env.evaluate('__ilium_host.presentation.subscribe(callback)');
    assert.equal(opened.ok, true);
    await drain_fixture();
    assert.equal(callbacks, 1, 'full native owner receipt must reach exactly one callback');
    assert.equal(opened.value.status().state, 'closed');
});

test("packed malformed owner planes close native subscription without invoking script", async () => {
    const variants = [
        "{...packed,ids:new Uint16Array([1])}",
        "{...packed,ids:new Uint32Array([8193])}",
        "{...packed,ids:new Uint32Array([0])}",
        "{...packed,ids:new Uint32Array([1,1]),dots:new Uint32Array([1,1])}",
        "{...packed,dots:new Uint32Array(0)}",
        "{...packed,dots:new Uint32Array([0])}",
        "{...packed,dots:new Uint32Array([3])}",
        "{...packed,token_prefix:'source-owner-18446744073709551616-'}",
        "{...packed,token_prefix:'source-owner-0-'}",
        "{...packed,token_prefix:'forged-1-'}",
        "{...packed,extra:true}",
        "{...packed,get ids(){getter_calls+=1;return new Uint32Array([1])}}"
    ];
    for (const variant of variants) {
        let callbacks = 0, closes = 0, pulls = 0;
        const env = environment((method, payload, references, realm) => {
            if (method === 'presentation.subscribe') return fixture_result(realm, opened_descriptor);
            if (method === 'presentation.next') {
                pulls += 1;
                if (pulls > 1) return fixture_result(realm, 'null');
                return fixture_result(realm,
                    "{frame_id:'f',source_identity:'s',composition_revision:1,emitted_dots:2,owners:"+variant+"}");
            }
            if (method === 'presentation.close') {
                closes += 1;
                return fixture_result(realm,"{id:'receipt_1',kind:'presentation',revision:2,status:{state:'closed'}}");
            }
            throw Error('unexpected native method');
        });
        env.evaluate("globalThis.getter_calls=0;globalThis.packed={token_prefix:'source-owner-1-',ids:new Uint32Array([1]),dots:new Uint32Array([1])}");
        env.context.callback = () => { callbacks += 1; };
        const opened = await env.evaluate('__ilium_host.presentation.subscribe(callback)');
        await drain_fixture();
        assert.equal(callbacks, 0, variant);
        assert.equal(closes, 1, variant);
        assert.equal(env.context.getter_calls, 0, variant);
        assert.equal(opened.value.status().state, 'closed', variant);
    }
});

test("packed receipt copies original owner projection before input alias mutation", async () => {
    let callbacks = 0, pulls = 0, received;
    const env = environment((method, payload, references, realm) => {
        if (method === 'presentation.subscribe') return fixture_result(realm, opened_descriptor);
        if (method === 'presentation.next') {
            pulls += 1;
            if (pulls > 1) return fixture_result(realm, 'null');
            return fixture_result(realm,"{frame_id:'f',source_identity:'s',composition_revision:1,emitted_dots:1,owners:{token_prefix:'source-owner-1-',ids:native_ids,dots:native_dots}}");
        }
        throw Error('unexpected native method');
    });
    env.evaluate('globalThis.native_ids=new Uint32Array([1]);globalThis.native_dots=new Uint32Array([1])');
    env.context.callback = receipt => {
        callbacks += 1; received = receipt;
        env.evaluate('native_ids[0]=8193;native_dots[0]=9');
    };
    await env.evaluate('__ilium_host.presentation.subscribe(callback)');
    await drain_fixture();
    assert.equal(callbacks, 1);
    assert.equal(received.owners[0].token, 'source-owner-1-1');
    assert.equal(received.owners[0].dots, 1);
    assert(Object.isFrozen(received.owners[0]));
});

test('replay freeze refuses invalid byte bounds before native dispatch', async () => {
    let calls = 0;
    const env = environment(() => {
        calls += 1;
        return { __proto__: null, ok: true, value: { __proto__: null, recording_id: 'fixture-recording', sha256: 'a'.repeat(64) } };
    });
    for (const expression of ['undefined', '0', '-1', '1.5', 'NaN', 'Infinity', '9007199254740992']) {
        const result = await env.evaluate(`__ilium_host.replay.freeze({sources:[],max_bytes:${expression}})`);
        assert.equal(result.ok, false, `invalid capture bound ${expression} must refuse`);
    }
    const unexpected = await env.evaluate('__ilium_host.replay.freeze({sources:[],max_bytes:1024,extra:true})');
    assert.equal(unexpected.ok, false);
    assert.equal(calls, 0, 'invalid replay capture options must not reach native dispatch');
});

test('replay freeze validates the native recording identity without inventing one', async () => {
    // Facade-only synthetic replies; no recording/source custody qualification.
    let reply = { __proto__: null, recording_id: 'fixture-recording', sha256: 'a'.repeat(64) };
    let calls = 0;
    const env = environment((method, payload) => {
        assert.equal(method, 'replay.freeze');
        assert.equal(payload.max_bytes, 1024);
        calls += 1;
        return { __proto__: null, ok: true, value: reply };
    });
    const valid = await env.evaluate('__ilium_host.replay.freeze({sources:[],max_bytes:1024})');
    assert.equal(valid.ok, true);
    assert.equal(valid.value.recording_id, 'fixture-recording');
    assert.equal(valid.value.sha256, 'a'.repeat(64));
    assert(Object.isFrozen(valid.value));
    for (const value of [{}, { recording_id: '', sha256: 'a'.repeat(64) }, { recording_id: 'fixture-recording', sha256: 'wrong' }, { recording_id: 'fixture-recording', sha256: 'a'.repeat(64), extra: true }]) {
        reply = { __proto__: null, ...value };
        const result = await env.evaluate('__ilium_host.replay.freeze({sources:[],max_bytes:1024})');
        assert.equal(result.ok, false, 'malformed native recording reply must refuse');
    }
    assert.equal(calls, 5);
});

test('source-sequence capture refuses invalid bounds before native dispatch', async () => {
    let calls = 0;
    const env = environment((method, payload, references, realm) => {
        if (method === 'sources.chess.open') return fixture_result(realm, "{id:'native_chess_feed',kind:'sources.chess',revision:1,status:{state:'ready'},latest:{revision:1,available:true,fen:'fixture'}}");
        calls += 1;
        assert.equal(method, 'replay.capture_sequence');
        return { __proto__: null, ok: true, value: { __proto__: null, recording_id: 'fixture-sequence', sha256: 'b'.repeat(64), frame_count: 2 } };
    });
    const opened = await env.evaluate("__ilium_host.sources.chess.open({game_id:'tv',max_hz:1})");
    assert.equal(opened.ok, true);
    env.context.chess_feed = opened.value;
    for (const [field, invalid] of [
        ['duration_ms', '0'], ['duration_ms', '120001'], ['sample_hz', '0'], ['sample_hz', '61'],
        ['max_frames', '0'], ['max_frames', '513'], ['max_bytes', '0'], ['max_bytes', '32000001'],
    ]) {
        const values = { duration_ms: '12000', sample_hz: '1', max_frames: '16', max_bytes: '8000000', [field]: invalid };
        const result = await env.evaluate(`__ilium_host.replay.capture_sequence({sources:[chess_feed],duration_ms:${values.duration_ms},sample_hz:${values.sample_hz},max_frames:${values.max_frames},max_bytes:${values.max_bytes}})`);
        assert.equal(result.ok, false, `invalid sequence bound ${field}=${invalid} must refuse`);
    }
    assert.equal((await env.evaluate("__ilium_host.replay.capture_sequence({sources:[],duration_ms:12000,sample_hz:1,max_frames:16,max_bytes:8000000})")).ok, false);
    assert.equal((await env.evaluate("__ilium_host.replay.capture_sequence({sources:[chess_feed,chess_feed],duration_ms:12000,sample_hz:1,max_frames:16,max_bytes:8000000})")).ok, false);
    assert.equal((await env.evaluate("__ilium_host.replay.capture_sequence({sources:[chess_feed],duration_ms:12000,sample_hz:1,max_frames:16,max_bytes:8000000,extra:true})")).ok, false);
    assert.equal(calls, 0, 'invalid sequence options must not reach native dispatch');
});

test('source-sequence capture validates bounded native receipts', async () => {
    let reply = { __proto__: null, recording_id: 'fixture-sequence', sha256: 'b'.repeat(64), frame_count: 2 };
    let calls = 0;
    const env = environment((method, payload, references, realm) => {
        if (method === 'sources.chess.open') return fixture_result(realm, "{id:'native_chess_feed',kind:'sources.chess',revision:1,status:{state:'ready'},latest:{revision:1,available:true,fen:'fixture'}}");
        assert.equal(method, 'replay.capture_sequence');
        assert.equal(payload.duration_ms, 12_000);
        calls += 1;
        return { __proto__: null, ok: true, value: reply };
    });
    const opened = await env.evaluate("__ilium_host.sources.chess.open({game_id:'tv',max_hz:1})");
    assert.equal(opened.ok, true);
    env.context.chess_feed = opened.value;
    const valid = await env.evaluate("__ilium_host.replay.capture_sequence({sources:[chess_feed],duration_ms:12000,sample_hz:1,max_frames:16,max_bytes:8000000})");
    assert.equal(valid.ok, true);
    assert.equal(valid.value.recording_id, 'fixture-sequence');
    assert.equal(valid.value.sha256, 'b'.repeat(64));
    assert.equal(valid.value.frame_count, 2);
    assert(Object.isFrozen(valid.value));
    for (const value of [
        {}, { recording_id: '', sha256: 'b'.repeat(64), frame_count: 2 },
        { recording_id: 'fixture-sequence', sha256: 'wrong', frame_count: 2 },
        { recording_id: 'fixture-sequence', sha256: 'b'.repeat(64), frame_count: 0 },
        { recording_id: 'fixture-sequence', sha256: 'b'.repeat(64), frame_count: 17 },
        { recording_id: 'fixture-sequence', sha256: 'b'.repeat(64), frame_count: 2, extra: true },
    ]) {
        reply = { __proto__: null, ...value };
        const result = await env.evaluate("__ilium_host.replay.capture_sequence({sources:[chess_feed],duration_ms:12000,sample_hz:1,max_frames:16,max_bytes:8000000})");
        assert.equal(result.ok, false, 'malformed sequence receipt must refuse');
    }
    assert.equal(calls, 7);
});
