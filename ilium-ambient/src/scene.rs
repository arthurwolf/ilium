//! The contract between a scene engine and its host (the ilium client).

use crate::gpu::GpuRunner;
use crate::location::GeoLocation;
use crate::minecraft::saved_runtime::SavedRuntime;
use crate::raster::{PaintedOwner, Raster};
use crate::registry::AmbientSettings;
use crate::style::ScenePalette;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Everything a scene may need besides its own settings.
#[derive(Clone)]
pub struct SceneEnv {
    /// Host-supplied shared finite and physical allocation admission.
    pub resources: crate::resources::AmbientResources,
    /// The shared observer position (stars, clouds, night lights).
    pub location: GeoLocation,
    /// Root for downloaded tiles, catalogs and thumbnails. Scenes create their
    /// own sub-directory below it. May not exist yet.
    pub cache_dir: PathBuf,
    /// The shared GPU device, when the host found a usable one. Scenes with a
    /// GPU renderer use it only when their setting asks for it.
    pub gpu: Option<Arc<dyn GpuRunner>>,
    /// One history drain fence shared by all scene generations of this host.
    pub saved_runtime: Arc<SavedRuntime>,
    /// The shared look's current palette. Every scene is built with it and
    /// receives later changes through `Scene::set_palette`. Scenes that draw
    /// natural colours should shift them with `ScenePalette::recolor`;
    /// monochrome scenes may ignore it (the host still recolours their ink).
    pub palette: ScenePalette,
}

impl std::fmt::Debug for SceneEnv {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SceneEnv")
            .field("location", &self.location)
            .field("cache_dir", &self.cache_dir)
            .field("saved_runtime", &"host-owned")
            .field(
                "gpu",
                &self.gpu.as_ref().map(|runner| runner.adapter_name()),
            )
            .finish()
    }
}

impl SceneEnv {
    /// Test/probe environment with a throw-away cache directory.
    pub fn for_test(cache_dir: PathBuf, resources: crate::resources::AmbientResources) -> Self {
        Self {
            resources,
            location: GeoLocation::default(),
            cache_dir,
            gpu: None,
            saved_runtime: Arc::new(SavedRuntime::new()),
            palette: ScenePalette::default(),
        }
    }
}

/// Which terminal cells of the host screen show something other than empty
/// space, rebuilt by the host from the live workspace for every frame request.
/// Scenes that interact with the screen (`Scene::wants_occupancy`) receive it
/// through `Scene::occupancy`; it never contains animation ink itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OccupancyMask {
    width: u16,
    height: u16,
    occupied: Vec<bool>,
    /// Content-free foreground anchors; forbidden cells are not necessarily text.
    characters: Vec<bool>,
}

impl OccupancyMask {
    /// An all-empty mask of the given size.
    pub fn empty(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            occupied: vec![false; usize::from(width) * usize::from(height)],
            characters: vec![false; usize::from(width) * usize::from(height)],
        }
    }

    /// Builds a mask by asking `is_occupied(column, row)` for every cell.
    pub fn from_fn(width: u16, height: u16, mut is_occupied: impl FnMut(u16, u16) -> bool) -> Self {
        let mut mask = Self::empty(width, height);
        for row in 0..height {
            for column in 0..width {
                if is_occupied(column, row) {
                    mask.set(column, row, true);
                }
            }
        }
        mask
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn set(&mut self, column: u16, row: u16, is_occupied: bool) {
        if column < self.width && row < self.height {
            let index = usize::from(row) * usize::from(self.width) + usize::from(column);
            self.occupied[index] = is_occupied;
            if !is_occupied {
                self.characters[index] = false;
            }
        }
    }

    /// Mark visible foreground without exposing its text. Anchors are always
    /// excluded from animation ink; removing an anchor does not relax exclusion.
    pub fn set_character(&mut self, column: u16, row: u16, is_character: bool) {
        if column < self.width && row < self.height {
            let index = usize::from(row) * usize::from(self.width) + usize::from(column);
            self.characters[index] = is_character;
            if is_character {
                self.occupied[index] = true;
            }
        }
    }

    /// Outside-screen walls never act as character-growth sources.
    pub fn is_character(&self, column: i32, row: i32) -> bool {
        if column < 0 || row < 0 || column >= i32::from(self.width) || row >= i32::from(self.height)
        {
            return false;
        }
        self.characters[row as usize * usize::from(self.width) + column as usize]
    }

    /// Cells outside the screen count as occupied: they are walls.
    pub fn is_occupied(&self, column: i32, row: i32) -> bool {
        if column < 0 || row < 0 || column >= i32::from(self.width) || row >= i32::from(self.height)
        {
            return true;
        }
        self.occupied[row as usize * usize::from(self.width) + column as usize]
    }

    /// Bytes retained, for admission accounting.
    pub fn retained_bytes(&self) -> usize {
        self.occupied.len().saturating_add(self.characters.len())
    }
}

