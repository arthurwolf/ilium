//! Twins of the tests of ilium-client/src/animation_hover.rs, run against the plugin's `hover`,
//! `rows` and `values` modules. The kernel tests drive the whole TUI over a `TestBackend`; the
//! plugin owns the pure parts (row metadata, hover timing, popover geometry, option stepping), so
//! each twin checks the same behaviour at that boundary. Drawing the dim marker is kernel
//! presentation and stays there.

use ilium_plugin_core_ambient::control::{Control, ControlValue};
use ilium_plugin_core_ambient::hover::{
    panel_rect, popover_geometry, popover_text, HoverState, Rect, HOVER_DELAY_MS, MAXIMUM_WIDTH,
    MINIMUM_HEIGHT,
};
use ilium_plugin_core_ambient::rows::{
    AnimationRow, DisabledOption, RowContext, RowInputs, RowModel, SceneEntry,
};
use ilium_plugin_core_ambient::values::checked_choice;

const REASONS: [&str; 4] = [
    "The Vulkan loader was not found. Install your distribution's vulkan-loader package, then restart Ilium so the renderer can use the graphics card.",
    "No graphics driver answered. Install the vendor driver, check that it lists a device with vulkaninfo, and restart.",
    "This build has no GPU renderer compiled in.",
    "Checking the graphics card.",
];

fn backend(index: usize, reason: Option<&str>) -> Control {
    let control = Control::choice(
        "render_backend",
        "Renderer",
        index,
        &["Software", "GPU"],
        "Which renderer draws the scene.",
    );
    match reason {
        Some(reason) => control.with_disabled_option(1, reason),
        None => control,
    }
}

fn model_with(control: Control) -> RowModel {
    let data = RowInputs {
        scenes: vec![SceneEntry {
            id: "waves".to_owned(),
            label: "Waves".to_owned(),
            description: "Waves.".to_owned(),
        }],
        selected: "waves".to_owned(),
        scene_controls: vec![control],
        ..RowInputs::default()
    };
    RowModel::new(&data, &RowContext::default())
}

