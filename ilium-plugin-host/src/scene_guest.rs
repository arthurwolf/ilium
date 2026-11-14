//! Scene renderer guests (reference-wit.md section 7, `ilium:anim`).
//!
//! A scene plugin is a component of the `ilium:anim/base` world that exports
//! `ilium:exports/renderer@0.1.0`: a `scene-renderer` resource the host constructs from a
//! spec and calls once per frame with a borrowed `frame-surface` it owns. This module is the
//! host half of that contract:
//!
//! - [`SceneSurface`] is the real frame buffer behind `ilium:anim/surface@0.1.0`. It
//!   implements the generated [`HostSurface`] trait and keeps one grey value per pixel (every
//!   pixel format is converted on write) plus an optional colour per terminal cell.
//! - [`SceneGuest`] instantiates the component on a linker of its own that defines the
//!   `frame-surface` resource over that buffer, constructs the renderer and drives it.
//!
//! The pixel field is the Braille field of the ambient scenes: two pixels per terminal column
//! and four per row. Calls are synchronous and belong on the thread that renders the scene
//! (the ambient worker), never the UI thread.

use std::sync::{Arc, Mutex, MutexGuard};

use ilium_plugin_api::generated::anim::surface::{
    FrameReceipt, FrameSurfaceHandle, GlyphCell, HostSurface, OccupancyGrid, ScenePointer,
};
use ilium_plugin_api::generated::anim::Readiness;
use ilium_plugin_api::generated::types::ui::PixelFormat;
use ilium_plugin_api::{ErrorCode, PluginError};
use wasmtime::component::{Instance, Linker, Resource, ResourceAny, ResourceType, Type, Val};
use wasmtime::{Store, StoreContextMut};

use crate::calls::{deadline_for, CallClass, ResolvedLimits, Trap};
use crate::engine::{CompiledComponent, PluginEngine};
use crate::error::HostError;
use crate::imports::generated_dispatch::ImportSet;
use crate::linker::build_runtime_linker;
use crate::store_data::{GuestIdentity, StoreData};
use crate::wasm_guest::{block_on, lift, trap, WasmGuest};

/// The renderer export of a scene component.
pub const RENDERER: &str = "ilium:exports/renderer@0.1.0";
/// The surface import a scene draws into.
pub const SURFACE: &str = "ilium:anim/surface@0.1.0";

/// Pixels per terminal column and row: the Braille field of the ambient scenes.
pub const PIXELS_PER_COLUMN: u32 = 2;
/// See [`PIXELS_PER_COLUMN`].
pub const PIXELS_PER_ROW: u32 = 4;

/// The largest frame a scene is given, in pixels (a 4096 x 4096 field). Larger requests are
/// refused before any guest call so a corrupt terminal size cannot allocate gigabytes.
pub const MAX_SURFACE_PIXELS: u64 = 1 << 24;

/// At most this many glyph cells per frame (reference-wit.md, `write-glyphs`).
pub const MAX_GLYPH_CELLS: usize = 131_072;

const SURFACE_METHODS: [&str; 10] = [
    "[method]frame-surface.size",
    "[method]frame-surface.write-rows",
    "[method]frame-surface.write-cell-colors",
    "[method]frame-surface.occupancy",
    "[method]frame-surface.pointer",
    "[method]frame-surface.present",
    "[method]frame-surface.write-glyphs",
    "[method]frame-surface.clear-glyphs",
    "[method]frame-surface.set-owner-ids",
    "[method]frame-surface.last-receipt",
];

/// Host type of the `frame-surface` resource. Its representation is the number of the frame
/// the handle was lent for, so a handle kept past its `render` call is recognised as stale.
#[derive(Debug)]
pub struct FrameSurfaceResource;

/// One frame a scene presented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedFrame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Width in terminal cells.
    pub columns: u32,
    /// Height in terminal cells.
    pub rows: u32,
    /// One grey value per pixel, row by row (0 is no ink, 255 full ink).
    pub pixels: Vec<u8>,
    /// One `0xRRGGBB` value per cell, row by row, when the scene wrote cell colours.
    pub cell_colors: Option<Vec<u32>>,
}

/// The frame buffer behind `ilium:anim/surface`: one open frame at a time, lent to the guest
/// for the duration of one `render` call.
#[derive(Debug, Default)]
pub struct SceneSurface {
    /// The representation of the handle lent for the open frame; 0 before the first frame.
    handle: u32,
    open: bool,
    frame: u64,
    width: u32,
    height: u32,
    columns: u32,
    rows: u32,
    pixels: Vec<u8>,
    colors: Vec<u32>,
    colors_written: bool,
    presented: Option<PresentedFrame>,
    /// Frames that ended without `present` since the last presented one.
    dropped: u32,
    receipt: Option<FrameReceipt>,
    pointer: Option<ScenePointer>,
    occupancy: Option<OccupancyGrid>,
}

