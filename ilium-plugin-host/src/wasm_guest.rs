//! Dynamic component backend. `instantiate` links nothing; `instantiate_linked` links WASI and
//! the generated import table and gives the instance its own import set.
//!
//! Callers must validate the component world against the package before constructing
//! this backend. Calls are synchronous at the GuestExports boundary and belong on
//! the scheduler's guest worker, never its orchestration thread.

use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::Submission;
use ilium_plugin_api::generated::types::layout::Area;
use ilium_plugin_api::generated::types::lifecycle::{Activation, DeactivationReason};
use ilium_plugin_api::generated::types::ui::{UiTree, ViewTarget};
use ilium_plugin_api::{Code, PluginError};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use wasmtime::component::{Instance, Type, Val};
use wasmtime::Store;

use crate::calls::{batch_call_class, deadline_for, streak_limit, CallClass, ResolvedLimits, Trap};
use crate::engine::{CompiledComponent, PluginEngine};
use crate::error::HostError;
use crate::imports::generated_dispatch::ImportSet;
use crate::instance::{ExportCall, ExportReply, GuestExports};
use crate::linker::RuntimeLinker;
use crate::store_data::{GuestIdentity, StoreData};

const LIFECYCLE: &str = "ilium:exports/lifecycle@0.1.0";
const HANDLER: &str = "ilium:exports/handler@0.1.0";
const VIEWS: &str = "ilium:exports/views@0.1.0";
const HOOK_HANDLER: &str = "ilium:exports/hook-handler@0.1.0";
const JOB_RUNNER: &str = "ilium:exports/job-runner@0.1.0";
const ANNOTATOR: &str = "ilium:exports/annotator@0.1.0";
const TOKEN_PROVIDER: &str = "ilium:exports/token-provider@0.1.0";
const CATALOG_PROVIDER: &str = "ilium:exports/catalog-provider@0.1.0";
const MENU_PROVIDER: &str = "ilium:exports/menu-provider@0.1.0";
const VIRTUAL_ROWS_PROVIDER: &str = "ilium:exports/virtual-rows-provider@0.1.0";
const SERVICE_PROVIDER: &str = "ilium:exports/service-provider@0.1.0";

/// A real Wasmtime store and component instance, with no ambient import authority.
pub struct WasmGuest {
    engine: Arc<PluginEngine>,
    store: Store<StoreData>,
    instance: Instance,
    limits: ResolvedLimits,
}

impl WasmGuest {
    /// Instantiates an already validated component without linking any imports.
    /// A component with any import, even a type-only one, is rejected.
    pub fn instantiate(
        engine: Arc<PluginEngine>,
        compiled: &CompiledComponent,
        limits: ResolvedLimits,
    ) -> Result<Self, HostError> {
        let runtime = engine.runtime_engine();
        if let Some((name, _)) = compiled.component.component_type().imports(runtime).next() {
            return Err(HostError::EngineError(format!(
                "component import {name} has no effective-grant scoped handler"
            )));
        }
        let linker = RuntimeLinker::closed(&engine);
        Self::instantiate_linked(
            engine,
            compiled,
            limits,
            &linker,
            ImportSet::default(),
            GuestIdentity::anonymous(),
        )
    }

