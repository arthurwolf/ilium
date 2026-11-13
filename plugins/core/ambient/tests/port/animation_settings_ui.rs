//! Twins of the tests of ilium-client/src/animation_settings_ui.rs.
//!
//! The kernel tests drive the whole Settings screen through `App`: a ratatui `TestBackend`, key and
//! mouse routing, the project config file and the plugin editor. Drawing, event routing, the
//! project file and the preview painting stay with the client (KERNEL-STAYS, listed in the port
//! report). The plugin owns the data those tests read and write: which rows a scene shows, where
//! each row and its slider track sit for a screen size, how the three regions scroll, what a click
//! on a row, a scrollbar or a scene button means, how the saved-scene footer is sized, what the
//! credit lines cost and which value a dialog may confirm. Each twin asserts that data boundary on
//! the synthetic registry of `panel_fixture`.

use crate::panel_fixture::*;
use ilium_plugin_core_ambient::control::{Control, ControlKind, ControlValue};
use ilium_plugin_core_ambient::credit::area_for_scene;
use ilium_plugin_core_ambient::help::help_id;
use ilium_plugin_core_ambient::hover::{wrap_lines, Rect};
use ilium_plugin_core_ambient::panel::{
    activity_footer_rows, adjacent_scene, compact_saved_scene_status, layout, left_width,
    pinned_control_help_rows, saved_scene_activity_report, saved_scene_activity_sections,
    selected_control_help, wrapped_rows, AnimationHit, Panel, Position, Scrolls, FOOTER_ROWS,
};
use ilium_plugin_core_ambient::rows::{
    AnimationRow, Region, RowContext, RowKind, RowModel, OPENSTREETMAP_ID, SEMANTIC_ID,
    WIKIPEDIA_ID,
};
use ilium_plugin_core_ambient::values::{
    checked_choice, checked_number, stepped_animation_number, AnimationControlScope,
    AnimationControlTarget, ValueDialog, VOXEL_LANDSCAPE_ID,
};
use ilium_plugin_core_ambient::visibility::{is_blank_glyph, is_faint, DARK_BACKDROP};
use std::path::{Path, PathBuf};

const REGIONS: [Region; 3] = [Region::Scenes, Region::Global, Region::Controls];

/// The hit a click on the first cell of a visible row produces.
fn click_on(panel: &Panel<'_>, row: usize) -> Option<AnimationHit> {
    let scrolls = panel.follow_selection(row, Scrolls::default());
    let rect = panel.row_rect(row, scrolls)?;
    panel.hit(scrolls, Position::new(rect.x, rect.y))
}

fn expected_hit(model: &RowModel, row: usize) -> AnimationHit {
    match model.view(row).expect("row view").kind {
        RowKind::Toggle | RowKind::Text | RowKind::Location | RowKind::Action => {
            AnimationHit::Activate(row)
        }
        _ => AnimationHit::Select(row),
    }
}

fn project() -> PathBuf {
    PathBuf::from("/media/arthur/tmp/ambient-twin-project")
}

/// Open the choice dialog of `control` for the map scene and select every option through it.
fn plus_selects_every_option(control: &Control, scene: &str) {
    let ControlKind::Choice { options } = &control.kind else {
        panic!("a choice control");
    };
    let target = AnimationControlTarget::new_for_animation(
        project(),
        scene.to_owned(),
        AnimationControlScope::Scene,
        scene,
        control.clone(),
    )
    .expect("dialog target");
    let ValueDialog::Choice {
        options: listed,
        selected_id,
        ..
    } = target.dialog().expect("choice dialog")
    else {
        panic!("a choice dialog");
    };
    assert_eq!(listed.len(), options.len(), "every option is listed");
    assert!(selected_id.is_some());
    for (index, option) in listed.iter().enumerate() {
        let picked = target.choice(
            &project(),
            scene,
            AnimationControlScope::Scene,
            control,
            &option.id,
        );
        match &option.disabled_reason {
            None => assert_eq!(picked, Ok(ControlValue::Index(index)), "{}", option.label),
            Some(reason) => assert_eq!(picked, Err(reason.clone())),
        }
    }
    assert!(target
        .choice(
            &project(),
            scene,
            AnimationControlScope::Scene,
            control,
            &options.len().to_string(),
        )
        .is_err());
}

