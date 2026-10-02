//! Controls for the original isometric block landscape. World identity is
//! independent of camera, palette and detail, so changing appearance never
//! regenerates a different landscape.
use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoxelLandscapeSettings {
    pub seed: u32,
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
}
impl Default for VoxelLandscapeSettings {
    fn default() -> Self {
        Self {
            seed: 71839,
            zoom_percent: 150,
            detail: 2,
            pan_speed_percent: 25,
            pan_direction: 0,
            color_mode: 1,
            palette: 0,
            hue_degrees: 180,
            saturation_percent: 65,
            lightness_percent: 90,
            vegetation_percent: 100,
            structures_percent: 100,
            rivers: true,
            ravines: true,
            caves: true,
        }
    }
}
impl SceneSettings for VoxelLandscapeSettings {
    fn normalized(&self) -> Self {
        Self {
            zoom_percent: self.zoom_percent.clamp(25, 400),
            detail: self.detail.min(3),
            pan_speed_percent: self.pan_speed_percent.clamp(0, 200),
            pan_direction: self.pan_direction.min(3),
            color_mode: self.color_mode.min(1),
            palette: self.palette.min(3),
            hue_degrees: self.hue_degrees.clamp(0, 360),
            saturation_percent: self.saturation_percent.clamp(0, 100),
            lightness_percent: self.lightness_percent.clamp(5, 100),
            vegetation_percent: self.vegetation_percent.clamp(0, 200),
            structures_percent: self.structures_percent.clamp(0, 200),
            ..self.clone()
        }
    }
    fn controls(&self) -> Vec<Control> {
        let value = self.normalized();
        vec![
            Control::text("seed", "World seed", &value.seed.to_string(), "0–4294967295", "The same seed recreates the same world, including structures. Camera and color changes preserve terrain."),
            Control::slider("zoom", "Tile zoom", value.zoom_percent, (25,400,5), "%", "Enlarge isometric tiles to inspect blocks, or zoom out to see more landscape."),
            Control::choice("detail", "Detail", value.detail, &["Terrain", "Landmarks", "Landscape", "All features"], "Choose the visible decoration density. Terrain and structure positions remain stable across detail levels."),
            Control::slider("pan_speed", "Camera speed", value.pan_speed_percent, (0,200,5), "%", "Slow continuous movement across the seeded world. Zero freezes the camera; global animation speed also applies."),
            Control::choice("pan_direction", "Camera direction", value.pan_direction, &["East", "South", "North-east", "South-east"], "Choose the direction in world space; the isometric projection keeps both ground axes visible."),
            Control::choice("color_mode", "Dither color", value.color_mode, &["Black and white", "Pastel color"], "Both modes use the same landscape tone curve and shared Braille dithering. Monochrome supplies gray dots; pastel supplies material colors. Global density still controls dot coverage."),
            Control::choice("palette", "Landscape palette", value.palette, &["Natural pastel", "Rose garden", "Cool mist", "Amber evening"], "Shift the landscape palette while retaining material distinctions."),
            Control::slider("hue", "Hue tint", value.hue_degrees, (0,360,5), "°", "180° is neutral; lower values favor warm tones and higher values favor cool tones."),
            Control::slider("saturation", "Color saturation", value.saturation_percent, (0,100,5), "%", "Pastel color intensity. Zero makes all material colors gray while retaining their shading."),
            Control::slider("lightness", "Color lightness", value.lightness_percent, (5,100,5), "%", "Brightness of lit dots in both modes, not the number of dots. Does not replace the global background lightness setting."),
            Control::slider("vegetation", "Vegetation density", value.vegetation_percent, (0,200,5), "%", "Density of biome-appropriate trees, flowers, crops and other plants. Existing anchor positions remain deterministic."),
            Control::slider("structures", "Structure density", value.structures_percent, (0,200,5), "%", "Density of villages, ruins and landscape landmarks. Zero retains natural terrain only."),
            Control::toggle("rivers", "Rivers", value.rivers, "Carve coherent river channels and fill them to their water level."),
            Control::toggle("ravines", "Ravines", value.ravines, "Open narrow deep fissures exposing stratified rock faces."),
            Control::toggle("caves", "Cave mouths", value.caves, "Show surface cave openings with dark entrances, not a subterranean camera."),
        ]
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let previous = self.clone();
        match id {
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
            "detail" | "pan_direction" | "color_mode" | "palette" => {
                let index = control::index(&value).ok_or("Expected a choice")?;
                let limit = if id == "color_mode" { 2 } else { 4 };
                if index >= limit {
                    return Err("Unknown choice".into());
                }
                match id {
                    "detail" => self.detail = index,
                    "pan_direction" => self.pan_direction = index,
                    "color_mode" => self.color_mode = index,
                    "palette" => self.palette = index,
                    _ => return Ok(false),
                }
            }
            "rivers" | "ravines" | "caves" => {
                let enabled = control::boolean(&value).ok_or("Expected on or off")?;
                match id {
                    "rivers" => self.rivers = enabled,
                    "ravines" => self.ravines = enabled,
                    "caves" => self.caves = enabled,
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != previous)
    }
}