    /// Instantiates an already validated component against the process linker. `imports`
    /// holds the host implementations this instance may call; an interface missing from
    /// it traps with `host.capability-not-requested` when called; a component that imports
    /// such an interface at all is refused here with `component.import-ungranted`.
    /// An interface the grants unlock but this process does not implement instantiates
    /// and traps with `host.engine-error` when it is called.
    pub fn instantiate_linked(
        engine: Arc<PluginEngine>,
        compiled: &CompiledComponent,
        limits: ResolvedLimits,
        linker: &RuntimeLinker,
        imports: ImportSet,
        identity: GuestIdentity,
    ) -> Result<Self, HostError> {
        let runtime = engine.runtime_engine();
        if !wasmtime::Engine::same(runtime, compiled.component.engine()) {
            return Err(HostError::EngineError(
                "component belongs to another engine".into(),
            ));
        }
        for (name, item) in compiled.component.component_type().imports(runtime) {
            // An interface imported only for its types needs no grant; one imported for
            // a function does.
            let has_functions = match &item.ty {
                wasmtime::component::types::ComponentItem::ComponentInstance(instance) => {
                    instance.exports(runtime).any(|(_, export)| {
                        matches!(
                            export.ty,
                            wasmtime::component::types::ComponentItem::ComponentFunc(_)
                        )
                    })
                }
                _ => false,
            };
            if has_functions
                && crate::imports::generated_dispatch::import_interface(name).is_some()
                && !crate::linker::interface_unlocked(name, &identity.granted)
            {
                return Err(HostError::EngineError(format!(
                    "component.import-ungranted: {name} is not unlocked by any granted capability"
                )));
            }
        }
        let mut store = Store::new(runtime, StoreData::new(&limits, imports, identity));
        store.limiter(|state| state);
        // Core start functions execute during instantiation too.
        engine.arm_deadline(
            &mut store,
            deadline_for(CallClass::Activate, &limits).as_millis() as u64,
            false,
        );
        let instance = block_on(
            linker
                .linker()
                .instantiate_async(&mut store, &compiled.component),
        )
        .map_err(|error| {
            if store.data().breach.is_some() || error.downcast_ref::<wasmtime::Trap>().is_some() {
                let code = store.data().breach.unwrap_or_else(|| {
                    match error.downcast_ref::<wasmtime::Trap>() {
                        Some(wasmtime::Trap::Interrupt) => "trap.deadline",
                        Some(wasmtime::Trap::MemoryOutOfBounds) => "trap.memory",
                        Some(wasmtime::Trap::TableOutOfBounds) => "trap.table",
                        _ => "trap.panic",
                    }
                });
                HostError::ActivationTrap(trap(code, LIFECYCLE, "activate"))
            } else {
                HostError::EngineError(error.to_string())
            }
        })?;
        engine.record_instantiation();
        Ok(Self {
            engine,
            store,
            instance,
            limits,
        })
    }

    /// The engine, store, instance and limits, for a guest kind that calls resource
    /// exports itself (`scene_guest`), after the same import checks and instantiation.
    pub(crate) fn into_parts(
        self,
    ) -> (
        Arc<PluginEngine>,
        Store<StoreData>,
        Instance,
        ResolvedLimits,
    ) {
        (self.engine, self.store, self.instance, self.limits)
    }

    /// The store data, for the runtime to adjust the import set between calls.
    pub fn store_data_mut(&mut self) -> &mut StoreData {
        self.store.data_mut()
    }

    /// Text the guest wrote to standard output and standard error since the last call.
    pub fn take_output(&mut self) -> (String, String) {
        self.store.data_mut().wasi.take_output()
    }

    /// Sends what the guest wrote to standard output and standard error to its log import
    /// (HST-077): output lines at info level, error lines (panic messages) at error level.
    /// Without a log import the text is dropped.
    fn forward_output(&mut self) {
        use ilium_plugin_api::generated::host::log::Level;
        let data = self.store.data_mut();
        let (output, errors) = data.wasi.take_output();
        let Some(log) = data.imports.log.as_deref_mut() else {
            return;
        };
        for line in output.lines().filter(|line| !line.trim().is_empty()) {
            log.log(Level::Info, line.to_owned());
        }
        for line in errors.lines().filter(|line| !line.trim().is_empty()) {
            log.log(Level::Error, line.to_owned());
        }
    }

    /// Whether the component exports an interface (by its versioned instance name).
    pub fn exports(&mut self, interface: &str) -> bool {
        self.instance
            .get_export_index(&mut self.store, None, interface)
            .is_some()
    }

