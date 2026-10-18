//! Render rigs: drive views, contributions, the mirror, the scene thread, surfaces and fonts of
//! the host render pipeline (test plan lane render, HST-250..271, RND-001..019, WID-001..030,
//! WIT:ilium:anim/*). Test support only. View ids are `u32`.

use std::collections::BTreeMap;

use ilium_plugin_api::generated::types::UiTree;

/// Drives views, contributions, the mirror and the scene thread of one plugin (HST-250..271).
#[derive(Debug)]
pub struct Rig {
    _private: (),
}

impl Rig {
    /// A rig for a plugin of the given tier.
    pub fn new(_tier: &str) -> Rig {
        unimplemented!("host::testing::render::Rig::new")
    }

    /// Adds a scene.
    pub fn with_scene(self) -> Rig {
        unimplemented!("host::testing::render::Rig::with_scene")
    }

    /// Adds a prerendered scene.
    pub fn with_prerendered_scene(self, _fps: u32) -> Rig {
        unimplemented!("host::testing::render::Rig::with_prerendered_scene")
    }

    /// Moves the clock.
    pub fn advance_ms(&mut self, _ms: u64) {
        unimplemented!("host::testing::render::Rig::advance_ms")
    }

    /// The clock.
    pub fn now_ms(&self) -> u64 {
        unimplemented!("host::testing::render::Rig::now_ms")
    }

    /// Paints a frame and returns its duration.
    pub fn paint_frame_ms(&mut self) -> u64 {
        unimplemented!("host::testing::render::Rig::paint_frame_ms")
    }

    /// The paint generation.
    pub fn paint_generation(&self) -> u64 {
        unimplemented!("host::testing::render::Rig::paint_generation")
    }

    /// Begins a frame.
    pub fn begin_frame(&mut self) {
        unimplemented!("host::testing::render::Rig::begin_frame")
    }

    /// Ends a frame.
    pub fn end_frame(&mut self) {
        unimplemented!("host::testing::render::Rig::end_frame")
    }

    /// Blocks the plugin executor.
    pub fn block_plugin(&mut self, _blocked: bool) {
        unimplemented!("host::testing::render::Rig::block_plugin")
    }

    /// Opens a view.
    pub fn open_view(&mut self, _name: &str) -> u32 {
        unimplemented!("host::testing::render::Rig::open_view")
    }

    /// Opens a settings view.
    pub fn open_settings_view(&mut self, _name: &str) -> u32 {
        unimplemented!("host::testing::render::Rig::open_settings_view")
    }

    /// Opens a code view with lines.
    pub fn open_code_view(&mut self, _name: &str, _lines: u32) -> u32 {
        unimplemented!("host::testing::render::Rig::open_code_view")
    }

    /// Open views.
    pub fn open_view_count(&self) -> usize {
        unimplemented!("host::testing::render::Rig::open_view_count")
    }

    /// Invalidates a view.
    pub fn invalidate(&mut self, _view: u32) {
        unimplemented!("host::testing::render::Rig::invalidate")
    }

    /// Resizes a view.
    pub fn resize_view(&mut self, _view: u32, _columns: u16, _rows: u16) {
        unimplemented!("host::testing::render::Rig::resize_view")
    }

    /// Changes the theme.
    pub fn change_theme(&mut self) {
        unimplemented!("host::testing::render::Rig::change_theme")
    }

    /// Changes the font.
    pub fn change_font(&mut self) {
        unimplemented!("host::testing::render::Rig::change_font")
    }

    /// Scrolls a view.
    pub fn scroll_view(&mut self, _view: u32, _rows: i32) {
        unimplemented!("host::testing::render::Rig::scroll_view")
    }

    /// A user action (click or key) inside a view (HST-251).
    pub fn user_action(&mut self, _view: u32) {
        unimplemented!("host::testing::render::Rig::user_action")
    }

    /// Focuses a view.
    pub fn focus_view(&mut self, _view: u32) {
        unimplemented!("host::testing::render::Rig::focus_view")
    }

    /// Render calls of a view.
    pub fn render_calls(&self, _view: u32) -> u32 {
        unimplemented!("host::testing::render::Rig::render_calls")
    }

    /// Renders in flight of a view.
    pub fn render_in_flight(&self, _view: u32) -> u32 {
        unimplemented!("host::testing::render::Rig::render_in_flight")
    }

