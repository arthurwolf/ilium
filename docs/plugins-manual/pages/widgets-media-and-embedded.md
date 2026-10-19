---
layout: page
title: Media and embedded views
permalink: /pages/widgets-media-and-embedded/
---

## Media and embedded views

Five widgets show things that text and tables cannot. `image` shows a picture from the package, a file or encoded bytes. `surface` shows pixels that the plugin draws itself, with an optional glyph plane and owner ids. `code-view` shows a text buffer with the native editor, read-only or editable. `terminal-view` shows an existing pane, read-only or interactive. `video` plays a clip from the package, a file or a range of a file.

| Widget | Shows | The plugin supplies |
|---|---|---|
| `image` | A picture | An asset path, an `fs-path`, decoded pixels or encoded bytes |
| `surface` | Pixels the plugin writes | Rows of pixels with `ui.write-surface`, glyph cells, owner ids |
| `code-view` | A text buffer | A buffer id |
| `terminal-view` | A live view of a pane | A pane node id |
| `video` | A clip | An asset, a file or a file range |

`image` mirrors ratatui-image's `StatefulImage`, and `surface` mirrors the Ilium frame surface. The editing and terminal widgets mirror the native views, so a plugin sees keystrokes only through the events of the grants it holds.

## You need

- The `ui.panels` grant for a panel, settings page or onboarding step. A pane kind that shows these widgets needs `panes.provide-kind` instead. Both are client-side grants.
- `fs.read` for the scope of an `fs-path` image or video source. Package assets need no grant, because `fs.read:package` is always granted.
- For `code-view`: `editor.edit` to show a buffer that the plugin created with `create-buffer`, and `editor.read` to show any other buffer or to receive `editor-changed` events.
- For `terminal-view`: `terminal.read-screen`, and `terminal.raw-input` for `interactive`.
- For `video`: `media.video`. Without `audio.playback` the video is always muted.
- The SDK for Rust (`ilium-plugin-sdk`) or TypeScript (`@ilium/plugin-sdk`). The worked examples use the builders from `ilium_plugin_sdk::ui::build` and `@ilium/plugin-sdk/ui/build`.

Without a grant, the widget draws an empty frame with the text `not permitted`.

## Properties

Every widget takes an optional `key`. The host keeps widget state by key, and actions name the key. Set the `focusable` flag on a `surface` or `image` that sends clicks.

| Widget | Property | Type | Default | Meaning |
|---|---|---|---|---|
| `image` | `source` | `image-source` | required | `asset(string)`, `fs-path(fs-path)`, `rgba(decoded-image)` or `bytes(list<u8>, string)`. |
| `image` | `fit` | `contain`, `cover` or `fill` | `contain` | How the picture meets the area. |
| `surface` | `format` | `pixel-format` | required | `mask8`, `mono1`, `gray8`, `rgb8` or `rgba8`. |
| `surface` | `min-columns`, `min-rows` | `u16` | 1 | Size in cells. |
| `surface` | `glyph-plane` | `bool` | `false` | Allow `write-glyphs`. |
| `surface` | `owner-ids` | `bool` | `false` | Allow `set-owner-ids` and pointer hit tests. |
| `code-view` | `buffer` | `buffer-id` | required | The buffer to show. |
| `code-view` | `read-only` | `bool` | `true` | Whether the user can type. |
| `code-view` | `line-numbers` | `bool` | `true` | Whether line numbers are shown. |
| `code-view` | `wrap` | `bool` | `false` | Whether long lines wrap. |
| `code-view` | `minimap` | `bool` | `false` | Whether a minimap is drawn at the right edge. |
| `terminal-view` | `pane` | `node-id` | required | The existing pane to show. |
| `terminal-view` | `rows` | `option<u16>` | none | Height in rows. Without it the view fills the area. |
| `terminal-view` | `interactive` | `bool` | `false` | Forward keys and mouse to the program. Needs `terminal.raw-input`. |
| `terminal-view` | `scrollback` | `bool` | `false` | Allow scrolling the history. |
| `terminal-view` | `highlights` | `option<string>` | none | Name of an own highlight layer. |
| `video` | `source` | `video-source` | required | `asset(string)`, `fs(fs-path)` or `range(fs-path, u64, option<u64>)`. |
| `video` | `autoplay`, `loop`, `muted`, `controls` | `bool` | `false` | Playback options. Rust spells `loop` as `looped`. |
| `video` | `fit` | `contain`, `cover` or `fill` | `contain` | How the frame meets the area. |