    fn invoke(
        &mut self,
        interface: &str,
        function: &str,
        class: CallClass,
        arguments: &[Value],
    ) -> Result<(Val, Type), Trap> {
        let invalid = || trap("trap.abi", interface, function);
        let interface_index = self
            .instance
            .get_export_index(&mut self.store, None, interface)
            .ok_or_else(invalid)?;
        let function_index = self
            .instance
            .get_export_index(&mut self.store, Some(&interface_index), function)
            .ok_or_else(invalid)?;
        let export = self
            .instance
            .get_func(&mut self.store, function_index)
            .ok_or_else(invalid)?;
        let signature = export.ty(&self.store);
        let parameters: Vec<_> = signature.params().collect();
        if parameters.len() != arguments.len() || signature.results().len() != 1 {
            return Err(invalid());
        }
        let result_type = signature.results().next().ok_or_else(invalid)?;
        let arguments = parameters
            .iter()
            .zip(arguments)
            .map(|((_, ty), value)| lower(value, ty))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| invalid())?;
        self.engine.arm_deadline(
            &mut self.store,
            deadline_for(class, &self.limits).as_millis() as u64,
            streak_limit(class).is_some(),
        );
        self.store.data_mut().begin_call();
        let mut results = [Val::Bool(false)];
        // Wasmtime 49 call_async includes canonical post-return cleanup.
        let called = block_on(export.call_async(&mut self.store, &arguments, &mut results));
        self.forward_output();
        called.map_err(|error| {
            let data = self.store.data();
            let import_code = data.import_fault.as_ref().map(|fault| fault.code);
            let code = data.breach.or(import_code).unwrap_or_else(|| {
                match error.downcast_ref::<wasmtime::Trap>() {
                    Some(wasmtime::Trap::Interrupt) => "trap.deadline",
                    Some(wasmtime::Trap::MemoryOutOfBounds) => "trap.memory",
                    Some(wasmtime::Trap::TableOutOfBounds) => "trap.table",
                    Some(_) => "trap.panic",
                    None => "trap.abi",
                }
            });
            trap(code, interface, function)
        })?;
        Ok((results.into_iter().next().ok_or_else(invalid)?, result_type))
    }

    fn invoke_decoded<T: DeserializeOwned>(
        &mut self,
        interface: &str,
        function: &str,
        class: CallClass,
        arguments: &[Value],
    ) -> Result<T, Trap> {
        let (result, ty) = self.invoke(interface, function, class, arguments)?;
        lift(&result, &ty)
            .and_then(|value| serde_json::from_value(value).map_err(|_| ()))
            .map_err(|_| trap("trap.abi", interface, function))
    }
}

impl GuestExports for WasmGuest {
    fn activate(&mut self, activation: Activation) -> Result<Result<(), PluginError>, Trap> {
        let argument = encode(&activation, LIFECYCLE, "activate")?;
        self.invoke_decoded(LIFECYCLE, "activate", CallClass::Activate, &[argument])
    }

    fn deactivate(&mut self, reason: DeactivationReason) -> Result<Option<Vec<u8>>, Trap> {
        let argument = encode(&reason, LIFECYCLE, "deactivate")?;
        self.invoke_decoded(LIFECYCLE, "deactivate", CallClass::Deactivate, &[argument])
    }

    fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        let class = batch_call_class(&batch);
        let argument = encode(&batch, HANDLER, "on-events")?;
        self.invoke_decoded(HANDLER, "on-events", class, &[argument])
    }

    fn render(&mut self, view: ViewTarget, area: Area) -> Result<UiTree, Trap> {
        let view = encode(&view, VIEWS, "render")?;
        let area = encode(&area, VIEWS, "render")?;
        self.invoke_decoded(VIEWS, "render", CallClass::Render, &[view, area])
    }

    fn call_export(&mut self, call: ExportCall) -> Result<ExportReply, Trap> {
        match call {
            ExportCall::Hook(call) => {
                let argument = encode(&call, HOOK_HANDLER, "handle-hook")?;
                self.invoke_decoded(HOOK_HANDLER, "handle-hook", CallClass::Event, &[argument])
                    .map(ExportReply::HookResult)
            }
            ExportCall::Job { kind, input } => {
                let kind = encode(&kind, JOB_RUNNER, "run")?;
                let input = encode(&input, JOB_RUNNER, "run")?;
                self.invoke_decoded(JOB_RUNNER, "run", CallClass::Job, &[kind, input])
                    .map(ExportReply::Job)
            }
            // WIT `migrator.migrate(from, to)` carries no data, while this call carries the
            // stored bytes: the two contracts disagree, so the call is refused until one of
            // them changes (docs/tasks/02-foundation-contract.md section 11).
            ExportCall::Migrate { .. } => {
                Err(trap("trap.abi", "ilium:exports/migrator@0.1.0", "migrate"))
            }
            ExportCall::Annotate {
                buffer,
                revision,
                first_line,
                lines,
            } => {
                let arguments = [
                    encode(&buffer, ANNOTATOR, "annotate")?,
                    encode(&revision, ANNOTATOR, "annotate")?,
                    encode(&first_line, ANNOTATOR, "annotate")?,
                    encode(&lines, ANNOTATOR, "annotate")?,
                ];
                self.invoke_decoded(ANNOTATOR, "annotate", CallClass::Annotate, &arguments)
                    .map(ExportReply::Annotations)
            }
            ExportCall::Tokens {
                buffer,
                first_line,
                lines,
                state_in,
            } => {
                let arguments = [
                    encode(&buffer, TOKEN_PROVIDER, "tokens")?,
                    encode(&first_line, TOKEN_PROVIDER, "tokens")?,
                    encode(&lines, TOKEN_PROVIDER, "tokens")?,
                    encode(&state_in, TOKEN_PROVIDER, "tokens")?,
                ];
                let (tokens, state_out) =
                    self.invoke_decoded(TOKEN_PROVIDER, "tokens", CallClass::Tokens, &arguments)?;
                Ok(ExportReply::Tokens(tokens, state_out))
            }
            ExportCall::QueryCatalog(query) => {
                let argument = encode(&query, CATALOG_PROVIDER, "query-catalog")?;
                self.invoke_decoded(
                    CATALOG_PROVIDER,
                    "query-catalog",
                    CallClass::QueryCatalog,
                    &[argument],
                )
                .map(ExportReply::CatalogPage)
            }
            ExportCall::MenuItems(context) => {
                let argument = encode(&context, MENU_PROVIDER, "menu-items")?;
                self.invoke_decoded(
                    MENU_PROVIDER,
                    "menu-items",
                    CallClass::MenuItems,
                    &[argument],
                )
                .map(ExportReply::MenuEntries)
            }
            ExportCall::ListVirtualChildren {
                row_kind,
                parent,
                cursor,
            } => {
                let arguments = [
                    encode(&row_kind, VIRTUAL_ROWS_PROVIDER, "list-virtual-children")?,
                    encode(&parent, VIRTUAL_ROWS_PROVIDER, "list-virtual-children")?,
                    encode(&cursor, VIRTUAL_ROWS_PROVIDER, "list-virtual-children")?,
                ];
                self.invoke_decoded(
                    VIRTUAL_ROWS_PROVIDER,
                    "list-virtual-children",
                    CallClass::ListVirtualChildren,
                    &arguments,
                )
                .map(ExportReply::VirtualChildren)
            }
            ExportCall::ServiceHandle(call) => {
                let argument = encode(&call, SERVICE_PROVIDER, "handle")?;
                self.invoke_decoded(SERVICE_PROVIDER, "handle", CallClass::Event, &[argument])
                    .map(ExportReply::ServiceReply)
            }
        }
    }
}

fn encode(value: &impl Serialize, interface: &str, function: &str) -> Result<Value, Trap> {
    serde_json::to_value(value).map_err(|_| trap("trap.abi", interface, function))
}

pub(crate) fn trap(code: &'static str, interface: &str, function: &str) -> Trap {
    Trap {
        code: Code::from_static(code),
        interface: interface.into(),
        function: function.into(),
    }
}