#[cfg(test)]
mod character_occupancy_tests {
    use super::OccupancyMask;

    #[test]
    fn character_anchors_preserve_exclusion_and_distinguish_forbidden_cells() {
        let mut mask = OccupancyMask::from_fn(4, 2, |column, _| column == 0);
        assert!(mask.is_occupied(0, 0));
        assert!(!mask.is_character(0, 0));
        assert!(mask.is_occupied(-1, 0));
        assert!(!mask.is_character(-1, 0));
        mask.set_character(2, 1, true);
        assert!(mask.is_occupied(2, 1));
        assert!(mask.is_character(2, 1));
        let cloned = mask.clone();
        assert_eq!(cloned, mask);
        mask.set_character(2, 1, false);
        assert!(mask.is_occupied(2, 1));
        assert!(!mask.is_character(2, 1));
        assert_ne!(cloned, mask);
        mask.set_character(2, 1, true);
        mask.set(2, 1, false);
        assert!(!mask.is_occupied(2, 1));
        assert!(!mask.is_character(2, 1));
        assert_eq!(mask.retained_bytes(), 16);
    }
}

/// Applies the shared palette to a scene on its behalf: after each render the
/// scene's natural cell colours are shifted onto the palette by brightness.
/// `create_scene` wraps every scene in it, so every animation follows the
/// palette it was constructed with (`SceneEnv::palette`) and later changes
/// (`Scene::set_palette`). A scene that wants finer control (palette-aware
/// colour choices instead of a brightness remap) reads `SceneEnv::palette`
/// itself and may override `set_palette`; plugins will do exactly that.
pub struct PaletteScene {
    inner: Box<dyn Scene>,
    palette: ScenePalette,
}

impl PaletteScene {
    pub fn new(mut inner: Box<dyn Scene>, palette: ScenePalette) -> Self {
        inner.set_palette(&palette);
        Self { inner, palette }
    }
}

