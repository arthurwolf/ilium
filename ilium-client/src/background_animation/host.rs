//! Owns the one live `ilium-ambient` scene shared by the ambient background
//! and the Settings preview.
//!
//! The scene is built lazily for the current settings key, rebuilt when that
//! key changes and dropped (which, by the `Scene` contract, stops every thread
//! and child process it owns) whenever nothing shows it. A scene that panics
//! is replaced by a message scene so the client keeps running.

use ilium_ambient::minecraft::saved_runtime::SavedRuntime;
use ilium_ambient::raster::PaintedOwner;
use ilium_ambient::scene::FrameReceiptId;
use ilium_ambient::style::ScenePalette;
use ilium_ambient::{AmbientKind, AmbientSettings, Frame, MessageScene, Scene, SceneEnv};
use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::Duration;

/// Builds a scene for one kind. The default asks the crate's registry; unit
/// tests substitute a fake so they need no network, ffmpeg or audio device.
pub type SceneFactory =
    Box<dyn Fn(AmbientKind, &AmbientSettings, &SceneEnv) -> Box<dyn Scene> + Send>;

struct HostedScene {
    kind: AmbientKind,
    key: String,
    scene: Box<dyn Scene>,
    /// Session-clock value at construction: scene time starts at zero.
    built_at: Duration,
}

pub struct AmbientHost {
    resources: Option<ilium_ambient::resources::AmbientResources>,
    scene: Option<HostedScene>,
    factory: SceneFactory,
    /// Increments on every scene construction; render caches key on it.
    generation: u64,
    last_wall: Duration,
    /// Retained after release/rebuild while accepted history writes drain.
    saved_runtime: Arc<SavedRuntime>,
    /// The shared look's palette, handed to every scene at construction and
    /// pushed to the running scene when it changes.
    palette: ScenePalette,
}

impl std::fmt::Debug for AmbientHost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AmbientHost")
            .field("key", &self.scene.as_ref().map(|hosted| &hosted.key))
            .field("generation", &self.generation)
            .finish()
    }
}

impl Default for AmbientHost {
    fn default() -> Self {
        Self {
            resources: {
                #[cfg(test)]
                {
                    Some(ilium_ambient::resources::AmbientResources::new(
                        crate::execution::test_client(),
                    ))
                }
                #[cfg(not(test))]
                {
                    None
                }
            },
            scene: None,
            factory: Box::new(|kind, settings, env| settings.create_scene(kind, env)),
            generation: 0,
            last_wall: Duration::ZERO,
            saved_runtime: Arc::new(SavedRuntime::new()),
            palette: ScenePalette::default(),
        }
    }
}

impl AmbientHost {
    pub fn configure_resources(&mut self, resources: ilium_ambient::resources::AmbientResources) {
        self.resources = Some(resources);
    }
    pub fn resources(&self) -> Option<&ilium_ambient::resources::AmbientResources> {
        self.resources.as_ref()
    }

    /// Shared saved-world history/runtime authority used by both native
    /// scenes and an admitted live animation package.  The returned Arc is
    /// the existing host-owned runtime; callers cannot construct a substitute
    /// world history namespace from JavaScript metadata.
    pub fn saved_runtime(&self) -> Arc<SavedRuntime> {
        Arc::clone(&self.saved_runtime)
    }

    /// Hands the shared look's palette to the hosted scene. A change never
    /// rebuilds the scene; it moves the generation on so the next frame is
    /// rendered again with the new colours.
    pub fn set_palette(&mut self, palette: ScenePalette) {
        if self.palette == palette {
            return;
        }
        if let Some(hosted) = self.scene.as_mut() {
            let scene = &mut hosted.scene;
            // A panicking scene is handled like in `sync`: ignore this update.
            let _ = catch_unwind(AssertUnwindSafe(|| scene.set_palette(&palette)));
            self.generation += 1;
        }
        self.palette = palette;
    }

    /// A host that builds scenes with `factory` instead of the crate registry.
    #[cfg(test)]
    pub fn with_factory(factory: SceneFactory) -> Self {
        Self {
            factory,
            ..Self::default()
        }
    }