The pixel size of a surface is `min-columns` and `min-rows` multiplied by the sub-cell factor of the user's active dither mode, so it depends on that setting. The plugin asks for the size with `ui::surface_size` and does not compute it.

## Actions and events

| Widget | Sends | Value | Notes |
|---|---|---|---|
| `image` | `activated` when focusable | `cell(cell-position)` | Pictures are otherwise not interactive. |
| `surface` | `activated` on a click | `cell(cell-position)`, relative to the surface | Needs `focusable`. |
| `surface` | `pointer` | `pointer(view-pointer)` | With the `pointer-events` flag. The receipt names the owner id under the pointer. |
| `surface` | `scrolled` on the wheel | `number(f64)`, notches since the last action | Negative up, positive down. Coalesced. |
| `code-view` | no `ui-action` | none | Changes arrive as `editor-changed` events, debounced by 100 ms. |
| `terminal-view` | `activated` | `none` | A request to focus the view. |
| `video` | `changed` on seek | `number(f64)`, the position in ms | |
| `video` | `activated` | `none` | Play or pause. |

## Keys, mouse and roles

| Widget | Keys | Mouse | Theme |
|---|---|---|---|
| `image`, `surface` | none | click | The shared look and dithering apply. |
| `code-view` | The editor's keys. | The editor's mouse rules. | The `syntax-*` roles and the gutter in `muted`. |
| `terminal-view` | None, or all keys when `interactive`. | Click focuses. With `interactive` the mouse goes to the program. | The terminal colours of the pane. |
| `video` | Space play or pause, Left and Right seek 5 s. | Click toggles play. | The shared look. The progress line is `━` and `─`. |

An interactive `terminal-view` forwards keys and mouse to the pane's program while it has focus. Press Escape twice to leave it.

## image

An asset is a path relative to the package's `assets/` folder, for example `asset("icons/logo.png")`, and the file must be listed in the package `files` inventory. An `fs-path` names a file under a file scope and needs the `fs.read` grant. `rgba` carries decoded pixels, and `bytes` carries an encoded PNG, JPEG, GIF or WebP up to 16 MiB with its MIME type.

The host decodes the first frame with the limits of `codec.decode-image`: input up to 16 MiB and up to 67,108,864 pixels. The decoded picture is kept until the source changes. Supported formats are PNG, JPEG, GIF, WebP and BMP.

- `contain` keeps the aspect ratio. Unused cells keep the cell background.
- `cover` keeps the aspect ratio and crops the overflow equally at both ends.
- `fill` stretches the picture to the area.

<figure>
<pre class="term">╭─ Logo ─────────────────────────────────╮
│          <span class="t-hi">      ▄▄▄▄▄▄      </span>            │
│          <span class="t-hi">   ▄██▀▀▀▀▀▀██▄   </span>            │
│          <span class="t-hi">  ██▀  ▄▄▄▄  ▀██  </span>            │
│          <span class="t-hi">  ██   ▀██▀   ██  </span>            │
│          <span class="t-hi">  ▀██▄▄▄▄▄▄▄▄██▀  </span>            │
│          <span class="t-hi">     ▀▀▀▀▀▀▀▀     </span>            │
╰────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 42 columns. An <code>image</code> fitted with <code>contain</code>. The host draws the picture with half blocks when the terminal has no graphics protocol, and the empty margin keeps the cell background.</figcaption>
</figure>

