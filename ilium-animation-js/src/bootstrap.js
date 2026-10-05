(() => { // Trusted bootstrap runs before package evaluation; it imports no browser/Node APIs.
    "use strict"; // Never create accidental global authority.
    const native_bigint = BigInt, native_number = Number; // Capture conversion before package mutation.
    let context_random_state = null; // One versioned reproducible stream per helper instance.
    const global = globalThis; // Capture the embedding realm once.
    const { create, freeze, defineProperty: define_property, getOwnPropertyDescriptor: descriptor, getPrototypeOf: prototype, setPrototypeOf: set_prototype, keys, preventExtensions: prevent_extensions } = Object; // Capture primordials before package code.
    const apply = Reflect.apply, own_keys = Reflect.ownKeys, stringify = JSON.stringify, parse = JSON.parse, is_array = Array.isArray, integer = Number.isSafeInteger, finite = Number.isFinite; // Capture every structural operation before package code can replace it.
    const u8 = Uint8Array, f32 = Float32Array, u16 = Uint16Array, u32 = Uint32Array, array_buffer = ArrayBuffer, error_type = Error; // Only these typed kinds are supported.
    const set_type = Set, weak_map_type = WeakMap, string_type = String, max_integer = Number.MAX_SAFE_INTEGER, ceil = Math.ceil, floor = Math.floor, round = Math.round, abs = Math.abs, min = Math.min, max = Math.max; // Never reread mutable collection, string, or geometry bindings during handoff.
    const constructors = freeze({ u8, f32, u16, u32 }), sizes = freeze({ u8: 1, f32: 4, u16: 2, u32: 4 }); // Match the frozen engine's ArraySpec kinds.
    const names = freeze({ u8: "Uint8Array", f32: "Float32Array", u16: "Uint16Array", u32: "Uint32Array" }); // Branded typed-array validation.
    const typed = prototype(u8.prototype), typed_set = typed.set, typed_fill = typed.fill; // Intrinsics operate on validated typed arrays.
    const object_prototype = Object.prototype, array_prototype = Array.prototype; // Retain exact native JSON-container prototypes independently of mutable global constructors.
    const array_iterator_prototype = prototype([][Symbol.iterator]()); define_property(array_iterator_prototype, "return", { __proto__: null, value: undefined }); // Shield abrupt iterator close from an inherited package-controlled return getter.
    for (let iterator = array_iterator_prototype; iterator && iterator !== object_prototype; iterator = prototype(iterator)) freeze(iterator); // Freeze iterator next/identity lookup before a package can inject callbacks into trusted native-seed loops.
    for (const value of [array_prototype, String.prototype, RegExp.prototype, Set.prototype, WeakMap.prototype, Promise.prototype, typed, u8.prototype, f32.prototype, u16.prototype, u32.prototype]) freeze(value); // Protect captured collection operations and native Promise constructor lookup from package mutation.
    const get_buffer = descriptor(typed, "buffer").get, get_length = descriptor(typed, "length").get, get_offset = descriptor(typed, "byteOffset").get, get_kind = descriptor(typed, Symbol.toStringTag).get; // Do not trust shadow properties.
    const buffer_length = descriptor(array_buffer.prototype, "byteLength").get, buffer_resizable = descriptor(array_buffer.prototype, "resizable")?.get, buffer_detached = descriptor(array_buffer.prototype, "detached")?.get; // Verify physical handoff when the engine returns.
    const native_dispatch = global.__ilium_dispatch, native_phase = global.__ilium_service_phase, native_project = global.__ilium_geography_project, native_observe = global.__ilium_astronomy_observe, native_text_measure = global.__ilium_text_measure, weak_get = WeakMap.prototype.get, weak_set = WeakMap.prototype.set, frames = new weak_map_type(), handles = new weak_map_type(); // Keep genuine frame/handle brands and sealed native entrypoints private.
    require_value(typeof native_dispatch === "function" && typeof native_phase === "function" && typeof native_project === "function" && typeof native_observe === "function" && typeof native_text_measure === "function" && global.__ilium_service_wire_version === 1, "native_service_wire_version"); // Require the actual binary service boundary instead of silently falling back to JSON.
    const max_cells = 131072, max_meta = 65536, max_commands = 256, max_edits = 4194304, max_handoff = 67108864; // Match surface.rs exactly.
    let seed = null, active = null, awaiting = null, pending_rpc = 0, opening_handles = 0, bundle_handle = null, seeding = false; // Frame lifetime, pending RPCs, and native-handle materialization have separate local state.
    let ambient_configured = false, ambient_mode = null, ambient_ms = 0; // Native launch selects deterministic ambient behavior before guest modules load.
    const handle_entries = [], service_kinds = freeze(["http.stream", "http.poll", "media.video", "compute", "worlds", "sources.series", "sources.earthquakes", "sources.aircraft", "sources.boats", "sources.chess", "sources.weather", "tasks.poll"]); let service_snapshots = freeze(create(null)); // Bound live wrapper references and accept only the exact native service-handle inventory.
    let status_records = [], status_bytes = 0, status_dropped = 0; // Bounded informational diagnostics; never a service request or drawing command.
    let host_info = freeze(create(null)); // Informational metadata starts empty, never default-allow.
    function fail(code) { throw new error_type(code); } // Static bounded error codes avoid retaining arbitrary user strings.
    function require_value(condition, code) { if (!condition) fail(code); } // Guard clauses keep failures explicit.
    function utf8_length(text) { let bytes = 0; for (let i = 0; i < text.length; i += 1) { const c = text.charCodeAt(i); if (c < 128) bytes += 1; else if (c < 2048) bytes += 2; else if (c >= 55296 && c <= 56319 && i + 1 < text.length && text.charCodeAt(i + 1) >= 56320 && text.charCodeAt(i + 1) <= 57343) { bytes += 4; i += 1; } else bytes += 3; } return bytes; } // No TextEncoder dependency.
    function append_data(array, value) { define_property(array, string_type(array.length), { __proto__: null, value, writable: true, configurable: true, enumerable: true }); } // Own data writes cannot invoke numeric setters inherited from a mutable Object.prototype.
    function remove_entry(state) { const index = handle_entries.indexOf(state); if (index < 0) return; for (let position = index; position + 1 < handle_entries.length; position += 1) define_property(handle_entries, string_type(position), { __proto__: null, value: handle_entries[position + 1], writable: true, configurable: true, enumerable: true }); handle_entries.length -= 1; } // Avoid species constructors and proxy callbacks that would expose private state through splice results.
    function discard_unpublished_handles(first) { while (handle_entries.length > first) handle_entries.length -= 1; } // Roll back only newly appended local wrapper custody; this does not acknowledge native closure or alter an existing handle state.
    function record(value, allowed) { // Read own data properties without invoking user accessors.
        require_value(value !== null && typeof value === "object" && !is_array(value), "object_required"); // Exclude primitives and arrays.
        const result = create(null), list = keys(value); require_value(list.length <= 64, "object_key_limit"); // Bound descriptor work.
        for (const key of list) { require_value(!["__proto__", "prototype", "constructor"].includes(key) && (!allowed || allowed.includes(key)), "unknown_field"); const item = descriptor(value, key); require_value(item && descriptor(item, "value"), "accessor_forbidden"); result[key] = item.value; } // No inherited authority.
        return result; // Typed-array-valued fields are validated separately.
    } // Proxy traps can fail/retire the caller, never authorize native effects.
    function json_copy(value, budget = { nodes: 0, bytes: 0 }, depth = 0) { // Strict bounded JSON for frame and informational metadata only.
        require_value(depth <= 16 && ++budget.nodes <= 8192, "json_structure_limit"); // Bound traversal before stringification.
        if (value === null || typeof value === "boolean") return value; // Exact JSON primitives.
        if (typeof value === "number") { require_value(finite(value), "nonfinite_json"); return value; } // No NaN/infinity coercion.
        if (typeof value === "string") { budget.bytes += utf8_length(value) + 2; require_value(budget.bytes <= max_meta, "json_byte_limit"); return value; } // Check strings before copying.
        if (is_array(value)) { require_value(value.length <= 2048, "json_array_limit"); const result = []; define_property(result, "toJSON", { __proto__: null, value: undefined }); for (let i = 0; i < value.length; i += 1) { const item = descriptor(value, string_type(i)); require_value(item && descriptor(item, "value"), "sparse_or_accessor_array"); append_data(result, json_copy(item.value, budget, depth + 1)); } return result; } // Arrays remain iterable but cannot inherit toJSON.
        const source = record(value), result = create(null); for (const key of keys(source).sort()) { budget.bytes += utf8_length(key) + 2; require_value(budget.bytes <= max_meta, "json_byte_limit"); result[key] = json_copy(source[key], budget, depth + 1); } return result; // Canonical key order, no user toJSON.
    } // Binary frame baselines never pass through this function.
    function json_text(value) { const text = stringify(json_copy(value)); require_value(utf8_length(text) <= max_meta, "json_byte_limit"); return text; } // Bound escaped JSON as well as raw strings.
    function frozen_json(value) { const copy = json_copy(value); const visit = (item) => { if (item && typeof item === "object") { for (const key of keys(item)) visit(item[key]); freeze(item); } }; visit(copy); return copy; } // Deeply immutable informational snapshots.
    function number(value, maximum = max_integer) { require_value(integer(value) && value >= 0 && value <= maximum, "integer_range"); return value; } // Never coerce coordinates or dimensions.
    function identity(value, zero = false) { require_value(typeof value === "string" && /^(0|[1-9][0-9]{0,19})$/.test(value) && (zero || value !== "0") && (value.length < 20 || value <= "18446744073709551615"), "identity"); return value; } // Exact decimal u64 transport.
    function layout(shape_value) { // Allocate only the accepted format and declared optional colour plane.
        const shape = record(shape_value, ["cell_width", "cell_height", "mode", "format", "update", "cell_rgb", "colour_space"]); // Complete shape schema.
        const w = number(shape.cell_width, 4096), h = number(shape.cell_height, 4096); require_value(w > 0 && h > 0 && w * h <= max_cells, "dimensions"); // Match Rust's hard cell bound.
        require_value(["mask8", "mono1", "mono8", "gray8", "gray32", "rgb8", "rgba8"].includes(shape.format) && ["replace", "retain"].includes(shape.update) && ["srgb", "linear"].includes(shape.colour_space) && typeof shape.cell_rgb === "boolean", "shape"); // No unknown formats/default authority.
        require_value((shape.mode === "cells" && shape.format === "mask8") || (shape.mode === "pixels" && shape.format !== "mask8"), "mode"); require_value(!shape.cell_rgb || !["rgb8", "rgba8"].includes(shape.format), "redundant_cell_rgb"); // One authoritative representation.
        const cells = w * h, width = shape.mode === "cells" ? w : w * 2, height = shape.mode === "cells" ? h : h * 4, channels = shape.format === "rgb8" ? 3 : shape.format === "rgba8" ? 4 : 1; // Checked small dimensions.
        const row = shape.format === "mono1" ? ceil(width / 8) : width * channels, kind = shape.format === "gray32" ? "f32" : "u8", elements = row * height, samples = width * height; // mono1 rows have independent padding.
        const handoff_bytes = 2 * (elements * sizes[kind] + samples * 5 + (shape.cell_rgb ? cells * 8 : 0)); require_value(handoff_bytes <= max_handoff, "frame_byte_limit"); // Include working and sealed planes.
        return { shape: frozen_json(shape), cells, width, height, channels, row, kind, elements, samples, handoff_bytes }; // Parent admission remains stricter authority.
    } // No always-RGBA intermediate or JSON pixel array is created.
    function view(value, kind, elements, exclusive = false) { // Validate branded type and exact logical length.
        require_value(kind in constructors && apply(get_kind, value, []) === names[kind] && apply(get_length, value, []) === elements, "typed_plane_shape"); // Reject wrong kinds and detached nonempty views.
        const buffer = apply(get_buffer, value, []), bytes = apply(buffer_length, buffer, []); require_value(!buffer_resizable || !apply(buffer_resizable, buffer, []), "resizable_buffer"); // Shared buffers fail the ArrayBuffer brand check.
        if (exclusive) require_value(apply(get_offset, value, []) === 0 && bytes === elements * sizes[kind], "oversized_or_offset_view"); // Engine-owned frame planes use entire exact buffers.
        return value; // Bulk row sources may be bounded subviews; they are copied immediately.
    } // Native independently repeats all ArraySpec checks.
    function alloc(kind, elements) { return prevent_extensions(new constructors[kind](elements)); } // Only called after layout/input budget checks.
    function set(target, source) { apply(typed_set, target, [source]); } // Captured typed-array copy intrinsic.
    function own(value, key, required = true) { const item = descriptor(value, key), field = item && descriptor(item, "value"); require_value(field || !required, "own_data_required"); return field ? field.value : undefined; } // Never accept an inherited descriptor value or invoke a property accessor.
    function result_ok(value) { return freeze({ __proto__: null, ok: true, value }); } // Async return envelopes cannot inherit a then getter from Object.prototype.
    function error_result(code, message = code) { return freeze({ __proto__: null, ok: false, error: freeze({ __proto__: null, code, message }) }); } // Every public failure supplies the complete SDK HostError shape.
    function host_error(value) { const source = record(value, ["code", "message", "retry_after_ms"]); require_value(typeof source.code === "string" && source.code.length > 0 && utf8_length(source.code) <= 128 && typeof source.message === "string" && utf8_length(source.message) <= 4096, "invalid_host_error"); if (source.retry_after_ms !== undefined) number(source.retry_after_ms); return freeze(source); } // Native error metadata stays bounded and informational.
    function service_result(value) { require_value(value !== null && typeof value === "object" && !is_array(value), "invalid_result"); const ok = own(value, "ok"); require_value(typeof ok === "boolean", "invalid_result"); return ok ? result_ok(own(value, "value")) : freeze({ __proto__: null, ok: false, error: host_error(own(value, "error")) }); } // Preserve native binary result views without JSON parsing or a second byte copy.
    function acquiring() { const phase = native_phase(); return !active && !awaiting && (phase === 1 || phase === 2); } // Native phase, including module/plan/seed/acknowledgement, is authoritative for acquisition.
    function reference_table() { const result = []; for (const state of handle_entries) append_data(result, [state.handle, state.projection]); return result; } // Only private branded wrapper identities are replaced by untrusted kind/id records in native traversal.
    function dispatch_request(method, payload) { // Call the concrete binary ABI with the complete original caller tree.
        require_value(acquiring(), "service_phase"); require_value(pending_rpc < 64, "backpressure"); pending_rpc += 1; // This local hard ceiling supplements the engine's actual configured limits.
        try { const references = reference_table(), promise = references.length ? native_dispatch(method, payload, references) : native_dispatch(method, payload); const item = descriptor(promise, "__ilium_admitted"); require_value(item && descriptor(item, "value") && typeof item.value === "boolean" && item.writable === false && item.configurable === false && item.enumerable === false, "native_admission_receipt"); return { __proto__: null, promise, admitted: item.value }; } catch (error) { pending_rpc -= 1; throw error; } // The receipt confirms queue admission only; native registries and broker still authorize every effect.
    } // No JS traversal or handle rewrite can launder the original payload before native validation.
    async function settle_request(request) { try { return service_result(await request.promise); } catch { return error_result("host_error", "Native service result was rejected or malformed."); } finally { pending_rpc -= 1; } } // Native payload leases remain independent of this local Promise counter.
    async function rpc(method, payload = create(null)) { if (!acquiring()) return error_result(active || awaiting ? "render_phase" : "service_phase", "Service acquisition requires native create or async phase."); try { return await settle_request(dispatch_request(method, payload)); } catch { return error_result("host_request_rejected", "Native request admission was rejected."); } } // Typed arrays and every original proxy/accessor reach the native boundary unchanged.
    function binary(value, kinds) { const kind = apply(get_kind, value, []); require_value(kinds.some((entry) => names[entry] === kind), "binary_kind"); const selected = kinds.find((entry) => names[entry] === kind); view(value, selected, apply(get_length, value, [])); require_value(!buffer_detached || !apply(buffer_detached, apply(get_buffer, value, []), []), "detached_binary"); return value; } // Validate SDK leaf types while the engine independently rejects all unstable or forged views.
    function request_binary(options, field, kinds, optional = false) { const value = own(options, field, !optional); if (value !== undefined) binary(value, kinds); return options; } // Validation never substitutes a clone for the caller's original options.
    function native_json(value, budget = { nodes: 0 }, depth = 0) { // Freeze native-returned JSON metadata without adding toJSON properties or copying binary arrays.
        require_value(depth <= 32 && ++budget.nodes <= 4096, "native_snapshot_structure"); if (value === null || typeof value === "string" || typeof value === "boolean") return value; if (typeof value === "number") { require_value(finite(value), "native_snapshot_number"); return value; } // Native ingress already admitted encoded bytes before creating these values.
        require_value(value && typeof value === "object", "native_snapshot_type"); const is_list = is_array(value), expected = is_list ? array_prototype : object_prototype; require_value(prototype(value) === expected || (!is_list && prototype(value) === null), "native_snapshot_prototype"); const list = own_keys(value); require_value(list.length <= 4097, "native_snapshot_keys"); // Typed arrays, functions, symbols, and exotic prototypes are not retained in frame-independent JSON snapshots.
        for (const key of list) { require_value(typeof key === "string", "native_snapshot_symbol"); if (is_list && key === "length") continue; require_value(!["__proto__", "prototype", "constructor"].includes(key), "native_snapshot_key"); native_json(own(value, key), budget, depth + 1); } return freeze(value); // A native service result remains reusable as strict data because no synthetic array properties are added.
    } // Durable typed results use binary() and remain charged by the native V8 allocator.
    function handle_id(value) { require_value(typeof value === "string" && /^[a-zA-Z0-9_.:-]{1,128}$/.test(value), "native_handle_id"); return value; } // Opaque identifiers remain untrusted data requiring the current native registry and broker.
    function lookup_handle(value, kinds) { const state = apply(weak_get, handles, [value]); require_value(state && kinds.includes(state.kind), "foreign_handle"); refresh_handle(state); require_value(!state.closed, "closed_handle"); return state; } // Copied IDs, wrong kinds, proxies, and wrappers from another bootstrap cannot pass the ergonomic brand check.
    function handle_field(options, field, kinds, optional = false) { const value = own(options, field, !optional); if (value !== undefined) lookup_handle(value, kinds); return options; } // Leave the original options and genuine wrapper identity for native substitution.
    function service_status(value) { const source = record(value, ["state", "error"]); require_value(["preparing", "ready", "closed", "error"].includes(source.state), "native_service_status"); if (source.error !== undefined) source.error = host_error(source.error); return freeze(source); } // No local queue event is promoted to a native lifecycle state.
    function find_handle(kind, id) { return handle_entries.find((entry) => entry.kind === kind && entry.id === id); } // The retained active inventory is bounded to 64 entries.
    function remove_handle(state) { remove_entry(state); state.closed = true; state.close_requested = true; state.callback = null; state.result_promise = null; state.latest = undefined; } // Only authenticated native closed state releases this active wrapper slot.
    function image_handle(value) { // Materialize only a native-returned image identity and validated immutable metadata.
        const source = record(value, ["id", "kind", "width", "height", "format", "sha256"]), id = handle_id(source.id); require_value((source.kind === undefined || source.kind === "image") && /^[a-zA-Z0-9_.-]{1,128}$/.test(id), "native_image_handle_kind_or_id"); number(source.width, 16777216); number(source.height, 16777216); require_value(source.width > 0 && source.height > 0 && source.format === "rgba8" && typeof source.sha256 === "string" && /^[a-f0-9]{64}$/.test(source.sha256), "native_image_metadata"); // Image pixels and provenance remain in the native registry.
        const previous = find_handle("image", id); if (previous) { require_value(previous.handle.width === source.width && previous.handle.height === source.height && previous.handle.sha256 === source.sha256, "native_image_identity_reused"); return previous.handle; } // Stable native identity reuses the same local wrapper.
        const handle = freeze({ __proto__: null, id, width: source.width, height: source.height, format: source.format, sha256: source.sha256 }); register_handle("image", id, handle); return handle; // Public image records contain no release or status methods outside the SDK.
    } // Native image closure is exposed only through host.media.images.close(image).
    function asset_handle(value) { const source = record(value, ["id", "kind"]), id = handle_id(source.id); require_value(source.kind === undefined || source.kind === "asset", "native_handle_kind"); const previous = find_handle("asset", id); if (previous) return previous.handle; const handle = freeze({ __proto__: null, id }); register_handle("asset", id, handle); return handle; } // A literal bundle label or copied permission request cannot manufacture an AssetHandle.
    function register_handle(kind, id, handle) { require_value(handle_entries.length + opening_handles < 64, "handle_limit"); const state = { kind, id, handle, projection: freeze({ __proto__: null, id, kind }), revision: -1, snapshot: null, latest: undefined, closed: false, close_requested: false, control_pending: false, read_pending: false, result_promise: null, eof: false, callback: null }; append_data(handle_entries, state); apply(weak_set, handles, [handle, state]); return state; } // The strong registry is bounded; old closed wrappers retain only their own local state through WeakMap reachability.
    function source_latest(kind, value) { // Interpret only native snapshots; frame seed buffers are never cached here.
        if (value === null) return null; const source = record(value); number(source.revision); require_value(typeof source.available === "boolean", "native_source_snapshot"); // Every source follows the SDK's common Snapshot prefix.
        if (kind === "sources.weather") {
            require_value(is_array(source.layers) && source.layers.length <= 64, "native_weather_layers");
            const first = handle_entries.length, layers = [];
            const convert_tiles = (tiles) => {
                require_value(is_array(tiles) && tiles.length <= 64, "native_weather_tiles");
                const converted = [];
                for (const tile of tiles) { const copy = record(tile); copy.image = image_handle(copy.image); append_data(converted, freeze(copy)); }
                return freeze(converted);
            };
            try {
                for (const layer of source.layers) {
                    const copy = record(layer);
                    if (copy.tiles !== undefined) copy.tiles = convert_tiles(copy.tiles);
                    if (copy.frames !== undefined) {
                        require_value(is_array(copy.frames) && copy.frames.length <= 64, "native_weather_frames");
                        const frames = [];
                        for (const frame of copy.frames) {
                            const item = record(frame);
                            if (item.image !== undefined) item.image = image_handle(item.image);
                            if (item.tiles !== undefined) item.tiles = convert_tiles(item.tiles);
                            append_data(frames, freeze(item));
                        }
                        copy.frames = freeze(frames);
                    }
                    require_value(copy.tiles !== undefined || copy.frames !== undefined, "native_weather_image_location");
                    append_data(layers, freeze(copy));
                }
                source.layers = freeze(layers);
                return native_json(source);
            } catch (error) { discard_unpublished_handles(first); throw error; }
        } // Match native satellite snapshots: direct night tiles, or cloud frames with an image or tiles.
        return native_json(value); // Series, geographic feeds, and chess retain authenticated immutable metadata.
    } // Freshness, revisions, provider floors, grants, and attribution remain native service responsibilities.
    function video_latest(value) { if (value === null) return null; const source = record(value, ["image", "time_seconds", "revision"]); require_value(typeof source.time_seconds === "number" && finite(source.time_seconds) && source.time_seconds >= 0, "native_video_time"); number(source.revision); source.image = image_handle(source.image); return freeze(source); } // Synchronous latest returns only an already registered native-decoded image snapshot.
    function apply_snapshot(state, value) { // Native result/seed revisions order local lifecycle and latest observations.
        const source = record(value, ["id", "kind", "revision", "status", "latest", "identity"]); require_value(handle_id(source.id) === state.id && (source.kind === undefined || source.kind === state.kind), "native_snapshot_identity"); number(source.revision); if (source.revision <= state.revision) return; // Ignore old or duplicate snapshots without reviving a handle.
        require_value(!state.closed, "native_closed_handle_reused"); const status = service_status(source.status); if (state.kind === "worlds") require_value(source.identity === undefined || source.identity === state.handle.identity, "native_world_identity_reused"); if (status.state === "closed") { state.revision = source.revision; state.snapshot = status; remove_handle(state); return; } // Authenticated closure drops the cache without creating image wrappers that cannot be published.
        let latest = state.latest; if (source.latest !== undefined) { require_value(state.kind === "media.video" || state.kind.startsWith("sources."), "unexpected_latest_snapshot"); latest = result_ok(state.kind === "media.video" ? video_latest(source.latest) : source_latest(state.kind, source.latest)); } state.revision = source.revision; state.snapshot = status; state.latest = latest; // Commit a complete valid snapshot atomically; queue admission and local callback errors never change native lifecycle state.
    } // Native closed acknowledgements retain no active handle slot, while actual native body custody remains external.
    function refresh_handle(state) { if (state.closed) return; const entry = service_snapshots[`${state.kind}/${state.id}`]; if (entry && entry.revision > state.revision) apply_snapshot(state, entry); } // A synchronous bounded lookup performs no RPC or implicit acquisition.
    function handle_status(state) { refresh_handle(state); require_value(state.snapshot, "native_status_unavailable"); return state.snapshot; } // Even preparing state must have been explicitly supplied by the native owner.
    function handle_latest(state) { try { refresh_handle(state); if (state.closed) return error_result("closed_handle", "The native handle is closed."); return state.latest ?? error_result("snapshot_unavailable", "The native service has not supplied a latest snapshot."); } catch { return error_result("invalid_result", "The native latest snapshot was malformed or exceeded the handle bound."); } } // No polling, decoding, or service acquisition occurs inside latest().
    function close_method(kind) { if (kind === "http.stream" || kind === "http.poll") return "http.close"; if (kind === "image") return "media.images.close"; return `${kind}.close`; } // The parent must route these explicit methods to actual native registries.
    async function finish_control(state, request, closing) { // Observe the actual native acknowledgement without blocking the SDK's void control.
        try { const result = await settle_request(request); if (!result.ok) { status_record("log", "warning", "Native handle control failed; its last native status is retained."); return; } if (state.kind === "image") { require_value(closing && result.value === null, "native_image_close_ack"); remove_handle(state); return; } if (result.value !== null) apply_snapshot(state, result.value); } catch { status_record("log", "error", "Native handle control acknowledgement was malformed."); } finally { state.control_pending = false; if (!state.closed) state.close_requested = false; } // Only actual native closed data or an image-close acknowledgement releases a wrapper.
    } // A rejected close is retryable; cancellation is never claimed to roll back an already issued body.
    function control_handle(state, method, payload, closing = false) { refresh_handle(state); if (state.closed) return; if (closing && state.close_requested) return; require_value(!state.control_pending, "handle_control_pending"); const request = dispatch_request(method, payload); if (!request.admitted) { void settle_request(request); fail("host_request_rejected"); } state.control_pending = true; state.close_requested = closing; void finish_control(state, request, closing); } // Throw synchronously when a void operation was not actually admitted to the native queue.
    function close_handle(state) { control_handle(state, close_method(state.kind), { __proto__: null, id: state.id, kind: state.kind }, true); } // Repeated close while admitted or after actual closure is a local no-op.
    async function stream_next(state) { try { refresh_handle(state); } catch { return error_result("invalid_result", "Native stream status snapshot was malformed."); } if (state.closed || state.close_requested) return error_result("closed_handle", "The stream is closing or closed."); if (state.eof) return result_ok(null); if (state.read_pending) return error_result("read_pending", "Only one stream read may be outstanding."); state.read_pending = true; try { const result = await rpc("http.stream.next", { __proto__: null, id: state.id, kind: state.kind }); if (!result.ok) return result; if (result.value === null) { state.eof = true; return result; } const value = record(result.value, ["data", "event", "id"]); require_value(typeof value.data === "string" && (value.event === undefined || typeof value.event === "string") && (value.id === undefined || typeof value.id === "string"), "native_stream_record"); return result_ok(freeze(value)); } catch { return error_result("invalid_result", "Native stream record was malformed."); } finally { state.read_pending = false; } } // A native EOF can be repeated locally without consuming another queue slot.
    async function read_compute(request) { const result = await settle_request(request); if (!result.ok) return result; try { return result_ok(binary(result.value, ["f32", "u8"])); } catch { return error_result("invalid_result", "Native compute returned an unsupported result type."); } } // Preserve actual float32/byte result buffers and their native V8 backing lifetime.
    async function compute_result(state) { try { refresh_handle(state); } catch { return error_result("invalid_result", "Native compute status snapshot was malformed."); } if (state.closed || state.close_requested) return error_result("closed_handle", "The compute handle is closing or closed."); if (state.result_promise) return await state.result_promise; try { const request = dispatch_request("compute.result", { __proto__: null, id: state.id, kind: state.kind }); const result = read_compute(request); if (request.admitted) state.result_promise = result; return await result; } catch { return error_result("host_request_rejected", "Native compute result retrieval was not admitted."); } } // A successfully admitted result retrieval is cached and never resubmits the native job.
    async function video_seek(state, time_seconds) { try { refresh_handle(state); } catch { return error_result("invalid_result", "Native video status snapshot was malformed."); } if (state.closed || state.close_requested) return error_result("closed_handle", "The video handle is closing or closed."); if (typeof time_seconds !== "number" || !finite(time_seconds) || time_seconds < 0) return error_result("invalid_request", "Video seek requires a finite nonnegative time."); const result = await rpc("media.video.seek", { __proto__: null, id: state.id, kind: state.kind, time_seconds }); if (!result.ok) return result; if (result.value !== null) { try { apply_snapshot(state, result.value); } catch { return error_result("invalid_result", "Native video seek snapshot was malformed."); } } return result_ok(null); } // Preserve the old latest image until a real newer native snapshot arrives.
    function service_handle(kind, value) { // Add exact SDK methods only to a native-issued, typed service descriptor.
        const source = record(value, ["id", "kind", "revision", "status", "latest", "identity"]), id = handle_id(source.id); require_value(source.kind === undefined || source.kind === kind, "native_handle_kind"); number(source.revision); service_status(source.status); const previous = find_handle(kind, id); if (previous) { apply_snapshot(previous, source); return previous.handle; } // One active native identity has one stable local wrapper.
        const handle = create(null); handle.id = id; if (kind === "worlds") { require_value(typeof source.identity === "string" && source.identity.length > 0 && utf8_length(source.identity) <= 256, "native_world_identity"); handle.identity = source.identity; } const first = handle_entries.length, state = register_handle(kind, id, handle); // Capture local unpublished custody before native snapshots can materialize dependent images.
        define_property(handle, "close", { __proto__: null, value: () => close_handle(state) }); define_property(handle, "status", { __proto__: null, value: () => handle_status(state) }); if (kind === "http.stream") define_property(handle, "next", { __proto__: null, value: async () => await stream_next(state) }); if (kind === "compute") define_property(handle, "result", { __proto__: null, value: async () => await compute_result(state) }); // Non-enumerable methods still require native identity substitution rather than JSON traversal.
        if (kind === "media.video" || kind.startsWith("sources.")) define_property(handle, "latest", { __proto__: null, value: () => handle_latest(state) }); if (kind === "media.video") { define_property(handle, "seek", { __proto__: null, value: async (time_seconds) => await video_seek(state, time_seconds) }); define_property(handle, "pause", { __proto__: null, value: (paused) => { require_value(typeof paused === "boolean", "video_pause_boolean"); control_handle(state, "media.video.pause", { __proto__: null, id, kind, paused }); } }); } // All synchronous observation methods use already supplied snapshots only.
        freeze(handle); try { const supplied = service_snapshots[`${kind}/${id}`], snapshot = supplied && supplied.revision > source.revision ? { __proto__: null, id: supplied.id, kind: supplied.kind, revision: supplied.revision, status: supplied.status, latest: supplied.latest === undefined ? source.latest : supplied.latest, identity: supplied.identity === undefined ? source.identity : supplied.identity } : source; apply_snapshot(state, snapshot); return handle; } catch (error) { discard_unpublished_handles(first); throw error; } // Materialize only the newest native observation once, preserving omitted latest/identity fields; failed publication drops only new local wrappers and never claims native closure.
    } // Native registry publication, operation cancellation, and physical resource release are required dispatcher integrations.
    async function open_rpc(method, options, kind, validate = null) { // Reserve local wrapper capacity before asking native code to create a handle.
        if (handle_entries.length + opening_handles >= 64) return error_result("handle_limit", "The bounded native-handle facade inventory is full."); opening_handles += 1; let reserved = true; try { if (validate) validate(options); const result = await rpc(method, options); if (!result.ok) return result; opening_handles -= 1; reserved = false; return result_ok(kind === "image" ? image_handle(result.value) : service_handle(kind, result.value)); } catch { return error_result("invalid_result", "Native handle creation or its bounded descriptor was rejected."); } finally { if (reserved) opening_handles -= 1; } // Native remains responsible for request-owned handles whose publication fails or whose instance retires.
    } // This is a facade admission bound, not a replacement storage quota or a native grant.
    function seed_services(source) { // Called only inside the native phase-3 seed hook, before package execution resumes.
        const entries = own(source, "services", false); if (entries === undefined) return; require_value(is_array(entries) && entries.length <= 64, "native_service_snapshot_count"); const next = create(null); for (const value of entries) { const entry = record(value, ["id", "kind", "revision", "status", "latest", "identity"]); handle_id(entry.id); require_value(service_kinds.includes(entry.kind) || entry.kind === "image", "native_snapshot_kind"); number(entry.revision); const status = service_status(entry.status); require_value(entry.kind !== "image" || (status.state === "closed" && entry.latest === undefined && entry.identity === undefined), "native_image_terminal_snapshot"); const key = `${entry.kind}/${entry.id}`; require_value(!descriptor(next, key), "duplicate_native_snapshot"); next[key] = native_json(entry); } service_snapshots = freeze(next); for (let index = handle_entries.length - 1; index >= 0; index -= 1) { const state = handle_entries[index]; if (state.kind === "tasks.poll" || state.kind === "image") refresh_handle(state); } // Apply authenticated task and image closure before seed ACK, so native terminal identities can be retired without losing wrapper observation.
    } // Missing entries preserve a wrapper's last authenticated observation and do not grant new identities.
    function seed_asset_handles(source) { const supplied_host = own(source, "host", false); if (supplied_host === undefined) return; const supplied_bundle = own(supplied_host, "bundle", false); if (supplied_bundle && typeof supplied_bundle === "object") bundle_handle = asset_handle(supplied_bundle); const grants = own(supplied_host, "permissions", false); if (grants === undefined) return; require_value(is_array(grants) && grants.length <= 64, "native_grant_count"); for (const grant of grants) { const handle = own(grant, "handle", false); if (handle && typeof handle === "object") asset_handle(handle); } } // Only genuinely native-seeded handle records are branded; accepted request descriptions and the legacy string bundle label remain informational.
    function guard(state) { require_value(state && active === state && !state.closed && !state.finished && !state.error, "closed_frame"); } // Drawing belongs to the one synchronous render callback.
    function next(state) { guard(state); require_value(state.order < max_edits, "edit_limit"); state.order += 1; return state.order; } // All ordinary/native commands share one order domain.
    function run(state, action) { try { guard(state); return action(); } catch (error) { state.error = "frame_operation_failed"; throw error; } } // A caught helper error still rejects the transaction.
    function rect(state, value, unit = null) { // Strict bounds; helpers do not silently clip source offsets.
        const r = record(value, ["unit", "x", "y", "width", "height", "colour"]); const chosen = unit ?? r.unit; require_value(chosen === "cells" || chosen === "pixels", "rectangle_unit"); // Caller states the coordinate space.
        let x = number(r.x), y = number(r.y), width = number(r.width), height = number(r.height); // Zero area is a legal no-op.
        if (chosen === "cells" && state.layout.shape.mode === "pixels") { x *= 2; width *= 2; y *= 4; height *= 4; } else require_value(chosen === state.layout.shape.mode, "inactive_representation"); // Cells expand to complete 2x4 blocks.
        require_value(x <= state.layout.width && y <= state.layout.height && width <= state.layout.width - x && height <= state.layout.height - y, "rectangle_bounds"); // Checked before loops.
        return { x, y, width, height, colour: r.colour }; // Damage/clear methods share exactly these coordinates.
    } // Native vectors have a separately documented clipped geometry contract.
    function sample_value(state, value) { // Scalar component units match Rust Data exactly.
        const format = state.layout.shape.format; let result; // Colour samples are plain objects only.
        if (format === "rgb8" || format === "rgba8") { const colour = record(value, format === "rgba8" ? ["r", "g", "b", "a"] : ["r", "g", "b"]); result = [colour.r, colour.g, colour.b]; if (format === "rgba8") result[3] = colour.a ?? 255; } else result = [value]; // Straight alpha defaults to full coverage.
        const maximum = ["mono1", "mono8", "gray32"].includes(format) ? 1 : 255; for (const v of result) require_value(typeof v === "number" && finite(v) && v >= 0 && v <= maximum && (format === "gray32" || integer(v)), "sample_value"); // Do not clamp malformed input.
        return result; // Assignment to Float32Array performs the declared float rounding.
    } // Zero intensity remains distinct from clear/defer.
    function write_sample(state, index, value) { // Internal storage is always tight and shape-validated.
        const l = state.layout; if (l.shape.format === "mono1") { const x = index % l.width, offset = floor(index / l.width) * l.row + floor(x / 8), bit = 1 << (7 - x % 8); state.work_data[offset] = (state.work_data[offset] & ~bit) | (value[0] ? bit : 0); return; } // Preserve neighbouring pixel bits and row padding.
        for (let channel = 0; channel < l.channels; channel += 1) state.work_data[index * l.channels + channel] = value[channel] ?? 0; // One scalar sample, no Rust crossing.
    } // Internal writes cannot grow a plane.
    function colour_cell(state, cell, rgb, order) { require_value(state.work_cell_rgb, "undeclared_cell_rgb"); state.work_colour_touch[cell] = rgb ? 1 : 2; state.work_colour_order[cell] = order; for (let i = 0; i < 3; i += 1) state.work_cell_rgb[cell * 3 + i] = rgb ? rgb[i] : 0; } // Presence and bytes are separate.
    function cells_of(state, r, action) { if (r.width === 0 || r.height === 0) return; const pixel = state.layout.shape.mode === "pixels", sx = pixel ? 2 : 1, sy = pixel ? 4 : 1; for (let y = floor(r.y / sy); y < ceil((r.y + r.height) / sy); y += 1) for (let x = floor(r.x / sx); x < ceil((r.x + r.width) / sx); x += 1) action(y * state.layout.shape.cell_width + x); } // Outward cell rounding preserves zero-area no-ops.
    function mark(state, r, touch, order, colour = false) { // Touch metadata authenticates no source; it removes previous provenance.
        for (let y = r.y; y < r.y + r.height; y += 1) for (let x = r.x; x < r.x + r.width; x += 1) { const index = y * state.layout.width + x; state.work_touch[index] = touch; state.work_order[index] = order; if (touch >= 2) write_sample(state, index, [0, 0, 0, 0]); } // Clear/defer cannot retain stale sample bytes.
        if (state.work_cell_rgb && (touch >= 2 || colour)) cells_of(state, r, (cell) => colour_cell(state, cell, touch >= 2 ? null : [state.work_cell_rgb[cell * 3], state.work_cell_rgb[cell * 3 + 1], state.work_cell_rgb[cell * 3 + 2]], order)); // Clear also removes colour; explicit direct colour damage sets presence.
    } // Native independently derives implicit colour erasure and every owner reset.
    function direct(state, colour = false) { // Acquiring a replacement writable plane claims its still-implicit samples.
        return run(state, () => { const plane = colour ? state.work_cell_rgb : state.work_data; require_value(plane, "undeclared_plane"); if (colour) state.rgb_exposed = true; const flag = colour ? "rgb_direct" : "data_direct"; if (!state[flag] && state.layout.shape.update === "replace") { const order = next(state), touch = colour ? state.work_colour_touch : state.work_touch, orders = colour ? state.work_colour_order : state.work_order; for (let i = 0; i < touch.length; i += 1) if (touch[i] === 0) { touch[i] = 1; orders[i] = order; } } state[flag] = true; return plane; }); // Retain mode always requires explicit damage.
    } // Rewriting a region after an explicit clear/defer needs a setter or damage declaration.
    function scalar(state, x, y, value, colour = undefined) { // A validated scalar write has one transaction order.
        const r = rect(state, { x, y, width: 1, height: 1 }, state.layout.shape.mode), sample = sample_value(state, value); let rgb; if (colour !== undefined) { const c = record(colour, ["r", "g", "b"]); rgb = [number(c.r, 255), number(c.g, 255), number(c.b, 255)]; require_value(state.work_cell_rgb, "undeclared_cell_rgb"); } // Validate before writing.
        const order = next(state), index = r.y * state.layout.width + r.x; write_sample(state, index, sample); mark(state, r, 1, order); if (rgb) colour_cell(state, index, rgb, order); // Optional scalar colour is a cell-mode operation.
    } // Pixel colour is carried by rgb8/rgba8 or frame.set_cell_rgb.
    function bulk(state, value, rows_mode = false) { // Rows/rectangles are synchronous local copies, never RPC per dot.
        const fields = record(value, ["x", "y", "width", "height", "format", "data", "stride_bytes", "rows", "masks", "cell_rgb"]), l = state.layout; const r = rect(state, { x: fields.x, y: fields.y, width: fields.width, height: fields.height }, l.shape.mode); // Strict target bounds.
        require_value(l.shape.mode === "cells" || fields.format === l.shape.format, "bulk_format"); const tight = l.shape.format === "mono1" ? ceil(r.width / 8) : r.width * l.channels; // Source format must match the accepted surface.
        const stride_bytes = fields.stride_bytes ?? tight * sizes[l.kind]; number(stride_bytes); require_value(stride_bytes % sizes[l.kind] === 0 && stride_bytes >= tight * sizes[l.kind], "stride"); const stride = stride_bytes / sizes[l.kind], sources = []; // Byte stride includes final-row padding.
        if (rows_mode) { require_value(is_array(fields.rows) && fields.rows.length === r.height && fields.stride_bytes === undefined, "row_count"); for (let y = 0; y < r.height; y += 1) { const item = descriptor(fields.rows, string_type(y)); require_value(item && descriptor(item, "value"), "row_accessor"); sources[y] = view(item.value, l.kind, tight); } } else sources[0] = view(l.shape.mode === "cells" ? fields.masks : fields.data, l.kind, stride * r.height); // Caller subviews are copied, not retained.
        const colour = fields.cell_rgb === undefined ? null : view(fields.cell_rgb, "u8", r.width * r.height * 3); require_value(!colour || (l.shape.mode === "cells" && l.shape.cell_rgb), "bulk_colour"); // No undeclared RGB allocation.
        const order = next(state); // No accessor/proxy validation remains after this synchronous guard.
        for (let y = 0; y < r.height; y += 1) { const data = rows_mode ? sources[y] : sources[0], offset = rows_mode ? 0 : y * stride; if (l.shape.format === "mono1" && r.width % 8) require_value((data[offset + tight - 1] & ((1 << (8 - r.width % 8)) - 1)) === 0, "mono1_padding"); for (let x = 0; x < r.width; x += 1) { const value = []; for (let c = 0; c < l.channels; c += 1) value[c] = l.shape.format === "mono1" ? (data[offset + floor(x / 8)] >> (7 - x % 8)) & 1 : data[offset + x * l.channels + c]; const max = ["mono1", "mono8", "gray32"].includes(l.shape.format) ? 1 : 255; for (const v of value) require_value(finite(v) && v >= 0 && v <= max, "bulk_sample"); write_sample(state, (r.y + y) * l.width + r.x + x, value); } } // A later validation error poisons the whole frame.
        mark(state, r, 1, order); if (colour) for (let y = 0; y < r.height; y += 1) for (let x = 0; x < r.width; x += 1) { const src = (y * r.width + x) * 3; colour_cell(state, (r.y + y) * l.width + r.x + x, [colour[src], colour[src + 1], colour[src + 2]], order); } // All copied samples clear old provenance.
    } // Empty rectangles do not read a source sample.
    function command(state, value) { const copy = json_copy(value); require_value(state.commands.length < max_commands, "command_limit"); copy.order = next(state); const candidate = state.commands.concat([copy]); json_text({ wire_version: 1, key: state.key, shape: state.layout.shape, presented: true, error: "frame_operation_failed", commands: candidate }); state.commands = candidate; } // Reserve full metadata and failure diagnostic space before adding a command.
    function vector(op, value) { // host.draw calls only a local frame encoder.
        const state = active; require_value(state, "no_active_frame"); return run(state, () => { const v = record(value, ["points", "width", "fill", "closed", "value", "blend", "rgb"]); const points = json_copy(v.points); require_value(is_array(points) && points.length >= 2 && points.length <= 1024 && points.every((p) => is_array(p) && p.length === 2 && p.every((n) => typeof n === "number" && finite(n) && abs(n) <= 1000000)), "vector_points"); require_value((op === "path") || points.length === (op === "triangle" ? 3 : 2), "primitive_points"); const width = v.width ?? 1, blend = v.blend ?? "overwrite"; require_value(finite(width) && width >= 0 && width <= 1024 && ["overwrite", "max", "alpha"].includes(blend) && (blend !== "alpha" || state.layout.shape.format === "rgba8"), "vector_style"); require_value(op !== "ellipse" || (points[1][0] >= 0 && points[1][1] >= 0), "ellipse_radii"); command(state, { kind: "vector", op, points, width, fill: v.fill ?? false, closed: v.closed ?? false, value: sample_value(state, v.value), blend, ...(v.rgb === undefined ? {} : {rgb: json_copy(v.rgb)}) }); }); // Native validates and clips the same closed schema.
    } // Geometry/rasterization is the primary's native adapter, not a hidden scene renderer.
    function native_blit(value) { const state = active; require_value(state, "no_active_frame"); return run(state, () => { const v = record(value, ["handle", "source", "target", "blend"]); require_value(typeof v.handle === "string" && /^[a-zA-Z0-9_.-]{1,128}$/.test(v.handle), "source_handle"); const source = record(v.source, ["x", "y", "width", "height"]); for (const key of keys(source)) number(source[key], 4294967295); require_value(source.width > 0 && source.height > 0 && source.x + source.width <= 4294967295 && source.y + source.height <= 4294967295, "source_rect"); const target = rect(state, v.target, state.layout.shape.mode), blend = v.blend ?? "overwrite"; require_value(["overwrite", "max", "alpha"].includes(blend) && (blend !== "alpha" || state.layout.shape.format === "rgba8"), "blit_blend"); command(state, { kind: "blit", handle: v.handle, source, target: { x: target.x, y: target.y, width: target.width, height: target.height }, blend }); }); } // Prepared handles confer no authority without native validation.
    function text_span(value) { const state = active; require_value(state, "no_active_frame"); return run(state, () => { const v = record(value, ["x", "y", "text", "rgb", "bold", "italic", "underline"]); number(v.x, state.layout.shape.cell_width - 1); number(v.y, state.layout.shape.cell_height - 1); require_value(typeof v.text === "string" && v.text.length > 0 && utf8_length(v.text) <= 16384 && !/[\u0000-\u001f\u007f-\u009f]/.test(v.text), "text"); const rgb = v.rgb === undefined ? null : json_copy(v.rgb); require_value(rgb === null || (is_array(rgb) && rgb.length === 3 && rgb.every((c) => integer(c) && c >= 0 && c <= 255)), "text_rgb"); command(state, { kind: "text", x: v.x, y: v.y, text: v.text, style: { rgb, bold: v.bold ?? false, italic: v.italic ?? false, underline: v.underline ?? false } }); }); } // Native Unicode layout supplies widths/continuations and clipping.
    const draw = freeze({ batch: sdk_draw_batch, line: (value) => vector("line", value), path: (value) => vector("path", value), triangle: (value) => vector("triangle", value), ellipse: (value) => vector("ellipse", value), blit: native_blit }); // Synchronous local drawing family.
    function seal(state) { // present freezes submitted bytes before control returns to package code.
        guard(state); set(state.data, state.work_data); set(state.touch, state.work_touch); set(state.order_plane, state.work_order); // Aliases to working arrays cannot affect these private copies.
        if (state.cell_rgb) { set(state.cell_rgb, state.work_cell_rgb); set(state.colour_touch, state.work_colour_touch); set(state.colour_order, state.work_colour_order); } // Seal colour presence/order independently.
        state.sealed_commands = frozen_json(state.commands); state.presented = true; state.closed = true; // Exactly one submission closes all helpers.
    } // Native still rejects exceptions, wrong bases, invalid samples, and allocation violations.
    function random_word() { // Same mix64 and wrapping counter as Native SeededRandom.
        const mask = 0xffffffffffffffffn;
        let value = (context_random_state + 0x9e3779b97f4a7c15n) & mask;
        value = ((value ^ (value >> 30n)) * 0xbf58476d1ce4e5b9n) & mask;
        value = ((value ^ (value >> 27n)) * 0x94d049bb133111ebn) & mask;
        context_random_state = (context_random_state + 1n) & mask;
        return value ^ (value >> 31n);
    }
    function context_helpers(state, entry) { // Native-selected inputs remain data, never screen-protection authority.
        if (context_random_state === null) context_random_state = native_bigint(identity(entry.random_seed, true));
        state.context.random = freeze({ __proto__: null,
            next: () => { guard(state); return native_number(random_word() >> 11n) / 9007199254740992; },
            integer: (minimum, maximum) => {
                guard(state); require_value(integer(minimum) && integer(maximum) && minimum <= maximum, "random_integer_bounds");
                const lower = native_bigint(minimum), range = native_bigint(maximum) - lower + 1n;
                const threshold = (0x10000000000000000n - range) % range;
                for (let attempt = 0; attempt < 64; attempt += 1) { const word = random_word(); if (word >= threshold) return native_number(lower + word % range); }
                throw new error_type("random_rejection_budget");
            }
        }); // Not cryptographic randomness; integer endpoints are inclusive.
        const observation = state.context.inputs.occlusion;
        if (observation === undefined) return;
        require_value(observation && typeof observation === "object" && !is_array(observation), "occlusion_snapshot");
        const cells = observation.cells, pixels = observation.pixels;
        if (cells !== undefined) view(cells, "u8", state.layout.cells, true);
        if (pixels !== undefined) view(pixels, "u8", state.layout.cells * 8, true);
        const width = state.layout.shape.cell_width, height = state.layout.shape.cell_height;
        const coordinates = (x, y) => require_value(integer(x) && integer(y), "occlusion_coordinates");
        define_property(observation, "is_cell_blocked", { __proto__: null, enumerable: false, value: (x, y) => {
            guard(state); coordinates(x, y); if (x < 0 || y < 0 || x >= width || y >= height) return false;
            if (cells !== undefined) return cells[y * width + x] !== 0;
            require_value(pixels !== undefined, "occlusion_plane_unavailable");
            for (let dy = 0; dy < 4; dy += 1) for (let dx = 0; dx < 2; dx += 1) if (pixels[(y * 4 + dy) * width * 2 + x * 2 + dx] !== 0) return true;
            return false;
        } });
        define_property(observation, "is_pixel_blocked", { __proto__: null, enumerable: false, value: (x, y) => {
            guard(state); coordinates(x, y); if (x < 0 || y < 0 || x >= width * 2 || y >= height * 4) return false;
            if (pixels !== undefined) return pixels[y * width * 2 + x] !== 0;
            require_value(cells !== undefined, "occlusion_plane_unavailable");
            return cells[floor(y / 4) * width + floor(x / 2)] !== 0;
        } }); // Derive the unrequested representation without extra native gathering or plane allocation.
    }
    function install(name, value) { define_property(global, name, { __proto__: null, value, writable: false, configurable: false, enumerable: false }); } // Package code cannot replace trusted hooks.
    install("__ilium_configure_ambient", (mode, initial_seed) => {
        require_value(!ambient_configured && native_phase() === 0 && !seed && !active, "ambient_configuration_phase");
        require_value(mode === "live" || mode === "pre_rendered", "ambient_mode");
        number(initial_seed, 0xffffffff);
        ambient_configured = true; ambient_mode = mode;
        if (mode !== "pre_rendered") return;
        context_random_state = native_bigint(initial_seed);
        const native_date = global.Date, native_math = global.Math;
        // The guest never receives native Date.prototype: its local-time and
        // locale methods would make a "sealed" replay depend on the host TZ.
        const date_prototype = create(null);
        const date_methods = ["getTime", "valueOf", "toISOString", "toUTCString",
            "getUTCFullYear", "getUTCMonth", "getUTCDate", "getUTCDay",
            "getUTCHours", "getUTCMinutes", "getUTCSeconds", "getUTCMilliseconds",
            "setTime", "setUTCFullYear", "setUTCMonth", "setUTCDate",
            "setUTCHours", "setUTCMinutes", "setUTCSeconds", "setUTCMilliseconds"];
        for (const name of date_methods) {
            const method = native_date.prototype[name];
            define_property(date_prototype, name, { __proto__: null,
                value: function (...args) { return apply(method, this, args); },
                writable: false, configurable: false });
        }
        const utc_string = native_date.prototype.toUTCString;
        const make_utc_string = function () { return apply(utc_string, this, []); };
        for (const name of ["toString", "toDateString", "toTimeString"]) {
            define_property(date_prototype, name, { __proto__: null,
                value: make_utc_string, writable: false, configurable: false });
        }
        define_property(date_prototype, "getTimezoneOffset", { __proto__: null,
            value: function () {
                const time = apply(native_date.prototype.valueOf, this, []);
                return finite(time) ? 0 : NaN;
            }, writable: false, configurable: false });
        define_property(date_prototype, Symbol.toPrimitive, { __proto__: null,
            value: function (hint) {
                require_value(hint === "default" || hint === "number" || hint === "string", "replay_date_hint");
                return hint === "number" ? apply(native_date.prototype.valueOf, this, []) : make_utc_string.call(this);
            }, writable: false, configurable: false });
        const parse_utc = function (value) {
            require_value(typeof value === "string" && /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{1,3})?(?:Z|[+-]\d\d:\d\d)$/.test(value), "replay_date_parse");
            return apply(native_date.parse, native_date, [value]);
        };
        function ReplayDate(...args) {
            if (!new.target) return apply(utc_string, new native_date(ambient_ms), []);
            let result;
            if (args.length === 0) result = new native_date(ambient_ms);
            else if (args.length === 1 && typeof args[0] === "string") result = new native_date(parse_utc(args[0]));
            else if (args.length === 1) result = new native_date(args[0]);
            else result = new native_date(apply(native_date.UTC, native_date, args));
            return set_prototype(result, date_prototype);
        }
        define_property(date_prototype, "constructor", { __proto__: null, value: ReplayDate, writable: false, configurable: false });
        define_property(ReplayDate, "prototype", { __proto__: null, value: freeze(date_prototype), writable: false, configurable: false });
        define_property(ReplayDate, "now", { __proto__: null, value: () => ambient_ms, writable: false, configurable: false });
        define_property(ReplayDate, "parse", { __proto__: null, value: parse_utc, writable: false, configurable: false });
        define_property(ReplayDate, "UTC", { __proto__: null, value: (...args) => apply(native_date.UTC, native_date, args), writable: false, configurable: false });
        freeze(ReplayDate);
        define_property(global, "Date", { __proto__: null, value: ReplayDate, writable: false, configurable: false });
        // These realm facilities can observe wall time, locale, GC timing or
        // ambient randomness independently of the replay clock/seed.
        for (const name of ["Intl", "Temporal", "performance", "crypto", "WeakRef", "FinalizationRegistry", "SharedArrayBuffer", "Atomics"]) {
            define_property(global, name, { __proto__: null, value: undefined, writable: false, configurable: false });
        }
        define_property(native_math, "random", { __proto__: null, value: () => native_number(random_word() >> 11n) / 9007199254740992, writable: false, configurable: false });
        freeze(native_math);
        define_property(global, "Math", { __proto__: null, value: native_math, writable: false, configurable: false });
    }); // The native helper calls exactly once before module instantiation/evaluation.
    install("__ilium_seed_frame", (metadata, planes) => { // Native-only binary baseline/input and authenticated informational snapshot injection.
        require_value(native_phase() === 3, "native_seed_phase"); require_value(!seeding && !active && !awaiting && !seed, "seed_busy"); seeding = true; try { if (typeof metadata === "string") require_value(utf8_length(metadata) <= max_meta, "seed_metadata_limit"); const source = typeof metadata === "string" ? parse(metadata) : metadata, info = json_copy(source); json_text(info); const supplied = record(planes); // Native must bound input before this call too.
        seed_services(source); seed_asset_handles(source); if (info.host !== undefined) host_info = frozen_json(info.host); // Only this native phase may update service snapshots and brand genuine handle records; permission/package fields stay informational.
        if (info.frame === null) { require_value(keys(supplied).length === 0, "unexpected_planes"); return; } // Bootstrap host metadata before module.create.
        const frame = record(info.frame, ["key", "shape", "reset", "invalid_rects", "input_specs"]), l = layout(frame.shape), key = record(frame.key, ["instance_id", "revision", "base_version", "sequence"]); // Same shape/key as Surface::begin.
        identity(key.instance_id); identity(key.revision); identity(key.base_version, true); identity(key.sequence); require_value(typeof frame.reset === "boolean" && is_array(frame.invalid_rects) && frame.invalid_rects.length <= 128 && is_array(frame.input_specs) && frame.input_specs.length <= 32, "seed_metadata"); // Bounded lifecycle/backing invalidity.
        const expected = ["work_data"], buffers = new set_type(); let bytes = l.handoff_bytes; // Track distinct exact buffers for detachment.
        view(supplied.work_data, l.kind, l.elements, true); buffers.add(apply(get_buffer, supplied.work_data, [])); // Adopt the native-created exclusive working baseline.
        if (l.shape.cell_rgb) { append_data(expected, "work_cell_rgb"); view(supplied.work_cell_rgb, "u8", l.cells * 3, true); require_value(!buffers.has(apply(get_buffer, supplied.work_cell_rgb, [])), "aliased_seed"); buffers.add(apply(get_buffer, supplied.work_cell_rgb, [])); } // Optional plane only when admitted.
        const paths = new set_type(); for (let i = 0; i < frame.input_specs.length; i += 1) { const spec = record(frame.input_specs[i], ["path", "kind", "elements"]), name = `input_${i}`; require_value(typeof spec.path === "string" && /^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){1,3}$/.test(spec.path) && !spec.path.split(".").some((p) => ["__proto__", "constructor", "prototype"].includes(p)) && !paths.has(spec.path), "input_path"); paths.add(spec.path); number(spec.elements, max_handoff); require_value(spec.kind in constructors, "input_kind"); bytes += spec.elements * sizes[spec.kind]; require_value(bytes <= max_handoff, "input_byte_limit"); view(supplied[name], spec.kind, spec.elements, true); const buffer = apply(get_buffer, supplied[name], []); require_value(!buffers.has(buffer), "aliased_seed"); buffers.add(buffer); append_data(expected, name); } // Only native-selected subscribed input fields are injected.
        require_value(keys(supplied).length === expected.length && keys(supplied).every((name) => expected.includes(name)), "seed_plane_names"); seed = { frame, layout: l, planes: supplied, random_seed: identity(info.random_seed ?? "0", true) }; // Consume once at make_frame.
        } finally { seeding = false; } // Native seed identity cannot be reentered through any descriptor, iterator, or nested callback failure.
    }); // Native retains fallback ownership for detachment if make_frame itself fails.
    install("__ilium_make_frame", (context_json) => { // Frozen hook: input may be a parsed object or bounded JSON string.
        require_value(!active && !awaiting && seed, "frame_lifecycle"); if (typeof context_json === "string") require_value(utf8_length(context_json) <= max_meta, "context_limit"); const original = typeof context_json === "string" ? parse(context_json) : context_json, context = json_copy(original); // Binary inputs are injected after JSON validation.
        json_text(context); require_value(json_text(context._ilium_frame) === json_text({ key: seed.frame.key, shape: seed.frame.shape }), "seed_context_mismatch"); const entry = seed, l = entry.layout; // A rejected base cannot be guessed locally.
        if (ambient_mode === "pre_rendered") { require_value(typeof context.time === "number" && finite(context.time) && context.time >= 0 && context.time * 1000 <= max_integer, "replay_ambient_time"); ambient_ms = round(context.time * 1000); } // Explicit native sample time, never physical wall time.
        const state = { __proto__: null, layout: l, key: frozen_json(entry.frame.key), context, work_data: entry.planes.work_data, work_touch: alloc("u8", l.samples), work_order: alloc("u32", l.samples), data: alloc(l.kind, l.elements), touch: alloc("u8", l.samples), order_plane: alloc("u32", l.samples), commands: [], sealed_commands: freeze([]), callbacks: [], order: 0, presented: false, closed: false, finished: false, error: null, rgb_exposed: false, data_direct: false, rgb_direct: false }; // Null-prototype private state prevents inherited setters from exposing working or sealed planes during later field installation.
        if (l.shape.cell_rgb) { state.work_cell_rgb = entry.planes.work_cell_rgb; state.work_colour_touch = alloc("u8", l.cells); state.work_colour_order = alloc("u32", l.cells); state.cell_rgb = alloc("u8", l.cells * 3); state.colour_touch = alloc("u8", l.cells); state.colour_order = alloc("u32", l.cells); } // No extra RGB companion on rgb8/rgba8.
        if (l.shape.update === "replace") { apply(typed_fill, state.work_data, [0]); if (state.work_cell_rgb) apply(typed_fill, state.work_cell_rgb, [0]); } // Empty-present replacement starts known empty.
        const returned = { __proto__: null, work_data: state.work_data, data: state.data, work_touch: state.work_touch, touch: state.touch, work_order: state.work_order, order: state.order_plane }; // Every exposed working buffer is returned for physical detachment.
        if (l.shape.cell_rgb) for (const name of ["work_cell_rgb", "cell_rgb", "work_colour_touch", "colour_touch", "work_colour_order", "colour_order"]) returned[name] = state[name]; // Native derives these ArraySpec entries from shape.
        context.inputs ??= create(null); for (let i = 0; i < entry.frame.input_specs.length; i += 1) { const spec = entry.frame.input_specs[i], parts = spec.path.split("."); let target = context.inputs; for (let p = 0; p < parts.length - 1; p += 1) { target[parts[p]] ??= create(null); require_value(target[parts[p]] && typeof target[parts[p]] === "object" && !is_array(target[parts[p]]), "input_parent"); target = target[parts[p]]; } require_value(target[parts[parts.length - 1]] === undefined, "input_collision"); target[parts[parts.length - 1]] = entry.planes[`input_${i}`]; returned[`input_${i}`] = entry.planes[`input_${i}`]; } // No million-pixel JSON baseline/input conversion.
        context_helpers(state, entry);
        context.surface = { revision: entry.frame.key.base_version, reset: entry.frame.reset, invalid_rects: entry.frame.invalid_rects }; delete context._ilium_frame; state.returned = freeze(returned); // Only backing invalidity, never final UI protection authority.
        const frame = { cell_width: l.shape.cell_width, cell_height: l.shape.cell_height, dot_width: l.shape.cell_width * 2, dot_height: l.shape.cell_height * 4, context, draw, damage_rect: (value) => run(state, () => { const r = rect(state, value); mark(state, r, 1, next(state), r.colour ?? state.rgb_exposed); }), defer_rect: (value) => run(state, () => { const r = rect(state, value); mark(state, r, 3, next(state)); }), present: () => run(state, () => seal(state)), after_accept: (callback) => run(state, () => { require_value(typeof callback === "function" && state.callbacks.length < 64, "after_accept_limit"); state.callbacks.push(callback); }) }; // All frame operations are local and synchronous.
        frame.set_cell_rgb = (x, y, rgb) => run(state, () => { number(x, l.shape.cell_width - 1); number(y, l.shape.cell_height - 1); const c = rgb === null ? null : record(rgb, ["r", "g", "b"]), value = c ? [number(c.r, 255), number(c.g, 255), number(c.b, 255)] : null; colour_cell(state, y * l.shape.cell_width + x, value, next(state)); }); // Metadata-only colour changes preserve sample validity.
        define_property(frame, "cell_rgb", { __proto__: null, get: () => direct(state, true), enumerable: true }); // Optional writable colour plane uses explicit damage/presence rules.
        const drawing = { clear_rect: (value) => run(state, () => { const r = rect(state, value, l.shape.mode); mark(state, r, 2, next(state)); }), write_rect: (value) => run(state, () => bulk(state, value)), write_rows: (value) => run(state, () => bulk(state, value, true)) }; // Common bounded rectangle operations.
        if (l.shape.mode === "cells") { drawing.set_cell = (x, y, value) => run(state, () => { const v = record(value, ["mask", "rgb"]); scalar(state, x, y, v.mask, v.rgb); }); drawing.set_braille = (x, y, glyph, rgb) => run(state, () => { require_value(typeof glyph === "string" && glyph.length === 1 && glyph.charCodeAt(0) >= 10240 && glyph.charCodeAt(0) <= 10495, "braille_glyph"); scalar(state, x, y, glyph.charCodeAt(0) - 10240, rgb); }); define_property(drawing, "masks", { __proto__: null, get: () => direct(state), enumerable: true }); define_property(drawing, "rgb", { __proto__: null, get: () => direct(state, true), enumerable: true }); frame.cells = freeze(drawing); } else { drawing.set_pixel = (x, y, value) => run(state, () => scalar(state, x, y, value)); define_property(drawing, "data", { __proto__: null, get: () => direct(state), enumerable: true }); frame.pixels = freeze(drawing); if (["gray8", "gray32"].includes(l.shape.format)) define_property(frame, "gray", { __proto__: null, get: () => direct(state), enumerable: true }); } // The inactive representation is absent.
        const frame_draw = create(null); for (const name of keys(draw)) frame_draw[name] = (...args) => run(state, () => draw[name](...args)); frame.draw = freeze(frame_draw); state.frame = freeze(frame); apply(weak_set, frames, [state.frame, state]); active = state; seed = null; // Publish the fully constructed facade once.
        if (typeof context_json !== "string") { original.inputs = context.inputs; original.surface = context.surface; } return state.frame; // Native should pass frame.context to render, especially for string input.
    }); // The native engine must reject a Promise/thenable returned by instance.render.
    install("__ilium_frame_buffers", (frame) => { require_value(native_phase() === 4, "native_frame_inventory_phase"); const state = apply(weak_get, frames, [frame]); require_value(state && active === state && !state.finished, "foreign_frame"); return state.returned; }); // Only the native pre-render inventory phase may inspect private working, sealed, and input views; package render cannot recover sealed writable aliases.
    install("__ilium_finish_frame", (frame) => { // Frozen hook: always return owned planes for cleanup, even on failure/no-present.
        require_value(native_phase() === 5, "native_frame_finish_phase"); const state = apply(weak_get, frames, [frame]); require_value(state, "foreign_frame"); if (state.finished || active !== state) state.error = "finish_reentry"; state.finished = true; state.closed = true; awaiting = state; // Gate sealed-plane access and handoff mutation before private-state lookup.
        const metadata = frozen_json({ wire_version: 1, key: state.key, shape: state.layout.shape, presented: state.presented, error: state.error, commands: state.presented ? state.sealed_commands : [] }); json_text(metadata); return { __proto__: null, metadata, planes: state.returned }; // No owner IDs or final masks cross from JS.
    }); // Native validates/copies semantic planes and detaches every distinct returned buffer in finally.
    install("__ilium_accept_frame", (accepted) => { // Frozen hook: acknowledgement follows physical handoff and the Rust decision.
        const state = awaiting; require_value(state && !state.accepting && typeof accepted === "boolean", "missing_handoff"); for (const name of keys(state.returned)) { const buffer = apply(get_buffer, state.returned[name], []); require_value(apply(buffer_length, buffer, []) === 0 && (!buffer_detached || apply(buffer_detached, buffer, [])), "buffers_not_detached"); } // Reject a host that merely copied without detaching aliases.
        require_value(!accepted || (state.presented && !state.error), "invalid_acceptance"); state.accepting = true; try { if (accepted) for (const callback of state.callbacks) { const result = callback(); require_value(!result || (typeof result !== "object" && typeof result !== "function") || typeof result.then !== "function", "async_after_accept"); } } finally { state.callbacks.length = 0; awaiting = null; active = null; } // Reentrant acknowledgement is forbidden; callback failure retires after publication.
    }); // Rejected frames never advance local accepted drawing bookkeeping or a JS baseline cache.
    function status_record(kind, level, message, percent = undefined) { // Synchronous, bounded and informational; safe during render.
        require_value(typeof message === "string" && utf8_length(message) <= 4096, "status_message");
        require_value(["debug", "info", "warning", "error"].includes(level), "status_level");
        const entry = create(null); entry.kind = kind; entry.level = level; entry.message = message;
        if (percent !== undefined) { require_value(typeof percent === "number" && finite(percent) && percent >= 0 && percent <= 100, "status_percent"); entry.percent = percent; }
        const bytes = utf8_length(json_text(entry)) + 1; // Include JSON escaping before reserving the drain envelope.
        if (status_records.length >= 64 || status_bytes + bytes > 16384) { status_dropped = number(status_dropped + 1); return; }
        status_records.push(freeze(entry)); status_bytes += bytes;
    }
    install("__ilium_take_status", () => { // Native may drain after render; data conveys no rights and never enters FrameMeta.
        const result = create(null); result.records = status_records; result.dropped = status_dropped;
        freeze(result.records); freeze(result); status_records = []; status_bytes = 0; status_dropped = 0; return result;
    });
    async function checked_rpc(method, options, validate = null, adapt = null) { try { if (validate) validate(options); const result = await rpc(method, options); if (!result.ok || !adapt) return result; return result_ok(adapt(result.value)); } catch { return error_result("invalid_request_or_result", "The SDK request or native result does not match its declared data shape."); } } // Domain authorization and byte admission remain independent native checks.
    function json_result(value) { return native_json(value); } // Freeze native JSON without changing typed binary result ownership.
    function http_response(value, response) { const source = record(value, ["status", "headers", "body", "final_url"]); number(source.status, 599); require_value(source.status >= 100 && typeof source.final_url === "string", "native_http_response"); const headers = record(source.headers); require_value(keys(headers).every((key) => typeof headers[key] === "string"), "native_http_headers"); source.headers = freeze(headers); if (response === "bytes") binary(source.body, ["u8"]); else if (response === "text") require_value(typeof source.body === "string", "native_http_text"); else native_json(source.body); return freeze(source); } // HTTP bytes stay Uint8Array while credentials, redirect targets, and actual effects remain native-only.
    async function http_request(options) { try { const response = own(options, "response"); require_value(["bytes", "text", "json"].includes(response), "http_response_kind"); const body = own(options, "body", false); if (body !== undefined && typeof body !== "string") binary(body, ["u8"]); return await checked_rpc("http.request", options, null, (value) => http_response(value, response)); } catch { return error_result("invalid_request", "HTTP response kind or binary request body is invalid."); } } // The exact original options object is dispatched after noncoercing checks.
    async function poll_loop(state) { // One outstanding native pull serializes callbacks without timers or a local polling clock.
        try { while (!state.closed && !state.close_requested && state.callback) { const result = await rpc("http.poll.next", { __proto__: null, id: state.id, kind: state.kind }); refresh_handle(state); if (state.closed || state.close_requested || !state.callback) return; require_value(acquiring(), "poll_callback_phase"); if (!result.ok) { await apply(state.callback, undefined, [result]); return; } if (result.value === null) return; const notification = service_result(result.value), delivered = notification.ok ? result_ok(http_response(notification.value, "json")) : notification; await apply(state.callback, undefined, [delivered]); } } catch { status_record("log", "error", "HTTP polling callback or native notification failed; polling is stopped."); } finally { state.callback = null; } // Callback failure never fabricates a native closed/error status or loops on immediate rejection.
    } // The parent owns provider timing, minimum interval, cancellation, record bounds, and completion authorization for http.poll.next.
    async function http_poll(options, callback) { if (typeof callback !== "function") return error_result("invalid_callback", "HTTP polling requires a result callback."); const result = await open_rpc("http.poll", options, "http.poll"); if (!result.ok) return result; const state = apply(weak_get, handles, [result.value]); if (state.closed) return result; if (state.callback) return error_result("invalid_result", "Native polling returned an already subscribed identity."); state.callback = callback; void poll_loop(state); return result; } // Callback functions stay inside this isolate and are never serialized into a request.
    async function task_loop(state) { // One native next and one awaited callback; no JS timer or local cadence.
        try {
            while (!state.closed && !state.close_requested && state.callback) {
                const result = await rpc("tasks.poll.next", { __proto__: null, id: state.id, kind: state.kind });
                refresh_handle(state);
                if (state.closed || state.close_requested || !state.callback) return;
                if (!result.ok) { status_record("log", "warning", "Native task next failed; callback stopped."); return; }
                if (result.value === null) { remove_handle(state); return; } // Null is the native terminal ACK.
                const notification = record(result.value, ["tick"]);
                require_value(keys(notification).length === 1 && notification.tick === true && acquiring(), "native_task_tick");
                await apply(state.callback, undefined, []);
            }
        } catch {
            status_record("log", "error", "Task callback rejected or native task notification was invalid.");
            if (!state.closed && !state.close_requested) {
                try { close_handle(state); } catch { status_record("log", "warning", "Native task close was not admitted after callback failure."); }
            }
        } finally { state.callback = null; }
    }
    async function task_poll(options, callback) {
        if (typeof callback !== "function") return error_result("invalid_callback", "Task polling needs a callback.");
        if (ambient_mode === "pre_rendered") return error_result("live_only", "Periodic task time has no frozen replay clock.");
        try { const fields = record(options, ["interval_ms", "deadline_ms"]); require_value(keys(fields).length === 2, "task_options"); number(fields.interval_ms); number(fields.deadline_ms); }
        catch { return error_result("invalid_request", "Task cadence and deadline must be integers."); }
        const result = await open_rpc("tasks.poll.open", options, "tasks.poll");
        if (!result.ok) return result;
        const state = apply(weak_get, handles, [result.value]);
        if (!state || state.closed || state.callback) return error_result("invalid_result", "Native task handle is unavailable.");
        state.callback = callback; void task_loop(state);
        return result;
    }
    async function task_yield() {
        if (!ambient_configured) return error_result("task_unavailable", "Native ambient mode has not been configured.");
        const result = await rpc("tasks.yield", create(null));
        if (!result.ok) return result;
        return result.value === null ? result_ok(null) : error_result("invalid_result", "Native yield must resolve to null.");
    }
    function asset_read_result(value) { const source = record(value, ["bytes", "sha256", "name"]); binary(source.bytes, ["u8"]); require_value(typeof source.sha256 === "string" && /^[a-f0-9]{64}$/.test(source.sha256) && typeof source.name === "string", "native_asset_result"); return freeze(source); } // Preserve byte results and native content identity exactly.
    function world_region_result(value, world) {
        const source = record(value, ["origin", "size", "blocks", "palette", "identity"]);
        require_value(keys(source).length === 5 && source.identity === world.identity, "native_world_region_identity");
        binary(source.blocks, ["u16"]); // Retain the original typed plane; no JSON or numeric coercion.
        const origin = [], size = [];
        require_value(is_array(source.origin) && own(source.origin, "length") === 3 && is_array(source.size) && own(source.size, "length") === 3, "native_world_region_dimensions");
        let cells = 1;
        for (let axis = 0; axis < 3; axis += 1) {
            const coordinate = own(source.origin, string_type(axis)), extent = own(source.size, string_type(axis));
            require_value(integer(coordinate) && coordinate >= -2147483648 && coordinate <= 2147483647, "native_world_region_origin");
            require_value(integer(extent) && extent > 0 && extent <= 4294967295 && coordinate + extent - 1 <= 2147483647, "native_world_region_extent");
            cells *= extent; require_value(integer(cells) && cells <= max_handoff / 2, "native_world_region_cell_limit");
            append_data(origin, coordinate); append_data(size, extent);
        }
        require_value(apply(get_length, source.blocks, []) === cells, "native_world_region_shape");
        require_value(is_array(source.palette), "native_world_region_palette");
        const count = own(source.palette, "length"), palette = [];
        require_value(integer(count) && count > 0 && count <= 65536 && count <= cells, "native_world_region_palette_limit");
        let text_bytes = 0;
        for (let index = 0; index < count; index += 1) {
            const state = record(own(source.palette, string_type(index)), ["name", "properties"]);
            require_value(keys(state).length === 2 && typeof state.name === "string" && state.name.length > 0 && state.name.length <= max_meta, "native_world_region_state");
            const properties = record(state.properties);
            text_bytes += utf8_length(state.name);
            for (const key of keys(properties)) {
                require_value(typeof properties[key] === "string" && properties[key].length <= max_meta, "native_world_region_property");
                text_bytes += utf8_length(key) + utf8_length(properties[key]);
                require_value(text_bytes <= max_meta, "native_world_region_text_limit");
            }
            require_value(text_bytes <= max_meta, "native_world_region_text_limit");
            state.properties = freeze(properties); append_data(palette, freeze(state));
        }
        for (let index = 0; index < cells; index += 1) require_value(source.blocks[index] < count, "native_world_region_palette_index");
        source.origin = freeze(origin); source.size = freeze(size); source.palette = freeze(palette);
        return freeze(source);
    } // Complete semantic SDK volume, X-fast/Z-next/Y-last; native identity never creates source authority.
    function world_model_result(value) { const source = record(value, ["vertices", "indices", "texture"]); binary(source.vertices, ["f32"]); binary(source.indices, ["u32"]); if (source.texture !== undefined) source.texture = image_handle(source.texture); return freeze(source); } // Model and GPU indices retain the SDK's exact uint32 type.
    async function world_region(options) { try { const state = lookup_handle(own(options, "world"), ["worlds"]); return await checked_rpc("worlds.region", options, null, (value) => world_region_result(value, state.handle)); } catch { return error_result("invalid_request", "World region requires a current native world handle."); } } // Native substitutes the genuine wrapper without cloning caller options.
    function require_frame(value) { const state = apply(weak_get, frames, [value]); guard(state); return state; } // Synchronous SDK drawing can target only the current unclosed frame.
    function image_blit(options) { // Reuse the existing synchronous native-blit command schema and transaction order.
        let state; try { const source = record(options, ["image", "frame", "rectangle", "fit"]); state = require_frame(source.frame); const image = lookup_handle(source.image, ["image"]).handle, target = rect(state, source.rectangle, state.layout.shape.mode), fit = source.fit ?? "contain"; require_value(["contain", "cover", "stretch"].includes(fit), "image_fit"); if (target.width === 0 || target.height === 0) return result_ok(null); // A zero-area local draw is a validated no-op, never a service request.
            let sx = 0, sy = 0, sw = image.width, sh = image.height, tx = target.x, ty = target.y, tw = target.width, th = target.height; if (fit === "contain") { const ratio = min(tw / sw, th / sh); const width = max(1, floor(sw * ratio)), height = max(1, floor(sh * ratio)); tx += floor((tw - width) / 2); ty += floor((th - height) / 2); tw = width; th = height; } if (fit === "cover") { const ratio = max(tw / sw, th / sh); const width = max(1, min(sw, floor(tw / ratio))), height = max(1, min(sh, floor(th / ratio))); sx = floor((sw - width) / 2); sy = floor((sh - height) / 2); sw = width; sh = height; } // Resolve fit into bounded integer source and target rectangles before encoding.
            native_blit({ handle: image.id, source: { x: sx, y: sy, width: sw, height: sh }, target: { x: tx, y: ty, width: tw, height: th }, blend: "overwrite" }); return result_ok(null); // Native must resolve the image ID and perform the actual existing blit/provenance validation.
        } catch { if (state) state.error = "frame_operation_failed"; return error_result("image_blit_rejected", "Image blit metadata or the current frame was invalid."); } // A caught local drawing error still poisons the complete transaction.
    } // Facade encoding does not claim that the unsupplied native drawing adapter is implemented.
    function unavailable_result(code, message) { return error_result(code, message); } // Missing native synchronous facilities return explicit failures, never fabricated successful data.
    // SDK intensity is normalized; geometry is measured in Braille dots.
    // Colour samples respect the accepted transfer space. Optional scalar
    // tinting requires a declared cell_rgb plane and colours only touched cells.
    function sdk_draw_batch(frame, commands, blend = "overwrite") {
        let state;
        try { state = require_frame(frame); } catch { return error_result("invalid_request", "Drawing batch requires the current frame."); }
        try {
            return run(state, () => {
                require_value(is_array(commands) && commands.length <= max_commands && ["max", "overwrite", "alpha"].includes(blend), "draw_batch_shape");
                require_value(blend !== "alpha" || state.layout.shape.format === "rgba8", "draw_batch_alpha_format");
                const format = state.layout.shape.format;
                const point = (value) => {
                    const p = record(value, ["x", "y"]);
                    require_value(typeof p.x === "number" && finite(p.x) && abs(p.x) <= 1000000 && typeof p.y === "number" && finite(p.y) && abs(p.y) <= 1000000, "draw_point");
                    return [p.x, p.y];
                };
                for (let index = 0; index < commands.length; index += 1) {
                    const item = descriptor(commands, string_type(index));
                    require_value(item && descriptor(item, "value"), "draw_command_accessor");
                    const kind = own(item.value, "kind");
                    const allowed = kind === "line" ? ["kind", "from", "to", "width", "intensity", "rgb"]
                        : kind === "circle" ? ["kind", "centre", "radius", "fill", "intensity", "rgb"]
                        : kind === "path" ? ["kind", "points", "closed", "width", "intensity", "rgb"]
                        : kind === "triangle" ? ["kind", "vertices", "intensity", "rgb"] : null;
                    require_value(allowed, "draw_command_kind");
                    const c = record(item.value, allowed), intensity = c.intensity;
                    require_value(typeof intensity === "number" && finite(intensity) && intensity >= 0 && intensity <= 1, "draw_intensity");
                    const colour = c.rgb === undefined ? {r:255,g:255,b:255} : record(c.rgb, ["r", "g", "b"]);
                    const rgb = [number(colour.r, 255), number(colour.g, 255), number(colour.b, 255)];
                    let value, tint;
                    if (format === "rgb8" || format === "rgba8") {
                        const convert = (byte) => {
                            const srgb = byte / 255;
                            const linear = srgb <= 0.04045 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4;
                            const scaled = linear * (format === "rgb8" ? intensity : 1);
                            const encoded = state.layout.shape.colour_space === "linear" ? scaled : scaled <= 0.0031308 ? scaled * 12.92 : 1.055 * scaled ** (1 / 2.4) - 0.055;
                            return round(encoded * 255);
                        };
                        value = {r:convert(rgb[0]),g:convert(rgb[1]),b:convert(rgb[2])};
                        if (format === "rgba8") value.a = round(intensity * 255);
                    } else {
                        value = format === "gray32" ? intensity : format === "gray8" ? round(intensity * 255) : intensity >= 0.5 ? (format === "mask8" ? 255 : 1) : 0;
                        if (c.rgb !== undefined) { require_value(state.work_cell_rgb, "undeclared_cell_rgb"); tint = rgb; }
                    }
                    let op = kind, points, width = 1, fill = false, closed = false;
                    if (kind === "line") { points = [point(c.from), point(c.to)]; width = c.width; }
                    else if (kind === "circle") {
                        require_value(typeof c.radius === "number" && finite(c.radius) && c.radius >= 0 && c.radius <= 1000000 && typeof c.fill === "boolean", "draw_circle");
                        op = "ellipse"; points = [point(c.centre), [c.radius, c.radius]]; fill = c.fill;
                    } else {
                        const values = kind === "path" ? c.points : c.vertices;
                        require_value(is_array(values) && (kind === "path" ? values.length >= 2 && values.length <= 1024 : values.length === 3), "draw_points");
                        points = [];
                        for (let i = 0; i < values.length; i += 1) {
                            const entry = descriptor(values, string_type(i)); require_value(entry && descriptor(entry, "value"), "draw_point_accessor");
                            append_data(points, point(entry.value));
                        }
                        if (kind === "path") { require_value(typeof c.closed === "boolean", "draw_path_closed"); width = c.width; closed = c.closed; }
                        else { fill = true; closed = true; width = 0; }
                    }
                    vector(op, {points,width,fill,closed,value,blend,rgb:tint});
                }
                return {ok:true,value:null};
            });
        } catch { return error_result("invalid_request", "Invalid drawing batch; the frame transaction is rejected."); }
    }
    const default_font = "CascadiaCode-Regular", default_size_px = 16;
    function text_font(value) { const font = value ?? default_font; require_value(font === default_font, "unsupported_font"); return font; }
    function text_size(value) { const size = value ?? default_size_px; require_value(typeof size === "number" && finite(size) && size >= 8 && size <= 128, "font_size_px"); return size; }
    function text_colour(value) { if (value === undefined) return undefined; const colour = record(value, ["r", "g", "b"]); return [number(colour.r, 255), number(colour.g, 255), number(colour.b, 255)]; }
    function sdk_text_measure(options) {
        const value = record(options, ["text", "font", "size_px"]);
        require_value(typeof value.text === "string" && utf8_length(value.text) <= 16384 && !/[\u0000-\u001f\u007f-\u009f]/.test(value.text), "text");
        const answer = service_result(native_text_measure({ __proto__: null, text: value.text, font: text_font(value.font), size_px: text_size(value.size_px) }));
        if (!answer.ok) fail(answer.error.code);
        const metrics = record(answer.value, ["width", "height"]);
        number(metrics.width, 16384); number(metrics.height, 16384);
        return freeze({ __proto__: null, width: metrics.width, height: metrics.height });
    }
    function sdk_text_raster(options) {
        const state = active;
        try { return run(state, () => {
            const value = record(options, ["frame", "text", "x", "y", "font", "size_px", "intensity", "rgb"]);
            require_frame(value.frame);
            const x = number(value.x, state.layout.shape.cell_width * 2 - 1), y = number(value.y, state.layout.shape.cell_height * 4 - 1);
            require_value(typeof value.text === "string" && utf8_length(value.text) <= 16384 && !/[\u0000-\u001f\u007f-\u009f]/.test(value.text), "text");
            const font = text_font(value.font), size_px = text_size(value.size_px);
            require_value(typeof value.intensity === "number" && finite(value.intensity) && value.intensity >= 0 && value.intensity <= 1, "text_intensity");
            const rgb = text_colour(value.rgb);
            if (value.text.length > 0) command(state, { kind: "raster_text", x, y, text: value.text, font, size_px, intensity: value.intensity, ...(rgb === undefined ? {} : {rgb}) });
            return result_ok(null);
        }); } catch { return error_result("invalid_request", "Raster text was rejected; the frame transaction is invalid."); }
    }
    function sdk_text_spans(options) {
        const state = active;
        try { return run(state, () => {
            const value = record(options, ["frame", "x", "y", "spans", "max_cells"]);
            require_frame(value.frame);
            const x = number(value.x, state.layout.shape.cell_width - 1), y = number(value.y, state.layout.shape.cell_height - 1);
            const max_cells = number(value.max_cells, state.layout.shape.cell_width - x);
            require_value(is_array(value.spans) && value.spans.length <= 64, "text_spans");
            const spans = []; let bytes = 0;
            for (let index = 0; index < value.spans.length; index += 1) {
                const slot = descriptor(value.spans, string_type(index)); require_value(slot && descriptor(slot, "value"), "text_span_accessor");
                const item = record(slot.value, ["text", "foreground", "background", "bold", "italic", "underline"]);
                require_value(typeof item.text === "string" && item.text.length > 0 && !/[\u0000-\u001f\u007f-\u009f]/.test(item.text), "text_span");
                bytes += utf8_length(item.text); require_value(bytes <= 16384, "text_bytes");
                for (const flag of ["bold", "italic", "underline"]) require_value(item[flag] === undefined || typeof item[flag] === "boolean", "text_style_flag");
                append_data(spans, { __proto__: null, text: item.text, style: { __proto__: null,
                    rgb: text_colour(item.foreground) ?? null, background: text_colour(item.background) ?? null,
                    bold: item.bold ?? false, italic: item.italic ?? false, underline: item.underline ?? false } });
            }
            require_value(spans.length === 0 || max_cells > 0, "text_max_cells");
            if (spans.length > 0) command(state, { kind: "text_spans", x, y, spans, max_cells });
            return result_ok(null);
        }); } catch { return error_result("invalid_request", "Styled text was rejected; the frame transaction is invalid."); }
    }
    function synchronous_factory(code) { require_value(acquiring(), "service_phase"); fail(code); } // The asynchronous dispatcher cannot honestly return a newly native-issued handle synchronously.
    function replay_metadata() { const value = host_info.replay?.metadata; require_value(value && ["live", "pre_rendered"].includes(value.mode), "native_replay_metadata_unavailable"); number(value.seed); number(value.sample_index); return value; } // Never infer replay identity from accepted plan JSON or invent a seed/sample index.
    function replay_eligibility() { const value = host_info.replay?.eligibility; if (!value) return freeze({ __proto__: null, supported: false, reason: "Native replay eligibility has not been supplied." }); require_value(typeof value.supported === "boolean" && (value.reason === undefined || typeof value.reason === "string"), "native_replay_eligibility"); return value; } // Absence is explicit unavailability, while positive eligibility comes only from a native seed.
    function permission_snapshot(grant) { const source = record(grant); if (!integer(source.epoch) || source.epoch <= 0 || !["input.pointer", "screen.occlusion", "location.observer", "network.http", "network.local", "disk.read", "disk.write", "audio.loopback", "audio.microphone", "state.persist", "device.gpu"].includes(source.id) || source.scope === undefined) return undefined; if (source.request_id !== undefined && (typeof source.request_id !== "string" || source.request_id.length === 0)) return undefined; const result = { __proto__: null, id: source.id, scope: source.scope, epoch: source.epoch }; if (source.request_id !== undefined) result.request_id = source.request_id; if (source.handle !== undefined) { const state = source.handle && typeof source.handle === "object" ? find_handle("asset", own(source.handle, "id")) : undefined; if (!state || state.closed) return undefined; result.handle = state.handle; } return freeze(result); } // Only complete native PermissionGrant observations are exposed; accepted request descriptions cannot synthesize epoch, rights, or handles.
    function family(prefix, methods) { const result = create(null); for (const method of methods.split(" ")) result[method] = async (payload = create(null)) => await rpc(`${prefix}.${method}`, payload); return result; } // Retain existing explicitly named extension facades while native unknown-method errors remain failures.
    const permissions = freeze({ get: (id, scope) => { const grants = host_info.permissions ?? []; const exact = grants.filter((grant) => grant.request_id === id); const matches = (exact.length ? exact : grants.filter((grant) => grant.id === id)).filter((grant) => scope === undefined || json_text(grant.scope) === json_text(scope)); return matches.length === 1 ? permission_snapshot(matches[0]) : undefined; }, has: (id, scope) => permissions.get(id, scope) !== undefined }); // This unique informational lookup never substitutes for the current native channel and operation ticket.
    const assets = family("assets", "stat hash open"); assets.read = async (options) => await checked_rpc("assets.read", options, (value) => handle_field(value, "grant", ["asset"]), asset_read_result); assets.list = async (options) => await checked_rpc("assets.list", options, (value) => handle_field(value, "grant", ["asset"]), json_result); assets.write = async (options) => await checked_rpc("assets.write", options, (value) => { handle_field(value, "grant", ["asset"]); request_binary(value, "bytes", ["u8"]); }, json_result); // Every asset operation preserves caller structure and passes native-scoped handle data through the binary bridge.
    const cache = family("cache", "delete status"); cache.get = async (options) => await checked_rpc("cache.get", options, null, (value) => value === null ? null : binary(value, ["u8"])); cache.put = async (options) => await checked_rpc("cache.put", options, (value) => request_binary(value, "bytes", ["u8"]), json_result); cache.remove = async (key) => { if (typeof key !== "string") return error_result("invalid_request", "Cache key must be a string."); return await checked_rpc("cache.remove", { __proto__: null, key }, null, (value) => { require_value(typeof value === "boolean", "native_cache_remove"); return value; }); }; // Binary cache values never enter JSON copy/stringify.
    const images = family("media.images", "release"); images.decode = async (options) => await open_rpc("media.images.decode", options, "image", (value) => request_binary(value, "bytes", ["u8"])); images.resize = async (options) => await open_rpc("media.images.resize", options, "image", (value) => handle_field(value, "image", ["image"])); images.sample = async (options) => { try { const format = own(options, "format"); require_value(["mono1", "mono8", "gray8", "gray32", "rgb8", "rgba8"].includes(format), "sample_format"); return await checked_rpc("media.images.sample", options, (value) => handle_field(value, "image", ["image"]), (value) => binary(value, format === "gray32" ? ["f32"] : ["u8"])); } catch { return error_result("invalid_request", "Image sample format is invalid."); } }; images.blit = image_blit; images.close = (image) => { const state = apply(weak_get, handles, [image]); require_value(state && state.kind === "image", "foreign_handle"); close_handle(state); }; // Image close is idempotent only after true queue admission or native closure acknowledgement.
    const video = family("media.video", "poll seek pause close status"); video.open = async (options) => await open_rpc("media.video.open", options, "media.video", (value) => handle_field(value, "asset", ["asset"], true)); // Actual VideoHandle methods live on the returned branded instance.
    const http = family("http", "open_stream close"); http.request = http_request; http.stream = async (options) => await open_rpc("http.stream", options, "http.stream"); http.poll = http_poll; // The SDK spells the async stream factory stream(), with next()/close()/status() on its native handle.
    const sources = create(null); for (const [name, methods] of [["series", "catalogue poll close"], ["earthquakes", "poll close"], ["aircraft", "poll close"], ["boats", "poll close"], ["chess", "poll close"], ["weather", "poll close"]]) { const source = family(`sources.${name}`, methods); source.open = async (options) => await open_rpc(`sources.${name}.open`, options, `sources.${name}`); sources[name] = source; } // Each public feed returns a real SourceHandle with synchronous seeded/returned latest and status.
    sources.chess.discover = async (options) => await checked_rpc("sources.chess.discover", options, null, json_result); sources.geography = family("sources.geography", "open sample close"); sources.geography.coastlines = async (options) => await checked_rpc("sources.geography.coastlines", options, null, json_result); sources.geography.elevation = async (options) => await checked_rpc("sources.geography.elevation", options, null, (value) => binary(value, ["f32"])); sources.geography.project = (options) => native_json(native_project(options)); // The native callback validates the original object without invoking getters and returns a synchronous normalized point.
    sources.wikipedia = family("sources.wikipedia", "image close"); sources.wikipedia.search = async (options) => await checked_rpc("sources.wikipedia.search", options, null, json_result); sources.wikipedia.article = async (options) => await checked_rpc("sources.wikipedia.article", options, null, (value) => { const source = record(value, ["title", "url", "revision", "blocks", "images", "warnings", "attribution"]); require_value(is_array(source.images) && source.images.length <= 32, "native_article_images"); const first = handle_entries.length, images = []; try { for (const entry of source.images) { const copy = record(entry, ["url", "image"]); require_value(typeof copy.url === "string", "native_article_image_url"); copy.image = image_handle(copy.image); append_data(images, freeze(copy)); } source.images = freeze(images); return native_json(source); } catch (error) { discard_unpublished_handles(first); throw error; } }); sources.osm = family("sources.osm", "tiles tour close"); sources.osm.geocode = async (options) => await checked_rpc("sources.osm.geocode", options, null, json_result); sources.osm.tile = async (options) => await checked_rpc("sources.osm.tile", options, null, (value) => { const source = record(value, ["image", "paths", "attribution"]); if (source.paths !== undefined) native_json(source.paths); require_value(typeof source.attribution === "string", "native_osm_attribution"); if (source.image !== undefined) source.image = image_handle(source.image); return freeze(source); }); // The frozen SDK's public tile() is singular; legacy tiles() remains an explicit extension only.
    sources.astronomy = family("sources.astronomy", "orbits sun close"); sources.astronomy.catalogue = async (options) => await checked_rpc("sources.astronomy.catalogue", options, null, json_result); sources.astronomy.observe = (options) => { const answer = service_result(native_observe(options)); return answer.ok ? result_ok(native_json(answer.value)) : answer; }; // Native computation remains synchronous; invalid options and quota refusals return the SDK Result envelope.
    const worlds = family("worlds", "prepare close"); worlds.list = async (options) => await checked_rpc("worlds.list", options, (value) => handle_field(value, "grant", ["asset"]), json_result); worlds.open = async (options) => await open_rpc("worlds.open", options, "worlds", (value) => handle_field(value, "grant", ["asset"], true)); worlds.region = world_region; worlds.model = async (options) => await checked_rpc("worlds.model", options, (value) => handle_field(value, "world", ["worlds"]), world_model_result); // WorldHandle methods and readonly identity now match the SDK without encoding methods as JSON.
    const compute = family("compute", "poll cancel"); compute.submit = async (options) => await open_rpc("compute.submit", options, "compute", (value) => request_binary(value, "input", ["f32"])); // Native math validation, original-root admissions, actual job handles, and retained results remain concrete parent integrations.
    const gpu = family("gpu", "status run cancel"); define_property(gpu, "available", { __proto__: null, get: () => host_info.gpu?.available === true, enumerable: true }); gpu.render = async (options) => await open_rpc("gpu.render", options, "image", (value) => { request_binary(value, "vertices", ["f32"]); request_binary(value, "indices", ["u32"]); request_binary(value, "camera", ["f32"]); handle_field(value, "texture", ["image"], true); }); // Capability observation is informational; native GPU permissions and resource availability are rechecked on actual issue.
    const presentation = family("presentation", "status"); presentation.subscribe = (callback) => { require_value(typeof callback === "function", "invalid_callback"); return synchronous_factory("native_synchronous_presentation_subscription_unavailable"); }; presentation.blit_source = (options) => { try { require_frame(own(options, "frame")); lookup_handle(own(options, "source"), ["worlds"]); lookup_handle(own(options, "image"), ["image"]); } catch { return error_result("invalid_request", "Presentation blit requires current native world/image handles and frame."); } return unavailable_result("native_presentation_source_adapter_unavailable", "The native protected source-binding command adapter must be connected before this operation is available."); }; // No JSON source identity or facade call can mint protected drawing provenance.
    const tasks = family("tasks", "cancel"); tasks.poll = task_poll; tasks.yield = task_yield; tasks.cancelled = () => { require_value(typeof host_info.cancelled === "boolean", "native_cancellation_snapshot_unavailable"); return host_info.cancelled; }; // Native deadlines and one-pending-next cadence belong to the scene actor.
    const replay = family("replay", "prepare capture status cancel seek"); replay.freeze = async (options) => await checked_rpc("replay.freeze", options, (value) => { const list = own(value, "sources"); require_value(is_array(list) && list.length <= 64, "replay_source_limit"); for (let index = 0; index < list.length; index += 1) lookup_handle(own(list, string_type(index)), service_kinds); }, json_result); replay.metadata = replay_metadata; replay.eligibility = replay_eligibility; // Native replay capture still owns authorization, actual source provenance, eligibility, and storage retention.
    const host = { permissions, draw, text: { ...family("text", "metrics"), span: text_span, draw: text_span, measure: sdk_text_measure, raster: sdk_text_raster, spans: sdk_text_spans }, http, assets, cache, media: { images, video }, inputs: family("inputs", "poll"), sources, worlds, models: family("models", "decode prepare release"), textures: family("textures", "decode prepare release"), compute, gpu, presentation, tasks, status: { ...family("status", "report"), log: (level, message) => status_record("log", level, message), progress: (percent, message) => status_record("progress", "info", message, percent) }, replay }; // Existing local drawing/status extensions remain available without claiming all named native services are implemented.
    define_property(host.assets, "bundle", { __proto__: null, get: () => { require_value(bundle_handle, "native_bundle_handle_unavailable"); return bundle_handle; }, enumerable: true }); define_property(host, "package", { __proto__: null, get: () => host_info.package, enumerable: true }); define_property(host, "selection", { __proto__: null, get: () => host_info.selection, enumerable: true }); // Package/selection snapshots remain informational, and bundle requires an actual native registry handle record.
    const freeze_host = (value) => { for (const key of keys(value)) { const field = descriptor(value, key); if (field && descriptor(field, "value") && field.value && typeof field.value === "object") freeze_host(field.value); } freeze(value); }; freeze_host(host); install("__ilium_host", host); // Seal non-enumerable hooks and the public facade without invoking dynamic getters.
})(); // End the complete standalone trusted bootstrap.