fn backend_row(model: &RowModel) -> usize {
    model
        .rows()
        .iter()
        .position(|row| *row == AnimationRow::SceneControl("render_backend"))
        .expect("render_backend row")
}

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::a_ready_gpu_can_be_selected_from_the_keyboard
#[test]
fn a_ready_gpu_can_be_selected_from_the_keyboard_twin() {
    let control = backend(0, None);
    assert_eq!(control.stepped(1), Some(ControlValue::Index(1)));
    let model = model_with(control.clone());
    assert!(model
        .view(backend_row(&model))
        .unwrap()
        .disabled_options
        .is_empty());
    assert_eq!(
        checked_choice(&control, &control, 1),
        Ok(ControlValue::Index(1))
    );
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::a_wide_screen_puts_the_popover_beside_the_panel
#[test]
fn a_wide_screen_puts_the_popover_beside_the_panel_twin() {
    let area = Rect::new(0, 2, 140, 38);
    let panel = panel_rect(area);
    let geometry = popover_geometry(area, panel, 10, REASONS[0]).unwrap();
    assert_eq!(geometry.rectangle.x, panel.right() + 1);
    assert_eq!(geometry.rectangle.y, 10, "level with the row");
    assert!(geometry.rectangle.width <= MAXIMUM_WIDTH);
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::disabled_gpu_option_shows_a_dim_marker_and_stays_within_the_panel
#[test]
fn disabled_gpu_option_shows_a_dim_marker_and_stays_within_the_panel_twin() {
    let model = model_with(backend(0, Some(REASONS[1])));
    let view = model.view(backend_row(&model)).unwrap();
    assert_eq!(view.value, "Software");
    assert_eq!(view.disabled_options.len(), 1);
    assert_eq!(view.disabled_options[0].label, "GPU");
    // The marker column is inside the panel: the panel never exceeds the screen width.
    let area = Rect::new(0, 2, 140, 38);
    assert!(panel_rect(area).right() <= area.right());
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::hover_popover_shows_each_reason_text_inside_the_screen
#[test]
fn hover_popover_shows_each_reason_text_inside_the_screen_twin() {
    for reason in REASONS {
        let area = Rect::new(0, 2, 80, 21);
        let panel = panel_rect(area);
        let y = 8;
        let geometry = popover_geometry(area, panel, y, reason).unwrap();
        let rectangle = geometry.rectangle;
        assert!(
            rectangle.right() <= 80 && rectangle.bottom() <= 23,
            "{rectangle:?}"
        );
        assert!(
            !rectangle.intersects(Rect::new(panel.x, y, panel.width, 1)),
            "the row stays visible: {rectangle:?} row y={y}"
        );
        assert_eq!(normalized(&geometry.lines.join(" ")), normalized(reason));
        let option = DisabledOption {
            label: "GPU".to_owned(),
            reason: reason.to_owned(),
        };
        assert_eq!(popover_text(&[option]).0, " GPU unavailable ");
    }
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::keyboard_and_click_skip_the_disabled_option_and_explain_why
#[test]
fn keyboard_and_click_skip_the_disabled_option_and_explain_why_twin() {
    let control = backend(0, Some(REASONS[2]));
    let software = ControlValue::Index(0);
    for direction in [1, -1] {
        assert_eq!(control.stepped(direction), Some(software.clone()));
    }
    // The full catalog keeps the unavailable option and its exact reason; committing it fails.
    let model = model_with(control.clone());
    let view = model.view(backend_row(&model)).unwrap();
    let reason = view.disabled_options[0].reason.clone();
    let options = ilium_plugin_core_ambient::values::choices(&control).unwrap();
    assert_eq!(options.len(), 2);
    assert_eq!(options[1].label, "GPU");
    assert_eq!(options[1].disabled_reason.as_deref(), Some(reason.as_str()));
    assert_eq!(checked_choice(&control, &control, 1), Err(reason));
    assert!(view
        .disabled_notice()
        .is_some_and(|notice| notice.starts_with("GPU unavailable:")));
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::multiple_disabled_options_are_listed_with_their_labels
#[test]
fn multiple_disabled_options_are_listed_with_their_labels_twin() {
    let options = vec![
        DisabledOption {
            label: "GPU".to_owned(),
            reason: "No driver.".to_owned(),
        },
        DisabledOption {
            label: "Other".to_owned(),
            reason: "Not here.".to_owned(),
        },
    ];
    let (title, body) = popover_text(&options);
    assert_eq!(title.trim(), "Unavailable options");
    assert_eq!(body, "GPU: No driver.\n\nOther: Not here.");
    assert_eq!(popover_text(&[]), (String::new(), String::new()));
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::nothing_appears_while_the_gpu_is_ready
#[test]
fn nothing_appears_while_the_gpu_is_ready_twin() {
    let control = backend(0, None).with_help_detail("Adapter: Test GPU");
    let model = model_with(control);
    let view = model.view(backend_row(&model)).unwrap();
    assert!(view.disabled_options.is_empty());
    assert!(view.help.contains("Test GPU"), "help names the adapter");
    // A row without disabled options never starts a hover, so a tick changes nothing.
    let mut hover = HoverState::default();
    hover.set(
        (!view.disabled_options.is_empty()).then(|| backend_row(&model)),
        0,
    );
    assert!(hover.current().is_none());
    assert!(!hover.tick(HOVER_DELAY_MS * 2, true));
    assert_eq!(popover_text(&view.disabled_options).0, "");
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::popover_geometry_stays_on_screen_and_clear_of_the_row_at_every_size
#[test]
fn popover_geometry_stays_on_screen_and_clear_of_the_row_at_every_size_twin() {
    for (width, height) in [(80, 24), (60, 16), (44, 12), (46, 10), (140, 40), (30, 8)] {
        let area = Rect::new(0, 2, width, height - 2);
        let panel = panel_rect(area);
        for anchor_y in area.y..area.bottom() {
            let Some(geometry) = popover_geometry(area, panel, anchor_y, REASONS[0]) else {
                continue;
            };
            let rectangle = geometry.rectangle;
            assert!(rectangle.width >= 4 && rectangle.height >= MINIMUM_HEIGHT);
            assert!(rectangle.x >= area.x && rectangle.right() <= area.right());
            assert!(rectangle.y >= area.y && rectangle.bottom() <= area.bottom());
            assert!(
                area.height < 3 || rectangle.bottom() < area.bottom(),
                "keeps the voice row free: {rectangle:?}"
            );
            assert!(
                geometry.lines.len() <= usize::from(rectangle.height - 2),
                "text fits the box"
            );
            let row = Rect::new(panel.x, anchor_y, panel.width, 1);
            assert!(
                !rectangle.intersects(row),
                "{width}x{height} y={anchor_y}: {rectangle:?} covers the row"
            );
        }
    }
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::popover_needs_the_delay_and_is_dismissed_by_leaving_keys_clicks_and_wheel
#[test]
fn popover_needs_the_delay_and_is_dismissed_by_leaving_keys_clicks_and_wheel_twin() {
    let mut hover = HoverState::default();
    hover.set(Some(4), 1_000);
    assert!(hover.current().is_some_and(|state| !state.is_shown));
    assert!(
        !hover.tick(1_000 + HOVER_DELAY_MS - 1, true),
        "hidden until the delay"
    );
    assert!(hover.tick(1_000 + HOVER_DELAY_MS * 2, true));
    assert!(hover.current().is_some_and(|state| state.is_shown));
    // Moving within the row keeps it (and its first-arrival time); another row dismisses it.
    hover.set(Some(4), 2_000);
    assert!(hover
        .current()
        .is_some_and(|state| state.is_shown && state.since_ms == 1_000));
    hover.set(None, 2_000);
    assert!(hover.current().is_none(), "leaving the row dismisses");
    // A key, a click and the wheel all clear it.
    for _dismissal in 0..3 {
        hover.set(Some(4), 3_000);
        assert!(hover.tick(3_000 + HOVER_DELAY_MS, true));
        hover.clear();
        assert!(hover.current().is_none());
    }
    // Leaving the Animations tab drops a shown popover and reports the change once.
    hover.set(Some(4), 4_000);
    hover.tick(4_000 + HOVER_DELAY_MS, true);
    assert!(hover.tick(5_000, false));
    assert!(hover.current().is_none());
    assert!(!hover.tick(5_001, false));
}