**Rust**

```rust
// types::ui::{ImageSource, MediaFit}
let logo = tree
    .image(ImageSource::Asset("icons/logo.png".into()))
    .fit(MediaFit::Contain)
    .key("logo");
let avatar = tree.image(ImageSource::Bytes(png_bytes, "image/png".into())).key("avatar");
```

**TypeScript**

```ts
const logo = image({ tag: 'asset', val: 'icons/logo.png' }, { key: 'logo', fit: 'contain' });
const avatar = image({ tag: 'bytes', val: [pngBytes, 'image/png'] }, { key: 'avatar' });
```

## surface

A `surface` has a fixed pixel format. The plugin writes rows of pixels into it with `ui::write_surface(view, key, first_row, format, data)` in Rust, or `writeSurface(view, key, firstRow, format, data)` in TypeScript. The host converts those pixels to cells with the shared look and dithering.

| Format | Bytes per pixel | Layout |
|---|---|---|
| `mask8` | 1 | Coverage from 0 to 255. 255 is fully on. |
| `mono1` | 1 bit | One bit per pixel, most significant bit first. Each row is padded to a whole byte, so a row is `ceil(width / 8)` bytes. |
| `gray8` | 1 | One tone per pixel. |
| `rgb8` | 3 | Red, green, blue. |
| `rgba8` | 4 | Red, green, blue, alpha. |

A row holds `width` pixels, where `width` is the pixel width from `ui::surface_size(view, key)`. The `data` must hold whole rows, and `first-row` plus the number of rows written must stay inside `height`.

<figure>
<svg viewBox="0 0 360 240" role="img" aria-label="Plugin pixels pass through the surface, the shared look and dithering, and become cells">
  <title>From pixels to cells</title>
  <desc>The plugin writes rows of pixels. The host keeps them in a surface buffer, applies the shared look and dithering, and draws cells.</desc>
  <defs>
    <marker id="media-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="20" y="10" width="320" height="44" rx="6" fill="none" stroke="currentColor" stroke-dasharray="6 4"/>
  <text x="180" y="30" font-size="11" text-anchor="middle" fill="currentColor">Plugin: ui.write-surface</text>
  <text x="180" y="46" font-size="11" text-anchor="middle" fill="currentColor">rows of pixels in one pixel format</text>
  <line x1="180" y1="54" x2="180" y2="76" stroke="currentColor" marker-end="url(#media-arrow)"/>
  <rect x="20" y="76" width="320" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="180" y="96" font-size="11" text-anchor="middle" fill="currentColor">Surface buffer</text>
  <text x="180" y="112" font-size="11" text-anchor="middle" fill="currentColor">one value per pixel, in the declared format</text>
  <line x1="180" y1="120" x2="180" y2="142" stroke="currentColor" marker-end="url(#media-arrow)"/>
  <rect x="20" y="142" width="320" height="44" rx="6" fill="none" stroke="currentColor"/>
  <text x="180" y="162" font-size="11" text-anchor="middle" fill="currentColor">Shared look and dithering</text>
  <text x="180" y="178" font-size="11" text-anchor="middle" fill="currentColor">colour mode, palette, filters, dither</text>
  <line x1="180" y1="186" x2="180" y2="208" stroke="currentColor" marker-end="url(#media-arrow)"/>
  <rect x="20" y="208" width="320" height="26" rx="6" fill="none" stroke="currentColor"/>
  <text x="180" y="225" font-size="11" text-anchor="middle" fill="currentColor">Cells of the surface widget</text>
</svg>
<figcaption>The host converts surface pixels to cells, so the shared colour settings apply to pictures that the plugin draws.</figcaption>
</figure>