impl Scene for PaletteScene {
    fn saved_world_source(&self) -> Option<SavedWorldSource<'_>> {
        self.inner.saved_world_source()
    }

    fn readiness(&mut self) -> SceneReadiness {
        self.inner.readiness()
    }

    fn has_prepared_frame(&self) -> bool {
        self.inner.has_prepared_frame()
    }

    fn pointer(&mut self, position: Option<[f32; 2]>) {
        self.inner.pointer(position);
    }

    fn render(&mut self, frame: &mut Frame<'_>) {
        self.inner.render(frame);
        if self.palette.is_provided()
            && self.inner.uses_cell_colors()
            && !self.inner.follows_palette()
        {
            self.palette.recolor_cells(frame.cell_colors);
        }
    }

    fn presented(&mut self, owners: &[PaintedOwner]) {
        self.inner.presented(owners);
    }

    fn receipt_bytes(&self) -> usize {
        self.inner.receipt_bytes()
    }

    fn seal_frame(&mut self, id: FrameReceiptId) {
        self.inner.seal_frame(id);
    }

    fn presented_frame(&mut self, id: FrameReceiptId, owners: &[PaintedOwner]) {
        self.inner.presented_frame(id, owners);
    }

    fn native_glyph(&self, x: u16, y: u16) -> Option<char> {
        self.inner.native_glyph(x, y)
    }

    fn set_palette(&mut self, palette: &ScenePalette) {
        self.palette = palette.clone();
        self.inner.set_palette(palette);
    }

    fn wants_occupancy(&self) -> bool {
        self.inner.wants_occupancy()
    }

    fn occupancy(&mut self, mask: &OccupancyMask) {
        self.inner.occupancy(mask);
    }

    fn follows_palette(&self) -> bool {
        self.inner.follows_palette()
    }

    fn uses_cell_colors(&self) -> bool {
        self.inner.uses_cell_colors()
    }

    fn frames_per_second(&self) -> u32 {
        self.inner.frames_per_second()
    }

    fn reconfigure(&mut self, settings: &AmbientSettings) -> bool {
        self.inner.reconfigure(settings)
    }

    fn status(&self) -> Option<String> {
        self.inner.status()
    }
}

/// One frame request. The raster is already resized to `2 * width` by
/// `4 * height` dots (Braille sub-cells) and cleared to zero.
pub struct Frame<'a> {
    /// Dot intensities 0.0..=1.0. The host thresholds them with the user's
    /// density and dither settings to produce Braille.
    pub raster: &'a mut Raster,
    /// One RGB value per terminal cell, row-major, `width * height` long.
    /// Only read by the host when the scene reports `uses_cell_colors()`.
    pub cell_colors: &'a mut Vec<[u8; 3]>,
    pub width: u16,
    pub height: u16,
    /// Animation clock: wall time since the scene started, multiplied by the
    /// user's global Speed setting. Monotonic non-decreasing within a scene.
    pub time: Duration,
    /// Unscaled wall time since the scene started.
    pub wall: Duration,
    /// Current civil time, for scenes that show the real sky/weather "now".
    pub now: SystemTime,
}

/// The worker admits at most three snapshots. A slot cannot be reused while
/// its snapshot or presentation lease exists; the sequence prevents ABA after
/// reuse. Only this small identity crosses the terminal/UI boundary. Heavy
/// palette and owner tables remain in the hosted scene on the worker.
pub const MAX_SCENE_RECEIPT_SLOTS: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameReceiptId {
    slot: u8,
    sequence: u64,
}

impl FrameReceiptId {
    pub fn new(slot: u8, sequence: u64) -> Option<Self> {
        (slot < MAX_SCENE_RECEIPT_SLOTS && sequence != 0).then_some(Self { slot, sequence })
    }

    pub fn slot(self) -> usize {
        usize::from(self.slot)
    }

    pub fn sequence(self) -> u64 {
        self.sequence
    }
}

impl Frame<'_> {
    /// Set both the intensity of a Braille dot and nothing else.
    pub fn cell_color_mut(&mut self, cell_x: u16, cell_y: u16) -> Option<&mut [u8; 3]> {
        if cell_x >= self.width || cell_y >= self.height {
            return None;
        }
        self.cell_colors
            .get_mut(usize::from(cell_y) * usize::from(self.width) + usize::from(cell_x))
    }
}

/// Actual source preparation, independent of informational status messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneReadiness {
    Preparing,
    Ready,
    Unavailable(String),
}

/// Borrow the selected native source under its original lifetime. This is a
/// Rust host boundary, never a script-supplied source or capability.
#[derive(Clone, Copy)]
pub struct SavedWorldSource<'a> {
    pub map: &'a crate::minecraft::tours::PreparedMap,
    pub stop: &'a ilium_platform::owned_worker::StopToken,
}

