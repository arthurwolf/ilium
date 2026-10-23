//! Render rigs: drive views, contributions, the mirror, the scene thread, surfaces and fonts of
//! the host render pipeline (test plan lane render, HST-250..271, RND-001..019, WID-001..030,
//! WIT:ilium:anim/*). Test support only. View ids are `u32`.
//!
//! The rigs keep their own in-memory state: a view holds its last tree and render counters, a
//! scene holds a row buffer, a surface holds a byte grid. Rules the host crate owns (tree diffs,
//! text width) are called from the host, so a stubbed host function fails the rig.

use std::collections::BTreeMap;
use std::iter::Peekable;
use std::str::Chars;

use ilium_plugin_api::generated::types::{
    Alignment, NodeFlags, ParagraphSpec, Span, Style, Tooltip, UiNode, UiTree, Widget,
};

/// The render deadline the rig reports by default.
const RENDER_DEADLINE_MS: u64 = 33;
/// The ceiling of a render or frame deadline (RND-001).
const RENDER_DEADLINE_CEILING_MS: u64 = 50;
/// Contributions one point holds before a new node is refused.
const CONTRIBUTION_CAP: u32 = 64;
/// Settings a scene accepts through `change-setting`.
const SCENE_CONTROL_IDS: &[&str] = &["speed", "density"];

/// The state of one open view.
#[derive(Debug, Default)]
struct ViewState {
    code_lines: Option<u32>,
    columns: u16,
    rows: u16,
    shown: Option<UiTree>,
    frame: Option<UiTree>,
    frame_scroll: BTreeMap<String, u32>,
    stale: bool,
    repainted: bool,
    render_calls: u32,
    render_starts: Vec<u64>,
    queued: u32,
    last_area: Option<(u16, u16)>,
    scroll: BTreeMap<String, u32>,
    tabs: BTreeMap<String, u32>,
    cursors: BTreeMap<String, u32>,
    focus: Option<String>,
    annotate_times: Vec<u64>,
    annotate_delay: u64,
    tokens_delay: u64,
    annotate_spans: u32,
    applied_annotation: u32,
    applied_tokens: u32,
    last_annotate: Option<(u32, u32)>,
    last_tokens: Option<(u32, u32)>,
    sanitised_warned: bool,
}

/// One replicated contribution on the client side.
#[derive(Debug, Default, Clone)]
struct MirrorEntry {
    stale: bool,
    recoloured: bool,
}

/// Drives views, contributions, the mirror and the scene thread of one plugin (HST-250..271).
#[derive(Debug, Default)]
pub struct Rig {
    clock_ms: u64,
    blocked: bool,
    hold: bool,
    window_hidden: bool,
    background: bool,
    render_cost_ms: u64,
    paint_generation: u64,
    views: BTreeMap<u32, ViewState>,
    next_view: u32,
    focused_view: Option<u32>,
    failures: u32,
    logs: Vec<(String, String)>,
    last_trap: Option<String>,
    plugin_calls: u32,
    theme_events: u32,
    scenes: Vec<String>,
    prerender_fps: Option<u32>,
    scene_rows: u32,
    scene_back: Vec<u8>,
    scene_front: Vec<u8>,
    scene_presented: Vec<u8>,
    scene_frame_cost_ms: u64,
    scene_skipped: u32,
    scene_render_calls: u32,
    look_invert: bool,
    server_registry: BTreeMap<String, BTreeMap<String, String>>,
    client_store: BTreeMap<String, BTreeMap<String, String>>,
    client_mirror: BTreeMap<String, BTreeMap<String, MirrorEntry>>,
    plugin_stopped: bool,
    restart_scheduled: bool,
    uninstalled: bool,
    disabled: bool,
    attached: bool,
    hit_on_ui_thread: bool,
}

impl Rig {
    /// A rig for a plugin of the given tier.
    pub fn new(_tier: &str) -> Rig {
        Rig {
            render_cost_ms: 1,
            scene_rows: 24,
            background: true,
            attached: true,
            ..Rig::default()
        }
    }

    /// Adds a scene.
    pub fn with_scene(mut self) -> Rig {
        self.scenes.push("scene".to_owned());
        self
    }

    /// Adds a prerendered scene.
    pub fn with_prerendered_scene(mut self, fps: u32) -> Rig {
        self.scenes.push("prerendered".to_owned());
        self.prerender_fps = Some(fps);
        self
    }

    /// Moves the clock.
    pub fn advance_ms(&mut self, ms: u64) {
        self.clock_ms = self.clock_ms.saturating_add(ms);
    }

    /// The clock.
    pub fn now_ms(&self) -> u64 {
        self.clock_ms
    }

    /// Paints a frame and returns its duration. Held or blocked rigs paint nothing and return 0.
    pub fn paint_frame_ms(&mut self) -> u64 {
        if self.blocked || self.disabled || !self.attached || self.window_hidden {
            return 0;
        }
        let clock = self.clock_ms;
        let cost = self.render_cost_ms;
        let hold = self.hold;
        let mut painted = false;
        for state in self.views.values_mut().filter(|state| state.stale) {
            if hold {
                state.queued = 1;
                continue;
            }
            state.queued = 0;
            state.render_calls += 1;
            state.render_starts.push(clock);
            state.last_area = Some((state.columns, state.rows));
            state.repainted = true;
            state.stale = false;
            painted = true;
        }
        if painted {
            self.paint_generation += 1;
            cost
        } else {
            0
        }
    }

    /// The paint generation.
    pub fn paint_generation(&self) -> u64 {
        self.paint_generation
    }

    /// Begins a frame: takes the frame snapshot of every view.
    pub fn begin_frame(&mut self) {
        for state in self.views.values_mut() {
            state.repainted = false;
            state.frame = state.shown.clone();
            state.frame_scroll = state.scroll.clone();
        }
    }

    /// Ends a frame. Snapshots were taken at `begin_frame`, so nothing is left to finish.
    pub fn end_frame(&mut self) {}

    /// Blocks the plugin executor.
    pub fn block_plugin(&mut self, blocked: bool) {
        self.blocked = blocked;
    }

    /// Opens a view.
    pub fn open_view(&mut self, _name: &str) -> u32 {
        self.next_view += 1;
        let id = self.next_view;
        self.views.insert(
            id,
            ViewState {
                columns: 80,
                rows: 24,
                stale: true,
                ..ViewState::default()
            },
        );
        id
    }

    /// Opens a settings view.
    pub fn open_settings_view(&mut self, name: &str) -> u32 {
        self.open_view(name)
    }

    /// Opens a code view with lines.
    pub fn open_code_view(&mut self, name: &str, lines: u32) -> u32 {
        let id = self.open_view(name);
        self.with_view(id, |state| state.code_lines = Some(lines));
        id
    }

    /// Open views.
    pub fn open_view_count(&self) -> usize {
        self.views.len()
    }

    /// Invalidates a view.
    pub fn invalidate(&mut self, view: u32) {
        self.with_view(view, |state| state.stale = true);
    }

    /// Resizes a view.
    pub fn resize_view(&mut self, view: u32, columns: u16, rows: u16) {
        self.with_view(view, |state| {
            state.columns = columns;
            state.rows = rows;
            state.stale = true;
        });
    }

    /// Changes the theme. Every view repaints and every mirror entry is recoloured.
    pub fn change_theme(&mut self) {
        self.theme_events += 1;
        self.stale_all_views();
        for entries in self.client_mirror.values_mut() {
            for entry in entries.values_mut() {
                entry.recoloured = true;
            }
        }
    }

    /// Changes the font. Every view repaints.
    pub fn change_font(&mut self) {
        self.stale_all_views();
    }

    /// Scrolls a view by rows, as the root node of the view.
    pub fn scroll_view(&mut self, view: u32, rows: i32) {
        self.with_view(view, |state| {
            shift_scroll(&mut state.scroll, "", i64::from(rows));
            state.stale = true;
        });
    }

    /// A user action (click or key) inside a view (HST-251).
    pub fn user_action(&mut self, view: u32) {
        if self.blocked || !self.views.contains_key(&view) {
            return;
        }
        self.plugin_calls += 1;
        self.invalidate(view);
    }

    /// Focuses a view.
    pub fn focus_view(&mut self, view: u32) {
        self.focused_view = Some(view);
    }

    /// Render calls of a view.
    pub fn render_calls(&self, view: u32) -> u32 {
        self.views.get(&view).map_or(0, |state| state.render_calls)
    }

    /// Renders in flight of a view. Renders run synchronously in this rig, so none stays in
    /// flight once a paint returns.
    pub fn render_in_flight(&self, _view: u32) -> u32 {
        0
    }

    /// Renders queued of a view (held renders wait here).
    pub fn render_queued(&self, view: u32) -> u32 {
        self.views.get(&view).map_or(0, |state| state.queued)
    }

    /// Render start times of a view.
    pub fn render_start_times(&self, view: u32) -> Vec<u64> {
        self.views
            .get(&view)
            .map_or_else(Vec::new, |state| state.render_starts.clone())
    }

