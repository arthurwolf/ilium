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

The 30 core plugins that ship with Ilium show their menu labels, settings tab titles, command titles and notices in English. Until the host catalogue of [AUT-623] ships, the user's language setting does not change this text. Each kind of text has one owner.

| Text | Who owns it | Who translates it |
|---|---|---|
| Id of a command, settings tab, pane kind, setting key, error code or event | The plugin that defines it | Nobody. Ids are never translated |
| Title or notice of a built-in core plugin | The Ilium project, through the host catalogue | Translated into the launch languages, [AUT-623] and [AUT-624] |
| Title or notice of a replacement's own tab, command or notice | The author of the replacement | The author, in any language the author chooses |

A replacement takes over the surface of a core plugin. The rules below say what it keeps and what it writes itself. The install rules are in [Extending core plugins](/pages/extending-core-plugins/) and [Tutorial 21: replace a core plugin](/pages/tutorial-21-replace-a-core-plugin/).

- [USR-239] A replacement MUST keep every command id and every settings tab id of the core plugin it replaces, with the same spelling, and MUST NOT translate them. A replacement MAY give its tab and command titles in another language. If a command id is missing from the replacement, install refuses with `manifest.replaces-surface-incomplete`, which names the missing item.
- [USR-240] A replacement that takes over a settings tab, a command or a notice MUST supply the title and notice text of that item itself, in the language it chooses. The built-in English text of the replaced plugin returns only when the replacement is turned off, and the stored settings of the tab stay as they were.

Decided: the Ilium project owns the host text catalogue. English is its source language. The catalogue holds the menu labels, tab titles, command titles and notices of the host and of the bundled core plugins, and the wording of host dialogs such as the grant review. The Ilium project translates that text itself. Third-party replacements always translate their own text, as [USR-240] states.

- [AUT-623] The Ilium project MUST own the host text catalogue, keep English as its source, and translate into each launch language every host text that is listed above, including the text of the 30 bundled core plugins. Translation is the work of the Ilium project maintainers, and a release MUST NOT ship a launch language with a host text missing from its catalogue unless the missing text falls back to English under the lookup order of [USR-312].
- [AUT-624] The host MUST ship its catalogue in these launch languages: English, German and French (proposed, owner to confirm). A language tag outside this set MUST show English for every host text.
- [AUT-625] The release candidate MUST pass the `en-XA` pseudo-locale check of [USR-316] over every host dialog and every view of a bundled core plugin, and an `i18n.layout-overflow` finding MUST block the release. The check runs in CI on each release candidate (proposed, owner to confirm).

## What the host catalogue translates and what the plugin must: settings rows and help topics

This section states which plugin-declared strings the host translates, which it never touches, and how a plugin gets translated settings rows and help. It extends [AUT-609] and [AUT-623]. It builds on the plugin catalogue files of [USR-310], the lookup order of [USR-312] and the catalogue rule of [USR-259] and [USR-260]. The proposed forms below (the `{ "id": ... }` reference and `help/<tag>/<id>.md`) are marked as proposed, owner to confirm, because no requirements file carries their wording yet.

### Who translates which string

The host catalogue ([AUT-623]) holds host text and the text of the bundled core plugins. A third-party plugin never edits it. The plugin's own `i18n/<tag>.json` ([USR-310]) is consulted only for the plugin's own message ids ([USR-317]).

| String | In the host catalogue? | In the plugin's `i18n/<tag>.json`? | Result when no translation exists |
|---|---|---|---|
| Host menu labels, tab titles of host pages, host dialog wording, grant and risk labels | Yes | Never. A plugin cannot override it ([USR-317]) | English ([USR-260]) |
| Menu labels, tab titles, command titles and notices of a bundled core plugin | Yes ([AUT-623]) | No | English |
| Command `title`, settings tab `title`, settings row `title` and `description`, option `title` of a third-party plugin | No | Yes, through the reference form below | The literal string written in the manifest |
| Grant `reason` of a third-party plugin | No | Yes, through the reference form below | The literal string, cut at the dialog width with `…` ([AUT-620]) |
| Help topic of a third-party plugin | No | Not in the JSON file. In `help/<tag>/<id>.md` | `help/<id>.md` |
| Toast and notice bodies a plugin sends at run time | No | The plugin chooses the language itself ([AUT-618]) | Whatever the plugin passed |
| Ids: command ids, setting keys, error codes, event names, `contributes.help` ids | Never translated | Never translated | Not applicable ([AUT-610]) |

The host never machine-translates any string. A string that no catalogue reaches is shown as written, cut with `…` at its box, and never as a message id.

### Catalogue references in the manifest (proposed, owner to confirm)

A manifest field that is listed as translatable in the table above accepts either a literal string or a reference object.

