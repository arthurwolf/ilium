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
    pub fn show_text_area_with_initial(&mut self, _key: &str, _debounce_ms: Option<u32>, _initial: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_text_area_with_initial")
    }

    /// Shows this widget (WID-016).
    pub fn show_paragraph(&mut self, _text: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_paragraph")
    }

    /// Shows this widget (WID-016).
    pub fn show_paragraph_with_action_span(&mut self, _key: &str, _text: &str) {
        unimplemented!("client::plugin_host::widget_rig::WidgetRig::show_paragraph_with_action_span")
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
    pub fn show_layout(&mut self, _children: usize, _constraints: &[Constraint]) -> Result<(), String> {
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
