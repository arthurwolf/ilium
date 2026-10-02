//! Controls for the surface Overworld landscape. World identity is
//! independent of camera, palette and detail, so changing appearance never
//! regenerates a different landscape.
use crate::control::{self, Control, ControlValue, SceneSettings};
use crate::minecraft::settings::{SavedMapsSettings, WorldSource};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// User overrides are retained separately for each named pack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackSourceSettings {
    pub path: String,
    pub root: String,
    pub mount: usize,
    pub format_major: u32,
    pub format_minor: u32,
    pub edition: usize,
    pub addon_path: String,
    pub addon_mount: usize,
    pub duplicate_last_wins: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoxelLandscapeSettings {
    pub saved_maps: SavedMapsSettings,
    pub seed: u32,
    /// Static generated-scene context: 0 day, 1 night, 2 thunderstorm.
    pub atmosphere: usize,
    pub zoom_percent: i32,
    pub detail: usize,
    pub pan_speed_percent: i32,
    pub pan_direction: usize,
    pub color_mode: usize,
    pub palette: usize,
    pub hue_degrees: i32,
    pub saturation_percent: i32,
    pub lightness_percent: i32,
    pub vegetation_percent: i32,
    pub structures_percent: i32,
    pub rivers: bool,
    pub ravines: bool,
    pub caves: bool,
    /// Stable index into FULL_PACKS. The selected source must also have a path.
    pub pack_profile: usize,
    pub pack_path: String,
    /// Optional archive/extracted directory root inside the selected source.
    pub pack_root: String,
    /// 0=ZIP, 1=directory; edition/layout comes from the reviewed profile.
    pub pack_mount: usize,
    pub pack_format_major: u32,
    pub pack_format_minor: u32,
    /// 0=profile primary edition; 1=reviewed Plasticator Bedrock variant.
    pub pack_edition: usize,
    pub pack_addon_path: String,
    pub pack_addon_mount: usize,
    pub pack_duplicate_last_wins: bool,
    pub pack_custom_sources: BTreeMap<String, PackSourceSettings>,
}
impl Default for VoxelLandscapeSettings {
    fn default() -> Self {
        Self {
            saved_maps: SavedMapsSettings::default(),
            seed: 71839,
            atmosphere: 0,
            zoom_percent: 150,
            detail: 2,
            pan_speed_percent: 25,
            pan_direction: 0,
            color_mode: 1,
            palette: 0,
            hue_degrees: 180,
            saturation_percent: 100,
            lightness_percent: 100,
            vegetation_percent: 100,
            structures_percent: 100,
            rivers: true,
            ravines: true,
            caves: false,
            pack_profile: 3,
            pack_path: String::new(),
            pack_root: String::new(),
            pack_mount: 0,
            pack_format_major: 999,
            pack_format_minor: 0,
            pack_edition: 0,
            pack_addon_path: String::new(),
            pack_addon_mount: 0,
            pack_duplicate_last_wins: false,
            pack_custom_sources: BTreeMap::new(),
        }
    }
}

impl VoxelLandscapeSettings {
    pub fn source_settings(&self) -> PackSourceSettings {
        PackSourceSettings {
            path: self.pack_path.clone(),
            root: self.pack_root.clone(),
            mount: self.pack_mount,
            format_major: self.pack_format_major,
            format_minor: self.pack_format_minor,
            edition: self.pack_edition,
            addon_path: self.pack_addon_path.clone(),
            addon_mount: self.pack_addon_mount,
            duplicate_last_wins: self.pack_duplicate_last_wins,
        }
    }

    pub fn apply_source_settings(&mut self, source: &PackSourceSettings) {
        self.pack_path.clone_from(&source.path);
        self.pack_root.clone_from(&source.root);
        self.pack_mount = source.mount;
        self.pack_format_major = source.format_major;
        self.pack_format_minor = source.format_minor;
        self.pack_edition = source.edition;
        self.pack_addon_path.clone_from(&source.addon_path);
        self.pack_addon_mount = source.addon_mount;
        self.pack_duplicate_last_wins = source.duplicate_last_wins;
    }