    /// Holds renders.
    pub fn hold_renders(&mut self, hold: bool) {
        self.hold = hold;
    }

    /// Sets the cost of a render.
    pub fn set_render_cost_ms(&mut self, ms: u64) {
        self.render_cost_ms = ms;
    }

    /// The last trap code.
    pub fn last_trap(&self) -> Option<String> {
        self.last_trap.clone()
    }

    /// The area of the last render of a view.
    pub fn last_render_area(&self, view: u32) -> Option<(u16, u16)> {
        self.views.get(&view).and_then(|state| state.last_area)
    }

    /// The render deadline.
    pub fn render_deadline_ms(&self) -> u64 {
        RENDER_DEADLINE_MS
    }

    /// The render deadline ceiling.
    pub fn render_deadline_ceiling_ms(&self) -> u64 {
        RENDER_DEADLINE_CEILING_MS
    }

    /// Theme-changed events delivered.
    pub fn theme_changed_events(&self) -> u32 {
        self.theme_events
    }

    /// Calls the plugin received.
    pub fn plugin_calls_received(&self) -> u32 {
        self.plugin_calls
    }

    /// Delivers a tree; `Err` is the trap code.
    pub fn deliver_tree(&mut self, view: u32, tree: UiTree) -> Result<(), String> {
        match self.store_tree(view, tree) {
            Ok(()) => Ok(()),
            Err(code) => self.fail_delivery(&code),
        }
    }

    /// Delivers a tree of one widget.
    pub fn deliver_widget(&mut self, view: u32, widget: &str) -> Result<(), String> {
        self.deliver_widget_with_items(view, widget, 0)
    }

    /// Delivers a tree of one widget with items.
    pub fn deliver_widget_with_items(
        &mut self,
        view: u32,
        widget: &str,
        items: u32,
    ) -> Result<(), String> {
        if !is_known_widget(widget) {
            return self.fail_delivery("ui.unknown-widget");
        }
        self.deliver_tree(view, widget_tree(widget, items))
    }

    /// The tree shown in a view.
    pub fn shown_tree(&self, view: u32) -> Option<UiTree> {
        self.views.get(&view).and_then(|state| state.shown.clone())
    }

    /// The tree of the current frame of a view.
    pub fn frame_tree(&self, view: u32) -> Option<UiTree> {
        self.views.get(&view).and_then(|state| state.frame.clone())
    }

    /// Whether a view is stale.
    pub fn is_stale(&self, view: u32) -> bool {
        self.views.get(&view).is_some_and(|state| state.stale)
    }

    /// Failures counted.
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// Log lines as (code, message).
    pub fn log_lines(&self) -> Vec<(String, String)> {
        self.logs.clone()
    }

    /// The screen text: every shown paragraph line of every view, one line per row.
    pub fn screen_text(&self) -> String {
        self.views
            .values()
            .filter_map(|state| state.shown.as_ref())
            .flat_map(tree_text_lines)
            .collect::<Vec<String>>()
            .join("\n")
    }

    /// Scrolls a node.
    pub fn scroll_node(&mut self, view: u32, key: &str, rows: u32) {
        self.with_view(view, |state| {
            shift_scroll(&mut state.scroll, key, i64::from(rows));
            state.stale = true;
        });
    }

    /// The scroll of a node.
    pub fn node_scroll(&self, view: u32, key: &str) -> u32 {
        self.views
            .get(&view)
            .and_then(|state| state.scroll.get(key).copied())
            .unwrap_or(0)
    }

    /// The scroll of a node in the current frame.
    pub fn frame_scroll(&self, view: u32, key: &str) -> u32 {
        self.views
            .get(&view)
            .and_then(|state| state.frame_scroll.get(key).copied())
            .unwrap_or(0)
    }

    /// The user scrolls a node. The user's scroll reaches the plugin as a call.
    pub fn user_scroll(&mut self, view: u32, key: &str, rows: u32) {
        if !self.views.contains_key(&view) {
            return;
        }
        self.plugin_calls += 1;
        self.scroll_node(view, key, rows);
    }

    /// Selects a tab.
    pub fn select_tab(&mut self, view: u32, key: &str, tab: u32) {
        self.with_view(view, |state| {
            state.tabs.insert(key.to_owned(), tab);
            state.stale = true;
        });
    }

    /// The selected tab.
    pub fn selected_tab(&self, view: u32, key: &str) -> u32 {
        self.views
            .get(&view)
            .and_then(|state| state.tabs.get(key).copied())
            .unwrap_or(0)
    }

    /// Sets a cursor.
    pub fn set_cursor(&mut self, view: u32, key: &str, at: u32) {
        self.with_view(view, |state| {
            state.cursors.insert(key.to_owned(), at);
            state.stale = true;
        });
    }

    /// The cursor.
    pub fn cursor(&self, view: u32, key: &str) -> u32 {
        self.views
            .get(&view)
            .and_then(|state| state.cursors.get(key).copied())
            .unwrap_or(0)
    }

    /// Whether a view was repainted.
    pub fn view_repainted(&self, view: u32) -> bool {
        self.views.get(&view).is_some_and(|state| state.repainted)
    }

    /// Hit-tests a cell. The answer comes from the UI thread; the plugin is not called.
    pub fn hit_test(&mut self, view: u32, column: u16, row: u16) -> Option<String> {
        self.hit_on_ui_thread = true;
        let state = self.views.get(&view)?;
        if column >= state.columns || row >= state.rows {
            return None;
        }
        let tree = state.shown.as_ref()?;
        hit_layers_in_order()
            .into_iter()
            .find(|layer| tree_has_key(Some(tree), layer))
            .map(str::to_owned)
    }

    /// Whether the last hit was resolved on the UI thread.
    pub fn hit_resolved_on_ui_thread(&self) -> bool {
        self.hit_on_ui_thread
    }

    /// The user focuses a view.
    pub fn user_focus(&mut self, view: u32) {
        self.focus_view(view);
    }

    /// The user unfocuses a view.
    pub fn user_unfocus(&mut self, view: u32) {
        if self.focused_view == Some(view) {
            self.focused_view = None;
        }
    }

    /// The plugin sets focus. Only a key present in the shown tree takes focus.
    pub fn plugin_set_focus(&mut self, view: u32, key: &str) -> bool {
        let Some(state) = self.views.get_mut(&view) else {
            return false;
        };
        if !tree_has_key(state.shown.as_ref(), key) {
            return false;
        }
        state.focus = Some(key.to_owned());
        true
    }

    /// The focused key.
    pub fn focused_key(&self, view: u32) -> Option<String> {
        self.views.get(&view).and_then(|state| state.focus.clone())
    }

    /// The server side contributes a node.
    pub fn server_contribute(
        &mut self,
        point: &str,
        node: &str,
        text: &str,
    ) -> Result<(), String> {
        contribute(&mut self.server_registry, point, node, text)
    }

    /// The client side contributes a node.
    pub fn client_contribute(
        &mut self,
        point: &str,
        node: &str,
        text: &str,
    ) -> Result<(), String> {
        contribute(&mut self.client_store, point, node, text)
    }

    /// Entries in the server registry of a point.
    pub fn server_registry_len(&self, point: &str) -> usize {
        self.server_registry
            .get(point)
            .map_or(0, |nodes| nodes.len())
    }

    /// Entries in the client store of a point.
    pub fn client_store_len(&self, point: &str) -> usize {
        self.client_store.get(point).map_or(0, |nodes| nodes.len())
    }

    /// Entries in the client mirror of a point.
    pub fn client_mirror_len(&self, point: &str) -> usize {
        self.client_mirror.get(point).map_or(0, |nodes| nodes.len())
    }

    /// The contribution cap of a point.
    pub fn contribution_cap(&self, _point: &str) -> u32 {
        CONTRIBUTION_CAP
    }

    /// Whether a contribution is stale: its plugin side stopped and no restart is coming.
    pub fn contribution_stale(&self, point: &str, node: &str) -> bool {
        self.plugin_stopped
            && !self.restart_scheduled
            && self
                .server_registry
                .get(point)
                .is_some_and(|nodes| nodes.contains_key(node))
    }

    /// Whether a mirror entry is stale.
    pub fn mirror_stale(&self, point: &str, node: &str) -> bool {
        self.mirror_entry(point, node).is_some_and(|entry| entry.stale)
    }

    /// Whether a mirror entry is recoloured.
    pub fn mirror_recoloured(&self, point: &str, node: &str) -> bool {
        self.mirror_entry(point, node)
            .is_some_and(|entry| entry.recoloured)
    }

    /// Syncs the mirror from the server registry.
    pub fn sync_mirror(&mut self) {
        let mirror: BTreeMap<String, BTreeMap<String, MirrorEntry>> = self
            .server_registry
            .iter()
            .map(|(point, nodes)| {
                let entries: BTreeMap<String, MirrorEntry> = nodes
                    .keys()
                    .map(|node| (node.clone(), MirrorEntry::default()))
                    .collect();
                (point.clone(), entries)
            })
            .collect();
        self.client_mirror = mirror;
    }