// Generated records use kebab-case names and externally tagged variants. The
// runtime type determines order, widths and allowed cases; JSON is only a typed
// serialization bridge, never a component interpreter. Resources are forbidden.
pub(crate) fn lower(value: &Value, ty: &Type) -> Result<Val, ()> {
    macro_rules! unsigned {
        ($variant:ident, $target:ty) => {
            Val::$variant(<$target>::try_from(value.as_u64().ok_or(())?).map_err(|_| ())?)
        };
    }
    macro_rules! signed {
        ($variant:ident, $target:ty) => {
            Val::$variant(<$target>::try_from(value.as_i64().ok_or(())?).map_err(|_| ())?)
        };
    }
    Ok(match ty {
        Type::Bool => Val::Bool(value.as_bool().ok_or(())?),
        Type::U8 => unsigned!(U8, u8),
        Type::U16 => unsigned!(U16, u16),
        Type::U32 => unsigned!(U32, u32),
        Type::U64 => Val::U64(value.as_u64().ok_or(())?),
        Type::S8 => signed!(S8, i8),
        Type::S16 => signed!(S16, i16),
        Type::S32 => signed!(S32, i32),
        Type::S64 => Val::S64(value.as_i64().ok_or(())?),
        Type::Float32 => Val::Float32(value.as_f64().ok_or(())? as f32),
        Type::Float64 => Val::Float64(value.as_f64().ok_or(())?),
        Type::String => Val::String(value.as_str().ok_or(())?.into()),
        Type::Char => {
            let mut chars = value.as_str().ok_or(())?.chars();
            let character = chars.next().ok_or(())?;
            if chars.next().is_some() {
                return Err(());
            }
            Val::Char(character)
        }
        Type::List(list) => Val::List(
            value
                .as_array()
                .ok_or(())?
                .iter()
                .map(|v| lower(v, &list.ty()))
                .collect::<Result<_, _>>()?,
        ),
        Type::Record(record) => {
            let object = value.as_object().ok_or(())?;
            let fields: Vec<_> = record.fields().collect();
            if object.len() != fields.len() {
                return Err(());
            }
            Val::Record(
                fields
                    .into_iter()
                    .map(|field| {
                        Ok((
                            field.name.into(),
                            lower(object.get(field.name).ok_or(())?, &field.ty)?,
                        ))
                    })
                    .collect::<Result<_, ()>>()?,
            )
        }
        Type::Tuple(tuple) => {
            let values = value.as_array().ok_or(())?;
            let types: Vec<_> = tuple.types().collect();
            if types.len() != values.len() {
                return Err(());
            }
            Val::Tuple(
                values
                    .iter()
                    .zip(types)
                    .map(|(v, t)| lower(v, &t))
                    .collect::<Result<_, _>>()?,
            )
        }
        Type::Enum(enumeration) => {
            let name = value.as_str().ok_or(())?;
            if !enumeration.names().any(|case| case == name) {
                return Err(());
            }
            Val::Enum(name.into())
        }
        Type::Option(option) => Val::Option(if value.is_null() {
            None
        } else {
            Some(Box::new(lower(value, &option.ty())?))
        }),
        Type::Variant(variant) => {
            let (name, payload) = tag(value)?;
            let case = variant.cases().find(|case| case.name == name).ok_or(())?;
            Val::Variant(
                name.into(),
                match case.ty {
                    Some(ty) => Some(Box::new(lower(payload.ok_or(())?, &ty)?)),
                    None if payload.is_none() => None,
                    _ => return Err(()),
                },
            )
        }
        Type::Result(result) => {
            let (name, payload) = tag(value)?;
            let (ok, ty) = match name {
                "Ok" => (true, result.ok()),
                "Err" => (false, result.err()),
                _ => return Err(()),
            };
            let payload = match ty {
                Some(ty) => Some(Box::new(lower(payload.ok_or(())?, &ty)?)),
                None if payload.is_none_or(Value::is_null) => None,
                _ => return Err(()),
            };
            Val::Result(if ok { Ok(payload) } else { Err(payload) })
        }
        Type::Flags(flags) => {
            let bits = value.as_u64().ok_or(())?;
            let names: Vec<_> = flags.names().collect();
            if names.len() < 64 && bits >> names.len() != 0 {
                return Err(());
            }
            Val::Flags(
                names
                    .into_iter()
                    .enumerate()
                    .filter(|(index, _)| *index < 64 && bits & (1u64 << index) != 0)
                    .map(|(_, name)| name.into())
                    .collect(),
            )
        }
        _ => return Err(()),
    })
}

