---
layout: page
title: Bidirectional text and control characters
permalink: /pages/guide-bidirectional-text-and-control-characters/
---

## Bidirectional text and control characters

This page specifies what the host does with right-to-left text and with bidirectional control characters in text that a plugin supplies. It covers drawing order, replacement on approval, permission and install prompts, grapheme-safe truncation, the `ui.bidi-control` lint rule and the test fixtures that prove each rule. Plugin authors who show names, paths or labels in either direction need this page, and so do the people who write the host and its tests.

## What the host does with RTL runs

The host draws every run of text in logical order, from left to right in cells. A run of right-to-left characters keeps its logical order in the cells, and the host does not reverse it. The host also does not mirror any character, so a bracket or other mirrored glyph stays as the plugin wrote it.

For example, the fixture string `abc שלום` occupies cells in this order: `a`, `b`, `c`, a space, then the four Hebrew letters `ש`, `ל`, `ו`, `ם`. A test of this rule renders the fixture in a plugin view with the test harness and reads the cells from left to right.

## Bidi control characters in plugin text

Bidirectional control characters change how a terminal orders the text around them, so they can make a name or a path look like something else. The host treats two ranges as bidi controls: U+202A to U+202E (embedding and override characters) and U+2066 to U+2069 (isolate characters).

On an approval, permission or install prompt, the host replaces each bidi control in a plugin-supplied string with one U+FFFD cell. The replacement keeps the position of the string visible, so a reader can see that something was there. The prompt text then contains no code point from either range.

Names follow the same spoofing rule as the dialog names in [SEC-104](/pages/reference-grant-review-and-consent/). The host removes control characters and bidi controls from display names, plugin titles and publisher names before it draws them in the permission dialog. The rule for every other plugin-supplied text on the client is [SEC-380](/pages/reference-extension-security/).

## Spoofing-sensitive surfaces (names, paths, approvals)

Three kinds of surface are sensitive to spoofing: names, paths and approval prompts. A plugin title or publisher name can carry a bidi override that reverses the words after it, so the name on the screen differs from the name in the manifest. A file path with an override can make the end of a path look like a harmless extension. An approval prompt that shows such a path could lead a person to approve something other than what they read.

For these surfaces the host applies the rules in this order:

1. Every plugin-supplied string on an approval, permission or install prompt has each bidi control replaced by one U+FFFD cell.
2. Names in the permission dialog have their bidi controls stripped, so the dialog shows the stripped title.
3. The raw value stays available for inspection. The command `ilium plugin inspect <id> --json` returns the `title` with its bidi code points still present, so a reviewer can see exactly what the manifest contains.

The inspect output is the place to check the raw value. The dialog is the place to check what a person reads at install time.

## Truncation

When the host cuts a plugin label to fit a cell width, it must not split a grapheme cluster or a combining sequence. A base letter and its combining accent, for example `e` followed by U+0301, form one unit, and the host keeps or drops the whole unit.

When a cut happens, the ellipsis `…` (U+2026) takes the last cell of the label. The cells before the ellipsis hold only whole clusters. A 10-cell label whose tenth cell would fall inside the cluster `e` plus U+0301 therefore ends with `…` in cell 10, and cell 9 holds no part of that cluster. Plugin authors can apply the same rule with the grapheme tools in [Internationalisation](/pages/guide-internationalisation/).

## Lint rules and test fixtures

`ilium plugin lint` checks every literal bidi control character in a manifest string or catalogue message. When it finds one, it reports the finding with the code `ui.bidi-control` and names the key that holds it. For example, a U+202E in the manifest `title` produces a report that contains `ui.bidi-control` and the word `title`. Authors should remove the character from the source text and, where the text needs a direction change, use the host's own text rules instead of a literal control character.

The test fixtures for this page are the same strings used in the requirement tests below. They are: the Hebrew fixture `abc שלום` for drawing order; a path that contains U+202E for the approval prompt; a 10-cell label that ends inside the cluster `e` plus U+0301 for truncation; and a manifest `title` that contains U+202E for the lint rule and for the install dialog. Each fixture is a test input only and never a real plugin name.

## Edge cases

- A bidi control inside a grapheme cluster is still a bidi control. The host checks the code point, not the cluster, so the replacement or stripping rule applies to it.
- A string that contains several bidi controls gets one U+FFFD cell for each control. The number of cells does not change for the rest of the text in the same string.
- Stripping in the dialog removes the bidi controls from what is drawn, while the inspect output still shows the raw value. Use the inspect output when you need to know what the manifest really contains.
- A label cut at the ellipsis never shows a partial cluster, even when the cut falls on a combining mark.
- Lint reports every literal bidi control it finds, including one in a catalogue message. A clean manifest produces no `ui.bidi-control` finding.

## Requirements

- **[PDT-001]** The host MUST draw RTL runs in logical order, left to right in cells, and MUST NOT reorder or mirror characters.
- **[PDT-002]** The host MUST replace each of U+202A to U+202E and U+2066 to U+2069 in every plugin-supplied string drawn on an approval, permission or install prompt with one U+FFFD cell, consistent with [SEC-104] for names.
- **[PDT-003]** Truncation MUST NOT split a grapheme cluster or a combining sequence and MUST place the ellipsis `…` in the last cell.
- **[PDT-004]** `ilium plugin lint` MUST report `ui.bidi-control` for any literal bidi control character in a manifest string or catalogue message.
- **[PDT-005]** Names shown in the permission dialog MUST be rendered with bidi controls stripped, and `ilium plugin inspect <id> --json` MUST expose the raw unstripped value.