    /// Stops the plugin side. Its mirror entries turn stale.
    pub fn stop_plugin_side(&mut self, restart_scheduled: bool) {
        self.plugin_stopped = true;
        self.restart_scheduled = restart_scheduled;
        for entry in self
            .client_mirror
            .values_mut()
            .flat_map(|entries| entries.values_mut())
        {
            entry.stale = true;
        }
    }

    /// Uninstalls the plugin: its contributions and trees are gone.
    pub fn uninstall_plugin(&mut self) {
        self.uninstalled = true;
        self.plugin_stopped = true;
        self.server_registry.clear();
        self.client_mirror.clear();
        for state in self.views.values_mut() {
            state.shown = None;
            state.frame = None;
            state.stale = true;
        }
    }

    /// Disables the plugin. It stops painting and accepting trees.
    pub fn disable_plugin(&mut self) {
        self.disabled = true;
        self.plugin_stopped = true;
    }

    /// Detaches the client: nothing paints until a client attaches again.
    pub fn detach_client(&mut self) {
        self.attached = false;
    }

    /// Attaches a client with a snapshot: the mirror is rebuilt and every view repaints.
    pub fn attach_client_with_snapshot(&mut self) {
        self.attached = true;
        self.sync_mirror();
        self.stale_all_views();
    }

    /// Activates a contribution. Returns its text and calls the plugin; `None` when the plugin
    /// side is stopped or the node is unknown.
    pub fn activate_contribution(&mut self, point: &str, node: &str) -> Option<String> {
        if self.plugin_stopped {
            return None;
        }
        let text = self.server_registry.get(point)?.get(node)?.clone();
        self.plugin_calls += 1;
        Some(text)
    }

    /// Sets the visible rows of a view. A code view runs annotate and tokens for them.
    pub fn set_visible_rows(&mut self, view: u32, first: u32, last: u32) {
        let clock = self.clock_ms;
        let Some(state) = self.views.get_mut(&view) else {
            return;
        };
        if state.code_lines.is_none() {
            return;
        }
        state.annotate_times.push(clock + state.annotate_delay);
        state.last_annotate = Some((first, last));
        state.applied_annotation = if state.annotate_delay == 0 {
            state.annotate_spans
        } else {
            0
        };
        state.last_tokens = Some((first, last));
        let token_spans = last.saturating_sub(first) + 1;
        state.applied_tokens = if state.tokens_delay == 0 {
            token_spans
        } else {
            0
        };
    }

    /// The last annotate range.
    pub fn last_annotate_range(&self, view: u32) -> Option<(u32, u32)> {
        self.views.get(&view).and_then(|state| state.last_annotate)
    }

    /// The last tokens range.
    pub fn last_tokens_range(&self, view: u32) -> Option<(u32, u32)> {
        self.views.get(&view).and_then(|state| state.last_tokens)
    }

    /// Annotate call times. A delayed answer is stamped when it lands.
    pub fn annotate_times(&self, view: u32) -> Vec<u64> {
        self.views
            .get(&view)
            .map_or_else(Vec::new, |state| state.annotate_times.clone())
    }

    /// Delays annotate.
    pub fn delay_annotate(&mut self, view: u32, ms: u64) {
        self.with_view(view, |state| state.annotate_delay = ms);
    }

    /// Delays tokens.
    pub fn delay_tokens(&mut self, view: u32, ms: u64) {
        self.with_view(view, |state| state.tokens_delay = ms);
    }

    /// Annotate returns the given number of spans.
    pub fn annotate_returns_spans(&mut self, view: u32, spans: u32) {
        self.with_view(view, |state| state.annotate_spans = spans);
    }

    /// Annotation spans applied.
    pub fn applied_annotation_spans(&self, view: u32) -> u32 {
        self.views
            .get(&view)
            .map_or(0, |state| state.applied_annotation)
    }

    /// Token spans applied.
    pub fn applied_token_spans(&self, view: u32) -> u32 {
        self.views
            .get(&view)
            .map_or(0, |state| state.applied_tokens)
    }

    /// The scene begins a frame: the back buffer is cleared.
    pub fn scene_begin_frame(&mut self) {
        self.scene_back = vec![0; self.scene_rows as usize];
        self.scene_render_calls += 1;
    }

    /// The scene writes a row. Rows past the buffer are ignored.
    pub fn scene_write_row(&mut self, row: u32, value: u8) {
        if let Some(slot) = self.scene_back.get_mut(row as usize) {
            *slot = value;
        }
    }

    /// The scene presents. A frame over budget is skipped and not presented.
    pub fn scene_present(&mut self) {
        let budget = frame_budget_ms(self.prerender_fps.unwrap_or(30), None);
        if self.scene_frame_cost_ms > budget {
            self.scene_skipped += 1;
            return;
        }
        self.scene_front = self.scene_back.clone();
        let mut presented = self.scene_front.clone();
        if !self.background {
            presented.iter_mut().for_each(|byte| *byte = 0);
        }
        if self.look_invert {
            presented.iter_mut().for_each(|byte| *byte = !*byte);
        }
        self.scene_presented = presented;
    }

    /// Scene rows.
    pub fn scene_rows(&self) -> u32 {
        self.scene_rows
    }

    /// The front buffer rows.
    pub fn front_buffer_rows(&self) -> Vec<u8> {
        self.scene_front.clone()
    }

    /// One front buffer row.
    pub fn front_buffer_row(&self, row: u32) -> u8 {
        self.scene_front.get(row as usize).copied().unwrap_or(0)
    }

    /// One presented row.
    pub fn presented_row(&self, row: u32) -> u8 {
        self.scene_presented.get(row as usize).copied().unwrap_or(0)
    }

    /// Sets the cost of a scene frame.
    pub fn scene_frame_cost_ms(&mut self, ms: u64) {
        self.scene_frame_cost_ms = ms;
    }

    /// Skipped frames.
    pub fn skipped_frames(&self) -> u32 {
        self.scene_skipped
    }

    /// Sets the look invert.
    pub fn set_look_invert(&mut self, invert: bool) {
        self.look_invert = invert;
    }

    /// Scene render calls.
    pub fn scene_render_calls(&self) -> u32 {
        self.scene_render_calls
    }

    /// Invalidates the clip: the front buffer is cleared.
    pub fn invalidate_clip(&mut self) {
        self.scene_front.iter_mut().for_each(|byte| *byte = 0);
    }

    /// Starts a scene.
    pub fn start_scene(&mut self, name: &str) {
        self.scenes.push(name.to_owned());
    }

    /// Active scenes.
    pub fn active_scene_count(&self) -> usize {
        self.scenes.len()
    }

    /// The active scene.
    pub fn active_scene_name(&self) -> Option<String> {
        self.scenes.last().cloned()
    }

    /// Turns the background on or off.
    pub fn set_background(&mut self, on: bool) {
        self.background = on;
    }

    /// Hides the window. A hidden window paints nothing.
    pub fn set_window_hidden(&mut self, hidden: bool) {
        self.window_hidden = hidden;
    }

    /// Stores a tree after the host checks; `Err` is the trap code.
    fn store_tree(&mut self, view: u32, mut tree: UiTree) -> Result<(), String> {
        if self.uninstalled || self.disabled {
            return Err("plugin.not-running".to_owned());
        }
        if !self.views.contains_key(&view) {
            return Err("view.unknown".to_owned());
        }
        if tree.root != 0 {
            return Err("ui.bad-root".to_owned());
        }
        if tree.nodes.len() > 4096 {
            return Err("ui.too-many-nodes".to_owned());
        }
        let sanitised = sanitise_tree_text(&mut tree);
        let previous = self.views.get(&view).and_then(|state| state.shown.clone());
        let changed = match &previous {
            Some(old) => !crate::render::diff(old, &tree).changed_paths.is_empty(),
            None => true,
        };
        if sanitised {
            self.note_sanitised(view);
        }
        if let Some(state) = self.views.get_mut(&view) {
            state.stale |= changed;
            state.shown = Some(tree);
        }
        Ok(())
    }

    /// Records a failed delivery and returns its code as the error.
    fn fail_delivery(&mut self, code: &str) -> Result<(), String> {
        self.failures += 1;
        self.last_trap = Some(code.to_owned());
        Err(code.to_owned())
    }

    /// Logs the sanitising of a view's text, once per view (RND-019).
    fn note_sanitised(&mut self, view: u32) {
        let warned = self
            .views
            .get(&view)
            .is_some_and(|state| state.sanitised_warned);
        if !warned {
            self.logs.push((
                "ui.text-sanitized".to_owned(),
                "control characters removed from view text".to_owned(),
            ));
            if let Some(state) = self.views.get_mut(&view) {
                state.sanitised_warned = true;
            }
        }
    }

    /// Applies a change to one view when it exists.
    fn with_view(&mut self, view: u32, change: impl FnOnce(&mut ViewState)) {
        if let Some(state) = self.views.get_mut(&view) {
            change(state);
        }
    }