```json
{ "type": "number", "key": "interval", "default": 30,
  "title": { "id": "settings.interval" },
  "description": { "id": "settings.interval.description" },
  "options": [ { "value": "fast", "title": { "id": "settings.interval.fast" } } ] }
```

Rules:

- The object has exactly one member, `id`, a message id of 1 to 128 characters from `a-z`, `0-9`, `.`, `-` and `_`. Any other member, an empty id or a wrong type makes install refuse with `manifest.i18n-reference-invalid`, naming the field path.
- The host resolves the id through the plugin's own files in the order of [USR-312]: exact tag, language without region, then `en`. The file `i18n/en.json` is the source and MUST contain every referenced id. A referenced id missing from `i18n/en.json` makes `ilium plugin lint` report `i18n.reference-unresolved` as an error, naming file and id.
- A literal string stays valid and is shown in every language. A plugin that does not ship translations needs no change.
- Placeholder rules, plural messages and the 256 KiB file limit of [USR-310] to [USR-313] apply to these messages unchanged. A placeholder-count mismatch falls back to English for that one message.
- An identifier field (`key`, `id`, `command`, `type`, `value` of an option) MUST NOT be a reference. A reference there is `manifest.i18n-reference-invalid`, because a translated identifier would break stored settings ([AUT-610]).
- The host resolves the manifest text at activation and again when the UI language changes. The language is stable for a session ([USR-311]), so a language change takes effect on the next start.

### Translated help topics (proposed, owner to confirm)

A help topic is a Markdown file. For a `contributes.help` entry with the id `overview`, the host reads the first file that exists in this order:

1. `help/<tag>/overview.md` for the exact tag, for example `help/pt-BR/overview.md`.
2. `help/<language>/overview.md` for the language without region, for example `help/pt/overview.md`.
3. `help/overview.md`, the source topic, normally English.

A translated topic replaces the whole source topic. The host never merges two files. The first heading of the chosen file is the topic title in the help viewer. Topic ids, file names other than the tag folder, and relative links stay unchanged, so a link written as `[Setup](setup.md)` resolves inside the same tag folder first and then in `help/`.

### Lint and search

- `lint.help-topic-missing` ([reference](/pages/reference-check-and-lint-rules/)) tests the source topic `help/<id>.md` only. A tag folder without a matching source topic is reported as `lint.help-translation-orphan` (warn, proposed), because the viewer would show it for no entry.
- A translated topic that omits a command, setting or view named in the source topic does not fail lint, but a plugin SHOULD name each command, setting and view in every language ([DEV-788]), because the search of the help viewer matches the translated text of the chosen file, not the English one.
- The Settings search matches the resolved row `title` and `description` in the active language, the literal English when no translation exists, and the row's `keywords`. A `keywords` entry is a list of strings and MAY be a reference object. A row is found by its translated words and by its English `keywords`. The English `title` is not searched when a translation exists, so a plugin that wants English search to keep working lists the English words in `keywords`.
- The pseudo-locale check of [USR-316] expands every resolved catalogue string, including referenced manifest text. A literal manifest string is not expanded and is therefore outside the check. The check cannot reach text the catalogue does not reach, which is why a plugin that wants full coverage uses references for every translatable field.

### Failure modes

| Situation | Result |
|---|---|
| `i18n/de.json` lacks a referenced id | That string shows in English; no error to the user |
| `i18n/de.json` is invalid | `i18n.catalogue-invalid` ([USR-310]); the whole file is ignored and English is used |
| Manifest reference to an id missing from `i18n/en.json` | Lint error `i18n.reference-unresolved`; at run time the host shows the literal English fallback if the manifest gave one, otherwise an empty text, and logs once; it never shows the message id |
| Tag folder present for a language the host does not ship | Used for the plugin; the language list of [AUT-624] limits host text only |
| User sets a language the plugin does not ship | English for plugin text, host catalogue text as far as it exists |

Until a plugin uses references, [AUT-609] remains the rule: keep the manifest in one language and translate in views the plugin draws itself.

## East Asian Ambiguous width is a user setting

Unicode marks some characters as East Asian Ambiguous: Greek and Cyrillic letters, box-drawing characters, some arrows, circled digits and many symbols. A Western terminal draws them in one cell. Many Chinese, Japanese and Korean terminals draw them in two. The host cannot detect which one the terminal does, so the user chooses.

### The setting

```toml
[ui]
ambiguous_width = "narrow"   # "narrow" (1 cell, default) or "wide" (2 cells)
```

- The default is `narrow`, which is the rule of [USR-315] and [WID-019]: Ambiguous characters count as 1 cell. Any other value is rejected when the config loads, with a message naming the key and the two allowed values, and the previous value stays active.
- The guided first-run setup proposes `wide` when `LC_CTYPE` (or `LC_ALL`, then `LANG`, in that order) names a CJK locale: language `ja`, `zh` or `ko`. The user confirms or declines; the host never switches on its own. This is proposed behaviour, owner to confirm.
- The setting is in the Settings screen under the interface page, as a two-option row, and takes effect without a restart. It is global: one value for the whole session and every plugin.