/// A stateful scene. Created by the registry from settings; the host drops it
/// (which must stop every thread and child process it owns) whenever the
/// settings that affect it change or another scene is selected.
///
/// Rules:
/// * `render` must never block on I/O, a child process, a lock held by a
///   worker, or a sleep. Slow work belongs on an owned background thread
///   (see `source::Worker`); `render` shows the last good data and a
///   tasteful placeholder until data arrive.
/// * `render` must overwrite the whole raster (it arrives zeroed) and, when
///   `uses_cell_colors()`, every cell color.
/// * Time may only be read from `Frame`, never from the system clock, so
///   scenes are testable.
pub trait Scene: Send {
    /// Borrow an already selected saved source without opening paths or
    /// acquiring another source. Ordinary and generated scenes provide none.
    fn saved_world_source(&self) -> Option<SavedWorldSource<'_>> {
        None
    }

    /// Poll already owned preparation state without I/O, blocking or starting
    /// work. A ready source may still be preparing its first viewport.
    fn readiness(&mut self) -> SceneReadiness {
        SceneReadiness::Ready
    }

    /// Whether the last render produced a real prepared frame and receipt.
    /// Saved sources must not publish their initial placeholder as ready ink.
    fn has_prepared_frame(&self) -> bool {
        true
    }

    /// Latest pointer position in normalized screen coordinates. Optional input
    /// does not consume terminal/UI mouse events and must never block.
    fn pointer(&mut self, _position: Option<[f32; 2]>) {}

    fn render(&mut self, frame: &mut Frame<'_>);

    /// The host calls this for each successful terminal draw of this scene,
    /// including redraws that reused the cached raster. Scenes map frame-local
    /// owner ids to retained source states and reject stale frame/source tags.
    /// The default keeps ordinary scenes free of receipt bookkeeping.
    fn presented(&mut self, _owners: &[PaintedOwner]) {}

    /// Conservative bytes retained by the current raster's presentation
    /// receipt. The worker adds this to MAX_FRAME_BYTES before publication;
    /// the scene also reserves its allocations in its own ByteBudget.
    fn receipt_bytes(&self) -> usize {
        0
    }

    /// Called only after snapshot admission and before publication. The
    /// worker owns all slot tables and drops replaced heavy receipts itself.
    fn seal_frame(&mut self, _id: FrameReceiptId) {}

    /// Called after an actual terminal emission, with exactly the slot and
    /// sequence retained by that emitted snapshot. Ordinary scenes preserve
    /// their existing presentation callback without receipt bookkeeping.
    fn presented_frame(&mut self, _id: FrameReceiptId, owners: &[PaintedOwner]) {
        self.presented(owners);
    }

    /// Optional single-cell text above Braille ink (Pi text mode and live
    /// map labels). Hosts call this after rendering; it must be bounded,
    /// nonblocking, and return only characters of one terminal cell width.
    fn native_glyph(&self, _x: u16, _y: u16) -> Option<char> {
        None
    }

    /// The shared palette changed while the scene runs. Scenes with natural
    /// colours store it and use `ScenePalette::recolor` from the next frame;
    /// the default ignores it. Never rebuild the scene for this.
    fn set_palette(&mut self, _palette: &ScenePalette) {}

    /// True when the scene reacts to what the workspace draws. The host then
    /// builds an `OccupancyMask` of the screen before every frame request and
    /// delivers it through `occupancy`; other scenes cost nothing.
    fn wants_occupancy(&self) -> bool {
        false
    }

    /// The latest screen occupancy, delivered before `render`. The mask is the
    /// whole field (`width * height` cells); it changes when the workspace does.
    fn occupancy(&mut self, _mask: &OccupancyMask) {}

    /// True when the scene applies `SceneEnv::palette` / `set_palette` to its
    /// own colours (palette-aware choices at the source). `PaletteScene` then
    /// skips its generic brightness remap for this scene.
    fn follows_palette(&self) -> bool {
        false
    }

    /// True when the scene supplies `Frame::cell_colors`; otherwise the
    /// host paints every dot in the user's global palette.
    fn uses_cell_colors(&self) -> bool {
        false
    }

    /// Requested redraw cadence, 1..=30.
    fn frames_per_second(&self) -> u32 {
        12
    }

    /// Take changed settings without being rebuilt, keeping the scene's own
    /// state: a running game keeps playing while its colours are adjusted.
    /// Return `true` when the settings were applied; the default `false`
    /// makes the host drop this scene and build a fresh one.
    fn reconfigure(&mut self, _settings: &AmbientSettings) -> bool {
        false
    }

    /// One short status line for the Settings panel ("Downloading tiles 40%",
    /// "ffmpeg not found", "No audio input"). `None` when all is well.
    fn status(&self) -> Option<String> {
        None
    }
}