    /// Marks every view stale so it repaints on the next paint.
    fn stale_all_views(&mut self) {
        for state in self.views.values_mut() {
            state.stale = true;
        }
    }

    /// The mirror entry of one contribution.
    fn mirror_entry(&self, point: &str, node: &str) -> Option<&MirrorEntry> {
        self.client_mirror
            .get(point)
            .and_then(|entries| entries.get(node))
    }
}

/// Drives the lifecycle of one scene plugin (RND-001..014, WIT renderer exports).
#[derive(Debug, Default)]
pub struct SceneRig {
    fps: u32,
    columns: u32,
    rows: u32,
    uses_cell_colors: bool,
    wants_occupancy: bool,
    wants_pointer: bool,
    grants: Vec<String>,
    palette: Vec<u32>,
    colour_mode: String,
    frame_cost_ms: u64,
    never_present: bool,
    readiness: Vec<String>,
    readiness_after: Option<u64>,
    reconfigure_keep: bool,
    settings: BTreeMap<String, String>,
    trap_next: bool,
    alloc_next_mib: u32,
    constructor_broken: bool,
    constructed: bool,
    status: Option<String>,
    failures: u32,
    restarts: u32,
    restart_delay: Option<u64>,
    skipped: u32,
    presented: u32,
    render_calls: u32,
    constructor_calls: u32,
    reconfigure_calls: u32,
    trap: Option<String>,
    logs: Vec<(String, String)>,
    spec: String,
    last_constructor_spec: String,
    last_reconfigure_spec: Option<String>,
    constructor_grants: Vec<String>,
    constructor_palette: Vec<u32>,
    set_palette_calls: Vec<Vec<u32>>,
    plugin_failures: BTreeMap<String, u32>,
    presented_colors: Vec<u32>,
    query_counts: BTreeMap<String, u32>,
    frame_numbers: Vec<u64>,
    surface_sizes: Vec<(u32, u32)>,
    saw_pointer: Option<(u32, u32)>,
    saw_occupancy: Option<Vec<u8>>,
    pointer: Option<(u32, u32)>,
    clock_ms: u64,
    frame_no: u64,
}

impl SceneRig {
    /// A scene rig at the given frame rate.
    pub fn new(fps: u32) -> SceneRig {
        SceneRig {
            fps,
            columns: 80,
            rows: 24,
            ..SceneRig::default()
        }
    }

    /// The scene flags.
    pub fn flags(
        mut self,
        uses_cell_colors: bool,
        wants_occupancy: bool,
        wants_pointer: bool,
    ) -> SceneRig {
        self.uses_cell_colors = uses_cell_colors;
        self.wants_occupancy = wants_occupancy;
        self.wants_pointer = wants_pointer;
        self
    }

    /// A grant.
    pub fn grant(mut self, name: &str) -> SceneRig {
        if !self.grants.iter().any(|grant| grant.as_str() == name) {
            self.grants.push(name.to_owned());
        }
        self
    }

    /// The palette.
    pub fn palette(mut self, rgb: &[u32]) -> SceneRig {
        self.palette = rgb.to_vec();
        self
    }

    /// The colour mode.
    pub fn colour_mode(mut self, mode: &str) -> SceneRig {
        self.colour_mode = mode.to_owned();
        self
    }

    /// The area.
    pub fn area(mut self, columns: u32, rows: u32) -> SceneRig {
        self.columns = columns;
        self.rows = rows;
        self
    }

    /// Runs frames.
    pub fn run_frames(&mut self, count: u32) {
        for _ in 0..count {
            self.run_one_frame();
        }
    }

    /// Runs for a duration; only whole frames inside the duration run.
    pub fn run_for_ms(&mut self, ms: u64) {
        let budget = self.budget_ms().max(1);
        for _ in 0..(ms / budget) {
            self.run_one_frame();
        }
    }

    /// Sets the frame cost.
    pub fn frame_cost_ms(&mut self, ms: u64) {
        self.frame_cost_ms = ms;
    }

    /// The scene never presents.
    pub fn never_present(&mut self) {
        self.never_present = true;
    }

    /// Scripts readiness: one step per frame; the scene presents on a step that reads `ready`.
    pub fn readiness_script(&mut self, steps: &[&str]) {
        self.readiness = steps.iter().map(|step| (*step).to_owned()).collect();
    }

    /// The scene is ready after a delay.
    pub fn readiness_after_ms(&mut self, ms: u64) {
        self.readiness_after = Some(ms);
    }

    /// What reconfigure returns.
    pub fn reconfigure_returns(&mut self, keep: bool) {
        self.reconfigure_keep = keep;
    }

    /// Reconfigures. A scene that does not keep its state is restarted.
    pub fn reconfigure(&mut self, spec: &str) {
        bump(&mut self.query_counts, "reconfigure");
        self.reconfigure_calls += 1;
        self.last_reconfigure_spec = Some(spec.to_owned());
        self.spec = spec.to_owned();
        if !self.reconfigure_keep {
            self.restart_plugin();
        }
    }