    /// Renders queued of a view.
    pub fn render_queued(&self, _view: u32) -> u32 {
        unimplemented!("host::testing::render::Rig::render_queued")
    }

    /// Render start times of a view.
    pub fn render_start_times(&self, _view: u32) -> Vec<u64> {
        unimplemented!("host::testing::render::Rig::render_start_times")
    }

    /// Holds renders.
    pub fn hold_renders(&mut self, _hold: bool) {
        unimplemented!("host::testing::render::Rig::hold_renders")
    }

    /// Sets the cost of a render.
    pub fn set_render_cost_ms(&mut self, _ms: u64) {
        unimplemented!("host::testing::render::Rig::set_render_cost_ms")
    }

    /// The last trap code.
    pub fn last_trap(&self) -> Option<String> {
        unimplemented!("host::testing::render::Rig::last_trap")
    }

    /// The area of the last render of a view.
    pub fn last_render_area(&self, _view: u32) -> Option<(u16, u16)> {
        unimplemented!("host::testing::render::Rig::last_render_area")
    }

    /// The render deadline.
    pub fn render_deadline_ms(&self) -> u64 {
        unimplemented!("host::testing::render::Rig::render_deadline_ms")
    }

    /// The render deadline ceiling.
    pub fn render_deadline_ceiling_ms(&self) -> u64 {
        unimplemented!("host::testing::render::Rig::render_deadline_ceiling_ms")
    }

    /// Theme-changed events delivered.
    pub fn theme_changed_events(&self) -> u32 {
        unimplemented!("host::testing::render::Rig::theme_changed_events")
    }

    /// Calls the plugin received.
    pub fn plugin_calls_received(&self) -> u32 {
        unimplemented!("host::testing::render::Rig::plugin_calls_received")
    }

    /// Delivers a tree; `Err` is the trap code.
    pub fn deliver_tree(&mut self, _view: u32, _tree: UiTree) -> Result<(), String> {
        unimplemented!("host::testing::render::Rig::deliver_tree")
    }

    /// Delivers a tree of one widget.
    pub fn deliver_widget(&mut self, _view: u32, _widget: &str) -> Result<(), String> {
        unimplemented!("host::testing::render::Rig::deliver_widget")
    }

    /// Delivers a tree of one widget with items.
    pub fn deliver_widget_with_items(
        &mut self,
        _view: u32,
        _widget: &str,
        _items: u32,
    ) -> Result<(), String> {
        unimplemented!("host::testing::render::Rig::deliver_widget_with_items")
    }

    /// The tree shown in a view.
    pub fn shown_tree(&self, _view: u32) -> Option<UiTree> {
        unimplemented!("host::testing::render::Rig::shown_tree")
    }

    /// The tree of the current frame of a view.
    pub fn frame_tree(&self, _view: u32) -> Option<UiTree> {
        unimplemented!("host::testing::render::Rig::frame_tree")
    }

    /// Whether a view is stale.
    pub fn is_stale(&self, _view: u32) -> bool {
        unimplemented!("host::testing::render::Rig::is_stale")
    }

    /// Failures counted.
    pub fn failures(&self) -> u32 {
        unimplemented!("host::testing::render::Rig::failures")
    }

    /// Log lines as (code, message).
    pub fn log_lines(&self) -> Vec<(String, String)> {
        unimplemented!("host::testing::render::Rig::log_lines")
    }

    /// The screen text.
    pub fn screen_text(&self) -> String {
        unimplemented!("host::testing::render::Rig::screen_text")
    }

    /// Scrolls a node.
    pub fn scroll_node(&mut self, _view: u32, _key: &str, _rows: u32) {
        unimplemented!("host::testing::render::Rig::scroll_node")
    }

    /// The scroll of a node.
    pub fn node_scroll(&self, _view: u32, _key: &str) -> u32 {
        unimplemented!("host::testing::render::Rig::node_scroll")
    }

    /// The scroll of a node in the current frame.
    pub fn frame_scroll(&self, _view: u32, _key: &str) -> u32 {
        unimplemented!("host::testing::render::Rig::frame_scroll")
    }

    /// The user scrolls a node.
    pub fn user_scroll(&mut self, _view: u32, _key: &str, _rows: u32) {
        unimplemented!("host::testing::render::Rig::user_scroll")
    }

    /// Selects a tab.
    pub fn select_tab(&mut self, _view: u32, _key: &str, _tab: u32) {
        unimplemented!("host::testing::render::Rig::select_tab")
    }

