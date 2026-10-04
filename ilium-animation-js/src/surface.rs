//! Pure retained drawing transactions. Native presentation/permissions remain separate.
use serde::{Deserialize, Serialize}; // Metadata is bounded JSON; sample planes are binary.
use std::collections::{BTreeMap, BTreeSet}; // Bound retained text and unique native evidence identities.
use std::io::{self, Write}; // Count metadata bytes without allocating an oversized JSON copy.
use std::num::NonZeroU64; // Option<SourceToken> remains one machine-sized provenance entry.
pub const MAX_CELLS: usize = 131_072; // Match the supplied worker's absolute cell ceiling.
pub const MAX_META_BYTES: usize = 65_536; // Bound command and diagnostic metadata before parsing.
pub const MAX_COMMANDS: usize = 256; // Native rendering is one bounded ordered batch.
pub const MAX_EDITS: u32 = 4_194_304; // Includes scalar edits, rectangles, and native commands.
pub const MAX_FRAGMENTS: usize = 128; // More fragmented damage becomes full-viewport damage.
pub const MAX_TEXT_BYTES: usize = 16_384; // Bound retained glyph strings, not only new text commands.
pub const MAX_HANDOFF_BYTES: usize = 67_108_864; // Includes working and sealed planes plus injected inputs.
const BRAILLE: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]]; // Unicode's 2x4 dot mapping.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)] // No error publishes a partial surface.
pub enum SurfaceError {
    // Keep pure validation independent of V8 and application APIs.
    #[error("invalid frame: {0}")] // Static diagnostics cannot retain unbounded script text.
    Invalid(&'static str), // Malformed shapes, samples, commands, and bounds.
    #[error("stale or foreign frame base")]
    // A new time request does not change this base identity.
    Stale, // Instance/revision/base/sequence must match the host-issued lease.
    #[error("surface budget exhausted")] // Backpressure preserves the accepted state.
    Capacity, // Includes dimensions, metadata, text, and native sample work.
    #[error("a frame is already open")] // One synchronous render transaction per surface.
    Busy, // The host must finish or abort before issuing another lease.
    #[error("frame callback failed")] // A caught facade error still poisons its transaction.
    Callback, // Native exceptions and timeout paths must reject too.
    #[error("native drawing handler unavailable")] // No silent drop of unsupported commands.
    Unsupported, // Primary-owned raster/text/prepared-blit adapters supply the handler.
} // Native emission acknowledgement is intentionally not represented here.
type Result<T> = std::result::Result<T, SurfaceError>; // One explicit error contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] // Seven formats including direct cells.
#[serde(rename_all = "snake_case")] // Match the external SDK's format names.
pub enum Format {
    Mask8,
    Mono1,
    Mono8,
    Gray8,
    Gray32,
    Rgb8,
    Rgba8,
} // No implicit reinterpretation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] // One authoritative representation.
#[serde(rename_all = "snake_case")] // Stable JSON mode names.
pub enum Mode {
    Cells,
    Pixels,
} // Switching mode requires a new Surface/revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] // Replacement and retained updates differ.
#[serde(rename_all = "snake_case")] // Stable JSON update names.
pub enum Update {
    Replace,
    Retain,
} // An empty replace is an explicit erasure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] // RGB values have declared colour space.
#[serde(rename_all = "snake_case")] // sRGB is spelled srgb in the wire schema.
pub enum ColourSpace {
    Srgb,
    Linear,
} // Alpha is always linear coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] // Accepted immutable output shape.
#[serde(deny_unknown_fields)] // Scripts cannot extend the allocation schema.
pub struct Shape {
    pub cell_width: u32,
    pub cell_height: u32,
    pub mode: Mode,
    pub format: Format,
    pub update: Update,
    pub cell_rgb: bool,
    pub colour_space: ColourSpace,
} // Full manifest projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)] // The host derives ArraySpec from these checked counts.
pub struct Layout {
    pub cells: usize,
    pub dots: usize,
    pub width: usize,
    pub height: usize,
    pub samples: usize,
    pub elements: usize,
    pub row_elements: usize,
    pub data_bytes: usize,
    pub canonical_bytes: usize,
    pub handoff_bytes: usize,
} // No allocation during calculation.
impl Shape {
    // All arithmetic is checked before allocating planes.
    pub fn layout(self) -> Result<Layout> {
        // Fixed caps may be narrowed by parent admission.
        let w = self.cell_width as usize;
        let h = self.cell_height as usize; // Dimensions are unsigned wire integers.
        if w == 0 || h == 0 || w > 4096 || h > 4096 {
            return Err(SurfaceError::Invalid("dimensions"));
        } // Hidden/zero viewports do not render.
        let cells = w.checked_mul(h).ok_or(SurfaceError::Capacity)?; // Reject overflow before multiplication reuse.
        if cells > MAX_CELLS || (self.mode == Mode::Cells) != (self.format == Format::Mask8) {
            return Err(SurfaceError::Invalid("mode or cell count"));
        } // Cells require mask8 exclusively.
        if self.cell_rgb && matches!(self.format, Format::Rgb8 | Format::Rgba8) {
            return Err(SurfaceError::Invalid("redundant cell RGB"));
        } // Colour sources already carry per-dot RGB.
        let dots = cells * 8;
        let width = if self.mode == Mode::Cells { w } else { w * 2 };
        let height = if self.mode == Mode::Cells { h } else { h * 4 }; // Caps make subsequent products safe.
        let samples = width * height;
        let channels = self.format.channels(); // mono1 is packed separately.
        let row_elements = if self.format == Format::Mono1 {
            width.div_ceil(8)
        } else {
            width * channels
        }; // Each mono1 row has independent padding.
        let elements = row_elements * height;
        let data_bytes = elements * if self.format == Format::Gray32 { 4 } else { 1 }; // F32 elements are not bytes.
        let colour_bytes = if self.cell_rgb { cells * 8 } else { 0 }; // RGB + colour-touch U8 + colour-order U32.
        let handoff_bytes = 2 * (data_bytes + samples * 5 + colour_bytes); // Working and sealed data/touch/order planes.
        let canonical_bytes = data_bytes
            + samples
            + dots * 8
            + if self.cell_rgb { cells * 4 } else { 0 }
            + MAX_TEXT_BYTES
            + cells.min(MAX_TEXT_BYTES) * 128; // Include bounded text/map overhead allowance.
        if handoff_bytes > MAX_HANDOFF_BYTES || canonical_bytes > 25_165_824 {
            return Err(SurfaceError::Capacity);
        } // Preserve the supplied 24 MiB canonical envelope.
        Ok(Layout {
            cells,
            dots,
            width,
            height,
            samples,
            elements,
            row_elements,
            data_bytes,
            canonical_bytes,
            handoff_bytes,
        }) // Parent must also admit simultaneous copies and frame leases.
    } // These are logical envelopes, not a claim about allocator RSS.
} // End accepted-shape arithmetic.
impl Format {
    // Shared sample layout and validation helpers.
    fn channels(self) -> usize {
        match self {
            Self::Rgb8 => 3,
            Self::Rgba8 => 4,
            _ => 1,
        }
    } // Packed mono1 uses one logical component.
    fn value_ok(self, value: [f32; 4]) -> bool {
        // Native values are checked like JavaScript data.
        let count = self.channels(); // Ignore unused components only after callers initialize them.
        let maximum = if matches!(self, Self::Mono1 | Self::Mono8 | Self::Gray32) {
            1.0
        } else {
            255.0
        }; // Wire units match the SDK.
        value[..count].iter().all(|v| {
            v.is_finite()
                && *v >= 0.0
                && *v <= maximum
                && (self == Self::Gray32 || v.fract() == 0.0)
        }) // No lossy casts of malformed samples.
    } // RGBA alpha has byte units in the backing plane.
} // No unknown pixel format can reach allocation.
#[derive(Clone, Debug, PartialEq)] // Rust owns copies after native handoff.
pub enum Data {
    U8(Vec<u8>),
    F32(Vec<f32>),
} // No JSON array conversion is required.
impl Data {
    // Keep storage type checks explicit.
    pub fn zero(shape: Shape) -> Result<Self> {
        // Parent reserves allocation before calling this API.
        let layout = shape.layout()?;
        Ok(if shape.format == Format::Gray32 {
            Self::F32(vec![0.0; layout.elements])
        } else {
            Self::U8(vec![0; layout.elements])
        }) // Allocate only the selected representation.
    } // No hidden RGBA companion is allocated for greyscale output.
    fn validate(&self, format: Format, width: usize, height: usize) -> Result<()> {
        // Validate complete tight planes before indexing.
        let row = if format == Format::Mono1 {
            width.div_ceil(8)
        } else {
            width * format.channels()
        };
        let count = row.checked_mul(height).ok_or(SurfaceError::Capacity)?; // Native patches use their own dimensions.
        match (format, self) {
            // Reject mismatched typed-array kinds.
            (Format::Gray32, Self::F32(values))
                if values.len() == count
                    && values
                        .iter()
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v)) =>
            {
                Ok(())
            } // Finite normalized floats.
            (Format::Gray32, _) | (_, Self::F32(_)) => {
                Err(SurfaceError::Invalid("data kind or float values"))
            } // No byte reinterpretation.
            (_, Self::U8(values)) if values.len() == count => {
                // Validate compact format-specific encodings.
                if format == Format::Mono8 && values.iter().any(|v| *v > 1) {
                    return Err(SurfaceError::Invalid("mono8 value"));
                } // Binary bytes are 0 or 1.
                if format == Format::Mono1
                    && !width.is_multiple_of(8)
                    && (0..height)
                        .any(|y| values[y * row + row - 1] & ((1_u8 << (8 - width % 8)) - 1) != 0)
                {
                    return Err(SurfaceError::Invalid("mono1 padding"));
                } // Unused trailing bits are zero.
                Ok(()) // Ordinary U8 formats already have bounded component values.
            } // The exact length includes every row's padding.
            _ => Err(SurfaceError::Invalid("data length")), // Extra capacity cannot masquerade as a correctly sized view.
        } // Engine separately validates ArrayBuffer offset and total backing length.
    } // Admission bounds the scan before this method runs.
    fn read(&self, format: Format, width: usize, index: usize) -> [f32; 4] {
        // Called only after layout validation.
        let mut value = [0.0; 4]; // All unused channels are deterministic.
        match self {
            // A shape never switches storage kind in place.
            Self::F32(values) => value[0] = values[index], // One float per dot.
            Self::U8(values) if format == Format::Mono1 => {
                value[0] = f32::from(
                    (values[(index / width) * width.div_ceil(8) + (index % width) / 8]
                        >> (7 - index % width % 8))
                        & 1,
                )
            } // MSB-first pixel bits.
            Self::U8(values) => {
                for channel in 0..format.channels() {
                    value[channel] = f32::from(values[index * format.channels() + channel]);
                }
            } // Interleaved colour components.
        } // Mask bytes remain Braille masks, not packed pixel rows.
        value // Return copied scalar components only.
    } // Native adapters never retain a mutable borrowed sample.
    fn write(&mut self, format: Format, width: usize, index: usize, value: [f32; 4]) {
        // Callers already validated sample units.
        match self {
            // Preserve tight row layout.
            Self::F32(values) => values[index] = value[0], // No byte quantization for gray32.
            Self::U8(values) if format == Format::Mono1 => {
                let offset = (index / width) * width.div_ceil(8) + (index % width) / 8;
                let bit = 1 << (7 - index % width % 8);
                values[offset] = (values[offset] & !bit) | if value[0] != 0.0 { bit } else { 0 };
            } // Preserve neighbouring packed bits.
            Self::U8(values) => {
                for channel in 0..format.channels() {
                    values[index * format.channels() + channel] = value[channel] as u8;
                }
            } // Conversion follows prior integer/range checks.
        } // This method cannot touch another row's padding.
    } // End sample write.
} // End binary data implementation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] // Decimal strings preserve all u64 identity bits in JS.
#[serde(deny_unknown_fields)] // No script-created publisher or source authority fields.
pub struct FrameKey {
    pub instance_id: String,
    pub revision: String,
    pub base_version: String,
    pub sequence: String,
} // Exact host-issued identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] // Rectangles are strict bounds in sample coordinates unless documented as cells.
#[serde(deny_unknown_fields)] // No hidden clipping or stride policy.
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
} // Zero-area rectangles are legal no-ops.
impl Rect {
    // Use checked unsigned arithmetic rather than wrapping coordinates.
    fn check(self, width: usize, height: usize) -> Result<()> {
        // Negative JSON coordinates fail deserialization.
        if self.x as usize > width
            || self.y as usize > height
            || self.width as usize > width - self.x as usize
            || self.height as usize > height - self.y as usize
        {
            return Err(SurfaceError::Invalid("rectangle bounds"));
        } // No partial clipping of scalar/row data.
        Ok(()) // Vector rasterizers separately clip bounded finite geometry.
    } // A malformed rectangle poisons the whole transaction.
} // End rectangle validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] // Explicit native blend semantics.
#[serde(rename_all = "snake_case")] // Match local frame commands.
pub enum Blend {
    Overwrite,
    Max,
    Alpha,
} // Alpha is supported only on rgba8 surfaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] // Vector geometry remains a native raster boundary.
#[serde(rename_all = "snake_case")] // Packed command operation names.
pub enum VectorOp {
    Line,
    Path,
    Triangle,
    Ellipse,
} // No arbitrary native entry-point name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] // Bounded styled text attributes.
#[serde(deny_unknown_fields)] // Escape sequences are never a styling API.
pub struct TextStyle {
    pub rgb: Option<[u8; 3]>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
} // Width comes from the native text adapter.
#[derive(Clone, Debug, Serialize, Deserialize)] // Script commands contain prepared handles, never owner IDs.
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)] // Closed command schema.
pub enum Command {
    // Orders interleave commands with the last ordinary overwrite of each sample.
    Vector {
        order: u32,
        op: VectorOp,
        points: Vec<[f32; 2]>,
        width: f32,
        fill: bool,
        closed: bool,
        value: Vec<f32>,
        blend: Blend,
    }, // Native bounded rasterization.
    Text {
        order: u32,
        x: u32,
        y: u32,
        text: String,
        style: TextStyle,
    }, // Native Unicode/font layout.
    Blit {
        order: u32,
        handle: String,
        source: Rect,
        target: Rect,
        blend: Blend,
    }, // Native validates the prepared handle and source identity.
} // No RPC or resource acquisition occurs per sample.
impl Command {
    // Validate metadata before invoking any native adapter.
    pub fn order(&self) -> u32 {
        match self {
            Self::Vector { order, .. } | Self::Text { order, .. } | Self::Blit { order, .. } => {
                *order
            }
        }
    } // One total transaction order.
    fn blend(&self) -> Blend {
        match self {
            Self::Vector { blend, .. } | Self::Blit { blend, .. } => *blend,
            Self::Text { .. } => Blend::Overwrite,
        }
    } // Text uses native overlay replacement.
    fn validate(&self, shape: Shape) -> Result<()> {
        // Adapter errors remain rollback-safe.
        if self.order() == 0
            || self.order() > MAX_EDITS
            || (self.blend() == Blend::Alpha && shape.format != Format::Rgba8)
        {
            return Err(SurfaceError::Invalid("command order or blend"));
        } // Never silently substitute blending rules.
        match self {
            // Keep all untrusted command graphs bounded.
            Self::Vector {
                op,
                points,
                width,
                value,
                ..
            } => {
                // Geometry is in dot coordinates; the adapter clips before raster loops.
                let count_ok = match op {
                    VectorOp::Line | VectorOp::Ellipse => points.len() == 2,
                    VectorOp::Triangle => points.len() == 3,
                    VectorOp::Path => (2..=1024).contains(&points.len()),
                }; // Exact primitive shapes.
                if !count_ok
                    || !width.is_finite()
                    || *width < 0.0
                    || *width > 1024.0
                    || points
                        .iter()
                        .flatten()
                        .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
                    || value.len() != shape.format.channels()
                {
                    return Err(SurfaceError::Invalid("vector geometry"));
                } // Bound pathological geometry.
                if *op == VectorOp::Ellipse && (points[1][0] < 0.0 || points[1][1] < 0.0) {
                    return Err(SurfaceError::Invalid("ellipse radii"));
                } // Negative radii are malformed.
                let mut sample = [0.0; 4];
                sample[..value.len()].copy_from_slice(value);
                if !shape.format.value_ok(sample) {
                    return Err(SurfaceError::Invalid("vector sample"));
                } // Match accepted component units.
            } // Native vector code may not invent source provenance.
            Self::Text { x, y, text, .. } => {
                if *x >= shape.cell_width || *y >= shape.cell_height || !text_ok(text) {
                    return Err(SurfaceError::Invalid("text command"));
                }
            } // Single-line bounded plain Unicode text.
            Self::Blit {
                handle,
                source,
                target,
                ..
            } => {
                // Only already-prepared native data is eligible.
                if handle.is_empty()
                    || handle.len() > 128
                    || !handle
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
                    || source.width == 0
                    || source.height == 0
                    || source.x.checked_add(source.width).is_none()
                    || source.y.checked_add(source.height).is_none()
                {
                    return Err(SurfaceError::Invalid("blit handle or source"));
                } // Native checks actual source dimensions and grants.
                let layout = shape.layout()?;
                target.check(layout.width, layout.height)?; // Target coordinates use the accepted sample representation.
            } // No filesystem path or publisher claim enters this command.
        } // Unknown variants fail serde decoding.
        Ok(()) // The native adapter still validates its concrete resource/backend contract.
    } // End command validation.
} // End packed commands.
#[derive(Clone, Debug, Serialize, Deserialize)] // Only metadata crosses JSON.
#[serde(deny_unknown_fields)] // Native compares this against its own accepted shape/key.
pub struct FrameMeta {
    pub wire_version: u32,
    pub key: FrameKey,
    pub shape: Shape,
    pub presented: bool,
    pub error: Option<String>,
    pub commands: Vec<Command>,
} // No final masks or owner IDs.
impl FrameMeta {
    // Engine must enforce the byte cap before V8-to-Rust conversion too.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        // No giant JSON baseline or pixel array.
        if bytes.len() > MAX_META_BYTES {
            return Err(SurfaceError::Capacity);
        } // Bound before parsing.
        serde_json::from_slice(bytes).map_err(|_| SurfaceError::Invalid("frame metadata JSON"))
        // Unknown fields and malformed values fail closed.
    } // The complete package plan is validated elsewhere.
} // End metadata parser.
#[derive(Clone, Debug)] // Exactly the sealed semantic planes, copied by the engine.
pub struct Planes {
    pub data: Data,
    pub touch: Vec<u8>,
    pub order: Vec<u32>,
    pub cell_rgb: Option<Vec<u8>>,
    pub colour_touch: Option<Vec<u8>>,
    pub colour_order: Option<Vec<u32>>,
} // Working/input planes are detached but not consumed here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)] // No Deserialize implementation or public integer field.
#[repr(transparent)] // Preserve NonZeroU64's guaranteed Option niche for the declared owner-plane budget.
pub struct SourceToken(NonZeroU64); // The native evidence store owns the authenticated meaning.
impl SourceToken {
    // Only a trusted prepared-source adapter constructs a token.
    pub(crate) fn from_native(value: u64) -> Result<Self> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(SurfaceError::Invalid("zero source token"))
    } // Validate source authentication before this call.
    pub fn evidence_key(self) -> u64 {
        self.0.get()
    } // Native snapshot sealing resolves this into retained evidence.
} // Tokens never appear in JS metadata or writable output planes.
#[derive(Clone, Debug)] // A native-rendered patch has already paid its source/preparation admission.
pub struct NativePatch {
    pub rect: Rect,
    pub data: Data,
    pub state: Vec<u8>,
    pub owners: Vec<Option<SourceToken>>,
} // Owners are per dot, including eight entries per mask cell.
#[derive(Clone, Debug, PartialEq, Eq)] // Native Unicode layout produces complete leading-cell glyphs.
pub struct NativeText {
    pub x: u32,
    pub y: u32,
    pub text: String,
    pub width: u8,
    pub style: TextStyle,
} // Native must verify actual Unicode width/grapheme rules.
#[derive(Default)] // Native handlers return an explicit complete result or an error.
pub struct NativeOutput {
    pub patches: Vec<NativePatch>,
    pub text: Vec<NativeText>,
    pub colours: Vec<(u32, u32, Option<[u8; 3]>)>,
} // None is accepted directly from script JSON.
pub trait NativeRenderer {
    // Integration-ready pure rendering boundary; no invented ambient API.
    fn render(
        &mut self,
        command: &Command,
        shape: Shape,
        remaining_samples: usize,
    ) -> Result<NativeOutput>; // No I/O acquisition or unchecked allocation here.
} // A handler must validate prepared-resource permission/evidence before returning source-owned data.
pub struct NoNativeRenderer; // Useful for pure procedural packages and standalone tests.
impl NativeRenderer for NoNativeRenderer {
    // Unsupported commands are errors, never silently ignored.
    fn render(&mut self, _: &Command, _: Shape, _: usize) -> Result<NativeOutput> {
        Err(SurfaceError::Unsupported)
    } // Parent supplies actual native adapters.
} // This default does not claim native service implementation.
#[derive(Clone, Debug)] // Immutable snapshots are complete logical surfaces, not transport deltas.
pub struct Snapshot {
    shape: Shape,
    layout: Layout,
    data: Data,
    state: Vec<u8>,
    owners: Vec<Option<SourceToken>>,
    cell_rgb: Option<Vec<u8>>,
    colour_present: Option<Vec<u8>>,
    text: BTreeMap<usize, NativeText>,
} // State: 0 invalid, 1 known empty, 2 drawn.
#[derive(Clone, Debug)] // The binary seed is a fresh copy of the last accepted logical surface.
pub struct FrameSeed {
    pub key: FrameKey,
    pub shape: Shape,
    pub data: Data,
    pub cell_rgb: Option<Vec<u8>>,
    pub reset: bool,
    pub invalid_rects: Vec<Rect>,
} // No provenance is sent to JavaScript.
#[derive(Clone, Debug, PartialEq, Eq)] // Logical acknowledgement remains distinct from actual emission.
pub struct Outcome {
    pub accepted: bool,
    pub changed: bool,
    pub version: u64,
    pub damage: Vec<Rect>,
} // Damage is in cells.
pub struct Surface {
    instance_id: u64,
    revision: u64,
    version: u64,
    last_sequence: u64,
    pending: Option<FrameKey>,
    snapshot: Snapshot,
} // One canonical owner.
impl Surface {
    // Candidate snapshots remain private until the complete transaction succeeds.
    pub fn new(instance_id: u64, revision: u64, shape: Shape) -> Result<Self> {
        // Parent reserves the declared simultaneous memory first.
        if instance_id == 0 || revision == 0 {
            return Err(SurfaceError::Invalid("surface identity"));
        } // Zero identifies no active instance.
        Ok(Self {
            instance_id,
            revision,
            version: 0,
            last_sequence: 0,
            pending: None,
            snapshot: Snapshot::empty(shape)?,
        }) // Initial backing is known empty.
    } // A format/size/settings reset creates a new revision through this constructor.
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    } // Borrowing is read-only; retained transport copies require admission.
    pub fn version(&self) -> u64 {
        self.version
    } // This is the accepted logical version, not the terminal generation.
    pub fn begin(&mut self, sequence: u64) -> Result<FrameSeed> {
        // Native owns this operation, never a helper-supplied sequence.
        if self.pending.is_some() {
            return Err(SurfaceError::Busy);
        } // Reject render re-entry.
        if sequence == 0 || sequence <= self.last_sequence {
            return Err(SurfaceError::Stale);
        } // Never reuse a retired lease.
        let key = FrameKey {
            instance_id: self.instance_id.to_string(),
            revision: self.revision.to_string(),
            base_version: self.version.to_string(),
            sequence: sequence.to_string(),
        }; // Decimal identity is exact in JSON.
        let shape = self.snapshot.shape;
        let replace = shape.update == Update::Replace; // Retain reads only the last accepted backing.
        let data = if replace {
            Data::zero(shape)?
        } else {
            self.snapshot.data.clone()
        }; // Baseline bytes go through trusted binary injection.
        let cell_rgb = if replace {
            self.snapshot.cell_rgb.as_ref().map(|v| vec![0; v.len()])
        } else {
            self.snapshot.cell_rgb.clone()
        }; // No RGB plane outside the accepted format.
        let seed = FrameSeed {
            key: key.clone(),
            shape,
            data,
            cell_rgb,
            reset: self.version == 0,
            invalid_rects: self.snapshot.invalid_rects(),
        }; // Lifecycle/backing invalidity reveals no UI occupancy.
        self.last_sequence = sequence;
        self.pending = Some(key); // Publish the lease after bounded seed construction.
        Ok(seed) // JS must receive a fresh seed even after a rejected frame.
    } // Copies and V8 backing stores remain charged until physically released.
    pub fn abort(&mut self) {
        self.pending = None;
    } // Exceptions/timeouts consume the open lease without touching accepted pixels.
    pub fn finish(
        &mut self,
        metadata: FrameMeta,
        planes: Planes,
        renderer: &mut impl NativeRenderer,
    ) -> Result<Outcome> {
        // One complete host-side decision.
        self.finish_with(metadata, planes, renderer, |_, _| Ok(())) // Pure consumers have no additional fallible transport staging.
    } // The real engine uses finish_with to stage immutable output/evidence before committing.
    pub fn finish_with(
        &mut self,
        metadata: FrameMeta,
        planes: Planes,
        renderer: &mut impl NativeRenderer,
        before_commit: impl FnOnce(&Snapshot, &Outcome) -> Result<()>,
    ) -> Result<Outcome> {
        // Coupled native admission/validation hook.
        let expected = self.pending.take().ok_or(SurfaceError::Stale)?; // Any finish attempt consumes the native lease.
        if metadata.key != expected
            || metadata.shape != self.snapshot.shape
            || metadata.wire_version != 1
        {
            return Err(SurfaceError::Stale);
        } // Compare against native state, not a script baseline.
        if metadata.error.is_some() {
            return Err(SurfaceError::Callback);
        } // Poison cannot be cleared by catching a facade exception.
        if metadata.commands.len() > MAX_COMMANDS {
            return Err(SurfaceError::Capacity);
        } // Bound typed Rust inputs too.
        metadata_size(&metadata)?; // Engine must also cap metadata before conversion/allocation.
        self.validate_planes(&planes, &metadata.commands)?; // No indexing or native callbacks before full plane validation.
        if !metadata.presented {
            return Ok(Outcome {
                accepted: false,
                changed: false,
                version: self.version,
                damage: Vec::new(),
            });
        } // No-present keeps the previous accepted frame.
        let replace = self.snapshot.shape.update == Update::Replace; // A submitted empty replacement erases all old state.
        let mut candidate = if replace {
            Snapshot::empty(self.snapshot.shape)?
        } else {
            self.snapshot.clone()
        }; // Rollback is destruction of this private candidate.
        let mut damage = vec![replace; candidate.layout.cells];
        let mut cell_order = vec![0_u32; candidate.layout.cells];
        let mut colour_order = planes
            .colour_order
            .clone()
            .unwrap_or_else(|| vec![0; candidate.layout.cells]); // Damage, colour, and sample order are independent.
        let mut changed = replace; // Replace includes implicit clearing even when every touch entry is zero.
        for index in 0..candidate.layout.samples {
            // Final ordinary overwrites can be applied before native commands.
            let touch = planes.touch[index];
            if touch == 0 {
                continue;
            } // Untouched retained samples preserve exact identity.
            let cell = candidate.cell_of(index);
            cell_order[cell] = cell_order[cell].max(planes.order[index]); // Native text cannot straddle a later overwrite.
            if touch >= 2 && candidate.shape.cell_rgb && planes.order[index] > colour_order[cell] {
                candidate.set_colour(cell, None)?;
                candidate.clear_cell_owners(cell);
                colour_order[cell] = planes.order[index];
            } // Native derives implicit clear/defer colour erasure independently of JS.
            let value = if touch == 1 {
                planes
                    .data
                    .read(candidate.shape.format, candidate.layout.width, index)
            } else {
                [0.0; 4]
            }; // Clear/defer never resurrect old intensity.
            candidate
                .data
                .write(candidate.shape.format, candidate.layout.width, index, value); // One authoritative representation.
            candidate.state[index] = match touch {
                1 => candidate.drawn_state(value),
                2 => 1,
                _ => 0,
            }; // Defer is invalid; clear is known empty.
            candidate.clear_owners(index);
            candidate.erase_text(cell, &mut damage);
            damage[cell] = true;
            changed = true; // Equal-byte writes still erase source metadata.
        } // Command execution below skips samples overwritten later in the transaction.
        if let (Some(rgb), Some(touch), Some(order)) =
            (&planes.cell_rgb, &planes.colour_touch, &planes.colour_order)
        {
            // Explicit colour presence is separate from black RGB.
            for cell in 0..candidate.layout.cells {
                // Colour edits affect the complete Braille cell.
                if touch[cell] == 0 || order[cell] < colour_order[cell] {
                    continue;
                } // A later clear/defer wins over an earlier explicit colour edit.
                candidate.set_colour(
                    cell,
                    if touch[cell] == 1 {
                        Some([rgb[cell * 3], rgb[cell * 3 + 1], rgb[cell * 3 + 2]])
                    } else {
                        None
                    },
                )?; // Palette reset does not mean explicit black.
                cell_order[cell] = cell_order[cell].max(order[cell]);
                candidate.clear_cell_owners(cell);
                candidate.erase_text(cell, &mut damage);
                damage[cell] = true;
                changed = true; // Colour rewrites invalidate old source attribution too.
            } // Per-sample paint order remains independent of colour order.
        } // Shapes without optional RGB never allocate these planes.
        let mut remaining = candidate
            .layout
            .samples
            .checked_mul(4)
            .ok_or(SurfaceError::Capacity)?; // Bound total native patch work across commands.
        for command in &metadata.commands {
            // Commands are validated in strictly increasing order.
            let output = renderer.render(command, candidate.shape, remaining)?; // Native validates current resource authority and prepared source identity.
            if output.patches.len() > MAX_FRAGMENTS
                || output.text.len() > MAX_TEXT_BYTES
                || output.colours.len() > candidate.layout.cells
            {
                return Err(SurfaceError::Capacity);
            } // Do not trust a buggy adapter's output envelope.
            for (x, y, rgb) in output.colours {
                // Native colour reduction must name exact affected cells.
                if x >= candidate.shape.cell_width || y >= candidate.shape.cell_height {
                    return Err(SurfaceError::Invalid("native colour bounds"));
                } // No inferred clipping.
                let cell = y as usize * candidate.shape.cell_width as usize + x as usize; // Checked cell coordinate.
                if colour_order[cell] > command.order() {
                    continue;
                } // A later explicit edit or implicit clear wins.
                candidate.set_colour(cell, rgb)?;
                candidate.clear_cell_owners(cell);
                candidate.erase_text(cell, &mut damage);
                damage[cell] = true; // Native mixed colour conservatively drops source ownership.
            } // A handler may instead return RGB/RGBA samples with authenticated ownership.
            for patch in output.patches {
                // Verified source patches restore ownership after the command's colour changes.
                let count = (patch.rect.width as usize)
                    .checked_mul(patch.rect.height as usize)
                    .ok_or(SurfaceError::Capacity)?; // Check work before data scans.
                if count > remaining {
                    return Err(SurfaceError::Capacity);
                }
                remaining -= count; // Work remains charged even where clipping/order skips writes.
                candidate.apply_patch(command, patch, &planes.order, &colour_order, &mut damage)?;
                // Source/validity/damage changes stay in the candidate.
            } // Any later failure rolls back earlier command results too.
            for text in output.text {
                // Native supplies grapheme/width correctness; Rust enforces cell consistency.
                candidate.put_text(text, command.order(), &cell_order, &mut damage)?;
                // Whole wide glyphs are accepted or suppressed.
            } // Text remains part of the same logical transaction.
            changed = true; // Accepted command bookkeeping may matter even with no visible output.
        } // No final-compositor masks are consumed here.
        if !changed {
            let outcome = Outcome {
                accepted: true,
                changed: false,
                version: self.version,
                damage: Vec::new(),
            };
            before_commit(&self.snapshot, &outcome)?;
            return Ok(outcome);
        } // A retained no-op is acknowledged without a new base.
        let mut evidence = BTreeSet::new();
        for owner in candidate.owners.iter().flatten() {
            evidence.insert(owner.evidence_key());
            if evidence.len() > 8192 {
                return Err(SurfaceError::Capacity);
            }
        } // Preserve the supplied receipt owner's finite identity bound.
        let version = self.version.checked_add(1).ok_or(SurfaceError::Capacity)?; // Never wrap retained base identity.
        let damage = rectangles(
            &damage,
            candidate.shape.cell_width as usize,
            candidate.shape.cell_height as usize,
        ); // Bounded cell damage with fragmentation fallback.
        let outcome = Outcome {
            accepted: true,
            changed: true,
            version,
            damage,
        };
        before_commit(&candidate, &outcome)?; // Reserve/copy/seal output and evidence without making it visible yet.
        self.snapshot = candidate;
        self.version = version; // The only canonical publication point.
        Ok(outcome) // Parent performs only its already-reserved infallible publication after this return.
    } // Native must call __ilium_accept_frame with this logical outcome only.
    fn validate_planes(&self, planes: &Planes, commands: &[Command]) -> Result<()> {
        // Validate all semantic planes and edit orders.
        let shape = self.snapshot.shape;
        let layout = self.snapshot.layout;
        planes
            .data
            .validate(shape.format, layout.width, layout.height)?; // Exact selected type/length.
        if planes.touch.len() != layout.samples || planes.order.len() != layout.samples {
            return Err(SurfaceError::Invalid("touch/order length"));
        } // No undersized or oversized view.
        let mut previous = 0;
        let mut command_orders = Vec::new(); // A small sorted set supports collision checks.
        for command in commands {
            command.validate(shape)?;
            let order = command.order();
            if order <= previous {
                return Err(SurfaceError::Invalid("command order"));
            }
            previous = order;
            command_orders.push(order);
        } // Strict total native-command order.
        let order_ok = |touch: u8, order: u32, maximum: u8| {
            touch <= maximum
                && ((touch == 0) == (order == 0))
                && order <= MAX_EDITS
                && command_orders.binary_search(&order).is_err()
        }; // No simultaneous command and ordinary overwrite at one order.
        if planes
            .touch
            .iter()
            .zip(&planes.order)
            .any(|(t, o)| !order_ok(*t, *o, 3))
        {
            return Err(SurfaceError::Invalid("sample edit order"));
        } // Even unchanged bytes need an explicit nonzero edit order.
        match (
            &planes.cell_rgb,
            &planes.colour_touch,
            &planes.colour_order,
            shape.cell_rgb,
        ) {
            // No optional-plane shape ambiguity.
            (None, None, None, false) => {} // No colour storage was declared.
            (Some(rgb), Some(touch), Some(order), true)
                if rgb.len() == layout.cells * 3
                    && touch.len() == layout.cells
                    && order.len() == layout.cells =>
            {
                if touch.iter().zip(order).any(|(t, o)| !order_ok(*t, *o, 2)) {
                    return Err(SurfaceError::Invalid("colour edit order"));
                }
            } // Explicit colour or palette reset.
            _ => return Err(SurfaceError::Invalid("colour plane shape")), // Reject additional RGB planes on incompatible formats.
        } // Native validates ignored working/input planes before detaching them too.
        Ok(()) // All scans are bounded by Shape::layout.
    } // End complete input validation.
} // End transactional surface owner.
impl Snapshot {
    // Canonical state is private; native consumers receive read-only views.
    fn empty(shape: Shape) -> Result<Self> {
        // Construct known-empty state, independently of intensity.
        let layout = shape.layout()?;
        Ok(Self {
            shape,
            layout,
            data: Data::zero(shape)?,
            state: vec![1; layout.samples],
            owners: vec![None; layout.dots],
            cell_rgb: shape.cell_rgb.then(|| vec![0; layout.cells * 3]),
            colour_present: shape.cell_rgb.then(|| vec![0; layout.cells]),
            text: BTreeMap::new(),
        }) // Palette sentinel is explicit.
    } // The caller owns pre-allocation admission.
    pub fn shape(&self) -> Shape {
        self.shape
    } // Read-only accepted shape.
    pub fn data(&self) -> &Data {
        &self.data
    } // Never expose mutable native pixels to JavaScript.
    pub fn states(&self) -> &[u8] {
        &self.state
    } // 0 invalid, 1 empty, 2 drawn.
    pub fn owners(&self) -> &[Option<SourceToken>] {
        &self.owners
    } // Native evidence sealing consumes these exact per-dot tokens.
    pub fn text(&self) -> impl Iterator<Item = &NativeText> {
        self.text.values()
    } // Final renderer owns clipping and wide-cell masks.
    fn cell_of(&self, sample: usize) -> usize {
        // Samples are cells only for mask8.
        if self.shape.mode == Mode::Cells {
            return sample;
        } // Direct cells already use cell indices.
        (sample / self.layout.width / 4) * self.shape.cell_width as usize
            + sample % self.layout.width / 2 // Pixel damage rounds outward to its cell.
    } // All eight retained dots are later repacked together.
    fn dot_indices(&self, sample: usize) -> ([usize; 8], usize) {
        // Fixed stack storage avoids per-dot heap allocations.
        if self.shape.mode == Mode::Pixels {
            let mut indices = [0; 8];
            indices[0] = sample;
            return (indices, 1);
        } // Pixel sample order equals dot order.
        let x = sample % self.shape.cell_width as usize * 2;
        let y = sample / self.shape.cell_width as usize * 4;
        let width = self.shape.cell_width as usize * 2; // Expand one Braille cell.
        let mut indices = [0; 8];
        for dy in 0..4 {
            for dx in 0..2 {
                indices[dy * 2 + dx] = (y + dy) * width + x + dx;
            }
        }
        (indices, 8) // Spatial dot order, not bit-number order.
    } // Native source maps use spatial dot order, not bit-number order.
    fn clear_owners(&mut self, sample: usize) {
        let (dots, count) = self.dot_indices(sample);
        for dot in &dots[..count] {
            self.owners[*dot] = None;
        }
    } // Equal-value writes still clear provenance.
    fn clear_cell_owners(&mut self, cell: usize) {
        // Explicit colour changes affect all dots in a cell.
        let width = self.shape.cell_width as usize * 2;
        let x = cell % self.shape.cell_width as usize * 2;
        let y = cell / self.shape.cell_width as usize * 4; // Valid cell index.
        for dy in 0..4 {
            for dx in 0..2 {
                self.owners[(y + dy) * width + x + dx] = None;
            }
        } // Never transfer attribution from an earlier colour.
    } // This conservative rule may undercount mixed content, never invent ownership.
    fn drawn_state(&self, value: [f32; 4]) -> u8 {
        // Zero intensity differs from explicit erasure.
        if (self.shape.format == Format::Mask8 && value[0] == 0.0)
            || (self.shape.format == Format::Rgba8 && value[3] == 0.0)
        {
            return 1;
        } // Zero mask/alpha means known empty.
        2 // Mono/grey/RGB zero is an intentionally drawn sample and may be inverted.
    } // Clear and defer bypass this conversion entirely.
    fn set_colour(&mut self, cell: usize, rgb: Option<[u8; 3]>) -> Result<()> {
        // Presence is independent of byte value.
        let plane = self
            .cell_rgb
            .as_mut()
            .ok_or(SurfaceError::Invalid("undeclared cell RGB"))?;
        let present = self
            .colour_present
            .as_mut()
            .ok_or(SurfaceError::Invalid("missing colour presence"))?; // No lazy undeclared allocation.
        plane[cell * 3..cell * 3 + 3].copy_from_slice(&rgb.unwrap_or([0; 3]));
        present[cell] = u8::from(rgb.is_some());
        Ok(()) // Explicit [0,0,0] stays present.
    } // Palette selection happens only when presence is zero.
    fn erase_text(&mut self, cell: usize, damage: &mut [bool]) {
        // Never leave half of a wide glyph after an overwrite.
        let starts = [cell, cell.saturating_sub(1)];
        let count = if cell.is_multiple_of(self.shape.cell_width as usize) {
            1
        } else {
            2
        }; // Fixed storage avoids a heap allocation per overwritten dot.
        for &start in &starts[..count] {
            // At most two candidate leading cells can cover the target.
            let overlaps = self
                .text
                .get(&start)
                .is_some_and(|text| start + usize::from(text.width) > cell); // A preceding narrow glyph does not overlap.
            if !overlaps {
                continue;
            } // Preserve adjacent independent text.
            if let Some(text) = self.text.remove(&start) {
                for offset in 0..usize::from(text.width) {
                    damage[start + offset] = true;
                }
            } // Whole-glyph removal contributes damage.
        } // Final renderer still validates its own wide-cell protection mask.
    } // Ordinary writes, clears, and native later paint all use this rule.
    fn put_text(
        &mut self,
        mut text: NativeText,
        order: u32,
        later: &[u32],
        damage: &mut [bool],
    ) -> Result<()> {
        // Native Unicode adapter owns actual glyph-width validation.
        if !matches!(text.width, 1 | 2)
            || !text_ok(&text.text)
            || text.text.len() > 256
            || text.x >= self.shape.cell_width
            || text.y >= self.shape.cell_height
            || u32::from(text.width) > self.shape.cell_width - text.x
        {
            return Err(SurfaceError::Invalid("native text cell"));
        } // No split or out-of-bounds glyph.
        let cell = text.y as usize * self.shape.cell_width as usize + text.x as usize; // Valid leading-cell position.
        if (0..usize::from(text.width)).any(|dx| later[cell + dx] > order) {
            return Ok(());
        } // A later ordinary write suppresses the whole glyph.
        for dx in 0..usize::from(text.width) {
            self.erase_text(cell + dx, damage);
            self.clear_cell_owners(cell + dx);
            damage[cell + dx] = true;
        } // Remove conflicting retained glyphs and attribution.
        let bytes = self.text.values().try_fold(text.text.len(), |sum, cell| {
            sum.checked_add(cell.text.len())
                .ok_or(SurfaceError::Capacity)
        })?; // Bound complete retained text.
        if bytes > MAX_TEXT_BYTES || self.text.len() >= MAX_TEXT_BYTES {
            return Err(SurfaceError::Capacity);
        } // Candidate rollback handles a rejected insertion.
        text.text = text.text.into_boxed_str().into_string();
        self.text.insert(cell, text);
        Ok(()) // Do not retain a native String's excessive spare capacity.
    } // No terminal emission claim is produced.
    pub fn invalid_rects(&self) -> Vec<Rect> {
        // Report backing invalidity without inspecting UI visibility.
        let mut invalid = vec![false; self.layout.cells];
        for (sample, state) in self.state.iter().enumerate() {
            if *state == 0 {
                invalid[self.cell_of(sample)] = true;
            }
        } // Any invalid dot requires cell repair.
        rectangles(
            &invalid,
            self.shape.cell_width as usize,
            self.shape.cell_height as usize,
        ) // Exposure filtering is a separately authorized native concern.
    } // Rejected and unsubmitted transactions cannot acknowledge away this invalidity.
} // Native patches and packing follow below.
impl Snapshot {
    // Execute only validated native results on the private candidate.
    fn apply_patch(
        &mut self,
        command: &Command,
        patch: NativePatch,
        later: &[u32],
        later_colour: &[u32],
        damage: &mut [bool],
    ) -> Result<()> {
        // Preserve mixed command/ordinary-write order.
        patch.rect.check(self.layout.width, self.layout.height)?;
        let width = patch.rect.width as usize;
        let height = patch.rect.height as usize; // Native output still needs exact bounds.
        patch.data.validate(self.shape.format, width, height)?;
        let count = width * height;
        let dots_per_sample = if self.shape.mode == Mode::Cells { 8 } else { 1 }; // Source owners are always spatial dots.
        if patch.state.len() != count
            || patch.state.iter().any(|v| *v > 2)
            || patch.owners.len() != count * dots_per_sample
        {
            return Err(SurfaceError::Invalid("native patch planes"));
        } // Native state 0 skips, 1 clears, 2 draws.
        if !matches!(command, Command::Blit { .. }) && patch.owners.iter().any(Option::is_some) {
            return Err(SurfaceError::Invalid("unowned command claimed provenance"));
        } // Vector/text rasterization cannot invent a source.
        for source in 0..count {
            // Visit bounded patch samples in row-major order.
            if patch.state[source] == 0 {
                continue;
            } // Unrasterized bounding-box samples have no effect.
            let x = patch.rect.x as usize + source % width;
            let y = patch.rect.y as usize + source / width;
            let target = y * self.layout.width + x; // Dimensions already checked.
            if later[target] > command.order() {
                continue;
            } // A later scalar/row/direct overwrite wins even when its bytes match.
            let cell = self.cell_of(target);
            let old = self.data.read(self.shape.format, self.layout.width, target);
            let mut value = patch.data.read(self.shape.format, width, source); // Read only the current target sample.
            let mut mixed = false;
            let clear = patch.state[source] == 1; // Clearing is independent of sample intensity.
            if clear && command.blend() != Blend::Overwrite {
                continue;
            } // Transparent geometry does not erase under max/alpha blend.
            if clear {
                value = [0.0; 4];
            } // Never retain hidden clear bytes.
            if !clear
                && command.blend() == Blend::Max
                && self.state[target] == 2
                && self.shape.format != Format::Mask8
            {
                // Preserve strongest-sample arbitration.
                let incoming = luminance(self.shape, value);
                let previous = luminance(self.shape, old); // Compare whole samples, not per-channel maxima.
                if incoming < previous {
                    continue;
                } // The previous stronger source survives unchanged.
                if incoming == previous {
                    self.clear_owners(target);
                    damage[cell] = true;
                    continue;
                } // Ties have conservative mixed ownership.
            } // A new sample still draws over invalid/known-empty state.
            if !clear && command.blend() == Blend::Alpha {
                // Only rgba8 passed command validation.
                if value[3] == 0.0 {
                    continue;
                } // Zero-alpha compositing cannot erase existing pixels.
                let previous = if self.state[target] == 2 {
                    old
                } else {
                    [0.0; 4]
                };
                mixed = value[3] < 255.0 && previous[3] > 0.0;
                value = alpha_over(self.shape, value, previous); // Straight RGB, linear alpha, declared colour space.
            } // Mixed samples cannot retain a single-source claim.
            let old_mask = if self.shape.format == Format::Mask8 && self.state[target] == 2 {
                old[0] as u8
            } else {
                0
            };
            let incoming_mask = value[0] as u8; // Direct masks use bitwise union for max.
            if !clear && command.blend() == Blend::Max && self.shape.format == Format::Mask8 {
                value[0] = f32::from(old_mask | incoming_mask);
            } // Preserve exact Braille geometry.
            self.data
                .write(self.shape.format, self.layout.width, target, value);
            self.state[target] = if clear { 1 } else { self.drawn_state(value) }; // Logical state and pixels change together.
            let (targets, used) = self.dot_indices(target); // Stack-only dot mapping.
            for offset in 0..used {
                // Authenticate only actually drawn source dots.
                let source_dot = if used == 1 {
                    source
                } else {
                    (source / width * 4 + offset / 2) * width * 2 + source % width * 2 + offset % 2
                }; // Match spatial patch-owner order.
                let mut owner = if !clear
                    && self.state[target] == 2
                    && !mixed
                    && later_colour[cell] <= command.order()
                {
                    patch.owners[source_dot]
                } else {
                    None
                }; // Later colour rewrites invalidate attribution.
                if used == 8 {
                    let bit = BRAILLE[offset / 2][offset % 2];
                    if incoming_mask & bit == 0 {
                        owner = if command.blend() == Blend::Max && !clear {
                            self.owners[targets[offset]]
                        } else {
                            None
                        };
                    } else if command.blend() == Blend::Max && old_mask & bit != 0 {
                        owner = None;
                    }
                } // Shared mask bits conservatively lose ownership.
                self.owners[targets[offset]] = owner; // Never derive an owner from JS metadata.
            } // A native token's evidence store must outlive the resulting snapshot/receipt.
            if clear && self.shape.cell_rgb && later_colour[cell] <= command.order() {
                self.set_colour(cell, None)?;
                self.clear_cell_owners(cell);
            } // Clear removes explicit colour too.
            self.erase_text(cell, damage);
            damage[cell] = true; // Raster overpaint removes complete overlapping glyphs.
        } // No partial result escapes if any subsequent native command fails.
        Ok(()) // Candidate publication remains in Surface::finish.
    } // End native patch application.
    pub fn pack(
        &self,
        mut tone: impl FnMut(f32, usize, usize) -> f32,
        mut threshold: impl FnMut(f32, usize, usize) -> bool,
    ) -> Result<PackedSurface> {
        // Pure pre-compositor encoding boundary.
        let mut masks = vec![0; self.layout.cells];
        let mut rgb = vec![None; self.layout.cells];
        let mut owners = vec![None; self.layout.dots]; // Caller admits this immutable output separately.
        let width = self.shape.cell_width as usize * 2; // Pixel row stride for all formats.
        for cell in 0..self.layout.cells {
            // Recompute from all eight retained dots in each cell.
            let cx = cell % self.shape.cell_width as usize;
            let cy = cell / self.shape.cell_width as usize;
            let mut sums = [0.0_f32; 3];
            let mut weight_sum = 0.0_f32; // Coverage-weighted linear colour reduction.
            for (dy, bits) in BRAILLE.iter().enumerate() {
                for (dx, bit) in bits.iter().enumerate() {
                    // Exact Unicode bit positions.
                    let x = cx * 2 + dx;
                    let y = cy * 4 + dy;
                    let dot = y * width + x;
                    let sample = if self.shape.mode == Mode::Cells {
                        cell
                    } else {
                        dot
                    }; // Canonical sample index.
                    if self.state[sample] != 2 {
                        continue;
                    } // Invalid and erased samples bypass tone/inversion entirely.
                    let value = self.data.read(self.shape.format, self.layout.width, sample); // Validated canonical sample.
                    let direct = self.shape.format == Format::Mask8;
                    if direct && (value[0] as u8 & *bit) == 0 {
                        continue;
                    } // Direct masks never regenerate absent bits.
                    let alpha = if self.shape.format == Format::Rgba8 {
                        value[3] / 255.0
                    } else {
                        1.0
                    };
                    if alpha == 0.0 {
                        continue;
                    } // Alpha gates coverage after tone.
                    let shaped = if direct {
                        1.0
                    } else {
                        tone(luminance(self.shape, value), x, y)
                    };
                    if !shaped.is_finite() || !(0.0..=1.0).contains(&shaped) {
                        return Err(SurfaceError::Invalid("tone coverage"));
                    } // Native tone must return bounded coverage.
                    let coverage = shaped * alpha;
                    if coverage == 0.0 || (!direct && !threshold(coverage, x, y)) {
                        continue;
                    } // Local/nonlocal threshold dependency policy belongs to the host.
                    masks[cell] |= *bit;
                    owners[dot] = self.owners[dot]; // These remain logical masks, never final-emission authority.
                    let colour = if let (Some(plane), Some(present)) =
                        (&self.cell_rgb, &self.colour_present)
                    {
                        if present[cell] == 1 {
                            Some(std::array::from_fn(|channel| {
                                let value = f32::from(plane[cell * 3 + channel]) / 255.0;
                                if self.shape.colour_space == ColourSpace::Srgb {
                                    linear(value)
                                } else {
                                    value
                                }
                            }))
                        } else {
                            None
                        }
                    } else if matches!(self.shape.format, Format::Rgb8 | Format::Rgba8) {
                        Some(rgb_linear(self.shape, value))
                    } else {
                        None
                    }; // Palette sentinel contributes no explicit colour.
                    if let Some(colour) = colour {
                        for channel in 0..3 {
                            sums[channel] += colour[channel] * coverage;
                        }
                        weight_sum += coverage;
                    } // Only surviving dots contribute.
                }
            } // Final UI clipping may still remove any of these dots.
            if weight_sum > 0.0 {
                rgb[cell] = Some(std::array::from_fn(|channel| {
                    (srgb(sums[channel] / weight_sum) * 255.0)
                        .round()
                        .clamp(0.0, 255.0) as u8
                }));
            } // Explicit black remains Some([0,0,0]).
        } // Styled text is carried separately through Snapshot::text.
        Ok(PackedSurface { masks, rgb, owners }) // Host recomputes final source receipts from actual surviving emitted dots.
    } // Direct mask8 bypasses intensity controls and threshold callbacks.
} // End snapshot mutation and encoding.
pub struct PackedSurface {
    pub masks: Vec<u8>,
    pub rgb: Vec<Option<[u8; 3]>>,
    pub owners: Vec<Option<SourceToken>>,
} // Native pre-compositor output only.
fn text_ok(text: &str) -> bool {
    !text.is_empty() && text.len() <= MAX_TEXT_BYTES && !text.chars().any(|c| c.is_control())
} // Native font/grapheme validation remains additional.
fn linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
} // sRGB decoding for luminance/compositing.
fn srgb(value: f32) -> f32 {
    if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
} // Encode terminal RGB after linear reduction.
fn rgb_linear(shape: Shape, value: [f32; 4]) -> [f32; 3] {
    std::array::from_fn(|i| {
        if shape.colour_space == ColourSpace::Srgb {
            linear(value[i] / 255.0)
        } else {
            value[i] / 255.0
        }
    })
} // Alpha is excluded from colour conversion.
fn luminance(shape: Shape, value: [f32; 4]) -> f32 {
    // Shared pre-tone intensity policy.
    match shape.format {
        Format::Mono1 | Format::Mono8 | Format::Gray32 => value[0],
        Format::Mask8 | Format::Gray8 => value[0] / 255.0,
        _ => {
            let rgb = rgb_linear(shape, value);
            (0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]).clamp(0.0, 1.0)
        }
    } // Standard weights with bounded floating rounding.
} // Native appearance may transform this intensity but cannot bypass state/alpha gating.
fn alpha_over(shape: Shape, source: [f32; 4], target: [f32; 4]) -> [f32; 4] {
    // Straight-alpha source-over in linear colour.
    let a = source[3] / 255.0;
    let b = target[3] / 255.0;
    let out = a + b * (1.0 - a);
    if out == 0.0 {
        return [0.0; 4];
    } // Avoid division by zero.
    let s = rgb_linear(shape, source);
    let t = rgb_linear(shape, target);
    let mut value = [0.0; 4]; // Work in linear light.
    for i in 0..3 {
        let c = (s[i] * a + t[i] * b * (1.0 - a)) / out;
        value[i] = (if shape.colour_space == ColourSpace::Srgb {
            srgb(c)
        } else {
            c
        } * 255.0)
            .round()
            .clamp(0.0, 255.0);
    } // Return the admitted byte colour space.
    value[3] = (out * 255.0).round().clamp(0.0, 255.0);
    value // Alpha stays linear coverage.
} // Mixed-source ownership is removed by apply_patch.
fn rectangles(bits: &[bool], width: usize, height: usize) -> Vec<Rect> {
    // Coalesce horizontal runs and identical adjacent rows.
    let full = || {
        vec![Rect {
            x: 0,
            y: 0,
            width: width as u32,
            height: height as u32,
        }]
    }; // Bounded fallback for fragmentation or dense change.
    let area = bits.iter().filter(|v| **v).count();
    if area == 0 {
        return Vec::new();
    }
    if area * 2 >= bits.len() {
        return full();
    } // Dense updates use one full rectangle.
    let mut result: Vec<Rect> = Vec::new(); // Output count never exceeds MAX_FRAGMENTS.
    for y in 0..height {
        let mut x = 0;
        while x < width {
            // Scan each cell at most once.
            if !bits[y * width + x] {
                x += 1;
                continue;
            }
            let start = x;
            while x < width && bits[y * width + x] {
                x += 1;
            } // One contiguous row run.
            if let Some(old) = result.iter_mut().rev().find(|rect| {
                rect.x == start as u32
                    && rect.width == (x - start) as u32
                    && rect.y + rect.height == y as u32
            }) {
                old.height += 1;
                continue;
            } // Extend only an exactly matching previous-row run.
            if result.len() >= MAX_FRAGMENTS {
                return full();
            }
            result.push(Rect {
                x: start as u32,
                y: y as u32,
                width: (x - start) as u32,
                height: 1,
            }); // Fragmentation fallback is conservative.
        }
    } // No UI visibility data enters this coalescer.
    result // Returned coordinates are always cells.
} // Invalidity and damage call this independently.
fn metadata_size(value: &impl Serialize) -> Result<()> {
    // Bound serialization without producing an unbounded intermediate string.
    struct Counter(usize); // Only a byte count is retained.
    impl Write for Counter {
        // serde_json streams through the same maximum as native metadata.
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| io::Error::other("overflow"))?;
            if self.0 > MAX_META_BYTES {
                return Err(io::Error::other("metadata budget"));
            }
            Ok(bytes.len())
        } // Reject before retaining another chunk.
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        } // There is no external I/O sink.
    } // This does not replace native V8 object/depth limits.
    serde_json::to_writer(Counter(0), value).map_err(|_| SurfaceError::Capacity)
    // Schema serialization failures are rejection, never default acceptance.
} // End complete pure surface implementation.
#[cfg(test)] // These pure tests require no V8, OS sandbox, or application crate.
mod tests {
    // Exercise the actual wire/transaction/packing contracts.
    use super::*; // Test native constructors are deliberately unavailable to scripts.
    fn shape(format: Format, update: Update, cell_rgb: bool) -> Shape {
        Shape {
            cell_width: 3,
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
    } // Include mono1 row padding.
    fn start(surface: &mut Surface, sequence: u64) -> (FrameMeta, Planes) {
        // Follow the same host-issued lease path as the engine.
        let seed = surface.begin(sequence).unwrap();
        let layout = seed.shape.layout().unwrap(); // Binary seed is the accepted baseline.
        let planes = Planes {
            data: seed.data,
            touch: vec![0; layout.samples],
            order: vec![0; layout.samples],
            cell_rgb: seed.cell_rgb,
            colour_touch: seed.shape.cell_rgb.then(|| vec![0; layout.cells]),
            colour_order: seed.shape.cell_rgb.then(|| vec![0; layout.cells]),
        }; // Separate semantic planes.
        (
            FrameMeta {
                wire_version: 1,
                key: seed.key,
                shape: seed.shape,
                presented: true,
                error: None,
                commands: Vec::new(),
            },
            planes,
        ) // A test explicitly chooses whether to submit.
    } // No test reconstructs a base from the most recently attempted frame.
    fn edit(
        planes: &mut Planes,
        shape: Shape,
        index: usize,
        value: [f32; 4],
        touch: u8,
        order: u32,
    ) {
        planes
            .data
            .write(shape.format, shape.layout().unwrap().width, index, value);
        planes.touch[index] = touch;
        planes.order[index] = order;
    } // Scalar/direct damage semantics.
    fn blit(order: u32, x: u32, handle: &str) -> Command {
        Command::Blit {
            order,
            handle: handle.to_owned(),
            source: Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            target: Rect {
                x,
                y: 0,
                width: 1,
                height: 1,
            },
            blend: Blend::Overwrite,
        }
    } // Prepared-source command fixture.
    struct SourceRenderer; // A tiny deterministic native adapter, with an explicit failure trigger.
    impl NativeRenderer for SourceRenderer {
        // Returned source identity is host constructed.
        fn render(&mut self, command: &Command, _: Shape, _: usize) -> Result<NativeOutput> {
            // No external side effects.
            let Command::Blit { handle, target, .. } = command else {
                return Err(SurfaceError::Unsupported);
            }; // Closed fixture contract.
            if handle == "fail" {
                return Err(SurfaceError::Unsupported);
            } // Fail after an earlier candidate command succeeded.
            Ok(NativeOutput {
                patches: vec![NativePatch {
                    rect: *target,
                    data: Data::U8(vec![128]),
                    state: vec![2],
                    owners: vec![Some(SourceToken::from_native(7)?)],
                }],
                ..NativeOutput::default()
            }) // Authenticated immutable source fixture.
        } // Parent adapters must supply real grant/evidence validation before this boundary.
    } // This fixture does not claim any service implementation.
    #[test] // Every accepted format reaches the same exact Braille geometry.
    fn all_seven_formats_pack_one_top_left_dot() {
        // Include packed pixel bits and direct cell masks separately.
        for format in [
            Format::Mask8,
            Format::Mono1,
            Format::Mono8,
            Format::Gray8,
            Format::Gray32,
            Format::Rgb8,
            Format::Rgba8,
        ] {
            // Complete format inventory.
            let shape = shape(format, Update::Replace, false);
            let mut surface = Surface::new(1, 1, shape).unwrap();
            let (meta, mut planes) = start(&mut surface, 1); // Known-empty initial backing.
            let value = match format {
                Format::Mask8 | Format::Mono1 | Format::Mono8 | Format::Gray32 => {
                    [1.0, 0.0, 0.0, 0.0]
                }
                Format::Rgb8 | Format::Rgba8 => [255.0; 4],
                _ => [255.0, 0.0, 0.0, 0.0],
            }; // Format-specific component units.
            edit(&mut planes, shape, 0, value, 1, 1);
            surface.finish(meta, planes, &mut NoNativeRenderer).unwrap(); // Submit one logical sample.
            let packed = surface
                .snapshot()
                .pack(|v, _, _| v, |v, _, _| v > 0.0)
                .unwrap();
            assert_eq!(packed.masks[0], 1);
            assert!(packed.masks[1..].iter().all(|mask| *mask == 0)); // Exact top-left bit only.
        } // There is no implicit conversion to an always-RGBA workspace.
    } // All formats share the same final compositor boundary.
    #[test] // Clear/defer cannot be revived under inversion; drawn zero can.
    fn erasure_invalidity_and_alpha_gate_after_tone() {
        // Test the semantic difference from a zero-intensity Raster sample.
        let shape = shape(Format::Gray8, Update::Retain, false);
        let mut surface = Surface::new(1, 1, shape).unwrap();
        let (meta, mut planes) = start(&mut surface, 1); // Retained empty background.
        edit(&mut planes, shape, 0, [0.0; 4], 1, 1);
        edit(&mut planes, shape, 1, [255.0; 4], 2, 2);
        edit(&mut planes, shape, 6, [255.0; 4], 3, 3); // Drawn zero, clear, and defer.
        surface.finish(meta, planes, &mut NoNativeRenderer).unwrap();
        assert_eq!(
            surface
                .snapshot()
                .pack(|v, _, _| 1.0 - v, |v, _, _| v > 0.0)
                .unwrap()
                .masks[0],
            1
        ); // Only intentional zero is invertible.
        assert!(!surface.snapshot().invalid_rects().is_empty());
        let (mut meta, planes) = start(&mut surface, 2);
        meta.error = Some("failed".to_owned());
        assert!(surface.finish(meta, planes, &mut NoNativeRenderer).is_err());
        assert!(!surface.snapshot().invalid_rects().is_empty()); // Rejection does not acknowledge invalidity.
        let rgba = super::tests::shape(Format::Rgba8, Update::Replace, false);
        let mut surface = Surface::new(2, 1, rgba).unwrap();
        let (meta, mut planes) = start(&mut surface, 1);
        edit(&mut planes, rgba, 0, [255.0, 0.0, 0.0, 0.0], 1, 1); // Zero alpha is erasure.
        surface.finish(meta, planes, &mut NoNativeRenderer).unwrap();
        assert!(surface
            .snapshot()
            .pack(
                |_, _, _| panic!("empty samples must bypass tone"),
                |_, _, _| true
            )
            .unwrap()
            .masks
            .iter()
            .all(|v| *v == 0)); // No stale/transparent sample survives inversion.
    } // Alpha and validity remain independent from appearance.
    #[test] // Present/no-present and retained no-op have distinct outcomes.
    fn empty_replace_no_present_and_single_finish() {
        // Verify exactly one host-side commit.
        let shape = shape(Format::Gray8, Update::Replace, false);
        let mut surface = Surface::new(1, 1, shape).unwrap();
        let (meta, mut planes) = start(&mut surface, 1);
        edit(&mut planes, shape, 0, [255.0; 4], 1, 1);
        surface.finish(meta, planes, &mut NoNativeRenderer).unwrap(); // First visible sample.
        let (mut meta, planes) = start(&mut surface, 2);
        meta.presented = false;
        let saved_meta = meta.clone();
        let saved_planes = planes.clone();
        assert!(
            !surface
                .finish(meta, planes, &mut NoNativeRenderer)
                .unwrap()
                .accepted
        ); // Omitting present preserves old ink.
        assert_eq!(surface.version(), 1);
        assert_eq!(
            surface.finish(saved_meta, saved_planes, &mut NoNativeRenderer),
            Err(SurfaceError::Stale)
        ); // The same lease cannot finish twice.
        let (meta, planes) = start(&mut surface, 3);
        let outcome = surface.finish(meta, planes, &mut NoNativeRenderer).unwrap();
        assert!(outcome.changed);
        assert_eq!(surface.snapshot().states()[0], 1); // Empty replace erases old ink.
        let mut retained = Surface::new(
            2,
            1,
            super::tests::shape(Format::Gray8, Update::Retain, false),
        )
        .unwrap();
        let (meta, planes) = start(&mut retained, 1);
        let outcome = retained
            .finish(meta, planes, &mut NoNativeRenderer)
            .unwrap();
        assert!(outcome.accepted && !outcome.changed);
        assert_eq!(outcome.version, 0); // Retained empty-present preserves its base.
    } // Accepted logical no-op does not claim terminal emission.
    #[test] // Rejection leaves canonical bytes/base unchanged and requires a fresh lease.
    fn stale_base_bad_planes_and_native_failure_roll_back() {
        // Include failure after a native candidate was modified.
        let shape = shape(Format::Gray8, Update::Retain, false);
        let mut surface = Surface::new(1, 1, shape).unwrap();
        let (mut meta, planes) = start(&mut surface, 1);
        meta.key.base_version = "99".to_owned();
        assert_eq!(
            surface.finish(meta, planes, &mut SourceRenderer),
            Err(SurfaceError::Stale)
        ); // Script cannot advance its base.
        let (meta, mut planes) = start(&mut surface, 2);
        planes.touch.pop();
        assert!(surface.finish(meta, planes, &mut SourceRenderer).is_err()); // Exact lengths are mandatory.
        let (mut meta, planes) = start(&mut surface, 3);
        meta.commands = vec![blit(1, 0, "good"), blit(2, 1, "fail")];
        assert_eq!(
            surface.finish(meta, planes, &mut SourceRenderer),
            Err(SurfaceError::Unsupported)
        ); // Second native failure aborts the first candidate write.
        assert_eq!(surface.version(), 0);
        assert!(surface.snapshot().owners().iter().all(Option::is_none));
        assert!(surface.snapshot().states().iter().all(|v| *v == 1)); // Complete rollback includes provenance and validity.
        let seed = surface.begin(4).unwrap();
        assert_eq!(seed.key.base_version, "0");
        surface.abort(); // Next JS baseline is the accepted surface only.
    } // New same-revision time requests do not enter the base comparison.
    #[test] // Drawing order and equal-byte damage both affect authenticated ownership.
    fn native_order_and_equal_overwrite_clear_provenance() {
        // No arbitrary script owner plane exists.
        let shape = shape(Format::Gray8, Update::Retain, false);
        let mut surface = Surface::new(1, 1, shape).unwrap();
        let (mut meta, mut planes) = start(&mut surface, 1); // Test two relative orders in one frame.
        edit(&mut planes, shape, 0, [7.0; 4], 1, 3);
        edit(&mut planes, shape, 1, [7.0; 4], 1, 1);
        meta.commands = vec![blit(2, 0, "good"), blit(4, 1, "good")];
        surface.finish(meta, planes, &mut SourceRenderer).unwrap(); // Later ordinary write wins at 0; later blit wins at 1.
        assert_eq!(surface.snapshot().data.read(Format::Gray8, 6, 0)[0], 7.0);
        assert_eq!(surface.snapshot().data.read(Format::Gray8, 6, 1)[0], 128.0);
        assert!(surface.snapshot().owners()[0].is_none());
        assert!(surface.snapshot().owners()[1].is_some()); // Correct pixels and source identity.
        let (meta, mut planes) = start(&mut surface, 2);
        edit(&mut planes, shape, 1, [128.0; 4], 1, 1);
        surface.finish(meta, planes, &mut NoNativeRenderer).unwrap();
        assert!(surface.snapshot().owners()[1].is_none()); // Equality of bytes does not preserve provenance.
    } // Immutable snapshots retain their own exact owner map for later receipts.
    #[test] // Native clear semantics do not trust the JS colour-touch bookkeeping.
    fn explicit_black_and_implicit_colour_clear_are_distinct() {
        // Optional colour presence cannot use black as its sentinel.
        let shape = shape(Format::Mask8, Update::Retain, true);
        let mut surface = Surface::new(1, 1, shape).unwrap();
        let (meta, mut planes) = start(&mut surface, 1);
        edit(&mut planes, shape, 0, [255.0; 4], 1, 1);
        planes.colour_touch.as_mut().unwrap()[0] = 1;
        planes.colour_order.as_mut().unwrap()[0] = 1; // Explicit black bytes are already zero.
        surface.finish(meta, planes, &mut NoNativeRenderer).unwrap();
        assert_eq!(
            surface
                .snapshot()
                .pack(|_, _, _| panic!("mask bypass"), |_, _, _| false)
                .unwrap()
                .rgb[0],
            Some([0; 3])
        ); // Direct masks bypass intensity controls.
        let (meta, mut planes) = start(&mut surface, 2);
        edit(&mut planes, shape, 0, [0.0; 4], 2, 1);
        surface.finish(meta, planes, &mut NoNativeRenderer).unwrap(); // Deliberately omit colour-touch clear.
        let (meta, mut planes) = start(&mut surface, 3);
        edit(&mut planes, shape, 0, [1.0; 4], 1, 1);
        surface.finish(meta, planes, &mut NoNativeRenderer).unwrap();
        assert_eq!(
            surface
                .snapshot()
                .pack(|v, _, _| v, |_, _, _| true)
                .unwrap()
                .rgb[0],
            None
        ); // Native implicit clear removed the old explicit colour.
    } // Native appearance can now select the host palette.
    #[test] // Format-specific malformed data is rejected before publication.
    fn padding_nonfinite_values_and_order_collisions_are_rejected() {
        // All failures consume only the attempted lease.
        let mut mono = Surface::new(1, 1, shape(Format::Mono1, Update::Replace, false)).unwrap();
        let (meta, mut planes) = start(&mut mono, 1);
        let Data::U8(data) = &mut planes.data else {
            panic!("byte plane");
        };
        data[0] = 1;
        assert!(mono.finish(meta, planes, &mut NoNativeRenderer).is_err()); // Six-pixel rows have two zero padding bits.
        let mut gray = Surface::new(2, 1, shape(Format::Gray32, Update::Replace, false)).unwrap();
        let (meta, mut planes) = start(&mut gray, 1);
        let Data::F32(data) = &mut planes.data else {
            panic!("float plane");
        };
        data[0] = f32::NAN;
        assert!(gray.finish(meta, planes, &mut NoNativeRenderer).is_err()); // NaN never reaches native packing.
        let mut gray = Surface::new(3, 1, shape(Format::Gray8, Update::Retain, false)).unwrap();
        let (mut meta, mut planes) = start(&mut gray, 1);
        planes.touch[0] = 1;
        planes.order[0] = 1;
        meta.commands.push(blit(1, 0, "good"));
        assert!(gray.finish(meta, planes, &mut SourceRenderer).is_err()); // Native and ordinary edits cannot share an order.
        assert!(shape(Format::Rgba8, Update::Retain, true).layout().is_err()); // No undeclared/redundant companion colour plane.
    } // Kind, length, range, padding, and order are independently checked.
    #[test] // Partial packing keeps every unchanged dot in the affected cell.
    fn partial_updates_repack_complete_cells_and_bound_fragments() {
        // Damage does not discard retained samples.
        let shape = shape(Format::Gray8, Update::Retain, false);
        let mut surface = Surface::new(1, 1, shape).unwrap();
        let (meta, mut planes) = start(&mut surface, 1);
        edit(&mut planes, shape, 0, [255.0; 4], 1, 1);
        edit(&mut planes, shape, 7, [255.0; 4], 1, 2);
        surface.finish(meta, planes, &mut NoNativeRenderer).unwrap(); // Top-left and second-row right.
        let (meta, mut planes) = start(&mut surface, 2);
        edit(&mut planes, shape, 13, [255.0; 4], 1, 1);
        surface.finish(meta, planes, &mut NoNativeRenderer).unwrap();
        assert_eq!(
            surface
                .snapshot()
                .pack(|v, _, _| v, |v, _, _| v > 0.0)
                .unwrap()
                .masks[0],
            49
        ); // Bits 1,16,32 all survive.
        let bits: Vec<bool> = (0..2048).map(|i| i % 4 == 0).collect();
        assert_eq!(
            rectangles(&bits, 1024, 2),
            vec![Rect {
                x: 0,
                y: 0,
                width: 1024,
                height: 2
            }]
        ); // Sparse excessive fragmentation becomes one full rectangle.
    } // Coalescing consumers can take complete snapshots without a missing intermediate patch.
    #[test] // Fallible immutable-output staging occurs before canonical publication.
    fn transport_rejection_keeps_the_old_base_and_pixels() {
        // Reproduce quota/evidence refusal after rendering succeeded.
        let shape = shape(Format::Gray8, Update::Retain, false);
        let mut surface = Surface::new(1, 1, shape).unwrap();
        let (meta, mut planes) = start(&mut surface, 1);
        edit(&mut planes, shape, 0, [255.0; 4], 1, 1); // Valid rendered candidate.
        let result =
            surface.finish_with(meta, planes, &mut NoNativeRenderer, |candidate, outcome| {
                assert_eq!(candidate.states()[0], 2);
                assert_eq!(outcome.version, 1);
                Err(SurfaceError::Capacity)
            }); // Simulate actual output admission refusal.
        assert_eq!(result, Err(SurfaceError::Capacity));
        assert_eq!(surface.version(), 0);
        assert_eq!(surface.snapshot().states()[0], 1); // Neither pixels nor drawing base advanced.
        assert_eq!(std::mem::size_of::<Option<SourceToken>>(), 8); // The declared provenance-plane element size is explicit.
    } // The native engine acknowledges false and seeds the unchanged canonical base next time.
    #[test] // Retained native text is removed as a whole when either cell is overwritten.
    fn wide_glyph_overwrite_and_later_cell_order_are_atomic() {
        // Native supplies verified glyph width; this checks the transaction boundary.
        let shape = shape(Format::Gray8, Update::Retain, false);
        let mut snapshot = Snapshot::empty(shape).unwrap();
        let mut damage = vec![false; 6];
        let mut later = vec![0; 6]; // Two rows of three cells.
        let glyph = NativeText {
            x: 0,
            y: 0,
            text: "\u{754c}".to_owned(),
            width: 2,
            style: TextStyle {
                rgb: None,
                bold: false,
                italic: false,
                underline: false,
            },
        }; // Width is native fixture evidence, not a JS assertion.
        snapshot
            .put_text(glyph.clone(), 1, &later, &mut damage)
            .unwrap();
        assert_eq!(snapshot.text().count(), 1);
        damage.fill(false);
        snapshot.erase_text(1, &mut damage); // Overwrite the continuation cell.
        assert_eq!(snapshot.text().count(), 0);
        assert_eq!(damage, vec![true, true, false, false, false, false]); // Both cells are damaged and neither half remains.
        later[1] = 3;
        snapshot.put_text(glyph, 2, &later, &mut damage).unwrap();
        assert_eq!(snapshot.text().count(), 0); // A later scalar write suppresses an earlier wide glyph entirely.
    } // Final UI protection and emission masks still require native compositor tests.
    #[test]
    fn explicit_cell_colour_obeys_declared_colour_space() {
        for (colour_space, expected) in [(ColourSpace::Srgb, 128), (ColourSpace::Linear, 188)] {
            let mut shape = shape(Format::Mask8, Update::Retain, true);
            shape.colour_space = colour_space;
            let mut surface = Surface::new(1, 1, shape).unwrap();
            let (meta, mut planes) = start(&mut surface, 1);
            edit(&mut planes, shape, 0, [1.0, 0.0, 0.0, 0.0], 1, 1);
            planes.cell_rgb.as_mut().unwrap()[..3].fill(128);
            planes.colour_touch.as_mut().unwrap()[0] = 1;
            planes.colour_order.as_mut().unwrap()[0] = 1;
            surface.finish(meta, planes, &mut NoNativeRenderer).unwrap();
            let packed = surface
                .snapshot()
                .pack(|value, _, _| value, |_, _, _| true)
                .unwrap();
            assert_eq!(packed.rgb[0], Some([expected; 3]));
        }
    }
    #[test]
    fn native_colour_overpaint_removes_entire_retained_glyph() {
        struct ColourRenderer;
        impl NativeRenderer for ColourRenderer {
            fn render(&mut self, _: &Command, _: Shape, _: usize) -> Result<NativeOutput> {
                Ok(NativeOutput {
                    colours: vec![(1, 0, Some([255, 0, 0]))],
                    ..NativeOutput::default()
                })
            }
        }
        let shape = shape(Format::Gray8, Update::Retain, true);
        let mut surface = Surface::new(1, 1, shape).unwrap();
        let mut damage = vec![false; 6];
        surface
            .snapshot
            .put_text(
                NativeText {
                    x: 0,
                    y: 0,
                    text: "界".into(),
                    width: 2,
                    style: TextStyle {
                        rgb: None,
                        bold: false,
                        italic: false,
                        underline: false,
                    },
                },
                1,
                &[0; 6],
                &mut damage,
            )
            .unwrap();
        let (mut meta, planes) = start(&mut surface, 1);
        meta.commands.push(blit(1, 1, "fixture"));
        let outcome = surface.finish(meta, planes, &mut ColourRenderer).unwrap();
        assert_eq!(surface.snapshot().text().count(), 0);
        assert!(outcome
            .damage
            .iter()
            .any(|rect| rect.x == 0 && rect.width >= 2));
    }
} // End surface regression tests.
