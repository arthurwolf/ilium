---
layout: page
title: Internationalisation
permalink: /pages/guide-internationalisation/
---

## Internationalisation

This guide tells an author what `ilium:plugin@0.1.0` gives a plugin for text in many languages, what it does not give, and what to do about the gaps. Requirement ids of this page are in the range `AUT-600` to `AUT-699`.

Terms:

- **Cell**: one column of the terminal grid. A character takes one or two cells.
- **Display width**: the number of cells the host uses for a piece of text.
- **Grapheme**: one character as a reader sees it. It can be several code points, such as a letter plus an accent.
- **Locale**: the language and regional settings of the user, such as date order and decimal mark.
- **UTC**: Coordinated Universal Time, the zone without an offset.

### What the contract provides and what it does not

| Need | In `ilium:plugin@0.1.0`? | What to do |
|---|---|---|
| Cell width of a string | Yes, `display-width(text)` in `host::ui` | Call it before you cut or pad text |
| Wide characters (two cells) | Yes, the host lays them out | Never count bytes or code points for width |
| Wall-clock time | Yes, `now-unix-millis` in UTC | Format it yourself. See below |
| The user's locale | No | Offer a setting, or keep text locale-neutral |
| The user's UTC offset or time zone | No | Offer a setting, show UTC, or show relative time |
| A message catalogue or translation function | No | Ship your own tables |
| Right-to-left layout | No mirroring | Treat text as left-to-right cells |
| Accessible labels | No | Put meaning in text. See [Accessibility and theming](/pages/accessibility-and-theming/) |

A plugin MUST NOT assume that it can learn the user's locale or time zone from the host. [AUT-601]

## Width

A terminal draws a grid. A Latin letter takes one cell. A Chinese, Japanese or Korean character, and most emoji, take two. A combining accent takes none, because it joins the letter before it.

| Text | Bytes | Code points | Cells |
|---|---|---|---|
| `build` | 5 | 5 | 5 |
| `café` (precomposed) | 5 | 4 | 4 |
| `cafe` plus a combining accent | 6 | 5 | 4 |
| two CJK characters | 6 | 2 | 4 |

A plugin MUST measure width with `display-width` and MUST NOT use `len()`, `chars().count()` or `String.length` for layout. [AUT-602]

A plugin that cuts text to fit MUST cut at a grapheme boundary, and SHOULD end a cut with `…`, which takes one cell. [AUT-603] The host cuts a badge at 12 cells and ends it with `…`, using the same width rules.

```rust
use ilium_plugin_sdk::host::ui;

/// Cut `text` to at most `max_cells`, never inside a grapheme.
fn fit(text: &str, max_cells: u16) -> String {
    if ui::display_width(text) <= max_cells {
        return text.to_string();
    }
    let mut out = String::new();
    for grapheme in graphemes(text) {
        let candidate = format!("{out}{grapheme}");
        if ui::display_width(&format!("{candidate}…")) > max_cells {
            break;
        }
        out = candidate;
    }
    out.push('…');
    out
}
```

`graphemes` stands for a grapheme splitter from the `unicode-segmentation` crate. In TypeScript use `Intl.Segmenter`.

```ts
import * as ui from '@ilium/plugin-sdk/host/ui';

export function fit(text: string, maxCells: number): string {
  if (ui.displayWidth(text) <= maxCells) return text;
  const segmenter = new Intl.Segmenter(undefined, { granularity: 'grapheme' });
  let out = '';
  for (const { segment } of segmenter.segment(text)) {
    if (ui.displayWidth(out + segment + '…') > maxCells) break;
    out += segment;
  }
  return out + '…';
}
```

The call to `display-width` crosses into the host. A plugin SHOULD NOT call `display-width` once per character for a long text. Measure once, and cut only when the text is too wide. [AUT-604]

### Label and value pairs

Use `leader-row` for a label and a value. It shrinks the value first, then the label, with `…`. A translated label is often longer than an English one by 30 percent or more. A plugin SHOULD leave room for a longer label and SHOULD NOT depend on a fixed width. [AUT-605] See [Layout constraints](/pages/layout/).

## Text that the plugin does not control

File names, branch names, notes and agent output come in any language. A plugin MUST treat text from outside as data: it MAY contain wide characters, combining marks, text in any direction and any length. [AUT-606] The host removes terminal escape sequences from text a plugin passes to a widget, so such text cannot move the cursor or change colours. See [Text widgets](/pages/widgets-text/).

