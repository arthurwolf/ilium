//! Dynamic component backend. Imports remain closed until grant-scoped adapters exist.
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
use wasmtime::component::{Instance, Linker, Type, Val};
use wasmtime::{ResourceLimiter, Store, StoreLimits};

use crate::calls::{batch_call_class, deadline_for, streak_limit, CallClass, ResolvedLimits, Trap};
use crate::engine::{store_limits, CompiledComponent, PluginEngine};
use crate::error::HostError;
use crate::instance::{ExportCall, ExportReply, GuestExports};

const LIFECYCLE: &str = "ilium:exports/lifecycle@0.1.0";
const HANDLER: &str = "ilium:exports/handler@0.1.0";

struct GuestState {
    limits: StoreLimits,
    breach: Option<&'static str>,
}

impl ResourceLimiter for GuestState {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let result = self.limits.memory_growing(current, desired, maximum);
        if !matches!(result, Ok(true)) {
            self.breach = Some("trap.memory");
        }
        result
    }
    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let result = self.limits.table_growing(current, desired, maximum);
        if !matches!(result, Ok(true)) {
            self.breach = Some("trap.table");
        }
        result
    }
    fn memory_grow_failed(&mut self, error: wasmtime::Error) -> wasmtime::Result<()> {
        self.breach = Some("trap.memory");
        self.limits.memory_grow_failed(error)
    }
    fn table_grow_failed(&mut self, error: wasmtime::Error) -> wasmtime::Result<()> {
        self.breach = Some("trap.table");
        self.limits.table_grow_failed(error)
    }
    fn instances(&self) -> usize {
        self.limits.instances()
    }
    fn tables(&self) -> usize {
        self.limits.tables()
    }
    fn memories(&self) -> usize {
        self.limits.memories()
    }
}

/// A real Wasmtime store and component instance, with no ambient import authority.
pub struct WasmGuest {
    engine: Arc<PluginEngine>,
    store: Store<GuestState>,
    instance: Instance,
    limits: ResolvedLimits,
}

impl WasmGuest {
    /// Instantiates an already validated component without linking any imports.
    /// Even type-only imports are rejected until their authorization is modeled.
    pub fn instantiate(
        engine: Arc<PluginEngine>,
        compiled: &CompiledComponent,
        limits: ResolvedLimits,
    ) -> Result<Self, HostError> {
        let runtime = engine.runtime_engine();
        if !wasmtime::Engine::same(runtime, compiled.component.engine()) {
            return Err(HostError::EngineError(
                "component belongs to another engine".into(),
            ));
        }
        if let Some((name, _)) = compiled.component.component_type().imports(runtime).next() {
            return Err(HostError::EngineError(format!(
                "component import {name} has no effective-grant scoped handler"
            )));
        }
        let mut store = Store::new(
            runtime,
            GuestState {
                limits: store_limits(&limits),
                breach: None,
            },
        );
        store.limiter(|state| state);
        // Core start functions execute during instantiation too.
        engine.arm_deadline(
            &mut store,
            deadline_for(CallClass::Activate, &limits).as_millis() as u64,
            false,
        );
        let linker = Linker::<GuestState>::new(runtime);
        let instance = block_on(linker.instantiate_async(&mut store, &compiled.component))
            .map_err(|error| HostError::EngineError(error.to_string()))?;
        engine.record_instantiation();
        Ok(Self {
            engine,
            store,
            instance,
            limits,
        })
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
        self.store.data_mut().breach = None;
        let mut results = [Val::Bool(false)];
        // Wasmtime 49 call_async includes canonical post-return cleanup.
        block_on(export.call_async(&mut self.store, &arguments, &mut results)).map_err(
            |error| {
                let code = self.store.data().breach.unwrap_or_else(|| {
                    match error.downcast_ref::<wasmtime::Trap>() {
                        Some(wasmtime::Trap::Interrupt) => "trap.deadline",
                        Some(wasmtime::Trap::MemoryOutOfBounds) => "trap.memory",
                        Some(wasmtime::Trap::TableOutOfBounds) => "trap.table",
                        Some(_) => "trap.panic",
                        None => "trap.abi",
                    }
                });
                trap(code, interface, function)
            },
        )?;
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

    fn render(&mut self, _view: ViewTarget, _area: Area) -> Result<UiTree, Trap> {
        Err(trap("trap.abi", "ilium:exports/views@0.1.0", "render"))
    }

    fn call_export(&mut self, _call: ExportCall) -> Result<ExportReply, Trap> {
        // Optional provider interfaces require their own purity and argument guards.
        Err(trap("trap.abi", "unsupported-provider", "call-export"))
    }
}

fn encode(value: &impl Serialize, interface: &str, function: &str) -> Result<Value, Trap> {
    serde_json::to_value(value).map_err(|_| trap("trap.abi", interface, function))
}

fn trap(code: &'static str, interface: &str, function: &str) -> Trap {
    Trap {
        code: Code::from_static(code),
        interface: interface.into(),
        function: function.into(),
    }
}

// Generated records use kebab-case names and externally tagged variants. The
// runtime type determines order, widths and allowed cases; JSON is only a typed
// serialization bridge, never a component interpreter. Resources are forbidden.
fn lower(value: &Value, ty: &Type) -> Result<Val, ()> {
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

fn lift(value: &Val, ty: &Type) -> Result<Value, ()> {
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
fn block_on<F: Future>(future: F) -> F::Output {
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
