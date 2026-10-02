use super::super::*;
use ilium_ambient::ControlValue;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn wikipedia_is_live_and_never_uses_a_precomputed_article_loop() {
    let mut settings = AnimationSettings {
        kind: AnimationKind::Wikipedia,
        ..Default::default()
    };
    assert!(!settings.enabled);
    assert!(settings.kind.is_live_only());
    assert!(!settings.kind.is_ambient());
    assert!(!settings.uses_loop_cache());
    settings
        .set_scene_control("wiki_render_mode", ControlValue::Index(1))
        .unwrap();
    settings
        .set_scene_control("wiki_scroll", ControlValue::Number(0))
        .unwrap();
    let value = serde_json::to_value(&settings).unwrap();
    assert_eq!(value["kind"], "wikipedia");
    assert_eq!(value["wikipedia"]["render_mode"], "text");
    assert_eq!(value["wikipedia"]["scroll_tenths"], 0);
    assert_eq!(
        serde_json::from_value::<AnimationSettings>(value).unwrap(),
        settings
    );
}

#[test]
fn wikipedia_help_and_controls_share_the_stable_catalog() {
    let settings = AnimationSettings {
        kind: AnimationKind::Wikipedia,
        ..Default::default()
    };
    let context = crate::animation_rows::RowContext {
        scene_uses_cell_colors: true,
        ..Default::default()
    };
    let rows = crate::animation_rows::rows(&settings, &context);
    assert!(!rows.contains(&crate::animation_rows::AnimationRow::Common("playback")));
    assert!(rows.contains(&crate::animation_rows::AnimationRow::SceneStatus));
    for hidden in [
        "speed",
        "density",
        "dither",
        "lightness",
        "hue",
        "saturation",
    ] {
        assert!(!rows.contains(&crate::animation_rows::AnimationRow::Common(hidden)));
    }
    assert!(!rows.contains(&crate::animation_rows::AnimationRow::Common("hue")));
    assert_eq!(
        crate::animation_rows::AnimationRow::Scene(AnimationKind::Wikipedia).help_id(&[]),
        "AN-54"
    );
}

#[test]
fn wikipedia_controls_survive_actual_project_save_reload_without_affecting_another_project() {
    // Synthetic isolated projects exercise the real YAML writer and loader,
    // including the flattened hosted settings and retained authored metadata.
    let project = tempfile::tempdir().unwrap();
    let other_project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join(".ilium")).unwrap();
    let path = project.path().join(".ilium/config.yaml");
    std::fs::write(
        &path,
        "project name: Authored project\nauthored metadata:\n  retained: exactly\n",
    )
    .unwrap();
    let mut settings = AnimationSettings {
        enabled: true,
        kind: AnimationKind::Wikipedia,
        ..Default::default()
    };
    for (id, value) in [
        ("wiki_render_mode", ControlValue::Index(1)),
        ("wiki_greyscale", ControlValue::Bool(false)),
        ("wiki_palette", ControlValue::Index(1)),
        ("wiki_zoom", ControlValue::Number(175)),
        ("wiki_scroll", ControlValue::Number(0)),
        ("wiki_dwell", ControlValue::Number(17)),
        ("wiki_hue", ControlValue::Number(120)),
        ("wiki_saturation", ControlValue::Number(145)),
        ("wiki_lightness", ControlValue::Number(72)),
    ] {
        assert!(settings.set_scene_control(id, value).unwrap());
    }
    crate::project_config::set_animation(project.path(), settings.clone()).unwrap();
    let reloaded = crate::project_config::load(project.path()).unwrap();
    assert_eq!(reloaded.animation, settings.normalized());
    assert_eq!(reloaded.project_name.as_deref(), Some("Authored project"));
    let saved: serde_norway::Value =
        serde_norway::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        saved["authored metadata"]["retained"].as_str(),
        Some("exactly")
    );
    assert_eq!(
        crate::project_config::load(other_project.path())
            .unwrap()
            .animation,
        AnimationSettings::default()
    );
    settings
        .set_scene_control("wiki_render_mode", ControlValue::Index(0))
        .unwrap();
    crate::project_config::set_animation(project.path(), settings.clone()).unwrap();
    let second_reload = crate::project_config::load(project.path())
        .unwrap()
        .animation;
    assert_eq!(
        second_reload.wikipedia.render_mode,
        super::RenderMode::Braille
    );
    assert_eq!(second_reload.wikipedia.zoom_percent, 175);
    assert_eq!(second_reload.wikipedia.scroll_tenths, 0);
}

#[test]
fn one_prepared_article_survives_background_and_preview_frames_until_release() {
    let mut settings = AnimationSettings {
        enabled: true,
        kind: AnimationKind::Wikipedia,
        ..Default::default()
    };
    settings.wikipedia.render_mode = super::RenderMode::Text;
    let document = Arc::new(
        ilium_wikipedia::parse_article(
            "Current article",
            "https://en.wikipedia.org/wiki/Current_article",
            "2026-10-02",
            "<p>Current article body with <b>bold</b> words.</p>",
        )
        .unwrap(),
    );
    let mut frame = AnimationFrame::default();
    frame.inject_wikipedia_document_for_test(Arc::clone(&document), &settings.wikipedia, 80);
    frame.render(&settings, 80, 24, Duration::ZERO);
    let source_status = frame.status().unwrap();
    assert!(source_status.contains(&document.url));
    assert!(frame.article_symbol(0, 0).is_none());
    assert!(frame.article_symbol(1, 1).is_some());
    assert_eq!(frame.wikipedia_layout_count_for_test(), Some(1));
    frame.render(&settings, 80, 24, Duration::ZERO);
    assert_eq!(frame.wikipedia_layout_count_for_test(), Some(1));
    assert_eq!(frame.status().unwrap(), source_status);
    frame.release_hosts();
    assert!(frame.article_symbol(1, 1).is_none());
    assert!(!frame.is_wikipedia());
}