## Right-to-left text

The host draws each line as a sequence of cells from left to right. It does not reorder bidirectional text and it does not mirror a layout. A plugin SHOULD NOT promise correct display of right-to-left scripts, and SHOULD state this in its README if its users need them. [AUT-607] Keep right-to-left text in its own row, so that it is not mixed with left-to-right labels in one line.

## Your own strings

There is no catalogue function. The usual pattern is a table of strings by a message id, with English as the fallback.

```rust
fn message(language: &str, id: &str) -> &'static str {
    match (language, id) {
        ("de", "saved") => "Gespeichert",
        ("fr", "saved") => "Enregistré",
        (_, "saved") => "Saved",
        (_, _) => "?",
    }
}
```

The plugin must learn the language from somewhere. Because the host provides no locale, offer a `choice` settings row.

```json
{ "type": "choice", "key": "language", "title": "Language", "default": "en",
  "options": [ { "value": "en", "title": "English" }, { "value": "de", "title": "Deutsch" },
               { "value": "fr", "title": "Français" } ] }
```

A plugin that ships several languages MUST fall back to one default language for any missing message, and MUST NOT show a message id to the user. [AUT-608] The settings row `title` and `help` are text in the manifest and have one value. A plugin that wants a translated settings page SHOULD keep the manifest titles in one language and put the translated text in views it draws itself. [AUT-609]

Do not build sentences by joining pieces. Word order differs between languages.

| Avoid | Prefer |
|---|---|
| `"Deleted " + count + " notes"` | A message with a placeholder: `"Deleted {count} notes"` |
| A plural by adding `s` | One message for each plural form the language needs |
| A translated word inside a command id | Keep command ids, keys and codes in English |

Command ids, setting keys, error codes and event names are identifiers. A plugin MUST NOT translate command ids, setting keys, error codes and event names. [AUT-610] Titles shown to people, such as a command `title`, are text and MAY be in any language.

## Text on capability surfaces

Toasts, notices and the grant-review dialog are drawn by the host, but part of their text comes from the plugin. The host does not translate text a plugin supplies, and a plugin does not control the host's own words. The sections below say who owns which text.

A plugin MUST pass a toast or notice body already in the language it chose, and the host MUST show it unchanged apart from escape removal and the width cut. [AUT-618] The host's fixed words around it (a `Show toasts` row, a button, a grant label) are not in the plugin's language and a plugin MUST NOT rely on them matching its own. [AUT-619]

The one-line reason a plugin gives for a grant in its manifest has one value. The review dialog shows it verbatim, as data, cut at the dialog width with `…`, and never mixes it into a host sentence. [AUT-620] A plugin SHOULD write that reason in a language its users read and SHOULD keep it short enough to fit 60 cells after a translation. [AUT-621] Grant ids, the plugin id and capability names in the dialog are identifiers and are never translated. [AUT-622] See [Grant review and consent](/pages/reference-grant-review-and-consent/).

## Time

`now-unix-millis` returns milliseconds since 1970-01-01 in UTC. The contract has no function for the user's offset. Choose one of three ways.

| Way | Good for | Cost |
|---|---|---|
| Show UTC with the zone written out | Logs, records, shared data | Users must convert |
| Show relative time (`3 min ago`) | Recent events | Needs a refresh timer |
| A setting `utc_offset_minutes` | A clock for one person | The user must set it, and daylight saving changes are manual |

A plugin that shows a time MUST write the zone or say that it is relative, and MUST store times as UTC milliseconds, never as local text. [AUT-611]

The manual uses the form `YYYY-MM-DD HH:mm:ss` for dates, which has the same order in every language. A plugin SHOULD use `YYYY-MM-DD HH:mm:ss` for absolute times unless the user chooses another form. [AUT-612]

```rust
/// Format UTC milliseconds as `YYYY-MM-DD HH:mm:ss` plus the zone as `UTC+HH:MM`, so the zone is always written.
fn format_time(unix_millis: u64, offset_minutes: i64) -> String {
    let seconds = (unix_millis / 1000) as i64 + offset_minutes * 60;
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let offset = offset_minutes.abs();
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC{sign}{:02}:{:02}",
        rest / 3600, (rest % 3600) / 60, rest % 60, offset / 60, offset % 60)
}
```

