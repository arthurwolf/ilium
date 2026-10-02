use super::*;
use ilium_ambient::control::{ControlValue, SceneSettings};

#[test]
fn wikipedia_defaults_and_ranges_are_explicit() {
    let settings = WikipediaSettings::default();
    assert_eq!(settings.render_mode, RenderMode::Braille);
    assert!(settings.greyscale);
    assert_eq!(settings.scroll_tenths, 2);
    assert_eq!(settings.zoom_percent, 100);
    let normalized = WikipediaSettings {
        zoom_percent: 0,
        scroll_tenths: 999,
        lightness_percent: 0,
        ..settings
    }
    .normalized();
    assert_eq!(normalized.zoom_percent, 50);
    assert_eq!(normalized.scroll_tenths, 100);
    assert_eq!(normalized.lightness_percent, 10);
}

#[test]
fn wikipedia_controls_round_trip_and_presets_keep_custom_sliders() {
    let mut settings = WikipediaSettings::default();
    assert!(settings
        .set_control("wiki_render_mode", ControlValue::Index(1))
        .unwrap());
    assert_eq!(settings.render_mode, RenderMode::Text);
    assert!(!settings.controls().iter().any(|c| c.id == "wiki_zoom"));
    settings
        .set_control("wiki_render_mode", ControlValue::Index(0))
        .unwrap();
    settings
        .set_control("wiki_palette", ControlValue::Index(1))
        .unwrap();
    assert_eq!(settings.palette, Palette::Pastel);
    settings
        .set_control("wiki_hue", ControlValue::Number(30))
        .unwrap();
    settings
        .set_control("wiki_saturation", ControlValue::Number(140))
        .unwrap();
    settings
        .set_control("wiki_lightness", ControlValue::Number(70))
        .unwrap();
    settings
        .set_control("wiki_scroll", ControlValue::Number(0))
        .unwrap();
    assert_eq!(settings.scroll_tenths, 0);
    let saved = serde_json::to_string(&settings).unwrap();
    assert_eq!(
        serde_json::from_str::<WikipediaSettings>(&saved).unwrap(),
        settings
    );
    assert!(!settings
        .set_control("unknown", ControlValue::Number(10))
        .unwrap());
    assert!(!settings
        .set_control("wiki_zoom", ControlValue::Bool(true))
        .unwrap());
}

#[test]
fn wiki_color_mode_and_greyscale_apply_to_images_and_text() {
    let gray = WikipediaSettings::default().color([50, 100, 200]);
    assert_eq!(gray[0], gray[1]);
    assert_eq!(gray[1], gray[2]);
    let settings = WikipediaSettings {
        greyscale: false,
        palette: Palette::Pastel,
        ..Default::default()
    };
    let color = settings.color([50, 100, 200]);
    assert_ne!(color[0], color[2]);
    assert_ne!(
        settings.color([50, 100, 200]),
        WikipediaSettings {
            hue_degrees: 180,
            ..settings
        }
        .color([50, 100, 200])
    );
}