    /// Ensures the hosted scene matches `kind` and `settings`, building or
    /// rebuilding it when the crate's scene key differs, and returns the
    /// scene generation. `elapsed` is the session clock at this frame.
    pub fn sync(
        &mut self,
        kind: AmbientKind,
        settings: &AmbientSettings,
        elapsed: Duration,
    ) -> u64 {
        let key = settings.scene_key(kind);
        if let Some(hosted) = self.scene.as_mut() {
            if hosted.key == key {
                return self.generation;
            }
            // A scene may take changed settings in place (a running game
            // keeps playing while its colours change). The generation still
            // moves on so the render of the old settings is not reused.
            if hosted.kind == kind {
                let normalized = settings.normalized();
                let applied =
                    catch_unwind(AssertUnwindSafe(|| hosted.scene.reconfigure(&normalized)))
                        .unwrap_or(false);
                if applied {
                    hosted.key = key;
                    self.generation += 1;
                    return self.generation;
                }
            }
        }
        // Drop the old scene first so its workers stop before the new ones start.
        self.scene = None;
        let normalized = settings.normalized();
        let Some(resources) = self.resources.clone() else {
            self.generation += 1;
            self.scene = Some(HostedScene {
                kind,
                key,
                scene: Box::new(MessageScene(
                    ilium_ambient::resources::MissingResources.to_string(),
                )),
                built_at: elapsed,
            });
            return self.generation;
        };
        let env = SceneEnv {
            resources,
            location: normalized.location.clone(),
            cache_dir: ilium_ambient::source::default_cache_dir(),
            gpu: ilium_gpu::runner(),
            saved_runtime: Arc::clone(&self.saved_runtime),
            palette: self.palette.clone(),
        };
        let scene = catch_unwind(AssertUnwindSafe(|| (self.factory)(kind, &normalized, &env)))
            .unwrap_or_else(|payload| {
                Box::new(MessageScene(format!(
                    "Scene failed to start: {}",
                    panic_message(payload.as_ref())
                )))
            });
        self.generation += 1;
        self.last_wall = Duration::ZERO;
        self.scene = Some(HostedScene {
            kind,
            key,
            scene,
            built_at: elapsed,
        });
        self.generation
    }

    /// Stops and drops the hosted scene, if any.
    pub fn release(&mut self) {
        self.scene = None;
    }

