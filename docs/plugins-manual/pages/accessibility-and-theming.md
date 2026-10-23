---
layout: page
title: Theming and readable output
permalink: /pages/accessibility-and-theming/
---

## Theming and readable output

Plugins never choose the exact colour of their text. A view names a colour role, such as `danger` for a failed check, and the host paints that role with the theme tokens of the dark or light theme the user selected. One plugin build therefore renders correctly in both themes. This page lists the roles, the theme extras (reduced motion, density and icon mode), the colour roles of every widget family including the status and feedback widgets, and the styled-text fields. It shows one complete view in Rust and in TypeScript, and states which part of readability the host guarantees and which part stays with the plugin.

### You will need

- **Grants.** The styling fields need no grant. `theme()` and `display-width()` belong to the `ui` interface, so the plugin needs the `ui.panels` or `panes.provide-kind` grant of the view it draws.
- **Side and exports.** Views are drawn on the client. Use `side = "client"` in Rust or `side: 'client'` in TypeScript, and export `lifecycle`, `handler` and `views`.
- **Widgets.** The `paragraph`, `leader-row` and `markdown` widgets of [Text widgets](/pages/widgets-text/) carry the styled text. Forms use the leader rule described in [Building settings screens](/pages/settings-screens/).
- **Status and feedback widgets.** `badge`, `spinner`, `notice`, `icon`, `meter`, `progress-row`, `diff-view` and `minimap` take their colours from roles or from a `tone`. See [Widget colour roles](#widget-colour-roles).
- **Event.** `theme-changed`, listed in [Events and intents](/pages/reference-events-and-intents/).

## Colour roles

The host gives every view one `theme-tokens` record. A role names a token, so a plugin never needs the palette itself.

```wit
enum color-role { foreground, muted, accent, success, warning, danger, info, selection, border }

record theme-tokens {
  foreground: u32, background: u32, accent: u32, muted: u32,
  success: u32, warning: u32, danger: u32,
  is-dark: bool, stable-glyphs: bool
}
```

| Role | Painted with | Use it for |
|---|---|---|
| `foreground` | `foreground` token | Primary text |
| `muted` | `muted` token | Help lines, hints, secondary values |
| `accent` | `accent` token | Focus and the active tab |
| `success` | `success` token | A healthy plugin, a passed check |
| `warning` | `warning` token | Attention that is not a failure |
| `danger` | `danger` token | A failure or a destructive action |
| `info` | `info` token of `theme-extras` | Neutral information; the `ℹ` tone of a `notice` |
| `selection` | `selection` token of `theme-extras`, reversed | The highlighted row of a list or table |
| `border` | `border` token of `theme-extras` | Block borders, separators, dividers and the scrollbar track |

Colour tokens are packed `0x00RRGGBB` numbers. `background` is not a role. The host paints the panel background, and a plugin cannot choose it. Read `is-dark` only when a choice cannot be expressed with a role, for example when a scene picks a natural image for the theme.

## Theme extras

`theme-tokens` holds the seven base colours. The contract also defines `theme-extras`, a second record read with `theme-extras()`. It carries the three colours that the roles `info`, `selection` and `border` use, and three settings that change how a view should behave.

```wit
record theme-extras { reduced-motion: bool, density: density, info: u32, selection: u32, border: u32, icon-mode: icon-mode }
enum density { compact, normal, comfortable }
enum icon-mode { normal, stable }   // stable only when the user opted in
theme-extras: func() -> theme-extras;   // in host::ui
```

| Field | Meaning | What a plugin does with it |
|---|---|---|
| `reduced-motion` | The user asked for less motion. | Stop its own timers that animate. The host already shows one static frame in `spinner`, `progress-row` busy state and decoration frame animation, and draws no hover or scroll animation. A change of the setting delivers `theme-changed`. |
| `density` | `compact`, `normal` or `comfortable`. | Choose row padding, for example one blank row between groups only in `comfortable`. |
| `info`, `selection`, `border` | Packed `0x00RRGGBB` tokens. | Rarely needed: roles already use them. Read them for a `canvas` or a `surface` that draws its own frame. |
| `icon-mode` | `stable` only after the user turned on stable glyphs. | Keep icon names unchanged. The host swaps the glyphs. `theme().stable-glyphs` is derived and always equals `icon-mode == stable`. |

**Rust**

```rust
use ilium_plugin_sdk::host::ui::{self, Density};

fn row_gap_rows() -> u16 {
    let extras = ui::theme_extras();
    if extras.reduced_motion {
        stop_own_animation_timer();
    }
    match extras.density {
        Density::Compact => 0,
        Density::Normal => 0,
        Density::Comfortable => 1,
    }
}
```

**TypeScript**

```ts
import { themeExtras } from '@ilium/plugin-sdk/host/ui';

function rowGapRows(): number {
  const extras = themeExtras();
  if (extras.reducedMotion) stopOwnAnimationTimer();
  return extras.density === 'comfortable' ? 1 : 0;
}
```

`stopOwnAnimationTimer` stands for your own code that cancels a timer from the `clock` import. Read the extras again after `theme-changed`.

## Widget colour roles

The host paints every widget from roles, so a plugin that uses the widgets and tones below needs no colour of its own. The table is the colour column of the widget reference, for the widgets that carry state.

| Widget | Roles and tones | Glyphs the host draws |
|---|---|---|
| `block`, `separator`, `split` | `border`; `accent` for a focused block and a dragged divider | rounded corners `╭╮╰╯`, `─`, `│` |
| `tabs` | `muted`, `accent` (selected), `border` | divider ` │ ` |
| `%list`, `table`, `tree-list`, `menu` | `selection` for the highlighted row, `muted` for hints and disabled items, `danger` for danger items | highlight symbol, submenu `›` |
| `diff-view` | `success` added, `danger` removed, `muted` context and gutter | gutter `+`, `-` |
| `gauge`, `line-gauge`, `meter` | `accent` filled, `muted` remainder; `meter` takes the `tone` of each segment | `█`, `░`, `━`, `─` |
| `progress-row` | `warning` error, `muted` waiting, `accent` running, `success` done | `!`, `○`, `▶`, `✓` |
| `badge` | the `tone` | `●` or a short label |
| `notice` | the `tone` and `border` | `ℹ`, `✓`, `!`, `✗` |
| `spinner` | `accent`; one static frame with reduced motion | `⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏` |
| `toggle`, `check-list` | `success` on, `muted` off or disabled, `accent` focus | `[x]`, `[ ]`, `[-]` |
| `text-input`, `path-field` | `danger` for an invalid or missing value, `muted` placeholder | `[ value ]`, `••••` for secrets |
| `key-capture` | `accent` while capturing, `warning` on conflict | `⌨ press keys…` |
| `leader-row` | `foreground` label, `muted` leaders | `…` |

The `tone` values that the widgets accept are `info`, `success`, `warning`, `danger` and `muted`; the settings `notice` row accepts only `info`, `success`, `warning` and `danger`. They map to the roles of the same name, so the dark and the light theme both stay readable.

<figure>
<pre class="term">╭  ≡ ● · Roles at work ────────────────────────╮
│ <span class="t-hi">ℹ</span> The provider sends place names off site.   │
│ <span class="t-warn">!</span> Quota is at 92 percent.                    │
│ <span class="t-err">✗</span> The index build failed.                    │
│ <span class="t-ok">✓</span> The index was built.                       │
│ <span class="t-hi">⠋</span> Indexing 412 sessions                      │
│ <span class="t-ok">▶ RUNNING</span>  37%  <span class="t-hi">█████████</span><span class="t-dim">░░░░░░░░░░░░░░░</span>     │
│ Today ………………………………………………… <span class="t-ok">████████████</span><span class="t-err">██</span><span class="t-dim">░░░░</span> │
│ <span class="t-ok">+ added line</span>                                 │
│ <span class="t-err">- removed line</span>                               │
╰──────────────────────────────────────────────╯</pre>
<figcaption>Illustrative, not generated. A notice in the info, warning, error and success tone, a spinner, a progress row, a meter with a danger segment and the diff colours.</figcaption>
</figure>

## Styled text

Every visible string in a view is `styled-text`, a list of spans. A span holds text and a style. A style has an optional foreground colour, an optional background colour, and two modifier sets: one to add and one to remove.

```wit
// Excerpt of ilium:types/text in reference-wit
variant color { role(color-role), rgb(u32), indexed(u8) }
flags modifiers {
  bold, dim, italic, underlined, slow-blink, rapid-blink, reversed, hidden, crossed-out
}
record style { fg: option<color>, bg: option<color>, add: modifiers, remove: modifiers }
record span { text: string, style: style }
```

An empty style inherits the theme. A colour given as `rgb` or `indexed` is painted exactly as given. Use it only where a scene or an editor needs a natural colour. The host guarantees a contrast ratio of at least 4.5:1 between each of the seven text roles (`foreground`, `muted`, `accent`, `success`, `warning`, `danger`, `info`) and the theme background, in every built-in theme; a theme test computes the ratio from the token values. `selection` and `border` are a background and a line, not text; this page states no ratio for them. This guarantee covers roles only. A plugin that picks its own colours is responsible for contrast on both backgrounds.

## The full surface

| Name | Kind | Meaning |
|---|---|---|
| `theme()` | function in `host::ui` | Returns the current `theme-tokens`. It never fails. Read it again after `theme-changed`. |
| `display-width(text)` | function in `host::ui` | Returns the cell width the host uses for the text, as `u16`. Call it before you cut text yourself. |
| `theme-extras()` | function in `host::ui` | Returns reduced motion, density, icon mode and the `info`, `selection` and `border` tokens. Rust `ui::theme_extras()`, TypeScript `themeExtras()`. |
| `invalidate(view)` | function in `host::ui` | Draws the view again with the current tokens. |
| `theme-changed` | client event, no payload | Sent once for a burst of theme changes. Always delivered on the client. |
| `color-role` | enum | The nine roles of the enum above (seven text roles, plus `selection` and `border`). |
| `color` | variant | `role(color-role)`, `rgb(u32)` or `indexed(u8)`. |
| `style`, `span`, `modifiers` | records and flags | The fields described above. |
| `paragraph` | widget | `lines: styled-lines`, `wrap`, `alignment`, `style`, `scrollable`. |
| `leader-row` | widget | `label: styled-text` and `value: styled-text`, joined by the `…` leaders. |
| `markdown` | widget | `source: string` and `link-command`, rendered by the host's Markdown renderer. |
| `stable-glyphs` | flag in `theme-tokens` | `true` when the user chose the opt-in stable glyph mode. |

## Worked example: a checks panel

This client view shows the number of failing checks on a leader row, a muted note, and a redraw when the theme changes. The Rust version comes first, the TypeScript version follows.

**Rust**

```rust
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::host::ui;
use ilium_plugin_sdk::types::layout::Area;
use ilium_plugin_sdk::types::text::{ColorRole, Modifiers, Style};
use ilium_plugin_sdk::types::ui::{UiTree, ViewTarget};
use ilium_plugin_sdk::ui::build::Tree;

#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
struct ChecksPanel {
    failed_checks: u32,
    last_view: Option<ViewTarget>,
}

impl Lifecycle for ChecksPanel {}

impl Handler for ChecksPanel {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let theme_changed = batch.iter().any(|event| matches!(event, Event::ThemeChanged));
        if let (true, Some(view)) = (theme_changed, self.last_view.clone()) {
            ui::invalidate(view);
        }
        Vec::new()
    }
}

impl Views for ChecksPanel {
    fn render(&mut self, view: ViewTarget, _area: Area) -> UiTree {
        self.last_view = Some(view);
        let mut tree = Tree::new();
        let value = vec![Span::styled(
            format!("{} failing", self.failed_checks),
            Style::role(ColorRole::Danger).add(Modifiers::BOLD),
        )];
        let row = tree.leader_row(vec![Span::plain("Failed checks")], value);
        let note = tree.paragraph(vec![vec![Span::styled(
            "Run the checks again to refresh.",
            Style::role(ColorRole::Muted),
        )]]);
        let panel = tree.block().title("Checks").child(row).child(note);
        tree.finish(panel)
    }
}
```

The panel names roles only. The danger colour is bold, and the note is muted, so both read on the dark and the light background. The handler keeps the last view target and redraws it when `theme-changed` arrives.

**TypeScript**

The TypeScript version uses the same roles. It reads `theme()` to show the active theme name, and it redraws on `theme-changed`.

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { Event, Submission } from '@ilium/plugin-sdk/types';
import { invalidate, theme } from '@ilium/plugin-sdk/host/ui';
import { block, leaderRow, paragraph, tree } from '@ilium/plugin-sdk/ui/build';
import { role, span, type ViewTarget } from '@ilium/plugin-sdk/types';

const FAILED_CHECKS = 3;
let lastView: ViewTarget | undefined;

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate() {},
  onEvents(batch: Event[]): Submission[] {
    const themeChanged = batch.some((event) => event.tag === 'theme-changed');
    if (themeChanged && lastView !== undefined) invalidate(lastView);
    return [];
  },
  render(view: ViewTarget) {
    lastView = view;
    const { isDark } = theme();
    const value = [span(`${FAILED_CHECKS} failing`, { fg: role('danger'), add: { bold: true } })];
    const row = leaderRow([span('Failed checks')], value);
    const note = paragraph([[span(isDark ? 'Dark theme' : 'Light theme', { fg: role('muted') })]]);
    return tree(block({ title: 'Checks' }, [row, note]));
  },
});
```

The TypeScript field `isDark` is the camel-case form of `is-dark`. The `add` object uses the modifier names as keys, so `{ bold: true }` sets bold.

## Glyphs and icon widths

The host draws icons with their normal UTF-8 glyphs and measures them with their real cell width. A plugin must not replace an icon with a plain stable glyph to fix a width problem. The stable-glyph mode is an explicit user setting, `ui.stable_glyphs`, and the `stable-glyphs` flag tells the plugin that the user chose it. Icon names in the manifest, such as the `icon` of a command or a settings page, are Tabler icon names from the host icon set. The `icon` widget takes a `slot` of the `ilium.icons` catalogue and follows the icon preset the user chose, so a plugin never hard-codes an emoji for a state that the host already has a slot for.

## The leader rule

A label and a value far apart are joined by grey dotted leaders made of the `…` character, one cell each. The `leader-row` widget applies the rule for you.

- One blank cell sits on each side of the dots.
- A gap shorter than five cells stays blank.
- The leaders never overwrite label text, glyphs or values.
- The leaders are inert. They are never part of a click or hit target.
- When label and value do not fit the row, the value is cut first, with `…`.

A plugin that builds its own row with spaces produces a gap that does not match the host's rows, so use `leader-row` for every label and value pair that can sit far apart.

## Markdown and terminal output

A `markdown` widget takes a source string and renders it with the host's Markdown renderer, so headings, lists and links match the rest of Ilium. Plugins never send raw ANSI escape sequences. The host removes them from every text field and logs one warning per view. Use `styled-text` and the roles instead.

## What the user sees

The dark theme is the default. Dark renders stay dark: the host never draws a white card inside the dark theme. Panels have rounded corners, and the title carries the host's title icons.

<figure>
<pre class="term">╭  ≡ ● · Checks ───────────────────────────────╮
│ Failed checks …………………………………………………… <span class="t-err">3 failing</span> │
│ <span class="t-dim">Run the checks again to refresh.</span>             │
╰──────────────────────────────────────────────╯</pre>
<figcaption>Illustrative, not generated. The checks panel in the dark theme, 48 columns by 4 rows. The danger role marks the count, and the muted role marks the note. The light theme draws the same characters.</figcaption>
</figure>

The light theme shows the same widgets with the same roles on a light background. The layout does not change, so a plugin that is correct in one theme needs no second layout. Only the colours of the border, the text and the roles differ, so the same characters appear in the render above.

<figure>
<svg viewBox="0 0 360 120" role="img" aria-label="Plugin roles pass through the host paint to two themes">
  <title>Plugin roles, host paint, two themes</title>
  <desc>A plugin builds a ui-tree that names colour roles. The host paints the tree with the theme tokens, once for the dark theme and once for the light theme.</desc>
  <defs>
    <marker id="theme-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
      <path d="M0 0 L10 5 L0 10 z" fill="currentColor"/>
    </marker>
  </defs>
  <rect x="6" y="44" width="88" height="32" rx="6" fill="none" stroke="currentColor" stroke-dasharray="4 3"/>
  <text x="50" y="64" text-anchor="middle" fill="currentColor" font-size="11">Plugin tree</text>
  <rect x="122" y="44" width="110" height="32" rx="6" fill="none" stroke="currentColor"/>
  <text x="177" y="64" text-anchor="middle" fill="currentColor" font-size="11">Host paint</text>
  <rect x="262" y="12" width="92" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="308" y="32" text-anchor="middle" fill="currentColor" font-size="11">Dark theme</text>
  <rect x="262" y="78" width="92" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="308" y="98" text-anchor="middle" fill="currentColor" font-size="11">Light theme</text>
  <path d="M96 60 L118 60" stroke="currentColor" fill="none" marker-end="url(#theme-arrow)"/>
  <path d="M234 54 L256 32" stroke="currentColor" fill="none" marker-end="url(#theme-arrow)"/>
  <path d="M234 66 L256 88" stroke="currentColor" fill="none" marker-end="url(#theme-arrow)"/>
</svg>
<figcaption>The plugin names roles, never colours. The host paints the same tree once per theme, so both themes come from one plugin build.</figcaption>
</figure>

## Sound and notices are never the only signal

Meaning carried by sound, a desktop notice or a toast MUST also be carried by text the user can read in Ilium. See [sound.play](/pages/capability-sound-play/), [notify.desktop](/pages/capability-notify-desktop/) and [notify.toast](/pages/capability-notify-toast/).

| ID | Rule | Exact value | Source |
|---|---|---|---|
| CAP-884 | Channels are independent | Turning off sound, desktop notices or toasts, or a channel failing, MUST leave the text signal of the same event unchanged. Colour or sound alone never carries a state: every state has a text label | `src/docs/notifications.md` |

## Edge cases and errors

| Case | What happens |
|---|---|
| Text contains an ESC sequence | The host removes the sequence and logs one warning per view at level warn. |
| `rgb` colour with weak contrast | The host paints it as given. The plugin owns that contrast. |
| Label and value do not fit a `leader-row` | The value is cut first, with `…`, then the label. |
| Badge text wider than 12 cells | The host cuts it and ends it with `…`. Widths use `display-width` rules. |
| Theme changes during a render | The next render uses the new tokens. Bursts of `theme-changed` arrive as one event. |
| Stable-glyph mode is on | The host draws the stable glyphs. The plugin keeps its own glyph names unchanged. |
| Reduced motion is on | `spinner` shows one static frame. A plugin timer that animates should stop. |
| Tree above its limit (`ui_tree_nodes`, default 4,096 nodes; `ui_tree_kib`, default 1,024 KiB) | The plugin traps. Repeated traps count toward the restart and disable rule. A first-party (T1) plugin may raise the limits through `limits` in its manifest, up to 16,384 nodes and 4,096 KiB. |
| `render` runs past its deadline (default 10 ms) | The render is a trap, like any other `views.render` failure. |

A T1 plugin may raise the render deadline through `render_deadline_ms`, up to 20 ms. The styling functions return no error code. `theme()` and `display-width()` always answer, and styled-text fields have no validation error. Limits show up only as traps or as cut text.

## Checks before you ship

- Open the view in the dark and in the light theme. Each status mark must stay readable against the background.
- Check that every label and value pair that can sit far apart uses `leader-row`.
- Turn on the stable-glyph setting and confirm that icon widths still line up.
- Handle `theme-changed` by calling `invalidate` for each open view, so the view is rendered again with the new tokens.
- Search the plugin for `rgb` and `indexed` colours. Each one needs its own contrast check.
- Turn on reduced motion and confirm that nothing in the view still moves.
- Check that every state has a glyph or a word as well as a colour, for example `✓` and `✗` with a `notice`, so the state reads without colour.
- Run the screen-reader check in [USR-951](#production-additions-t7-users-privacy-a11y) on each shipped platform, with the reader that the release names for that platform (USR-950).

### What the contract does not provide

`ilium:plugin@0.1.0` has no accessible-label, screen-reader or high-contrast import, and no locale import. The terminal has no accessibility tree, so the plugin carries meaning in text. A plugin MUST NOT use colour as the only carrier of meaning, and MUST give every status a word or a glyph beside its colour role. [AUT-501] A plugin MUST read `reduced-motion` and stop its own animating timers when it is true. [AUT-502] A plugin MUST work in `compact` density at 40 columns and in both icon modes, and MUST end every icon-only control with a text label that stays when icons are not drawn. [AUT-503] A plugin MUST use the nine colour roles and avoid `rgb` and `indexed` colours, because only a role follows the theme. [AUT-504] The list for release time is in [Quality checklist](/pages/guide-quality-checklist/), and width rules for other scripts are in [Internationalisation](/pages/guide-internationalisation/).

### See also

- [Quality checklist](/pages/guide-quality-checklist/)
- [Building settings screens](/pages/settings-screens/)
- [Text widgets](/pages/widgets-text/)
- [Events and intents](/pages/reference-events-and-intents/)
- [Performance](/pages/performance/)

Checked against the API version `ilium:plugin@0.1.0`.

## Production additions: T7-users-privacy-a11y

- **[USR-300]** Every state that a host-drawn plugin widget conveys by colour (error, warning, success, selected, disabled, focus) MUST also be conveyed by a text label, glyph or attribute (bold, underline, reverse) that survives `NO_COLOR` and a 16-colour theme.
- **[USR-301]** Every built-in theme MUST give each colour role (`danger`, `warning`, `success`, `muted`, `accent`, `info`) a contrast ratio of at least 4.5:1 (proposed, owner to confirm) against the theme background, computed on the resolved RGB, and a theme that fails MUST be refused by the theme loader with `theme.low-contrast` naming the role.
- **[USR-302]** The host MUST provide a high-contrast setting (`[ui] contrast = "high"`, default `"normal"`) that raises every role to at least 7:1, replaces `muted` text by the foreground colour, and MUST be delivered to plugins through the theme extras so that a plugin's `rgb` colour can be remapped; a plugin that ignores it MUST still render without error.
- **[USR-303]** Every action on a plugin screen drawn by a host widget MUST be reachable and operable with the keyboard alone, focus order MUST follow reading order (top to bottom, left to right), Tab MUST never trap focus, and Esc MUST always leave a modal plugin dialog.
- **[USR-304]** The focused widget MUST be drawn with an indicator that differs from the unfocused state in at least one attribute besides colour (reverse, bold or a `>` marker) and exactly one widget per window MUST carry it.
- **[USR-305]** When `[ui] screen_reader = true` (default false) the host MUST render widgets in a linear mode: one logical element per line in focus order, with role words (button, checked, unchecked, selected, N of M), no box-drawing or spinner frames, and MUST emit focus changes so that the cursor sits on the focused element's first cell.
- **[USR-306]** A widget kind that can be focused (button, toggle, input, list row) MUST carry a non-empty text label; the view validator MUST reject a focusable widget with an empty label with `ui.label-missing` naming the widget id, and an icon-only button MUST supply a `label` used in screen-reader mode.
- **[USR-307]** When reduced motion is on, the host MUST deliver at most 1 full-view redraw per second for a plugin that animates, MUST stop host-run decorative animations, and MUST count a plugin timer below 1000 ms that triggers only redraws as a `ui.motion-ignored` warning in the plugin log once per session.
- **[USR-950]** The release notes MUST name, for each shipped platform, the screen reader and the terminal emulator that the screen-reader check of [USR-951](#production-additions-t7-users-privacy-a11y) passed with. A platform with no named reader MUST NOT be described as screen-reader supported in any user interface or document.
- **[USR-951]** Before each release, a tester MUST run the consent dialog and one plugin settings screen with `[ui] screen_reader = true` on each shipped platform, at 48 columns (as [SEC-346](/pages/reference-security-testing/) requires), and MUST pass every step: (1) each focusable element is read on its own line, in focus order; (2) each element is read with its role word and state (button, checked, unchecked, selected, N of M); (3) no box-drawing character or spinner frame is read; (4) after each Tab the cursor sits on the first cell of the focused element; (5) the reader announces the focus change without the tester moving the cursor. A failed step blocks the release and the release record names the view and the step.

### Operating system settings

The host reads only its own `[ui]` settings. It does not read the operating system's accessibility settings to decide what a plugin receives. The table shows the settings a user may expect to take effect, and what the host does with each.

| Operating system | Setting the user may expect | Read by the host | Effect on a plugin |
|---|---|---|---|
| Windows | High contrast theme | No | Use `[ui] contrast = "high"` (USR-302) |
| macOS | Increase Contrast, Reduce Motion | No | Use `[ui] contrast = "high"` and the reduced-motion setting (USR-302, USR-307) |
| Linux (GNOME) | High Contrast theme, Animations off | No | Use `[ui] contrast = "high"` and the reduced-motion setting (USR-302, USR-307) |

- **[USR-952]** The host MUST NOT change any value that a plugin receives from an operating system accessibility setting. When a later version reads one, the `[ui]` value MUST win if both are set, and `ilium plugin doctor` MUST print each accessibility value with its source, `[ui]` or the operating system, as [USR-311](/pages/guide-internationalisation/) does for the UI language (proposed, owner to confirm).