/// A scene that draws nothing and says why. Registry stub and failure fallback.
pub struct MessageScene(pub String);

impl Scene for MessageScene {
    fn readiness(&mut self) -> SceneReadiness {
        SceneReadiness::Unavailable(self.0.clone())
    }

    fn has_prepared_frame(&self) -> bool {
        false
    }

    fn render(&mut self, _frame: &mut Frame<'_>) {}

    fn status(&self) -> Option<String> {
        Some(self.0.clone())
    }
}

#[cfg(test)]
mod palette_scene_tests {
    use super::*;

    struct Flat;
    impl Scene for Flat {
        fn render(&mut self, frame: &mut Frame<'_>) {
            frame.cell_colors.clear();
            frame.cell_colors.resize(4, [200, 40, 40]);
        }
        fn uses_cell_colors(&self) -> bool {
            true
        }
    }

    fn render(scene: &mut dyn Scene) -> Vec<[u8; 3]> {
        let mut raster = Raster::default();
        raster.resize(4, 4);
        let mut colors = Vec::new();
        scene.render(&mut Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width: 2,
            height: 2,
            time: Duration::ZERO,
            wall: Duration::ZERO,
            now: SystemTime::UNIX_EPOCH,
        });
        colors
    }

    #[test]
    fn palette_wrapper_preserves_unavailable_source_and_missing_frame() {
        let mut scene = PaletteScene::new(
            Box::new(MessageScene("source qualification failed".into())),
            ScenePalette::default(),
        );
        assert_eq!(
            scene.readiness(),
            SceneReadiness::Unavailable("source qualification failed".into())
        );
        assert!(!scene.has_prepared_frame());
        assert!(scene.saved_world_source().is_none());
    }

    #[test]
    fn ordinary_scene_is_ready_without_a_preparation_owner() {
        let mut scene = PaletteScene::new(Box::new(Flat), ScenePalette::default());
        assert_eq!(scene.readiness(), SceneReadiness::Ready);
        assert!(scene.has_prepared_frame());
        assert!(scene.saved_world_source().is_none());
    }

    #[test]
    fn palette_wrapper_preserves_pending_source_without_a_prepared_frame() {
        struct PreparingScene;
        impl Scene for PreparingScene {
            fn render(&mut self, _frame: &mut Frame<'_>) {}
            fn readiness(&mut self) -> SceneReadiness {
                SceneReadiness::Preparing
            }
            fn has_prepared_frame(&self) -> bool {
                false
            }
        }
        let mut scene = PaletteScene::new(Box::new(PreparingScene), ScenePalette::default());
        assert_eq!(scene.readiness(), SceneReadiness::Preparing);
        assert!(!scene.has_prepared_frame());
    }

    #[test]
    fn palette_scene_recolors_only_when_a_palette_is_provided() {
        let mut scene = PaletteScene::new(Box::new(Flat), ScenePalette::default());
        assert_eq!(render(&mut scene)[0], [200, 40, 40]);
        scene.set_palette(&ScenePalette {
            stops: vec![[0, 0, 40], [0, 200, 255]],
            ..Default::default()
        });
        assert_ne!(render(&mut scene)[0], [200, 40, 40]);
    }
}