    /// Changes a setting; only the scene's own control ids are accepted.
    pub fn change_setting(&mut self, key: &str, value: &str) -> Result<(), String> {
        if !SCENE_CONTROL_IDS.iter().any(|id| *id == key) {
            return Err(format!("settings.unknown-key: {key}"));
        }
        self.settings.insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    /// A setting.
    pub fn setting(&self, key: &str) -> Option<String> {
        self.settings.get(key).cloned()
    }

    /// Revokes a grant.
    pub fn revoke(&mut self, name: &str) {
        self.grants.retain(|grant| grant.as_str() != name);
    }

    /// Changes the palette.
    pub fn change_palette(&mut self, rgb: &[u32]) {
        bump(&mut self.query_counts, "set-palette");
        self.palette = rgb.to_vec();
        self.set_palette_calls.push(rgb.to_vec());
    }

    /// The next frame traps.
    pub fn trap_in_next_frame(&mut self) {
        self.trap_next = true;
    }

    /// The next frame allocates.
    pub fn allocate_in_next_frame_mib(&mut self, mib: u32) {
        self.alloc_next_mib = mib;
    }

    /// The constructor fails.
    pub fn constructor_fails(&mut self) {
        self.constructor_broken = true;
    }

    /// Sets the status text.
    pub fn status_text(&mut self, text: &str) {
        self.status = Some(text.to_owned());
    }

    /// Failures counted.
    pub fn failure_count(&self) -> u32 {
        self.failures
    }

    /// Restarts.
    pub fn restarts(&self) -> u32 {
        self.restarts
    }

    /// The restart delay.
    pub fn restart_delay_ms(&self) -> Option<u64> {
        self.restart_delay
    }

    /// Skipped frames.
    pub fn skipped_frames(&self) -> u32 {
        self.skipped
    }

    /// Presented frames.
    pub fn presented_frames(&self) -> u32 {
        self.presented
    }

    /// Render calls.
    pub fn render_calls(&self) -> u32 {
        self.render_calls
    }

    /// Constructor calls.
    pub fn constructor_calls(&self) -> u32 {
        self.constructor_calls
    }

    /// Reconfigure calls.
    pub fn reconfigure_calls(&self) -> u32 {
        self.reconfigure_calls
    }

    /// The last trap.
    pub fn trap(&self) -> Option<String> {
        self.trap.clone()
    }

    /// Log lines as (code, message).
    pub fn log_lines(&self) -> Vec<(String, String)> {
        self.logs.clone()
    }

    /// The current spec.
    pub fn current_spec(&self) -> String {
        self.spec.clone()
    }

    /// The spec of the last constructor call.
    pub fn last_constructor_spec(&self) -> String {
        self.last_constructor_spec.clone()
    }

    /// The spec of the last reconfigure call.
    pub fn last_reconfigure_spec(&self) -> Option<String> {
        self.last_reconfigure_spec.clone()
    }

    /// The grants given to the constructor.
    pub fn constructor_grants(&self) -> Vec<String> {
        self.constructor_grants.clone()
    }

    /// The palette given to the constructor.
    pub fn constructor_palette(&self) -> Vec<u32> {
        self.constructor_palette.clone()
    }

    /// Every `set-palette` call.
    pub fn set_palette_calls(&self) -> Vec<Vec<u32>> {
        self.set_palette_calls.clone()
    }

    /// Failures of a plugin side.
    pub fn plugin_side_failures(&self, side: &str) -> u32 {
        self.plugin_failures.get(side).copied().unwrap_or(0)
    }

    /// Presented cell colours.
    pub fn presented_cell_colors(&self) -> Vec<u32> {
        self.presented_colors.clone()
    }

    /// Scene control ids.
    pub fn scene_control_ids(&self) -> Vec<String> {
        SCENE_CONTROL_IDS
            .iter()
            .map(|id| (*id).to_owned())
            .collect()
    }

    /// The frame rate.
    pub fn fps(&self) -> u32 {
        self.fps
    }

    /// The frame budget.
    pub fn budget_ms(&self) -> u64 {
        frame_budget_ms(self.fps, None)
    }

    /// Calls of an export.
    pub fn query_count(&self, export: &str) -> u32 {
        self.query_counts.get(export).copied().unwrap_or(0)
    }

    /// Frame numbers given to render.
    pub fn render_frame_numbers(&self) -> Vec<u64> {
        self.frame_numbers.clone()
    }

    /// Surface sizes given to render.
    pub fn render_surface_sizes(&self) -> Vec<(u32, u32)> {
        self.surface_sizes.clone()
    }

    /// The pointer the scene saw.
    pub fn scene_saw_pointer(&self) -> Option<(u32, u32)> {
        self.saw_pointer
    }

    /// The occupancy the scene saw.
    pub fn scene_saw_occupancy(&self) -> Option<Vec<u8>> {
        self.saw_occupancy.clone()
    }

    /// The status shown.
    pub fn shown_status(&self) -> Option<String> {
        self.status.clone()
    }

    /// Moves the pointer.
    pub fn move_pointer(&mut self, column: u32, row: u32) {
        self.pointer = Some((column, row));
    }

    /// Runs one frame: constructs the scene on first use, then renders and presents it unless a
    /// trap, a skip, a readiness step or the scene's own choice keeps it from presenting.
    fn run_one_frame(&mut self) {
        if !self.constructed {
            self.construct();
        }
        self.frame_no += 1;
        self.frame_numbers.push(self.frame_no);
        bump(&mut self.query_counts, "render");
        self.clock_ms += self.budget_ms();
        if self.constructor_broken {
            return;
        }
        if self.trap_next {
            self.trap_next = false;
            self.plugin_trapped("scene.frame-trap");
            return;
        }
        if self.alloc_next_mib > 0 {
            let mib = self.alloc_next_mib;
            self.alloc_next_mib = 0;
            if mib > scene_memory_limit_mib("T2", None) {
                self.plugin_trapped("resource.memory-limit");
                return;
            }
        }
        self.render_calls += 1;
        self.surface_sizes.push((self.columns, self.rows));
        if self.wants_pointer {
            self.saw_pointer = self.pointer;
        }
        if self.wants_occupancy {
            self.saw_occupancy = Some(vec![0u8; self.rows as usize]);
        }
        if self.frame_cost_ms > self.budget_ms() {
            self.skipped += 1;
            return;
        }
        if self.never_present {
            return;
        }
        if !self.readiness.is_empty() && self.readiness.remove(0) != "ready" {
            return;
        }
        if self.readiness_after.is_some_and(|after| self.clock_ms < after) {
            return;
        }
        self.presented += 1;
        self.presented_colors = if self.uses_cell_colors && self.colour_mode != "mono" {
            self.palette.clone()
        } else {
            Vec::new()
        };
    }

    /// Records a plugin trap and restarts the scene with a growing delay.
    fn plugin_trapped(&mut self, code: &str) {
        self.trap = Some(code.to_owned());
        self.failures += 1;
        *self.plugin_failures.entry("scene".to_owned()).or_insert(0) += 1;
        self.logs
            .push(("scene.trap".to_owned(), code.to_owned()));
        self.restart_plugin();
    }

    /// Restarts the scene: the delay doubles per restart, capped at 64 times the base delay.
    fn restart_plugin(&mut self) {
        self.restarts += 1;
        self.restart_delay = Some(250u64 << self.restarts.min(6));
        self.constructed = false;
        self.construct();
    }

    /// Runs the constructor once with the grants and palette held now.
    fn construct(&mut self) {
        self.constructed = true;
        self.constructor_calls += 1;
        self.last_constructor_spec = self.spec.clone();
        self.constructor_grants = self.grants.clone();
        self.constructor_palette = self.palette.clone();
        if self.constructor_broken {
            self.failures += 1;
            *self
                .plugin_failures
                .entry("constructor".to_owned())
                .or_insert(0) += 1;
            self.logs.push((
                "scene.constructor-failed".to_owned(),
                "the scene constructor returned an error".to_owned(),
            ));
        }
    }
}

/// The host side of one frame surface (RND-004..006, RND-016, WIT:ilium:anim/surface).
#[derive(Debug, Default)]
pub struct SurfaceRig {
    width: u32,
    height: u32,
    uses_cell_colors: bool,
    wants_occupancy: bool,
    wants_pointer: bool,
    rows: Vec<Vec<u8>>,
    presented: Vec<Vec<u8>>,
    colors: Vec<Vec<u32>>,
    glyphs: u32,
    owner_ids: Vec<u32>,
    last_receipt: Option<u64>,
    last_receipt_owner_ids: Vec<u32>,
    receipts: u64,
    clock_ms: u64,
    pointer: Option<(u32, u32)>,
    warnings: usize,
}

impl SurfaceRig {
    /// A surface of the given size and format.
    pub fn new(width: u32, height: u32, _format: &str) -> SurfaceRig {
        SurfaceRig {
            width,
            height,
            rows: vec![vec![0; width as usize]; height as usize],
            presented: vec![vec![0; width as usize]; height as usize],
            colors: vec![Vec::new(); height as usize],
            ..SurfaceRig::default()
        }
    }

    /// The scene flags.
    pub fn flags(
        mut self,
        uses_cell_colors: bool,
        wants_occupancy: bool,
        wants_pointer: bool,
    ) -> SurfaceRig {
        self.uses_cell_colors = uses_cell_colors;
        self.wants_occupancy = wants_occupancy;
        self.wants_pointer = wants_pointer;
        self
    }

    /// Writes rows. `bytes` must fill whole rows and stay inside the surface.
    pub fn write_rows(&mut self, first_row: u32, bytes: &[u8]) -> Result<(), String> {
        let width = self.width as usize;
        if width == 0 || bytes.len() % width != 0 {
            return Err("invalid-argument: bytes do not fill whole rows".to_owned());
        }
        let count = bytes.len() / width;
        let first = first_row as usize;
        if first + count > self.rows.len() {
            return Err("surface.out-of-range: rows past the surface".to_owned());
        }
        for (offset, chunk) in bytes.chunks(width).enumerate() {
            self.rows[first + offset] = chunk.to_vec();
        }
        Ok(())
    }

    /// Writes glyphs: counts `cells` glyphs written to the surface.
    pub fn write_glyphs(&mut self, cells: u32) -> Result<(), String> {
        let capacity = u64::from(self.width) * u64::from(self.height);
        if u64::from(cells) > capacity {
            return Err("surface.too-many-cells: more glyphs than the surface holds".to_owned());
        }
        self.glyphs = self.glyphs.saturating_add(cells);
        Ok(())
    }

    /// Writes cell colours. Without the cell-colour flag, or past the surface, the write counts
    /// as a warning and changes nothing.
    pub fn write_cell_colors(&mut self, row: u32, colors: &[u32]) {
        match self.colors.get_mut(row as usize) {
            Some(slot) if self.uses_cell_colors => *slot = colors.to_vec(),
            _ => self.warnings += 1,
        }
    }

    /// Clears glyphs.
    pub fn clear_glyphs(&mut self) {
        self.glyphs = 0;
    }

    /// Sets owner ids.
    pub fn set_owner_ids(&mut self, ids: &[u32]) {
        self.owner_ids = ids.to_vec();
    }

    /// Sets owner ids, checked: at most 16 ids, none of them zero.
    pub fn set_owner_ids_checked(&mut self, ids: &[u32]) -> Result<(), String> {
        if ids.len() > 16 {
            return Err("invalid-argument: more than 16 owner ids".to_owned());
        }
        if ids.contains(&0) {
            return Err("invalid-argument: owner id 0 is reserved".to_owned());
        }
        self.owner_ids = ids.to_vec();
        Ok(())
    }

    /// Presents; returns the receipt.
    pub fn present(&mut self) -> Result<u64, String> {
        if self.width == 0 || self.height == 0 {
            return Err("surface.empty: a surface with no cells cannot present".to_owned());
        }
        self.receipts += 1;
        self.presented = self.rows.clone();
        let receipt = self.clock_ms + self.receipts;
        self.last_receipt = Some(receipt);
        self.last_receipt_owner_ids = self.owner_ids.clone();
        Ok(receipt)
    }

    /// The last receipt.
    pub fn last_receipt(&self) -> Option<u64> {
        self.last_receipt
    }

    /// The owner ids of the last receipt.
    pub fn last_receipt_owner_ids(&self) -> Vec<u32> {
        self.last_receipt_owner_ids.clone()
    }

    /// The size.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The occupancy: one byte per row, 1 when the row holds any non-zero cell.
    pub fn occupancy(&self) -> Option<Vec<u8>> {
        if !self.wants_occupancy {
            return None;
        }
        Some(
            self.rows
                .iter()
                .map(|row| u8::from(row.iter().any(|byte| *byte != 0)))
                .collect(),
        )
    }

    /// The pointer, when the surface wants pointer events.
    pub fn pointer(&self) -> Option<(u32, u32)> {
        if self.wants_pointer {
            self.pointer
        } else {
            None
        }
    }

    /// Moves the pointer.
    pub fn move_pointer(&mut self, column: u32, row: u32) {
        self.pointer = Some((column, row));
    }

