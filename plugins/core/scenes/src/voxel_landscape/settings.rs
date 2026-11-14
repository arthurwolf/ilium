//! Controls for the surface Overworld landscape. World identity is
//! independent of camera, palette and detail, so changing appearance never
//! regenerates a different landscape.
use crate::control::{self, Control, ControlValue, SceneSettings};
use crate::minecraft::settings::{SavedMapsSettings, WorldSource};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const GENERATED_TEXTURE_SOURCE_JAVA_DEFAULT: usize = 1;

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
#[serde(try_from = "PersistedVoxelLandscapeSettings")]
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
    /// Generated-only texture source: 0 selected pack, 1 installed Java 1.19.3.
    pub generated_texture_source: usize,
    /// 0 is the legacy eleven-pack order; 1 is the retained eight-pack order.
    pub pack_profile_version: u32,
    /// Index into FULL_PACKS in the current schema.
    pub pack_profile: usize,
    /// Retired active sources that differed from an existing override entry.
    /// These records are archival only and never participate in source selection.
    pub pack_retired_sources: Vec<RetiredPackSource>,
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
            generated_texture_source: 0,
            pack_profile_version: PACK_PROFILE_VERSION,
            pack_profile: 0,
            pack_retired_sources: Vec::new(),
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

const PACK_PROFILE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetiredPackSource {
    pub profile: String,
    pub source: PackSourceSettings,
}

// Keep absence distinct from an explicit legacy index 0. A missing index means
// GoodVibes: legacy index 3 for unversioned settings, current index 0 otherwise.
#[derive(Deserialize)]
#[serde(default)]
struct PersistedVoxelLandscapeSettings {
    saved_maps: SavedMapsSettings,
    seed: u32,
    atmosphere: usize,
    zoom_percent: i32,
    detail: usize,
    pan_speed_percent: i32,
    pan_direction: usize,
    color_mode: usize,
    palette: usize,
    hue_degrees: i32,
    saturation_percent: i32,
    lightness_percent: i32,
    vegetation_percent: i32,
    structures_percent: i32,
    rivers: bool,
    ravines: bool,
    caves: bool,
    generated_texture_source: usize,
    #[serde(deserialize_with = "deserialize_present")]
    pack_profile_version: Option<u32>,
    #[serde(deserialize_with = "deserialize_present")]
    pack_profile: Option<usize>,
    pack_retired_sources: Vec<RetiredPackSource>,
    pack_path: String,
    pack_root: String,
    pack_mount: usize,
    pack_format_major: u32,
    pack_format_minor: u32,
    pack_edition: usize,
    pack_addon_path: String,
    pack_addon_mount: usize,
    pack_duplicate_last_wins: bool,
    pack_custom_sources: BTreeMap<String, PackSourceSettings>,
}

fn deserialize_present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl Default for PersistedVoxelLandscapeSettings {
    fn default() -> Self {
        let settings = VoxelLandscapeSettings::default();
        Self {
            saved_maps: settings.saved_maps,
            seed: settings.seed,
            atmosphere: settings.atmosphere,
            zoom_percent: settings.zoom_percent,
            detail: settings.detail,
            pan_speed_percent: settings.pan_speed_percent,
            pan_direction: settings.pan_direction,
            color_mode: settings.color_mode,
            palette: settings.palette,
            hue_degrees: settings.hue_degrees,
            saturation_percent: settings.saturation_percent,
            lightness_percent: settings.lightness_percent,
            vegetation_percent: settings.vegetation_percent,
            structures_percent: settings.structures_percent,
            rivers: settings.rivers,
            ravines: settings.ravines,
            caves: settings.caves,
            generated_texture_source: settings.generated_texture_source,
            pack_profile_version: None,
            pack_profile: None,
            pack_retired_sources: settings.pack_retired_sources,
            pack_path: settings.pack_path,
            pack_root: settings.pack_root,
            pack_mount: settings.pack_mount,
            pack_format_major: settings.pack_format_major,
            pack_format_minor: settings.pack_format_minor,
            pack_edition: settings.pack_edition,
            pack_addon_path: settings.pack_addon_path,
            pack_addon_mount: settings.pack_addon_mount,
            pack_duplicate_last_wins: settings.pack_duplicate_last_wins,
            pack_custom_sources: settings.pack_custom_sources,
        }
    }
}

