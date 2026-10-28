# intents: toast, status segments, open-external

## Toast, status segments, frame slots, open-external, titles and decorations

These intents change the kernel's own chrome without letting a plugin draw it. A plugin sends a toast, writes a segment in the status bar, fills a slot in a pane frame, opens a link in the desktop handler, marks a node as needing attention, and decorates tree rows with rich, animated content. The kernel decides placement, timing, colour mapping and policy.

**You need:** `notify.toast` for `toast` and `dismiss-toast`; `ui.chrome` for the status segment and frame slot intents, together with the matching `status_segments` and `pane_frame_slots` manifest entries; `open-external` for `open-external`; `tree.attention` for the attention marks; `tree.decorate` for `set-decoration`, `clear-decoration` and `set-inferred-icon`; `tree.write` for `rename-node`. See [intents](/pages/api-intents/) for batches and keys, [notifications](/pages/api-notifications/) for desktop notices, [tree.decorate](/pages/capability-tree-decorate/) and [ui.panels](/pages/capability-ui-panels/).

```json
"capabilities": { "client": ["notify.toast", "ui.chrome", "open-external"] },
"status_segments": [{ "id": "quota" }]
```

### Records

```wit
enum tone { info, success, warning, danger, muted }
record toast-action { label: string, command: string }
record toast-spec {
  id: option<string>, tone: tone, title: string,                    // title at most 60 characters
  body: option<rich-text>,                                          // at most 200 characters
  actions: list<toast-action>,                                      // at most 2
  duration-ms: option<u32>,                                         // 1,000 to 30,000
}
variant external-target { url(string), file(fs-path), settings(string) }
record open-external-spec { target: external-target }
record set-status-segment-spec { segment: string, content: rich-text, tooltip: option<tooltip>, tone: option<tone> }
record set-frame-slot-spec { pane: node-id, slot: string, content: rich-text, icon: option<string>, tooltip: option<tooltip> }
record clear-frame-slot-spec { pane: node-id, slot: string }
record mark-attention-spec { node: node-id, reason: string }        // at most 80 characters
record set-inferred-icon-spec { node: node-id, icon: option<string>, source: string }
enum title-source { plugin, inferred }
enum decoration-visibility { always, on-hover }
```

### Toast

A toast is a short message that the kernel draws near the corner of the client and removes by itself. `duration-ms` of none means until dismissed for the `danger` tone and 5,000 ms for the others. A toast with an `id` that is already shown replaces it. The action buttons run a command: its arrival is a `command-invoked` event with the origin `toast`. At most 3 toasts per 10 s per plugin (`rate-limited`, with `retry-after-ms`) and `toasts_pending` is 3, replacing the oldest.

**Rust**

```rust
let toast = Intent::Toast(ToastSpec {
    id: Some("deploy".into()),
    tone: Tone::Danger,
    title: "Deploy failed".into(),
    body: Some(RichText::plain("exit status 2 on api-deploy")),
    actions: vec![ToastAction { label: "Open log".into(), command: "com.example.deploy/open-log".into() }],
    duration_ms: None,
});
let dismiss = Intent::DismissToast("deploy".into());
```

**TypeScript**

```ts
const toast = { tag: 'toast', val: {
  id: 'deploy', tone: 'danger', title: 'Deploy failed',
  body: [{ text: 'exit status 2 on api-deploy', style: {}, target: { tag: 'none' }, tooltip: undefined }],
  actions: [{ label: 'Open log', command: 'com.example.deploy/open-log' }],
  durationMs: undefined,
} } as const;
const dismiss = { tag: 'dismiss-toast', val: 'deploy' } as const;
```

### Status segments and frame slots

`set-status-segment` writes one of the plugin's declared `status_segments` (at most 2 per plugin). `set-frame-slot` fills one of its `pane_frame_slots` (at most 4) on a pane: the slot's `placement` is `header-icon`, `header-text`, `footer-row` or `banner`, and `applies_to` limits the panes. Both are limited to 10 per second per slot. A pane that does not match `applies_to` gives `refused(invalid-argument)`. The `clear-` intents remove the content. A click on a rich span with a `command` target arrives as `command-invoked` with the origin `status-segment` or `frame-slot`.

**Rust**

```rust
let segment = Intent::SetStatusSegment(SetStatusSegmentSpec {
    segment: "quota".into(),
    content: RichText::plain("quota 82%"),
    tooltip: None,
    tone: Some(Tone::Warning),
});
let slot = Intent::SetFrameSlot(SetFrameSlotSpec {
    pane, slot: "branch".into(), content: RichText::plain("main"), icon: Some("🌿".into()), tooltip: None,
});
```