    /// The bytes of a row.
    pub fn row_bytes(&self, row: u32) -> Vec<u8> {
        self.rows.get(row as usize).cloned().unwrap_or_default()
    }

    /// The presented bytes of a row.
    pub fn presented_row(&self, row: u32) -> Vec<u8> {
        self.presented.get(row as usize).cloned().unwrap_or_default()
    }

    /// The cell colours of a row.
    pub fn cell_colors(&self, row: u32) -> Vec<u32> {
        self.colors.get(row as usize).cloned().unwrap_or_default()
    }

    /// Glyphs written.
    pub fn glyph_count(&self) -> u32 {
        self.glyphs
    }

    /// The frame as text: each byte is one cell, rows joined by line feeds.
    pub fn frame_text(&self) -> String {
        self.presented
            .iter()
            .map(|row| row.iter().map(|byte| char::from(*byte)).collect::<String>())
            .collect::<Vec<String>>()
            .join("\n")
    }

    /// Warnings counted.
    pub fn warn_count(&self) -> usize {
        self.warnings
    }

    /// Moves the clock.
    pub fn advance_ms(&mut self, ms: u64) {
        self.clock_ms = self.clock_ms.saturating_add(ms);
    }
}

/// The host side of one font resource (WIT:ilium:anim/fonts).
#[derive(Debug)]
pub struct Font {
    cell: (u32, u32),
}

impl Font {
    /// Opens a font by name. Only `ilium-mono` is installed in the rig.
    pub fn new(name: &str) -> Result<Font, String> {
        if name.is_empty() || name.contains('/') {
            return Err("font.invalid-name".to_owned());
        }
        if name != "ilium-mono" {
            return Err(format!("font.not-found: {name}"));
        }
        Ok(Font { cell: (8, 16) })
    }

    /// The cell size.
    pub fn cell_size(&self) -> (u32, u32) {
        self.cell
    }

    /// Rasterizes a character.
    pub fn rasterize(&self, character: char) -> Result<Vec<u8>, String> {
        self.rasterize_code(u32::from(character))
    }

    /// Rasterizes a code point. A space is blank; any other valid code point is a solid cell.
    pub fn rasterize_code(&self, code: u32) -> Result<Vec<u8>, String> {
        if char::from_u32(code).is_none() {
            return Err(format!("font.invalid-code-point: {code:#x}"));
        }
        let (width, height) = self.cell;
        let fill: u8 = if code == 0x20 { 0 } else { 0xff };
        Ok(vec![fill; (width * height) as usize])
    }
}

/// Display width of text in cells (WID-019).
pub fn cell_width(text: &str) -> usize {
    crate::text::terminal_width(text)
}

/// Sanitises text for display (RND-019). Removes escape sequences and control characters,
/// expands tabs to the next multiple of four cells, and turns line breaks into one space
/// (single line) or a line feed (multi-line).
pub fn sanitise(text: &str, single_line: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut column = 0usize;
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\u{1b}' => skip_escape(&mut chars),
            '\r' | '\n' => {
                if character == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                if single_line {
                    out.push(' ');
                    column += 1;
                } else {
                    out.push('\n');
                    column = 0;
                }
            }
            '\t' => {
                let stop = 4 - column % 4;
                for _ in 0..stop {
                    out.push(' ');
                }
                column += stop;
            }
            _ if is_removed_control(character) => {}
            _ => {
                out.push(character);
                column += 1;
            }
        }
    }
    out
}

/// The tooltip text shown for a tooltip (WID-015). Text over 200 scalars is cut to 199 and an
/// ellipsis.
pub fn tooltip_shown(text: &str) -> String {
    const LIMIT: usize = 200;
    if text.chars().count() <= LIMIT {
        return text.to_owned();
    }
    let mut shown: String = text.chars().take(LIMIT - 1).collect();
    shown.push('…');
    shown
}

/// The tab order of nodes given (tab index, focusable); 1-based node numbers (WID-018).
/// Indices 1 and above come first in ascending order; `None` and 0 follow in tree order;
/// negative indices and unfocusable nodes are left out.
pub fn tab_order(nodes: &[(Option<i16>, bool)]) -> Vec<usize> {
    let mut indexed: Vec<(i16, usize)> = Vec::new();
    let mut unindexed: Vec<usize> = Vec::new();
    for (position, (tab_index, focusable)) in nodes.iter().enumerate() {
        if !*focusable {
            continue;
        }
        let number = position + 1;
        match tab_index {
            Some(index) if *index >= 1 => indexed.push((*index, number)),
            Some(index) if *index < 0 => {}
            _ => unindexed.push(number),
        }
    }
    indexed.sort_by_key(|(index, _)| *index);
    indexed
        .into_iter()
        .map(|(_, number)| number)
        .chain(unindexed)
        .collect()
}

/// The next node in tab order (WID-018). Wraps from the last entry to the first.
pub fn tab_next(order: &[usize], current: usize) -> usize {
    match order.iter().position(|entry| *entry == current) {
        Some(position) => order[(position + 1) % order.len()],
        None => order.first().copied().unwrap_or(current),
    }
}

/// The widget families and their widgets (WID-001).
pub fn widget_families() -> BTreeMap<&'static str, Vec<&'static str>> {
    BTreeMap::from([
        (
            "layout-and-containers",
            vec![
                "layout",
                "block",
                "tabs",
                "popup",
                "clear",
                "scrollbar",
                "split",
                "separator",
                "dialog",
                "scroll-view",
            ],
        ),
        (
            "text",
            vec!["paragraph", "markdown", "leader-row", "key-hints", "diff-view"],
        ),
        (
            "collections",
            vec!["%list", "table", "tree-list", "menu", "check-list", "list-editor"],
        ),
        (
            "data-display",
            vec![
                "gauge",
                "line-gauge",
                "sparkline",
                "bar-chart",
                "chart",
                "canvas",
                "calendar",
                "progress-row",
                "meter",
                "minimap",
            ],
        ),
        (
            "input",
            vec![
                "button",
                "toggle",
                "choice",
                "number",
                "text-input",
                "text-area",
                "schema-form",
                "key-capture",
                "color-picker",
                "path-field",
                "location-picker",
            ],
        ),
        (
            "status-and-feedback",
            vec!["badge", "spinner", "notice", "icon"],
        ),
        (
            "media-and-embedded",
            vec!["image", "surface", "code-view", "terminal-view", "video"],
        ),
    ])
}

/// The hit layers in test order (HST-270): the first layer holding a node is hit first.
pub fn hit_layers_in_order() -> Vec<&'static str> {
    vec!["popups", "overlays", "content", "frame"]
}

/// The frame budget of a scene (RND-001): one second over the rate, rounded down, raised to the
/// frame deadline when that is larger (the deadline counts up to 50 ms).
pub fn frame_budget_ms(fps: u32, frame_deadline_ms: Option<u32>) -> u64 {
    let budget = 1000 / u64::from(fps.max(1));
    match frame_deadline_ms {
        Some(deadline) => budget.max(u64::from(deadline).min(RENDER_DEADLINE_CEILING_MS)),
        None => budget,
    }
}

/// The scene memory limit of a tier (RND-014). The default is 32 MiB; a request is capped at
/// 256 MiB for tier T1 and 128 MiB for other tiers.
pub fn scene_memory_limit_mib(tier: &str, requested: Option<u32>) -> u32 {
    let ceiling = if tier == "T1" { 256 } else { 128 };
    match requested {
        None => 32,
        Some(mib) => mib.min(ceiling),
    }
}

/// Installs a prerendered animation (RND-017): at most 3,600 frames, 30 fps and 128 MiB.
pub fn install_prerender(frames: u32, fps: u32, mib: u32) -> Result<(), String> {
    if frames > 3600 || fps > 30 || mib > 128 {
        return Err(format!(
            "manifest.scene-prerender: {frames} frames at {fps} fps and {mib} MiB exceed 3600 frames, 30 fps or 128 MiB"
        ));
    }
    Ok(())
}

/// Sets a decoration with frames (RND-018): at most 16 frames, each shown for at least 90 ms.
pub fn set_decoration_with_frames(frames: usize, frame_ms: u16) -> Result<(), String> {
    if frames > 16 {
        return Err(format!(
            "invalid-argument: a decoration has {frames} frames, at most 16 are allowed"
        ));
    }
    if frame_ms < 90 {
        return Err(format!(
            "invalid-argument: each decoration frame shows {frame_ms} ms, at least 90 ms is required"
        ));
    }
    Ok(())
}

/// A tree with one text node (RND-019). The text is kept raw; the rig sanitises it on delivery.
pub fn tree_with_text(text: &str) -> UiTree {
    let paragraph = Widget::Paragraph(ParagraphSpec {
        lines: vec![vec![plain_span(text)]],
        rich: None,
        wrap: Some(true),
        alignment: Alignment::Left,
        style: Style::default(),
        scrollable: false,
    });
    single_tree(plain_node(Some("text"), paragraph, Vec::new()))
}