<figure>
<pre class="term">╭─ Surface 34 x 5 ───────────────────╮
│ <span class="t-ok"> ░▒▓█ ░▒▓█ ░▒▓█ ░▒▓█ ░▒▓█ ░▒▓█ ░▒▓</span> │
│ <span class="t-ok">░▒▓█ ░▒▓█ ░▒▓█ ░▒▓█ ░▒▓█ ░▒▓█ ░▒▓█</span> │
│ <span class="t-ok">▒▓█ ░▒▓█ ░▒▓█ ░▒▓█ ░▒▓█ ░▒▓█ ░▒▓█ </span> │
│ <span class="t-ok">▓█ ░▒▓█ ░▒▓█ ░▒▓█ ░▒▓█ ░▒▓█ ░▒▓█ ░</span> │
│ <span class="t-ok">█ ░▒▓█ ░▒▓█ ░▒▓█ ░▒▓█ ░▒▓█ ░▒▓█ ░▒</span> │
╰────────────────────────────────────╯
</pre>
<figcaption>Rendered at 38 columns. A <code>gray8</code> <code>surface</code> after dithering. The pixels come from the plugin, and the shared look and dither mode decide the cell glyphs.</figcaption>
</figure>

A click on a focusable surface arrives as `activated` with `cell(cell-position)`, the column and row relative to the surface. A wheel movement arrives as `scrolled`.

### Glyph plane and owner ids

A surface created with `glyph-plane: true` accepts glyph cells. `write-glyphs` takes a list of `glyph-cell` records (`column`, `row`, `text`, `fg`, `bg`, `add`) with at most 131,072 cells per frame, and `clear-glyphs` removes them. The cells are drawn over the pixel layer after dithering and the shared look, with icon widths, and their colours go through the shared look too. Without `glyph-plane`, `write-glyphs` returns `unsupported`.

<figure>
<pre class="term">╭─ Board ────────────────────────────╮
│ <span class="t-dim">░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░</span> │
│ <span class="t-hi">      ♜ ♞ ♝ ♛ ♚ ♝ ♞ ♜             </span> │
│ <span class="t-hi">      ♟ ♟ ♟ ♟ ♟ ♟ ♟ ♟             </span> │
│ <span class="t-dim">░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░</span> │
│ <span class="t-dim">░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░</span> │
╰────────────────────────────────────╯
</pre>
<figcaption>Rendered at 38 columns. A <code>surface</code> with <code>glyph-plane</code>. The pixel layer is the background, and the glyph cells are drawn over it after dithering. Glyph colours go through the shared look.</figcaption>
</figure>

A surface created with `owner-ids: true` accepts `set-owner-ids(first-row, ids)`, one `u16` per pixel, where 0 means none. After a frame, `last-receipt` returns a `frame-receipt` with `frame`, `presented-at-millis`, `dropped-before` and `pointer-owner`. A plugin that sets the `pointer-events` flag can hit-test a click on a logical object without keeping a copy of the frame. The same calls exist on scene surfaces, described in [Primitives](/pages/primitives/).

**Rust**

```rust
// host::ui, types::ui::{GlyphCell, PixelFormat}
let board = tree
    .surface(PixelFormat::Rgb8)
    .min_columns(40)
    .min_rows(12)
    .glyph_plane(true)
    .owner_ids(true)
    .key("board")
    .flags(NodeFlags::FOCUSABLE | NodeFlags::POINTER_EVENTS);

// After render, in an event handler:
ui::write_glyphs(panel_view(), "board", &[GlyphCell {
    column: 3, row: 1, text: "N".into(), fg: Some(0xffffff), bg: None, add: Modifiers::BOLD,
}])?;
ui::set_owner_ids(panel_view(), "board", 0, &owner_ids)?;
```

**TypeScript**

```ts
const board = surface({
  key: 'board',
  format: 'rgb8',
  minColumns: 40,
  minRows: 12,
  glyphPlane: true,
  ownerIds: true,
  flags: { focusable: true, pointerEvents: true },
});

// After render, in an event handler:
writeGlyphs(panelView(), 'board', [
  { column: 3, row: 1, text: 'N', fg: 0xffffff, bg: undefined, add: { bold: true } },
]);
setOwnerIds(panelView(), 'board', 0, ownerIds);
```