    /// The selected tab.
    pub fn selected_tab(&self, _view: u32, _key: &str) -> u32 {
        unimplemented!("host::testing::render::Rig::selected_tab")
    }

    /// Sets a cursor.
    pub fn set_cursor(&mut self, _view: u32, _key: &str, _at: u32) {
        unimplemented!("host::testing::render::Rig::set_cursor")
    }

    /// The cursor.
    pub fn cursor(&self, _view: u32, _key: &str) -> u32 {
        unimplemented!("host::testing::render::Rig::cursor")
    }

    /// Whether a view was repainted.
    pub fn view_repainted(&self, _view: u32) -> bool {
        unimplemented!("host::testing::render::Rig::view_repainted")
    }

    /// Hit-tests a cell.
    pub fn hit_test(&mut self, _view: u32, _column: u16, _row: u16) -> Option<String> {
        unimplemented!("host::testing::render::Rig::hit_test")
    }

    /// Whether the last hit was resolved on the UI thread.
    pub fn hit_resolved_on_ui_thread(&self) -> bool {
        unimplemented!("host::testing::render::Rig::hit_resolved_on_ui_thread")
    }

    /// The user focuses a view.
    pub fn user_focus(&mut self, _view: u32) {
        unimplemented!("host::testing::render::Rig::user_focus")
    }

    /// The user unfocuses a view.
    pub fn user_unfocus(&mut self, _view: u32) {
        unimplemented!("host::testing::render::Rig::user_unfocus")
    }

    /// The plugin sets focus.
    pub fn plugin_set_focus(&mut self, _view: u32, _key: &str) -> bool {
        unimplemented!("host::testing::render::Rig::plugin_set_focus")
    }

    /// The focused key.
    pub fn focused_key(&self, _view: u32) -> Option<String> {
        unimplemented!("host::testing::render::Rig::focused_key")
    }

    /// The server side contributes a node.
    pub fn server_contribute(
        &mut self,
        _point: &str,
        _node: &str,
        _text: &str,
    ) -> Result<(), String> {
        unimplemented!("host::testing::render::Rig::server_contribute")
    }

    /// The client side contributes a node.
    pub fn client_contribute(
        &mut self,
        _point: &str,
        _node: &str,
        _text: &str,
    ) -> Result<(), String> {
        unimplemented!("host::testing::render::Rig::client_contribute")
    }

    /// Entries in the server registry of a point.
    pub fn server_registry_len(&self, _point: &str) -> usize {
        unimplemented!("host::testing::render::Rig::server_registry_len")
    }

    /// Entries in the client store of a point.
    pub fn client_store_len(&self, _point: &str) -> usize {
        unimplemented!("host::testing::render::Rig::client_store_len")
    }

    /// Entries in the client mirror of a point.
    pub fn client_mirror_len(&self, _point: &str) -> usize {
        unimplemented!("host::testing::render::Rig::client_mirror_len")
    }

    /// The contribution cap of a point.
    pub fn contribution_cap(&self, _point: &str) -> u32 {
        unimplemented!("host::testing::render::Rig::contribution_cap")
    }

    /// Whether a contribution is stale.
    pub fn contribution_stale(&self, _point: &str, _node: &str) -> bool {
        unimplemented!("host::testing::render::Rig::contribution_stale")
    }

    /// Whether a mirror entry is stale.
    pub fn mirror_stale(&self, _point: &str, _node: &str) -> bool {
        unimplemented!("host::testing::render::Rig::mirror_stale")
    }

    /// Whether a mirror entry is recoloured.
    pub fn mirror_recoloured(&self, _point: &str, _node: &str) -> bool {
        unimplemented!("host::testing::render::Rig::mirror_recoloured")
    }

    /// Syncs the mirror.
    pub fn sync_mirror(&mut self) {
        unimplemented!("host::testing::render::Rig::sync_mirror")
    }

    /// Stops the plugin side.
    pub fn stop_plugin_side(&mut self, _restart_scheduled: bool) {
        unimplemented!("host::testing::render::Rig::stop_plugin_side")
    }

    /// Uninstalls the plugin.
    pub fn uninstall_plugin(&mut self) {
        unimplemented!("host::testing::render::Rig::uninstall_plugin")
    }

    /// Disables the plugin.
    pub fn disable_plugin(&mut self) {
        unimplemented!("host::testing::render::Rig::disable_plugin")
    }