**TypeScript**

```ts
const segment = { tag: 'set-status-segment', val: {
  segment: 'quota', content: richText('quota 82%'), tooltip: undefined, tone: 'warning' } } as const;
const slot = { tag: 'set-frame-slot', val: {
  pane, slot: 'branch', content: richText('main'), icon: '🌿', tooltip: undefined } } as const;
```

### open-external

`open-external` hands a target to the desktop. `url` accepts `http`, `https` and `mailto` only. `file` opens a path with the desktop handler. `settings` takes an `ilium://settings/<tab-id>#<key>` link and opens the Settings screen at that row, which is how a plugin points at a kernel setting it may not write. The first time a plugin opens a target origin the user is asked, and the answer is remembered for that plugin. A declined request is `refused(not-permitted)`. There is no separate allowlist of targets: the only checks are the scheme list above (a URL with any other scheme is `refused(not-permitted)`, SEC-043) and the per-origin consent, and both run in the kernel, never in the plugin.

**Rust**

```rust
let url = Intent::OpenExternal(OpenExternalSpec { target: ExternalTarget::Url("https://status.example.com".into()) });
let settings = Intent::OpenExternal(OpenExternalSpec { target: ExternalTarget::Settings("ilium://settings/keyboard#chords".into()) });
```

**TypeScript**

```ts
const url = { tag: 'open-external', val: { target: { tag: 'url', val: 'https://status.example.com' } } } as const;
const settings = { tag: 'open-external', val: { target: { tag: 'settings', val: 'ilium://settings/keyboard#chords' } } } as const;
```

### Titles and icons: rename-node source and inferred icons

`rename-node` gains `source` (`plugin` or `inferred`) and `expected-revision`. A stale `expected-revision` is `refused(conflict)`. A title with the source `inferred` never replaces a title the user set by hand, so an automatic titler cannot overwrite a manual name. `set-inferred-icon` follows the same rule for icons: a manual icon wins and the intent is `refused(conflict)`. `icon` is a catalogue id or a glyph, and none clears the inferred icon. `source` names the origin of the inference.

**Rust**

```rust
use ilium_plugin_sdk::types::tree::TitleSource;

let rename = Intent::RenameNode(RenameNodeSpec {
    node, title: "Fix auth".into(), source: TitleSource::Inferred, expected_revision: Some(view.revision),
});
let icon = Intent::SetInferredIcon(SetInferredIconSpec { node, icon: Some("🔧".into()), source: "titler".into() });
```

**TypeScript**

```ts
const rename = { tag: 'rename-node', val: {
  node, title: 'Fix auth', source: 'inferred', expectedRevision: view.revision } } as const;
const icon = { tag: 'set-inferred-icon', val: { node, icon: '🔧', source: 'titler' } } as const;
```

### Attention marks

`mark-attention` flags a node with a `reason` of at most 80 characters. `ack-attention` clears it. The kernel keeps the attention policy: the done prefix and the sound are not plugin controls. Every change produces `attention-changed { node, marked, reason, by }` for plugins with `tree.attention` or `tree.read`, coalesced by node.

**Rust**

```rust
let mark = Intent::MarkAttention(MarkAttentionSpec { node, reason: "build needs review".into() });
let ack = Intent::AckAttention(node);
```

**TypeScript**

```ts
const mark = { tag: 'mark-attention', val: { node, reason: 'build needs review' } } as const;
const ack = { tag: 'ack-attention', val: node } as const;
```

### Rich decorations

`set-decoration` also takes these optional fields: `spans: option<rich-text>` (clickable styled text), `tooltip`, `frames: list<styled-text>` (2 to 16 frames), `frame-ms: u16` (at least 90), `trailing: bool` (placed after the title), `visibility` (`always` or `on-hover`) and `tone`. The host steps the frames with no plugin call per frame. With reduced motion the first frame is shown. Frames outside 2 to 16 or a `frame-ms` under 90 is `invalid-argument`.

**Rust**

```rust
let deco = Intent::SetDecoration(SetDecorationSpec {
    node, slot: "build".into(), content: styled("building"), icon: None, sort_key: None, persist: false,
    spans: None,
    tooltip: Some(Tooltip::text("Build running")),
    frames: vec![styled("⠋"), styled("⠙"), styled("⠹"), styled("⠸")],
    frame_ms: 100,
    trailing: true,
    visibility: DecorationVisibility::Always,
    tone: Some(Tone::Info),
});
```

**TypeScript**