impl SceneSurface {
    /// Opens a frame of `columns` x `rows` cells and returns the representation of the handle
    /// to lend for it. `None` when the field is empty or larger than [`MAX_SURFACE_PIXELS`].
    pub fn begin(&mut self, columns: u32, rows: u32) -> Option<u32> {
        let width = columns.checked_mul(PIXELS_PER_COLUMN)?;
        let height = rows.checked_mul(PIXELS_PER_ROW)?;
        let pixels = u64::from(width) * u64::from(height);
        if pixels == 0 || pixels > MAX_SURFACE_PIXELS {
            return None;
        }
        self.frame += 1;
        self.handle = self.handle.wrapping_add(1).max(1);
        self.open = true;
        self.width = width;
        self.height = height;
        self.columns = columns;
        self.rows = rows;
        // Bounded by MAX_SURFACE_PIXELS above, so the product fits a usize.
        self.pixels.clear();
        self.pixels.resize(pixels as usize, 0);
        self.colors.clear();
        self.colors
            .resize(columns as usize * rows as usize, 0x00ff_ffff);
        self.colors_written = false;
        self.presented = None;
        Some(self.handle)
    }

    /// Closes the open frame: the presented frame, or `None` (counted as dropped) when the
    /// scene returned without `present`.
    pub fn finish(&mut self) -> Option<PresentedFrame> {
        self.open = false;
        let presented = self.presented.take();
        if presented.is_none() {
            self.dropped = self.dropped.saturating_add(1);
        }
        presented
    }

    /// The pointer the next frames see, in cells and sub-cell pixels; `None` when it is away.
    pub fn set_pointer(&mut self, pointer: Option<ScenePointer>) {
        self.pointer = pointer;
    }

    /// The screen occupancy the next frames see.
    pub fn set_occupancy(&mut self, occupancy: Option<OccupancyGrid>) {
        self.occupancy = occupancy;
    }

    /// Frames number of the open or last frame.
    pub fn frame(&self) -> u64 {
        self.frame
    }

    fn lent(&self, handle: FrameSurfaceHandle) -> bool {
        self.open && handle.0 == self.handle
    }

    fn stale(&self, handle: FrameSurfaceHandle) -> Result<(), PluginError> {
        if self.lent(handle) {
            Ok(())
        } else {
            Err(error(
                ErrorCode::InvalidArgument,
                "the frame surface is only valid during its render call",
            ))
        }
    }
}