    /// Detaches the client.
    pub fn detach_client(&mut self) {
        unimplemented!("host::testing::render::Rig::detach_client")
    }

    /// Attaches a client with a snapshot.
    pub fn attach_client_with_snapshot(&mut self) {
        unimplemented!("host::testing::render::Rig::attach_client_with_snapshot")
    }

    /// Activates a contribution.
    pub fn activate_contribution(&mut self, _point: &str, _node: &str) -> Option<String> {
        unimplemented!("host::testing::render::Rig::activate_contribution")
    }

    /// Sets the visible rows of a view.
    pub fn set_visible_rows(&mut self, _view: u32, _first: u32, _last: u32) {
        unimplemented!("host::testing::render::Rig::set_visible_rows")
    }

    /// The last annotate range.
    pub fn last_annotate_range(&self, _view: u32) -> Option<(u32, u32)> {
        unimplemented!("host::testing::render::Rig::last_annotate_range")
    }

    /// The last tokens range.
    pub fn last_tokens_range(&self, _view: u32) -> Option<(u32, u32)> {
        unimplemented!("host::testing::render::Rig::last_tokens_range")
    }

    /// Annotate call times.
    pub fn annotate_times(&self, _view: u32) -> Vec<u64> {
        unimplemented!("host::testing::render::Rig::annotate_times")
    }

    /// Delays annotate.
    pub fn delay_annotate(&mut self, _view: u32, _ms: u64) {
        unimplemented!("host::testing::render::Rig::delay_annotate")
    }

    /// Delays tokens.
    pub fn delay_tokens(&mut self, _view: u32, _ms: u64) {
        unimplemented!("host::testing::render::Rig::delay_tokens")
    }

    /// Annotate returns the given number of spans.
    pub fn annotate_returns_spans(&mut self, _view: u32, _spans: u32) {
        unimplemented!("host::testing::render::Rig::annotate_returns_spans")
    }

    /// Annotation spans applied.
    pub fn applied_annotation_spans(&self, _view: u32) -> u32 {
        unimplemented!("host::testing::render::Rig::applied_annotation_spans")
    }

    /// Token spans applied.
    pub fn applied_token_spans(&self, _view: u32) -> u32 {
        unimplemented!("host::testing::render::Rig::applied_token_spans")
    }

    /// The scene begins a frame.
    pub fn scene_begin_frame(&mut self) {
        unimplemented!("host::testing::render::Rig::scene_begin_frame")
    }

    /// The scene writes a row.
    pub fn scene_write_row(&mut self, _row: u32, _value: u8) {
        unimplemented!("host::testing::render::Rig::scene_write_row")
    }

    /// The scene presents.
    pub fn scene_present(&mut self) {
        unimplemented!("host::testing::render::Rig::scene_present")
    }

    /// Scene rows.
    pub fn scene_rows(&self) -> u32 {
        unimplemented!("host::testing::render::Rig::scene_rows")
    }

    /// The front buffer rows.
    pub fn front_buffer_rows(&self) -> Vec<u8> {
        unimplemented!("host::testing::render::Rig::front_buffer_rows")
    }

    /// One front buffer row.
    pub fn front_buffer_row(&self, _row: u32) -> u8 {
        unimplemented!("host::testing::render::Rig::front_buffer_row")
    }

    /// One presented row.
    pub fn presented_row(&self, _row: u32) -> u8 {
        unimplemented!("host::testing::render::Rig::presented_row")
    }

    /// Sets the cost of a scene frame.
    pub fn scene_frame_cost_ms(&mut self, _ms: u64) {
        unimplemented!("host::testing::render::Rig::scene_frame_cost_ms")
    }

    /// Skipped frames.
    pub fn skipped_frames(&self) -> u32 {
        unimplemented!("host::testing::render::Rig::skipped_frames")
    }

    /// Sets the look invert.
    pub fn set_look_invert(&mut self, _invert: bool) {
        unimplemented!("host::testing::render::Rig::set_look_invert")
    }

    /// Scene render calls.
    pub fn scene_render_calls(&self) -> u32 {
        unimplemented!("host::testing::render::Rig::scene_render_calls")
    }

    /// Invalidates the clip.
    pub fn invalidate_clip(&mut self) {
        unimplemented!("host::testing::render::Rig::invalidate_clip")
    }

    /// Starts a scene.
    pub fn start_scene(&mut self, _name: &str) {
        unimplemented!("host::testing::render::Rig::start_scene")
    }

