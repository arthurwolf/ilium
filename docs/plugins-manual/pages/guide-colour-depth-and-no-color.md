---
layout: page
title: Colour depth, NO_COLOR and colour-vision safety
permalink: /pages/guide-colour-depth-and-no-color/
---

## Colour depth, NO_COLOR and colour-vision safety

This page specifies how a plugin's text keeps its meaning when the terminal shows fewer colours, when the `NO_COLOR` environment variable asks for no colour, and when a reader cannot tell two colours apart. It gives the exact degradation of theme roles and of `rgb` and indexed colours at 256, 16 and monochrome depth, the effect of `NO_COLOR` together with the high-contrast setting, and the pairing rule that keeps states readable for colour-blind users. Plugin authors need it before they choose colours for a state, and anyone who tests a plugin at each depth needs the harness options listed at the end.

## Detecting the colour depth

The colour depth is a property of the terminal that shows the plugin. The host renders plugin text at one of four depths: `truecolor`, `256`, `16` or `8`, where `8` is monochrome. A plugin does not choose the depth and does not read it. The plugin writes its text with theme roles and, where it must, with `rgb` or indexed colours, and the host reduces every colour to the depth in use.

When you test a plugin, you choose the depth yourself. The harness takes `--colour-depth` with one of the values `8`, `16`, `256` or `truecolor`. The depth you chose is written into the report file, so a report always names the depth it was made at. See [Testing at each depth](#testing-at-each-depth) below.

## How roles degrade at 256, 16 and monochrome

A plugin uses the nine colour roles of the theme: `foreground`, `muted`, `accent`, `success`, `warning`, `danger`, `info`, `selection` and `border`. A role takes its colour from the active theme, so the host can reduce it to whatever palette the depth offers. The role keeps its place in the theme at every depth. A `muted` span is still the dimmer text, and a `danger` span is still the alarm colour, even when the colour itself has been reduced.

Colour is the first thing a low depth removes, so a role must never be the only carrier of meaning. At monochrome depth (`8`), the host shows the `danger` role as bold text with the literal prefix that the plugin supplied, for example `!`, placed before the text. The emphasis is the bold attribute and the prefix is characters, so a reader sees the warning with no colour at all. A plugin that wants the same effect at every depth should write the prefix in its text rather than rely on the colour of the role.

## rgb and indexed colours at low depth

An `rgb` colour is a fixed colour value, not a theme role, and it does not follow the theme. At a 256-colour depth the host maps each `rgb` colour to the nearest entry of the xterm-256 palette. Nearest means the entry with the smallest CIE76 distance in CIELAB. For example, `rgb(250, 128, 114)` shown at `--colour-depth 256` becomes index 209. At 16 colours and at monochrome, the harness reduces every colour to that smaller palette, as the client does; see [Test harness](/pages/reference-test-harness/).

Because a fixed colour follows neither the theme nor the depth, a plugin should use theme roles for its state colours and keep `rgb` and indexed colours for content that is truly a colour, such as a chart series. The [Accessibility and theming](/pages/accessibility-and-theming/) page explains why roles are the default.

## NO_COLOR and the high-contrast setting together

`NO_COLOR` is a request from the user for no colour. When it is set to any non-empty value, the host emits no colour escape sequences for plugin text. The bold, underline and reverse attributes remain, so a bold span is still bold, and an underlined or reversed span shows its emphasis. The host does not remove these attributes to make the output plainer.

The high-contrast setting (`[ui] contrast = "high"`) raises every role to a higher contrast ratio. Under `NO_COLOR`, the host draws no colour for plugin text, so the extra contrast has no visible effect on that text. The user still reads emphasis through bold, underline and reverse, and through the words and characters the plugin wrote. A plugin should therefore put every status into its text or its glyph, and treat the high-contrast setting as an extra aid, not as the only way to read a state.

## Colour-vision-safe pairs

Some readers cannot tell red from green or some blues from greys. Two states of one widget therefore must never differ only by a role colour. Each state needs its own text, its own glyph or both. For a badge with an `ok` state and a `warn` state, the words `ok` and `warn` carry the difference, and the colours `success` and `warning` only add to it.

Check each widget with `ilium plugin lint`. The lint command reports `ui.colour-only-state` for any widget whose two declared states have the same text and glyph and differ only by a role colour. Fix the finding by changing the text or the glyph of one state, not by changing the colour. The lint rule is listed in [Check and lint rules](/pages/reference-check-and-lint-rules/).

## Testing at each depth

Render each view of the plugin at every depth that you support. The harness accepts `ilium plugin test --colour-depth 8`, `16`, `256` or `truecolor`, and it refuses any other value with exit code 2 and the error code `usage.invalid-value`. A check at `--colour-depth 16` runs and writes 16 as the depth in its report file. Use the report to confirm which depth each result came from.

For each depth, look for three things. First, the colour escape sequences are gone when `NO_COLOR=1` is set, and the bold sequence remains for a bold span. Second, the `danger` role at monochrome depth shows bold text and its literal prefix. Third, the `rgb` colours of the view map to the palette of that depth as the table above describes. Run `ilium plugin lint` as well, because it finds colour-only states that a render check does not show. For how the harness is driven from code, see [Plugin test harness](/pages/reference-plugin-test-harness/).

## Requirements

- **[PDT-012]** With `NO_COLOR` set to any non-empty value the host MUST emit no colour escape sequences for plugin text and MUST keep bold, underline and reverse attributes.
- **[PDT-013]** An rgb colour on a 256-colour terminal MUST map to the nearest xterm-256 index by CIE76 distance in CIELAB.
- **[PDT-014]** At monochrome depth the host MUST show the `danger` role as bold plus the literal prefix text the plugin supplied, never by colour alone.
- **[PDT-015]** The harness MUST accept `--colour-depth 8|16|256|truecolor` for render checks and MUST refuse any other value with exit 2 and `usage.invalid-value`.
- **[PDT-016]** `ilium plugin lint` MUST report `ui.colour-only-state` when two declared states of one widget differ only by a role colour.

## Production additions (round 3)

- **[PRY-032]** After downgrade to the 16-colour palette, any plugin-supplied foreground and background pair rendered together MUST have a contrast ratio of at least 4.5:1 (WCAG relative luminance), otherwise the host MUST substitute the theme foreground for that cell and record `ui.contrast-adjusted` once per plugin per session.

## Production additions (round 10)

- **[PRR-023]** The system must render every status role (danger, warning, success, attention) with a distinct non-colour cue such as a glyph or text prefix at every colour depth, and the conformance suite must fail a core plugin whose status row, tree decoration or toolbar badge differs from its neighbour only by colour.