fn error(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

/// Bytes per row of a pixel format at a width.
fn row_bytes(format: PixelFormat, width: u32) -> usize {
    let width = width as usize;
    match format {
        PixelFormat::Mask8 | PixelFormat::Gray8 => width,
        PixelFormat::Mono1 => width.div_ceil(8),
        PixelFormat::Rgb8 => width * 3,
        PixelFormat::Rgba8 => width * 4,
    }
}

/// Converts one row of a pixel format to grey values. `mono1` is most significant bit first;
/// colours use the Rec. 601 luma weights and `rgba8` is premultiplied by its alpha.
fn convert_row(format: PixelFormat, row: &[u8], out: &mut [u8]) {
    match format {
        PixelFormat::Mask8 | PixelFormat::Gray8 => out.copy_from_slice(&row[..out.len()]),
        PixelFormat::Mono1 => {
            for (index, pixel) in out.iter_mut().enumerate() {
                let bit = (row[index / 8] >> (7 - index % 8)) & 1;
                *pixel = if bit == 1 { 255 } else { 0 };
            }
        }
        PixelFormat::Rgb8 => {
            for (pixel, rgb) in out.iter_mut().zip(row.chunks_exact(3)) {
                *pixel = luma(rgb[0], rgb[1], rgb[2]);
            }
        }
        PixelFormat::Rgba8 => {
            for (pixel, rgba) in out.iter_mut().zip(row.chunks_exact(4)) {
                let grey = u32::from(luma(rgba[0], rgba[1], rgba[2]));
                // grey and alpha are at most 255, so the quotient is at most 255.
                *pixel = (grey * u32::from(rgba[3]) / 255) as u8;
            }
        }
    }
}

fn luma(red: u8, green: u8, blue: u8) -> u8 {
    let weighted = 299 * u32::from(red) + 587 * u32::from(green) + 114 * u32::from(blue);
    // The weights sum to 1000, so the quotient is at most 255.
    (weighted / 1000) as u8
}

fn now_unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

impl HostSurface for SceneSurface {
    fn frame_surface_size(&mut self, handle: FrameSurfaceHandle) -> (u32, u32) {
        if self.lent(handle) {
            (self.width, self.height)
        } else {
            (0, 0)
        }
    }

    fn frame_surface_write_rows(
        &mut self,
        handle: FrameSurfaceHandle,
        first_row: u32,
        format: PixelFormat,
        data: Vec<u8>,
    ) -> Result<(), PluginError> {
        self.stale(handle)?;
        if data.is_empty() {
            return Err(error(ErrorCode::InvalidArgument, "write-rows needs data"));
        }
        if first_row >= self.height {
            return Err(error(
                ErrorCode::InvalidArgument,
                "write-rows starts at or past the height",
            ));
        }
        let stride = row_bytes(format, self.width);
        let width = self.width as usize;
        // A partial trailing row is dropped; rows past the height are ignored.
        for (offset, row) in data.chunks_exact(stride).enumerate() {
            let target = first_row as usize + offset;
            if target >= self.height as usize {
                break;
            }
            let start = target * width;
            convert_row(format, row, &mut self.pixels[start..start + width]);
        }
        Ok(())
    }

    fn frame_surface_write_cell_colors(
        &mut self,
        handle: FrameSurfaceHandle,
        first_row: u32,
        colors: Vec<u32>,
    ) -> Result<(), PluginError> {
        self.stale(handle)?;
        if colors.is_empty() {
            return Err(error(
                ErrorCode::InvalidArgument,
                "write-cell-colors needs colours",
            ));
        }
        if first_row >= self.rows {
            return Err(error(
                ErrorCode::InvalidArgument,
                "write-cell-colors starts at or past the last row",
            ));
        }
        let start = first_row as usize * self.columns as usize;
        let room = self.colors.len() - start;
        for (slot, color) in self.colors[start..]
            .iter_mut()
            .zip(colors.iter().take(room))
        {
            *slot = color & 0x00ff_ffff;
        }
        self.colors_written = true;
        Ok(())
    }

    fn frame_surface_occupancy(&mut self, handle: FrameSurfaceHandle) -> Option<OccupancyGrid> {
        self.lent(handle).then(|| self.occupancy.clone()).flatten()
    }

    fn frame_surface_pointer(&mut self, handle: FrameSurfaceHandle) -> Option<ScenePointer> {
        self.lent(handle).then(|| self.pointer.clone()).flatten()
    }

    fn frame_surface_present(&mut self, handle: FrameSurfaceHandle) {
        if !self.lent(handle) {
            return;
        }
        self.presented = Some(PresentedFrame {
            width: self.width,
            height: self.height,
            columns: self.columns,
            rows: self.rows,
            pixels: self.pixels.clone(),
            cell_colors: self.colors_written.then(|| self.colors.clone()),
        });
        self.receipt = Some(FrameReceipt {
            frame: self.frame,
            presented_at_millis: now_unix_millis(),
            dropped_before: self.dropped,
            pointer_owner: None,
        });
        self.dropped = 0;
    }

    fn frame_surface_write_glyphs(
        &mut self,
        handle: FrameSurfaceHandle,
        cells: Vec<GlyphCell>,
    ) -> Result<(), PluginError> {
        self.stale(handle)?;
        if cells.len() > MAX_GLYPH_CELLS {
            return Err(error(
                ErrorCode::InvalidArgument,
                "more than 131,072 glyph cells in one frame",
            ));
        }
        Err(error(
            ErrorCode::Unsupported,
            "this host draws no glyph plane over scenes",
        ))
    }

    fn frame_surface_clear_glyphs(&mut self, _handle: FrameSurfaceHandle) {
        // No glyph plane is ever drawn (write-glyphs is unsupported), so there is nothing
        // to clear.
    }

    fn frame_surface_set_owner_ids(
        &mut self,
        handle: FrameSurfaceHandle,
        _first_row: u32,
        _ids: Vec<u16>,
    ) -> Result<(), PluginError> {
        self.stale(handle)?;
        Err(error(
            ErrorCode::Unsupported,
            "owner ids are not tracked for plugin scenes",
        ))
    }

    fn frame_surface_last_receipt(&mut self, handle: FrameSurfaceHandle) -> Option<FrameReceipt> {
        self.lent(handle).then(|| self.receipt.clone()).flatten()
    }
}

/// Calls one surface method with component values. `Err` is an ABI mismatch, which traps.
fn dispatch(
    surface: &mut SceneSurface,
    handle: FrameSurfaceHandle,
    function: &str,
    types: &[Type],
    arguments: &[Val],
) -> Result<Option<Val>, ()> {
    let expect = |count: usize| {
        if arguments.len() == count {
            Ok(())
        } else {
            Err(())
        }
    };
    Ok(match function {
        "[method]frame-surface.size" => {
            expect(0)?;
            let (width, height) = surface.frame_surface_size(handle);
            Some(Val::Tuple(vec![Val::U32(width), Val::U32(height)]))
        }
        "[method]frame-surface.write-rows" => {
            expect(3)?;
            let reply = surface.frame_surface_write_rows(
                handle,
                u32_value(&arguments[0])?,
                pixel_format(&arguments[1])?,
                list_of(&arguments[2], |value| match value {
                    Val::U8(byte) => Ok(*byte),
                    _ => Err(()),
                })?,
            );
            Some(unit_result(reply))
        }
        "[method]frame-surface.write-cell-colors" => {
            expect(2)?;
            let reply = surface.frame_surface_write_cell_colors(
                handle,
                u32_value(&arguments[0])?,
                list_of(&arguments[1], u32_value)?,
            );
            Some(unit_result(reply))
        }
        "[method]frame-surface.occupancy" => {
            expect(0)?;
            Some(Val::Option(surface.frame_surface_occupancy(handle).map(
                |grid| {
                    Box::new(Val::Record(vec![
                        ("columns".into(), Val::U16(grid.columns)),
                        ("rows".into(), Val::U16(grid.rows)),
                        (
                            "cells".into(),
                            Val::List(grid.cells.into_iter().map(Val::Bool).collect()),
                        ),
                    ]))
                },
            )))
        }
        "[method]frame-surface.pointer" => {
            expect(0)?;
            Some(Val::Option(surface.frame_surface_pointer(handle).map(
                |pointer| {
                    Box::new(Val::Record(vec![
                        ("column".into(), Val::U16(pointer.column)),
                        ("row".into(), Val::U16(pointer.row)),
                        ("sub-x".into(), Val::U8(pointer.sub_x)),
                        ("sub-y".into(), Val::U8(pointer.sub_y)),
                        ("pressed".into(), Val::Bool(pointer.pressed)),
                    ]))
                },
            )))
        }
        "[method]frame-surface.present" => {
            expect(0)?;
            surface.frame_surface_present(handle);
            None
        }
        "[method]frame-surface.write-glyphs" => {
            expect(1)?;
            let ty = types.first().ok_or(())?;
            let cells: Vec<GlyphCell> =
                serde_json::from_value(lift(&arguments[0], ty)?).map_err(|_| ())?;
            Some(unit_result(
                surface.frame_surface_write_glyphs(handle, cells),
            ))
        }
        "[method]frame-surface.clear-glyphs" => {
            expect(0)?;
            surface.frame_surface_clear_glyphs(handle);
            None
        }
        "[method]frame-surface.set-owner-ids" => {
            expect(2)?;
            let reply = surface.frame_surface_set_owner_ids(
                handle,
                u32_value(&arguments[0])?,
                list_of(&arguments[1], |value| match value {
                    Val::U16(id) => Ok(*id),
                    _ => Err(()),
                })?,
            );
            Some(unit_result(reply))
        }
        "[method]frame-surface.last-receipt" => {
            expect(0)?;
            Some(Val::Option(surface.frame_surface_last_receipt(handle).map(
                |receipt| {
                    Box::new(Val::Record(vec![
                        ("frame".into(), Val::U64(receipt.frame)),
                        (
                            "presented-at-millis".into(),
                            Val::U64(receipt.presented_at_millis),
                        ),
                        ("dropped-before".into(), Val::U32(receipt.dropped_before)),
                        (
                            "pointer-owner".into(),
                            Val::Option(receipt.pointer_owner.map(|id| Box::new(Val::U16(id)))),
                        ),
                    ]))
                },
            )))
        }
        _ => return Err(()),
    })
}

fn u32_value(value: &Val) -> Result<u32, ()> {
    match value {
        Val::U32(number) => Ok(*number),
        _ => Err(()),
    }
}

fn list_of<T>(value: &Val, item: impl Fn(&Val) -> Result<T, ()>) -> Result<Vec<T>, ()> {
    match value {
        Val::List(items) => items.iter().map(item).collect(),
        _ => Err(()),
    }
}

fn pixel_format(value: &Val) -> Result<PixelFormat, ()> {
    let Val::Enum(name) = value else {
        return Err(());
    };
    Ok(match name.as_str() {
        "mask8" => PixelFormat::Mask8,
        "mono1" => PixelFormat::Mono1,
        "gray8" => PixelFormat::Gray8,
        "rgb8" => PixelFormat::Rgb8,
        "rgba8" => PixelFormat::Rgba8,
        _ => return Err(()),
    })
}

fn unit_result(reply: Result<(), PluginError>) -> Val {
    Val::Result(match reply {
        Ok(()) => Ok(None),
        Err(failure) => Err(Some(Box::new(plugin_error_value(&failure)))),
    })
}

fn plugin_error_value(failure: &PluginError) -> Val {
    let code = serde_json::to_value(failure.code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "internal".to_owned());
    Val::Record(vec![
        ("code".into(), Val::Enum(code)),
        ("message".into(), Val::String(failure.message.clone())),
        (
            "retry-after-ms".into(),
            Val::Option(failure.retry_after_ms.map(|ms| Box::new(Val::U32(ms)))),
        ),
    ])
}

fn lock(surface: &Mutex<SceneSurface>) -> MutexGuard<'_, SceneSurface> {
    // The buffer holds no invariant a panicking writer could break halfway.
    surface.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Defines the `frame-surface` resource and its methods over `surface` in `linker`.
pub fn define_frame_surface(
    linker: &mut Linker<StoreData>,
    surface: &Arc<Mutex<SceneSurface>>,
) -> Result<(), HostError> {
    let failed = |error: wasmtime::Error| {
        HostError::EngineError(format!("linking {SURFACE} failed: {error}"))
    };
    let mut instance = linker.instance(SURFACE).map_err(failed)?;
    // The host owns every surface; a guest never receives an owned handle to drop.
    instance
        .resource(
            "frame-surface",
            ResourceType::host::<FrameSurfaceResource>(),
            |_, _| Ok(()),
        )
        .map_err(failed)?;
    for function in SURFACE_METHODS {
        let surface = Arc::clone(surface);
        instance
            .func_new(
                function,
                move |mut store: StoreContextMut<'_, StoreData>, signature, parameters, results| {
                    let abi = || {
                        wasmtime::format_err!(
                            "trap.abi: import {SURFACE}.{function} does not match its WIT signature"
                        )
                    };
                    let Some((Val::Resource(handle), arguments)) = parameters.split_first() else {
                        return Err(abi());
                    };
                    let rep = Resource::<FrameSurfaceResource>::try_from_resource_any(
                        *handle, &mut store,
                    )?
                    .rep();
                    let types: Vec<Type> = signature.params().skip(1).map(|(_, ty)| ty).collect();
                    let reply = dispatch(
                        &mut lock(&surface),
                        FrameSurfaceHandle(rep),
                        function,
                        &types,
                        arguments,
                    )
                    .map_err(|()| abi())?;
                    match (reply, results.first_mut()) {
                        (Some(value), Some(slot)) => *slot = value,
                        (None, None) => {}
                        _ => return Err(abi()),
                    }
                    Ok(())
                },
            )
            .map_err(failed)?;
    }
    Ok(())
}