    /// Active scenes.
    pub fn active_scene_count(&self) -> usize {
        unimplemented!("host::testing::render::Rig::active_scene_count")
    }

    /// The active scene.
    pub fn active_scene_name(&self) -> Option<String> {
        unimplemented!("host::testing::render::Rig::active_scene_name")
    }

    /// Turns the background on or off.
    pub fn set_background(&mut self, _on: bool) {
        unimplemented!("host::testing::render::Rig::set_background")
    }

    /// Hides the window.
    pub fn set_window_hidden(&mut self, _hidden: bool) {
        unimplemented!("host::testing::render::Rig::set_window_hidden")
    }
}

/// Drives the lifecycle of one scene plugin (RND-001..014, WIT renderer exports).
#[derive(Debug)]
pub struct SceneRig {
    _private: (),
}

impl SceneRig {
    /// A scene rig at the given frame rate.
    pub fn new(_fps: u32) -> SceneRig {
        unimplemented!("host::testing::render::SceneRig::new")
    }

    /// The scene flags.
    pub fn flags(
        self,
        _uses_cell_colors: bool,
        _wants_occupancy: bool,
        _wants_pointer: bool,
    ) -> SceneRig {
        unimplemented!("host::testing::render::SceneRig::flags")
    }

    /// A grant.
    pub fn grant(self, _name: &str) -> SceneRig {
        unimplemented!("host::testing::render::SceneRig::grant")
    }

    /// The palette.
    pub fn palette(self, _rgb: &[u32]) -> SceneRig {
        unimplemented!("host::testing::render::SceneRig::palette")
    }

    /// The colour mode.
    pub fn colour_mode(self, _mode: &str) -> SceneRig {
        unimplemented!("host::testing::render::SceneRig::colour_mode")
    }

    /// The area.
    pub fn area(self, _columns: u32, _rows: u32) -> SceneRig {
        unimplemented!("host::testing::render::SceneRig::area")
    }

    /// Runs frames.
    pub fn run_frames(&mut self, _count: u32) {
        unimplemented!("host::testing::render::SceneRig::run_frames")
    }

    /// Runs for a duration.
    pub fn run_for_ms(&mut self, _ms: u64) {
        unimplemented!("host::testing::render::SceneRig::run_for_ms")
    }

    /// Sets the frame cost.
    pub fn frame_cost_ms(&mut self, _ms: u64) {
        unimplemented!("host::testing::render::SceneRig::frame_cost_ms")
    }

    /// The scene never presents.
    pub fn never_present(&mut self) {
        unimplemented!("host::testing::render::SceneRig::never_present")
    }

    /// Scripts readiness.
    pub fn readiness_script(&mut self, _steps: &[&str]) {
        unimplemented!("host::testing::render::SceneRig::readiness_script")
    }

    /// The scene is ready after a delay.
    pub fn readiness_after_ms(&mut self, _ms: u64) {
        unimplemented!("host::testing::render::SceneRig::readiness_after_ms")
    }

    /// What reconfigure returns.
    pub fn reconfigure_returns(&mut self, _keep: bool) {
        unimplemented!("host::testing::render::SceneRig::reconfigure_returns")
    }

    /// Reconfigures.
    pub fn reconfigure(&mut self, _spec: &str) {
        unimplemented!("host::testing::render::SceneRig::reconfigure")
    }

    /// Changes a setting.
    pub fn change_setting(&mut self, _key: &str, _value: &str) -> Result<(), String> {
        unimplemented!("host::testing::render::SceneRig::change_setting")
    }

    /// A setting.
    pub fn setting(&self, _key: &str) -> Option<String> {
        unimplemented!("host::testing::render::SceneRig::setting")
    }

    /// Revokes a grant.
    pub fn revoke(&mut self, _name: &str) {
        unimplemented!("host::testing::render::SceneRig::revoke")
    }

    /// Changes the palette.
    pub fn change_palette(&mut self, _rgb: &[u32]) {
        unimplemented!("host::testing::render::SceneRig::change_palette")
    }

    /// The next frame traps.
    pub fn trap_in_next_frame(&mut self) {
        unimplemented!("host::testing::render::SceneRig::trap_in_next_frame")
    }

    /// The next frame allocates.
    pub fn allocate_in_next_frame_mib(&mut self, _mib: u32) {
        unimplemented!("host::testing::render::SceneRig::allocate_in_next_frame_mib")
    }