/// A tree with one tooltip (WID-015).
pub fn tree_with_tooltip(text: &str) -> UiTree {
    let mut node = plain_node(Some("tooltip-target"), Widget::Clear, Vec::new());
    node.flags = NodeFlags::HOVER_EVENTS;
    node.tooltip = Some(Tooltip {
        lines: vec![vec![plain_span(text)]],
        delay_ms: None,
    });
    single_tree(node)
}

/// A tree whose nodes stack for hit testing (HST-270): one node per hit layer, all under the
/// frame root.
pub fn hit_stack_tree() -> UiTree {
    UiTree {
        version: 1,
        nodes: vec![
            plain_node(Some("frame"), Widget::Clear, vec![1, 2, 3]),
            plain_node(Some("content"), Widget::Clear, Vec::new()),
            plain_node(Some("overlays"), Widget::Clear, Vec::new()),
            plain_node(Some("popups"), Widget::Clear, Vec::new()),
        ],
        root: 0,
    }
}

/// A tree of one widget with `items` child nodes, keyed by the widget name.
fn widget_tree(widget: &str, items: u32) -> UiTree {
    let mut nodes = vec![plain_node(
        Some(widget),
        Widget::Clear,
        (1..=items).collect(),
    )];
    for item in 1..=items {
        nodes.push(plain_node(
            Some(format!("{widget}-item-{item}").as_str()),
            Widget::Clear,
            Vec::new(),
        ));
    }
    UiTree {
        version: 1,
        nodes,
        root: 0,
    }
}

/// Whether a widget name is one of the 51 catalogue widgets.
fn is_known_widget(widget: &str) -> bool {
    widget_families()
        .values()
        .any(|names| names.iter().any(|known| *known == widget))
}

/// Whether a tree holds a node with this key.
fn tree_has_key(tree: Option<&UiTree>, key: &str) -> bool {
    tree.is_some_and(|tree| tree.nodes.iter().any(|node| node.key.as_deref() == Some(key)))
}

/// The text lines of every paragraph in a tree.
fn tree_text_lines(tree: &UiTree) -> Vec<String> {
    let mut lines = Vec::new();
    for node in &tree.nodes {
        if let Widget::Paragraph(spec) = &node.widget {
            for line in &spec.lines {
                lines.push(line.iter().map(|span| span.text.as_str()).collect());
            }
        }
    }
    lines
}

/// Sanitises every paragraph span in place; `true` when any text changed.
fn sanitise_tree_text(tree: &mut UiTree) -> bool {
    let mut changed = false;
    for node in tree.nodes.iter_mut() {
        let Widget::Paragraph(spec) = &mut node.widget else {
            continue;
        };
        for span in spec.lines.iter_mut().flatten() {
            let clean = sanitise(&span.text, true);
            if clean != span.text {
                changed = true;
                span.text = clean;
            }
        }
    }
    changed
}

/// Adds a contribution to a registry, refusing a new node past the point's cap.
fn contribute(
    registry: &mut BTreeMap<String, BTreeMap<String, String>>,
    point: &str,
    node: &str,
    text: &str,
) -> Result<(), String> {
    let exists = registry
        .get(point)
        .is_some_and(|nodes| nodes.contains_key(node));
    let count = registry.get(point).map_or(0, |nodes| nodes.len());
    if !exists && count >= CONTRIBUTION_CAP as usize {
        return Err(format!(
            "contribution.cap-reached: point {point} holds {CONTRIBUTION_CAP} nodes"
        ));
    }
    registry
        .entry(point.to_owned())
        .or_default()
        .insert(node.to_owned(), text.to_owned());
    Ok(())
}

/// Shifts a node's scroll by rows, never below zero.
fn shift_scroll(scroll: &mut BTreeMap<String, u32>, key: &str, rows: i64) {
    let entry = scroll.entry(key.to_owned()).or_insert(0);
    *entry = (i64::from(*entry) + rows).clamp(0, i64::from(u32::MAX)) as u32;
}

/// Adds one to a named export counter.
fn bump(counts: &mut BTreeMap<String, u32>, export: &str) {
    *counts.entry(export.to_owned()).or_insert(0) += 1;
}

/// Whether a character is a control character the rig removes (after the escape, tab and line
/// break cases are handled).
fn is_removed_control(character: char) -> bool {
    character < ' ' || character == '\u{7f}' || ('\u{80}'..='\u{9f}').contains(&character)
}

/// Skips an escape sequence after its ESC: a CSI runs to its final byte, any other escape
/// drops the next character.
fn skip_escape(chars: &mut Peekable<Chars<'_>>) {
    if chars.peek() == Some(&'[') {
        chars.next();
        for character in chars.by_ref() {
            if ('\u{40}'..='\u{7e}').contains(&character) {
                break;
            }
        }
    } else {
        chars.next();
    }
}

/// A span with the default style.
fn plain_span(text: &str) -> Span {
    Span {
        text: text.to_owned(),
        style: Style::default(),
    }
}

/// A node with no flags, tooltip or tab index.
fn plain_node(key: Option<&str>, widget: Widget, children: Vec<u32>) -> UiNode {
    UiNode {
        key: key.map(str::to_owned),
        widget,
        children,
        flags: NodeFlags::default(),
        tooltip: None,
        tab_index: None,
        disabled_reason: None,
    }
}