`civil_from_days` stands for a standard day-count to calendar conversion. A date library is simpler. A plugin that formats time MUST be tested with the fake clock, which starts at 2030-01-01 00:00:00 UTC. [AUT-613] See [Clock and timers](/pages/api-clock-and-timers/).

### Numbers, units and sizes

The decimal mark and the group separator differ by region. A plugin SHOULD use a dot and no group separator in machine-readable output, and SHOULD use metric units for sizes it shows. [AUT-614] A setting for the decimal mark is cheap and welcome.

## Sorting and search

Byte order is not alphabetical order in most languages. The host sorts storage keys by byte order. A plugin that sorts names for people SHOULD fold case and accents before comparing, and SHOULD NOT claim a language-correct order. [AUT-615]

## Testing

| Test | How |
|---|---|
| Width of wide and combining text | Render a view with `"日本語"` and `"café"` and check no row is wider than the area |
| Long labels | Use a label twice as long as the English one |
| Missing translation | Ask for an id absent in `de` and expect the English text |
| Time | `set_clock` and check the string for two offsets |
| Escape sequences in input | Pass `"\u001b[31mred"` and check that the view shows no colour change |

```rust
#[test]
fn wide_text_never_overflows() {
    let mut host = FakeHost::for_manifest(MANIFEST).expect("manifest").grant("ui.panels");
    host.activate();
    host.with_fixture("tests/fixtures/wide-titles.json");
    let tree = host.render("notes");
    assert!(tree.max_row_width() <= 40);
}
```

## Edge cases

| Situation | Result |
|---|---|
| A wide character at the last cell of a row | The host moves it to the next row or cuts it |
| An empty string | Width 0. Do not pad with a space for it |
| A combining mark with no base | Width 0. It may attach to the previous cell |
| A cut inside a grapheme | A broken character. Cut at boundaries |
| Text with an ESC sequence | The host removes the sequence and logs a warning |
| A badge over 12 cells | Cut with `…` |
| A translated label longer than the row | The value is cut first, then the label |

## Built-in and replaced core plugin text

The 30 core plugins that ship with Ilium show their menu labels, settings tab titles, command titles and notices in English. The host has no locale and no catalogue, so the user's language setting does not change this text. Each kind of text has one owner.

| Text | Who owns it | Who translates it |
|---|---|---|
| Id of a command, settings tab, pane kind, setting key, error code or event | The plugin that defines it | Nobody. Ids are never translated |
| Title or notice of a built-in core plugin | The Ilium project | Not translated in this release. See the decision below |
| Title or notice of a replacement's own tab, command or notice | The author of the replacement | The author, in any language the author chooses |

A replacement takes over the surface of a core plugin. The rules below say what it keeps and what it writes itself. The install rules are in [Extending core plugins](/pages/extending-core-plugins/) and [Tutorial 21: replace a core plugin](/pages/tutorial-21-replace-a-core-plugin/).

- [USR-239] A replacement MUST keep every command id and every settings tab id of the core plugin it replaces, with the same spelling, and MUST NOT translate them. A replacement MAY give its tab and command titles in another language. If a command id is missing from the replacement, install refuses with `manifest.replaces-surface-incomplete`, which names the missing item.
- [USR-240] A replacement that takes over a settings tab, a command or a notice MUST supply the title and notice text of that item itself, in the language it chooses. The built-in English text of the replaced plugin returns only when the replacement is turned off, and the stored settings of the tab stay as they were.

Decision needed: whether built-in core plugin text (menu labels, tab titles, command titles and notices) gets a translation catalogue owned by the Ilium project. Default: no catalogue in this release, so core text stays English. A catalogue needs a translator for each language and a locale setting that the host does not yet have.

Decision needed: who translates the built-in core plugin text when a catalogue exists. Default: the Ilium project translates its own bundled text. Third-party replacements always translate their own text, as [USR-240] states.

## See also

- [Text widgets](/pages/widgets-text/)
- [Accessibility and theming](/pages/accessibility-and-theming/)
- [Layout constraints](/pages/layout/)
- [Clock and timers](/pages/api-clock-and-timers/)
- [Quality checklist](/pages/guide-quality-checklist/)

API version checked: `ilium:plugin@0.1.0`.