### What uses the value

- `display-width` in `host::ui` returns the width under the chosen value ([AUT-602]). So does every host widget: text, leader rows, tables, tab strips, badges and the borders the host draws. A plugin sees one rule, never two.
- Wide cells for Ambiguous characters are never split across a wrap or cut ([USR-315]). A cut that would fall inside a two-cell character moves before it, and the freed cell is padded with a space.
- Cell width of emoji, CJK characters and combining marks does not change with the setting.

### Telling the plugin

A change of the setting emits the existing `theme-changed` event with the reason field `width`. A dedicated `width-changed` event is proposed, owner to confirm; until it exists, a plugin that caches widths subscribes to `theme-changed` ([accessibility and theming](/pages/accessibility-and-theming/)). On either event a plugin MUST drop every cached width and cut position and recompute with `display-width`. A plugin that never caches, and calls `display-width` while it builds a view, needs no change.

A plugin MUST NOT hard-code a width for a character, and MUST NOT ship its own width table. A table in the plugin disagrees with the host on the first Ambiguous character the user's setting turns wide.

### Testing

| Test | How |
|---|---|
| A view with Greek, box-drawing and arrow characters | Render it under `narrow` and under `wide` and check no row is wider than the area in either |
| A cached cut | Change the setting through the fake host and check the plugin renders again |
| The release candidate | The release test renders the Settings screen of every bundled core plugin and every host dialog with `ambiguous_width = "wide"` and with the `en-XA` locale of [USR-316], and asserts no row overflows and no two-cell character is split. An overflow blocks the release as `i18n.layout-overflow` ([AUT-625]) |

### Edge cases

| Situation | Result |
|---|---|
| The terminal draws narrow but the user sets `wide` | Every right edge sits one cell early per Ambiguous character; the fix is to set `narrow` |
| The terminal draws wide but the setting is `narrow` | Borders and right-aligned values are one cell off per Ambiguous character; the fix is to set `wide` |
| A plugin cached widths and ignores the event | Its rows are off until its next redraw from fresh `display-width` calls |
| The setting changes while a toast is on screen | The toast is laid out again with the new value |

## See also

- [Text widgets](/pages/widgets-text/)
- [Accessibility and theming](/pages/accessibility-and-theming/)
- [Layout constraints](/pages/layout/)
- [Clock and timers](/pages/api-clock-and-timers/)
- [Quality checklist](/pages/guide-quality-checklist/)

API version checked: `ilium:plugin@0.1.0`.

## Production additions: T7-users-privacy-a11y

- **[USR-310]** A plugin that ships translations MUST put them in `i18n/<tag>.json` as a flat object of message id to string, tag a BCP 47 language tag such as `de` or `pt-BR`; the loader MUST reject a file larger than 256 KiB, a non-string value, a duplicate id or invalid UTF-8 with `i18n.catalogue-invalid` naming file and id.
- **[USR-311]** The host MUST choose the UI language in this order: `[ui] language` setting, `ILIUM_LANG`, `LC_ALL`, `LC_MESSAGES`, `LANG`, then `en`; the choice MUST be stable for the whole session, and `ilium plugin doctor` MUST print the chosen tag and its source.
- **[USR-312]** Lookup for a message MUST try the exact tag, then the language without region (`pt-BR` then `pt`), then `en`; a placeholder-count mismatch between the translation and the English source MUST cause that one message to fall back to English and log `i18n.placeholder-mismatch` once per message.
- **[USR-313]** A message with a count MUST be a plural message with the CLDR categories zero, one, two, few, many, other, `other` MUST be required, and the host MUST pick the category with the CLDR plural rule of the active language; a missing category MUST use `other`.
- **[USR-314]** The host MUST format numbers, sizes and dates in its own notices by the active language's separators and date order, MUST keep the log and audit timestamps in `YYYY-MM-DD HH:mm:ss` regardless of language, and MUST NOT change any machine-readable output (`--json`, lockfile, bundle names).
- **[USR-315]** Text from a catalogue MUST be measured in terminal cells using Unicode East Asian Width and combining marks, a wide glyph MUST never be split across a line wrap, and a string that exceeds its box MUST be truncated with `…` rather than overflow.
- **[USR-316]** The host MUST accept the language tag `en-XA` which expands every catalogue string by 40 percent (proposed, owner to confirm) with accents and brackets, and `ilium plugin lint` MUST fail a plugin whose view clips or overflows at `en-XA` with `i18n.layout-overflow`.
- **[USR-317]** A plugin catalogue MUST only be consulted for the plugin's own message ids; a plugin MUST NOT override host permission wording, risk labels or consent prompts, and every user-visible text in a host dialog MUST come from the host catalogue.