/// What a scene says about itself, asked at construction and after each reconfigure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneTraits {
    /// Requested redraw cadence.
    pub frames_per_second: u32,
    /// Whether the pointer changes what is drawn.
    pub wants_pointer: bool,
    /// Whether the screen occupancy changes what is drawn.
    pub wants_occupancy: bool,
    /// Whether the scene writes cell colours.
    pub uses_cell_colors: bool,
}

/// What one frame is drawn from (reference-wit.md `frame-input`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SceneFrameInput {
    /// The animation clock, scaled by the user's speed.
    pub time_millis: u64,
    /// Unscaled time since the scene started.
    pub wall_millis: u64,
    /// The civil time.
    pub now_unix_millis: u64,
    /// The field in terminal cells.
    pub columns: u32,
    /// See `columns`.
    pub rows: u32,
    /// The shared look's palette, `0xRRGGBB` per stop; empty when none applies.
    pub palette_rgb: Vec<u32>,
}

/// A scene component instance with its renderer and frame buffer.
pub struct SceneGuest {
    engine: Arc<PluginEngine>,
    store: Store<StoreData>,
    instance: Instance,
    limits: ResolvedLimits,
    surface: Arc<Mutex<SceneSurface>>,
    renderer: Option<ResourceAny>,
    traits: SceneTraits,
}

