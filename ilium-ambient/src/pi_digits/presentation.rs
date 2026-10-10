//! Exact Pi in native text or coverage rasterized from the bundled real font.
use super::digits;
use crate::control::{Control, ControlValue, SceneSettings};
use crate::resources::{AmbientResources, WorkerCost};
use crate::scene::{Frame, Scene, SceneEnv};
use crate::source::Worker;
use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Wrap};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
const FONT: &[u8] = include_bytes!("../../assets/fonts/CascadiaCode-Regular.otf");
const MAX_CELLS: usize = 131_072;
const PI_PREPARATION_WORKER_BYTES: usize = 16 * 1024 * 1024;
const PI_PREPARATION_RETRY_BACKOFF: Duration = Duration::from_secs(5);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PiSettings {
    /// 0 is native terminal text, 1 is true-font Braille.
    pub mode: usize,
    pub digits_limit: i32,
    /// Height in Braille dots; native text follows the terminal's font size.
    pub font_size: i32,
    /// Decimal characters per second, divided by ten.
    pub scroll_speed: i32,
    pub hue_degrees: i32,
    pub brightness_percent: i32,
    pub digit_colors: bool,
    pub digit_hues: [i32; 10],
}
impl Default for PiSettings {
    fn default() -> Self {
        Self {
            mode: 0,
            digits_limit: 4096,
            font_size: 20,
            scroll_speed: 10,
            hue_degrees: 180,
            brightness_percent: 65,
            digit_colors: false,
            digit_hues: [0, 36, 72, 108, 144, 180, 216, 252, 288, 324],
        }
    }
}
impl SceneSettings for PiSettings {
    fn normalized(&self) -> Self {
        let mut next = self.clone();
        next.mode = next.mode.min(1);
        next.digits_limit = next.digits_limit.clamp(1, digits::MAX_DIGITS as i32);
        next.font_size = next.font_size.clamp(8, 48);
        next.scroll_speed = next.scroll_speed.clamp(0, 2000);
        next.hue_degrees = next.hue_degrees.clamp(0, 360);
        next.brightness_percent = next.brightness_percent.clamp(0, 100);
        next.digit_hues
            .iter_mut()
            .for_each(|hue| *hue = (*hue).clamp(0, 360));
        next
    }
    fn controls(&self) -> Vec<Control> {
        let mut rows = vec![
            Control::choice(
                "mode",
                "Pi rendering",
                self.mode,
                &["Native text", "Font Braille"],
                "Native text uses the terminal font. Font Braille rasterizes the bundled Cascadia Code on an owned worker.",
            ),
            Control::slider(
                "digits_limit",
                "Exact digits",
                self.digits_limit,
                (1, 20000, 100),
                "",
                "Number of exact decimal digits, including the initial 3. The decimal separator is added for display; the sequence repeats after this prefix.",
            ),
            Control::slider(
                "scroll_speed",
                "Scroll speed",
                self.scroll_speed,
                (0, 2000, 10),
                " ×0.1 chars/s",
                "Move through the exact digit sequence. Zero holds the prefix; global Speed also applies.",
            ),
            Control::slider(
                "hue_degrees",
                "Pi hue",
                self.hue_degrees,
                (0, 360, 5),
                "°",
                "Common color of decimal digits when digit colors are disabled.",
            ),
            Control::slider(
                "brightness_percent",
                "Pi brightness",
                self.brightness_percent,
                (0, 100, 5),
                "%",
                "Zero hides both native and Braille text. Color brightness and dot coverage are independent.",
            ),
            Control::toggle(
                "digit_colors",
                "Colors by digit",
                self.digit_colors,
                "Assign a separate hue to each decimal digit. The decimal separator keeps the common hue.",
            ),
        ];
        if self.mode == 1 {
            rows.insert(1,Control::slider("font_size","Font size",self.font_size,(8,48,2)," dots","Real-font height in Braille raster dots. Native text uses the terminal emulator's configured size."));
        }
        if self.digit_colors {
            for (digit, hue) in self.digit_hues.iter().enumerate() {
                rows.push(Control::slider(
                    DIGIT_IDS[digit],
                    DIGIT_LABELS[digit],
                    *hue,
                    (0, 360, 5),
                    "°",
                    "Hue assigned to this exact decimal digit.",
                ));
            }
        }
        rows
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let mut next = self.clone();
        match id {
            "mode" => {
                let Some(value) = crate::control::index(&value) else {
                    return Ok(false);
                };
                next.mode = value;
            }
            "digit_colors" => {
                let Some(value) = crate::control::boolean(&value) else {
                    return Ok(false);
                };
                next.digit_colors = value;
            }
            _ => {
                let Some(value) = crate::control::number(&value) else {
                    return Ok(false);
                };
                match id {
                    "digits_limit" => next.digits_limit = value,
                    "font_size" => next.font_size = value,
                    "scroll_speed" => next.scroll_speed = value,
                    "hue_degrees" => next.hue_degrees = value,
                    "brightness_percent" => next.brightness_percent = value,
                    _ => {
                        let Some(digit) = DIGIT_IDS.iter().position(|candidate| *candidate == id)
                        else {
                            return Ok(false);
                        };
                        next.digit_hues[digit] = value;
                    }
                }
            }
        }
        next = next.normalized();
        let changed = next != *self;
        *self = next;
        Ok(changed)
    }
}
const DIGIT_IDS: [&str; 10] = [
    "digit_0_hue",
    "digit_1_hue",
    "digit_2_hue",
    "digit_3_hue",
    "digit_4_hue",
    "digit_5_hue",
    "digit_6_hue",
    "digit_7_hue",
    "digit_8_hue",
    "digit_9_hue",
];
const DIGIT_LABELS: [&str; 10] = [
    "Digit 0 hue",
    "Digit 1 hue",
    "Digit 2 hue",
    "Digit 3 hue",
    "Digit 4 hue",
    "Digit 5 hue",
    "Digit 6 hue",
    "Digit 7 hue",
    "Digit 8 hue",
    "Digit 9 hue",
];
struct Atlas {
    width: usize,
    height: usize,
    glyphs: Vec<Vec<f32>>,
}
struct Prepared {
    digits: Arc<DigitPrefix>,
    atlas: Atlas,
    _atlas_storage: Arc<ilium_execution::StorageAdmission>,
}
struct DigitPrefix {
    text: String,
    _storage: Arc<ilium_execution::StorageAdmission>,
}
fn build_atlas(size: i32) -> Result<Atlas, String> {
    let size = size.clamp(8, 48) as f32;
    let mut database = cosmic_text::fontdb::Database::new();
    database.load_font_data(FONT.to_vec());
    let family = database
        .faces()
        .next()
        .and_then(|face| face.families.first())
        .map(|(name, _)| name.clone())
        .ok_or("Bundled Pi font has no face")?;
    let mut fonts = FontSystem::new_with_locale_and_db("en-US".into(), database);
    let mut cache = SwashCache::new();
    let width = ((size * 0.75).ceil() as usize + 2).div_ceil(2) * 2;
    let height = ((size * 1.4).ceil() as usize).div_ceil(4) * 4;
    let mut glyphs = vec![];
    for character in "0123456789.".chars() {
        let mut buffer = Buffer::new(&mut fonts, Metrics::new(size, height as f32));
        buffer.set_size(&mut fonts, Some(width as f32), Some(height as f32));
        buffer.set_wrap(&mut fonts, Wrap::None);
        buffer.set_text(
            &mut fonts,
            &character.to_string(),
            &Attrs::new().family(Family::Name(&family)),
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut fonts, false);
        let mut ink = vec![0.0f32; width * height];
        buffer.draw(
            &mut fonts,
            &mut cache,
            Color::rgb(255, 255, 255),
            |x, y, _, _, color| {
                if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
                    let sample = &mut ink[y as usize * width + x as usize];
                    *sample = sample.max(f32::from(color.a()) / 255.0);
                }
            },
        );
        if !ink.iter().any(|value| *value > 0.0) {
            return Err(format!("Bundled font produced no ink for {character}"));
        }
        glyphs.push(ink);
    }
    Ok(Atlas {
        width,
        height,
        glyphs,
    })
}
/// The scene retains its own exact prefix across font-only rebuilds; the
/// bounded spigot computation runs only on its admitted worker.
fn atlas_storage_bytes(size: i32) -> Result<usize, String> {
    let size = size.clamp(8, 48) as f32;
    let width = ((size * 0.75).ceil() as usize + 2).div_ceil(2) * 2;
    let height = ((size * 1.4).ceil() as usize).div_ceil(4) * 4;
    width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(11))
        .and_then(|samples| samples.checked_mul(std::mem::size_of::<f32>()))
        .and_then(|bytes| bytes.checked_add(16 * std::mem::size_of::<Vec<f32>>()))
        .ok_or_else(|| "Pi atlas storage size overflow".to_owned())
}