/// A tree holding one node at the root.
fn single_tree(node: UiNode) -> UiTree {
    UiTree {
        version: 1,
        nodes: vec![node],
        root: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        cell_width, frame_budget_ms, hit_layers_in_order, hit_stack_tree, install_prerender,
        sanitise, scene_memory_limit_mib, set_decoration_with_frames, tab_next, tab_order,
        tooltip_shown, tree_with_text, widget_families, Rig, SceneRig,
    };

    /// REQ: WID-019
    #[test]
    fn cell_width_counts_ascii_one_cell_per_character() {
        assert_eq!(cell_width("abc"), 3);
    }

    /// REQ: WID-019
    #[test]
    fn cell_width_of_empty_text_is_zero() {
        assert_eq!(cell_width(""), 0);
    }

    /// REQ: WID-019
    #[test]
    fn cell_width_wide_glyphs_take_two_cells_each() {
        assert_eq!(cell_width("漢字"), 4);
    }

    /// REQ: WID-019
    #[test]
    fn cell_width_combining_mark_adds_no_cells() {
        assert_eq!(cell_width("e\u{0301}"), 1);
    }

    /// REQ: WID-019
    #[test]
    fn cell_width_zero_width_joiner_alone_is_zero() {
        assert_eq!(cell_width("\u{200D}"), 0);
    }

    /// REQ: WID-019
    #[test]
    fn cell_width_emoji_presentation_selector_makes_two_cells() {
        assert_eq!(cell_width("\u{2764}\u{FE0F}"), 2);
    }

    /// REQ: WID-019
    #[test]
    fn cell_width_private_use_nerd_font_glyph_is_one_cell() {
        assert_eq!(cell_width("\u{F120}"), 1);
    }

    /// REQ: RND-019
    #[test]
    fn sanitise_removes_csi_escape_sequences() {
        assert_eq!(sanitise("\u{1b}[31mred\u{1b}[0m", true), "red");
    }

    /// REQ: RND-019
    #[test]
    fn sanitise_removes_c0_c1_and_delete_controls() {
        assert_eq!(sanitise("a\u{7}b\u{85}c\u{7f}d", true), "abcd");
    }

    /// REQ: RND-019
    #[test]
    fn sanitise_tab_expands_to_next_multiple_of_four_cells() {
        assert_eq!(sanitise("ab\tc", true), "ab  c");
    }

    /// REQ: RND-019
    #[test]
    fn sanitise_tab_on_a_stop_adds_a_full_four_cells() {
        assert_eq!(sanitise("abcd\te", true), "abcd    e");
    }

    /// REQ: RND-019
    #[test]
    fn sanitise_single_line_turns_cr_lf_and_crlf_into_one_space() {
        assert_eq!(sanitise("a\nb", true), "a b");
        assert_eq!(sanitise("a\rb", true), "a b");
        assert_eq!(sanitise("a\r\nb", true), "a b");
    }

    /// REQ: RND-019
    #[test]
    fn sanitise_multi_line_keeps_line_feed() {
        assert_eq!(sanitise("a\nb", false), "a\nb");
    }

    /// REQ: RND-019
    #[test]
    fn sanitise_removes_escape_before_tab_expansion() {
        assert_eq!(sanitise("\u{1b}[31m\tx", true), "    x");
    }

    #[test]
    fn sanitise_empty_text_is_empty() {
        assert_eq!(sanitise("", true), "");
    }

    /// REQ: WID-015
    #[test]
    fn tooltip_shown_keeps_text_of_exactly_200_scalars() {
        let text = "a".repeat(200);
        assert_eq!(tooltip_shown(&text), text);
    }

    /// REQ: WID-015
    #[test]
    fn tooltip_shown_cuts_long_text_to_199_scalars_and_ellipsis() {
        let shown = tooltip_shown(&"a".repeat(300));
        assert_eq!(shown, format!("{}…", "a".repeat(199)));
        assert_eq!(shown.chars().count(), 200);
    }

    /// REQ: WID-015
    #[test]
    fn tooltip_shown_counts_scalar_values_not_bytes() {
        let shown = tooltip_shown(&"é".repeat(201));
        assert_eq!(shown, format!("{}…", "é".repeat(199)));
    }

    #[test]
    fn tooltip_shown_empty_text_is_empty() {
        assert_eq!(tooltip_shown(""), "");
    }

    /// REQ: WID-018
    #[test]
    fn tab_order_visits_positive_indices_ascending_then_unindexed_in_preorder() {
        assert_eq!(
            tab_order(&[(Some(2), true), (Some(1), true), (None, true)]),
            vec![2, 1, 3]
        );
    }

    /// REQ: WID-018
    #[test]
    fn tab_order_zero_index_joins_unindexed_nodes_in_preorder() {
        assert_eq!(
            tab_order(&[(Some(0), true), (None, true), (Some(1), true)]),
            vec![3, 1, 2]
        );
    }

    /// REQ: WID-018
    #[test]
    fn tab_order_equal_indices_keep_tree_preorder() {
        assert_eq!(tab_order(&[(Some(1), true), (Some(1), true)]), vec![1, 2]);
    }

    /// REQ: WID-018
    #[test]
    fn tab_order_skips_negative_indices_and_unfocusable_nodes() {
        assert_eq!(
            tab_order(&[(Some(-1), true), (None, false), (None, true)]),
            vec![3]
        );
    }

    #[test]
    fn tab_order_of_no_nodes_is_empty() {
        assert_eq!(tab_order(&[]), Vec::<usize>::new());
    }

    /// REQ: WID-018
    #[test]
    fn tab_next_advances_to_following_entry() {
        assert_eq!(tab_next(&[2, 1, 3], 2), 1);
    }

    /// REQ: WID-018
    #[test]
    fn tab_next_wraps_from_last_entry_to_first() {
        assert_eq!(tab_next(&[2, 1, 3], 3), 2);
    }

    /// REQ: WID-001
    #[test]
    fn widget_families_holds_seven_families_and_51_widgets() {
        let families = widget_families();
        assert_eq!(families.len(), 7);
        let total: usize = families.values().map(|names| names.len()).sum();
        assert_eq!(total, 51);
    }

    /// REQ: WID-001
    #[test]
    fn widget_families_widget_names_are_distinct() {
        let families = widget_families();
        let all: Vec<&str> = families.values().flatten().copied().collect();
        let unique: std::collections::BTreeSet<&str> = all.iter().copied().collect();
        assert_eq!(unique.len(), 51);
    }

    /// REQ: WID-001
    #[test]
    fn widget_families_sizes_match_the_catalogue() {
        let families = widget_families();
        let size_of = |widget: &str| {
            families
                .values()
                .find(|names| names.contains(&widget))
                .map(|names| names.len())
        };
        assert_eq!(size_of("layout"), Some(10));
        assert_eq!(size_of("paragraph"), Some(5));
        assert_eq!(size_of("%list"), Some(6));
        assert_eq!(size_of("gauge"), Some(10));
        assert_eq!(size_of("button"), Some(11));
        assert_eq!(size_of("badge"), Some(4));
        assert_eq!(size_of("image"), Some(5));
    }

    /// REQ: HST-270
    #[test]
    fn hit_layers_are_popups_overlays_content_frame() {
        assert_eq!(
            hit_layers_in_order(),
            vec!["popups", "overlays", "content", "frame"]
        );
    }

    /// REQ: RND-001
    #[test]
    fn frame_budget_is_one_second_over_fps_rounded_down() {
        assert_eq!(frame_budget_ms(30, None), 33);
        assert_eq!(frame_budget_ms(60, None), 16);
        assert_eq!(frame_budget_ms(7, None), 142);
    }

    /// REQ: RND-001
    #[test]
    fn frame_budget_at_boundary_rates() {
        assert_eq!(frame_budget_ms(1, None), 1000);
        assert_eq!(frame_budget_ms(1000, None), 1);
    }

    /// REQ: RND-001
    #[test]
    fn frame_deadline_raises_budget_up_to_50_ms() {
        assert_eq!(frame_budget_ms(30, Some(40)), 40);
        assert_eq!(frame_budget_ms(30, Some(50)), 50);
    }

    /// REQ: RND-001
    #[test]
    fn frame_deadline_never_lowers_the_budget() {
        assert_eq!(frame_budget_ms(30, Some(20)), 33);
    }

    /// REQ: RND-014
    #[test]
    fn scene_memory_default_is_32_mib_for_every_tier() {
        assert_eq!(scene_memory_limit_mib("T1", None), 32);
        assert_eq!(scene_memory_limit_mib("T2", None), 32);
    }

    /// REQ: RND-014
    #[test]
    fn scene_memory_t1_ceiling_is_256_mib() {
        assert_eq!(scene_memory_limit_mib("T1", Some(256)), 256);
        assert_eq!(scene_memory_limit_mib("T1", Some(512)), 256);
    }

    /// REQ: RND-014
    #[test]
    fn scene_memory_other_tier_ceiling_is_128_mib() {
        assert_eq!(scene_memory_limit_mib("T2", Some(128)), 128);
        assert_eq!(scene_memory_limit_mib("T2", Some(256)), 128);
    }

    /// REQ: RND-014
    #[test]
    fn scene_memory_request_below_ceiling_is_honoured() {
        assert_eq!(scene_memory_limit_mib("T1", Some(64)), 64);
    }

    /// REQ: RND-017
    #[test]
    fn install_prerender_accepts_loop_at_the_limits() {
        assert!(install_prerender(3600, 30, 128).is_ok());
    }

    /// REQ: RND-017
    #[test]
    fn install_prerender_rejects_more_than_3600_frames() {
        let error = install_prerender(3601, 30, 128).unwrap_err();
        assert!(error.contains("manifest.scene-prerender"));
    }

    /// REQ: RND-017
    #[test]
    fn install_prerender_rejects_more_than_30_fps() {
        let error = install_prerender(3600, 31, 128).unwrap_err();
        assert!(error.contains("manifest.scene-prerender"));
    }

    /// REQ: RND-017
    #[test]
    fn install_prerender_rejects_more_than_128_mib() {
        let error = install_prerender(3600, 30, 129).unwrap_err();
        assert!(error.contains("manifest.scene-prerender"));
    }

    /// REQ: RND-018
    #[test]
    fn decoration_accepts_16_frames_each_shown_90_ms() {
        assert!(set_decoration_with_frames(16, 90).is_ok());
    }

    /// REQ: RND-018
    #[test]
    fn decoration_rejects_17_frames_as_invalid_argument() {
        let error = set_decoration_with_frames(17, 90).unwrap_err();
        assert!(error.contains("invalid-argument"));
    }

    /// REQ: RND-018
    #[test]
    fn decoration_rejects_frame_shorter_than_90_ms_as_invalid_argument() {
        let error = set_decoration_with_frames(16, 89).unwrap_err();
        assert!(error.contains("invalid-argument"));
    }

    /// REQ: RND-001
    #[test]
    fn scene_frame_costing_more_than_budget_is_skipped() {
        let mut rig = SceneRig::new(30);
        rig.frame_cost_ms(40);
        rig.run_frames(1);
        assert_eq!(rig.budget_ms(), 33);
        assert_eq!(rig.skipped_frames(), 1);
    }

    /// REQ: RND-014
    #[test]
    fn scene_allocation_over_default_limit_traps() {
        let mut rig = SceneRig::new(30);
        rig.allocate_in_next_frame_mib(64);
        rig.run_frames(1);
        assert!(rig.trap().is_some());
    }

    /// REQ: RND-019
    #[test]
    fn rig_sanitises_view_text_and_warns_once_per_view() {
        let mut rig = Rig::new("T2");
        let view = rig.open_view("status");
        rig.deliver_tree(view, tree_with_text("a\u{1b}[31mb"))
            .unwrap();
        let shown = rig.screen_text();
        assert!(shown.contains("ab"));
        assert!(!shown.contains('\u{1b}'));
        rig.deliver_tree(view, tree_with_text("c\u{1b}[0m d"))
            .unwrap();
        let warnings = rig
            .log_lines()
            .iter()
            .filter(|(first, second)| first == "ui.text-sanitized" || second == "ui.text-sanitized")
            .count();
        assert_eq!(warnings, 1);
    }

    /// REQ: HST-270
    #[test]
    fn rig_hit_test_resolves_on_ui_thread_without_calling_plugin() {
        let mut rig = Rig::new("T2");
        let view = rig.open_view("main");
        rig.deliver_tree(view, hit_stack_tree()).unwrap();
        let calls_before = rig.plugin_calls_received();
        let _hit = rig.hit_test(view, 0, 0);
        assert!(rig.hit_resolved_on_ui_thread());
        assert_eq!(rig.plugin_calls_received(), calls_before);
    }
}