    pub fn is_hosted(&self) -> bool {
        self.scene.is_some()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Unscaled scene time for the session-clock value `elapsed`. Never runs
    /// backwards, even when the cadence (and so the quantization) changes.
    pub fn wall(&mut self, elapsed: Duration) -> Duration {
        let built_at = self
            .scene
            .as_ref()
            .map_or(Duration::ZERO, |hosted| hosted.built_at);
        self.last_wall = self.last_wall.max(elapsed.saturating_sub(built_at));
        self.last_wall
    }

    /// Renders the hosted scene, replacing it with a message scene if it panics.
    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let Some(hosted) = self.scene.as_mut() else {
            return;
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| hosted.scene.render(frame)));
        if let Err(payload) = outcome {
            let message = panic_message(payload.as_ref());
            tracing::error!(scene = %hosted.key, %message, "ambient scene panicked; showing a message instead");
            hosted.scene = Box::new(MessageScene(format!("Scene failed: {message}")));
            frame.raster.dots.fill(0.0);
            frame.raster.owner_ids.fill(0);
            frame.cell_colors.fill([0; 3]);
        }
    }

    /// Conservative scene receipt bytes participate in the worker's frame
    /// admission. A panicking scene cannot publish an unaccounted snapshot.
    pub fn receipt_bytes(&self) -> usize {
        self.scene.as_ref().map_or(0, |hosted| {
            catch_unwind(AssertUnwindSafe(|| hosted.scene.receipt_bytes())).unwrap_or(usize::MAX)
        })
    }

    /// Seal one admitted snapshot into a worker-owned slot before publishing
    /// it to the UI. The UI carries only this identity, never the world Arc.
    pub fn seal_frame(&mut self, generation: u64, id: FrameReceiptId) -> bool {
        if self.generation != generation {
            return false;
        }
        let Some(hosted) = self.scene.as_mut() else {
            return false;
        };
        match catch_unwind(AssertUnwindSafe(|| hosted.scene.seal_frame(id))) {
            Ok(()) => true,
            Err(payload) => {
                let message = panic_message(payload.as_ref());
                tracing::error!(scene = %hosted.key, %message, "ambient frame seal failed");
                hosted.scene = Box::new(MessageScene(format!("Scene failed: {message}")));
                false
            }
        }
    }

    /// Final painted owners are interpreted only by the matching snapshot's
    /// sealed frame-local table after the hosted-generation fence succeeds.
    pub fn presented_frame(
        &mut self,
        generation: u64,
        id: FrameReceiptId,
        owners: &[PaintedOwner],
    ) {
        if self.generation != generation {
            return;
        }
        let Some(hosted) = self.scene.as_mut() else {
            return;
        };
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
            hosted.scene.presented_frame(id, owners)
        })) {
            let message = panic_message(payload.as_ref());
            tracing::error!(scene = %hosted.key, %message, "ambient frame receipt failed");
            hosted.scene = Box::new(MessageScene(format!("Scene failed: {message}")));
        }
    }

    /// Only the matching hosted generation may receive a final paint receipt.
    /// The scene checks its own frame/source tag before crediting history.
    pub fn presented(&mut self, generation: u64, owners: &[PaintedOwner]) {
        if self.generation != generation {
            return;
        }
        let Some(hosted) = self.scene.as_mut() else {
            return;
        };
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| hosted.scene.presented(owners))) {
            let message = panic_message(payload.as_ref());
            tracing::error!(scene = %hosted.key, %message, "ambient presentation handler panicked");
            hosted.scene = Box::new(MessageScene(format!("Scene failed: {message}")));
        }
    }

    /// Input for pointer-aware scenes; the scene owns coordinate interpretation.
    pub fn pointer(&mut self, position: Option<[f32; 2]>) {
        if let Some(hosted) = self.scene.as_mut() {
            let outcome = catch_unwind(AssertUnwindSafe(|| hosted.scene.pointer(position)));
            if let Err(payload) = outcome {
                tracing::error!(message = %panic_message(payload.as_ref()), "ambient pointer handler panicked");
            }
        }
    }

    /// True when the hosted scene reacts to what the workspace draws.
    pub fn wants_occupancy(&self) -> bool {
        self.scene
            .as_ref()
            .is_some_and(|hosted| hosted.scene.wants_occupancy())
    }

    /// Delivers the latest screen occupancy to a scene that asked for it.
    pub fn occupancy(&mut self, mask: &ilium_ambient::OccupancyMask) {
        if let Some(hosted) = self.scene.as_mut() {
            if !hosted.scene.wants_occupancy() {
                return;
            }
            let outcome = catch_unwind(AssertUnwindSafe(|| hosted.scene.occupancy(mask)));
            if let Err(payload) = outcome {
                tracing::error!(message = %panic_message(payload.as_ref()), "ambient occupancy handler panicked");
            }
        }
    }

    pub fn uses_cell_colors(&self) -> bool {
        self.scene
            .as_ref()
            .is_some_and(|hosted| hosted.scene.uses_cell_colors())
    }

    /// The hosted scene's requested cadence, clamped to 1..=30.
    pub fn frames_per_second(&self) -> Option<u32> {
        self.scene
            .as_ref()
            .map(|hosted| hosted.scene.frames_per_second().clamp(1, 30))
    }

    /// Single-cell labels supplied by the current hosted scene.
    pub fn native_glyph(&self, x: u16, y: u16) -> Option<char> {
        self.scene
            .as_ref()
            .and_then(|hosted| hosted.scene.native_glyph(x, y))
    }

    pub fn status(&self) -> Option<String> {
        self.scene.as_ref().and_then(|hosted| hosted.scene.status())
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_owned())
}

#[cfg(test)]
mod saved_runtime_tests {
    use super::*;
    use ilium_ambient::minecraft::saved_runtime::SavedRuntime;
    use std::sync::{Arc, Mutex};

    #[test]
    fn one_writer_fence_survives_release_and_scene_rebuild() {
        let seen = Arc::new(Mutex::new(Vec::<Arc<SavedRuntime>>::new()));
        let recorded = Arc::clone(&seen);
        let mut host = AmbientHost::with_factory(Box::new(move |_, _, env| {
            recorded
                .lock()
                .unwrap()
                .push(Arc::clone(&env.saved_runtime));
            Box::new(MessageScene("synthetic saved host scene".into()))
        }));
        let settings = AmbientSettings::default();
        host.sync(AmbientKind::VoxelLandscape, &settings, Duration::ZERO);
        host.release();
        host.sync(
            AmbientKind::VoxelLandscape,
            &settings,
            Duration::from_secs(1),
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(Arc::ptr_eq(&seen[0], &seen[1]));
    }
}