impl std::fmt::Debug for SceneGuest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SceneGuest")
            .field("traits", &self.traits)
            .finish_non_exhaustive()
    }
}

impl SceneGuest {
    /// Instantiates a scene component and constructs its renderer from `spec_json` (the
    /// scene id, its settings, the palette and the mode). The component must export
    /// [`RENDERER`]. Imports follow the same grant checks as every other guest; imports this
    /// process does not implement (fonts) instantiate and trap when called.
    pub fn instantiate(
        engine: Arc<PluginEngine>,
        compiled: &CompiledComponent,
        limits: ResolvedLimits,
        imports: ImportSet,
        identity: GuestIdentity,
        spec_json: &str,
    ) -> Result<Self, HostError> {
        let surface = Arc::new(Mutex::new(SceneSurface::default()));
        let mut linker = build_runtime_linker(&engine)?;
        define_frame_surface(linker.linker_mut(), &surface)?;
        linker
            .linker_mut()
            .define_unknown_imports_as_traps(&compiled.component)
            .map_err(|error| HostError::EngineError(error.to_string()))?;
        let (engine, store, instance, limits) =
            WasmGuest::instantiate_linked(engine, compiled, limits, &linker, imports, identity)?
                .into_parts();
        let mut guest = Self {
            engine,
            store,
            instance,
            limits,
            surface,
            renderer: None,
            traits: SceneTraits {
                frames_per_second: 12,
                wants_pointer: false,
                wants_occupancy: false,
                uses_cell_colors: false,
            },
        };
        let constructed = guest
            .call(
                "[constructor]scene-renderer",
                CallClass::Activate,
                vec![Val::String(spec_json.to_owned())],
                1,
            )
            .map_err(HostError::ActivationTrap)?;
        match constructed.into_iter().next() {
            Some(Val::Resource(renderer)) => guest.renderer = Some(renderer),
            _ => {
                return Err(HostError::ActivationTrap(trap(
                    "trap.abi",
                    RENDERER,
                    "[constructor]scene-renderer",
                )))
            }
        }
        guest.refresh_traits().map_err(HostError::ActivationTrap)?;
        Ok(guest)
    }

    /// What the scene said about itself at construction or the last reconfigure.
    pub fn traits(&self) -> SceneTraits {
        self.traits
    }