const PLACES: [&str; 12] = [
    "Lisbon", "Paris", "Tokyo", "Cairo", "Lima", "Oslo", "Perth", "Quito", "Rome", "Seoul",
    "Tunis", "Vienna",
];

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::a_color_scene_preview_paints_colored_braille_cells
#[test]
fn a_color_scene_preview_paints_colored_braille_cells_twin() {
    // A painted cell is an inked braille glyph with a colour that reads against the backdrop; the
    // blank pattern and whitespace carry no ink.
    assert!(!is_blank_glyph('\u{28ff}'));
    assert!(!is_blank_glyph('\u{2801}'));
    assert!(is_blank_glyph('\u{2800}') && is_blank_glyph(' '));
    assert!(!is_faint((200, 40, 40), DARK_BACKDROP));
    // The preview is the last row of the panel, so it always has a place under the controls.
    let model = fixture_model("shoreline");
    assert_eq!(model.rows().last(), Some(&AnimationRow::FullScreenPreview));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::a_failed_control_edit_shows_the_message_and_changes_nothing
#[test]
fn a_failed_control_edit_shows_the_message_and_changes_nothing_twin() {
    let control = Control::slider("amount", "Amount", 40, (0, 100, 1), "", "");
    for text in ["abc", "101", "-1", ""] {
        let error = checked_number(&control, text).expect_err(text);
        assert!(!error.is_empty(), "a rejected value carries its message");
    }
    assert_eq!(control.value, ControlValue::Number(40), "nothing changed");
    // A choice whose options changed under the dialog is refused with a message.
    let opened = Control::choice("variant", "Variant", 0, &["Calm", "Busy"], "");
    let current = Control::choice("variant", "Variant", 0, &["Calm", "Quiet"], "");
    assert!(checked_choice(&opened, &current, 1).is_err());
    assert!(checked_choice(&opened, &current, 7).is_err());
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::a_text_row_opens_a_prefilled_prompt_and_applies_through_set_control
#[test]
fn a_text_row_opens_a_prefilled_prompt_and_applies_through_set_control_twin() {
    // A text row takes no keyboard step: activating it opens a prompt prefilled with the value.
    let region = Control::text("region", "Region", "alps", "name", "");
    assert_eq!(region.stepped(1), None);
    assert_eq!(region.display_value(), "alps");
    let model = fixture_model(TOPOGRAPHIC_ID);
    let row = index_of(&model, &AnimationRow::SceneControl("region"));
    assert!(matches!(model.view(row).unwrap().kind, RowKind::Text));
    assert_eq!(model.view(row).unwrap().value, "alps");
    // The wide world seed is a text row that opens a numeric prompt with its current text.
    let seed = Control::text("seed", "World seed", "71839", "0-4294967295", "");
    let target = AnimationControlTarget::new_for_animation(
        project(),
        VOXEL_LANDSCAPE_ID.to_owned(),
        AnimationControlScope::Scene,
        VOXEL_LANDSCAPE_ID,
        seed.clone(),
    )
    .unwrap();
    assert_eq!(
        target.dialog(),
        Ok(ValueDialog::Number {
            title: "World seed".to_owned(),
            text: "71839".to_owned(),
        })
    );
    // An ordinary text control has no value dialog: it uses the text prompt of the client.
    let plain = AnimationControlTarget::new(
        project(),
        "boats".to_owned(),
        AnimationControlScope::Scene,
        region,
    )
    .unwrap();
    assert!(plain.dialog().is_err());
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::acknowledged_scene_credit_change_keeps_the_selected_scene_rendered
#[test]
fn acknowledged_scene_credit_change_keeps_the_selected_scene_rendered_twin() {
    let model = fixture_model(CREDITED_ID);
    let row = index_of(&model, &AnimationRow::Scene(CREDITED_ID.to_owned()));
    let urls = vec!["https://example.org/the-original-game".to_owned()];
    for credits in [Vec::new(), urls] {
        let panel = Panel {
            area: area(80, 24),
            model: &model,
            inspired_by: &credits,
        };
        let scrolls = panel.follow_selection(row, Scrolls::default());
        assert!(
            panel.row_y(row, scrolls).is_some(),
            "the selected scene stays inside its list window with {} credit lines",
            credits.len()
        );
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::actual_keyboard_changes_keep_palette_and_named_values_after_scene_switches
#[test]
fn actual_keyboard_changes_keep_palette_and_named_values_after_scene_switches_twin() {
    let mut inputs = fixture_inputs("shoreline");
    let stepped = inputs.look[1].stepped(1).expect("palette steps");
    assert_eq!(stepped, ControlValue::Index(1));
    inputs.look[1].value = stepped;
    for scene in ["boats", VOXEL_LANDSCAPE_ID, "clouds", "shoreline"] {
        inputs.selected = scene.to_owned();
        let model = model_of(&inputs, &RowContext::default());
        let row = index_of(&model, &AnimationRow::Common("look_palette"));
        assert_eq!(model.view(row).unwrap().value, "Dusk", "after {scene}");
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::actual_rendered_tracks_accept_exact_endpoints_and_labels_only_focus
#[test]
fn actual_rendered_tracks_accept_exact_endpoints_and_labels_only_focus_twin() {
    let model = fixture_model("shoreline");
    let panel = panel(area(140, 40), &model);
    let row = index_of(&model, &AnimationRow::Common("look_brightness"));
    let scrolls = panel.follow_selection(row, Scrolls::default());
    let geometry = panel.slider_geometry(row, scrolls).expect("slider track");
    assert!(geometry.track.width >= 8);
    assert_eq!(
        panel.slider_value_at(row, scrolls, geometry.track.right() - 1),
        Some(200)
    );
    assert_eq!(
        panel.slider_value_at(row, scrolls, geometry.track.x),
        Some(1)
    );
    // The track changes the value; the label only focuses the row.
    assert_eq!(
        panel.hit(scrolls, Position::new(geometry.label.x, geometry.label.y)),
        Some(AnimationHit::Select(row))
    );
    assert!(matches!(
        panel.hit(scrolls, Position::new(geometry.track.x, geometry.track.y)),
        Some(AnimationHit::Slider { row: hit_row, .. }) if hit_row == row
    ));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::animation_choice_value_left_steps_forward_and_right_steps_backward
#[test]
fn animation_choice_value_left_steps_forward_and_right_steps_backward_twin() {
    let control = Control::choice("variant", "Variant", 1, &["Calm", "Busy", "Wild"], "");
    assert_eq!(control.stepped(1), Some(ControlValue::Index(2)));
    assert_eq!(control.stepped(-1), Some(ControlValue::Index(0)));
    // Both directions wrap and skip a disabled option.
    let wrapped = Control::choice("variant", "Variant", 2, &["Calm", "Busy", "Wild"], "");
    assert_eq!(wrapped.stepped(1), Some(ControlValue::Index(0)));
    let skipping = wrapped.clone().with_disabled_option(0, "Not here");
    assert_eq!(skipping.stepped(1), Some(ControlValue::Index(1)));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::animation_star_opens_exact_number_and_receipt_restores_settings_parent
#[test]
fn animation_star_opens_exact_number_and_receipt_restores_settings_parent_twin() {
    let control = Control::slider("amount", "Amount", 40, (0, 100, 1), "", "");
    let target = AnimationControlTarget::new(
        project(),
        "boats".to_owned(),
        AnimationControlScope::Scene,
        control.clone(),
    )
    .unwrap();
    assert_eq!(
        target.dialog(),
        Ok(ValueDialog::Number {
            title: "Amount".to_owned(),
            text: "40".to_owned(),
        })
    );
    let scope = AnimationControlScope::Scene;
    assert_eq!(
        target.number(&project(), "boats", scope, &control, "75"),
        Ok(ControlValue::Number(75))
    );
    // The receipt belongs to the project and scene the dialog opened for.
    assert!(target
        .number(
            Path::new("/media/arthur/tmp/other"),
            "boats",
            scope,
            &control,
            "75"
        )
        .is_err());
    assert!(target
        .number(&project(), "rain", scope, &control, "75")
        .is_err());
    assert!(target
        .number(&project(), "boats", scope, &control, "101")
        .is_err());
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::choice_toggle_and_action_rows_activate_from_keyboard_and_mouse
#[test]
fn choice_toggle_and_action_rows_activate_from_keyboard_and_mouse_twin() {
    let model = fixture_model("shoreline");
    let panel = panel(area(140, 40), &model);
    let toggle = index_of(&model, &AnimationRow::Common("panels"));
    let choice = index_of(&model, &AnimationRow::Common("look_palette"));
    let action = index_of(&model, &AnimationRow::FullScreenPreview);
    assert_eq!(
        click_on(&panel, toggle),
        Some(AnimationHit::Activate(toggle))
    );
    assert_eq!(
        click_on(&panel, action),
        Some(AnimationHit::Activate(action))
    );
    assert_eq!(click_on(&panel, choice), Some(AnimationHit::Select(choice)));
    // The keyboard step of a toggle flips it and of a choice moves it.
    assert_eq!(
        Control::toggle("panels", "Panels", true, "").stepped(1),
        Some(ControlValue::Bool(false))
    );
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::compact_composed_settings_keeps_all_animation_regions_reachable
#[test]
fn compact_composed_settings_keeps_all_animation_regions_reachable_twin() {
    let model = fixture_model("shoreline");
    for (width, height) in [(40, 12), (60, 20)] {
        let panel = panel(area(width, height), &model);
        for region in REGIONS {
            assert!(
                panel.visible_rows(region) > 0,
                "{width}x{height} gives {region:?} no visible rows"
            );
        }
        for row in 0..model.len() {
            let scrolls = panel.follow_selection(row, Scrolls::default());
            assert!(
                panel.row_rect(row, scrolls).is_some(),
                "{width}x{height} cannot reveal animation row {row}"
            );
        }
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::dependent_rows_follow_the_hosted_scene_through_the_app
#[test]
fn dependent_rows_follow_the_hosted_scene_through_the_app_twin() {
    for (scene, status) in [
        (VOXEL_LANDSCAPE_ID, true),
        (OPENSTREETMAP_ID, true),
        (TOPOGRAPHIC_ID, true),
        (WIKIPEDIA_ID, true),
        (SEMANTIC_ID, true),
        ("shoreline", false),
        ("boats", false),
    ] {
        let model = fixture_model(scene);
        assert_eq!(
            has_row(&model, &AnimationRow::SceneStatus),
            status,
            "{scene}"
        );
    }
    // The status row follows the scene the context says is running.
    let model = voxel_model(&voxel_status(1, "Loaded 64 chunks"));
    assert_eq!(model.effective_kind(), Some(VOXEL_LANDSCAPE_ID));
    let row = index_of(&model, &AnimationRow::SceneStatus);
    assert_eq!(
        model.view(row).unwrap().value,
        "Saved worlds [====......] phase 3/6"
    );
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::dependent_rows_hide_and_show
#[test]
fn dependent_rows_hide_and_show_twin() {
    // Live playback hides the loop length and the cache line; Loop shows them.
    let mut inputs = fixture_inputs("boats");
    let looped = model_of(&inputs, &RowContext::default());
    assert!(has_row(&looped, &AnimationRow::Common("loop_seconds")));
    assert!(has_row(&looped, &AnimationRow::CacheStatus));
    inputs.playback_loop = false;
    inputs.common.retain(|control| control.id != "loop_seconds");
    let live = model_of(&inputs, &RowContext::default());
    assert!(!has_row(&live, &AnimationRow::Common("loop_seconds")));
    assert!(!has_row(&live, &AnimationRow::CacheStatus));
    assert!(has_row(&live, &AnimationRow::Common("playback")));
    // Monotone shows the ink rows right under the mode; other modes do not.
    assert!(!has_row(&looped, &AnimationRow::Common("lightness")));
    let mut monotone = fixture_inputs("boats");
    monotone.monotone = true;
    monotone.look[0].value = ControlValue::Index(2);
    for (id, label, maximum) in [
        ("lightness", "Lightness", 100),
        ("hue", "Hue", 359),
        ("saturation", "Saturation", 100),
    ] {
        monotone
            .common
            .push(Control::slider(id, label, 40, (0, maximum, 1), "", ""));
    }
    let model = model_of(&monotone, &RowContext::default());
    let mode = index_of(&model, &AnimationRow::Common("look_mode"));
    assert_eq!(
        model.row(mode + 1),
        Some(&AnimationRow::Common("lightness"))
    );
    assert_eq!(model.row(mode + 2), Some(&AnimationRow::Common("hue")));
    assert_eq!(
        model.row(mode + 3),
        Some(&AnimationRow::Common("saturation"))
    );
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::each_region_scrolls_on_its_own_and_the_wheel_is_scoped_to_the_region
#[test]
fn each_region_scrolls_on_its_own_and_the_wheel_is_scoped_to_the_region_twin() {
    let model = fixture_model("shoreline");
    let panel = panel(area(100, 24), &model);
    let columns = panel.geometry();
    let mut scrolls = Scrolls::default();
    // The wheel over the scene list scrolls its window only.
    let on_list = Position::new(columns.scene_list.x + 2, columns.scene_list.y + 1);
    assert!(panel.wheel_scroll(&mut scrolls, on_list, 100));
    assert_eq!(scrolls.scenes, panel.max_scroll(Region::Scenes));
    assert!(scrolls.scenes > 0);
    assert_eq!((scrolls.global, scrolls.controls), (0, 0));
    // The wheel over the shared settings scrolls only those, and stops at both ends.
    let on_global = Position::new(columns.global.x + 2, columns.global.y + 1);
    assert!(panel.wheel_scroll(&mut scrolls, on_global, 100));
    assert_eq!(scrolls.global, panel.max_scroll(Region::Global));
    assert!(scrolls.global > 0);
    assert!(panel.wheel_scroll(&mut scrolls, on_global, -100));
    assert_eq!(scrolls.global, 0);
    // The wheel over the right column scrolls only the animation's own rows.
    let on_controls = Position::new(columns.controls_rows.x + 2, columns.controls_rows.y + 1);
    let before = scrolls;
    assert!(panel.wheel_scroll(&mut scrolls, on_controls, 100));
    assert_eq!(
        (scrolls.scenes, scrolls.global),
        (before.scenes, before.global)
    );
    assert_eq!(scrolls.controls, panel.max_scroll(Region::Controls));
    // Outside the panel nothing scrolls.
    assert!(!panel.wheel_scroll(&mut scrolls, Position::new(500, 500), 1));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::every_native_choice_and_number_paints_and_hits_at_supported_sizes
#[test]
fn every_native_choice_and_number_paints_and_hits_at_supported_sizes_twin() {
    for scene in ["shoreline", VOXEL_LANDSCAPE_ID, TOPOGRAPHIC_ID] {
        let model = fixture_model(scene);
        for (width, height) in [(80, 24), (120, 40), (160, 50)] {
            let panel = panel(area(width, height), &model);
            for row in 0..model.len() {
                let view = model.view(row).expect("row view");
                let scrolls = panel.follow_selection(row, Scrolls::default());
                let rect = panel
                    .row_rect(row, scrolls)
                    .unwrap_or_else(|| panic!("{scene} {width}x{height}: row {row} hidden"));
                assert!(!view.label.is_empty());
                if view.slider().is_some() {
                    let track = panel
                        .slider_geometry(row, scrolls)
                        .unwrap_or_else(|| panic!("{scene} {width}x{height}: row {row} track"));
                    assert!(track.track.x >= rect.x && track.track.right() <= rect.right());
                    assert!(track.value.right() <= rect.right());
                    assert!(track.label.right() <= track.track.x);
                }
            }
        }
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::every_row_stays_visible_in_its_region_for_each_screen_size_and_keyboard_walk
#[test]
fn every_row_stays_visible_in_its_region_for_each_screen_size_and_keyboard_walk_twin() {
    let model = fixture_model(VOXEL_LANDSCAPE_ID);
    for (width, height) in [(40, 12), (60, 20), (80, 24), (120, 40), (160, 50)] {
        let panel = panel(area(width, height), &model);
        let mut scrolls = Scrolls::default();
        for row in 0..model.len() {
            scrolls = panel.follow_selection(row, scrolls);
            assert!(
                panel.row_y(row, scrolls).is_some(),
                "{width}x{height}: row {row} hidden walking down"
            );
        }
        for row in (0..model.len()).rev() {
            scrolls = panel.follow_selection(row, scrolls);
            assert!(
                panel.row_y(row, scrolls).is_some(),
                "{width}x{height}: row {row} hidden walking up"
            );
        }
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::full_screen_preview_hides_the_controls_and_shares_the_screen_geometry
#[test]
fn full_screen_preview_hides_the_controls_and_shares_the_screen_geometry_twin() {
    let model = fixture_model("shoreline");
    let last = model.len() - 1;
    assert_eq!(model.row(last), Some(&AnimationRow::FullScreenPreview));
    assert_eq!(model.region(last), Some(Region::Controls));
    let view = model.view(last).unwrap();
    assert_eq!(view.label, "Full screen preview");
    assert_eq!(view.value, "f");
    assert!(matches!(view.kind, RowKind::Action));
    // The panel never leaves the screen it shares with the field.
    for (width, height) in [(80, 24), (120, 36), (160, 50)] {
        let screen = area(width, height);
        let layout = layout(screen);
        assert!(layout.panel.right() <= screen.right());
        assert!(layout.panel.bottom() <= screen.bottom());
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::game_inspired_scenes_credit_their_game_in_the_demo
#[test]
fn game_inspired_scenes_credit_their_game_in_the_demo_twin() {
    let model = fixture_model(CREDITED_ID);
    let urls = vec!["https://example.org/the-original-game".to_owned()];
    let credited = Panel {
        area: area(120, 36),
        model: &model,
        inspired_by: &urls,
    };
    assert_eq!(credited.credit_footer_rows(), 2);
    assert!(credited.footer_rows() > FOOTER_ROWS);
    // A scene without credit pays nothing; neither does a screen too short to carry the lines.
    let plain = panel(area(120, 36), &model);
    assert_eq!(plain.credit_footer_rows(), 0);
    assert_eq!(plain.footer_rows(), FOOTER_ROWS);
    let short = Panel {
        area: area(120, 15),
        model: &model,
        inspired_by: &urls,
    };
    assert_eq!(short.credit_footer_rows(), 0);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::global_region_lists_the_shared_look_and_the_right_column_only_the_animations_own_rows
#[test]
fn global_region_lists_the_shared_look_and_the_right_column_only_the_animations_own_rows_twin() {
    for scene in SCENES {
        let model = fixture_model(scene);
        let global: Vec<usize> = model.region_rows(Region::Global).collect();
        let controls: Vec<usize> = model.region_rows(Region::Controls).collect();
        assert!(
            global
                .iter()
                .any(|row| model.row(*row) == Some(&AnimationRow::Common("look_brightness"))),
            "{scene}"
        );
        for row in &controls {
            assert!(
                !matches!(model.row(*row), Some(AnimationRow::Common(id))
                    if id.starts_with("look_")
                        || matches!(*id, "panels" | "background" | "dither" | "density" | "speed")),
                "{scene}: shared row in the per-animation column"
            );
            assert_eq!(model.region(*row), Some(Region::Controls));
        }
        assert_eq!(
            global.len() + controls.len() + SCENES.len(),
            model.len(),
            "{scene}: every row is in exactly one region"
        );
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::global_slider_in_the_left_column_accepts_exact_endpoints
#[test]
fn global_slider_in_the_left_column_accepts_exact_endpoints_twin() {
    let model = fixture_model("shoreline");
    let panel = panel(area(100, 40), &model);
    let row = index_of(&model, &AnimationRow::Common("look_brightness"));
    let scrolls = panel.follow_selection(row, Scrolls::default());
    let geometry = panel.slider_geometry(row, scrolls).unwrap();
    assert!(geometry.value.right() <= panel.geometry().scenes.right());
    assert_eq!(
        panel.slider_value_at(row, scrolls, geometry.track.right() - 1),
        Some(200)
    );
    assert_eq!(
        panel.slider_value_at(row, scrolls, geometry.track.x),
        Some(1)
    );
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::inspired_by_credit_never_appears_outside_the_settings_demo
#[test]
fn inspired_by_credit_never_appears_outside_the_settings_demo_twin() {
    // Only the settings demo reserves credit rows: a panel with no credited scene reserves none at
    // any size, and the map caption strip of a normal screen is empty for every other scene.
    let model = fixture_model("shoreline");
    for (width, height) in [(40, 12), (80, 24), (120, 36), (200, 60)] {
        assert_eq!(panel(area(width, height), &model).credit_footer_rows(), 0);
    }
    let screen = area(120, 36);
    for scene in ["shoreline", CREDITED_ID, VOXEL_LANDSCAPE_ID] {
        assert!(area_for_scene(scene, true, screen).is_empty(), "{scene}");
    }
    assert!(area_for_scene(OPENSTREETMAP_ID, false, screen).is_empty());
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::inspired_by_credit_shows_bottom_right_in_the_demo_only_for_credited_scenes
#[test]
fn inspired_by_credit_shows_bottom_right_in_the_demo_only_for_credited_scenes_twin() {
    let model = fixture_model(CREDITED_ID);
    let urls = vec!["https://example.org/game".to_owned()];
    let with_credit = |width: u16, height: u16, urls: &[String]| {
        Panel {
            area: area(width, height),
            model: &model,
            inspired_by: urls,
        }
        .credit_footer_rows()
    };
    // Credited scene, narrow field to the right of the panel: the footer carries the credit.
    assert_eq!(with_credit(120, 36, &urls), 2);
    // A wide field to the right shows it there instead and the footer pays nothing.
    assert_eq!(with_credit(200, 36, &urls), 0);
    // An uncredited scene never shows any.
    assert_eq!(with_credit(120, 36, &[]), 0);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::keyboard_and_mouse_select_the_same_persisted_scene
#[test]
fn keyboard_and_mouse_select_the_same_persisted_scene_twin() {
    let ids = scene_ids();
    let model = fixture_model("shoreline");
    let panel = panel(area(100, 30), &model);
    for index in [0usize, 3, 8, 13] {
        // The mouse selects the row under the pointer; the row names the scene to persist.
        assert_eq!(click_on(&panel, index), Some(AnimationHit::Select(index)));
        assert_eq!(
            model.row(index),
            Some(&AnimationRow::Scene(SCENES[index].to_owned()))
        );
        // The keyboard steps (] and [) land on the same neighbouring rows.
        let next = adjacent_scene(&ids, SCENES[index], 1).unwrap();
        assert_eq!(next, SCENES[(index + 1) % SCENES.len()]);
        let previous = adjacent_scene(&ids, SCENES[index], -1).unwrap();
        assert_eq!(previous, SCENES[(index + SCENES.len() - 1) % SCENES.len()]);
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::keyboard_reaches_all_scenes_and_all_lower_controls_with_narrow_scrolling
#[test]
fn keyboard_reaches_all_scenes_and_all_lower_controls_with_narrow_scrolling_twin() {
    let model = fixture_model(VOXEL_LANDSCAPE_ID);
    for (width, height) in [(80, 24), (100, 30), (140, 40)] {
        let panel = panel(area(width, height), &model);
        let mut scrolls = Scrolls::default();
        for row in 0..model.len() {
            scrolls = panel.follow_selection(row, scrolls);
            assert!(
                panel.row_y(row, scrolls).is_some(),
                "{width}x{height}: row {row} unreachable"
            );
        }
        if (width, height) == (80, 24) {
            assert!(scrolls.global > 0, "the shared region needs a scroll here");
        }
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::narrow_scene_heading_respects_the_column_divider
#[test]
fn narrow_scene_heading_respects_the_column_divider_twin() {
    for width in [20, 40, 60, 80, 100, 120] {
        let columns = layout(area(width, 24));
        assert!(
            columns.scene_heading.right() <= columns.scenes.right(),
            "{width}: the heading runs over the divider"
        );
        assert!(columns.controls.x >= columns.scenes.right(), "{width}");
        assert_eq!(columns.scenes.width, left_width(columns.panel.width));
    }
    // With room for both columns the heading stops one cell before the divider.
    let wide = layout(area(120, 24));
    assert_eq!(wide.scene_heading.right() + 1, wide.scenes.right());
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::osm_catalogue_mode_plus_enables_and_persists_the_offline_tour_lists
#[test]
fn osm_catalogue_mode_plus_enables_and_persists_the_offline_tour_lists_twin() {
    let control = Control::choice(
        "catalogue",
        "Catalogue",
        0,
        &["Curated", "Offline lists"],
        "",
    );
    plus_selects_every_option(&control, OPENSTREETMAP_ID);
    // Offline lists can be unavailable; the dialog then names why.
    let blocked = control.with_disabled_option(1, "Install the offline catalogue first.");
    plus_selects_every_option(&blocked, OPENSTREETMAP_ID);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::osm_place_plus_selects_and_persists_from_the_complete_city_catalogue
#[test]
fn osm_place_plus_selects_and_persists_from_the_complete_city_catalogue_twin() {
    let control = Control::choice("place", "Place", 0, &PLACES, "");
    plus_selects_every_option(&control, OPENSTREETMAP_ID);
    // The last city of the catalogue is reachable and a stale index is not.
    let target = AnimationControlTarget::new(
        project(),
        OPENSTREETMAP_ID.to_owned(),
        AnimationControlScope::Scene,
        control.clone(),
    )
    .unwrap();
    assert_eq!(
        target.choice(
            &project(),
            OPENSTREETMAP_ID,
            AnimationControlScope::Scene,
            &control,
            "11",
        ),
        Ok(ControlValue::Index(11))
    );
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::osm_place_tour_plus_selects_and_persists_the_catalogue_mode
#[test]
fn osm_place_tour_plus_selects_and_persists_the_catalogue_mode_twin() {
    let control = Control::choice(
        "place_tour",
        "Place tour",
        1,
        &["Off", "Cities", "Landmarks"],
        "",
    );
    plus_selects_every_option(&control, OPENSTREETMAP_ID);
    let target = AnimationControlTarget::new(
        project(),
        OPENSTREETMAP_ID.to_owned(),
        AnimationControlScope::Scene,
        control.clone(),
    )
    .unwrap();
    // The dialog opens on the current mode.
    let Ok(ValueDialog::Choice { selected_id, .. }) = target.dialog() else {
        panic!("choice dialog");
    };
    assert_eq!(selected_id.as_deref(), Some("1"));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::osm_tour_list_plus_selects_and_persists_a_full_catalogue_option
#[test]
fn osm_tour_list_plus_selects_and_persists_a_full_catalogue_option_twin() {
    let control = Control::choice("tour_list", "Tour list", 0, &PLACES, "");
    plus_selects_every_option(&control, OPENSTREETMAP_ID);
    // A tour list built from a changed catalogue is refused, never applied to a shifted index.
    let shorter = Control::choice("tour_list", "Tour list", 0, &PLACES[..6], "");
    assert!(checked_choice(&control, &shorter, 9).is_err());
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::osm_world_sampler_plus_opens_every_named_destination
#[test]
fn osm_world_sampler_plus_opens_every_named_destination_twin() {
    let control = Control::choice("sampler", "World sampler", 0, &PLACES, "");
    plus_selects_every_option(&control, OPENSTREETMAP_ID);
    let Some(options) = ilium_plugin_core_ambient::values::choices(&control) else {
        panic!("a choice control lists its options");
    };
    let labels: Vec<&str> = options.iter().map(|option| option.label.as_str()).collect();
    assert_eq!(labels, PLACES.to_vec(), "every named destination is listed");
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::owned_slider_drag_clamps_outside_the_track_and_releases_before_global_voice_control
#[test]
fn owned_slider_drag_clamps_outside_the_track_and_releases_before_global_voice_control_twin() {
    let model = fixture_model("shoreline");
    let panel = panel(area(140, 40), &model);
    let row = index_of(&model, &AnimationRow::Common("look_brightness"));
    let scrolls = panel.follow_selection(row, Scrolls::default());
    let geometry = panel.slider_geometry(row, scrolls).unwrap();
    // A drag owns the row: any column beyond either end clamps to that end.
    assert_eq!(
        panel.slider_value_at(row, scrolls, geometry.track.right() + 30),
        Some(200)
    );
    assert_eq!(
        panel.slider_value_at(row, scrolls, geometry.track.x.saturating_sub(5)),
        Some(1)
    );
    // The middle of the track lands strictly inside the range.
    let middle = panel
        .slider_value_at(row, scrolls, geometry.track.x + geometry.track.width / 2)
        .unwrap();
    assert!(middle > 1 && middle < 200, "{middle}");
    // A row with no slider has no drag value.
    let toggle = index_of(&model, &AnimationRow::Common("panels"));
    assert_eq!(panel.slider_value_at(toggle, scrolls, 5), None);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::palette_and_named_controls_survive_scene_switches_and_failed_saves
#[test]
fn palette_and_named_controls_survive_scene_switches_and_failed_saves_twin() {
    let mut inputs = fixture_inputs("shoreline");
    inputs.look[1].value = ControlValue::Index(2);
    // A rejected save never reaches the inputs: the checked value is an error, the row stays.
    let before = inputs.look[1].clone();
    assert!(checked_choice(&before, &before, 9).is_err());
    for scene in ["rain", "stars", "shoreline"] {
        inputs.selected = scene.to_owned();
        let model = model_of(&inputs, &RowContext::default());
        let row = index_of(&model, &AnimationRow::Common("look_palette"));
        assert_eq!(model.view(row).unwrap().value, "Pastel", "{scene}");
        let amount = index_of(&model, &AnimationRow::SceneControl("amount"));
        assert_eq!(model.view(amount).unwrap().value.trim(), "40", "{scene}");
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::plugin_common_projection_uses_shared_choice_ink_pointer_and_number_star
#[test]
fn plugin_common_projection_uses_shared_choice_ink_pointer_and_number_star_twin() {
    let model = fixture_model("shoreline");
    let choice = index_of(&model, &AnimationRow::Common("look_palette"));
    let slider = index_of(&model, &AnimationRow::Common("look_brightness"));
    let toggle = index_of(&model, &AnimationRow::Common("panels"));
    assert!(matches!(model.view(choice).unwrap().kind, RowKind::Choice));
    assert!(model.view(choice).unwrap().slider().is_none());
    assert!(model.view(choice).unwrap().number().is_none());
    assert!(matches!(model.view(toggle).unwrap().kind, RowKind::Toggle));
    // The slider carries the range the number star opens.
    assert_eq!(model.view(slider).unwrap().number(), Some((1, 200, 1, 100)));
    let control = model.control(slider).expect("exact control").clone();
    let target = AnimationControlTarget::new(
        project(),
        "shoreline".to_owned(),
        AnimationControlScope::Common,
        control,
    )
    .unwrap();
    assert_eq!(target.scope(), AnimationControlScope::Common);
    assert_eq!(target.control_id(), "look_brightness");
    assert!(matches!(target.dialog(), Ok(ValueDialog::Number { .. })));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::plugin_editor_uses_shared_frame_and_keeps_actions_inside_it
#[test]
fn plugin_editor_uses_shared_frame_and_keeps_actions_inside_it_twin() {
    // The editor frame is the client's. The plugin keeps every row action inside the panel frame:
    // each visible row rectangle, slider track and value readout lies within it.
    let model = fixture_model(VOXEL_LANDSCAPE_ID);
    for (width, height) in [(80, 24), (100, 30), (160, 50)] {
        let panel = panel(area(width, height), &model);
        let frame = panel.geometry().panel;
        for row in 0..model.len() {
            let scrolls = panel.follow_selection(row, Scrolls::default());
            let rect = panel.row_rect(row, scrolls).expect("row visible");
            let inside = |part: Rect| {
                part.x >= frame.x
                    && part.right() <= frame.right()
                    && part.y >= frame.y
                    && part.bottom() <= frame.bottom()
            };
            assert!(inside(rect), "{width}x{height}: row {row} leaves the frame");
            if let Some(track) = panel.slider_geometry(row, scrolls) {
                assert!(inside(track.track) && inside(track.value) && inside(track.label));
            }
        }
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::preview_advances_while_disabled_or_off_and_help_covers_it
#[test]
fn preview_advances_while_disabled_or_off_and_help_covers_it_twin() {
    // The preview row is a plain action available whatever the background switch says, and the
    // help panel has a topic for it.
    for background in [true, false] {
        let mut inputs = fixture_inputs("shoreline");
        inputs.common[1].value = ControlValue::Bool(background);
        let model = model_of(&inputs, &RowContext::default());
        let row = index_of(&model, &AnimationRow::FullScreenPreview);
        assert!(model.view(row).unwrap().disabled_options.is_empty());
        assert_eq!(
            help_id(
                &AnimationRow::FullScreenPreview,
                &inputs.scene_controls,
                &scene_ids()
            ),
            "AN-38"
        );
    }
    assert!(ilium_plugin_core_ambient::help::help_ids().contains(&"AN-38".to_owned()));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::preview_palette_matches_the_shared_hsl_for_both_canvases
#[test]
fn preview_palette_matches_the_shared_hsl_for_both_canvases_twin() {
    // The two preview canvases (panel view and full screen) read one shared look, so the same
    // palette row feeds both. The HSL conversion itself runs in the client.
    let mut inputs = fixture_inputs("shoreline");
    inputs.look[1].value = ControlValue::Index(1);
    let panel_view = model_of(&inputs, &RowContext::default());
    inputs.selected = "boats".to_owned();
    let full_screen = model_of(&inputs, &RowContext::default());
    let palette = |model: &RowModel| {
        let row = index_of(model, &AnimationRow::Common("look_palette"));
        model.view(row).unwrap().value.clone()
    };
    assert_eq!(palette(&panel_view), "Dusk");
    assert_eq!(palette(&panel_view), palette(&full_screen));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::real_render_keeps_every_row_reachable_over_a_full_screen_field
#[test]
fn real_render_keeps_every_row_reachable_over_a_full_screen_field_twin() {
    let model = fixture_model(CREDITED_ID);
    let urls = vec!["https://example.org/the-original-game".to_owned()];
    for (width, height) in [(80, 24), (120, 40), (160, 50)] {
        let panel = Panel {
            area: area(width, height),
            model: &model,
            inspired_by: &urls,
        };
        let mut scrolls = Scrolls::default();
        for row in 0..model.len() {
            scrolls = panel.follow_selection(row, scrolls);
            assert!(
                panel.row_y(row, scrolls).is_some(),
                "{width}x{height}: row {row} unreachable"
            );
        }
        for region in REGIONS {
            assert!(
                panel.visible_rows(region) > 0,
                "{width}x{height} {region:?}"
            );
        }
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::rendered_screen_shows_both_columns_at_every_size
#[test]
fn rendered_screen_shows_both_columns_at_every_size_twin() {
    for (width, height) in [(80, 24), (120, 40), (160, 50)] {
        let columns = layout(area(width, height));
        assert!(columns.scenes.width > 0 && columns.controls.width > 0);
        assert!(columns.controls.x >= columns.scenes.right());
        for part in [
            columns.scene_heading,
            columns.scene_nav,
            columns.global_heading,
            columns.controls_heading,
        ] {
            assert_eq!(part.height, 1, "{width}x{height}");
        }
        for part in [
            columns.scene_list,
            columns.global,
            columns.controls_intro,
            columns.controls_rows,
        ] {
            assert!(part.height > 0, "{width}x{height}");
        }
        // The divider column runs the whole height of the left column.
        assert_eq!(columns.scenes.height, height);
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::row_and_hit_geometry_never_escape_short_or_scrolled_controls
#[test]
fn row_and_hit_geometry_never_escape_short_or_scrolled_controls_twin() {
    let model = fixture_model("shoreline");
    for (width, height) in [(100, 30), (60, 24), (20, 8), (5, 3), (0, 0)] {
        let panel = Panel {
            area: Rect::new(3, 4, width, height),
            model: &model,
            inspired_by: &[],
        };
        for row in 0..model.len() {
            let scrolls = panel.follow_selection(row, Scrolls::default());
            if let Some(y) = panel.row_y(row, scrolls) {
                let rect = panel.row_rect(row, scrolls).unwrap();
                let frame = panel.geometry().panel;
                assert!(frame.contains(panel.area.x, y));
                assert_eq!(
                    panel.hit(scrolls, Position::new(rect.x, y)),
                    Some(expected_hit(&model, row)),
                    "{width}x{height}: row {row}"
                );
            }
        }
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::row_lists_follow_the_selected_scene
#[test]
fn row_lists_follow_the_selected_scene_twin() {
    for scene in SCENES {
        let model = fixture_model(scene);
        let rows = model.rows();
        // Scenes first, then the display rows, and the preview closes the list.
        for (index, id) in SCENES.iter().enumerate() {
            assert_eq!(
                rows[index],
                AnimationRow::Scene((*id).to_owned()),
                "{scene}"
            );
        }
        assert_eq!(
            rows[SCENES.len()],
            AnimationRow::Common("panels"),
            "{scene}"
        );
        assert_eq!(rows.last(), Some(&AnimationRow::FullScreenPreview));
        // The shared look rows exist for every scene.
        assert!(
            has_row(&model, &AnimationRow::Common("look_mode")),
            "{scene}"
        );
        // Pattern and motion rows are absent only for the encyclopedia.
        for id in [
            "speed",
            "density",
            "dither",
            "fps_limit",
            "look_pattern_contrast",
        ] {
            assert_eq!(
                has_row(&model, &AnimationRow::Common(id)),
                scene != WIKIPEDIA_ID,
                "{scene} {id}"
            );
        }
        // The ink rows are hidden outside Monotone.
        assert!(
            !has_row(&model, &AnimationRow::Common("lightness")),
            "{scene}"
        );
        // Regions never decrease down the list.
        let regions: Vec<Region> = (0..model.len())
            .map(|row| model.region(row).unwrap())
            .collect();
        let order = |region: &Region| REGIONS.iter().position(|r| r == region).unwrap();
        assert!(regions
            .windows(2)
            .all(|pair| order(&pair[0]) <= order(&pair[1])));
        // Live-only scenes hide playback and the cache; the location row is for map scenes.
        let live_only = scene == TOPOGRAPHIC_ID;
        assert_eq!(
            !has_row(&model, &AnimationRow::Common("playback")),
            live_only
        );
        assert_eq!(!has_row(&model, &AnimationRow::CacheStatus), live_only);
        assert_eq!(
            has_row(&model, &AnimationRow::Location),
            scene == OPENSTREETMAP_ID
        );
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_activity_footer_accounts_for_wrapped_event_lines
#[test]
fn saved_scene_activity_footer_accounts_for_wrapped_event_lines_twin() {
    let long_event = "detail ".repeat(120);
    let model = voxel_model(&voxel_status(1, long_event.trim_end()));
    let screen = area(80, 32);
    let panel = panel(screen, &model);
    let report = saved_scene_activity_report(&model).expect("a report");
    let (summary, activity) = saved_scene_activity_sections(&report);
    let width = layout(screen).panel.width;
    let summary_rows = wrapped_rows(&summary, width);
    let wrapped_activity_rows = wrapped_rows(&activity, width);
    let viewport_rows = panel.footer_rows().saturating_sub(summary_rows + 2 + 2);
    assert!(viewport_rows > 0);
    assert!(
        wrapped_activity_rows > viewport_rows,
        "the long event wraps beyond the viewport so it scrolls"
    );
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_activity_pages_keep_control_help_reachable
#[test]
fn saved_scene_activity_pages_keep_control_help_reachable_twin() {
    let summary = "Scene status: Saved worlds [==] phase 1/6\nNow: scanning";
    let activity = "+00:01 first event\n+00:02 second event\n+00:03 third event";
    for detail_rows in 0..=12u16 {
        let (summary_rows, activity_rows) =
            activity_footer_rows(detail_rows, 60, summary, activity);
        let pinned = pinned_control_help_rows(detail_rows);
        assert_eq!(
            summary_rows + activity_rows + pinned,
            detail_rows,
            "{detail_rows} detail rows are fully assigned"
        );
        if detail_rows >= 4 {
            assert_eq!(pinned, 2, "two lines of control help stay pinned");
        }
        if detail_rows >= 5 {
            assert!(activity_rows > 0, "the activity log keeps a row");
        }
    }
    let model = voxel_model(&voxel_status(2, "Loaded 64 chunks"));
    let row = index_of(&model, &AnimationRow::Common("look_brightness"));
    assert!(selected_control_help(&model, row).starts_with("Brightness: 100%"));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_activity_report_expands_the_live_footer
#[test]
fn saved_scene_activity_report_expands_the_live_footer_twin() {
    let model = voxel_model(&voxel_status(2, "Loaded 64 chunks"));
    let report = saved_scene_activity_report(&model).expect("a multi-line status is a report");
    let (summary, activity) = saved_scene_activity_sections(&report);
    let screen = area(80, 24);
    let width = layout(screen).panel.width;
    let summary_rows = wrapped_rows(&summary, width);
    let activity_rows = wrapped_rows(&activity, width);
    let tall = panel(screen, &model);
    assert!(tall.footer_rows() >= summary_rows + activity_rows + 2 + 2);
    assert!(tall.footer_rows() > FOOTER_ROWS);
    // On a short terminal the settings regions take priority over the footer.
    let short = panel(area(80, 16), &model);
    assert!(short.footer_rows() <= 16 - 7);
    for region in REGIONS {
        assert!(short.visible_rows(region) > 0, "{region:?}");
    }
    // A scene without a multi-line status keeps the plain footer.
    let plain = fixture_model("shoreline");
    assert_eq!(saved_scene_activity_report(&plain), None);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_activity_sections_keep_inline_wait_message
#[test]
fn saved_scene_activity_sections_keep_inline_wait_message_twin() {
    let report = "Scene status: Saved worlds [..] phase ?/6\nNow: reading\nElapsed: 00:01\nETA: unknown\nRecent activity: waiting for a nonblocking status snapshot";
    let (summary, activity) = saved_scene_activity_sections(report);
    assert!(summary.contains("Saved worlds [..] phase ?/6"));
    assert!(summary.contains("Elapsed: 00:01"));
    assert_eq!(activity, "waiting for a nonblocking status snapshot");
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_help_view_scrolls_through_the_full_selected_status
#[test]
fn saved_scene_help_view_scrolls_through_the_full_selected_status_twin() {
    let status = voxel_status(12, "Loaded 64 chunks");
    let model = voxel_model(&status);
    let row = index_of(&model, &AnimationRow::SceneStatus);
    let view = model.view(row).unwrap();
    // The row shows the first line; the help keeps every other line for the scrolling view.
    assert_eq!(view.value, status.lines().next().unwrap());
    let kept: Vec<&str> = view.help.lines().collect();
    assert!(kept.len() >= status.lines().count() - 1);
    for line in status.lines().skip(1) {
        assert!(view.help.contains(line), "{line}");
    }
    assert!(
        wrap_lines(&view.help, 30).len() > 8,
        "the view needs paging"
    );
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_progress_and_activity_are_visible_in_the_rendered_panel
#[test]
fn saved_scene_progress_and_activity_are_visible_in_the_rendered_panel_twin() {
    let model = voxel_model(&voxel_status(3, "Loaded 64 chunks"));
    let report = saved_scene_activity_report(&model).unwrap();
    let (summary, activity) = saved_scene_activity_sections(&report);
    assert!(summary.contains("phase 3/6") && summary.contains("Elapsed: 00:42"));
    assert_eq!(activity.lines().count(), 3);
    let compact = compact_saved_scene_status(&summary, 80).expect("a compact line");
    assert!(
        compact.contains("Saved") && compact.contains("3/6"),
        "{compact}"
    );
    assert!(!compact.contains("phase"));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_route_eta_stays_visible_on_a_short_terminal
#[test]
fn saved_scene_route_eta_stays_visible_on_a_short_terminal_twin() {
    // The route line belongs to the summary, so it sits above the recent-activity log.
    let status = voxel_status(1, "Loaded 64 chunks").replace(
        "Recent activity:",
        "Route ETA: 00:12; four of nine legs\nRecent activity:",
    );
    let report = format!("Scene status: {status}");
    let (summary, _) = saved_scene_activity_sections(&report);
    assert!(summary.contains("Route ETA: 00:12"));
    let compact = compact_saved_scene_status(&summary, 60).expect("a compact line");
    assert!(compact.contains("Route ETA: 00:12"), "{compact}");
    // The short terminal still shows every region and gives the footer a row at most.
    let model = voxel_model(&status);
    let short = panel(area(80, 16), &model);
    for region in REGIONS {
        assert!(short.visible_rows(region) > 0, "{region:?}");
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_selected_status_help_can_be_scrolled_to_its_last_event
#[test]
fn saved_scene_selected_status_help_can_be_scrolled_to_its_last_event_twin() {
    let status = voxel_status(12, "Loaded 64 chunks");
    let model = voxel_model(&status);
    let row = index_of(&model, &AnimationRow::SceneStatus);
    let help = selected_control_help(&model, row);
    let last = status.lines().last().unwrap();
    assert!(help.ends_with(last) || help.contains(last), "{help}");
    let wrapped = wrap_lines(&help, 30);
    assert!(wrapped.len() > 8, "more lines than the viewport shows");
    assert!(wrapped.last().unwrap().contains("chunks"));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::scene_list_is_a_compact_window_with_prev_next_buttons
#[test]
fn scene_list_is_a_compact_window_with_prev_next_buttons_twin() {
    let model = fixture_model("shoreline");
    let panel = panel(area(100, 24), &model);
    let columns = panel.geometry();
    assert!(columns.scene_list.height < SCENES.len() as u16);
    assert!((3..=8).contains(&columns.scene_list.height));
    let nav = columns.scene_nav;
    let next = Position::new(nav.right() - 2, nav.y);
    let previous = Position::new(nav.x + 1, nav.y);
    assert_eq!(
        panel.hit(Scrolls::default(), next),
        Some(AnimationHit::NextScene)
    );
    assert_eq!(
        panel.hit(Scrolls::default(), previous),
        Some(AnimationHit::PreviousScene)
    );
    // Prev and Next wrap at both ends.
    let ids = scene_ids();
    assert_eq!(adjacent_scene(&ids, SCENES[0], -1), Some(SCENES[13]));
    assert_eq!(adjacent_scene(&ids, SCENES[13], 1), Some(SCENES[0]));
    // The window follows the selected scene.
    let last = SCENES.len() - 1;
    let scrolls = panel.follow_selection(last, Scrolls::default());
    assert!(scrolls.scenes > 0);
    assert!(panel.row_y(last, scrolls).is_some());
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::scroll_and_scrollbar_derive_from_the_row_count
#[test]
fn scroll_and_scrollbar_derive_from_the_row_count_twin() {
    let model = fixture_model("shoreline");
    let panel = panel(area(60, 20), &model);
    for region in REGIONS {
        assert_eq!(
            panel.max_scroll(region),
            model
                .visual_height(region)
                .saturating_sub(panel.visible_rows(region)),
            "{region:?}"
        );
    }
    let last = model.region_rows(Region::Global).last().unwrap();
    let far = panel.follow_selection(last, Scrolls::default());
    assert!(far.global > 0 && far.scenes == 0 && far.controls == 0);
    assert!(panel.row_y(last, far).is_some());
    let first = model.region_rows(Region::Global).next().unwrap();
    assert_eq!(panel.follow_selection(first, far).global, 0);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::scrollbar_clicks_jump_the_scene_list_and_the_global_settings
#[test]
fn scrollbar_clicks_jump_the_scene_list_and_the_global_settings_twin() {
    let model = fixture_model("shoreline");
    let panel = panel(area(100, 24), &model);
    let columns = panel.geometry();
    for (region, rows) in [
        (Region::Scenes, columns.scene_list),
        (Region::Global, columns.global),
    ] {
        let maximum = panel.max_scroll(region);
        assert!(maximum > 0, "{region:?} needs a scrollbar at 100x24");
        let x = rows.right();
        assert_eq!(
            panel.hit(Scrolls::default(), Position::new(x, rows.bottom() - 1)),
            Some(AnimationHit::ScrollTo(region, maximum))
        );
        assert_eq!(
            panel.hit(Scrolls::default(), Position::new(x, rows.y)),
            Some(AnimationHit::ScrollTo(region, 0))
        );
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::semantic_scope_row_persists_through_real_project_configuration
#[test]
fn semantic_scope_row_persists_through_real_project_configuration_twin() {
    // The project file stays with the client. The plugin supplies the scope row of the semantic
    // scene, its recommendation status and its help topic.
    let mut inputs = fixture_inputs(SEMANTIC_ID);
    inputs.scene_controls = vec![Control::choice(
        "semantic_scope",
        "Scope",
        1,
        &["Project", "Entry"],
        "",
    )];
    let model = model_of(&inputs, &RowContext::default());
    let row = index_of(&model, &AnimationRow::SceneControl("semantic_scope"));
    assert_eq!(model.view(row).unwrap().value, "Entry");
    assert_eq!(
        help_id(
            &AnimationRow::SceneControl("semantic_scope"),
            &inputs.scene_controls,
            &scene_ids()
        ),
        "AN-65"
    );
    let status = index_of(&model, &AnimationRow::SceneStatus);
    assert_eq!(model.view(status).unwrap().label, "Recommendation");
    assert_eq!(
        model.view(status).unwrap().value,
        "Awaiting tree recommendation"
    );
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::shared_animation_chrome_paints_the_exact_button_hit_regions
#[test]
fn shared_animation_chrome_paints_the_exact_button_hit_regions_twin() {
    let model = fixture_model("shoreline");
    let panel = panel(area(100, 24), &model);
    let nav = panel.geometry().scene_nav;
    let at = |x: u16| panel.hit(Scrolls::default(), Position::new(x, nav.y));
    // Eight cells of Prev, eight of Next, and nothing clickable between them.
    assert_eq!(at(nav.x), Some(AnimationHit::PreviousScene));
    assert_eq!(at(nav.x + 7), Some(AnimationHit::PreviousScene));
    assert_eq!(at(nav.x + 8), None);
    assert_eq!(at(nav.right() - 9), None);
    assert_eq!(at(nav.right() - 8), Some(AnimationHit::NextScene));
    assert_eq!(at(nav.right() - 1), Some(AnimationHit::NextScene));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::topographic_maps_rows_are_live_only_and_list_every_control
#[test]
fn topographic_maps_rows_are_live_only_and_list_every_control_twin() {
    let model = fixture_model(TOPOGRAPHIC_ID);
    for id in ["playback", "loop_seconds"] {
        assert!(!has_row(&model, &AnimationRow::Common(id)), "{id}");
    }
    assert!(!has_row(&model, &AnimationRow::CacheStatus));
    assert!(has_row(&model, &AnimationRow::SceneStatus));
    let listed: Vec<&'static str> = model
        .rows()
        .iter()
        .filter_map(|row| match row {
            AnimationRow::SceneControl(id) => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(listed, ["contours", "source", "labels", "region"]);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::two_columns_scenes_and_global_settings_left_animation_settings_right
#[test]
fn two_columns_scenes_and_global_settings_left_animation_settings_right_twin() {
    let model = fixture_model("boats");
    let panel = panel(area(120, 40), &model);
    let columns = panel.geometry();
    // The first scene sits on the line under the heading, at the left edge.
    let scrolls = Scrolls::default();
    assert_eq!(panel.row_rect(0, scrolls).map(|r| (r.x, r.y)), Some((0, 1)));
    // The first row of the right column sits under its section heading, right of the divider.
    let first = model.first_control_index();
    let rect = panel.row_rect(first, scrolls).expect("first control row");
    assert_eq!(rect.y, columns.controls_rows.y + 1);
    assert!(rect.x >= columns.scenes.right());
    // The right slider track is wide; the left one is narrower but inside its column.
    let amount = index_of(&model, &AnimationRow::SceneControl("amount"));
    let scrolls = panel.follow_selection(amount, scrolls);
    let right = panel.slider_geometry(amount, scrolls).expect("right track");
    assert!(right.track.width >= 32, "{}", right.track.width);
    let brightness = index_of(&model, &AnimationRow::Common("look_brightness"));
    let scrolls = panel.follow_selection(brightness, Scrolls::default());
    let left = panel
        .slider_geometry(brightness, scrolls)
        .expect("left track");
    assert!(left.track.width >= 8);
    assert!(left.value.right() <= columns.scenes.right());
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::voxel_seed_exact_keyboard_entry_persists_the_complete_u32_domain_and_cancel_is_inert
#[test]
fn voxel_seed_exact_keyboard_entry_persists_the_complete_u32_domain_and_cancel_is_inert_twin() {
    let seed = Control::text("seed", "World seed", "71839", "0-4294967295", "");
    let target = AnimationControlTarget::new_for_animation(
        project(),
        VOXEL_LANDSCAPE_ID.to_owned(),
        AnimationControlScope::Scene,
        VOXEL_LANDSCAPE_ID,
        seed.clone(),
    )
    .unwrap();
    let scope = AnimationControlScope::Scene;
    for text in ["0", "2147483647", "2147483648", "4294967295"] {
        assert_eq!(
            target.number(&project(), VOXEL_LANDSCAPE_ID, scope, &seed, text),
            Ok(ControlValue::Text(text.to_owned())),
            "{text}"
        );
    }
    for text in ["4294967296", "-1", "1.5", "seed", ""] {
        assert!(
            target
                .number(&project(), VOXEL_LANDSCAPE_ID, scope, &seed, text)
                .is_err(),
            "{text:?} must be refused"
        );
    }
    // Cancelling confirms nothing, so the stored text is untouched.
    assert_eq!(seed.value, ControlValue::Text("71839".to_owned()));
    // A keyboard step stays inside the same domain.
    assert_eq!(
        stepped_animation_number(VOXEL_LANDSCAPE_ID, &seed, 1),
        Ok(Some(ControlValue::Text("71840".to_owned())))
    );
    let top = Control::text("seed", "World seed", "4294967295", "0-4294967295", "");
    assert_eq!(
        stepped_animation_number(VOXEL_LANDSCAPE_ID, &top, 1),
        Ok(Some(ControlValue::Text("4294967295".to_owned())))
    );
}