impl TryFrom<PersistedVoxelLandscapeSettings> for VoxelLandscapeSettings {
    type Error = String;

    fn try_from(settings: PersistedVoxelLandscapeSettings) -> Result<Self, Self::Error> {
        let version = settings.pack_profile_version.unwrap_or(0);
        if version > PACK_PROFILE_VERSION {
            return Err(format!(
                "Unsupported Overworld pack profile version: {version}"
            ));
        }
        let profile = settings
            .pack_profile
            .unwrap_or(if version == 0 { 3 } else { 0 });
        Ok(Self {
            saved_maps: settings.saved_maps,
            seed: settings.seed,
            atmosphere: settings.atmosphere,
            zoom_percent: settings.zoom_percent,
            detail: settings.detail,
            pan_speed_percent: settings.pan_speed_percent,
            pan_direction: settings.pan_direction,
            color_mode: settings.color_mode,
            palette: settings.palette,
            hue_degrees: settings.hue_degrees,
            saturation_percent: settings.saturation_percent,
            lightness_percent: settings.lightness_percent,
            vegetation_percent: settings.vegetation_percent,
            structures_percent: settings.structures_percent,
            rivers: settings.rivers,
            ravines: settings.ravines,
            caves: settings.caves,
            generated_texture_source: settings.generated_texture_source.min(1),
            pack_profile_version: version,
            pack_profile: profile,
            pack_retired_sources: settings.pack_retired_sources,
            pack_path: settings.pack_path,
            pack_root: settings.pack_root,
            pack_mount: settings.pack_mount,
            pack_format_major: settings.pack_format_major,
            pack_format_minor: settings.pack_format_minor,
            pack_edition: settings.pack_edition,
            pack_addon_path: settings.pack_addon_path,
            pack_addon_mount: settings.pack_addon_mount,
            pack_duplicate_last_wins: settings.pack_duplicate_last_wins,
            pack_custom_sources: settings.pack_custom_sources,
        }
        .normalized())
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

    fn migrate_pack_profile(&mut self) {
        if self.pack_profile_version != 0 {
            self.pack_profile = self
                .pack_profile
                .min(super::pack_profiles::FULL_PACKS.len() - 1);
            return;
        }
        if let Some(id) = super::pack_profiles::RETIRED_PACK_IDS.get(self.pack_profile) {
            let source = self.source_settings();
            // Preserve an existing saved override unchanged. If the active
            // source differs, retain both rather than overwrite either one.
            if source != Self::default().source_settings() {
                match self.pack_custom_sources.get(*id) {
                    Some(saved) if saved != &source => {
                        let retired = RetiredPackSource {
                            profile: (*id).into(),
                            source,
                        };
                        if !self.pack_retired_sources.contains(&retired) {
                            self.pack_retired_sources.push(retired);
                        }
                    }
                    Some(_) => {}
                    None => {
                        self.pack_custom_sources.insert((*id).into(), source);
                    }
                }
            }
            self.pack_profile = 0;
            let source = self
                .pack_custom_sources
                .get("goodvibes")
                .cloned()
                .unwrap_or_else(|| Self::default().source_settings());
            self.apply_source_settings(&source);
        } else {
            // The old normalizer clamped invalid high values to Antumbra.
            // Interpret the legacy order before applying the new range.
            self.pack_profile = self.pack_profile.min(10) - 3;
        }
        self.pack_profile_version = PACK_PROFILE_VERSION;
    }

    fn apply_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        if matches!(id, "world_source" | "saved_maps_folder") {
            self.saved_maps.set_control(id, value)?;
            return Ok(true);
        }
        match id {
            "generated_texture_source" => {
                let index = control::index(&value).ok_or("Expected a texture source choice")?;
                if index > GENERATED_TEXTURE_SOURCE_JAVA_DEFAULT {
                    return Err("Unknown generated texture source".into());
                }
                self.generated_texture_source = index;
            }
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
                let path = control::text(&value).ok_or("Expected a local path")?;
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
                    "pack_profile" => super::pack_profiles::FULL_PACKS.len(),
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
        Ok(true)
    }

    fn select_pack(&mut self, index: usize) {
        self.migrate_pack_profile();
        if index == self.pack_profile {
            return;
        }
        let old_id = super::pack_profiles::FULL_PACKS[self.pack_profile].id;
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
        let mut value = self.clone();
        value.migrate_pack_profile();
        Self {
            zoom_percent: value.zoom_percent.clamp(25, 400),
            detail: value.detail.min(3),
            atmosphere: value.atmosphere.min(2),
            pan_speed_percent: value.pan_speed_percent.clamp(0, 200),
            pan_direction: value.pan_direction.min(3),
            color_mode: value.color_mode.min(1),
            palette: value.palette.min(3),
            hue_degrees: value.hue_degrees.clamp(0, 360),
            saturation_percent: value.saturation_percent.clamp(0, 100),
            lightness_percent: value.lightness_percent.clamp(5, 100),
            vegetation_percent: value.vegetation_percent.clamp(0, 200),
            structures_percent: value.structures_percent.clamp(0, 200),
            generated_texture_source: value
                .generated_texture_source
                .min(GENERATED_TEXTURE_SOURCE_JAVA_DEFAULT),
            pack_profile: value.pack_profile,
            pack_mount: value.pack_mount.min(1),
            pack_edition: value.pack_edition.min(1),
            pack_addon_mount: value.pack_addon_mount.min(1),
            pack_format_major: value.pack_format_major.min(i32::MAX as u32),
            pack_format_minor: value.pack_format_minor.min(i32::MAX as u32),
            ..value
        }
    }
    fn controls(&self) -> Vec<Control> {
        let value = self.normalized();
        let mut rows = value.saved_maps.controls();
        let is_saved_source = value.saved_maps.source == WorldSource::SavedMaps;
        let renderer_rows = vec![
            Control::choice(
                "generated_texture_source",
                "Generated texture source",
                value.generated_texture_source,
                &[
                    "Selected full texture pack",
                    "Minecraft Java 1.19.3 default",
                ],
                "Generated landscapes use either the selected texture pack or installed, digest-pinned Java 1.19.3 assets.",
            ),
            Control::choice(
                "pack_profile",
                if is_saved_source {
                    "Custom pack profile"
                } else if value.generated_texture_source == 1 {
                    "Saved pack profile (inactive)"
                } else {
                    "Full texture pack"
                },
                value.pack_profile,
                &[
                    "GoodVibes / Acaitart",
                    "deathcap ProgrammerArt",
                    "Textureless",
                    "Plasticator",
                    "PixelPerfectionCE",
                    "Faithful32",
                    "Faithful64",
                    "Antumbra",
                ],
                if is_saved_source {
                    "Choose the profile whose custom override you want to configure. A blank custom path uses installed Java 1.19.3 assets."
                } else {
                    "Selected full-world source; missing texture/model coverage is reported explicitly."
                },
            ),
            Control::text(
                "pack_path",
                "Custom pack file or folder",
                &value.pack_path,
                if is_saved_source {
                    "Blank uses installed Java 1.19.3 assets"
                } else if value.generated_texture_source == 1 {
                    "Ignored while Java default is active"
                } else {
                    "Blank uses the selected installed pack"
                },
                if is_saved_source {
                    "Optional absolute local Java ZIP archive or extracted directory. Its models and textures override installed Java 1.19.3 assets. Each profile retains its own custom override."
                } else {
                    "Optional absolute local ZIP archive or extracted directory for this pack. Each pack retains its own override."
                },
            ),
            Control::text(
                "pack_root",
                "Root inside pack",
                &value.pack_root,
                "Optional relative folder",
                "Use only when the archive nests its pack files under a folder.",
            ),
            Control::choice(
                "pack_mount",
                "Pack source type",
                value.pack_mount,
                &["ZIP archive", "Directory"],
                "Choose how to read the selected local source.",
            ),
            Control::choice(
                "pack_edition",
                "Pack edition",
                value.pack_edition,
                &["Reviewed primary edition", "Plasticator Bedrock 2.4"],
                "Bedrock is available only for the reviewed Plasticator variant.",
            ),
            Control::text(
                "pack_addon_path",
                "Official models add-on",
                &value.pack_addon_path,
                "Optional absolute path",
                "Only Textureless has a reviewed internal model add-on.",
            ),
            Control::choice(
                "pack_addon_mount",
                "Add-on source type",
                value.pack_addon_mount,
                &["ZIP archive", "Directory"],
                "Applies only to the Textureless add-on.",
            ),
            Control::toggle(
                "pack_duplicate_last_wins",
                "Use last duplicate ZIP member",
                value.pack_duplicate_last_wins,
                "Textureless only: retain a duplicate-member report and choose the final central-directory entry.",
            ),
            Control::text(
                "pack_format",
                "Target pack format",
                &format!("{}.{}", value.pack_format_major, value.pack_format_minor),
                "major.minor",
                "Target for authored overlays; outside declared range requires explicit compatibility evidence.",
            ),
            Control::text(
                "seed",
                "World seed",
                &value.seed.to_string(),
                "0–4294967295",
                "The same seed recreates the same world, including structures. Camera and color changes preserve terrain.",
            ),
            Control::choice(
                "atmosphere",
                "Scene atmosphere",
                value.atmosphere,
                &["Day", "Night", "Thunderstorm"],
                "Choose a fixed atmosphere with matching light and surface creature scenes. Terrain and structures keep their positions.",
            ),
            Control::slider(
                "zoom",
                "Tile zoom",
                value.zoom_percent,
                (25, 400, 5),
                "%",
                "Enlarge isometric tiles to inspect blocks, or zoom out to see more landscape.",
            ),
            Control::choice(
                "detail",
                "Detail",
                value.detail,
                &["Terrain", "Landmarks", "Landscape", "All features"],
                "Choose the visible decoration density. Terrain and structure positions remain stable across detail levels.",
            ),
            Control::slider(
                "pan_speed",
                "Camera speed",
                value.pan_speed_percent,
                (0, 200, 5),
                "%",
                "Slow continuous movement across the world. Zero freezes the camera; global animation speed also applies.",
            ),
            Control::choice(
                "pan_direction",
                "Camera direction",
                value.pan_direction,
                &["East", "South", "North-east", "South-east"],
                "Choose the direction in world space; the isometric projection keeps both ground axes visible.",
            ),
            Control::choice(
                "color_mode",
                "Dither color",
                value.color_mode,
                &["Black and white", "Texture colors"],
                "Use the selected artwork's colors or monochrome shading. Global density controls dot coverage.",
            ),
            Control::choice(
                "palette",
                "Landscape palette",
                value.palette,
                &[
                    "Original colors",
                    "Rose garden",
                    "Cool mist",
                    "Amber evening",
                ],
                "Keep original texture colors or choose a color tint.",
            ),
            Control::slider(
                "hue",
                "Hue tint",
                value.hue_degrees,
                (0, 360, 5),
                "°",
                "180° is neutral; lower values favor warm tones and higher values favor cool tones.",
            ),
            Control::slider(
                "saturation",
                "Color saturation",
                value.saturation_percent,
                (0, 100, 5),
                "%",
                "Pastel color intensity. Zero makes all material colors gray while retaining their shading.",
            ),
            Control::slider(
                "lightness",
                "Color lightness",
                value.lightness_percent,
                (5, 100, 5),
                "%",
                "Brightness of lit dots in both modes, not the number of dots. Does not replace the global background lightness setting.",
            ),
            Control::slider(
                "vegetation",
                "Vegetation density",
                value.vegetation_percent,
                (0, 200, 5),
                "%",
                "Density of biome-appropriate trees, flowers, crops and other plants. Existing anchor positions remain deterministic.",
            ),
            Control::slider(
                "structures",
                "Structure density",
                value.structures_percent,
                (0, 200, 5),
                "%",
                "Density of villages, ruins and landscape landmarks. Zero retains natural terrain only.",
            ),
            Control::toggle(
                "rivers",
                "Rivers",
                value.rivers,
                "Carve coherent river channels and fill them to their water level.",
            ),
            Control::toggle(
                "ravines",
                "Ravines",
                value.ravines,
                "Open narrow deep fissures exposing stratified rock faces.",
            ),
            Control::toggle(
                "caves",
                "Cave mouths",
                value.caves,
                "Show surface cave openings with dark entrances, not a subterranean camera.",
            ),
        ];
        rows.extend(renderer_rows.into_iter().filter(|row| {
            value.saved_maps.source == WorldSource::Generated
                || !matches!(
                    row.id,
                    "generated_texture_source"
                        | "seed"
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
        let mut next = self.normalized();
        if !next.apply_control(id, value)? {
            return Ok(false);
        }
        let changed = next != *self;
        *self = next;
        Ok(changed)
    }
}

#[cfg(test)]
mod saved_source_tests {
    use super::*;

    #[test]
    fn generated_texture_source_defaults_to_pack_and_persists_java_default_choice() {
        let mut settings = VoxelLandscapeSettings {
            pack_profile: 3,
            pack_path: "/packs/plasticator.zip".into(),
            ..Default::default()
        };
        let source = settings
            .controls()
            .into_iter()
            .find(|row| row.id == "generated_texture_source")
            .expect("generated texture source selector");
        assert_eq!(source.value, ControlValue::Index(0));
        let original = settings.clone();
        assert!(settings
            .set_control("generated_texture_source", ControlValue::Index(2))
            .is_err());
        assert_eq!(settings, original);

        let legacy: VoxelLandscapeSettings = serde_json::from_str(
            r#"{"pack_profile_version":1,"pack_profile":3,"pack_path":"/packs/plasticator.zip"}"#,
        )
        .unwrap();
        assert_eq!(
            legacy
                .controls()
                .into_iter()
                .find(|row| row.id == "generated_texture_source")
                .unwrap()
                .value,
            ControlValue::Index(0)
        );
        assert_eq!(
            settings.set_control("generated_texture_source", ControlValue::Index(1)),
            Ok(true)
        );

        let reloaded: VoxelLandscapeSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();

        assert_eq!(reloaded.pack_profile, 3);
        assert_eq!(reloaded.pack_path, "/packs/plasticator.zip");
        assert_eq!(
            reloaded
                .controls()
                .into_iter()
                .find(|row| row.id == "generated_texture_source")
                .unwrap()
                .value,
            ControlValue::Index(1)
        );
        assert!(reloaded
            .controls()
            .iter()
            .any(|row| row.id == "generated_texture_source"));
        let mut saved = reloaded;
        assert_eq!(
            saved.set_control("world_source", ControlValue::Index(1)),
            Ok(true)
        );
        assert!(!saved
            .controls()
            .iter()
            .any(|row| row.id == "generated_texture_source"));
        assert_eq!(saved.pack_profile, 3);
        assert_eq!(saved.pack_path, "/packs/plasticator.zip");
    }

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
            .set_control("pack_profile", ControlValue::Index(5))
            .unwrap();
        assert!(settings.pack_path.is_empty());
        settings
            .set_control(
                "pack_path",
                ControlValue::Text("/private/faithful.zip".into()),
            )
            .unwrap();
        settings
            .set_control("pack_profile", ControlValue::Index(0))
            .unwrap();
        assert_eq!(settings.pack_path, "/private/goodvibes");
        assert_eq!(settings.pack_mount, 1);
        settings
            .set_control("pack_profile", ControlValue::Index(5))
            .unwrap();
        assert_eq!(settings.pack_path, "/private/faithful.zip");
    }

    #[test]
    fn authored_pack_path_preserves_significant_edge_spaces() {
        let mut settings = VoxelLandscapeSettings::default();
        let authored = " /private/pack with spaces.zip ";
        settings
            .set_control("pack_path", ControlValue::Text(authored.into()))
            .unwrap();
        assert_eq!(settings.pack_path, authored);
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

#[cfg(test)]
mod seed_domain_tests {
    use super::*;

    #[test]
    fn world_seed_text_setter_and_persistence_keep_the_complete_u32_domain() {
        let mut settings = VoxelLandscapeSettings::default();
        let seed_control = settings
            .controls()
            .into_iter()
            .find(|control| control.id == "seed")
            .expect("world seed control");
        assert!(matches!(
            seed_control.kind,
            crate::control::ControlKind::Text { .. }
        ));
        assert!(matches!(seed_control.value, ControlValue::Text(_)));
        for value in [0_u32, i32::MAX as u32, i32::MAX as u32 + 1, u32::MAX] {
            assert_eq!(
                settings.set_control("seed", ControlValue::Text(value.to_string())),
                Ok(true),
            );
            assert_eq!(settings.seed, value);
            let restored: VoxelLandscapeSettings =
                serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
            assert_eq!(restored.seed, value);
            assert_eq!(
                restored
                    .controls()
                    .into_iter()
                    .find(|control| control.id == "seed")
                    .expect("restored world seed control")
                    .value,
                ControlValue::Text(value.to_string()),
            );
        }
        for invalid in ["-1", "1.5", "4294967296", ""] {
            let before = settings.clone();
            assert!(
                settings
                    .set_control("seed", ControlValue::Text(invalid.to_owned()))
                    .is_err(),
                "accepted {invalid:?}",
            );
            assert_eq!(settings, before);
        }
    }
}

#[cfg(test)]
mod pack_migration_tests {
    use super::*;
    use serde_json::json;

    fn custom(path: &str) -> PackSourceSettings {
        PackSourceSettings {
            path: path.into(),
            root: "nested/root".into(),
            mount: 1,
            format_major: 88,
            format_minor: 2,
            edition: 0,
            addon_path: String::new(),
            addon_mount: 1,
            duplicate_last_wins: false,
        }
    }
    fn restored(index: usize, source: &PackSourceSettings) -> VoxelLandscapeSettings {
        let mut settings = VoxelLandscapeSettings::default();
        settings.apply_source_settings(source);
        let mut document = serde_json::to_value(settings).unwrap();
        document
            .as_object_mut()
            .unwrap()
            .remove("pack_profile_version");
        document["pack_profile"] = json!(index);
        serde_json::from_value(document).unwrap()
    }
    fn identity(settings: &VoxelLandscapeSettings) -> &'static str {
        super::super::pack_profiles::FULL_PACKS[settings.pack_profile].id
    }

    #[test]
    fn every_legacy_public_selection_retains_identity_and_exact_active_source() {
        let source = custom(" /packs/authored path.zip ");
        for (old, id) in [
            "goodvibes",
            "programmerart",
            "textureless",
            "plasticator",
            "pixelperfectionce",
            "faithful32",
            "faithful64",
            "antumbra",
        ]
        .into_iter()
        .enumerate()
        {
            let settings = restored(old + 3, &source);
            assert_eq!(identity(&settings), id);
            assert_eq!(settings.pack_profile_version, PACK_PROFILE_VERSION);
            assert_eq!(settings.source_settings(), source);
            assert_eq!(settings.normalized(), settings);
            let encoded = serde_json::to_value(&settings).unwrap();
            assert_eq!(encoded["pack_profile_version"], json!(1));
            let reloaded: VoxelLandscapeSettings = serde_json::from_value(encoded).unwrap();
            assert_eq!(reloaded, settings);
        }
    }

    #[test]
    fn missing_profile_uses_goodvibes_in_both_schemas_without_discarding_its_path() {
        for mut document in [json!({}), json!({"pack_profile_version": 1})] {
            document["pack_path"] = json!(" /packs/implicit-goodvibes ");
            let settings: VoxelLandscapeSettings = serde_json::from_value(document).unwrap();
            assert_eq!(identity(&settings), "goodvibes");
            assert_eq!(settings.pack_path, " /packs/implicit-goodvibes ");
            assert!(settings.pack_custom_sources.is_empty());
            assert!(settings.pack_retired_sources.is_empty());
        }
        let empty: VoxelLandscapeSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, VoxelLandscapeSettings::default());
    }

    #[test]
    fn retired_paths_are_archived_and_never_relabelled_as_goodvibes() {
        for old in 0..3 {
            let source = custom("/packs/retired");
            let settings = restored(old, &source);
            let id = super::super::pack_profiles::RETIRED_PACK_IDS[old];
            assert_eq!(identity(&settings), "goodvibes");
            assert_eq!(
                settings.source_settings(),
                VoxelLandscapeSettings::default().source_settings()
            );
            assert_eq!(settings.pack_custom_sources[id], source);
            assert!(!settings.pack_custom_sources.contains_key("goodvibes"));
            assert_eq!(settings.normalized(), settings);
        }
    }

    #[test]
    fn conflicting_retired_active_source_preserves_all_map_entries_and_both_sources() {
        for old in 0..3 {
            let id = super::super::pack_profiles::RETIRED_PACK_IDS[old];
            let active = custom("/packs/retired-active");
            let mut raw = VoxelLandscapeSettings {
                pack_profile_version: 0,
                pack_profile: old,
                ..Default::default()
            };
            raw.apply_source_settings(&active);
            raw.pack_custom_sources = BTreeMap::from([
                (id.into(), custom("/packs/retired-saved")),
                ("goodvibes".into(), custom("/packs/goodvibes")),
                ("faithful32".into(), custom("/packs/faithful")),
                ("unknown-future-id".into(), custom("/packs/preserved")),
            ]);
            let before = raw.pack_custom_sources.clone();
            let migrated = raw.normalized();
            assert_eq!(migrated.pack_custom_sources, before);
            assert_eq!(migrated.source_settings(), before["goodvibes"]);
            assert_eq!(
                migrated.pack_retired_sources,
                vec![RetiredPackSource {
                    profile: id.into(),
                    source: active
                }]
            );
            assert_eq!(migrated.normalized(), migrated);
            let reloaded: VoxelLandscapeSettings =
                serde_json::from_value(serde_json::to_value(&migrated).unwrap()).unwrap();
            assert_eq!(reloaded, migrated);
        }
    }

    #[test]
    fn blank_retired_path_still_preserves_authored_mount_metadata() {
        let mut source = custom("");
        source.addon_path = "/packs/retired-addon.zip".into();
        let migrated = restored(0, &source);
        assert_eq!(migrated.pack_custom_sources["jicklus"], source);
        assert_eq!(
            migrated.source_settings(),
            VoxelLandscapeSettings::default().source_settings()
        );
    }

    #[test]
    fn current_schema_is_never_shifted_and_invalid_high_values_keep_antumbra_clamp() {
        for index in 0..8 {
            let settings = VoxelLandscapeSettings {
                pack_profile: index,
                ..Default::default()
            };
            let reloaded: VoxelLandscapeSettings =
                serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
            assert_eq!(reloaded, settings);
        }
        for version in [0, 1] {
            for index in [11, usize::MAX] {
                let settings: VoxelLandscapeSettings = serde_json::from_value(json!({
                    "pack_profile_version": version, "pack_profile": index,
                    "pack_path": "/packs/antumbra.zip"
                }))
                .unwrap();
                assert_eq!(identity(&settings), "antumbra");
                assert_eq!(settings.pack_path, "/packs/antumbra.zip");
                assert_eq!(settings.pack_profile_version, 1);
            }
        }
        for index in [8, 9, 10] {
            let settings: VoxelLandscapeSettings = serde_json::from_value(json!({
                "pack_profile_version": 1, "pack_profile": index
            }))
            .unwrap();
            assert_eq!(identity(&settings), "antumbra");
        }
    }

    #[test]
    fn unsupported_versions_and_malformed_indices_are_rejected() {
        for document in [
            json!({"pack_profile_version": 2}),
            json!({"pack_profile_version": "1"}),
            json!({"pack_profile_version": null}),
            json!({"pack_profile": -1}),
            json!({"pack_profile": 1.5}),
            json!({"pack_profile": null}),
        ] {
            assert!(serde_json::from_value::<VoxelLandscapeSettings>(document).is_err());
        }
    }

    #[test]
    fn raw_legacy_controls_migrate_before_same_numeric_selection_or_source_edit() {
        let private = custom("/packs/private");
        let mut raw = VoxelLandscapeSettings {
            pack_profile_version: 0,
            pack_profile: 1,
            ..Default::default()
        };
        raw.apply_source_settings(&private);
        raw.set_control("pack_profile", ControlValue::Index(1))
            .unwrap();
        assert_eq!(identity(&raw), "programmerart");
        assert!(raw.pack_path.is_empty());
        assert_eq!(raw.pack_custom_sources["f8thful"], private);
        let mut raw = VoxelLandscapeSettings {
            pack_profile_version: 0,
            pack_profile: 0,
            ..Default::default()
        };
        raw.apply_source_settings(&private);
        raw.set_control("pack_path", ControlValue::Text("/packs/goodvibes".into()))
            .unwrap();
        assert_eq!(identity(&raw), "goodvibes");
        assert_eq!(raw.pack_path, "/packs/goodvibes");
        assert_eq!(raw.pack_custom_sources["jicklus"], private);
    }

    #[test]
    fn invalid_edits_are_atomic_even_when_migration_is_pending() {
        let raw = VoxelLandscapeSettings {
            pack_profile_version: 0,
            pack_profile: 2,
            pack_path: "/packs/private".into(),
            ..Default::default()
        };
        for (id, value) in [
            ("pack_profile", ControlValue::Index(8)),
            ("pack_format", ControlValue::Text("8.invalid".into())),
            ("pack_path", ControlValue::Text("bad\0path".into())),
        ] {
            let mut settings = raw.clone();
            assert!(settings.set_control(id, value).is_err());
            assert_eq!(settings, raw);
        }
        let mut settings = raw.clone();
        assert_eq!(
            settings.set_control("unknown", ControlValue::Index(0)),
            Ok(false)
        );
        assert_eq!(settings, raw);
    }

    #[test]
    fn legacy_public_override_map_survives_reload_and_switching() {
        let mut raw = VoxelLandscapeSettings {
            pack_profile_version: 0,
            pack_profile: 3,
            ..Default::default()
        };
        let goodvibes = custom("/packs/goodvibes");
        let faithful = custom("/packs/faithful");
        raw.apply_source_settings(&goodvibes);
        raw.pack_custom_sources
            .insert("faithful32".into(), faithful.clone());
        raw.pack_custom_sources
            .insert("whimscape".into(), custom("/packs/retained"));
        let mut migrated: VoxelLandscapeSettings =
            serde_json::from_value(serde_json::to_value(raw).unwrap()).unwrap();
        migrated
            .set_control("pack_profile", ControlValue::Index(5))
            .unwrap();
        assert_eq!(migrated.source_settings(), faithful);
        migrated
            .set_control("pack_profile", ControlValue::Index(0))
            .unwrap();
        assert_eq!(migrated.source_settings(), goodvibes);
        assert!(migrated.pack_custom_sources.contains_key("whimscape"));
    }
}

#[cfg(test)]
mod pack_control_inventory_tests {
    use super::*;
    use crate::control::ControlKind;

    #[test]
    fn both_source_modes_expose_exactly_the_eight_retained_pack_labels() {
        for saved in [false, true] {
            let mut settings = VoxelLandscapeSettings::default();
            if saved {
                settings
                    .set_control("world_source", ControlValue::Index(1))
                    .unwrap();
            }
            let row = settings
                .controls()
                .into_iter()
                .find(|row| row.id == "pack_profile")
                .unwrap();
            let ControlKind::Choice { options } = row.kind else {
                panic!("pack profile must remain a choice");
            };
            assert_eq!(
                options,
                super::super::pack_profiles::FULL_PACKS
                    .iter()
                    .map(|profile| profile.name)
                    .collect::<Vec<_>>()
            );
            assert_eq!(row.value, ControlValue::Index(0));
        }
    }
}