fn exact_digits(
    count: usize,
    stop: &AtomicBool,
    resources: &AmbientResources,
    cached: Option<Arc<DigitPrefix>>,
) -> Result<Arc<DigitPrefix>, String> {
    if stop.load(Ordering::Relaxed) {
        return Err("Pi preparation cancelled".into());
    }
    if let Some(cached) = cached.filter(|digits| digits.text.len() >= count) {
        return Ok(cached);
    }
    let storage = resources
        .reserve_storage(count.saturating_add(std::mem::size_of::<DigitPrefix>()))
        .map_err(|reason| format!("Pi digit storage admission refused: {reason:?}"))?;
    let text = digits::generate(count)?;
    Ok(Arc::new(DigitPrefix {
        text,
        _storage: storage,
    }))
}

pub struct PiScene {
    settings: PiSettings,
    resources: AmbientResources,
    worker: Option<Worker>,
    receiver: Option<mpsc::Receiver<Result<Prepared, String>>>,
    prepared: Option<Prepared>,
    digit_cache: Option<Arc<DigitPrefix>>,
    native: Vec<Option<char>>,
    native_width: usize,
    status: Option<String>,
    retry_after: Option<Instant>,
}
impl PiScene {
    // PALETTE (native Scene contract): `env.palette` is the shared look's current
    // palette. Custom native Scene implementations receive the current palette
    // and MUST follow it. Scenes with natural colours shift them onto it
    // (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. Monochrome scenes may ignore it. Today `PaletteScene` (scene.rs),
    // which `create_scene` wraps around every scene, shifts this scene's cell
    // colours onto the palette by brightness.
    pub fn new(settings: PiSettings, environment: &SceneEnv) -> Self {
        let mut scene = Self {
            settings: settings.normalized(),
            resources: environment.resources.clone(),
            worker: None,
            receiver: None,
            prepared: None,
            digit_cache: None,
            native: vec![],
            native_width: 0,
            status: None,
            retry_after: None,
        };
        scene.start();
        scene
    }
    fn start(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        }
        let reservation = match self.resources.reserve_worker(WorkerCost {
            threads: 1,
            resident_bytes: PI_PREPARATION_WORKER_BYTES,
        }) {
            Ok(reservation) => reservation,
            Err(error) => {
                self.status = Some(format!("Pi worker admission refused: {error:?}"));
                self.receiver = None;
                self.retry_after = Some(Instant::now() + PI_PREPARATION_RETRY_BACKOFF);
                return;
            }
        };
        let settings = self.settings.clone();
        let resources = self.resources.clone();
        let cached_digits = self.digit_cache.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        self.receiver = Some(receiver);
        self.status = Some("Preparing exact Pi digits and bundled font".into());
        match Worker::start_admitted("pi-font", reservation, move |stop| {
            let result = (|| {
                let digits = exact_digits(
                    settings.digits_limit as usize,
                    &stop,
                    &resources,
                    cached_digits,
                )?;
                if stop.load(Ordering::Relaxed) {
                    return Err("Pi preparation cancelled".into());
                }
                let atlas_storage = resources
                    .reserve_storage(atlas_storage_bytes(settings.font_size)?)
                    .map_err(|reason| format!("Pi atlas storage admission refused: {reason:?}"))?;
                let atlas = build_atlas(settings.font_size)?;
                Ok(Prepared {
                    digits,
                    atlas,
                    _atlas_storage: atlas_storage,
                })
            })();
            if !stop.load(Ordering::Relaxed) {
                let _ = sender.try_send(result);
            }
        }) {
            Ok(worker) => {
                self.worker = Some(worker);
                self.retry_after = None;
            }
            Err(error) => {
                self.status = Some(format!("Pi worker failed: {error}"));
                self.receiver = None;
                self.retry_after = Some(Instant::now() + PI_PREPARATION_RETRY_BACKOFF);
            }
        }
    }
    fn retry_preparation_if_due(&mut self) {
        if self
            .retry_after
            .is_some_and(|retry_after| Instant::now() >= retry_after)
        {
            self.start();
        }
    }
    pub fn apply_settings(&mut self, settings: PiSettings) {
        let settings = settings.normalized();
        let restart = self.settings.font_size != settings.font_size
            || self.settings.digits_limit != settings.digits_limit;
        self.settings = settings;
        self.native.fill(None);
        if restart {
            self.start();
        }
    }
    fn receive(&mut self) {
        let Some(receiver) = &self.receiver else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(prepared)) => {
                self.digit_cache = Some(Arc::clone(&prepared.digits));
                self.prepared = Some(prepared);
                self.status = None;
                self.receiver = None;
                self.retry_after = None;
            }
            Ok(Err(error)) => {
                self.status = Some(error);
                self.receiver = None;
                self.retry_after = Some(Instant::now() + PI_PREPARATION_RETRY_BACKOFF);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status = Some("Pi preparation ended without a result".into());
                self.receiver = None;
                self.retry_after = Some(Instant::now() + PI_PREPARATION_RETRY_BACKOFF);
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
    pub fn native_glyph(&self, x: u16, y: u16) -> Option<char> {
        if usize::from(x) >= self.native_width {
            return None;
        }
        self.native
            .get(usize::from(y) * self.native_width + usize::from(x))
            .copied()
            .flatten()
    }
    #[cfg(test)]
    fn prepared(settings: PiSettings, prepared: Prepared) -> Self {
        let digit_cache = Some(Arc::clone(&prepared.digits));
        Self {
            settings: settings.normalized(),
            resources: crate::resources::test_resources(),
            worker: None,
            receiver: None,
            prepared: Some(prepared),
            digit_cache,
            native: vec![],
            native_width: 0,
            status: None,
            retry_after: None,
        }
    }
}
impl Drop for PiScene {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop_in_background();
        }
    }
}
fn character_at(digits: &str, limit: usize, index: usize) -> char {
    let count = limit.min(digits.len());
    if count == 0 {
        return ' ';
    }
    let index = index % (count + 1);
    if index == 1 {
        '.'
    } else {
        char::from(digits.as_bytes()[index.saturating_sub(1)])
    }
}
fn digit_color(settings: &PiSettings, character: char) -> [u8; 3] {
    let hue = if settings.digit_colors {
        character
            .to_digit(10)
            .map_or(settings.hue_degrees, |digit| {
                settings.digit_hues[digit as usize]
            })
    } else {
        settings.hue_degrees
    };
    let sector = hue.rem_euclid(360) as f32 / 60.0;
    let c = 0.75;
    let secondary = c * (1.0 - (sector % 2.0 - 1.0).abs());
    let rgb = match sector as usize {
        0 => [c, secondary, 0.0],
        1 => [secondary, c, 0.0],
        2 => [0.0, c, secondary],
        3 => [0.0, secondary, c],
        4 => [secondary, 0.0, c],
        _ => [c, 0.0, secondary],
    };
    rgb.map(|component| {
        ((component + 0.25) * 255.0 * settings.brightness_percent as f32 / 100.0).round() as u8
    })
}
impl Scene for PiScene {
    fn reconfigure(&mut self, settings: &crate::AmbientSettings) -> bool {
        self.apply_settings(settings.pi.clone());
        true
    }
    fn render(&mut self, frame: &mut Frame<'_>) {
        self.receive();
        self.retry_preparation_if_due();
        let width = usize::from(frame.width);
        let height = usize::from(frame.height);
        let count = width * height;
        self.native_width = width;
        self.native.clear();
        frame.raster.dots.fill(0.0);
        frame.cell_colors.clear();
        if count > MAX_CELLS {
            self.status = Some("Pi viewport exceeds 131072-cell rendering bound".into());
            return;
        }
        self.native.resize(count, None);
        frame.cell_colors.resize(count, [0; 3]);
        let Some(prepared) = &self.prepared else {
            return;
        };
        if self.settings.brightness_percent == 0 {
            return;
        }
        let limit = self.settings.digits_limit as usize;
        let offset = ((frame.time.as_secs_f64() * self.settings.scroll_speed as f64 / 10.0).floor()
            % (limit + 1) as f64) as usize;
        if self.settings.mode == 0 {
            for (index, cell) in self.native.iter_mut().enumerate() {
                let character = character_at(&prepared.digits.text, limit, index + offset);
                *cell = Some(character);
                frame.cell_colors[index] = digit_color(&self.settings, character);
            }
            return;
        }
        let atlas = &prepared.atlas;
        let columns = frame.raster.width / atlas.width;
        let rows = frame.raster.height / atlas.height;
        if columns == 0 || rows == 0 {
            return;
        }
        for row in 0..rows {
            for column in 0..columns {
                let character = character_at(
                    &prepared.digits.text,
                    limit,
                    row * columns + column + offset,
                );
                let glyph_index = character.to_digit(10).map_or(10, |digit| digit as usize);
                let ink = &atlas.glyphs[glyph_index];
                let color = digit_color(&self.settings, character);
                for y in 0..atlas.height {
                    for x in 0..atlas.width {
                        let sample = ink[y * atlas.width + x];
                        if sample <= 0.0 {
                            continue;
                        }
                        let px = column * atlas.width + x;
                        let py = row * atlas.height + y;
                        frame.raster.dots[py * frame.raster.width + px] = sample;
                        let cell = (py / 4) * width + px / 2;
                        if let Some(cell) = frame.cell_colors.get_mut(cell) {
                            *cell = color;
                        }
                    }
                }
            }
        }
    }
    fn native_glyph(&self, x: u16, y: u16) -> Option<char> {
        PiScene::native_glyph(self, x, y)
    }
    fn uses_cell_colors(&self) -> bool {
        true
    }
    fn frames_per_second(&self) -> u32 {
        12
    }
    fn status(&self) -> Option<String> {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared(digit_count: usize, font_size: i32) -> Prepared {
        let resources = crate::resources::test_resources();
        let digit_storage = resources
            .reserve_storage(digit_count.saturating_add(std::mem::size_of::<DigitPrefix>()))
            .unwrap();
        let digits = Arc::new(DigitPrefix {
            text: digits::generate(digit_count).unwrap(),
            _storage: digit_storage,
        });
        let atlas_storage = resources
            .reserve_storage(atlas_storage_bytes(font_size).unwrap())
            .unwrap();
        let atlas = build_atlas(font_size).unwrap();
        Prepared {
            digits,
            atlas,
            _atlas_storage: atlas_storage,
        }
    }
    use crate::raster::Raster;
    #[test]
    fn controls_round_trip_with_independent_digit_hues_and_clamped_bounds() {
        let mut settings = PiSettings {
            mode: 1,
            digit_colors: true,
            ..Default::default()
        };
        let mut ids = std::collections::HashSet::new();
        for control in settings.controls() {
            assert!(ids.insert(control.id));
            assert!(settings
                .set_control(control.id, control.stepped(1).unwrap())
                .unwrap());
        }
        assert_eq!(
            serde_json::from_str::<PiSettings>(&serde_json::to_string(&settings).unwrap()).unwrap(),
            settings
        );
        let previous = settings.digit_hues;
        settings
            .set_control("digit_3_hue", ControlValue::Number(999))
            .unwrap();
        assert_eq!(settings.digit_hues[3], 360);
        for (digit, hue) in previous.iter().enumerate() {
            if digit != 3 {
                assert_eq!(*hue, settings.digit_hues[digit]);
            }
        }
        settings
            .set_control("digits_limit", ControlValue::Number(i32::MAX))
            .unwrap();
        assert_eq!(settings.digits_limit, 20000);
        assert!(!settings
            .set_control("font_size", ControlValue::Text("wrong".into()))
            .unwrap());
        assert!(!settings
            .set_control("unknown", ControlValue::Number(5))
            .unwrap());
    }
    #[test]
    fn font_braille_scroll_and_zero_brightness_preserve_real_digit_content() {
        let prepared = prepared(100, 16);
        let mut scene = PiScene::prepared(
            PiSettings {
                mode: 1,
                ..Default::default()
            },
            prepared,
        );
        let mut raster = Raster::default();
        raster.resize(80, 48);
        let mut colors = vec![];
        {
            let mut frame = Frame {
                raster: &mut raster,
                cell_colors: &mut colors,
                width: 40,
                height: 12,
                time: Duration::ZERO,
                wall: Duration::ZERO,
                now: std::time::SystemTime::UNIX_EPOCH,
            };
            scene.render(&mut frame);
        }
        assert!(raster.dots.iter().any(|value| *value > 0.0));
        assert!(raster
            .dots
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
        assert_eq!(colors.len(), 480);
        assert_eq!(scene.native_glyph(0, 0), None);
        let settings = PiSettings {
            mode: 0,
            digits_limit: 4096,
            scroll_speed: 10,
            ..Default::default()
        };
        scene.apply_settings(settings);
        {
            let mut frame = Frame {
                raster: &mut raster,
                cell_colors: &mut colors,
                width: 40,
                height: 12,
                time: Duration::from_secs(1),
                wall: Duration::from_secs(1),
                now: std::time::SystemTime::UNIX_EPOCH,
            };
            scene.render(&mut frame);
        }
        assert_eq!(scene.native_glyph(0, 0), Some('.'));
        assert_eq!(scene.native_glyph(1, 0), Some('1'));
        scene.settings.brightness_percent = 0;
        let mut frame = Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width: 40,
            height: 12,
            time: Duration::ZERO,
            wall: Duration::ZERO,
            now: std::time::SystemTime::UNIX_EPOCH,
        };
        scene.render(&mut frame);
        assert!(raster.dots.iter().all(|value| *value == 0.0));
        assert_eq!(scene.native_glyph(0, 0), None);
        assert!(colors.iter().all(|value| *value == [0; 3]));
    }
    #[test]
    fn worker_publishes_exact_digits_and_bounded_font_atlas() {
        let mut scene = PiScene::new(
            PiSettings {
                digits_limit: 64,
                font_size: 8,
                ..Default::default()
            },
            &SceneEnv::for_test(
                std::path::PathBuf::new(),
                crate::resources::test_resources(),
            ),
        );
        let prepared = scene
            .receiver
            .take()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert!(prepared
            .digits
            .text
            .starts_with("314159265358979323846264338327950"));
        assert!(prepared.digits.text.len() >= 64);
        assert_eq!(prepared.atlas.glyphs.len(), 11);
    }
    #[test]
    fn exact_digit_prefix_is_reused_without_allocating_another_charge() {
        let resources = crate::resources::test_resources();
        let stop = AtomicBool::new(false);
        let prefix = exact_digits(64, &stop, &resources, None).unwrap();
        let reused = exact_digits(32, &stop, &resources, Some(Arc::clone(&prefix))).unwrap();
        assert!(Arc::ptr_eq(&prefix, &reused));
        assert_eq!(reused.text.len(), 64);
    }
    #[test]
    fn worker_admission_refusal_can_retry_after_capacity_is_released() {
        use ilium_execution::{ShutdownMode, WorkerExit};
        use std::time::Instant;

        let (mut execution, resources) = crate::resources::isolated_test_resources();
        let pressure = resources
            .reserve_worker(WorkerCost {
                threads: 1,
                resident_bytes: 2300 * 1024 * 1024,
            })
            .unwrap();
        let environment = SceneEnv::for_test(std::path::PathBuf::new(), resources.clone());
        let mut scene = PiScene::new(
            PiSettings {
                digits_limit: 64,
                font_size: 8,
                ..Default::default()
            },
            &environment,
        );
        assert!(scene.worker.is_none());
        assert!(scene
            .status
            .as_deref()
            .unwrap()
            .contains("admission refused"));

        drop(pressure);
        scene.retry_after = Some(Instant::now());
        let mut raster = Raster::default();
        raster.resize(2, 4);
        let mut colors = vec![];
        let mut frame = Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width: 1,
            height: 1,
            time: Duration::ZERO,
            wall: Duration::ZERO,
            now: std::time::SystemTime::UNIX_EPOCH,
        };
        scene.render(&mut frame);
        assert!(scene.worker.is_some());
        let result = scene
            .receiver
            .as_ref()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(result.is_ok());
        drop(scene);

        execution.request_shutdown(ShutdownMode::Cancel);
        let report = execution
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert!(report.shutdown_complete);
        assert_eq!(report.remaining_workers, 0);
        assert!(report.observations.iter().all(|observation| matches!(
            observation,
            ilium_execution::JoinObservation::Exited {
                exit: WorkerExit::Joined,
                ..
            }
        )));
    }
    #[test]
    fn empty_tiny_targets_and_native_out_of_bounds_are_safe() {
        let prepared = prepared(10, 48);
        let mut scene = PiScene::prepared(
            PiSettings {
                mode: 1,
                ..Default::default()
            },
            prepared,
        );
        for (width, height) in [(0, 0), (1, 1), (2, 3)] {
            let mut raster = Raster::default();
            raster.resize(usize::from(width) * 2, usize::from(height) * 4);
            let mut colors = vec![];
            let mut frame = Frame {
                raster: &mut raster,
                cell_colors: &mut colors,
                width,
                height,
                time: Duration::ZERO,
                wall: Duration::ZERO,
                now: std::time::SystemTime::UNIX_EPOCH,
            };
            scene.render(&mut frame);
            assert_eq!(colors.len(), usize::from(width) * usize::from(height));
            assert!(raster.dots.iter().all(|value| *value == 0.0));
            assert_eq!(scene.native_glyph(width, height), None);
        }
    }
    #[test]
    fn exact_prefix_composes_native_cells_with_decimal() {
        let mut scene = PiScene::prepared(PiSettings::default(), prepared(100, 16));
        let mut raster = Raster::default();
        raster.resize(40, 16);
        let mut colors = vec![];
        let mut frame = Frame {
            raster: &mut raster,
            cell_colors: &mut colors,
            width: 20,
            height: 4,
            time: Duration::ZERO,
            wall: Duration::ZERO,
            now: std::time::SystemTime::UNIX_EPOCH,
        };
        scene.render(&mut frame);
        let text: String = (0..20).filter_map(|x| scene.native_glyph(x, 0)).collect();
        assert_eq!(text, "3.141592653589793238");
        assert!(raster.dots.iter().all(|value| *value == 0.0));
    }
    #[test]
    fn real_font_atlas_has_ink_for_ten_digits_and_decimal_at_bounded_sizes() {
        for size in [8, 16, 48] {
            let atlas = build_atlas(size).unwrap();
            assert!(atlas.width <= 64 && atlas.height <= 80);
            for glyph in atlas.glyphs {
                assert!(glyph.iter().any(|value| *value > 0.0));
            }
        }
    }
}
