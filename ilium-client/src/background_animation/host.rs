//! Owns the one live `ilium-ambient` scene shared by the ambient background
//! and the Settings preview.
//!
//! The scene is built lazily for the current settings key, rebuilt when that
//! key changes and dropped (which, by the `Scene` contract, stops every thread
//! and child process it owns) whenever nothing shows it. A scene that panics
//! is replaced by a message scene so the client keeps running.

use ilium_ambient::raster::PaintedOwner;
use ilium_ambient::{AmbientKind, AmbientSettings, Frame, MessageScene, Scene, SceneEnv};
use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};
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
    scene: Option<HostedScene>,
    factory: SceneFactory,
    /// Increments on every scene construction; render caches key on it.
    generation: u64,
    last_wall: Duration,
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
            scene: None,
            factory: Box::new(|kind, settings, env| settings.create_scene(kind, env)),
            generation: 0,
            last_wall: Duration::ZERO,
        }
    }
}

impl AmbientHost {
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
        let env = SceneEnv {
            location: normalized.location.clone(),
            cache_dir: ilium_ambient::source::default_cache_dir(),
            gpu: ilium_gpu::runner(),
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
