//! Test-support rig for the client widget renderer (WID-005..029, HST-250, HST-257).
//! Skeleton only: every body is `unimplemented!`; the implementation phase fills it in.

use ilium_plugin_api::generated::types::Constraint;

/// Solves one row of layout constraints into cell widths (WID-006, WID-007).
pub fn solve_row(_width: u16, _constraints: &[Constraint]) -> Vec<u16> {
    unimplemented!("client::plugin_host::widget_rig::solve_row")
}

/// Installs a plugin using `widget`; `Err` carries the diagnostic code (WID-022).
pub fn install_with_widget(_widget: &str, _listed_in_unstable: bool) -> Result<(), String> {
    unimplemented!("client::plugin_host::widget_rig::install_with_widget")
}

/// A recorded `ui-action` event sent to the plugin (WID-008, WID-027).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiActionRecord {
    /// Key of the node that was activated.
    pub key: String,
    /// Action id; equals the key for built-in widgets.
    pub action_id: String,
    /// Value carried by the action (for example "true" for a toggle).
    pub value: String,
}

impl UiActionRecord {
    /// Number of fields in the ui-action event (WID-027).
    pub fn field_count(&self) -> usize {
        unimplemented!("client::plugin_host::widget_rig::UiActionRecord::field_count")
    }
}

/// Renders a ui-tree into a screen buffer and feeds input back (WID-005..029).
#[derive(Debug)]
pub struct WidgetRig {
    _columns: u16,
    _rows: u16,
}

impl WidgetRig {
    /// Creates a rig with the given screen size (WID-005).
    pub fn new(_columns: u16, _rows: u16) -> WidgetRig {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::new")
    }

    /// Grants a capability by name (WID-020).
    pub fn grant(self, _name: &str) -> WidgetRig {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::grant")
    }

    /// Switches a terminal capability such as braille on or off (WID-024).
    pub fn capability(self, _name: &str, _on: bool) -> WidgetRig {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::capability")
    }

    /// Whole screen as text (WID-005..029).
    pub fn screen_text(&self) -> String {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::screen_text")
    }

    /// Screen rows as text (WID-015).
    pub fn screen_rows(&self) -> Vec<String> {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::screen_rows")
    }

    /// Text of the frame currently shown (WID-029).
    pub fn frame_text(&self) -> String {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::frame_text")
    }

    /// Milliseconds one paint took (HST-250, HST-257).
    pub fn paint_frame_ms(&mut self) -> u64 {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::paint_frame_ms")
    }

