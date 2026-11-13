//! Per-instance store data of a WebAssembly guest (reference-host-runtime.md HST-065..075).
//!
//! One `StoreData` belongs to exactly one guest instance. It carries the memory and table
//! limits, the WASI state under the manual's policy (HST-076/077: no environment, no
//! arguments, no preopened directory, no sockets, output captured for the plugin log) and
//! the import set the instance may call. Nothing here is shared between instances.
//!
//! Every use of `wasmtime-wasi` in the host is in this file, so an API difference of that
//! crate is repaired in one place.

use ilium_plugin_api::Side;
use wasmtime::component::{Linker, ResourceTable};
use wasmtime::{ResourceLimiter, StoreLimits};
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::calls::ResolvedLimits;
use crate::engine::store_limits;
use crate::error::HostError;
use crate::imports::generated_dispatch::{DispatchError, ImportSet};

/// Bytes of standard output and of standard error kept per instance. A guest that writes
/// more gets a stream error from WASI; panic messages of the Rust runtime are far smaller.
pub const CAPTURED_OUTPUT_BYTES: usize = 64 * 1024;

/// Which plugin instance a store belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestIdentity {
    /// The plugin id, or an empty string for a guest instantiated without one (tests).
    pub plugin: String,
    /// The side the instance runs on.
    pub side: Side,
    /// The granted capabilities; they decide which import interfaces are unlocked.
    pub granted: Vec<String>,
}

impl GuestIdentity {
    /// An identity for guests that are not part of a plugin (boundary tests).
    pub fn anonymous() -> Self {
        Self {
            plugin: String::new(),
            side: Side::Server,
            granted: Vec::new(),
        }
    }
}

/// An import call that could not be routed, kept so the export call that triggered it
/// reports the precise trap code instead of a generic one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportFault {
    /// Why the call failed; `None` when the host import panicked.
    pub error: Option<DispatchError>,
    /// The code the triggering export call reports.
    pub code: &'static str,
    /// The component instance name of the import.
    pub instance: String,
    /// The function name of the import.
    pub function: String,
}

/// The WASI state of one instance.
pub struct WasiState {
    context: WasiCtx,
    table: ResourceTable,
    stdout: MemoryOutputPipe,
    stderr: MemoryOutputPipe,
    stdout_read: usize,
    stderr_read: usize,
}

impl WasiState {
    fn new() -> Self {
        let stdout = MemoryOutputPipe::new(CAPTURED_OUTPUT_BYTES);
        let stderr = MemoryOutputPipe::new(CAPTURED_OUTPUT_BYTES);
        // The builder starts with nothing inherited: no environment, no arguments, no
        // preopened directory, no network. Only the two capture pipes are added.
        let context = WasiCtxBuilder::new()
            .stdout(stdout.clone())
            .stderr(stderr.clone())
            .build();
        Self {
            context,
            table: ResourceTable::new(),
            stdout,
            stderr,
            stdout_read: 0,
            stderr_read: 0,
        }
    }

    /// The text written to standard output and standard error since the last call, in
    /// that order. Invalid UTF-8 is replaced.
    pub fn take_output(&mut self) -> (String, String) {
        let stdout = self.stdout.contents();
        let stderr = self.stderr.contents();
        let out =
            String::from_utf8_lossy(&stdout[self.stdout_read.min(stdout.len())..]).into_owned();
        let err =
            String::from_utf8_lossy(&stderr[self.stderr_read.min(stderr.len())..]).into_owned();
        self.stdout_read = stdout.len();
        self.stderr_read = stderr.len();
        (out, err)
    }
}

/// The data of one guest store.
pub struct StoreData {
    limits: StoreLimits,
    /// The limit a growth request broke during the current call, if any.
    pub(crate) breach: Option<&'static str>,
    /// The import call that failed during the current call, if any.
    pub(crate) import_fault: Option<ImportFault>,
    /// The host implementations this instance may call.
    pub(crate) imports: ImportSet,
    /// The WASI state.
    pub(crate) wasi: WasiState,
    /// Which plugin instance this is.
    pub(crate) identity: GuestIdentity,
}

impl StoreData {
    /// Store data with the resolved limits, an import set and a closed WASI context.
    pub fn new(limits: &ResolvedLimits, imports: ImportSet, identity: GuestIdentity) -> Self {
        Self {
            limits: store_limits(limits),
            breach: None,
            import_fault: None,
            imports,
            wasi: WasiState::new(),
            identity,
        }
    }

    /// Which plugin instance this store belongs to.
    pub fn identity(&self) -> &GuestIdentity {
        &self.identity
    }

    /// The import set, for the runtime to adjust between calls (for example after a grant
    /// was revoked).
    pub fn imports_mut(&mut self) -> &mut ImportSet {
        &mut self.imports
    }

    /// Clears the per-call fault markers before an export is called.
    pub(crate) fn begin_call(&mut self) {
        self.breach = None;
        self.import_fault = None;
    }
}

impl std::fmt::Debug for StoreData {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoreData")
            .field("identity", &self.identity)
            .field("imports", &self.imports)
            .finish_non_exhaustive()
    }
}

impl WasiView for StoreData {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi.context,
            table: &mut self.wasi.table,
        }
    }
}

impl ResourceLimiter for StoreData {
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

/// Adds the WASI preview 2 interfaces to a linker, in their asynchronous form because
/// guest calls run through `call_async` (epoch yields need it).
pub fn add_wasi_to_linker(linker: &mut Linker<StoreData>) -> Result<(), HostError> {
    wasmtime_wasi::p2::add_to_linker_async(linker)
        .map_err(|error| HostError::EngineError(format!("linking WASI failed: {error}")))
}
