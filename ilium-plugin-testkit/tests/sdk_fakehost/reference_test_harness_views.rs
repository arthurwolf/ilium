//! reference-test-harness.md "Views and rendering": DEV-440 to DEV-446.
//!
//! The skeleton gives a native plugin no way to render (`NativeEntry` has no view method and
//! `FakeHost::render` has no way to reach `Views::render`), so every test that needs plugin
//! output sits behind `cfg(feature = "plugin_views")`. API_GAPS.md names the missing item. The
//! screen accessors and the size checks need no plugin view and are always compiled.

use ilium_plugin_sdk_core::types::{Color, NodeId, ViewTarget};
use ilium_plugin_testkit::errors::codes;
use ilium_plugin_testkit::{FakeHost, Screen, ScreenCell};

use crate::common::{Spy, assert_fails_with, panic_text, spy_reset, with_clean_env};

/// A screen whose rows are the given strings, one character per cell.
fn screen_of(columns: u16, rows: &[&str]) -> Screen {
    let mut cells = Vec::new();
    for row in rows {
        let mut symbols: Vec<String> = row.chars().map(|c| c.to_string()).collect();
        symbols.resize(columns as usize, " ".to_string());
        for symbol in symbols {
            cells.push(ScreenCell {
                symbol,
                foreground: Color::Rgb(0xffffff),
                background: Color::Rgb(0),
                bold: false,
                italic: false,
                underline: false,
            });
        }
    }
    Screen { columns, rows: rows.len() as u16, cells }
}

fn active_host() -> FakeHost {
    spy_reset();
    let mut host = FakeHost::new();
    host.activate::<Spy>();
    host
}

fn panel() -> ViewTarget {
    ViewTarget::Panel("main".to_string())
}

// ---- DEV-440 ---------------------------------------------------------------------------------

/// REQ: DEV-440
#[test]
fn dev_440_cell_takes_the_row_first_and_the_column_second() {
    let screen = screen_of(4, &["abcd", "efgh"]);
    assert_eq!(screen.cell(0, 3).symbol, "d");
    assert_eq!(screen.cell(1, 0).symbol, "e");
    assert_eq!(screen.cell(1, 2).symbol, "g");
}

/// REQ: DEV-440
#[test]
fn dev_440_both_indexes_start_at_zero() {
    let screen = screen_of(3, &["xyz"]);
    assert_eq!(screen.cell(0, 0).symbol, "x");
    assert_eq!(screen.cell(0, 2).symbol, "z");
}

/// REQ: DEV-440
#[test]
fn dev_440_a_row_past_the_last_row_fails_with_cell_out_of_range() {
    let screen = screen_of(4, &["abcd", "efgh"]);
    assert_eq!(screen.cell(1, 3).symbol, "h");
    assert_fails_with(codes::CELL_OUT_OF_RANGE, || screen.cell(2, 0));
}

/// REQ: DEV-440
#[test]
fn dev_440_a_column_past_the_last_column_fails_with_cell_out_of_range() {
    let screen = screen_of(4, &["abcd", "efgh"]);
    assert_fails_with(codes::CELL_OUT_OF_RANGE, || screen.cell(0, 4));
}

/// REQ: DEV-440
#[test]
fn dev_440_row_returns_the_text_of_one_row() {
    let screen = screen_of(4, &["abcd", "ef"]);
    assert_eq!(screen.row(0), "abcd");
    assert!(screen.row(1).starts_with("ef"));
}

/// REQ: DEV-440
#[test]
fn dev_440_find_returns_the_row_and_column_of_the_first_match() {
    let screen = screen_of(5, &["hello", "world"]);
    assert_eq!(screen.find("llo"), Some((0, 2)));
    assert_eq!(screen.find("orl"), Some((1, 1)));
    assert_eq!(screen.find("zzz"), None);
}

/// REQ: DEV-440
#[test]
fn dev_440_text_holds_every_row_in_order() {
    let screen = screen_of(3, &["abc", "def"]);
    let text = screen.text();
    assert_eq!(text.lines().count(), 2);
    assert!(text.find("abc").expect("row 0") < text.find("def").expect("row 1"));
}