    /// Starts a frame, applying queued tree updates (WID-029).
    pub fn begin_frame(&mut self) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::begin_frame")
    }

    /// Ends the current frame (WID-029).
    pub fn end_frame(&mut self) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::end_frame")
    }

    /// Makes the plugin stop answering (HST-250).
    pub fn block_plugin(&mut self, _blocked: bool) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::block_plugin")
    }

    /// Advances the virtual clock (WID-009, WID-018).
    pub fn advance_ms(&mut self, _ms: u64) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::advance_ms")
    }

    /// Shows this widget (WID-008).
    pub fn show_button(&mut self, _key: &str, _label: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_button")
    }

    /// Shows this widget (WID-008).
    pub fn show_button_row(&mut self, _key: &str, _label: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_button_row")
    }

    /// Shows this widget (WID-018).
    pub fn show_button_with_tooltip(&mut self, _key: &str, _label: &str, _tooltip: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_button_with_tooltip")
    }

    /// Shows this widget (WID-008).
    pub fn show_toggle(&mut self, _key: &str, _label: &str, _value: bool) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_toggle")
    }

    /// Shows this widget (WID-008).
    pub fn show_confirm(&mut self, _key: &str, _label: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_confirm")
    }

    /// Shows this widget (WID-025).
    pub fn show_text_input(&mut self, _key: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_text_input")
    }

    /// Shows this widget (WID-009).
    pub fn show_text_area(&mut self, _key: &str, _debounce_ms: Option<u32>) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_text_area")
    }

    /// Shows this widget (WID-009).
    pub fn show_text_area_with_initial(
        &mut self,
        _key: &str,
        _debounce_ms: Option<u32>,
        _initial: &str,
    ) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_text_area_with_initial")
    }

    /// Shows this widget (WID-016).
    pub fn show_paragraph(&mut self, _text: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_paragraph")
    }

    /// Shows this widget (WID-016).
    pub fn show_paragraph_with_action_span(&mut self, _key: &str, _text: &str) {
        unimplemented!(
            "client::plugin_host::widget_rig::WidgetRig::show_paragraph_with_action_span"
        )
    }

    /// Shows this widget (WID-023).
    pub fn show_gauge(&mut self, _ratio: f64) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_gauge")
    }

    /// Shows this widget (WID-023).
    pub fn show_progress_row(&mut self, _percent: f32) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_progress_row")
    }

    /// Shows this widget (WID-024).
    pub fn show_chart(&mut self, _values: &[f64]) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_chart")
    }

    /// Shows this widget (WID-022).
    pub fn show_calendar(&mut self, _year: u32, _month: u8) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_calendar")
    }

    /// Shows this widget (WID-019).
    pub fn show_image(&mut self, _asset: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_image")
    }

    /// Shows this widget (WID-019).
    pub fn show_image_bytes(&mut self, _bytes: &[u8]) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_image_bytes")
    }

    /// Shows this widget (WID-019).
    pub fn show_image_of_size(&mut self, _bytes: usize) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_image_of_size")
    }

    /// Shows this widget (WID-020).
    pub fn show_code_view_foreign_buffer(&mut self) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_code_view_foreign_buffer")
    }

    /// Shows this widget (WID-020).
    pub fn show_code_view_missing_buffer(&mut self) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_code_view_missing_buffer")
    }

    /// Shows this widget (WID-020).
    pub fn show_terminal_view_pane(&mut self, _pane: u64) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_terminal_view_pane")
    }

    /// Shows this widget (WID-021).
    pub fn show_list_of(&mut self, _key: &str, _items: u32) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_list_of")
    }

    /// Shows this widget (WID-010).
    pub fn show_layout_with_popup(&mut self) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_layout_with_popup")
    }

    /// Shows this widget (WID-010).
    pub fn show_modal_popup(&mut self, _key: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_modal_popup")
    }

    /// Shows this widget (WID-010).
    pub fn show_popup_over_button(&mut self, _key: &str, _under: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_popup_over_button")
    }

    /// Shows this widget (WID-008).
    pub fn show_pointer_only_node(&mut self, _key: &str, _label: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_pointer_only_node")
    }

    /// Shows this widget (WID-011).
    pub fn show_escape_stack(&mut self) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_escape_stack")
    }

    /// Shows this widget (WID-026).
    pub fn show_focus_order_tree(&mut self, _order: &[(&str, Option<i16>)]) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_focus_order_tree")
    }

    /// Shows this widget (WID-029).
    pub fn queue_tree_paragraph(&mut self, _text: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::queue_tree_paragraph")
    }

    /// Shows a layout row; `Err` carries the diagnostic code (WID-005).
    pub fn show_layout(
        &mut self,
        _children: usize,
        _constraints: &[Constraint],
    ) -> Result<(), String> {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_layout")
    }

    /// Input or popup operation (WID-008).
    pub fn click_text(&mut self, _text: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::click_text")
    }

    /// Input or popup operation (WID-018).
    pub fn hover_text(&mut self, _text: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::hover_text")
    }

    /// Input or popup operation (WID-009).
    pub fn type_text(&mut self, _text: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::type_text")
    }

    /// Input or popup operation (WID-011).
    pub fn press_escape(&mut self) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::press_escape")
    }

    /// Input or popup operation (WID-011).
    pub fn press_escape_and_report_layer(&mut self) -> Option<&'static str> {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::press_escape_and_report_layer")
    }

    /// Input or popup operation (WID-026).
    pub fn press_tab(&mut self) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::press_tab")
    }

    /// Input or popup operation (WID-021).
    pub fn scroll_list(&mut self, _key: &str, _rows: u32) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::scroll_list")
    }

    /// Input or popup operation (WID-025).
    pub fn unfocus_view(&mut self) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::unfocus_view")
    }

    /// Input or popup operation (WID-012).
    pub fn open_popup(&mut self, _id: &str) -> Result<(), String> {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::open_popup")
    }

    /// Input or popup operation (WID-012).
    pub fn open_popup_in_view(&mut self, _view: u32, _id: &str) -> Result<(), String> {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::open_popup_in_view")
    }

    /// Input or popup operation (WID-012).
    pub fn close_popup(&mut self, _id: &str) -> Result<(), String> {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::close_popup")
    }

    /// State query (WID-026).
    pub fn focused_key(&self) -> Option<String> {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::focused_key")
    }

    /// State query (WID-025).
    pub fn plugin_requests_focus(&self, _key: &str) -> bool {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::plugin_requests_focus")
    }

    /// State query (WID-011).
    pub fn closed_events(&self) -> Vec<String> {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::closed_events")
    }

    /// State query (WID-008).
    pub fn actions(&self) -> Vec<UiActionRecord> {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::actions")
    }

    /// State query (WID-008).
    pub fn actions_for_other_views(&self) -> usize {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::actions_for_other_views")
    }

    /// State query (WID-010).
    pub fn layout_child_widths(&self) -> Vec<u16> {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::layout_child_widths")
    }

    /// State query (WID-023).
    pub fn gauge_filled_cells(&self) -> u16 {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::gauge_filled_cells")
    }

    /// State query (WID-018).
    pub fn tooltip_text(&self) -> Option<String> {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::tooltip_text")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use ilium_plugin_api::generated::types::Constraint;

    /// REQ: WID-007 (fixed and fill solved in order; cells sum to the area width)
    #[test]
    fn solve_row_fixed_then_fill_takes_the_remaining_width() {
        let widths = solve_row(40, &[Constraint::Length(10), Constraint::Fill(1)]);
        assert_eq!(widths, vec![10, 30]);
        assert_eq!(widths.iter().sum::<u16>(), 40);
    }

    /// REQ: WID-006 (a percentage above 100 is clamped to 100)
    #[test]
    fn solve_row_clamps_percentage_above_100_to_the_full_width() {
        let widths = solve_row(40, &[Constraint::Percentage(150)]);
        assert_eq!(widths, vec![40]);
    }

    /// REQ: WID-005 (a child and constraint count mismatch is an error, never a silent layout)
    #[test]
    fn show_layout_with_mismatched_constraint_count_is_an_error() {
        let mut rig = WidgetRig::new(40, 10);
        let result = rig.show_layout(3, &[Constraint::Length(10), Constraint::Fill(1)]);
        assert!(result.is_err(), "3 children with 2 constraints must fail");
    }

    /// REQ: WID-022 (calendar needs the unstable list; without it the install fails)
    #[test]
    fn install_with_calendar_not_listed_in_unstable_is_rejected() {
        let result = install_with_widget("calendar", false);
        let diagnostic = result.expect_err("calendar without unstable listing must fail");
        assert!(diagnostic.contains("manifest.unstable-missing"));
    }

    /// REQ: WID-008 and WID-027 (a toggle click reaches the plugin as a ui-action with key and action-id)
    #[test]
    fn clicking_a_toggle_sends_one_ui_action_with_key_as_action_id() {
        let mut rig = WidgetRig::new(40, 10);
        rig.show_toggle("dark", "Dark", false);
        rig.click_text("Dark");
        let actions = rig.actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].key, "dark");
        assert_eq!(actions[0].action_id, "dark");
        assert_eq!(actions[0].value, "true");
    }

    /// REQ: WID-027 (the ui-action event carries five fields)
    #[test]
    fn ui_action_event_has_five_fields() {
        let record = UiActionRecord {
            key: "dark".to_string(),
            action_id: "dark".to_string(),
            value: "true".to_string(),
        };
        assert_eq!(record.field_count(), 5);
    }

    /// REQ: WID-023 (ratio above 1 is drawn full; NaN is drawn empty)
    #[test]
    fn gauge_clamps_ratio_above_one_and_draws_nan_as_empty() {
        let mut rig = WidgetRig::new(40, 10);
        rig.show_gauge(1.0);
        let full_cells = rig.gauge_filled_cells();
        rig.show_gauge(1.5);
        assert_eq!(rig.gauge_filled_cells(), full_cells);
        rig.show_gauge(f64::NAN);
        assert_eq!(rig.gauge_filled_cells(), 0);
    }

    /// REQ: WID-012 (more than 4 open popups in one view fails with quota-exceeded)
    #[test]
    fn fifth_open_popup_in_one_view_fails_with_quota_exceeded() {
        let mut rig = WidgetRig::new(40, 10);
        for index in 1..=4 {
            let popup_id = format!("p{index}");
            rig.open_popup_in_view(0, &popup_id)
                .expect("the first four popups in a view must open");
        }
        let fifth = rig.open_popup_in_view(0, "p5");
        let diagnostic = fifth.expect_err("the fifth popup in one view must be refused");
        assert!(diagnostic.contains("quota-exceeded"));
    }

    /// REQ: WID-011 (Escape closes a modal popup, which must send closed)
    #[test]
    fn escape_closes_modal_popup_and_sends_closed() {
        let mut rig = WidgetRig::new(40, 10);
        rig.show_modal_popup("confirm");
        rig.press_escape();
        let closed = rig.closed_events();
        assert!(
            closed.iter().any(|event| event.contains("confirm")),
            "closed events were {closed:?}"
        );
    }

    /// REQ: WID-015 (a tooltip over 200 scalar values is cut at 199 and ends with the ellipsis)
    #[test]
    fn tooltip_longer_than_200_scalars_is_cut_to_200_ending_in_ellipsis() {
        let mut rig = WidgetRig::new(40, 10);
        let long_tooltip = "x".repeat(300);
        rig.show_button_with_tooltip("save", "Save", &long_tooltip);
        let shown = rig.tooltip_text().expect("tooltip must be shown");
        assert_eq!(shown.chars().count(), 200);
        assert!(shown.ends_with('\u{2026}'));
    }
}
