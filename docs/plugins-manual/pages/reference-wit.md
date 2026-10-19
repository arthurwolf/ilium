---
layout: page
title: WIT reference
permalink: /pages/reference-wit/
---

## WIT reference

This page is the canonical listing of the contract `ilium:plugin@0.1.0`. It holds every interface, record, variant, enum, flags type, resource and function that a plugin can import or export. A plugin author generates bindings from this listing. A host author implements it. A test author derives round-trip tests from it. Where a snippet on another page differs from this page, this page wins [ABI-001].

The contract is a family of WIT packages that share one version. The name `ilium:plugin@0.1.0` is the umbrella identity: it is the value of the manifest key `api` and of the `ilium:api-version` custom section of a plugin component (see [ABI evolution](/pages/reference-abi-evolution/)). There is no other contract version.

### What you need

- **A reader of WIT.** Know the five type forms (`record`, `variant`, `enum`, `flags`, `resource`), the type aliases (`type x = ...`) and the generic types `option`, `result`, `list` and `tuple`. The [component model book](https://component-model.bytecodealliance.org/design/wit.html) explains them.
- **An SDK, if you only write plugins.** The [Rust SDK](/pages/rust-sdk/) and the [TypeScript SDK](/pages/typescript-sdk/) generate the bindings, so you read this page only for names, limits and error codes.
- **The tool limit.** The listing was checked by structure only: every name used is declared once, and every declaration appears once. No `wasm-tools` parse is part of this repository yet. A conforming parse is a requirement of the host build, see [ABI-001] in [Requirements: WIT and ABI](/pages/reference-requirements-wit-abi/).

## 1. How to read this page

- **One package family.** The listing has seven packages: `ilium:types`, `ilium:host`, `ilium:primitives`, `ilium:exports`, `ilium:server`, `ilium:client` and `ilium:anim`. All are at `@0.1.0`.
- **Doc comments carry the rules.** A `///` comment gives units, limits, error codes, deadlines and the grant. A trailing `//` comment is an aside.
- **Grants.** `Grant:` names the capability that a function needs. `Grant: none` means the function is always available to the side that imports it. A call without its grant fails as described in [Host conformance matrix](/pages/reference-host-conformance/).
- **Async pairs.** A function that returns `result<request-id, plugin-error>` is a slow call. Its answer arrives as the event `request-completed`, then the plugin calls the `take-*` function named in the comment. See [Requests and outcomes](/pages/requests-and-outcomes/).
- **Stability.** `@since(version = 0.1.0)` marks stable items. `@unstable(feature = x)` marks items that need the manifest opt-in. Four features exist: `ui-calendar`, `plugin-services`, `gpu-kernels` and `websocket`. In this listing a stable item carries no marker, because every item of 0.1.0 is `@since(version = 0.1.0)`.
- **Names.** WIT names are kebab-case. Rust uses `snake_case` and `PascalCase`. TypeScript uses `camelCase`. Section 10 gives the mapping.
- **Excerpts.** Other pages show parts of this listing as excerpts. An excerpt starts with the comment `// Excerpt of ilium:<package>/<interface> in reference-wit` and has to match this page [ABI-005]. The owner page of an interface holds the full listing of that interface and its explanation. The rule is in [Conventions for authors](/pages/reference-requirements-wit-abi/#how-the-rows-are-tested).

## 2. Conventions

These rules apply to every interface below. Limit tables on other pages link here instead of repeating them [ABI-016].

| Topic | Rule |
|---|---|
| Identifiers | `node-id`, `request-id`, `client-id`, `buffer-id`, `timer-id`, `watch-id`, `lock-id`, `stream-id`, `child-id`, `grab-id` and `index-id` are unsigned integers. Ids are opaque and never reused inside one server run. `plugin-id` is a string such as `acme.git-status`: 3 to 64 characters from `[a-z0-9.-]`. |
| Correlation numbers | `submission.correlation` is a `u64` chosen by the plugin. Values below 2^63 belong to the plugin. Values from 2^63 up belong to the SDK, which uses them for the submissions it makes for you. |
| Strings | UTF-8 only. A host rejects an invalid byte sequence at the ABI boundary with a trap. See [Runtime model](/pages/reference-runtime-model/). |
| String lengths | A limit in "characters" counts Unicode scalar values. A limit in "bytes" counts UTF-8 bytes. Terminal widths use `ui.display-width`, which counts cells. Grapheme limits (glyph fields) are named as such. |
| Colours | `u32` as `0x00RRGGBB`. The top byte MUST be zero. A non-zero top byte is `invalid-argument`. The `color` variant adds theme roles and 256-colour indexes for styled text. |
| Time | `epoch-ms` is a `u64` count of milliseconds since the Unix epoch, UTC. A duration is a `u32` or `u64` count of milliseconds and its field name ends in `-ms` or `-millis`. Percent is `f32` from 0 to 100. A ratio is `f64` from 0 to 1. Per-mille is a `u16` from 0 to 1000. |
| JSON | A field whose name ends in `-json` is UTF-8 JSON text (RFC 8259). See the JSON rule in [Runtime model](/pages/reference-runtime-model/#8-json-in-strings). |
| Paths | `fs-path` is a scope name plus a relative path with `/` separators. Parent segments (`..`) and absolute paths are `invalid-argument`. |
| Positions | Lines and columns are 0-based. A text column counts Unicode scalar values. A cell column counts terminal cells. |
| Lists | A list that the host returns is never `null`. An empty list is a valid value. Where a function states a maximum, a longer input is `invalid-argument` unless the page says it traps. |
| Case order | The case order of every `variant`, `enum` and `flags` type is part of the canonical ABI. A case is only ever appended [ABI-019]. |
| Empty types | No record, flags, enum or variant is empty, because the component model forbids it. A variant case without data has no payload. |

### Length and size limits that apply everywhere

| Field class | Limit | Unit |
|---|---|---|
| Command id | 1 to 96 | characters, from `[a-z0-9._:/-]` |
| Key of a widget | 1 to 64 | characters |
| Dedup key | 1 to 128 | characters from `[A-Za-z0-9._:/-]` |
| Toast title | 60 | characters |
| Toast body | 200 | characters |
| Notification title | 80 | characters |
| `mark-attention` reason | 80 | characters |
| Message channel | 1 to 64 | characters from `[a-z0-9.-]` |
| Message payload | 1,048,576 | bytes |
| Clipboard text | 1,048,576 | bytes |
| `arguments-json` of a command | 65,536 | bytes |
| Payload and reply of a service call | 1,048,576 | bytes |
| Journal payload | 4,096 | bytes |
| Restore bytes from `deactivate` | 1,048,576 | bytes |

## 3. Package and interface layout

The SDK ships these files in its `wit/` folder. The frozen copy of the same layout lives in `wit/since_v0_1/`.

| Package | Interface | What it holds | Section |
|---|---|---|---|
| `ilium:types` | `ids` | id aliases | 4.1 |
| `ilium:types` | `errors` | `error-code`, `plugin-error` | 4.2 |
| `ilium:types` | `text` | colours, styles, spans, rich text, tooltips | 4.3 |
| `ilium:types` | `paths` | `fs-path` | 4.4 |
| `ilium:types` | `editing` | text positions, buffers, viewport | 4.5 |
| `ilium:types` | `layout` | constraints and flex | 4.6 |
| `ilium:types` | `net` | HTTP header | 4.7a |
| `ilium:types` | `geo` | points and places | 4.7 |
| `ilium:types` | `tree` | nodes, panes, agents | 4.8 |
| `ilium:types` | `ui` | widgets, ui tree, actions, state, theme | 4.9 |
| `ilium:types` | `requests` | request refs and outcomes | 4.10 |
| `ilium:types` | `intents` | the 42 intents and their specs | 4.11 |
| `ilium:types` | `events` | the 56 events and their payloads | 4.12 |
| `ilium:types` | `lifecycle` | activation and deactivation | 4.13 |
| `ilium:host` | 39 interfaces | everything a plugin imports | 5 |
| `ilium:primitives` | `terminal`, `agent`, `editor`, `basic` | pane primitives | 6 |
| `ilium:anim` | `surface`, `fonts`, `renderer-types` | background scenes | 7 |
| `ilium:exports` | 16 interfaces | what a plugin implements | 8 |
| `ilium:server`, `ilium:client`, `ilium:anim` | `base` worlds and `with-*` fragments | what a plugin links | 9 |

## 4. Package ilium:types

All items of this package are shared data. A plugin never imports `ilium:types` as a whole. It imports the host interfaces, and the types come with them.

### 4.1 ids

```wit
package ilium:types@0.1.0;

interface ids {
  /// A node of the session tree: a container, a pane or a folder. Never reused in one server run.
  type node-id = u64;
  /// The id of a slow host call. Unique per plugin side for the whole server run, starting at 1 at
  /// server start, and never reused, also across instance restarts.
  type request-id = u64;
  /// Reverse-DNS style plugin id, 3 to 64 characters from [a-z0-9.-].
  type plugin-id = string;
  /// One attached client of the session.
  type client-id = u64;
  /// One editor buffer.
  type buffer-id = u64;
  /// One timer, volatile or durable.
  type timer-id = u64;
  /// A file watch taken through fs.watch.
  type watch-id = u64;
  /// An advisory file lock taken through fs.lock.
  type lock-id = u64;
  /// An open HTTP or websocket stream.
  type stream-id = u64;
  /// A vector index created through vector.create-index.
  type index-id = u32;
  /// A child process spawned through process.spawn.
  type child-id = u64;
  /// A key grab taken through keys.grab.
  type grab-id = u64;
  /// Milliseconds since the Unix epoch, UTC.
  type epoch-ms = u64;
}
```

### 4.2 errors

```wit
interface errors {
  /// The order is part of the ABI. Meaning of each code: see Errors and limits.
  enum error-code {
    not-permitted, not-found, invalid-argument, conflict, quota-exceeded, rate-limited,
    busy, timed-out, cancelled, unsupported, pane-not-ready, revoked, io, network, internal,
  }
  record plugin-error {
    code: error-code,
    /// Plain text for logs. At most 256 characters. Never parsed by the SDK.
    message: string,
    /// Present with rate-limited and busy: wait at least this long before a retry.
    retry-after-ms: option<u32>,
  }
}
```

### 4.3 text

```wit
interface text {
  enum color-role { foreground, muted, accent, success, warning, danger, info, selection, border }
  variant color { role(color-role), rgb(u32), indexed(u8) }
  flags modifiers { bold, dim, italic, underlined, slow-blink, rapid-blink, reversed, hidden, crossed-out }
  /// An empty style (no colours, no modifiers) inherits the theme.
  record style { fg: option<color>, bg: option<color>, add: modifiers, remove: modifiers }
  record span { text: string, style: style }
  /// One line of styled text.
  type styled-text = list<span>;
  type styled-lines = list<styled-text>;

  /// Where a clickable span goes.
  variant span-target {
    none,
    /// An own command or one covered by commands.invoke. Arrives as command-invoked with origin text.
    command(string),
    /// Arrives as a ui-action of kind activated with value text(<string>) on the widget key.
    action(string),
    /// A URL, opened only through the open-external rules.
    link(string),
  }
  record rich-span { text: string, style: style, target: span-target, tooltip: option<string> }
  type rich-text = list<rich-span>;
  type rich-lines = list<rich-text>;

  /// Hover help. At most 200 characters in all. Default delay 500 ms, allowed 0 to 2000.
  record tooltip { lines: styled-lines, delay-ms: option<u16> }

  /// A preview in a choice dialog.
  variant preview {
    /// One grapheme, drawn with icon widths.
    glyph(string),
    /// 1 to 16 colours, drawn as a strip of half blocks.
    swatch(list<u32>),
    /// 1 to 4 lines of at most 8 cells, drawn in the muted colour.
    pattern(list<string>),
  }

  enum tri-state { off, on, mixed }
  /// The one tone enum. Toast, notice, badge, status segment and meter use it.
  enum tone { info, success, warning, danger, muted }
}
```

### 4.4 paths

```wit
interface paths {
  /// `scope` is a granted scope name such as project, data or agent-sessions:codex.
  /// `path` is relative to the scope root and uses '/' separators.
  record fs-path { scope: string, path: string }
}
```

### 4.5 editing

```wit
interface editing {
  use ilium:types/ids@0.1.0.{buffer-id, node-id};
  use ilium:types/paths@0.1.0.{fs-path};
  /// Both fields are 0-based. `column` counts Unicode scalar values.
  record text-position { line: u32, column: u32 }
  record text-range { start: text-position, end: text-position }
  record text-edit { range: text-range, replacement: string }
  record buffer-ref {
    buffer: buffer-id, pane: option<node-id>, path: option<fs-path>, language: option<string>,
    revision: u64, line-count: u32, dirty: bool,
  }
  record editor-viewport { buffer: buffer-id, pane: node-id, first-line: u32, last-line: u32, columns: u16, rows: u16 }
}
```

### 4.6 layout

```wit
interface layout {
  enum direction { horizontal, vertical }
  variant constraint { length(u16), min(u16), max(u16), percentage(u16), ratio(tuple<u32, u32>), fill(u16) }
  enum flex { legacy, start, end, center, space-between, space-around, space-evenly }
  variant spacing { space(u16), overlap(u16) }
  record margin { horizontal: u16, vertical: u16 }
  enum alignment { left, center, right }
  /// A size in cells.
  record area { columns: u16, rows: u16 }
}
```

### 4.7 geo

```wit
interface geo {
  /// WGS 84 degrees. Latitude -90 to 90 and longitude -180 to 180, else invalid-argument.
  record geo-point { lat: f64, lon: f64 }
  record geo-place { point: geo-point, label: string, country-code: option<string>, kind: option<string> }
}
```

### 4.7a net

```wit
interface net {
  record header { name: string, value: string }
}
```

### 4.8 tree

```wit
interface tree {
  use ilium:types/ids@0.1.0.{node-id, plugin-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  enum node-class { container, pane, folder }
  enum agent-activity { none, working, idle, awaiting-approval, done, menu }
  record agent-view { signature: string, activity: agent-activity, objective: option<string>, goal: option<string> }
  record node-view {
    id: node-id, parent: option<node-id>, class: node-class,
    /// "ilium.terminal", "ilium.editor", "ilium.board" or "<plugin-id>.<kind>".
    content-kind: option<string>,
    title: string, cwd: option<string>, agent: option<agent-view>, locked: bool, revision: u64,
  }
  record node-query { content-kind: option<string>, signature: option<string>, under: option<node-id> }
  enum primitive-kind { terminal, editor, basic }
  record pane-info {
    node: node-id, content-kind: string, primitive: primitive-kind,
    columns: u16, rows: u16, focused: bool, title: string, is-restored: bool,
  }
  /// Where a node goes relative to a target. One definition for the whole contract.
  enum placement { before, after, inside-first, inside-last }
  enum split-direction { horizontal, vertical }
  enum pane-lifecycle-state { running, frozen, restarting, exited }
  enum signal { interrupt, terminate, kill, hangup }
  /// The model an agent runs. source is statusline, transcript or signature.
  record agent-model { model: string, provider: option<string>, context-window: option<u32>, source: string }
  /// Where a title came from.
  enum title-source { plugin, inferred }
  enum title-origin { osc, manual, inferred }
}
```

The enum `split-direction` has the same two cases as `direction` in `layout`. They stay two types: `direction` lays out widgets, `split-direction` splits panes. The intent `create-split` uses `split-direction`.


### 4.9 ui

The `widget` variant has 51 cases. The order is the order of the families in [Widgets index](/pages/widgets-index/): layout (10), text (5), collections (6), data (10), input (11), status (4), media (5). A container holds child indexes in `ui-node.children`, never nested widgets, so no type is recursive.

```wit
interface ui {
  use ilium:types/ids@0.1.0.{node-id, plugin-id, buffer-id};
  use ilium:types/text@0.1.0.{color, style, styled-text, styled-lines, rich-text, rich-lines, tooltip, tri-state, tone, preview};
  use ilium:types/layout@0.1.0.{direction, constraint, flex, spacing, margin, alignment, area};
  use ilium:types/paths@0.1.0.{fs-path};
  use ilium:types/geo@0.1.0.{geo-point};

  // ---- layout and containers
  record layout-spec { direction: direction, constraints: list<constraint>, flex: flex, spacing: spacing, margin: margin }
  flags borders { top, right, bottom, left }
  enum border-type { plain, rounded, double, thick, quadrant-inside, quadrant-outside }
  enum title-position { top, bottom }
  record block-title { text: styled-text, position: title-position, alignment: alignment }
  record padding { left: u16, right: u16, top: u16, bottom: u16 }
  record block-spec {
    borders: borders, border-type: border-type, titles: list<block-title>, padding: padding,
    style: style, border-style: style,
  }
  record tabs-spec {
    titles: list<styled-text>, divider: string, padding-left: u16, padding-right: u16,
    highlight-style: style,
  }
  /// id is the key of the popup node. Size: see the popup rules on the layout and containers page.
  record popup-spec { id: string, title: option<styled-text>, modal: bool, width: constraint, height: constraint }
  enum scrollbar-orientation { vertical-right, vertical-left, horizontal-bottom, horizontal-top }
  record scrollbar-spec { orientation: scrollbar-orientation, target-key: string }
  /// position is per mille, 50 to 950.
  record split-spec { direction: direction, position: u16, min-first: u16, min-second: u16, resizable: bool }
  record separator-spec { direction: direction, label: option<styled-text>, style: style }
  enum dialog-kind { plain, confirm, danger-confirm }
  enum button-role { primary, secondary, danger }
  record dialog-button { id: string, label: string, role: button-role, is-cancel: bool }
  record dialog-spec {
    title: styled-text, kind: dialog-kind, buttons: list<dialog-button>,
    width: constraint, height: constraint, message: option<styled-lines>,
  }
  record scroll-view-spec { content-rows: u32, show-scrollbar: bool, follow-tail: bool }

  // ---- text
  record paragraph-spec {
    lines: styled-lines, rich: option<rich-lines>, wrap: option<bool>, alignment: alignment,
    style: style, scrollable: bool,
  }
  record markdown-spec { source: string, link-command: option<string>, task-click: bool, images: bool, base: option<fs-path> }
  record leader-spec { label: styled-text, value: styled-text }
  record key-hint { keys: string, label: string, command: option<string> }
  enum diff-line-kind { added, removed, context }
  record diff-line { kind: diff-line-kind, old-line: option<u32>, new-line: option<u32>, text: string }
  record diff-hunk { header: string, lines: list<diff-line> }
  enum diff-layout { unified, split }
  record diff-view-spec { hunks: list<diff-hunk>, layout: diff-layout, language: option<string>, word-diff: bool }

  // ---- collections
  enum list-direction { top-to-bottom, bottom-to-top }
  enum highlight-spacing { always, when-selected, never }
  record list-item {
    id: option<string>, lines: styled-lines, style: style, icon: option<string>, trailing: option<styled-text>,
    tooltip: option<tooltip>, disabled-reason: option<string>,
  }
  record list-spec {
    items: list<list-item>, highlight-symbol: string, highlight-style: style, highlight-spacing: highlight-spacing,
    repeat-highlight-symbol: bool, direction: list-direction, filter: bool, filter-placeholder: option<string>,
    multi-select: bool, follow-tail: bool, empty: option<styled-lines>, item-tooltip: bool,
  }
  record table-column {
    id: string, title: styled-text, width: constraint, min-width: u16, priority: u8,
    align: alignment, sortable: bool,
  }
  record table-row { id: option<string>, cells: list<styled-lines>, height: u16, style: style }
  record table-spec {
    columns: list<table-column>, rows: list<table-row>, sort: option<tuple<string, bool>>,
    multi-select: bool, follow-tail: bool, column-spacing: u16, highlight-symbol: string,
    highlight-spacing: highlight-spacing,
  }
  record tree-item {
    id: string, label: styled-text, depth: u16, expandable: bool, expanded: bool, icon: option<string>,
    trailing: option<styled-text>, badge: option<string>, tooltip: option<tooltip>, draggable: bool, drop-target: bool,
  }
  record tree-list-spec { items: list<tree-item>, multi-select: bool }
  record menu-item { id: string, label: string, icon: option<string>, enabled: bool, shortcut: option<string>, separator-before: bool }
  record menu-spec { entries: list<menu-entry>, items: list<menu-item> }
  record menu-entry {
    id: string, label: string, icon: option<string>, shortcut: option<string>,
    /// Id of a submenu entry. Depth is at most 2.
    parent: option<string>,
    is-submenu: bool, separator-before: bool,
    checked: option<tri-state>,
    /// At most 40 characters.
    disabled-reason: option<string>,
    tooltip: option<tooltip>, danger: bool,
  }
  record check-item { id: string, label: styled-text, state: tri-state, disabled-reason: option<string>, children: list<check-item> }
  record check-list-spec { items: list<check-item>, all-toggle: bool, max-height: option<u16> }
  enum item-format { glob, url, host, path, regex, chord }
  record list-field { id: string, title: string, kind: string, width: u16 }
  record list-editor-spec {
    items: list<string>, item-format: option<item-format>, object-fields: list<list-field>,
    reorderable: bool, max-items: u32, item-states: list<row-state>, placeholder: option<string>,
  }
  enum row-state-kind { idle, loading, pending, error }
  /// message is at most 80 characters.
  record row-state { kind: row-state-kind, message: option<string> }

  // ---- data display
  record gauge-spec { ratio: f64, label: option<styled-text>, use-unicode: bool, gauge-style: style }
  enum line-set { normal, thick, double }
  record line-gauge-spec { ratio: f64, label: option<styled-text>, filled-style: style, unfilled-style: style, line-set: line-set }
  enum render-direction { left-to-right, right-to-left }
  record sparkline-spec { data: list<option<u64>>, max: option<u64>, direction: render-direction, style: style }
  record bar-item { value: u64, label: option<string>, text-value: option<string>, style: style }
  record bar-group { label: option<string>, bars: list<bar-item> }
  enum bar-direction { vertical, horizontal }
  record bar-chart-spec { groups: list<bar-group>, bar-width: u16, bar-gap: u16, group-gap: u16, direction: bar-direction, max: option<u64> }
  enum marker { dot, block, bar, braille, half-block, quadrant, sextant, octant }
  enum graph-type { scatter, line, bar }
  record dataset { name: string, marker: marker, graph-type: graph-type, points: list<tuple<f64, f64>>, style: style }
  record axis { title: option<styled-text>, bounds: tuple<f64, f64>, labels: list<styled-text> }
  enum legend-position { top, top-right, top-left, left, right, bottom, bottom-right, bottom-left }
  record chart-spec { datasets: list<dataset>, x-axis: axis, y-axis: axis, legend-position: option<legend-position> }
  record canvas-line { x1: f64, y1: f64, x2: f64, y2: f64, color: color }
  record canvas-rectangle { x: f64, y: f64, width: f64, height: f64, color: color }
  record canvas-circle { x: f64, y: f64, radius: f64, color: color }
  record canvas-points { coords: list<tuple<f64, f64>>, color: color }
  record canvas-label { x: f64, y: f64, text: styled-text }
  enum map-resolution { low, high }
  record canvas-map { resolution: map-resolution, color: color }
  variant shape {
    line(canvas-line), rectangle(canvas-rectangle), circle(canvas-circle),
    points(canvas-points), label(canvas-label), world-map(canvas-map),
  }
  record canvas-spec { x-bounds: tuple<f64, f64>, y-bounds: tuple<f64, f64>, marker: marker, shapes: list<shape> }
  record marked-day { day: u8, style: style }
  /// Gate ui-calendar.
  record calendar-spec { year: u32, month: u8, marked: list<marked-day>, show-month-header: bool, show-weekdays: bool }
  enum progress-state { waiting, running, done, error }
  record progress-row-spec {
    state: progress-state, title: styled-text, percent: option<f32>, message: option<string>, details: option<styled-lines>,
  }
  record meter-segment { label: string, value: f64, tone: tone }
  enum orientation { horizontal, vertical }
  /// At most 8 segments.
  record meter-spec { segments: list<meter-segment>, max: f64, label: option<styled-text>, show-values: bool, orientation: orientation }
  record minimap-mark { line: u32, color: color, lane: u8 }
  record minimap-spec { density: list<u8>, marks: list<minimap-mark>, viewport: tuple<u32, u32>, total-lines: u32 }

  // ---- input
  record button-spec { label: string, icon: option<string>, enabled: bool, role: button-role }
  enum toggle-style { switch, checkbox, icon }
  record toggle-spec { label: string, value: bool, style: toggle-style, tri-state: bool, icon-on: option<string>, icon-off: option<string> }
  enum choice-style { stepper, list, dialog }
  enum view-mode { list, grid }
  enum disabled-marker { reason, cross }
  record catalog-item {
    id: string, title: string,
    /// At most 120 characters.
    description: option<string>,
    preview: option<preview>,
    /// Heading in the choice dialog, at most 32 characters.
    group: option<string>,
    /// At most 8, matched by the filter.
    keywords: list<string>,
    /// At most 40 characters.
    disabled-reason: option<string>,
  }
  record choice-spec {
    label: string, options: list<string>, selected: u32, style: choice-style, items: list<catalog-item>,
    source: option<string>, selected-id: option<string>, filter: bool, view-modes: list<view-mode>,
    disabled-marker: disabled-marker, allow-custom: bool,
  }
  enum number-style { stepper, slider }
  record number-spec {
    label: string, value: f64, minimum: f64, maximum: f64, step: f64, unit: option<string>, style: number-style,
    steps: list<f64>, special-values: list<tuple<f64, string>>, scale: u32, debounce-ms: u32,
  }
  record text-input-spec {
    label: string, initial: string, placeholder: option<string>, secret: bool, reveal-last: u8, reveal: bool,
    max-length: u32, submit-on-enter: bool, debounce-ms: u32, validation: option<string>, suggestions: option<string>,
  }
  /// max-bytes 0 means no limit.
  record text-area-spec {
    initial: string, placeholder: option<string>, wrap: bool, line-numbers: bool, max-bytes: u32,
    debounce-ms: u32, tab-moves-focus: bool,
  }
  record schema-form-spec { plugin: plugin-id, section: string }
  record key-capture-spec { chord: option<string>, allow-modifier-only: bool, reserved-check: bool }
  record color-picker-spec { value: string, palette: option<string>, allow-theme-roles: bool }
  enum path-kind { open-file, open-directory, save-file }
  record path-field-spec { kind: path-kind, value: option<fs-path>, display: string, extensions: list<string>, editable: bool }
  record location-picker-spec {
    value: option<geo-point>, label: option<string>, search: bool, allow-current: bool, zoom: bool, bind: option<string>,
  }

  // ---- status and feedback
  record badge-spec { label: option<string>, count: option<u32>, dot: bool, tone: tone }
  variant spinner-frames { braille, blocks, moons, quarters, custom(list<string>) }
  record spinner-spec { frames: spinner-frames, interval-ms: u16, label: option<styled-text> }
  record notice-action { label: string, command: string }
  record notice-spec {
    tone: tone, title: option<string>, text: rich-lines, actions: list<notice-action>, dismissible: bool,
  }
  record icon-spec { name: string, tone: option<tone> }

  // ---- media and embedded views
  enum pixel-format { mask8, mono1, gray8, rgb8, rgba8 }
  record rgba-image { width: u32, height: u32, rgba: list<u8> }
  record encoded-image { bytes: list<u8>, mime: string }
  variant image-source { asset(string), file(fs-path), decoded(rgba-image), encoded(encoded-image) }
  enum media-fit { contain, cover, fill }
  record image-spec { source: image-source, fit: media-fit }
  record surface-spec { format: pixel-format, min-columns: u16, min-rows: u16, glyph-plane: bool, owner-ids: bool }
  record code-view-spec { buffer: buffer-id, read-only: bool, line-numbers: bool, wrap: bool, minimap: bool }
  record terminal-view-spec { pane: node-id, rows: option<u16>, interactive: bool, scrollback: bool, highlights: option<string> }
  record video-range { path: fs-path, start-millis: u64, end-millis: option<u64> }
  variant video-source { asset(string), file(fs-path), range(video-range) }
  record video-spec { source: video-source, autoplay: bool, looped: bool, muted: bool, controls: bool, fit: media-fit }

  /// The 51 widgets. The order is the ABI.
  variant widget {
    // layout (10)
    layout(layout-spec), block(block-spec), tabs(tabs-spec), popup(popup-spec), clear,
    scrollbar(scrollbar-spec), split(split-spec), separator(separator-spec), dialog(dialog-spec),
    scroll-view(scroll-view-spec),
    // text (5)
    paragraph(paragraph-spec), markdown(markdown-spec), leader-row(leader-spec), key-hints(list<key-hint>),
    diff-view(diff-view-spec),
    // collections (6)
    %list(list-spec), table(table-spec), tree-list(tree-list-spec), menu(menu-spec),
    check-list(check-list-spec), list-editor(list-editor-spec),
    // data (10)
    gauge(gauge-spec), line-gauge(line-gauge-spec), sparkline(sparkline-spec), bar-chart(bar-chart-spec),
    chart(chart-spec), canvas(canvas-spec),
    // @unstable(feature = ui-calendar)
    calendar(calendar-spec),
    progress-row(progress-row-spec), meter(meter-spec), minimap(minimap-spec),
    // input (11)
    button(button-spec), toggle(toggle-spec), choice(choice-spec), number(number-spec),
    text-input(text-input-spec), text-area(text-area-spec), schema-form(schema-form-spec),
    key-capture(key-capture-spec), color-picker(color-picker-spec), path-field(path-field-spec),
    location-picker(location-picker-spec),
    // status (4)
    badge(badge-spec), spinner(spinner-spec), notice(notice-spec), icon(icon-spec),
    // media (5)
    image(image-spec), surface(surface-spec), code-view(code-view-spec), terminal-view(terminal-view-spec),
    video(video-spec),
  }

  /// The order is the ABI. The last two flags switch on hover and pointer actions.
  flags node-flags { focusable, draggable, drop-target, hidden, hover-events, pointer-events }
  record ui-node {
    /// Widget key, 1 to 64 characters, unique across the whole view, not only among siblings.
    /// A duplicate traps with ui.duplicate-key. Required for every widget that sends actions.
    key: option<string>,
    widget: widget,
    /// Indexes into ui-tree.nodes. Every index is greater than the index of its parent and less
    /// than nodes.len(). Every node except the root appears exactly once in exactly one list.
    children: list<u32>,
    %flags: node-flags,
    /// Shown after the delay on hover, or on focus with F1.
    tooltip: option<tooltip>,
    /// Focus order: 1 and above first, ascending; then none and 0 in pre-order. Negative: click and
    /// set-focus only, skipped by Tab.
    tab-index: option<s16>,
    /// The node is drawn muted, ignores input, stays in the focus order and the reason shows as its tooltip.
    disabled-reason: option<string>,
  }
  /// root MUST be 0. At most 4,096 nodes and 1 MiB. A tree deeper than 32 levels (root = 1) traps
  /// ui.too-deep. Any invalid tree traps with a ui.* code; views.render has no result type.
  record ui-tree { version: u32, nodes: list<ui-node>, root: u32 }

  // ---- views, state and actions
  variant view-target { panel(string), pane(node-id), popup(string), settings-page(string), onboarding-step(string) }

  variant widget-state {
    none, selected(option<u32>), offset(u32), tab(u32), text(string), expanded(list<string>), checked(bool),
    /// Check-list items, or a tri-state toggle under key "".
    tri(list<tuple<string, tri-state>>),
    /// location-picker.
    geo(option<geo-point>),
    /// number, slider.
    number(f64),
    /// Split divider position in per mille.
    split(u16),
    /// Multi-selection in the list, table and tree-list.
    ids(list<string>),
  }

  variant popup-anchor {
    centered(tuple<constraint, constraint>), at-cell(tuple<u16, u16>), below-key(string),
    at-key(key-anchor),
    /// Next to a tree row (tree toolbar popovers, row actions).
    at-tree-node(node-id),
    /// At the last pointer position (context menus in own views).
    at-pointer,
  }
  enum anchor-side { below, above, right, left }
  record key-anchor { key: string, side: anchor-side }
  /// Overrides the modal and close-on-escape fields of the spec.
  record popup-options { modal: bool, pinnable: bool, dismiss-on-leave: bool, close-on-escape: bool }

  record theme-tokens {
    foreground: u32, background: u32, accent: u32, muted: u32, success: u32, warning: u32, danger: u32,
    is-dark: bool,
    /// Derived convenience field: always equal to (icon-mode == stable) in every delivery.
    stable-glyphs: bool,
  }
  enum density { compact, normal, comfortable }
  /// Widgets and the host follow icon-mode.
  enum icon-mode { normal, stable }
  record theme-extras { reduced-motion: bool, density: density, info: u32, selection: u32, border: u32, icon-mode: icon-mode }

  /// 17 cases. The first 11 are the base set, the last 6 were appended.
  enum action-kind {
    activated, changed, submitted, selected, toggled, scrolled, dropped,
    context-requested, closed, focus-gained, focus-lost,
    hover-enter, hover-leave, pointer, drag-over, resized, filter-changed,
  }

  enum pointer-kind { down, up, move, drag, wheel-up, wheel-down, wheel-left, wheel-right }
  enum pointer-button { none, left, middle, right }
  flags pointer-modifiers { shift, control, alt }
  record view-pointer {
    kind: pointer-kind, button: pointer-button, modifiers: pointer-modifiers,
    /// Cell inside the widget area, 0-based.
    column: u16, row: u16,
    /// Position inside the cell, 0 to 255 from left or top. 128 when the terminal reports cells only.
    sub-x: u8, sub-y: u8,
    /// 1, 2 or 3 for down events, 0 otherwise.
    clicks: u8,
  }
  enum drop-accept { none, before, after, inside }
  record drop-value { source-key: string, target-key: string, index: u32 }
  record drag-over-value { source-key: string, target-key: string, index: u32, accept: drop-accept }
  record cell-position { column: u16, row: u16 }

  /// 12 cases. The first 6 are the base set, the last 6 were appended or added later.
  variant action-value {
    none, text(string), index(u32), number(f64), flag(bool),
    drop(drop-value),
    cell(cell-position),
    pointer(view-pointer),
    drag-over(drag-over-value),
    /// Check-list item id, or "" for a tri-state toggle.
    item-state(tuple<string, tri-state>),
    /// location-picker.
    geo(geo-point),
    /// Multi-selection in the list, table and tree-list.
    ids(list<string>),
  }
  /// In 0.1.0 `action-id` is always a copy of `key`. value is none for button activated, menu closed,
  /// key-capture closed and the hover and focus kinds.
  record ui-action { view: view-target, key: string, action-id: string, kind: action-kind, value: option<action-value> }

}
```

The `action-value` count is 12: `none`, `text`, `index`, `number`, `flag`, `drop`, `cell`, `pointer`, `drag-over`, `item-state`, `geo` and `ids`. The event `ui-action` carries `value` as an option, so a widget with no value sends `none` in the option, and `action-value.none` is kept for SDK builders that always fill the field.

The enum `orientation` here belongs to `meter`. It has the cases of `direction`, but it is a separate type because it names a bar, not a split.


### 4.10 requests

```wit
interface requests {
  use ilium:types/ids@0.1.0.{node-id, request-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  /// Which request a result belongs to. submission(n) is a submission with correlation n.
  /// call(id) is a slow host call with that request-id.
  variant request-ref { submission(u64), call(request-id) }
  /// The order is the ABI.
  variant outcome {
    ok,
    ok-node(node-id),
    duplicate(request-ref),
    refused(plugin-error),
    failed(plugin-error),
    cancelled,
    /// send-input on an agent pane was applied. The value is the new prompt epoch.
    ok-epoch(u64),
  }
  record request-result { request: request-ref, outcome: outcome }
}
```

### 4.11 intents

The `intent` variant has 42 cases. The first 19 are the base set. The last 23 were appended in the order shown. A submission carries one intent.

```wit
interface intents {
  use ilium:types/ids@0.1.0.{node-id, plugin-id, buffer-id, client-id};
  use ilium:types/text@0.1.0.{rich-text, tooltip, styled-text, tone};
  use ilium:types/paths@0.1.0.{fs-path};
  use ilium:types/editing@0.1.0.{text-edit};
  use ilium:types/tree@0.1.0.{placement, split-direction, signal, title-source, agent-activity};
  use ilium:types/ui@0.1.0.{view-target, pointer-kind, pointer-button, pointer-modifiers};

  record submission { intent: intent, correlation: u64, dedup-key: option<string> }

  // ---- tree
  record rename-node-spec { node: node-id, title: string, source: title-source, expected-revision: option<u64> }
  record move-node-spec { node: node-id, target: node-id, placement: placement }
  record set-node-locked-spec { node: node-id, locked: bool }

  // ---- panes
  /// parent none means the root of the tree.
  record create-pane-spec {
    parent: option<node-id>, title: string, content-kind: string, cwd: option<string>,
    command: option<list<string>>, initial-input: option<string>,
  }
  record signal-pane-spec { pane: node-id, signal: signal }
  enum delivery { typed, submitted, pasted }
  record input-stage { text: string, delay-ms: u16, delivery: delivery }
  record terminal-mouse {
    kind: pointer-kind, button: pointer-button, column: u16, row: u16, modifiers: pointer-modifiers,
  }
  record input-precondition { activity: option<agent-activity>, screen-revision: option<u64>, foreground: option<string> }
  record send-input-spec {
    pane: node-id,
    /// Ignored when stages is not empty.
    text: string,
    delivery: delivery,
    /// refused(conflict) when stale.
    prompt-epoch: option<u64>,
    /// At most 16 stages, total delay at most 10 s.
    stages: list<input-stage>,
    /// At most 64 KiB. Needs terminal.raw-input.
    raw: option<list<u8>>,
    /// Needs terminal.raw-input.
    mouse: option<list<terminal-mouse>>,
    if-current: option<input-precondition>,
    /// At most 2 s. Needs terminal.input-lock.
    lock-user-input: bool,
    /// Write a journal entry before the PTY write.
    journal: bool,
  }
  record agent-action-spec { pane: node-id, action-id: string }

  // ---- decorations and virtual rows
  record decoration-key { node: node-id, slot: string }
  enum decoration-visibility { always, on-hover }
  record set-decoration-spec {
    node: node-id, slot: string, content: styled-text, icon: option<string>, sort-key: option<s64>, persist: bool,
    spans: option<rich-text>, tooltip: option<tooltip>,
    /// 2 to 16 frames.
    frames: list<styled-text>,
    /// At least 90.
    frame-ms: u16,
    trailing: bool, visibility: decoration-visibility, tone: option<tone>,
  }
  record virtual-row {
    id: string, label: rich-text, icon: option<string>, command: option<string>,
    parent: option<string>, expandable: bool, accepts-drop: bool, tooltip: option<tooltip>, trailing: option<styled-text>,
  }
  record set-virtual-rows-spec { row-kind: string, rows: list<virtual-row> }

  // ---- sound, notification, views, commands, messages
  enum urgency { low, normal, high }
  record notify-spec {
    title: string, body: option<string>, urgency: urgency, sound: option<string>,
    /// At most 60,000. 0 or no key: no coalescing.
    coalesce-key: option<string>, coalesce-window-ms: u32,
  }
  record open-view-spec { view: view-target, focus: bool }
  record invoke-command-spec { command: string, arguments-json: string }
  enum plugin-side { server, client }
  record message-target { plugin: plugin-id, side: plugin-side, client: option<client-id> }
  record message-spec { to: message-target, channel: string, payload: list<u8> }

  // ---- editor
  record editor-apply-edits-spec { buffer: buffer-id, base-revision: u64, edits: list<text-edit> }
  enum editor-placement { new-pane, split, replace }
  record editor-open-spec { path: fs-path, line: option<u32>, placement: editor-placement }

  // ---- appended: pane lifecycle
  record freeze-pane-spec { pane: node-id }
  record unfreeze-pane-spec { pane: node-id }
  record replace-pane-command-spec { pane: node-id, command: list<string>, cwd: option<fs-path>, keep-scrollback: bool }
  /// grace-ms is at most 10,000.
  record terminate-pane-process-spec { pane: node-id, grace-ms: u32 }
  record resize-pane-spec { pane: node-id, columns: option<u16>, rows: option<u16> }

  // ---- appended: tree structure
  record create-group-spec { parent: option<node-id>, title: string, placement: placement, children: list<node-id> }
  record create-folder-spec { parent: option<node-id>, title: string, path: option<fs-path> }
  record create-project-spec { title: string, root: fs-path }
  /// 2 to 4 panes.
  record create-split-spec { panes: list<node-id>, direction: split-direction }
  record set-bookmarked-spec { node: node-id, bookmarked: bool }
  variant expand-target { node(node-id), virtual-row(string) }
  record set-expanded-spec { target: expand-target, expanded: bool }
  variant restructure-op {
    move(move-node-spec),
    create-group(create-group-spec),
    ungroup(node-id),
    rename(rename-node-spec),
  }
  /// At most 256 operations.
  record restructure-spec { operations: list<restructure-op>, label: string }

  // ---- appended: chrome
  record set-inferred-icon-spec { node: node-id, icon: option<string>, source: string }
  /// reason is at most 80 characters.
  record mark-attention-spec { node: node-id, reason: string }
  variant external-target { url(string), file(fs-path), settings(string) }
  record open-external-spec { target: external-target }
  record set-status-segment-spec { segment: string, content: rich-text, tooltip: option<tooltip>, tone: option<tone> }
  record set-frame-slot-spec { pane: node-id, slot: string, content: rich-text, icon: option<string>, tooltip: option<tooltip> }
  record clear-frame-slot-spec { pane: node-id, slot: string }
  /// label is at most 24 characters.
  record toast-action { label: string, command: string }
  record toast-spec {
    /// A toast with the same id is replaced in place.
    id: option<string>,
    tone: tone,
    /// At most 60 characters.
    title: string,
    /// At most 200 characters of text.
    body: option<rich-text>,
    /// At most 2.
    actions: list<toast-action>,
    /// 1,000 to 30,000.
    duration-ms: option<u32>,
  }

  /// 42 cases. The order is the ABI.
  variant intent {
    rename-node(rename-node-spec),
    move-node(move-node-spec),
    close-node(node-id),
    set-node-locked(set-node-locked-spec),
    create-pane(create-pane-spec),
    signal-pane(signal-pane-spec),
    send-input(send-input-spec),
    agent-action(agent-action-spec),
    set-decoration(set-decoration-spec),
    clear-decoration(decoration-key),
    set-virtual-rows(set-virtual-rows-spec),
    play-sound(string),
    notify(notify-spec),
    open-view(open-view-spec),
    close-view(view-target),
    invoke-command(invoke-command-spec),
    message(message-spec),
    editor-apply-edits(editor-apply-edits-spec),
    editor-open(editor-open-spec),
    // appended in this order
    freeze-pane(freeze-pane-spec),
    unfreeze-pane(unfreeze-pane-spec),
    replace-pane-command(replace-pane-command-spec),
    terminate-pane-process(terminate-pane-process-spec),
    resize-pane(resize-pane-spec),
    create-group(create-group-spec),
    create-folder(create-folder-spec),
    create-project(create-project-spec),
    create-split(create-split-spec),
    set-bookmarked(set-bookmarked-spec),
    set-expanded(set-expanded-spec),
    restructure(restructure-spec),
    revert-restructure(u64),
    set-inferred-icon(set-inferred-icon-spec),
    mark-attention(mark-attention-spec),
    ack-attention(node-id),
    open-external(open-external-spec),
    set-status-segment(set-status-segment-spec),
    clear-status-segment(string),
    set-frame-slot(set-frame-slot-spec),
    clear-frame-slot(clear-frame-slot-spec),
    toast(toast-spec),
    dismiss-toast(string),
  }
}
```

The case `intent.message` uses `message-spec`. The old `set-timer` and `cancel-timer` intents do not exist. Timers are the `ilium:host/timers` import.


### 4.12 events

The `event` variant has 56 cases: 38 base cases and 18 appended cases. A reader who counts table rows in the event catalogue finds 35 rows for the base set, because `editor-opened` and `editor-closed`, `key-pressed` and `key-released`, and `client-attached` and `client-detached` share a row each. The case count is 38. Both numbers are correct, and the case count is the one that matters for the ABI [ABI-002].

Payload naming rule: a payload with several fields is a record named `<case>-event`. A payload with one value of an existing type uses that type. Delivery policy, grant and coalescing key of every case are in the [Host conformance matrix](/pages/reference-host-conformance/#4-event-matrix) and the [Event and intent tables](/pages/reference-events-and-intents/).

```wit
interface events {
  use ilium:types/ids@0.1.0.{node-id, plugin-id, client-id, buffer-id, request-id, timer-id, watch-id, stream-id, child-id, grab-id, epoch-ms};
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/paths@0.1.0.{fs-path};
  use ilium:types/net@0.1.0.{header};
  use ilium:types/editing@0.1.0.{buffer-ref, editor-viewport, text-position};
  use ilium:types/tree@0.1.0.{pane-info, agent-activity, agent-model, pane-lifecycle-state, title-origin};
  use ilium:types/ui@0.1.0.{view-target, ui-action, view-pointer, pointer-kind, pointer-button, pointer-modifiers};
  use ilium:types/requests@0.1.0.{request-result};
  use ilium:types/intents@0.1.0.{plugin-side};

  // ---- payload records
  record pane-exited-event { node: node-id, status: option<s32>, signal: option<string> }
  record pane-resized-event { node: node-id, columns: u16, rows: u16 }
  record pane-title-changed-event { node: node-id, title: string, source: title-origin }
  record agent-identified-event { node: node-id, signature: string }
  record agent-activity-event { node: node-id, previous: agent-activity, current: agent-activity, revision: u64 }
  record agent-goal-event { node: node-id, goal: option<string> }
  enum input-source { user, plugin, scheduled, voice }
  record input-submitted-event { node: node-id, text: string, source: input-source, epoch: u64 }
  record output-matched-event { node: node-id, filter-id: string, line: string, row: u32 }
  record link-activated-event { node: node-id, detector-id: string, text: string }
  /// Reported only while the plugin has claimed the pointer of the pane.
  /// kind move is reported only while the pointer is over the pane.
  record pointer-event {
    node: node-id, row: u32, column: u16, kind: pointer-kind, button: pointer-button,
    modifiers: pointer-modifiers, sub-x: u8, sub-y: u8,
  }
  record editor-changed-event { buffer: buffer-id, revision: u64, first-line: u32, last-line: u32 }
  record editor-saved-event { buffer: buffer-id, path: fs-path }

  enum settings-origin { user, plugin, reset, other-plugin, file }
  /// keys are the changed keys. origin is the last origin when several changes merged.
  /// side tells which side owns the keys.
  record settings-change { keys: list<string>, origin: settings-origin, side: plugin-side }

  record job-progress-event { job: request-id, percent: f32 }
  record message-event {
    %from: plugin-id, from-side: plugin-side, from-client: option<client-id>,
    channel: string, payload: list<u8>,
  }
  record topic-published-event { topic: string, %from: plugin-id, payload-json: string }

  /// 13 origins.
  enum command-origin {
    keybinding, palette, toolbar, menu, voice, cli, toast, row-action, status-segment,
    tree-toolbar, frame-slot, virtual-row-drop, text,
  }
  record command-context {
    pane: option<node-id>, view: option<view-target>, buffer: option<buffer-id>, line: option<u32>,
    virtual-row: option<string>, menu-item: option<string>,
  }
  record command-invoked-event {
    command: string, arguments-json: string, origin: command-origin, context: command-context,
  }
  record key-event { grab: grab-id, pane: option<node-id>, chord: string, repeat: bool }
  record view-visibility-event { view: view-target, visible: bool }
  record fs-changed-event { watch: watch-id, paths: list<string> }
  record audio-frames-event { capture: u64, samples: list<s16> }
  record cli-invoked-event { invocation: u64, name: string, argv: list<string>, cwd: string }
  record http-called-event {
    call: u64, method: string, path: string, headers: list<header>, body: list<u8>,
  }
  record dropped-count { event: string, source: option<string>, count: u32 }

  // ---- appended events
  record screen-changed-event { pane: node-id, first-row: u32, last-row: u32, revision: u64 }
  record user-line-event { pane: node-id, text: string, at: epoch-ms }
  record signal-changed-event { pane: node-id, signal: string, value: string }
  record agent-model-event { pane: node-id, model: agent-model }
  record editor-pointer-event { buffer: buffer-id, pane: node-id, position: text-position, pointer: view-pointer }
  record http-stream-data-event {
    %stream: stream-id, status: option<u16>, headers: option<list<header>>, chunk: list<u8>,
  }
  variant stream-outcome { ended, cancelled, failed(plugin-error) }
  record http-stream-closed-event { %stream: stream-id, outcome: stream-outcome }
  enum output-stream { stdout, stderr }
  record child-output-event { child: child-id, %stream: output-stream, bytes: list<u8> }
  record child-exited-event { child: child-id, status: option<s32>, signal: option<string> }
  record pane-lifecycle-event { pane: node-id, state: pane-lifecycle-state, by: option<plugin-id> }
  record attention-event { node: node-id, marked: bool, reason: option<string>, by: option<plugin-id> }
  record client-request-event { call: u64, kind: string, payload-json: string }
  record kernel-settings-event { tab: string, keys: list<string> }
  /// late-by-millis is how long after its due time the timer fired, in milliseconds.
  record durable-timer-event { timer: timer-id, name: string, late-by-millis: u64 }
  record input-uncertain-event { pane: node-id, dedup-key: string }

  /// 56 cases. The order is the ABI.
  variant event {
    tree-changed(list<node-id>),
    node-removed(node-id),
    pane-created(pane-info),
    pane-exited(pane-exited-event),
    pane-focused(option<node-id>),
    pane-resized(pane-resized-event),
    pane-title-changed(pane-title-changed-event),
    agent-identified(agent-identified-event),
    agent-activity-changed(agent-activity-event),
    agent-goal-changed(agent-goal-event),
    input-submitted(input-submitted-event),
    output-matched(output-matched-event),
    link-activated(link-activated-event),
    pointer(pointer-event),
    editor-opened(buffer-ref),
    editor-closed(buffer-ref),
    editor-changed(editor-changed-event),
    editor-saved(editor-saved-event),
    settings-changed(settings-change),
    timer-fired(timer-id),
    request-completed(request-result),
    job-progress(job-progress-event),
    message(message-event),
    topic-published(topic-published-event),
    command-invoked(command-invoked-event),
    ui-action(ui-action),
    key-pressed(key-event),
    key-released(key-event),
    view-visibility(view-visibility-event),
    fs-changed(fs-changed-event),
    audio-frames(audio-frames-event),
    cli-invoked(cli-invoked-event),
    cli-cancelled(u64),
    http-called(http-called-event),
    client-attached(client-id),
    client-detached(client-id),
    theme-changed,
    events-dropped(list<dropped-count>),
    // appended in this order
    screen-changed(screen-changed-event),
    user-line-submitted(user-line-event),
    signal-changed(signal-changed-event),
    agent-model-changed(agent-model-event),
    editor-pointer(editor-pointer-event),
    editor-viewport-changed(editor-viewport),
    http-stream-data(http-stream-data-event),
    http-stream-closed(http-stream-closed-event),
    child-output(child-output-event),
    child-exited(child-exited-event),
    pane-lifecycle-changed(pane-lifecycle-event),
    attention-changed(attention-event),
    client-request(client-request-event),
    kernel-settings-changed(kernel-settings-event),
    keymap-changed,
    durable-timer-fired(durable-timer-event),
    input-uncertain(input-uncertain-event),
    display-mode-changed(option<string>),
  }
}
```

The record `header` is declared once in `ilium:types/net`. The events and the `http` host interface both import it from there.

### 4.13 lifecycle

```wit
interface lifecycle {
  use ilium:types/ids@0.1.0.{plugin-id};
  /// The order is the ABI. crash-restart is reserved: the host never delivers it, because a
  /// crashed instance has already stopped and cannot be called. Five reasons are delivered.
  /// revoked and granted precede a re-activation with the changed granted list.
  enum deactivation-reason { shutdown, disabled, upgrade, crash-restart, revoked, granted }
  record activation {
    plugin: plugin-id,
    /// Always "ilium:plugin@0.1.0".
    api: string,
    /// Unique per activation of this plugin on this side. Starts at 1.
    instance: u64,
    /// "server", "client" or "scene".
    side: string,
    /// The plugin's own settings as one JSON object.
    settings-json: string,
    /// The capabilities actually granted, as strings.
    granted: list<string>,
    /// The bytes that the previous deactivate returned. None after a first start or a dropped restore.
    restored: option<list<u8>>,
  }
}
```

The field `activation.side` is a string and not the `plugin-side` enum, because a scene plugin has the side `scene` and `plugin-side` has two cases. Messages address only server and client plugins, so `message-target.side` uses `plugin-side`.


## 5. Package ilium:host

Every interface of this package is imported by a plugin. The line under each heading gives the sides that may import it (S server, C client, A scene) and the grant. The column "sync or async" of every function is in the [Host conformance matrix](/pages/reference-host-conformance/). A function that returns `result<request-id, plugin-error>` is async, and its `take-*` partner is named in the comment on the same line. For all async calls the rules of section 5.40 apply.

Common errors: every function that returns `plugin-error` can return `not-permitted` for a missing grant, `invalid-argument` for a bad value, `quota-exceeded` and `rate-limited` for a limit, and `internal` for a host fault. The lists below add only the codes that are specific to the function.

```wit
package ilium:host@0.1.0;
```

### 5.1 log

S C A. Grant: none. API page: [Log](/pages/api-log/).

```wit
interface log {
  enum level { trace, debug, info, warn, error }
  /// One entry per line. The budget is 200 lines per second per side instance (limits.log_lines_per_second,
  /// 1,000 for bundled plugins). Lines over the budget are dropped and counted.
  log: func(level: level, message: string);
}
```

### 5.2 clock

S C A. Grant: none. API page: [Clock and timers](/pages/api-clock-and-timers/).

```wit
interface clock {
  /// UTC wall clock in milliseconds. May jump when the user changes the clock.
  now-unix-millis: func() -> u64;
  /// Milliseconds since an unspecified start. Never goes backwards. Not comparable between instances.
  monotonic-millis: func() -> u64;
}
```

### 5.3 timers

S C. Grant: none. API pages: [Clock and timers](/pages/api-clock-and-timers/) and [Durable timers](/pages/api-durable-timers/).

```wit
interface timers {
  use ilium:types/ids@0.1.0.{timer-id, epoch-ms};
  use ilium:types/errors@0.1.0.{plugin-error};
  /// A volatile timer. after-millis and repeat-millis are at least 50. Fires timer-fired.
  /// Errors: invalid-argument below 50 ms, quota-exceeded above the timer limit.
  set: func(after-millis: u32, repeat-millis: option<u32>) -> result<timer-id, plugin-error>;
  cancel: func(timer: timer-id);
  /// A durable timer, kept in the snapshot. Fires durable-timer-fired, late after a downtime.
  /// name is 1 to 64 characters and unique per plugin: the same name replaces the earlier timer.
  at: func(epoch: epoch-ms, name: string) -> result<timer-id, plugin-error>;
  cancel-durable: func(name: string);
}
```

### 5.4 journal

S C. Grant: none for own entries, `journal.read` for others. API page: [Durable timers](/pages/api-durable-timers/).

```wit
interface journal {
  use ilium:types/ids@0.1.0.{node-id, epoch-ms};
  use ilium:types/errors@0.1.0.{plugin-error};
  record journal-entry { at: epoch-ms, pane: option<node-id>, source: string, kind: string, payload-json: string }
  /// payload-json is at most 4 KiB. The oldest entries are trimmed at limits.journal_entries.
  log-event: func(pane: option<node-id>, kind: string, payload-json: string) -> result<_, plugin-error>;
  /// Own entries, newest last. limit is at most 1,000; a larger limit is clamped to 1,000.
  read: func(pane: option<node-id>, since: epoch-ms, limit: u32) -> list<journal-entry>;
}
```

### 5.5 tree

S C. Grant: `tree.read`; `tree.metadata` for the metadata calls. API page: [Tree](/pages/api-tree/).

```wit
interface tree {
  use ilium:types/ids@0.1.0.{node-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/tree@0.1.0.{node-view, node-query};
  get: func(node: node-id) -> option<node-view>;
  children: func(node: node-id) -> list<node-id>;
  roots: func() -> list<node-id>;
  /// At most 4,096 ids.
  find: func(query: node-query) -> list<node-id>;
  /// The plugin's own metadata map for the node. At most 64 KiB per node.
  metadata: func(node: node-id) -> option<list<u8>>;
  set-metadata: func(node: node-id, bytes: list<u8>) -> result<_, plugin-error>;
}
```

### 5.6 panes

S C. Grant: `panes.read`. API page: [Panes](/pages/api-panes/).

```wit
interface panes {
  use ilium:types/ids@0.1.0.{node-id};
  use ilium:types/tree@0.1.0.{pane-info};
  info: func(pane: node-id) -> option<pane-info>;
  focused: func() -> option<node-id>;
}
```

### 5.7 intents

S C. Grant: depends on the intent. API page: [Intents](/pages/api-intents/).

```wit
interface intents-host {
  use ilium:types/ids@0.1.0.{request-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/intents@0.1.0.{submission};
  /// For use outside a batch. Carries one submission. Completes as request-completed with request-ref
  /// submission(correlation), taken from the submission passed in. The returned request-id is
  /// informational: it never appears in a completion.
  /// Errors: invalid-argument (malformed submission), not-permitted (grant missing: reported at once),
  /// rate-limited.
  submit: func(submission: submission) -> result<request-id, plugin-error>;
}
```

The interface name `intents-host` avoids a clash with `ilium:types/intents` in generated names. In the SDKs it is the module `intents`.

### 5.8 storage

S C A (A: `get` only). Grant: none. API page: [Storage](/pages/api-storage/).

```wit
interface storage {
  use ilium:types/errors@0.1.0.{plugin-error};
  /// shared is visible to every plugin that holds the grant `storage.shared:<name>`.
  enum bucket { global, project, session, shared }
  /// key is 1 to 256 bytes. A value is at most 1 MiB. A bucket holds 16 MiB by default (limits.storage_bucket_mib).
  get: func(bucket: bucket, key: string) -> option<list<u8>>;
  put: func(bucket: bucket, key: string, value: list<u8>) -> result<_, plugin-error>;
  remove: func(bucket: bucket, key: string);
  keys: func(bucket: bucket, prefix: string) -> list<string>;
  /// none removes the key.
  record entry { key: string, value: option<list<u8>> }
  /// All entries or none. At most 256 entries and 4 MiB of key and value bytes together.
  /// Errors: invalid-argument above the limits, quota-exceeded, conflict for a stale data version.
  write-batch: func(bucket: bucket, entries: list<entry>) -> result<_, plugin-error>;
  /// true for the one caller that may refresh the shared key. ttl-millis 1 to 3,600,000.
  lease: func(key: string, ttl-millis: u32) -> bool;
}
```

### 5.9 settings

S C. Grant: none for own values, `settings.read-all` and `settings.write-all` for other plugins. API pages: [Settings](/pages/api-settings/) and [Settings schema](/pages/reference-settings-schema/).

```wit
interface settings {
  use ilium:types/ids@0.1.0.{plugin-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/text@0.1.0.{rich-text};
  use ilium:types/intents@0.1.0.{plugin-side};
  use ilium:types/ui@0.1.0.{catalog-item, row-state, meter-segment};

  record choice-option { value: string, title: string }
  record item-state { index: u32, state: row-state }
  record settings-ref {
    plugin: plugin-id, tab: string, section: option<string>, title: string, side: plugin-side,
    kernel-tab: option<string>,
  }
  record field-error { key: string, message: string }

  /// Own values as one JSON object. Always allowed.
  get-json: func() -> string;
  /// Merge patch (RFC 7396), validated against the schema. Does not call settings-validate.
  /// Errors: invalid-argument for a key of the other side or one that is not in the schema.
  set-json: func(patch: string) -> result<_, plugin-error>;

  // own rows
  /// info row with live true. At most 120 characters.
  set-info: func(key: string, text: string) -> result<_, plugin-error>;
  /// choice row with dynamic true. 1 to 64 options.
  set-choices: func(key: string, options: list<choice-option>) -> result<_, plugin-error>;
  /// A dim note under the description. At most 120 characters.
  set-row-note: func(key: string, note: option<string>) -> result<_, plugin-error>;
  set-row-state: func(key: string, state: row-state) -> result<_, plugin-error>;
  set-item-states: func(key: string, items: list<item-state>) -> result<_, plugin-error>;
  set-choice-items: func(key: string, items: list<catalog-item>) -> result<_, plugin-error>;
  set-info-rich: func(key: string, text: rich-text) -> result<_, plugin-error>;
  /// At most 8 segments.
  set-meter: func(id: string, segments: list<meter-segment>) -> result<_, plugin-error>;
  /// Client only. Opens the settings page of the plugin, or a target.
  open: func(target: string) -> result<_, plugin-error>;

  // other plugins
  /// Grant settings.read-all.
  list-settings: func() -> list<settings-ref>;
  get-plugin-json: func(plugin: plugin-id) -> result<string, plugin-error>;
  /// Grant settings.write-all.
  set-plugin-json: func(plugin: plugin-id, patch: string) -> result<_, plugin-error>;
}
```

### 5.10 codec

S C A. Grant: none. API page: [Codec](/pages/api-codec/).

```wit
interface codec {
  use ilium:types/errors@0.1.0.{plugin-error};
  enum compression { zlib, gzip, deflate-raw }
  record image-info { width: u32, height: u32, frames: u32 }
  record decoded-image { width: u32, height: u32, rgba: list<u8> }
  record raster-text { text: string, size-px: u16, max-width-px: u32 }
  /// Errors: invalid-argument for corrupt data, quota-exceeded when the output passes max-output.
  inflate: func(format: compression, data: list<u8>, max-output: u32) -> result<list<u8>, plugin-error>;
  /// level 0 to 9.
  deflate: func(format: compression, data: list<u8>, level: u8) -> result<list<u8>, plugin-error>;
  image-info: func(data: list<u8>) -> result<image-info, plugin-error>;
  /// Errors: quota-exceeded when width times height passes max-pixels.
  decode-image: func(data: list<u8>, frame: u32, max-pixels: u32) -> result<decoded-image, plugin-error>;
  sha256: func(data: list<u8>) -> list<u8>;
  /// Gray in the alpha channel.
  rasterize-text: func(request: raster-text) -> result<decoded-image, plugin-error>;
}
```

### 5.11 fs

S C. Grant: `fs.read:<scope>`, `fs.write:<scope>`, `fs.watch:<scope>`, `fs.pick`. API pages: [Files](/pages/api-fs/) and [Atomic writes and tail](/pages/api-fs-atomic-and-tail/).

```wit
interface fs {
  use ilium:types/ids@0.1.0.{request-id, watch-id, lock-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/paths@0.1.0.{fs-path};

  enum entry-kind { file, directory, symlink, other }
  record fs-metadata { kind: entry-kind, length: u64, modified-unix-millis: option<u64>, read-only: bool }
  record dir-entry { name: string, metadata: fs-metadata }
  record scope-root { scope: string, writable: bool }
  record scanned-entry { relative-path: string, entry: dir-entry }
  enum write-mode { create-new, replace, append }
  enum pick-kind { open-file, open-directory, save-file }
  record pick-spec { kind: pick-kind, title: string, extensions: list<string> }
  /// Opaque. Equal tokens mean equal content.
  type revision-token = string;
  record revisioned { bytes: list<u8>, revision: revision-token }
  /// truncated: the file shrank or was replaced, and the next read restarts at 0.
  record tail-chunk { bytes: list<u8>, cursor: u64, truncated: bool }

  /// Granted scopes and whether each is writable.
  roots: func() -> list<scope-root>;
  /// Sync. Errors: not-found.
  metadata: func(path: fs-path) -> result<fs-metadata, plugin-error>;
  /// Sync. max-entries at most 4,096.
  list-dir: func(path: fs-path, max-entries: u32) -> result<list<dir-entry>, plugin-error>;
  /// Sync. max-bytes at most 1 MiB.
  read: func(path: fs-path, offset: u64, max-bytes: u32) -> result<list<u8>, plugin-error>;
  /// Async, take-bytes. At most 64 MiB.
  read-all: func(path: fs-path) -> result<request-id, plugin-error>;
  /// Async, take-entries. glob follows the scope root. max-entries at most 100,000.
  scan: func(path: fs-path, glob: string, max-entries: u32) -> result<request-id, plugin-error>;
  /// Async, take-written is not used: the answer is request-completed ok. replace is atomic.
  write: func(path: fs-path, data: list<u8>, mode: write-mode) -> result<request-id, plugin-error>;
  create-dir: func(path: fs-path) -> result<_, plugin-error>;
  /// Async. request-completed ok.
  remove: func(path: fs-path) -> result<request-id, plugin-error>;
  /// Async. Same scope only. request-completed ok.
  rename: func(source: fs-path, target: fs-path) -> result<request-id, plugin-error>;
  /// Grant fs.watch. Fires fs-changed.
  watch: func(path: fs-path, recursive: bool) -> result<watch-id, plugin-error>;
  unwatch: func(watch: watch-id);
  /// Async, take-lock. Advisory lock. timeout-millis at most 60,000. Errors: timed-out, busy.
  lock: func(path: fs-path, exclusive: bool, timeout-millis: u32) -> result<request-id, plugin-error>;
  unlock: func(lock: lock-id);
  /// Async, take-picked. Client only. Grant fs.pick. none: the user cancelled.
  pick: func(spec: pick-spec) -> result<request-id, plugin-error>;
  /// Async, take-revisioned.
  read-with-revision: func(path: fs-path) -> result<request-id, plugin-error>;
  /// Async, take-written. expected none: create only, conflict if the file exists.
  /// expected some(token): replace only if unchanged, else conflict.
  write-atomic: func(path: fs-path, data: list<u8>, expected: option<revision-token>) -> result<request-id, plugin-error>;
  /// Sync. max-bytes at most 1 MiB.
  tail: func(path: fs-path, cursor: u64, max-bytes: u32) -> result<tail-chunk, plugin-error>;

  take-bytes: func(request: request-id) -> option<list<u8>>;
  take-entries: func(request: request-id) -> option<list<scanned-entry>>;
  take-lock: func(request: request-id) -> option<lock-id>;
  take-picked: func(request: request-id) -> option<fs-path>;
  take-revisioned: func(request: request-id) -> option<revisioned>;
  take-written: func(request: request-id) -> option<revision-token>;
}
```

### 5.12 http

S C. Grant: `http:<host-pattern>`. API pages: [HTTP](/pages/api-http/) and [HTTP streams](/pages/api-http-streams/).

```wit
interface http {
  use ilium:types/ids@0.1.0.{request-id, stream-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/net@0.1.0.{header};
  record http-request { method: string, url: string, headers: list<header>, body: option<list<u8>>, timeout-millis: u32 }
  record http-response { status: u16, headers: list<header>, body: list<u8> }
  enum stream-framing { raw, lines, server-sent-events, ndjson }
  /// max-chunk-bytes at most 64 KiB.
  record stream-request { request: http-request, framing: stream-framing, max-chunk-bytes: u32 }
  /// Async, take-response. Body at most 8 MiB by default. Errors: network, timed-out.
  fetch: func(request: http-request) -> result<request-id, plugin-error>;
  take-response: func(request: request-id) -> option<http-response>;
  /// Delivers http-stream-data and http-stream-closed events. At most 4 streams per plugin by default, 64 per session.
  open-stream: func(request: stream-request) -> result<stream-id, plugin-error>;
  close-stream: func(stream: stream-id);
  // @unstable(feature = websocket)
  open-websocket: func(url: string, protocols: list<string>) -> result<stream-id, plugin-error>;
  // @unstable(feature = websocket)
  send-websocket: func(stream: stream-id, text: string) -> result<_, plugin-error>;
}
```

### 5.13 jobs

S C A. Grant: `jobs.run`. API page: [Jobs](/pages/api-jobs/).

```wit
interface jobs {
  use ilium:types/ids@0.1.0.{request-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  /// kind must be in contributes.jobs. input at most 4 MiB. Fires job-progress, then request-completed.
  start: func(kind: string, input: list<u8>) -> result<request-id, plugin-error>;
  cancel: func(job: request-id);
  take-output: func(job: request-id) -> option<list<u8>>;
  /// Valid only inside job-runner.run. Elsewhere a no-op.
  report-progress: func(percent: f32);
  /// Valid only inside job-runner.run. Elsewhere false.
  is-cancelled: func() -> bool;
  /// Valid only inside job-runner.run. Blocks until the request completes or the timeout passes.
  wait-request: func(request: request-id, timeout-millis: u32) -> result<_, plugin-error>;
}
```

### 5.14 process

S C. Grant: `process.run:<program>`, `process.long-lived`. API pages: [Process control](/pages/api-process-control/).

```wit
interface process {
  use ilium:types/ids@0.1.0.{request-id, child-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/paths@0.1.0.{fs-path};
  /// timeout-millis at most 600,000.
  record process-spec {
    program: string, arguments: list<string>, cwd: option<fs-path>, stdin: option<list<u8>>,
    environment: list<tuple<string, string>>, timeout-millis: u32,
  }
  /// status none when the process was killed by a signal. stdout and stderr are each cut at 4 MiB.
  record process-exit { status: option<s32>, signal: option<string>, stdout: list<u8>, stderr: list<u8> }
  record spawn-spec { process: process-spec, stdin-open: bool, line-mode: bool }
  /// Async, take-exit.
  run: func(spec: process-spec) -> result<request-id, plugin-error>;
  take-exit: func(request: request-id) -> option<process-exit>;
  resource child {
    /// For display only.
    pid-hint: func() -> u32;
    write-stdin: func(bytes: list<u8>) -> result<_, plugin-error>;
    close-stdin: func();
    /// interrupt, terminate, kill or hangup.
    signal: func(signal: string) -> result<_, plugin-error>;
    /// The id carried by child-output and child-exited events.
    id: func() -> child-id;
  }
  /// Delivers child-output and child-exited. At most 2 children per plugin by default, 16 per session.
  spawn: func(spec: spawn-spec) -> result<child, plugin-error>;
}
```

### 5.15 inference

S C. Grant: `inference`; `inference.provide` for provider replies. API pages: [Inference](/pages/api-inference/) and [Services](/pages/services/).

```wit
interface inference {
  use ilium:types/ids@0.1.0.{request-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  record chat-message { role: string, content: string }
  record completion-request {
    messages: list<chat-message>, max-output-tokens: option<u32>, json-schema: option<string>, purpose: string,
  }
  record completion { text: string, model: string, input-tokens: u32, output-tokens: u32 }
  /// Async, take-completion.
  complete: func(request: completion-request) -> result<request-id, plugin-error>;
  take-completion: func(request: request-id) -> option<completion>;
  /// Async, take-models.
  models: func() -> result<request-id, plugin-error>;
  take-models: func(request: request-id) -> option<list<string>>;
  /// Grant inference.provide. Answers inference-provider.complete.
  /// Errors: not-found for an unknown or already answered call id.
  provider-reply: func(call: u64, outcome: result<completion, plugin-error>) -> result<_, plugin-error>;
}
```

### 5.16 vector

S C. Grant: `inference.embed`. API page: [Vector](/pages/api-vector/).

```wit
interface vector {
  use ilium:types/ids@0.1.0.{request-id, index-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  /// Async, take-vectors. At most 256 texts of at most 8 KiB each.
  embed: func(texts: list<string>, purpose: string) -> result<request-id, plugin-error>;
  take-vectors: func(request: request-id) -> option<list<list<f32>>>;
  /// Persisted in the plugin's data scope. dimensions 1 to 4,096.
  create-index: func(name: string, dimensions: u16) -> result<index-id, plugin-error>;
  upsert: func(index: index-id, entries: list<tuple<string, list<f32>>>) -> result<_, plugin-error>;
  remove-entries: func(index: index-id, ids: list<string>) -> result<_, plugin-error>;
  /// Cosine similarity. k at most 100.
  top-k: func(index: index-id, query: list<f32>, k: u16) -> result<list<tuple<string, f32>>, plugin-error>;
  drop-index: func(index: index-id);
}
```

### 5.17 sounds

S C. Grant: none for the catalogue, `sound.play` for the intent. API page: [Capability: sound play](/pages/capability-sound-play/).

```wit
interface sounds {
  /// Bundled and contributed sound ids.
  catalog: func() -> list<string>;
}
```

### 5.18 topics

S C. Grant: `topics.subscribe:<topic-pattern>` to subscribe. Publishing needs no grant, only the plugin's own topic prefix and a publishes entry in the manifest. API page: [Messages and topics](/pages/api-messages-and-topics/).

```wit
interface topics {
  use ilium:types/errors@0.1.0.{plugin-error};
  /// Only topics declared under the plugin's own id. payload-json is at most 64 KiB.
  publish: func(topic: string, payload-json: string) -> result<_, plugin-error>;
}
```

### 5.19 commands

S C. Grant: none for own commands, `commands.invoke:<pattern>`. API pages: [Commands](/pages/api-commands/), [Command palette](/pages/api-command-palette/) and [Menus](/pages/api-menus/).

```wit
interface commands {
  use ilium:types/ids@0.1.0.{request-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  /// Own commands only. Client only.
  set-state: func(command: string, enabled: bool, checked: option<bool>) -> result<_, plugin-error>;
  /// Async. Completes with ok once the command has run. Errors: not-found. Times out after 30 s.
  invoke: func(command: string, arguments-json: string) -> result<request-id, plugin-error>;
}
```

### 5.20 services

S C. Grant: `services.use:<name>` for callers, `services.provide:<name>` for providers. Unstable feature `plugin-services` for calls between plugins. API page: [Services](/pages/services/).

```wit
interface services {
  use ilium:types/ids@0.1.0.{request-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  /// Async, take-reply. A call to the plugin's own service is queued, see Runtime model.
  call: func(service: string, method: string, payload-json: string) -> result<request-id, plugin-error>;
  take-reply: func(request: request-id) -> option<string>;
  /// Provider side, for deferred replies. Errors: not-found for an unknown call id.
  reply: func(call: u64, outcome: result<string, plugin-error>) -> result<_, plugin-error>;
}
```

### 5.21 ui

C. Grant: none for own views. API pages: [UI model](/pages/ui-model/) and [UI](/pages/api-ui/). This is the one table of `ui` functions; the Widgets index links here.

```wit
interface ui-host {
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/ui@0.1.0.{view-target, widget-state, popup-anchor, popup-options, theme-tokens, theme-extras, pixel-format};
  /// Marks the view as stale. The host calls views.render before the next frame. Always granted for own views.
  invalidate: func(view: view-target);
  /// Errors: not-found when the panel is not declared.
  open-panel: func(panel: string) -> result<_, plugin-error>;
  close-panel: func(panel: string);
  /// Errors: not-found when no popup or dialog node with that key is in the last committed tree of any
  /// open view, invalid-argument when two match, quota-exceeded for a fifth open popup in one view.
  open-popup: func(popup: string, anchor: popup-anchor) -> result<_, plugin-error>;
  open-popup-with: func(popup: string, anchor: popup-anchor, options: popup-options) -> result<_, plugin-error>;
  /// An id that is not open is a no-op.
  close-popup: func(popup: string);
  widget-state: func(view: view-target, key: string) -> option<widget-state>;
  /// Errors: not-found for an unknown key, invalid-argument when the case does not fit the widget.
  set-widget-state: func(view: view-target, key: string, state: widget-state) -> result<_, plugin-error>;
  /// Width in cells by the cell width rule of the UI model page (grapheme clusters, UAX #29).
  display-width: func(text: string) -> u16;
  theme: func() -> theme-tokens;
  theme-extras: func() -> theme-extras;
  /// Rows are top to bottom. Rows that fall at or past the surface height are ignored.
  /// Rows are top to bottom. first-row plus the rows written must be within the surface height.
  /// Errors: not-found for a key with no surface, invalid-argument for a bad length.
  write-surface: func(view: view-target, key: string, first-row: u32, format: pixel-format, data: list<u8>) -> result<_, plugin-error>;
}
```

The interface is named `ui-host` in the package so that it does not clash with `ilium:types/ui`. The SDK module is `ui`.

### 5.22 keys

C. Grant: `keys.intercept`. API page: [Keys](/pages/api-keys/).

```wit
interface keys {
  use ilium:types/ids@0.1.0.{node-id, grab-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  /// chords: 1 to 32. until-release stops the grab when the chord is released.
  record grab-spec { chords: list<string>, pane: option<node-id>, until-release: bool }
  /// Errors: conflict when a reserved chord is named, quota-exceeded above 8 grabs.
  grab: func(spec: grab-spec) -> result<grab-id, plugin-error>;
  release: func(grab: grab-id);
}
```

### 5.23 cli

S. Grant: `cli.contribute`. API page: [CLI](/pages/api-cli/).

```wit
interface cli {
  use ilium:types/ids@0.1.0.{node-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  record pane-selection { panes: list<node-id>, all: bool }
  /// Output "jsonl". One JSON object per line.
  write-line: func(invocation: u64, jsonl: string) -> result<_, plugin-error>;
  /// Plain text to stderr.
  write-error: func(invocation: u64, text: string) -> result<_, plugin-error>;
  /// Output "text".
  write-text: func(invocation: u64, text: string) -> result<_, plugin-error>;
  write-stderr: func(invocation: u64, text: string) -> result<_, plugin-error>;
  finish: func(invocation: u64, exit-code: u8);
  /// Async, take-stdin. At most 4 MiB in total.
  read-stdin: func(invocation: u64, max-bytes: u32) -> result<u64, plugin-error>;
  /// An empty list is the end of input.
  take-stdin: func(request: u64) -> option<list<u8>>;
  /// Whether stdout and stderr are terminals.
  is-tty: func(invocation: u64) -> tuple<bool, bool>;
  /// The pane the command ran in, from ILIUM_PANE_ID.
  caller-pane: func(invocation: u64) -> option<node-id>;
  selected-panes: func(invocation: u64) -> option<pane-selection>;
}
```

### 5.24 http-api

S. Grant: `http-api.route:<path>`. API page: [HTTP API](/pages/api-http-api/).

```wit
interface http-api {
  use ilium:types/errors@0.1.0.{plugin-error};
  /// Answers http-called. Errors: not-found for an unknown or answered call.
  respond: func(call: u64, status: u16, headers: list<tuple<string, string>>, body: list<u8>) -> result<_, plugin-error>;
}
```

### 5.25 workspace

S. Grant: `workspace.manage`. API page: [Workspace](/pages/api-workspace/).

```wit
interface workspace {
  use ilium:types/ids@0.1.0.{node-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/paths@0.1.0.{fs-path};
  record workspace-spec { repository: fs-path, path: fs-path, branch: string, base: option<string>, owner: option<node-id> }
  record workspace-info { id: u64, path: fs-path, branch: string, owner: option<node-id>, in-use: bool }
  register: func(spec: workspace-spec) -> result<u64, plugin-error>;
  release: func(workspace: u64) -> result<_, plugin-error>;
  %list: func() -> list<workspace-info>;
}
```

### 5.26 clipboard

C. Grant: `clipboard.read`, `clipboard.write`. API page: [Clipboard](/pages/api-clipboard/).

```wit
interface clipboard {
  use ilium:types/errors@0.1.0.{plugin-error};
  /// At most 1 MiB in UTF-8 bytes. Longer text is cut at a scalar boundary.
  read-text: func() -> option<string>;
  /// Errors: invalid-argument above 1 MiB.
  write-text: func(text: string) -> result<_, plugin-error>;
}
```

### 5.27 audio

C A. Grant: `audio.capture`, `audio.loopback`, `audio.playback`. API page: [Audio](/pages/api-audio/).

```wit
interface audio {
  use ilium:types/ids@0.1.0.{request-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  record audio-device { id: string, name: string, is-input: bool, is-default: bool }
  /// Needs any audio grant; with none held it returns an empty list (no error code).
  devices: func() -> list<audio-device>;
  /// Delivers audio-frames. sample-rate 8,000 to 48,000. channels 1 or 2.
  open-capture: func(device: option<string>, sample-rate: u32, channels: u8) -> result<u64, plugin-error>;
  open-loopback: func(sample-rate: u32, channels: u8) -> result<u64, plugin-error>;
  close-capture: func(capture: u64);
  /// Async. request-completed ok when playback ends.
  play: func(samples: list<s16>, sample-rate: u32, channels: u8) -> result<request-id, plugin-error>;
}
```

### 5.28 media

C A. Grant: `media.video`. API page: [Media](/pages/api-media/).

```wit
interface media {
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/paths@0.1.0.{fs-path};
  record video-info { width: u32, height: u32, frames-per-second: f32, duration-millis: option<u64> }
  open-video: func(path: fs-path) -> result<tuple<u64, video-info>, plugin-error>;
  /// At most 16 MiB.
  open-video-bytes: func(bytes: list<u8>, mime: string) -> result<tuple<u64, video-info>, plugin-error>;
  open-video-range: func(path: fs-path, start-millis: u64, end-millis: option<u64>) -> result<tuple<u64, video-info>, plugin-error>;
  /// RGB8 frame decoded ahead natively. none at the end of the clip.
  next-frame: func(video: u64, width: u32, height: u32) -> result<option<list<u8>>, plugin-error>;
  seek: func(video: u64, millis: u64) -> result<_, plugin-error>;
  close-video: func(video: u64);
}
```

### 5.29 location

C A. Grant: `location.observer`. API page: [Location](/pages/api-location/).

```wit
interface location {
  /// Latitude and longitude, or none when unknown.
  current: func() -> option<tuple<f64, f64>>;
}
```

### 5.30 help

C. Grant: `help.read`. API page: [Help and plugins](/pages/api-help-and-plugins/).

```wit
interface help {
  /// Markdown. none for an unknown topic.
  topic: func(id: string) -> option<string>;
}
```

### 5.31 plugins

C. Grant: `plugins.manage`. API page: [Help and plugins](/pages/api-help-and-plugins/).

```wit
interface plugins {
  use ilium:types/ids@0.1.0.{plugin-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  enum package-state {
    not-installed, installed-enabled, installed-disabled-by-user, disabled-untrusted,
    disabled-blocked, disabled-by-policy, refused-at-load, update-staged, uninstall-pending,
  }
  enum instance-state {
    inactive, activating, waiting-dependency, active, deactivating, restart-scheduled,
    stopped-by-failure, stopped,
  }
  enum health-state { healthy, degraded, restarting, failed, disabled, dormant, refused }
  record side-state {
    /// "server", "client" or "scene".
    side: string,
    /// "server", or the attached client id.
    scope: string,
    state: instance-state,
    failures-in-window: u32,
    /// For example "trap.deadline".
    last-error-code: option<string>,
    restart-at-unix-ms: option<u64>,
  }
  record plugin-info {
    id: plugin-id, name: string, version: string, tier: string, enabled: bool, failed: bool,
    state: package-state, side-states: list<side-state>,
    /// "global" in 0.1.0.
    scope: string,
    disabled-reason: option<string>, health: health-state,
    publisher: string, publisher-key: option<string>,
    source: string, channel: string, pinned: bool,
    update-available: option<string>, deprecated: bool, yanked: bool,
  }
  %list: func() -> list<plugin-info>;
  /// scope is "global" in 0.1.0.
  set-enabled: func(plugin: plugin-id, enabled: bool, scope: string) -> result<_, plugin-error>;
}
```

### 5.32 catalogs

S C. Grant: none. API page: [Catalogs](/pages/api-catalogs/).

```wit
interface catalogs {
  use ilium:types/ids@0.1.0.{request-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/ui@0.1.0.{catalog-item};
  record catalog-page { items: list<catalog-item>, next: option<string>, total: option<u32> }
  /// limit 1 to 256.
  record catalog-query { catalog: string, filter: string, cursor: option<string>, limit: u16 }
  /// Sync, for host catalogues.
  query: func(q: catalog-query) -> result<catalog-page, plugin-error>;
  /// Async, take-page. For plugin catalogues.
  query-async: func(q: catalog-query) -> result<request-id, plugin-error>;
  take-page: func(request: request-id) -> option<catalog-page>;
  lookup: func(catalog: string, id: string) -> option<catalog-item>;
}
```

### 5.33 geocode

S C A. Grant: `location.geocode`. API page: [Geocode](/pages/api-geocode/).

```wit
interface geocode {
  use ilium:types/ids@0.1.0.{request-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/geo@0.1.0.{geo-point, geo-place};
  /// text 2 to 200 characters. limit 1 to 8.
  record geocode-query { text: string, near: option<geo-point>, limit: u8, language: option<string> }
  /// Async, take-places.
  search: func(q: geocode-query) -> result<request-id, plugin-error>;
  /// Async, take-places. At most one place.
  reverse: func(point: geo-point) -> result<request-id, plugin-error>;
  take-places: func(request: request-id) -> option<list<geo-place>>;
}
```

### 5.34 sessions

S. Grant: `sessions.cross`. API page: [Sessions](/pages/api-sessions/).

```wit
interface sessions {
  use ilium:types/ids@0.1.0.{request-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  record live-session { name: string, project: string, socket-id: string, panes: u32, attached-clients: u32, is-self: bool }
  record cross-request { session: string, command: string, arguments-json: string }
  list-live: func() -> list<live-session>;
  /// Async, take-reply.
  send: func(request: cross-request) -> result<request-id, plugin-error>;
  take-reply: func(request: request-id) -> option<string>;
}
```

### 5.35 kernel-settings

S C. Grant: `settings.read-kernel:<tab-id>`. API page: [Kernel settings and keymap](/pages/api-kernel-settings-and-keymap/).

```wit
interface kernel-settings {
  use ilium:types/errors@0.1.0.{plugin-error};
  /// A JSON object of that tab's values. Secrets are "<redacted>".
  read: func(tab: string) -> result<string, plugin-error>;
}
```

### 5.36 keymap

C. Grant: `keymap.read`. API page: [Kernel settings and keymap](/pages/api-kernel-settings-and-keymap/).

```wit
interface keymap {
  record binding { command: string, chords: list<string>, context: string, is-default: bool }
  bindings: func() -> list<binding>;
  /// The user-visible form, for example "Ctrl+Shift+K".
  chord-label: func(chord: string) -> string;
}
```

### 5.37 gpu

C A. Grant: `gpu.kernels`. Unstable feature `gpu-kernels`. API page: [GPU](/pages/api-gpu/).

```wit
interface gpu {
  use ilium:types/errors@0.1.0.{plugin-error};
  enum gpu-kernel { blur, threshold, palette-map, dither-ordered, scale }
  // @unstable(feature = gpu-kernels)
  run: func(kernel: gpu-kernel, input: list<u8>, width: u32, height: u32, params-json: string) -> result<list<u8>, plugin-error>;
  // @unstable(feature = gpu-kernels)
  available: func() -> bool;
}
```

### 5.38 client-requests

S and C. Grant: none for declared kinds. API page: [Client requests](/pages/api-client-requests/).

```wit
interface client-requests {
  use ilium:types/ids@0.1.0.{client-id, request-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  /// Server side. Async, take-answer. timeout-millis 1 to 30,000.
  request: func(client: client-id, kind: string, payload-json: string, timeout-millis: u32) -> result<request-id, plugin-error>;
  take-answer: func(request: request-id) -> option<string>;
  /// Client side. Errors: not-found for an unknown call id.
  respond: func(call: u64, outcome: result<string, plugin-error>) -> result<_, plugin-error>;
}
```

### 5.39 extensions

S C. Grant: `extensions.provide:<point>`, `extensions.contribute:<point>`. API page: [Extension points](/pages/api-extension-points/).

```wit
interface extensions {
  use ilium:types/ids@0.1.0.{plugin-id, request-id};
  record extension-entry {
    /// Contributor-chosen id, unique per contributor and point.
    extension: string,
    contributor: plugin-id,
    priority: u8,
    /// The validated entry.
    entry-json: string,
  }
  /// A payload-less enum: the error model of this interface is the code only.
  enum extension-error { unknown-point, not-granted, schema-mismatch, cap-exceeded, reentrant, not-dynamic, not-found }
  // owner side
  list: func(point: string) -> result<list<extension-entry>, extension-error>;
  /// Async, take-reply.
  run-hook: func(point: string, payload-json: string) -> result<request-id, extension-error>;
  take-reply: func(request: request-id) -> option<string>;
  // contributor side, collect points with dynamic true
  register: func(point: string, extension: string, priority: u8, entry-json: string) -> result<_, extension-error>;
  unregister: func(point: string, extension: string) -> result<_, extension-error>;
}
```

`extension-error` is separate from `plugin-error` because the owner and the contributor need the exact failure of the registry, and the registry has no free-text message. The SDKs map it to `PluginError` with the code `invalid-argument`, `not-permitted`, `quota-exceeded`, `conflict` or `not-found` for the seven cases, and keep the case name in the message [ABI-001].

### 5.40 Take rules

These rules apply to every `take-*` function of this package [ABI-014].

- A result is held for 30 seconds. The hold starts when `on-events` returns for the batch that delivered `request-completed`. After that the result is dropped and the request no longer counts toward `pending_requests`.
- A `take-*` call before `request-completed` returns none. The plugin has to wait for the event.
- A second `take-*` call for the same request returns none. The first call consumed the result.
- A `take-*` call for an unknown request id returns none.
- A result a plugin never takes is dropped on restart. A restarted instance starts with an empty table. A `take-*` call with an id from an earlier instance returns none (the SDK wrapper reports `not-found`). After a trap or restart, completions whose ref is `submission(correlation)` and that were queued or produced after the trap are delivered once to the new instance after `activate` returns `ok`. Completions whose ref is `call(id)` are discarded because the ids belong to the dead instance, and their held results are freed. Slow calls still pending at the trap are cancelled and complete nowhere.
- A plugin holds at most 64 pending requests by default (`pending_requests`, ceiling 256 for tier T1). A session holds at most 4,096. The call that would exceed a limit returns `busy` at once.
- A `request-id` is a `u64`, unique per plugin side for the whole server run. It starts at 1 at server start and is never reused, also across instance restarts.
- A failed request completes with `failed(<error>)`, and then `take-*` returns none.

The full table of slow calls is in [Requests and outcomes](/pages/requests-and-outcomes/).


## 6. Package ilium:primitives

Primitives are the typed access to the built-in pane kinds. A primitive is imported by client and server plugins as the table in section 3 says. Each function names its grant in the comment.

```wit
package ilium:primitives@0.1.0;
```

### 6.1 terminal

S C. Grants: `terminal.read-screen`, `terminal.read-cells`, `terminal.history`, `terminal.output-filter`, `terminal.pointer`, `terminal.input-lock`. API pages: [Terminal](/pages/primitives-terminal/), [Terminal reads](/pages/primitives-terminal-reads/), [Terminal pointer](/pages/primitives-terminal-pointer/) and [Terminal input](/pages/primitives-terminal-input/).

```wit
interface terminal {
  use ilium:types/ids@0.1.0.{node-id, request-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/text@0.1.0.{style, color, modifiers};
  use ilium:types/ui@0.1.0.{pointer-modifiers};

  record screen { columns: u16, rows: list<string>, cursor-row: u16, cursor-column: u16, title: option<string>, revision: u64 }
  enum screen-part { visible, scrollback }
  record selection { first-row: u32, first-column: u16, last-row: u32, last-column: u16, text: string }
  record output-filter { id: string, regex: string, rate-per-second: u16 }
  record cell-region { first-row: u32, first-column: u16, last-row: u32, last-column: u16, style: style, label: option<string> }
  record cell { text: string, fg: option<color>, bg: option<color>, add: modifiers, wide: bool }
  record cell-rows { first-row: u32, columns: u16, rows: list<list<cell>>, revision: u64 }
  flags terminal-mode { alternate-screen, mouse-reporting, bracketed-paste, application-cursor, focus-reporting, synchronized-output }
  record foreground-process { name: string, command-line: list<string>, cwd: option<string> }
  record history-match { row: u32, column: u16, length: u16 }

  /// terminal.read-screen. Sync. max-rows is clamped to 10,000.
  read-screen: func(pane: node-id, part: screen-part, max-rows: u32) -> option<screen>;
  selection: func(pane: node-id) -> option<selection>;
  /// terminal.output-filter. regex is a bounded RE2-style expression. At most 8 filters per pane per plugin.
  add-output-filter: func(pane: node-id, filter: output-filter) -> result<_, plugin-error>;
  remove-output-filter: func(pane: node-id, filter-id: string);
  /// terminal.pointer. Errors: conflict when another plugin holds the claim.
  claim-pointer: func(pane: node-id) -> result<_, plugin-error>;
  /// Claims only the gestures that carry these modifiers. Errors: invalid-argument for an empty set.
  claim-pointer-gesture: func(pane: node-id, modifiers: pointer-modifiers) -> result<_, plugin-error>;
  release-pointer: func(pane: node-id);
  set-highlights: func(pane: node-id, layer: string, regions: list<cell-region>) -> result<_, plugin-error>;
  clear-highlights: func(pane: node-id, layer: string);
  /// terminal.read-cells. count at most 1,000.
  read-cells: func(pane: node-id, part: screen-part, first-row: u32, count: u32) -> option<cell-rows>;
  modes: func(pane: node-id) -> option<terminal-mode>;
  foreground-process: func(pane: node-id) -> option<foreground-process>;
  /// A stable copy of screen and scrollback. At most 4 snapshots per plugin.
  freeze-snapshot: func(pane: node-id) -> result<u64, plugin-error>;
  read-snapshot-cells: func(snapshot: u64, first-row: u32, count: u32) -> option<cell-rows>;
  drop-snapshot: func(snapshot: u64);
  /// terminal.history. Async, take-history. max-bytes at most 4 MiB, the newest text is kept.
  history-text: func(pane: node-id, max-bytes: u32) -> result<request-id, plugin-error>;
  take-history: func(request: request-id) -> option<string>;
  /// Async, take-matches. max-matches at most 1,000.
  search-history: func(pane: node-id, regex: string, max-matches: u16) -> result<request-id, plugin-error>;
  take-matches: func(request: request-id) -> option<list<history-match>>;
  /// terminal.input-lock. millis at most 2,000. User keys are queued, not lost.
  lock-input: func(pane: node-id, millis: u16) -> result<_, plugin-error>;
  /// Releases this plugin's lock. No result: a no-op when no lock is held or the pane is missing.
  unlock-input: func(pane: node-id);
}
```

### 6.2 agent

S C. Grants: `agent.read`, `agent.read-prompts`, `agent.transcript`, `agent.signatures`, `agent.control`. API pages: [Agent](/pages/primitives-agent/) and [Agent transcript](/pages/primitives-agent-transcript/).

```wit
interface agent {
  use ilium:types/ids@0.1.0.{node-id, request-id, epoch-ms};
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/tree@0.1.0.{agent-view, agent-model, agent-activity};

  record agent-pane { pane: node-id, view: agent-view }
  /// A path inside the scope agent-sessions:<agent>.
  record session-ref { scope: string, path: string, session-id: string }
  record signature-action { id: string, keys: string, allowed-when: option<list<agent-activity>> }
  record signature-spec {
    id: string, process-names: list<string>, aliases: list<string>, icon: option<string>,
    resume: option<list<string>>, actions: list<signature-action>,
    session-scope: option<string>, submit-keys: option<string>,
  }
  enum turn-role { user, assistant, tool, system }
  record transcript-turn { role: turn-role, text: string, tool: option<string>, at: option<epoch-ms>, tokens: option<u32> }
  /// last-turns at most 512. max-bytes at most 4 MiB.
  record transcript-query { pane: node-id, last-turns: u32, max-bytes: u32 }

  agents: func() -> list<agent-pane>;
  view: func(pane: node-id) -> option<agent-view>;
  session: func(pane: node-id) -> option<session-ref>;
  /// agent.read-prompts.
  last-prompt: func(pane: node-id) -> option<string>;
  /// agent.signatures. Runtime signatures come from settings.
  register-signature: func(spec: signature-spec) -> result<_, plugin-error>;
  /// agent.control.
  set-toolbar-state: func(button: string, pane: option<node-id>, enabled: bool, label: option<string>) -> result<_, plugin-error>;
  model: func(pane: node-id) -> option<agent-model>;
  /// The epoch that send-input compares against.
  prompt-epoch: func(pane: node-id) -> option<u64>;
  /// agent.transcript. Async, take-transcript.
  transcript: func(query: transcript-query) -> result<request-id, plugin-error>;
  take-transcript: func(request: request-id) -> option<list<transcript-turn>>;
}
```

### 6.3 editor

C. Grants: `editor.read`, `editor.edit`. API pages: [Editor](/pages/primitives-editor/) and [Editor viewport](/pages/primitives-editor-viewport/).

```wit
interface editor {
  use ilium:types/ids@0.1.0.{node-id, buffer-id, request-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/paths@0.1.0.{fs-path};
  use ilium:types/text@0.1.0.{style, color};
  use ilium:types/editing@0.1.0.{buffer-ref, text-range, editor-viewport};

  record range-decoration { range: text-range, style: style, hover: option<string> }
  record gutter-mark { line: u32, icon: string, tooltip: option<string>, command: option<string> }
  /// lane 0 to 2.
  record minimap-mark { line: u32, color: color, lane: u8 }

  buffers: func() -> list<buffer-ref>;
  buffer-for-pane: func(pane: node-id) -> option<buffer-ref>;
  /// count at most 10,000.
  read-lines: func(buffer: buffer-id, first-line: u32, count: u32) -> result<list<string>, plugin-error>;
  selections: func(buffer: buffer-id) -> list<text-range>;
  /// At most 10,000 items per layer, 65,536 per session (PRI-024).
  set-decorations: func(buffer: buffer-id, layer: string, items: list<range-decoration>) -> result<_, plugin-error>;
  set-gutter: func(buffer: buffer-id, layer: string, marks: list<gutter-mark>) -> result<_, plugin-error>;
  /// editor.edit. For a code-view widget.
  create-buffer: func(initial: string, language: option<string>) -> result<buffer-id, plugin-error>;
  /// editor.edit plus fs.read. Async, take-buffer.
  load-file: func(path: fs-path) -> result<request-id, plugin-error>;
  take-buffer: func(request: request-id) -> option<buffer-id>;
  /// editor.edit plus fs.write. Async, request-completed ok.
  save-buffer: func(buffer: buffer-id, path: option<fs-path>) -> result<request-id, plugin-error>;
  /// Grant: editor.edit. Closes a buffer and frees its quota slot.
  close-buffer: func(buffer: buffer-id);
  viewport: func(pane: node-id) -> option<editor-viewport>;
  /// At most 4,096 marks per layer.
  set-minimap-marks: func(buffer: buffer-id, layer: string, marks: list<minimap-mark>) -> result<_, plugin-error>;
}
```

The editor-provider status is decided: the editor is a built-in pane kind, a plugin does not provide its own `ilium.editor` kind, and the grant `editor.provide` does not exist. A plugin that wants its own text pane uses the `basic` primitive with a `panes.provide-kind` grant. See [Capability: editor provide](/pages/capability-editor-provide/).

### 6.4 basic

C. Grant: `panes.provide-kind` for the pane kinds the plugin contributes. API page: [Basic pane primitive](/pages/primitives-basic/).

```wit
interface basic {
  use ilium:types/ids@0.1.0.{node-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/text@0.1.0.{styled-text};
  use ilium:types/tree@0.1.0.{agent-activity};

  /// Own panes only. Errors: not-permitted for a pane of another kind.
  set-title: func(pane: node-id, title: string) -> result<_, plugin-error>;
  /// One line in the footer of the pane frame.
  set-status: func(pane: node-id, status: styled-text) -> result<_, plugin-error>;
  request-focus: func(pane: node-id) -> result<_, plugin-error>;
  /// Inner columns and rows.
  size: func(pane: node-id) -> option<tuple<u16, u16>>;
  /// Drives the activity icon of the tree.
  report-activity: func(pane: node-id, activity: agent-activity) -> result<_, plugin-error>;
  /// A dirty pane asks before it is closed.
  set-dirty: func(pane: node-id, dirty: bool) -> result<_, plugin-error>;
}
```

## 7. Package ilium:anim

Imported by scene plugins only. A scene also imports `log`, `clock`, `storage`, `fs`, `codec`, `jobs`, `http`, `audio`, `media`, `location`, `geocode` and `gpu` from `ilium:host`, as the import matrix says.

```wit
package ilium:anim@0.1.0;
```

### 7.1 surface

Grant: none. Manifest keys: `glyph_plane: true` for the glyph methods. API pages: [Scenes](/pages/api-scenes/) and [Glyph plane and receipts](/pages/api-surface-glyph-plane/).

```wit
interface surface {
  use ilium:types/ids@0.1.0.{epoch-ms};
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/text@0.1.0.{modifiers};
  use ilium:types/ui@0.1.0.{pixel-format};

  /// An occupancy value per cell, set by the host from the pane text: true where text is drawn.
  record occupancy-grid { columns: u16, rows: u16, cells: list<bool> }
  record scene-pointer { column: u16, row: u16, sub-x: u8, sub-y: u8, pressed: bool }
  record glyph-cell { column: u16, row: u16, text: string, fg: option<u32>, bg: option<u32>, add: modifiers }
  /// pointer-owner is the owner id under the pointer at the presented frame.
  record frame-receipt { frame: u64, presented-at-millis: u64, dropped-before: u32, pointer-owner: option<u16> }

  resource frame-surface {
    /// Width and height in pixels.
    size: func() -> tuple<u32, u32>;
    /// Rows are top to bottom. Rows that fall at or past the height are ignored.
    /// Rows are top to bottom. first-row plus the rows written must be within the height.
    /// Errors: invalid-argument for a format the scene did not declare, empty data, or first-row at or past the height. A partial trailing row is dropped, not an error.
    write-rows: func(first-row: u32, format: pixel-format, data: list<u8>) -> result<_, plugin-error>;
    /// Needs uses-cell-colors. One 0xRRGGBB value per cell, row by row.
    write-cell-colors: func(first-row: u32, colors: list<u32>) -> result<_, plugin-error>;
    /// Needs wants-occupancy.
    occupancy: func() -> option<occupancy-grid>;
    /// Needs wants-pointer.
    pointer: func() -> option<scene-pointer>;
    /// Ends the frame. A render call that returns without present drops the frame.
    present: func();
    /// Needs glyph_plane true. At most 131,072 cells per frame.
    write-glyphs: func(cells: list<glyph-cell>) -> result<_, plugin-error>;
    clear-glyphs: func();
    /// One id per pixel, 0 means none.
    set-owner-ids: func(first-row: u32, ids: list<u16>) -> result<_, plugin-error>;
    last-receipt: func() -> option<frame-receipt>;
  }
}
```

### 7.2 fonts

Grant: none. Scene only.

```wit
interface fonts {
  use ilium:types/errors@0.1.0.{plugin-error};
  resource font {
    /// asset is the path of a font file inside the plugin package.
    constructor(asset: string);
    /// Returns width, height and a gray8 bitmap.
    rasterize: func(text: string, size-px: u16) -> result<tuple<u32, u32, list<u8>>, plugin-error>;
  }
}
```

### 7.3 renderer types

```wit
interface renderer-types {
  /// The order is the ABI.
  enum readiness { ready, preparing, failed }
  record frame-input {
    time-millis: u64, wall-millis: u64, now-unix-millis: u64, width: u32, height: u32, palette-rgb: list<u32>,
  }
}
```

## 8. Package ilium:exports

This package holds everything a plugin implements. The host calls these interfaces. A plugin exports `lifecycle` and, on server and client, `handler`. Optional exports are named in the manifest `exports` list, and required exports are never listed there [ABI-018].

```wit
package ilium:exports@0.1.0;
```

### 8.1 lifecycle

```wit
interface lifecycle {
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/lifecycle@0.1.0.{activation, deactivation-reason};
  /// Called once per instance, before any event. An error stops the activation and counts as a failure.
  activate: func(context: activation) -> result<_, plugin-error>;
  /// Called with the reason. The returned bytes (at most 1 MiB) arrive in the next activation as restored.
  deactivate: func(reason: deactivation-reason) -> option<list<u8>>;
}
```

### 8.2 handler

```wit
interface handler {
  use ilium:types/events@0.1.0.{event};
  use ilium:types/intents@0.1.0.{submission};
  /// At most 256 events in, at most 256 submissions out. More submissions trap the call.
  on-events: func(batch: list<event>) -> list<submission>;
}
```

### 8.3 views

```wit
interface views {
  use ilium:types/ui@0.1.0.{view-target, ui-tree};
  use ilium:types/layout@0.1.0.{area};
  /// Pure with respect to the host: no intents are accepted from here.
  render: func(view: view-target, area: area) -> ui-tree;
}
```

### 8.4 detector

```wit
interface detector {
  use ilium:types/tree@0.1.0.{agent-activity};
  record detector-screen { columns: u16, rows: list<string>, cursor-row: u16, title: option<string>, revision: u64 }
  record detector-process { name: string, argv: list<string> }
  record classification { activity: agent-activity, objective: option<string>, goal: option<string> }
  identify: func(processes: list<detector-process>) -> option<string>;
  classify: func(signature: string, screen: detector-screen) -> classification;
}
```

### 8.5 job-runner

```wit
interface job-runner {
  use ilium:types/errors@0.1.0.{plugin-error};
  /// Runs in a job instance, see Runtime model. The output is at most 4 MiB.
  run: func(kind: string, input: list<u8>) -> result<list<u8>, plugin-error>;
}
```

### 8.6 service-provider

```wit
interface service-provider {
  use ilium:types/ids@0.1.0.{plugin-id};
  variant service-reply { done(string), deferred }
  record service-call { call: u64, service: string, method: string, payload-json: string, caller: plugin-id }
  /// deferred means the plugin answers later with services.reply.
  handle: func(call: service-call) -> service-reply;
}
```

### 8.7 inference-provider

```wit
interface inference-provider {
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:host/inference@0.1.0.{completion-request};
  record provider-call { call: u64, request: completion-request }
  /// Answer later with inference.provider-reply.
  complete: func(call: provider-call) -> result<_, plugin-error>;
  models: func() -> list<string>;
}
```

### 8.8 annotator

```wit
interface annotator {
  use ilium:types/ids@0.1.0.{buffer-id};
  use ilium:types/text@0.1.0.{style};
  record line-annotation { line: u32, first-column: u32, last-column: u32, style: style }
  annotate: func(buffer: buffer-id, revision: u64, first-line: u32, lines: list<string>) -> list<line-annotation>;
}
```

### 8.9 settings-validate

```wit
interface settings-validate {
  use ilium:host/settings@0.1.0.{field-error};
  /// 100 ms deadline. A message is at most 120 characters of plain text.
  settings-validate: func(patch: string, merged: string) -> result<_, list<field-error>>;
}
```

### 8.10 catalog-provider

```wit
interface catalog-provider {
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:host/catalogs@0.1.0.{catalog-query, catalog-page};
  query-catalog: func(q: catalog-query) -> result<catalog-page, plugin-error>;
}
```

### 8.11 menu-provider

```wit
interface menu-provider {
  use ilium:types/ids@0.1.0.{node-id, buffer-id};
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/text@0.1.0.{tooltip, tri-state};
  use ilium:types/ui@0.1.0.{view-target};
  record menu-context {
    menu: string, node: option<node-id>, pane: option<node-id>, view: option<view-target>,
    key: option<string>, buffer: option<buffer-id>, line: option<u32>, selection: option<string>,
  }
  record menu-entry {
    id: string, label: string, icon: option<string>, shortcut: option<string>,
    /// Id of a submenu entry. Depth is at most 2.
    parent: option<string>,
    is-submenu: bool, separator-before: bool,
    checked: option<tri-state>,
    /// At most 40 characters.
    disabled-reason: option<string>,
    tooltip: option<tooltip>, danger: bool,
  }
  /// At most 64 entries.
  menu-items: func(context: menu-context) -> list<menu-entry>;
}
```

### 8.12 virtual-rows-provider

```wit
interface virtual-rows-provider {
  use ilium:types/errors@0.1.0.{plugin-error};
  use ilium:types/text@0.1.0.{rich-text, tooltip};
  record virtual-row-child {
    id: string, label: rich-text, icon: option<string>, expandable: bool, command: option<string>,
    tooltip: option<tooltip>, accepts-drop: bool,
  }
  /// Returns a page and the next cursor. none means the last page.
  list-virtual-children: func(row-kind: string, parent: string, cursor: option<string>)
    -> result<tuple<list<virtual-row-child>, option<string>>, plugin-error>;
}
```

### 8.13 token-provider

```wit
interface token-provider {
  use ilium:types/ids@0.1.0.{buffer-id};
  enum token-kind { keyword, string, number, comment, type-name, function, variable, operator, punctuation, constant, tag, attribute, plain }
  record token { line: u32, start: u32, length: u32, kind: token-kind }
  tokens: func(buffer: buffer-id, first-line: u32, lines: list<string>, state-in: option<list<u8>>)
    -> tuple<list<token>, option<list<u8>>>;
}
```

### 8.14 hook-handler

```wit
interface hook-handler {
  use ilium:types/errors@0.1.0.{plugin-error};
  record hook-call { call: u64, point: string, payload-json: string }
  variant hook-reply { pass, replace(string), refuse(string), answer(string) }
  /// The owner of the point calls it through extensions.run-hook.
  handle-hook: func(call: hook-call) -> result<hook-reply, plugin-error>;
}
```

### 8.15 renderer

```wit
interface renderer {
  use ilium:anim/surface@0.1.0.{frame-surface};
  use ilium:anim/renderer-types@0.1.0.{readiness, frame-input};
  resource scene-renderer {
    /// spec-json holds scene, settings, palette and mode.
    constructor(spec-json: string);
    readiness: func() -> readiness;
    frames-per-second: func() -> u32;
    wants-pointer: func() -> bool;
    wants-occupancy: func() -> bool;
    uses-cell-colors: func() -> bool;
    render: func(frame: frame-input, surface: borrow<frame-surface>);
    set-palette: func(palette-rgb: list<u32>);
    /// false asks the host to restart the scene.
    reconfigure: func(settings-json: string) -> bool;
    status: func() -> option<string>;
  }
}
```

### 8.16 migrator

```wit
interface migrator {
  use ilium:types/errors@0.1.0.{plugin-error};
  /// The host calls it once per side before activate when the manifest data_version rose.
  /// It MUST be idempotent: an interrupted run starts again from the same arguments.
  migrate: func(from: u32, to: u32) -> result<_, plugin-error>;
}
```

## 9. Worlds

A world is one base plus fragments. A fragment is a world that `include`s nothing and only adds exports; the SDK composes them. Each fragment name is the one used in the manifest-independent tables of [Worlds and sides](/pages/worlds-and-sides/).

```wit
package ilium:server@0.1.0;

world base {
  import ilium:host/log@0.1.0;
  import ilium:host/clock@0.1.0;
  import ilium:host/timers@0.1.0;
  import ilium:host/journal@0.1.0;
  import ilium:host/tree@0.1.0;
  import ilium:host/panes@0.1.0;
  import ilium:host/intents-host@0.1.0;
  import ilium:host/storage@0.1.0;
  import ilium:host/settings@0.1.0;
  import ilium:host/codec@0.1.0;
  import ilium:host/fs@0.1.0;
  import ilium:host/http@0.1.0;
  import ilium:host/jobs@0.1.0;
  import ilium:host/process@0.1.0;
  import ilium:host/inference@0.1.0;
  import ilium:host/vector@0.1.0;
  import ilium:host/sounds@0.1.0;
  import ilium:host/topics@0.1.0;
  import ilium:host/commands@0.1.0;
  import ilium:host/services@0.1.0;
  import ilium:host/cli@0.1.0;
  import ilium:host/http-api@0.1.0;
  import ilium:host/workspace@0.1.0;
  import ilium:host/catalogs@0.1.0;
  import ilium:host/geocode@0.1.0;
  import ilium:host/sessions@0.1.0;
  import ilium:host/kernel-settings@0.1.0;
  import ilium:host/client-requests@0.1.0;
  import ilium:host/extensions@0.1.0;
  import ilium:primitives/terminal@0.1.0;
  import ilium:primitives/agent@0.1.0;
  export ilium:exports/lifecycle@0.1.0;
  export ilium:exports/handler@0.1.0;
}

world with-detector { export ilium:exports/detector@0.1.0; }
world with-jobs { export ilium:exports/job-runner@0.1.0; }
world with-service { export ilium:exports/service-provider@0.1.0; }
world with-inference-provider { export ilium:exports/inference-provider@0.1.0; }
world with-settings-validate { export ilium:exports/settings-validate@0.1.0; }
world with-catalog { export ilium:exports/catalog-provider@0.1.0; }
world with-virtual-rows { export ilium:exports/virtual-rows-provider@0.1.0; }
world with-hook-handler { export ilium:exports/hook-handler@0.1.0; }
world with-migrator { export ilium:exports/migrator@0.1.0; }
```

```wit
package ilium:client@0.1.0;

world base {
  import ilium:host/log@0.1.0;
  import ilium:host/clock@0.1.0;
  import ilium:host/timers@0.1.0;
  import ilium:host/journal@0.1.0;
  import ilium:host/tree@0.1.0;
  import ilium:host/panes@0.1.0;
  import ilium:host/intents-host@0.1.0;
  import ilium:host/storage@0.1.0;
  import ilium:host/settings@0.1.0;
  import ilium:host/codec@0.1.0;
  import ilium:host/fs@0.1.0;
  import ilium:host/http@0.1.0;
  import ilium:host/jobs@0.1.0;
  import ilium:host/process@0.1.0;
  import ilium:host/inference@0.1.0;
  import ilium:host/vector@0.1.0;
  import ilium:host/sounds@0.1.0;
  import ilium:host/topics@0.1.0;
  import ilium:host/commands@0.1.0;
  import ilium:host/services@0.1.0;
  import ilium:host/ui-host@0.1.0;
  import ilium:host/keys@0.1.0;
  import ilium:host/clipboard@0.1.0;
  import ilium:host/audio@0.1.0;
  import ilium:host/media@0.1.0;
  import ilium:host/location@0.1.0;
  import ilium:host/help@0.1.0;
  import ilium:host/plugins@0.1.0;
  import ilium:host/catalogs@0.1.0;
  import ilium:host/geocode@0.1.0;
  import ilium:host/kernel-settings@0.1.0;
  import ilium:host/keymap@0.1.0;
  import ilium:host/gpu@0.1.0;
  import ilium:host/client-requests@0.1.0;
  import ilium:host/extensions@0.1.0;
  import ilium:primitives/terminal@0.1.0;
  import ilium:primitives/agent@0.1.0;
  import ilium:primitives/editor@0.1.0;
  import ilium:primitives/basic@0.1.0;
  export ilium:exports/lifecycle@0.1.0;
  export ilium:exports/handler@0.1.0;
}

world with-views { export ilium:exports/views@0.1.0; }
world with-annotator { export ilium:exports/annotator@0.1.0; }
world with-jobs { export ilium:exports/job-runner@0.1.0; }
world with-service { export ilium:exports/service-provider@0.1.0; }
world with-inference-provider { export ilium:exports/inference-provider@0.1.0; }
world with-settings-validate { export ilium:exports/settings-validate@0.1.0; }
world with-catalog { export ilium:exports/catalog-provider@0.1.0; }
world with-menus { export ilium:exports/menu-provider@0.1.0; }
world with-virtual-rows { export ilium:exports/virtual-rows-provider@0.1.0; }
world with-tokens { export ilium:exports/token-provider@0.1.0; }
world with-hook-handler { export ilium:exports/hook-handler@0.1.0; }
world with-migrator { export ilium:exports/migrator@0.1.0; }
```

```wit
package ilium:anim@0.1.0;

world base {
  import ilium:host/log@0.1.0;
  import ilium:host/clock@0.1.0;
  import ilium:host/storage@0.1.0;
  import ilium:host/codec@0.1.0;
  import ilium:host/fs@0.1.0;
  import ilium:host/http@0.1.0;
  import ilium:host/jobs@0.1.0;
  import ilium:host/audio@0.1.0;
  import ilium:host/media@0.1.0;
  import ilium:host/location@0.1.0;
  import ilium:host/geocode@0.1.0;
  import ilium:host/gpu@0.1.0;
  import ilium:anim/surface@0.1.0;
  import ilium:anim/fonts@0.1.0;
  export ilium:exports/lifecycle@0.1.0;
  export ilium:exports/renderer@0.1.0;
}

world with-jobs { export ilium:exports/job-runner@0.1.0; }
```

The umbrella identity is `ilium:plugin@0.1.0`. It is not a WIT package. It is the value of the manifest `api` field and of the `ilium:api-version` custom section, and the host compares it before linking [ABI-021].

World fragments with the same name in different packages are separate worlds. The SDK composes a base and its fragments into one component world, and the manifest `exports` list has to match the exports of that component.

## 10. Mapping rules for Rust and TypeScript

The SDKs generate their types from this page. The rules below are the only naming and shape conversions [ABI-020].

| WIT | Rust | TypeScript |
|---|---|---|
| `kebab-case` function | `snake_case` function in a module named for the interface | `camelCase` function in a namespace for the interface |
| `record` | `struct` with `snake_case` fields | `interface` with `camelCase` fields |
| `variant` | `enum` with one tuple field per payload case | tagged union `{ tag: 'case-name', val: T }` |
| `enum` | fieldless `enum` | string union of the case names |
| `flags` | struct of `bool` fields with `bitflags` style operators | object of optional booleans |
| `option<T>` | `Option<T>` | `T or undefined` |
| `result<T, E>` | `Result<T, E>` | returns `T`, throws `PluginError` for `plugin-error` |
| `list<u8>` | `Vec<u8>` | `Uint8Array` |
| `u64`, `s64` | `u64`, `i64` | `bigint` |
| `u8`, `u16`, `u32`, `s16`, `s32`, `f32`, `f64` | the same width | `number` |
| `tuple<A, B>` | `(A, B)` | `[A, B]` |
| `resource` | owned handle type with a `Drop` | class with `[Symbol.dispose]` |
| `%name` | the Rust keyword is escaped as `r#name` where needed | the same name |

A `take-*` function returns `Option<T>` in Rust and `T or undefined` in TypeScript. A function with no `result` cannot fail; a trap is not catchable.

### Wrapper names in the SDKs

The conversions above change names only. The SDKs add two wrapper forms on top of them, and no others.

| WIT function | Rust | TypeScript | Meaning |
|---|---|---|---|
| `fetch` returns `result<request-id, plugin-error>` | `http::fetch(request).await` | `await fetch(request)` | The plain name starts the call and awaits the outcome. It returns the typed value. |
| the same function | `http::fetch_request(request)` | `fetchRequest(request)` | The `_request` / `Request` form starts the call and returns the `RequestId` at once, so the plugin can await it later or complete it by hand. |
| `get-json`, `set-json`, `get-plugin-json`, `set-plugin-json`, and any function that takes `payload-json` | `settings::get_json()`, `settings::set_json(&patch)` | `getJson()`, `setJson(patch)` | A name that ends in `-json` keeps the suffix as `_json` / `Json`. The value is JSON text: a Rust `String`, a TypeScript `string`. The SDK never parses it. |

- **[ABI-051]** Every WIT function that returns `request-id` MUST have an awaiting wrapper with the WIT name and a starting wrapper with the suffix `_request` in Rust and `Request` in TypeScript. A WIT name that ends in `-json` MUST keep the suffix as `_json` in Rust and `Json` in TypeScript.

## 11. Where each name is defined

Every type is defined once, with two deliberate exceptions: `minimap-mark` is declared in `ilium:types/ui` (for `minimap-spec`) and in the `editor` primitive interface (for `set-minimap-marks`), and `menu-entry` is declared in `ilium:types/ui` (for `menu-spec`) and in the `menu-provider` export interface (for `menu-items`). A package cannot use a type from a package that depends on it, so each pair stays as two records with identical fields, and an SDK exposes each pair as one type. This index names the owner of each group of names. A page that needs a type links to the section.

| Group | Interface | Section |
|---|---|---|
| Identifiers | `ilium:types/ids` | 4.1 |
| `plugin-error`, `error-code` | `ilium:types/errors` | 4.2 |
| Colour, style, rich text, tone, tooltip | `ilium:types/text` | 4.3 |
| `fs-path` | `ilium:types/paths` | 4.4 |
| Buffers, positions, edits | `ilium:types/editing` | 4.5 |
| Constraints, direction, flex | `ilium:types/layout` | 4.6 |
| Geographic points and places | `ilium:types/geo` | 4.7 |
| Tree nodes, panes, agents | `ilium:types/tree` | 4.8 |
| Widgets, view trees, widget state, pointer, actions | `ilium:types/ui` | 4.9 |
| `outcome`, `request-ref`, `request-result` | `ilium:types/requests` | 4.10 |
| `intent`, `submission` and every spec | `ilium:types/intents` | 4.11 |
| `event` and every payload | `ilium:types/events` | 4.12 |
| `activation`, `deactivation-reason` | `ilium:types/lifecycle` | 4.13 |
| Host calls | `ilium:host/*` | 5.1 to 5.39 |
| Terminal, agent, editor, basic | `ilium:primitives/*` | 6.1 to 6.4 |
| Scene surface, fonts, renderer types | `ilium:anim/*` | 7.1 to 7.3 |
| Everything a plugin implements | `ilium:exports/*` | 8.1 to 8.16 |
| Worlds | `ilium:server`, `ilium:client`, `ilium:anim` | 9 |