    /// The frame buffer, for the pointer and occupancy the next frames see.
    pub fn surface(&self) -> &Arc<Mutex<SceneSurface>> {
        &self.surface
    }

    /// Draws one frame. `Ok(None)` when the scene returned without `present` (the caller keeps
    /// the previous frame) or the field is empty or too large.
    pub fn render(&mut self, input: &SceneFrameInput) -> Result<Option<PresentedFrame>, Trap> {
        const FUNCTION: &str = "[method]scene-renderer.render";
        let renderer = self.renderer_value(FUNCTION)?;
        let Some(rep) = lock(&self.surface).begin(input.columns, input.rows) else {
            return Ok(None);
        };
        let (width, height) = (
            input.columns * PIXELS_PER_COLUMN,
            input.rows * PIXELS_PER_ROW,
        );
        let borrowed = Resource::<FrameSurfaceResource>::new_borrow(rep)
            .try_into_resource_any(&mut self.store)
            .map_err(|_| trap("trap.abi", RENDERER, FUNCTION))?;
        let frame = Val::Record(vec![
            ("time-millis".into(), Val::U64(input.time_millis)),
            ("wall-millis".into(), Val::U64(input.wall_millis)),
            ("now-unix-millis".into(), Val::U64(input.now_unix_millis)),
            ("width".into(), Val::U32(width)),
            ("height".into(), Val::U32(height)),
            (
                "palette-rgb".into(),
                Val::List(input.palette_rgb.iter().copied().map(Val::U32).collect()),
            ),
        ]);
        let called = self.call(
            FUNCTION,
            CallClass::Frame,
            vec![renderer, frame, Val::Resource(borrowed)],
            0,
        );
        // The lent handle leaves the host table whatever the guest did with it, so the table
        // does not grow by one entry per frame.
        let _ = block_on(borrowed.resource_drop_async(&mut self.store));
        let presented = lock(&self.surface).finish();
        called.map(|_| presented)
    }

    /// The scene's readiness (`preparing` while it loads what it draws from).
    pub fn readiness(&mut self) -> Result<Readiness, Trap> {
        const FUNCTION: &str = "[method]scene-renderer.readiness";
        let renderer = self.renderer_value(FUNCTION)?;
        match self
            .call(FUNCTION, CallClass::Frame, vec![renderer], 1)?
            .first()
        {
            Some(Val::Enum(name)) => match name.as_str() {
                "ready" => Ok(Readiness::Ready),
                "preparing" => Ok(Readiness::Preparing),
                "failed" => Ok(Readiness::Failed),
                _ => Err(trap("trap.abi", RENDERER, FUNCTION)),
            },
            _ => Err(trap("trap.abi", RENDERER, FUNCTION)),
        }
    }

    /// Hands the scene the shared look's new palette.
    pub fn set_palette(&mut self, palette_rgb: &[u32]) -> Result<(), Trap> {
        const FUNCTION: &str = "[method]scene-renderer.set-palette";
        let renderer = self.renderer_value(FUNCTION)?;
        let palette = Val::List(palette_rgb.iter().copied().map(Val::U32).collect());
        self.call(FUNCTION, CallClass::Event, vec![renderer, palette], 0)
            .map(|_| ())
    }

    /// Hands the scene changed settings. `Ok(false)` asks the host to build a new scene.
    pub fn reconfigure(&mut self, settings_json: &str) -> Result<bool, Trap> {
        const FUNCTION: &str = "[method]scene-renderer.reconfigure";
        let renderer = self.renderer_value(FUNCTION)?;
        let applied = match self
            .call(
                FUNCTION,
                CallClass::Event,
                vec![renderer, Val::String(settings_json.to_owned())],
                1,
            )?
            .first()
        {
            Some(Val::Bool(applied)) => *applied,
            _ => return Err(trap("trap.abi", RENDERER, FUNCTION)),
        };
        if applied {
            self.refresh_traits()?;
        }
        Ok(applied)
    }

    /// The scene's one-line status, `None` when all is well.
    pub fn status(&mut self) -> Result<Option<String>, Trap> {
        const FUNCTION: &str = "[method]scene-renderer.status";
        let renderer = self.renderer_value(FUNCTION)?;
        match self
            .call(FUNCTION, CallClass::Frame, vec![renderer], 1)?
            .into_iter()
            .next()
        {
            Some(Val::Option(None)) => Ok(None),
            Some(Val::Option(Some(text))) => match *text {
                Val::String(text) => Ok(Some(text)),
                _ => Err(trap("trap.abi", RENDERER, FUNCTION)),
            },
            _ => Err(trap("trap.abi", RENDERER, FUNCTION)),
        }
    }