    /// The constructor fails.
    pub fn constructor_fails(&mut self) {
        unimplemented!("host::testing::render::SceneRig::constructor_fails")
    }

    /// Sets the status text.
    pub fn status_text(&mut self, _text: &str) {
        unimplemented!("host::testing::render::SceneRig::status_text")
    }

    /// Failures counted.
    pub fn failure_count(&self) -> u32 {
        unimplemented!("host::testing::render::SceneRig::failure_count")
    }

    /// Restarts.
    pub fn restarts(&self) -> u32 {
        unimplemented!("host::testing::render::SceneRig::restarts")
    }

    /// The restart delay.
    pub fn restart_delay_ms(&self) -> Option<u64> {
        unimplemented!("host::testing::render::SceneRig::restart_delay_ms")
    }

    /// Skipped frames.
    pub fn skipped_frames(&self) -> u32 {
        unimplemented!("host::testing::render::SceneRig::skipped_frames")
    }

    /// Presented frames.
    pub fn presented_frames(&self) -> u32 {
        unimplemented!("host::testing::render::SceneRig::presented_frames")
    }

    /// Render calls.
    pub fn render_calls(&self) -> u32 {
        unimplemented!("host::testing::render::SceneRig::render_calls")
    }

    /// Constructor calls.
    pub fn constructor_calls(&self) -> u32 {
        unimplemented!("host::testing::render::SceneRig::constructor_calls")
    }

    /// Reconfigure calls.
    pub fn reconfigure_calls(&self) -> u32 {
        unimplemented!("host::testing::render::SceneRig::reconfigure_calls")
    }

    /// The last trap.
    pub fn trap(&self) -> Option<String> {
        unimplemented!("host::testing::render::SceneRig::trap")
    }

    /// Log lines as (code, message).
    pub fn log_lines(&self) -> Vec<(String, String)> {
        unimplemented!("host::testing::render::SceneRig::log_lines")
    }

    /// The current spec.
    pub fn current_spec(&self) -> String {
        unimplemented!("host::testing::render::SceneRig::current_spec")
    }

    /// The spec of the last constructor call.
    pub fn last_constructor_spec(&self) -> String {
        unimplemented!("host::testing::render::SceneRig::last_constructor_spec")
    }

    /// The spec of the last reconfigure call.
    pub fn last_reconfigure_spec(&self) -> Option<String> {
        unimplemented!("host::testing::render::SceneRig::last_reconfigure_spec")
    }

    /// The grants given to the constructor.
    pub fn constructor_grants(&self) -> Vec<String> {
        unimplemented!("host::testing::render::SceneRig::constructor_grants")
    }

    /// The palette given to the constructor.
    pub fn constructor_palette(&self) -> Vec<u32> {
        unimplemented!("host::testing::render::SceneRig::constructor_palette")
    }

    /// Every `set-palette` call.
    pub fn set_palette_calls(&self) -> Vec<Vec<u32>> {
        unimplemented!("host::testing::render::SceneRig::set_palette_calls")
    }

    /// Failures of a plugin side.
    pub fn plugin_side_failures(&self, _side: &str) -> u32 {
        unimplemented!("host::testing::render::SceneRig::plugin_side_failures")
    }

    /// Presented cell colours.
    pub fn presented_cell_colors(&self) -> Vec<u32> {
        unimplemented!("host::testing::render::SceneRig::presented_cell_colors")
    }

    /// Scene control ids.
    pub fn scene_control_ids(&self) -> Vec<String> {
        unimplemented!("host::testing::render::SceneRig::scene_control_ids")
    }

    /// The frame rate.
    pub fn fps(&self) -> u32 {
        unimplemented!("host::testing::render::SceneRig::fps")
    }

    /// The frame budget.
    pub fn budget_ms(&self) -> u64 {
        unimplemented!("host::testing::render::SceneRig::budget_ms")
    }

    /// Calls of an export.
    pub fn query_count(&self, _export: &str) -> u32 {
        unimplemented!("host::testing::render::SceneRig::query_count")
    }

    /// Frame numbers given to render.
    pub fn render_frame_numbers(&self) -> Vec<u64> {
        unimplemented!("host::testing::render::SceneRig::render_frame_numbers")
    }

    /// Surface sizes given to render.
    pub fn render_surface_sizes(&self) -> Vec<(u32, u32)> {
        unimplemented!("host::testing::render::SceneRig::render_surface_sizes")
    }