// ---- DEV-443 ---------------------------------------------------------------------------------

/// REQ: DEV-443
#[test]
fn dev_443_a_width_below_10_fails_with_render_size() {
    with_clean_env(|| {
        let mut host = active_host();
        assert_fails_with(codes::RENDER_SIZE, || host.render(panel(), 9, 5));
    });
}

/// REQ: DEV-443
#[test]
fn dev_443_a_width_above_500_fails_with_render_size() {
    with_clean_env(|| {
        let mut host = active_host();
        assert_fails_with(codes::RENDER_SIZE, || host.render(panel(), 501, 5));
    });
}

/// REQ: DEV-443
#[test]
fn dev_443_a_height_of_zero_fails_with_render_size() {
    with_clean_env(|| {
        let mut host = active_host();
        assert_fails_with(codes::RENDER_SIZE, || host.render(panel(), 80, 0));
    });
}

/// REQ: DEV-443
#[test]
fn dev_443_a_height_above_200_fails_with_render_size() {
    with_clean_env(|| {
        let mut host = active_host();
        assert_fails_with(codes::RENDER_SIZE, || host.render(panel(), 80, 201));
    });
}

/// REQ: DEV-443
#[test]
fn dev_443_a_size_error_names_the_render_size_code_and_not_a_trap() {
    with_clean_env(|| {
        let mut host = active_host();
        let text = panel_text(&mut host, 0, 0);
        assert!(text.contains(codes::RENDER_SIZE), "{text}");
        assert!(!text.contains("trap."), "{text}");
    });
}

fn panel_text(host: &mut FakeHost, columns: u16, rows: u16) -> String {
    panic_text(|| host.render(panel(), columns, rows)).expect("render must fail")
}

// ---- Rendering of plugin output (gated, see API_GAPS.md) -------------------------------------

#[cfg(feature = "plugin_views")]
mod plugin_output {
    use std::cell::RefCell;

    use ilium_plugin_sdk_core::Views as ExportViews;
    use ilium_plugin_sdk_core::types::{
        Activation, Alignment, Area, ButtonRole, ButtonSpec, DeactivationReason, Event, NodeFlags,
        ParagraphSpec, PluginError, Span, Style, Submission, UiNode, UiTree, Widget,
    };
    use ilium_plugin_sdk_core::NativeEntry;

    use super::*;
    use crate::common::spy;

    /// What the next `render` call returns.
    #[derive(Clone)]
    pub enum Shape {
        Text(String),
        Button(String),
        RootOutOfRange,
        TooManyNodes(usize),
        DuplicateKeys,
    }

    thread_local! {
        static SHAPE: RefCell<Shape> = RefCell::new(Shape::Text(String::new()));
        static RENDER_COUNT: RefCell<u32> = RefCell::new(0);
        static EVENTS: RefCell<Vec<Event>> = RefCell::new(Vec::new());
    }

    pub fn set_shape(shape: Shape) {
        SHAPE.with(|cell| *cell.borrow_mut() = shape);
        RENDER_COUNT.with(|cell| *cell.borrow_mut() = 0);
        EVENTS.with(|cell| cell.borrow_mut().clear());
    }

    pub fn render_count() -> u32 {
        RENDER_COUNT.with(|cell| *cell.borrow())
    }

    pub fn received_events() -> Vec<Event> {
        EVENTS.with(|cell| cell.borrow().clone())
    }

    fn paragraph(text: &str, key: Option<&str>) -> UiNode {
        UiNode {
            key: key.map(str::to_string),
            widget: Widget::Paragraph(ParagraphSpec {
                lines: vec![vec![Span { text: text.to_string(), style: Style::default() }]],
                rich: None,
                wrap: None,
                alignment: Alignment::Left,
                style: Style::default(),
                scrollable: false,
            }),
            children: Vec::new(),
            flags: NodeFlags::empty(),
            tooltip: None,
            tab_index: None,
            disabled_reason: None,
        }
    }

    #[derive(Default)]
    pub struct ViewPlugin;

    impl NativeEntry for ViewPlugin {
        const SIDE: &'static str = "client";
        const EXPORTS: &'static [&'static str] = &["views", "handler"];

        fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
            Ok(())
        }

        fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
            None
        }

        fn native_on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
            EVENTS.with(|cell| cell.borrow_mut().extend(batch));
            Vec::new()
        }
    }

    impl ExportViews for ViewPlugin {
        fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
            RENDER_COUNT.with(|cell| *cell.borrow_mut() += 1);
            let shape = SHAPE.with(|cell| cell.borrow().clone());
            match shape {
                Shape::Text(text) => UiTree { version: 1, nodes: vec![paragraph(&text, None)], root: 0 },
                Shape::Button(label) => UiTree {
                    version: 1,
                    nodes: vec![UiNode {
                        key: Some("go".to_string()),
                        widget: Widget::Button(ButtonSpec {
                            label,
                            icon: None,
                            enabled: true,
                            role: ButtonRole::Primary,
                        }),
                        children: Vec::new(),
                        flags: NodeFlags::FOCUSABLE,
                        tooltip: None,
                        tab_index: None,
                        disabled_reason: None,
                    }],
                    root: 0,
                },
                Shape::RootOutOfRange => UiTree { version: 1, nodes: vec![paragraph("x", None)], root: 7 },
                Shape::TooManyNodes(count) => UiTree {
                    version: 1,
                    nodes: (0..count).map(|index| paragraph("x", Some(&format!("k{index}")))).collect(),
                    root: 0,
                },
                Shape::DuplicateKeys => UiTree {
                    version: 1,
                    nodes: vec![paragraph("a", Some("same")), paragraph("b", Some("same"))],
                    root: 0,
                },
            }
        }
    }

    fn view_host() -> FakeHost {
        let mut host = FakeHost::new().side("client");
        host.activate::<ViewPlugin>();
        host
    }

    /// REQ: DEV-441
    #[test]
    fn dev_441_render_draws_the_paragraph_text_at_the_top_left() {
        with_clean_env(|| {
            set_shape(Shape::Text("hello".to_string()));
            let mut host = view_host();
            let screen = host.render(panel(), 20, 4);
            assert_eq!((screen.columns, screen.rows), (20, 4));
            assert_eq!(screen.find("hello"), Some((0, 0)));
        });
    }

    /// REQ: DEV-441
    #[test]
    fn dev_441_a_wide_character_takes_two_columns() {
        with_clean_env(|| {
            set_shape(Shape::Text("\u{6f22}x".to_string()));
            let mut host = view_host();
            let screen = host.render(panel(), 20, 2);
            assert_eq!(screen.find("x"), Some((0, 2)));
        });
    }

    /// REQ: DEV-441
    #[test]
    fn dev_441_render_at_the_smallest_accepted_size_is_a_screen_of_that_size() {
        with_clean_env(|| {
            set_shape(Shape::Text("ab".to_string()));
            let mut host = view_host();
            let screen = host.render(panel(), 10, 1);
            assert_eq!((screen.columns, screen.rows, screen.cells.len()), (10, 1, 10));
        });
    }

    /// REQ: DEV-441, DEV-443
    #[test]
    fn dev_441_render_at_the_largest_accepted_size_is_a_screen_of_that_size() {
        with_clean_env(|| {
            set_shape(Shape::Text("ab".to_string()));
            let mut host = view_host();
            let screen = host.render(panel(), 500, 200);
            assert_eq!((screen.columns, screen.rows, screen.cells.len()), (500, 200, 100_000));
        });
    }

    /// REQ: DEV-442
    #[test]
    fn dev_442_a_root_index_out_of_range_fails_with_invalid_ui_tree() {
        with_clean_env(|| {
            set_shape(Shape::RootOutOfRange);
            let mut host = view_host();
            assert_fails_with("trap.invalid-ui-tree", || host.render(panel(), 20, 4));
        });
    }

    /// REQ: DEV-442
    #[test]
    fn dev_442_4097_nodes_breach_the_node_limit_and_fail_with_invalid_ui_tree() {
        with_clean_env(|| {
            set_shape(Shape::TooManyNodes(4097));
            let mut host = view_host();
            assert_fails_with("trap.invalid-ui-tree", || host.render(panel(), 20, 4));
        });
    }

    /// REQ: DEV-442
    #[test]
    fn dev_442_a_duplicate_key_fails_with_invalid_ui_tree() {
        with_clean_env(|| {
            set_shape(Shape::DuplicateKeys);
            let mut host = view_host();
            assert_fails_with("trap.invalid-ui-tree", || host.render(panel(), 20, 4));
        });
    }

    /// REQ: DEV-443
    #[test]
    fn dev_443_the_boundary_sizes_10_by_1_and_500_by_200_render_without_failure() {
        with_clean_env(|| {
            set_shape(Shape::Text("ok".to_string()));
            let mut host = view_host();
            assert_eq!(panic_text(|| host.render(panel(), 10, 1)), None);
            assert_eq!(panic_text(|| host.render(panel(), 500, 200)), None);
        });
    }

    /// REQ: DEV-444
    #[test]
    fn dev_444_a_click_on_the_button_label_returns_true_and_delivers_an_action() {
        with_clean_env(|| {
            set_shape(Shape::Button("Go".to_string()));
            let mut host = view_host();
            let screen = host.render(panel(), 20, 4);
            let (row, column) = screen.find("Go").expect("the label is drawn");
            assert!(host.click(panel(), column, row));
            host.settle();
            let actions = received_events()
                .into_iter()
                .filter(|event| matches!(event, Event::UiAction(_)))
                .count();
            assert_eq!(actions, 1);
        });
    }

    /// REQ: DEV-444
    #[test]
    fn dev_444_a_click_on_a_cell_with_no_action_returns_false() {
        with_clean_env(|| {
            set_shape(Shape::Button("Go".to_string()));
            let mut host = view_host();
            host.render(panel(), 20, 4);
            assert!(!host.click(panel(), 19, 3));
            assert!(received_events().iter().all(|event| !matches!(event, Event::UiAction(_))));
        });
    }

    /// REQ: DEV-445
    #[test]
    fn dev_445_rendering_twice_with_no_event_between_gives_equal_screens() {
        with_clean_env(|| {
            set_shape(Shape::Text("same".to_string()));
            let mut host = view_host();
            let first = host.render(panel(), 30, 5);
            let second = host.render(panel(), 30, 5);
            assert_eq!(first, second);
            assert_eq!(render_count(), 2);
        });
    }

    /// REQ: DEV-445
    #[test]
    fn dev_445_a_render_delivers_no_event_to_the_plugin() {
        with_clean_env(|| {
            set_shape(Shape::Text("same".to_string()));
            let mut host = view_host();
            host.render(panel(), 30, 5);
            assert!(received_events().is_empty());
            assert!(spy().batches.is_empty());
        });
    }

    /// REQ: DEV-446
    #[test]
    fn dev_446_all_five_view_target_cases_render() {
        with_clean_env(|| {
            set_shape(Shape::Text("t".to_string()));
            let mut host = view_host();
            for target in [
                ViewTarget::Panel("p".to_string()),
                ViewTarget::Pane(NodeId(1)),
                ViewTarget::Popup("p".to_string()),
                ViewTarget::SettingsPage("s".to_string()),
                ViewTarget::OnboardingStep("o".to_string()),
            ] {
                let screen = host.render(target.clone(), 20, 3);
                assert_eq!(screen.rows, 3, "{target:?}");
            }
        });
    }
}

// ---- DEV-446 (always compiled) ---------------------------------------------------------------

/// REQ: DEV-446
#[test]
fn dev_446_render_of_each_view_target_case_fails_only_by_size_when_the_size_is_bad() {
    with_clean_env(|| {
        let mut host = active_host();
        for target in [
            ViewTarget::Panel("p".to_string()),
            ViewTarget::Pane(NodeId(1)),
            ViewTarget::Popup("p".to_string()),
            ViewTarget::SettingsPage("s".to_string()),
            ViewTarget::OnboardingStep("o".to_string()),
        ] {
            let text = panic_text(|| host.render(target.clone(), 5, 5)).expect("size 5 is too small");
            assert!(text.contains(codes::RENDER_SIZE), "{target:?}: {text}");
        }
    });
}