    fn refresh_traits(&mut self) -> Result<(), Trap> {
        let frames_per_second = match self.ask("[method]scene-renderer.frames-per-second")? {
            Val::U32(rate) => rate.clamp(1, 30),
            _ => {
                return Err(trap(
                    "trap.abi",
                    RENDERER,
                    "[method]scene-renderer.frames-per-second",
                ))
            }
        };
        let flag = |guest: &mut Self, function: &'static str| match guest.ask(function)? {
            Val::Bool(value) => Ok(value),
            _ => Err(trap("trap.abi", RENDERER, function)),
        };
        self.traits = SceneTraits {
            frames_per_second,
            wants_pointer: flag(self, "[method]scene-renderer.wants-pointer")?,
            wants_occupancy: flag(self, "[method]scene-renderer.wants-occupancy")?,
            uses_cell_colors: flag(self, "[method]scene-renderer.uses-cell-colors")?,
        };
        Ok(())
    }

    fn ask(&mut self, function: &str) -> Result<Val, Trap> {
        let renderer = self.renderer_value(function)?;
        self.call(function, CallClass::Frame, vec![renderer], 1)?
            .into_iter()
            .next()
            .ok_or_else(|| trap("trap.abi", RENDERER, function))
    }

    fn renderer_value(&self, function: &str) -> Result<Val, Trap> {
        self.renderer
            .map(Val::Resource)
            .ok_or_else(|| trap("trap.abi", RENDERER, function))
    }

    /// Calls one renderer function with `result_count` results, under the class deadline.
    fn call(
        &mut self,
        function: &str,
        class: CallClass,
        arguments: Vec<Val>,
        result_count: usize,
    ) -> Result<Vec<Val>, Trap> {
        let invalid = || trap("trap.abi", RENDERER, function);
        let interface = self
            .instance
            .get_export_index(&mut self.store, None, RENDERER)
            .ok_or_else(invalid)?;
        let index = self
            .instance
            .get_export_index(&mut self.store, Some(&interface), function)
            .ok_or_else(invalid)?;
        let export = self
            .instance
            .get_func(&mut self.store, index)
            .ok_or_else(invalid)?;
        let signature = export.ty(&self.store);
        if signature.params().count() != arguments.len()
            || signature.results().count() != result_count
        {
            return Err(invalid());
        }
        self.engine.arm_deadline(
            &mut self.store,
            deadline_for(class, &self.limits).as_millis() as u64,
            false,
        );
        self.store.data_mut().begin_call();
        let mut results = vec![Val::Bool(false); result_count];
        let called = block_on(export.call_async(&mut self.store, &arguments, &mut results));
        self.forward_output();
        match called {
            Ok(()) => Ok(results),
            Err(error) => {
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
                Err(trap(code, RENDERER, function))
            }
        }
    }

    /// Sends what the guest printed to its log import, as every other guest does.
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
}