## code-view

A `code-view` shows the buffer as the native editor does. The user types, moves the cursor, selects and undoes natively. Set `read-only: false` and hold `editor.edit` to make it a text editor. The plugin learns about changes through `editor-changed`, which carries `buffer`, `revision`, `first-line` and `last-line` but not the text. The plugin reads the lines it needs with the editor primitive ([Editor primitive](/pages/primitives-editor/)). Syntax colours come from the `token-provider` exports of languages that plugins declare.

<figure>
<pre class="term">╭─ notes.md ─────────────────────────────────╮
│ <span class="t-dim">  1 </span># Plugin notes                         │
│ <span class="t-dim">  2 </span>                                       │
│ <span class="t-dim">  3 </span>The build uses the release profile.    │
│ <span class="t-dim">  4 </span>The cursor sits on line four.▏         │
│ <span class="t-dim">  5 </span>                                       │
│ <span class="t-dim">  6 </span>- check the grant list                 │
│ <span class="t-dim">  7 </span>- run the probe                        │
╰────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 46 columns. A <code>code-view</code> with line numbers. The gutter uses <code>muted</code>. Syntax colours come from the <code>syntax-*</code> theme roles.</figcaption>
</figure>

**Rust**

```rust
let buffer = editor::create_buffer("# Plugin notes\n", Some("markdown"))?;
let notes = tree
    .code_view(buffer)
    .read_only(false)
    .line_numbers(true)
    .wrap(true)
    .minimap(true)
    .key("notes");
```

**TypeScript**

```ts
const buffer = createBuffer('# Plugin notes\n', 'markdown');
const notes = codeView(buffer, {
  key: 'notes',
  readOnly: false,
  lineNumbers: true,
  wrap: true,
  minimap: true,
});
```

## terminal-view

A `terminal-view` embeds a pane that already exists. It shows the bottom rows of the pane's visible area. With `rows` set, the view is that many rows tall at the top of its area. The view never resizes the pane. By default the view is read-only. With `interactive: true` the view forwards keys and mouse to the pane's program while it has focus, and the plugin needs `terminal.raw-input`. Press Escape twice to leave an interactive view.

<figure>
<pre class="term">╭─ Pane: build ──────────────────────────────────────────╮
│ <span class="t-prompt">$ </span>cargo test                                           │
│    Compiling ilium-core v0.1.0                         │
│     Running unittests src/lib.rs                       │
│ <span class="t-ok">test result: ok. 14 passed; 0 failed</span>                   │
│                                                        │
│ <span class="t-dim">Keys go to the pane owner, not to the plugin.</span>          │
╰────────────────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 58 columns. A read-only <code>terminal-view</code>. The cells use the terminal colours of the pane, not the theme.</figcaption>
</figure>

**Rust**

```rust
let build = tree.terminal_view(self.build_pane).rows(8).key("build");
let shell = tree
    .terminal_view(self.shell_pane)
    .interactive(true)   // needs terminal.raw-input
    .scrollback(true)
    .key("shell");
```

**TypeScript**

```ts
const build = terminalView(buildPane, { key: 'build', rows: 8 });
const shell = terminalView(shellPane, {
  key: 'shell',
  interactive: true,   // needs terminal.raw-input
  scrollback: true,
});
```

## video

A `video` is decoded natively, and the shared look and dithering apply to its frames. The source is a package asset, a file under a granted scope, or a range of a file given as a start and an optional end in milliseconds. Video bytes up to 16 MiB can also be opened from memory with `media.open-video-bytes`. Playback is always muted unless the plugin holds `audio.playback`. With `controls` the widget draws a progress line of `━` and `─`.