```ts
const deco = { tag: 'set-decoration', val: {
  node, slot: 'build', content: styled('building'), icon: undefined, sortKey: undefined, persist: false,
  spans: undefined,
  tooltip: tooltip('Build running'),
  frames: ['⠋', '⠙', '⠹', '⠸'].map(styled), frameMs: 100,
  trailing: true, visibility: 'always', tone: 'info',
} } as const;
```

<figure>
<svg viewBox="0 0 420 140" role="img" aria-label="Which side draws what for chrome intents">
  <defs>
    <marker id="chrome-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="40" width="100" height="60" rx="6" fill="none" stroke="currentColor"/>
  <text x="56" y="64" font-size="11" text-anchor="middle" fill="currentColor">Plugin</text>
  <text x="56" y="80" font-size="10" text-anchor="middle" fill="currentColor">content, tone, id</text>
  <rect x="160" y="40" width="100" height="60" rx="6" fill="none" stroke="currentColor"/>
  <text x="210" y="64" font-size="11" text-anchor="middle" fill="currentColor">Kernel</text>
  <text x="210" y="80" font-size="10" text-anchor="middle" fill="currentColor">checks, rates, policy</text>
  <rect x="314" y="8" width="100" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="364" y="30" font-size="10" text-anchor="middle" fill="currentColor">toast, status bar</text>
  <rect x="314" y="52" width="100" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="364" y="74" font-size="10" text-anchor="middle" fill="currentColor">pane frame, tree</text>
  <rect x="314" y="96" width="100" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="364" y="118" font-size="10" text-anchor="middle" fill="currentColor">desktop handler</text>
  <line x1="106" y1="70" x2="158" y2="70" stroke="currentColor" marker-end="url(#chrome-arrow)"/>
  <line x1="260" y1="60" x2="312" y2="28" stroke="currentColor" marker-end="url(#chrome-arrow)"/>
  <line x1="260" y1="70" x2="312" y2="70" stroke="currentColor" marker-end="url(#chrome-arrow)"/>
  <line x1="260" y1="80" x2="312" y2="114" stroke="currentColor" marker-end="url(#chrome-arrow)"/>
</svg>
<figcaption>The plugin supplies content. The kernel draws, throttles and applies policy.</figcaption>
</figure>

### Errors and limits

| Condition | Result |
|---|---|
| 4th toast in 10 s | `refused(rate-limited)` with `retry-after-ms` |
| Title over 60 characters, body over 200, more than 2 actions, duration outside 1,000 to 30,000 ms | `refused(invalid-argument)` |
| More than 10 `set-status-segment` or `set-frame-slot` per second per slot | `refused(rate-limited)` |
| Segment or slot id not declared by the plugin | `refused(not-found)` |
| Pane does not match `applies_to` | `refused(invalid-argument)` |
| `open-external` declined, or a scheme other than `http`, `https`, `mailto` | `refused(not-permitted)` |
| `mark-attention` reason over 80 characters | `refused(invalid-argument)` |
| Manual title or icon in place | `refused(conflict)` |
| Frames outside 2 to 16, `frame-ms` under 90 | `refused(invalid-argument)` |

### What the user sees

The toast shows as a rounded panel in the corner with a countdown line that uses the filled and unfilled bar glyphs of the client. A status segment is part of the status bar pill. A frame slot sits in the pane header or footer. An attention mark changes the tree row.

<pre class="term"><span class="t-err">╭  ≡ ● · Deploy failed───────────────────╮</span>
<span class="t-err">│ exit status 2 on api-deploy            │</span>
<span class="t-err">│ Open log                               │</span>
<span class="t-err">│ ━━━━━━━━━━━━━━━━━━━━───────────────────│</span>
<span class="t-err">╰────────────────────────────────────────╯</span>

<span class="t-hi">ℹ NORMAL  -  quota 82%  -  3 panes</span>
</pre>

Illustrative, not generated: the border is the client's rounded panel, the countdown uses the toast gauge glyphs of the client, and the status line uses the segment separator. The toast layout and the colour mapping are the kernel's, and this composition is not a capture.

### See also

- [Intents](/pages/api-intents/) and [events](/pages/api-events/) (`command-invoked` origins, `attention-changed`).
- [Notifications](/pages/api-notifications/), [UI](/pages/api-ui/), [tree.decorate](/pages/capability-tree-decorate/) and [tree.write](/pages/capability-tree-write/).
- [Tree structure](/pages/api-intent-tree-structure/) and [pane lifecycle](/pages/api-intent-pane-lifecycle/).

### Security

Chrome intents are sanitised ([SEC-380]).