impl Drop for SceneGuest {
    fn drop(&mut self) {
        // Runs the guest's destructor of the renderer; a trap there changes nothing for the
        // host, which drops the whole store next.
        if let Some(renderer) = self.renderer.take() {
            let _ = block_on(renderer.resource_drop_async(&mut self.store));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened(columns: u32, rows: u32) -> (SceneSurface, FrameSurfaceHandle) {
        let mut surface = SceneSurface::default();
        let rep = surface.begin(columns, rows).expect("frame opens");
        (surface, FrameSurfaceHandle(rep))
    }

    #[test]
    fn the_field_is_the_braille_field_of_the_terminal() {
        let (mut surface, handle) = opened(3, 2);
        assert_eq!(surface.frame_surface_size(handle), (6, 8));
        assert!(surface.begin(0, 4).is_none());
        assert!(surface.begin(u32::MAX, 2).is_none());
        assert!(surface.begin(5000, 5000).is_none());
    }

    #[test]
    fn grey_rows_land_on_their_rows_and_extra_rows_are_ignored() {
        let (mut surface, handle) = opened(1, 1);
        // Width 2, height 4: rows 2 and 3 written, a third row past the height ignored,
        // a trailing partial row dropped.
        let data = vec![10, 20, 30, 40, 50, 60, 7];
        surface
            .frame_surface_write_rows(handle, 2, PixelFormat::Gray8, data)
            .expect("rows written");
        surface.frame_surface_present(handle);
        let frame = surface.finish().expect("presented");
        assert_eq!(frame.pixels, vec![0, 0, 0, 0, 10, 20, 30, 40]);
        assert_eq!(frame.cell_colors, None);
    }

    #[test]
    fn every_pixel_format_converts_to_grey() {
        let mut out = [0u8; 2];
        convert_row(PixelFormat::Mono1, &[0b1000_0000], &mut out);
        assert_eq!(out, [255, 0]);
        convert_row(PixelFormat::Rgb8, &[255, 255, 255, 255, 0, 0], &mut out);
        assert_eq!(out, [255, 76]);
        convert_row(
            PixelFormat::Rgba8,
            &[255, 255, 255, 0, 255, 255, 255, 255],
            &mut out,
        );
        assert_eq!(out, [0, 255]);
        convert_row(PixelFormat::Mask8, &[3, 4], &mut out);
        assert_eq!(out, [3, 4]);
        assert_eq!(row_bytes(PixelFormat::Mono1, 9), 2);
        assert_eq!(row_bytes(PixelFormat::Rgba8, 3), 12);
    }

    #[test]
    fn bad_writes_are_invalid_arguments_not_traps() {
        let (mut surface, handle) = opened(1, 1);
        let empty = surface.frame_surface_write_rows(handle, 0, PixelFormat::Gray8, Vec::new());
        assert_eq!(empty.unwrap_err().code, ErrorCode::InvalidArgument);
        let past = surface.frame_surface_write_rows(handle, 4, PixelFormat::Gray8, vec![1, 2]);
        assert_eq!(past.unwrap_err().code, ErrorCode::InvalidArgument);
        let colors = surface.frame_surface_write_cell_colors(handle, 1, vec![1]);
        assert_eq!(colors.unwrap_err().code, ErrorCode::InvalidArgument);
        let glyphs = surface.frame_surface_write_glyphs(handle, Vec::new());
        assert_eq!(glyphs.unwrap_err().code, ErrorCode::Unsupported);
    }

    #[test]
    fn a_stale_handle_cannot_draw_into_a_later_frame() {
        let (mut surface, first) = opened(1, 1);
        assert!(surface.finish().is_none());
        let second = FrameSurfaceHandle(surface.begin(1, 1).expect("second frame"));
        assert_ne!(first, second);
        let stale = surface.frame_surface_write_rows(first, 0, PixelFormat::Gray8, vec![1, 1]);
        assert_eq!(stale.unwrap_err().code, ErrorCode::InvalidArgument);
        assert_eq!(surface.frame_surface_size(first), (0, 0));
        surface.frame_surface_present(first);
        assert!(
            surface.finish().is_none(),
            "a stale present presents nothing"
        );
    }

    #[test]
    fn cell_colours_and_receipts_follow_presented_frames() {
        let (mut surface, handle) = opened(2, 1);
        surface
            .frame_surface_write_cell_colors(handle, 0, vec![0xff12_3456, 0x00ab_cdef, 9])
            .expect("colours written");
        surface.frame_surface_present(handle);
        let frame = surface.finish().expect("presented");
        assert_eq!(frame.cell_colors, Some(vec![0x0012_3456, 0x00ab_cdef]));
        // One dropped frame, then the next receipt counts it.
        let dropped = FrameSurfaceHandle(surface.begin(2, 1).expect("frame"));
        assert!(surface.frame_surface_last_receipt(dropped).is_some());
        assert!(surface.finish().is_none());
        let next = FrameSurfaceHandle(surface.begin(2, 1).expect("frame"));
        surface.frame_surface_present(next);
        let receipt = surface.frame_surface_last_receipt(next).expect("receipt");
        assert_eq!(receipt.frame, 3);
        assert_eq!(receipt.dropped_before, 1);
    }

    #[test]
    fn pointer_and_occupancy_are_seen_only_through_the_open_frame() {
        let (mut surface, handle) = opened(1, 1);
        let pointer = ScenePointer {
            column: 0,
            row: 0,
            sub_x: 1,
            sub_y: 3,
            pressed: false,
        };
        surface.set_pointer(Some(pointer.clone()));
        assert_eq!(surface.frame_surface_pointer(handle), Some(pointer));
        surface.finish();
        assert_eq!(surface.frame_surface_pointer(handle), None);
    }

    #[test]
    fn dispatch_checks_the_component_values_it_is_given() {
        let (mut surface, handle) = opened(1, 1);
        let written = dispatch(
            &mut surface,
            handle,
            "[method]frame-surface.write-rows",
            &[],
            &[
                Val::U32(0),
                Val::Enum("gray8".into()),
                Val::List(vec![Val::U8(9), Val::U8(8)]),
            ],
        )
        .expect("well typed");
        assert_eq!(written, Some(Val::Result(Ok(None))));
        let refused = dispatch(
            &mut surface,
            handle,
            "[method]frame-surface.write-rows",
            &[],
            &[
                Val::U32(0),
                Val::Enum("gray8".into()),
                Val::List(Vec::new()),
            ],
        )
        .expect("well typed");
        let Some(Val::Result(Err(Some(record)))) = refused else {
            panic!("an error result: {refused:?}");
        };
        let Val::Record(fields) = *record else {
            panic!("a plugin-error record");
        };
        assert_eq!(
            fields[0],
            ("code".to_owned(), Val::Enum("invalid-argument".to_owned()))
        );
        assert!(dispatch(
            &mut surface,
            handle,
            "[method]frame-surface.write-rows",
            &[],
            &[Val::U32(0), Val::Enum("cmyk".into()), Val::List(Vec::new())],
        )
        .is_err());
        assert!(dispatch(&mut surface, handle, "[method]frame-surface.nope", &[], &[]).is_err());
        assert_eq!(
            dispatch(&mut surface, handle, "[method]frame-surface.size", &[], &[]),
            Ok(Some(Val::Tuple(vec![Val::U32(2), Val::U32(4)])))
        );
    }

    #[test]
    fn scene_guests_can_move_to_the_ambient_worker() {
        fn send<T: Send>() {}
        send::<SceneGuest>();
        send::<SceneSurface>();
    }
}