<figure>
<pre class="term">╭─ Shore ────────────────────────────────────╮
│ <span class="t-dim">▒▒▒▒▓▓▓▓████▓▓▒▒░░░░▒▒▓▓████▓▓▒▒░░░░▒▒▒▒</span>   │
│ <span class="t-dim">░░▒▒▓▓████▓▓▒▒░░░░▒▒▓▓████▓▓▒▒░░▒▒▓▓████▓▓</span> │
│ <span class="t-dim">▒▒▒▒░░░░▒▒▓▓████▓▓▒▒░░░░▒▒▓▓████▓▓▒▒░░░░▒▒</span> │
│                                            │
│ <span class="t-hi">━━━━━━━━━━━━━━━━━━</span><span class="t-dim">────────────────────────</span> │
│ <span class="t-dim">Space play/pause · Left/Right seek 5 s    </span> │
╰────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 46 columns. A <code>video</code> with <code>controls</code>. The frame is dithered into cells, and the progress line uses <code>━</code> and <code>─</code>.</figcaption>
</figure>

**Rust**

```rust
// types::ui::{VideoSource, MediaFit}
let clip = tree
    .video(VideoSource::Asset("loops/shore.webm".into()))
    .autoplay(true)
    .looped(true)
    .controls(true)
    .fit(MediaFit::Cover)
    .key("clip");
```

**TypeScript**

```ts
const clip = video({ tag: 'asset', val: 'loops/shore.webm' }, {
  key: 'clip',
  autoplay: true,
  loop: true,
  controls: true,
  fit: 'cover',
});
```

## Worked example in Rust

The example is a panel with five widgets. It creates a notes buffer on activation and repaints the surface on scroll.

```rust
use ilium_plugin_sdk::host::{editor, ui};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::ids::BufferId;
use ilium_plugin_sdk::types::layout::{Area, Constraint, Direction};
use ilium_plugin_sdk::types::ui::{
    ActionKind, ActionValue, MediaFit, ImageSource, PixelFormat, UiTree, VideoSource, ViewTarget,
};
use ilium_plugin_sdk::ui::build::{NodeFlags, Tree};

const PANEL: &str = "media-demo";
const SCOPE_KEY: &str = "scope";
const SCOPE_COLUMNS: u16 = 40;
const SCOPE_ROWS: u16 = 6;

#[ilium_plugin(side = "client", exports = ["lifecycle", "handler", "views"])]
#[derive(Default)]
struct MediaDemo {
    notes: Option<BufferId>,
    // Set by on_events when the build pane is known, for example from a PaneCreated event.
    build_pane: Option<NodeId>,
    scroll: u32,
}

impl Lifecycle for MediaDemo {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        self.notes = Some(editor::create_buffer("# Plugin notes\n", Some("markdown"))?);
        Ok(())
    }
}

impl Handler for MediaDemo {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            let Event::UiAction(action) = event else { continue };
            if action.key != SCOPE_KEY {
                continue;
            }
            let (ActionKind::Scrolled, ActionValue::Number(notches)) = (action.kind, action.value)
            else {
                continue;
            };
            // Wheel notches are negative for up and positive for down.
            self.scroll = self.scroll.wrapping_add(notches as i32 as u32);
            self.paint_scope();
        }
        Vec::new()
    }
}

impl MediaDemo {
    fn paint_scope(&self) {
        // None until the panel has rendered once.
        let Some((width, height)) = ui::surface_size(panel_view(), SCOPE_KEY) else {
            return;
        };
        let mut tones = Vec::with_capacity((width * height) as usize);
        for row in 0..height {
            for column in 0..width {
                tones.push((column + row + self.scroll) as u8);
            }
        }
        // gray8 is one byte per pixel; the rows start at the top of the surface.
        let _ = ui::write_surface(panel_view(), SCOPE_KEY, 0, PixelFormat::Gray8, &tones);
    }
}

fn panel_view() -> ViewTarget {
    ViewTarget::Panel(PANEL.to_string())
}

impl Views for MediaDemo {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut tree = Tree::new();
        let logo = tree
            .image(ImageSource::Asset("icons/logo.png".to_string()))
            .fit(MediaFit::Contain)
            .key("logo");
        let scope = tree
            .surface(PixelFormat::Gray8)
            .min_columns(SCOPE_COLUMNS)
            .min_rows(SCOPE_ROWS)
            .key(SCOPE_KEY)
            .flags(NodeFlags::FOCUSABLE);
        let clip = tree
            .video(VideoSource::Asset("loops/shore.webm".to_string()))
            .autoplay(true)
            .looped(true)
            .key("clip");
        let notes = match self.notes {
            Some(buffer) => tree.code_view(buffer).read_only(true).key("notes"),
            None => tree
                .paragraph(vec![vec![Span::plain("Notes are not open yet.")]])
                .key("notes"),
        };
        let build = match self.build_pane {
            Some(pane) => tree.terminal_view(pane).rows(8).key("build-view"),
            None => tree
                .paragraph(vec![vec![Span::plain("No build pane.")]])
                .key("build-view"),
        };
        let top = tree
            .layout(Direction::Horizontal, [Constraint::Length(20), Constraint::Fill(1), Constraint::Length(24)])
            .child(logo)
            .child(scope)
            .child(clip);
        let body = tree
            .layout(Direction::Vertical, [Constraint::Length(12), Constraint::Fill(1), Constraint::Length(8)])
            .child(top)
            .child(notes)
            .child(build);
        tree.finish(body)
    }
}
```