    fn select_pack(&mut self, index: usize) {
        if index == self.pack_profile {
            return;
        }
        let old_id = super::pack_profiles::FULL_PACKS[self.pack_profile.min(10)].id;
        if !self.pack_path.is_empty() {
            self.pack_custom_sources
                .insert(old_id.into(), self.source_settings());
        } else {
            self.pack_custom_sources.remove(old_id);
        }
        self.pack_profile = index;
        let id = super::pack_profiles::FULL_PACKS[index].id;
        let source = self
            .pack_custom_sources
            .get(id)
            .cloned()
            .unwrap_or_else(|| Self::default().source_settings());
        self.apply_source_settings(&source);
    }
}

impl SceneSettings for VoxelLandscapeSettings {
    fn normalized(&self) -> Self {
        Self {
            zoom_percent: self.zoom_percent.clamp(25, 400),
            detail: self.detail.min(3),
            atmosphere: self.atmosphere.min(2),
            pan_speed_percent: self.pan_speed_percent.clamp(0, 200),
            pan_direction: self.pan_direction.min(3),
            color_mode: self.color_mode.min(1),
            palette: self.palette.min(3),
            hue_degrees: self.hue_degrees.clamp(0, 360),
            saturation_percent: self.saturation_percent.clamp(0, 100),
            lightness_percent: self.lightness_percent.clamp(5, 100),
            vegetation_percent: self.vegetation_percent.clamp(0, 200),
            structures_percent: self.structures_percent.clamp(0, 200),
            pack_profile: self.pack_profile.min(10),
            pack_mount: self.pack_mount.min(1),
            pack_edition: self.pack_edition.min(1),
            pack_addon_mount: self.pack_addon_mount.min(1),
            pack_format_major: self.pack_format_major.min(i32::MAX as u32),
            pack_format_minor: self.pack_format_minor.min(i32::MAX as u32),
            ..self.clone()
        }
    }
    fn controls(&self) -> Vec<Control> {
        let value = self.normalized();
        let mut rows = value.saved_maps.controls();
        let renderer_rows = vec![
            Control::choice("pack_profile", "Full texture pack", value.pack_profile,
                &["Jicklus", "F8thful", "Whimscape", "GoodVibes / Acaitart",
                  "deathcap ProgrammerArt", "Textureless", "Plasticator",
                  "PixelPerfectionCE", "Faithful32", "Faithful64", "Antumbra"],
                "Private full-world test source; missing texture/model coverage is reported explicitly."),
            Control::text("pack_path", "Custom pack file or folder", &value.pack_path,
                "Blank uses the selected installed pack", "Optional absolute local ZIP archive or extracted directory for this pack. Each pack retains its own override."),
            Control::text("pack_root", "Root inside pack", &value.pack_root,
                "Optional relative folder", "Use only when the archive nests its pack files under a folder."),
            Control::choice("pack_mount", "Pack source type", value.pack_mount,
                &["ZIP archive", "Directory"], "Choose how to read the selected local source."),
            Control::choice("pack_edition", "Pack edition", value.pack_edition,
                &["Reviewed primary edition", "Plasticator Bedrock 2.4"],
                "Bedrock is available only for the reviewed Plasticator variant."),
            Control::text("pack_addon_path", "Official models add-on", &value.pack_addon_path,
                "Optional absolute path", "Only Textureless has a reviewed internal model add-on."),
            Control::choice("pack_addon_mount", "Add-on source type", value.pack_addon_mount,
                &["ZIP archive", "Directory"], "Applies only to the Textureless add-on."),
            Control::toggle("pack_duplicate_last_wins", "Use last duplicate ZIP member",
                value.pack_duplicate_last_wins,
                "Textureless only: retain a duplicate-member report and choose the final central-directory entry."),
            Control::text("pack_format", "Target pack format", &format!("{}.{}", value.pack_format_major, value.pack_format_minor),
                "major.minor", "Target for authored overlays; outside declared range requires explicit compatibility evidence."),
            Control::text("seed", "World seed", &value.seed.to_string(), "0–4294967295", "The same seed recreates the same world, including structures. Camera and color changes preserve terrain."),
            Control::choice("atmosphere", "Scene atmosphere", value.atmosphere, &["Day", "Night", "Thunderstorm"], "Choose a fixed atmosphere with matching light and surface creature scenes. Terrain and structures keep their positions."),
            Control::slider("zoom", "Tile zoom", value.zoom_percent, (25,400,5), "%", "Enlarge isometric tiles to inspect blocks, or zoom out to see more landscape."),
            Control::choice("detail", "Detail", value.detail, &["Terrain", "Landmarks", "Landscape", "All features"], "Choose the visible decoration density. Terrain and structure positions remain stable across detail levels."),
            Control::slider("pan_speed", "Camera speed", value.pan_speed_percent, (0,200,5), "%", "Slow continuous movement across the world. Zero freezes the camera; global animation speed also applies."),
            Control::choice("pan_direction", "Camera direction", value.pan_direction, &["East", "South", "North-east", "South-east"], "Choose the direction in world space; the isometric projection keeps both ground axes visible."),
            Control::choice("color_mode", "Dither color", value.color_mode, &["Black and white", "Texture colors"], "Use the selected artwork's colors or monochrome shading. Global density controls dot coverage."),
            Control::choice("palette", "Landscape palette", value.palette, &["Original colors", "Rose garden", "Cool mist", "Amber evening"], "Keep original texture colors or choose a color tint."),
            Control::slider("hue", "Hue tint", value.hue_degrees, (0,360,5), "°", "180° is neutral; lower values favor warm tones and higher values favor cool tones."),
            Control::slider("saturation", "Color saturation", value.saturation_percent, (0,100,5), "%", "Pastel color intensity. Zero makes all material colors gray while retaining their shading."),
            Control::slider("lightness", "Color lightness", value.lightness_percent, (5,100,5), "%", "Brightness of lit dots in both modes, not the number of dots. Does not replace the global background lightness setting."),
            Control::slider("vegetation", "Vegetation density", value.vegetation_percent, (0,200,5), "%", "Density of biome-appropriate trees, flowers, crops and other plants. Existing anchor positions remain deterministic."),
            Control::slider("structures", "Structure density", value.structures_percent, (0,200,5), "%", "Density of villages, ruins and landscape landmarks. Zero retains natural terrain only."),
            Control::toggle("rivers", "Rivers", value.rivers, "Carve coherent river channels and fill them to their water level."),
            Control::toggle("ravines", "Ravines", value.ravines, "Open narrow deep fissures exposing stratified rock faces."),
            Control::toggle("caves", "Cave mouths", value.caves, "Show surface cave openings with dark entrances, not a subterranean camera."),
        ];
        rows.extend(renderer_rows.into_iter().filter(|row| {
            value.saved_maps.source == WorldSource::Generated
                || !matches!(
                    row.id,
                    "seed"
                        | "atmosphere"
                        | "detail"
                        | "pan_direction"
                        | "vegetation"
                        | "structures"
                        | "rivers"
                        | "ravines"
                        | "caves"
                )
        }));
        rows
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        if matches!(id, "world_source" | "saved_maps_folder") {
            return self.saved_maps.set_control(id, value);
        }
        let previous = self.clone();
        match id {
            "pack_addon_path" => {
                let path = control::text(&value)
                    .ok_or("Expected a local add-on path")?
                    .trim();
                if path.len() > 4096 || path.chars().any(char::is_control) {
                    return Err("Add-on path is too long or contains controls".into());
                }
                self.pack_addon_path = path.to_owned();
            }
            "pack_path" => {
                let path = control::text(&value).ok_or("Expected a local path")?.trim();
                if path.len() > 4096 || path.chars().any(char::is_control) {
                    return Err("Pack path is too long or contains controls".into());
                }
                self.pack_path = path.to_owned();
            }
            "pack_root" => {
                let root = control::text(&value)
                    .ok_or("Expected a relative pack root")?
                    .trim();
                if root.len() > 512
                    || root.chars().any(char::is_control)
                    || (!root.is_empty()
                        && super::assets::identity::AssetPath::parse(root).is_err())
                {
                    return Err("Pack root must be a safe relative folder".into());
                }
                self.pack_root = root.to_owned();
            }
            "pack_format" => {
                let text = control::text(&value).ok_or("Expected major.minor")?;
                let (major, minor) = text.trim().split_once('.').ok_or("Expected major.minor")?;
                self.pack_format_major = major.parse().map_err(|_| "Invalid pack major format")?;
                self.pack_format_minor = minor.parse().map_err(|_| "Invalid pack minor format")?;
            }
            "seed" => {
                self.seed = control::text(&value)
                    .ok_or("Expected a world seed")?
                    .trim()
                    .parse()
                    .map_err(|_| "World seed must be an integer from 0 through 4294967295")?
            }
            "zoom" | "pan_speed" | "hue" | "saturation" | "lightness" | "vegetation"
            | "structures" => {
                let number = control::number(&value).ok_or("Expected a number")?;
                match id {
                    "zoom" => self.zoom_percent = number,
                    "pan_speed" => self.pan_speed_percent = number,
                    "hue" => self.hue_degrees = number,
                    "saturation" => self.saturation_percent = number,
                    "lightness" => self.lightness_percent = number,
                    "vegetation" => self.vegetation_percent = number,
                    "structures" => self.structures_percent = number,
                    _ => return Ok(false),
                }
            }
            "atmosphere" | "detail" | "pan_direction" | "color_mode" | "palette"
            | "pack_profile" | "pack_mount" | "pack_edition" | "pack_addon_mount" => {
                let index = control::index(&value).ok_or("Expected a choice")?;
                let limit = match id {
                    "color_mode" | "pack_mount" | "pack_edition" | "pack_addon_mount" => 2,
                    "pack_profile" => 11,
                    "atmosphere" => 3,
                    _ => 4,
                };
                if index >= limit {
                    return Err("Unknown choice".into());
                }
                match id {
                    "detail" => self.detail = index,
                    "atmosphere" => self.atmosphere = index,
                    "pan_direction" => self.pan_direction = index,
                    "color_mode" => self.color_mode = index,
                    "palette" => self.palette = index,
                    "pack_profile" => self.select_pack(index),
                    "pack_mount" => self.pack_mount = index,
                    "pack_edition" => self.pack_edition = index,
                    "pack_addon_mount" => self.pack_addon_mount = index,
                    _ => return Ok(false),
                }
            }
            "rivers" | "ravines" | "caves" | "pack_duplicate_last_wins" => {
                let enabled = control::boolean(&value).ok_or("Expected on or off")?;
                match id {
                    "rivers" => self.rivers = enabled,
                    "ravines" => self.ravines = enabled,
                    "caves" => self.caves = enabled,
                    "pack_duplicate_last_wins" => self.pack_duplicate_last_wins = enabled,
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != previous)
    }
}

#[cfg(test)]
mod saved_source_tests {
    use super::*;

    #[test]
    fn saved_choice_survives_reload_and_excludes_generation_controls() {
        let mut settings = VoxelLandscapeSettings::default();
        assert!(settings
            .controls()
            .iter()
            .any(|row| row.id == "world_source"));
        assert!(settings
            .set_control("world_source", ControlValue::Index(1))
            .unwrap());
        let reloaded: VoxelLandscapeSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        let rows = reloaded.controls();
        assert_eq!(
            rows.iter()
                .find(|row| row.id == "world_source")
                .unwrap()
                .value,
            ControlValue::Index(1)
        );
        assert!(rows.iter().any(|row| row.id == "saved_maps_folder"));
        for id in [
            "seed",
            "vegetation",
            "structures",
            "rivers",
            "ravines",
            "caves",
            "pan_direction",
            "detail",
        ] {
            assert!(!rows.iter().any(|row| row.id == id), "{id}");
        }
        for id in [
            "zoom",
            "pan_speed",
            "pack_profile",
            "pack_path",
            "color_mode",
        ] {
            assert!(rows.iter().any(|row| row.id == id), "{id}");
        }
    }

    #[test]
    fn source_and_authored_folder_preserve_pack_and_generated_settings() {
        let mut settings = VoxelLandscapeSettings {
            pack_path: "/private/pack.zip".into(),
            seed: 42,
            ..Default::default()
        };
        let folder = std::env::temp_dir().join("saved worlds 雪 ");
        let text = folder.to_str().unwrap().to_owned();
        assert!(settings
            .set_control("saved_maps_folder", ControlValue::Text(text.clone()))
            .unwrap());
        assert!(settings
            .set_control("world_source", ControlValue::Index(1))
            .unwrap());
        assert_eq!(settings.pack_path, "/private/pack.zip");
        assert_eq!(settings.seed, 42);
        let reloaded: VoxelLandscapeSettings =
            serde_json::from_value(serde_json::to_value(settings.normalized()).unwrap()).unwrap();
        assert_eq!(
            reloaded
                .controls()
                .iter()
                .find(|row| row.id == "saved_maps_folder")
                .unwrap()
                .value,
            ControlValue::Text(text)
        );
        settings
            .set_control("world_source", ControlValue::Index(0))
            .unwrap();
        assert!(settings.controls().iter().any(|row| row.id == "seed"));
        assert_eq!(settings.pack_path, "/private/pack.zip");
        assert_eq!(settings.seed, 42);
    }

    #[test]
    fn invalid_saved_controls_are_atomic_and_missing_source_defaults_generated() {
        let mut settings: VoxelLandscapeSettings = serde_json::from_str("{\"seed\":99}").unwrap();
        assert_eq!(
            settings
                .controls()
                .iter()
                .find(|row| row.id == "world_source")
                .unwrap()
                .value,
            ControlValue::Index(0)
        );
        let original = settings.clone();
        for (id, value) in [
            ("world_source", ControlValue::Index(2)),
            ("world_source", ControlValue::Text("saved".into())),
            (
                "saved_maps_folder",
                ControlValue::Text("relative/path".into()),
            ),
            ("saved_maps_folder", ControlValue::Text("bad\0path".into())),
        ] {
            assert!(settings.set_control(id, value).is_err());
            assert_eq!(settings, original);
        }
    }
}

#[cfg(test)]
mod pack_selection_tests {
    use super::*;

    #[test]
    fn changing_pack_never_relabels_the_previous_pack_path() {
        let mut settings = VoxelLandscapeSettings {
            pack_path: "/private/goodvibes".into(),
            pack_mount: 1,
            ..Default::default()
        };
        settings
            .set_control("pack_profile", ControlValue::Index(8))
            .unwrap();
        assert!(settings.pack_path.is_empty());
        settings
            .set_control(
                "pack_path",
                ControlValue::Text("/private/faithful.zip".into()),
            )
            .unwrap();
        settings
            .set_control("pack_profile", ControlValue::Index(3))
            .unwrap();
        assert_eq!(settings.pack_path, "/private/goodvibes");
        assert_eq!(settings.pack_mount, 1);
        settings
            .set_control("pack_profile", ControlValue::Index(8))
            .unwrap();
        assert_eq!(settings.pack_path, "/private/faithful.zip");
    }
}

#[cfg(test)]
mod atmosphere_tests {
    use super::*;
    #[test]
    fn generated_atmosphere_persists_defaults_and_rejects_unknown_choices() {
        let mut settings = VoxelLandscapeSettings::default();
        assert_eq!(
            settings.set_control("atmosphere", ControlValue::Index(1)),
            Ok(true)
        );
        let restored: VoxelLandscapeSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            restored
                .controls()
                .iter()
                .find(|r| r.id == "atmosphere")
                .unwrap()
                .value,
            ControlValue::Index(1)
        );
        let before = settings.clone();
        assert!(settings
            .set_control("atmosphere", ControlValue::Index(3))
            .is_err());
        assert_eq!(settings, before);
        let missing: VoxelLandscapeSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            missing
                .controls()
                .iter()
                .find(|r| r.id == "atmosphere")
                .unwrap()
                .value,
            ControlValue::Index(0)
        );
        settings
            .set_control("world_source", ControlValue::Index(1))
            .unwrap();
        assert!(!settings.controls().iter().any(|r| r.id == "atmosphere"));
    }
}