    /// The pointer the scene saw.
    pub fn scene_saw_pointer(&self) -> Option<(u32, u32)> {
        unimplemented!("host::testing::render::SceneRig::scene_saw_pointer")
    }

    /// The occupancy the scene saw.
    pub fn scene_saw_occupancy(&self) -> Option<Vec<u8>> {
        unimplemented!("host::testing::render::SceneRig::scene_saw_occupancy")
    }

    /// The status shown.
    pub fn shown_status(&self) -> Option<String> {
        unimplemented!("host::testing::render::SceneRig::shown_status")
    }

    /// Moves the pointer.
    pub fn move_pointer(&mut self, _column: u32, _row: u32) {
        unimplemented!("host::testing::render::SceneRig::move_pointer")
    }
}

/// The host side of one frame surface (RND-004..006, RND-016, WIT:ilium:anim/surface).
#[derive(Debug)]
pub struct SurfaceRig {
    _private: (),
}

impl SurfaceRig {
    /// A surface of the given size and format.
    pub fn new(_width: u32, _height: u32, _format: &str) -> SurfaceRig {
        unimplemented!("host::testing::render::SurfaceRig::new")
    }

    /// The scene flags.
    pub fn flags(
        self,
        _uses_cell_colors: bool,
        _wants_occupancy: bool,
        _wants_pointer: bool,
    ) -> SurfaceRig {
        unimplemented!("host::testing::render::SurfaceRig::flags")
    }

    /// Writes rows.
    pub fn write_rows(&mut self, _first_row: u32, _bytes: &[u8]) -> Result<(), String> {
        unimplemented!("host::testing::render::SurfaceRig::write_rows")
    }

    /// Writes glyphs.
    pub fn write_glyphs(&mut self, _cells: u32) -> Result<(), String> {
        unimplemented!("host::testing::render::SurfaceRig::write_glyphs")
    }

    /// Writes cell colours.
    pub fn write_cell_colors(&mut self, _row: u32, _colors: &[u32]) {
        unimplemented!("host::testing::render::SurfaceRig::write_cell_colors")
    }

    /// Clears glyphs.
    pub fn clear_glyphs(&mut self) {
        unimplemented!("host::testing::render::SurfaceRig::clear_glyphs")
    }

    /// Sets owner ids.
    pub fn set_owner_ids(&mut self, _ids: &[u32]) {
        unimplemented!("host::testing::render::SurfaceRig::set_owner_ids")
    }

    /// Sets owner ids, checked.
    pub fn set_owner_ids_checked(&mut self, _ids: &[u32]) -> Result<(), String> {
        unimplemented!("host::testing::render::SurfaceRig::set_owner_ids_checked")
    }

    /// Presents; returns the receipt.
    pub fn present(&mut self) -> Result<u64, String> {
        unimplemented!("host::testing::render::SurfaceRig::present")
    }

    /// The last receipt.
    pub fn last_receipt(&self) -> Option<u64> {
        unimplemented!("host::testing::render::SurfaceRig::last_receipt")
    }

    /// The owner ids of the last receipt.
    pub fn last_receipt_owner_ids(&self) -> Vec<u32> {
        unimplemented!("host::testing::render::SurfaceRig::last_receipt_owner_ids")
    }

    /// The size.
    pub fn size(&self) -> (u32, u32) {
        unimplemented!("host::testing::render::SurfaceRig::size")
    }

    /// The occupancy.
    pub fn occupancy(&self) -> Option<Vec<u8>> {
        unimplemented!("host::testing::render::SurfaceRig::occupancy")
    }

    /// The pointer.
    pub fn pointer(&self) -> Option<(u32, u32)> {
        unimplemented!("host::testing::render::SurfaceRig::pointer")
    }

    /// Moves the pointer.
    pub fn move_pointer(&mut self, _column: u32, _row: u32) {
        unimplemented!("host::testing::render::SurfaceRig::move_pointer")
    }

    /// The bytes of a row.
    pub fn row_bytes(&self, _row: u32) -> Vec<u8> {
        unimplemented!("host::testing::render::SurfaceRig::row_bytes")
    }

    /// The presented bytes of a row.
    pub fn presented_row(&self, _row: u32) -> Vec<u8> {
        unimplemented!("host::testing::render::SurfaceRig::presented_row")
    }