## Worked example in TypeScript

The TypeScript version builds the same panel with the camelCase builders.

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import { createBuffer } from '@ilium/plugin-sdk/primitives/editor';
import { surfaceSize, writeSurface } from '@ilium/plugin-sdk/host/ui';
import { codeView, image, layout, paragraph, surface, terminalView, tree, video }
  from '@ilium/plugin-sdk/ui/build';
import { span, type Event, type Submission, type ViewTarget } from '@ilium/plugin-sdk/types';

const PANEL = 'media-demo';
const SCOPE_KEY = 'scope';
const SCOPE_COLUMNS = 40;
const SCOPE_ROWS = 6;

const state = {
  notes: undefined as bigint | undefined,
  buildPane: undefined as bigint | undefined,
  scroll: 0,
};

function panelView(): ViewTarget {
  return { tag: 'panel', val: PANEL };
}

function paintScope(): void {
  // Undefined until the panel has rendered once.
  const size = surfaceSize(panelView(), SCOPE_KEY);
  if (size === undefined) return;
  const [width, height] = size;
  const tones = new Uint8Array(width * height);
  for (let row = 0; row < height; row += 1) {
    for (let column = 0; column < width; column += 1) {
      tones[row * width + column] = (column + row + state.scroll) & 0xff;
    }
  }
  // gray8 is one byte per pixel; the rows start at the top of the surface.
  writeSurface(panelView(), SCOPE_KEY, 0, 'gray8', tones);
}

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate() {
    state.notes = createBuffer('# Plugin notes\n', 'markdown');
  },
  deactivate() {
    return undefined;
  },
  onEvents(batch: Event[]): Submission[] {
    for (const event of batch) {
      if (event.tag !== 'ui-action' || event.val.key !== SCOPE_KEY) continue;
      const { kind, value } = event.val;
      if (kind === 'scrolled' && value.tag === 'number') {
        // Wheel notches are negative for up and positive for down.
        state.scroll = (state.scroll + Math.trunc(value.val)) >>> 0;
        paintScope();
      }
    }
    return [];
  },
  render(_view: ViewTarget) {
    const logo = image({ tag: 'asset', val: 'icons/logo.png' }, { key: 'logo', fit: 'contain' });
    const scope = surface({
      key: SCOPE_KEY,
      format: 'gray8',
      minColumns: SCOPE_COLUMNS,
      minRows: SCOPE_ROWS,
      flags: { focusable: true },
    });
    const clip = video({ tag: 'asset', val: 'loops/shore.webm' }, {
      key: 'clip', autoplay: true, loop: true,
    });
    const notes = state.notes !== undefined
      ? codeView(state.notes, { key: 'notes', readOnly: true })
      : paragraph([[span('Notes are not open yet.')]], { key: 'notes' });
    const build = state.buildPane !== undefined
      ? terminalView(state.buildPane, { key: 'build-view', rows: 8 })
      : paragraph([[span('No build pane.')]], { key: 'build-view' });
    const top = layout(
      {
        direction: 'horizontal',
        constraints: [
          { tag: 'length', val: 20 }, { tag: 'fill', val: 1 }, { tag: 'length', val: 24 },
        ],
      },
      [logo, scope, clip],
    );
    const body = layout(
      {
        direction: 'vertical',
        constraints: [
          { tag: 'length', val: 12 }, { tag: 'fill', val: 1 }, { tag: 'length', val: 8 },
        ],
      },
      [top, notes, build],
    );
    return tree(body);
  },
});
```

## Edge cases and errors

| Situation | Result |
|---|---|
| A grant is missing or declined | The widget draws an empty frame with the text `not permitted`. |
| An image asset or file is missing, or its data is corrupt, unsupported or over 16 MiB | The widget draws an empty frame with the text `image not available`. The plugin log gets one warn line that names the source. |
| An `rgba` picture has a length other than `width x height x 4` | The same as a corrupt image. |
| A video source is missing, not decodable or over 16 MiB from bytes | An empty frame, and `invalid-argument` or `quota-exceeded` from the open call. |
| A `code-view` names a buffer that does not exist | Empty frame with the text `not found`. |
| A `terminal-view` names a pane that is closed or was never created | Empty frame with the text `not found`. |
| `interactive` without `terminal.raw-input` | The view stays read-only. |
| `ui.write-surface` names a key with no surface | `not-found`. |
| The format differs from the widget's `format` | `invalid-argument`, nothing written. |
| The data is not a whole number of rows, or is over 16 MiB | `invalid-argument`, nothing written. |
| `first-row` plus the rows written goes past `height` | `invalid-argument`, nothing written. The host never truncates a write silently. |
| `write-glyphs` without `glyph-plane`, or outside the surface | `unsupported` or `invalid-argument`. More than 131,072 cells in a frame gives `quota-exceeded`. |
| The area is smaller than the surface | The surface is clipped at the right and bottom. |
| The area is larger than the surface | The surface is drawn from the top-left corner, and the extra cells keep the cell background. |
| A surface has received no rows | It draws the cell background. |

The `not-found`, `invalid-argument`, `unsupported` and `quota-exceeded` codes are described in [Errors and limits](/pages/reference-errors-and-limits/). A missing grant or bad source never fails the render, so check the plugin log when a widget stays empty.

## What the user sees

- An image keeps its proportions in `contain` and `cover` modes.
- A surface shows the plugin's picture, recoloured by the user's shared look. The user cannot edit its pixels.
- A code view behaves like the native editor, with typing, selection and undo, unless `read-only` is true.
- A terminal view looks like the pane itself. Its keys go to the pane owner unless the view is interactive.
- A video plays muted in a loop, dithered with the user's settings.

API version: `ilium:plugin@0.1.0`.

Requirement ids for this page: [WID-028]. See the [requirements index](/pages/reference-requirements/).

## See also

- [Widget catalogue](/pages/widgets-index/)
- [UI model](/pages/ui-model/)
- [Input](/pages/widgets-input/)
- [Data display](/pages/widgets-data-display/)
- [Text](/pages/widgets-text/)
- [Editor primitive](/pages/primitives-editor/)
- [Terminal primitive](/pages/primitives-terminal/)
- [Accessibility and theming](/pages/accessibility-and-theming/)
- [Errors and limits](/pages/reference-errors-and-limits/)