fn tag(value: &Value) -> Result<(&str, Option<&Value>), ()> {
    if let Some(name) = value.as_str() {
        return Ok((name, None));
    }
    let object = value.as_object().ok_or(())?;
    if object.len() != 1 {
        return Err(());
    }
    object
        .iter()
        .next()
        .map(|(name, payload)| (name.as_str(), Some(payload)))
        .ok_or(())
}

pub(crate) fn lift(value: &Val, ty: &Type) -> Result<Value, ()> {
    Ok(match value {
        Val::Bool(v) => json!(v),
        Val::S8(v) => json!(v),
        Val::U8(v) => json!(v),
        Val::S16(v) => json!(v),
        Val::U16(v) => json!(v),
        Val::S32(v) => json!(v),
        Val::U32(v) => json!(v),
        Val::S64(v) => json!(v),
        Val::U64(v) => json!(v),
        Val::Float32(v) if v.is_finite() => json!(v),
        Val::Float64(v) if v.is_finite() => json!(v),
        Val::Char(v) => json!(v.to_string()),
        Val::String(v) | Val::Enum(v) => json!(v),
        Val::List(values) => {
            let Type::List(list) = ty else {
                return Err(());
            };
            Value::Array(
                values
                    .iter()
                    .map(|value| lift(value, &list.ty()))
                    .collect::<Result<_, _>>()?,
            )
        }
        Val::Tuple(values) => {
            let Type::Tuple(tuple) = ty else {
                return Err(());
            };
            Value::Array(
                values
                    .iter()
                    .zip(tuple.types())
                    .map(|(value, ty)| lift(value, &ty))
                    .collect::<Result<_, _>>()?,
            )
        }
        Val::Record(fields) => {
            let Type::Record(record) = ty else {
                return Err(());
            };
            Value::Object(
                fields
                    .iter()
                    .zip(record.fields())
                    .map(|((name, value), field)| Ok((name.clone(), lift(value, &field.ty)?)))
                    .collect::<Result<_, ()>>()?,
            )
        }
        Val::Variant(name, None) => json!(name),
        Val::Variant(name, Some(value)) => {
            let mut object = serde_json::Map::new();
            let Type::Variant(variant) = ty else {
                return Err(());
            };
            let payload_type = variant
                .cases()
                .find(|case| case.name == name)
                .and_then(|case| case.ty)
                .ok_or(())?;
            object.insert(name.clone(), lift(value, &payload_type)?);
            Value::Object(object)
        }
        Val::Option(None) => Value::Null,
        Val::Option(Some(value)) => {
            let Type::Option(option) = ty else {
                return Err(());
            };
            lift(value, &option.ty())?
        }
        Val::Result(result) => {
            let (name, payload) = match result {
                Ok(value) => ("Ok", value),
                Err(value) => ("Err", value),
            };
            let mut object = serde_json::Map::new();
            object.insert(name.into(), {
                let Type::Result(result_type) = ty else {
                    return Err(());
                };
                let payload_type = if result.is_ok() {
                    result_type.ok()
                } else {
                    result_type.err()
                };
                match (payload.as_deref(), payload_type) {
                    (Some(value), Some(ty)) => lift(value, &ty)?,
                    (None, None) => Value::Null,
                    _ => return Err(()),
                }
            });
            Value::Object(object)
        }
        Val::Flags(values) => {
            let Type::Flags(flags) = ty else {
                return Err(());
            };
            let mut bits = 0u64;
            for (index, name) in flags.names().enumerate() {
                if values.iter().any(|value| value == name) {
                    bits |= 1u64.checked_shl(index as u32).ok_or(())?;
                }
            }
            json!(bits)
        }
        _ => return Err(()),
    })
}

struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

// No imports can wait for external I/O. Epoch yields reschedule the fiber by
// waking this thread. park/unpark retains a wake occurring before park.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(result) => return result,
            Poll::Pending => std::thread::park(),
        }
    }
}