    /// The cell colours of a row.
    pub fn cell_colors(&self, _row: u32) -> Vec<u32> {
        unimplemented!("host::testing::render::SurfaceRig::cell_colors")
    }

    /// Glyphs written.
    pub fn glyph_count(&self) -> u32 {
        unimplemented!("host::testing::render::SurfaceRig::glyph_count")
    }

    /// The frame as text.
    pub fn frame_text(&self) -> String {
        unimplemented!("host::testing::render::SurfaceRig::frame_text")
    }

    /// Warnings counted.
    pub fn warn_count(&self) -> usize {
        unimplemented!("host::testing::render::SurfaceRig::warn_count")
    }

    /// Moves the clock.
    pub fn advance_ms(&mut self, _ms: u64) {
        unimplemented!("host::testing::render::SurfaceRig::advance_ms")
    }
}

/// The host side of one font resource (WIT:ilium:anim/fonts).
#[derive(Debug)]
pub struct Font {
    _private: (),
}

impl Font {
    /// Opens a font by name.
    pub fn new(_name: &str) -> Result<Font, String> {
        unimplemented!("host::testing::render::Font::new")
    }

    /// The cell size.
    pub fn cell_size(&self) -> (u32, u32) {
        unimplemented!("host::testing::render::Font::cell_size")
    }

    /// Rasterizes a character.
    pub fn rasterize(&self, _character: char) -> Result<Vec<u8>, String> {
        unimplemented!("host::testing::render::Font::rasterize")
    }

    /// Rasterizes a code point.
    pub fn rasterize_code(&self, _code: u32) -> Result<Vec<u8>, String> {
        unimplemented!("host::testing::render::Font::rasterize_code")
    }
}

/// Display width of text in cells (WID-019).
pub fn cell_width(_text: &str) -> usize {
    unimplemented!("host::testing::render::cell_width")
}

/// Sanitises text for display (RND-019).
pub fn sanitise(_text: &str, _single_line: bool) -> String {
    unimplemented!("host::testing::render::sanitise")
}

/// The tooltip text shown for a tooltip (WID-015).
pub fn tooltip_shown(_text: &str) -> String {
    unimplemented!("host::testing::render::tooltip_shown")
}

/// The tab order of nodes given (tab index, focusable); 1-based node numbers (WID-018).
pub fn tab_order(_nodes: &[(Option<i16>, bool)]) -> Vec<usize> {
    unimplemented!("host::testing::render::tab_order")
}

/// The next node in tab order (WID-018).
pub fn tab_next(_order: &[usize], _current: usize) -> usize {
    unimplemented!("host::testing::render::tab_next")
}

/// The widget families and their widgets (WID-001).
pub fn widget_families() -> BTreeMap<&'static str, Vec<&'static str>> {
    unimplemented!("host::testing::render::widget_families")
}

/// The hit layers in test order (HST-270).
pub fn hit_layers_in_order() -> Vec<&'static str> {
    unimplemented!("host::testing::render::hit_layers_in_order")
}

/// The frame budget of a scene (RND-001).
pub fn frame_budget_ms(_fps: u32, _frame_deadline_ms: Option<u32>) -> u64 {
    unimplemented!("host::testing::render::frame_budget_ms")
}

/// The scene memory limit of a tier (RND-014).
pub fn scene_memory_limit_mib(_tier: &str, _requested: Option<u32>) -> u32 {
    unimplemented!("host::testing::render::scene_memory_limit_mib")
}

/// Installs a prerendered animation (RND-017).
pub fn install_prerender(_frames: u32, _fps: u32, _mib: u32) -> Result<(), String> {
    unimplemented!("host::testing::render::install_prerender")
}

/// Sets a decoration with frames (RND-018).
pub fn set_decoration_with_frames(_frames: usize, _frame_ms: u16) -> Result<(), String> {
    unimplemented!("host::testing::render::set_decoration_with_frames")
}

/// A tree with one text node (RND-019).
pub fn tree_with_text(_text: &str) -> UiTree {
    unimplemented!("host::testing::render::tree_with_text")
}

/// A tree with one tooltip (WID-015).
pub fn tree_with_tooltip(_text: &str) -> UiTree {
    unimplemented!("host::testing::render::tree_with_tooltip")
}

/// A tree whose nodes stack for hit testing (HST-270).
pub fn hit_stack_tree() -> UiTree {
    unimplemented!("host::testing::render::hit_stack_tree")
}
