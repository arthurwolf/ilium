---
layout: page
title: Text input, IME composition and paste in plugin views
permalink: /pages/guide-text-input-and-composition/
---

## Scope

This page specifies what a plugin text input receives while an input method is composing text, which key bindings the host suppresses during that composition, how a bracketed paste arrives and is bounded, and how wide characters and grapheme clusters move the cursor and count against a limit. It also describes the fixtures and harness checks that prove the behaviour. Plugin authors who build text inputs, and maintainers of the terminal harness, need these rules so that people who type Chinese, Japanese, Korean or other input-method text can use plugin inputs.

## Composition events as the terminal delivers them

An input method builds a character in stages. While the user is still choosing a character, the terminal shows preedit text, which is the provisional string the input method has not yet committed. When the user accepts a character, the terminal delivers a commit with the final string. For example, the Japanese syllable に is typed as the preedit letters `n` and `i`, and it becomes final only when the user commits it.

A cancelled composition delivers no committed string at all. The host treats preedit as a state of the input, not as text, so it keeps the preedit separate from the value that a plugin sees.

## What a plugin input receives

A plugin text input sees only committed text. Each committed string produces one text-input change event, and that event carries the value after the change. The event never carries preedit text. In the example above, the plugin receives one change event whose value ends with に, and no event contains `n` or `ni`.

Because of this rule, a plugin never has to decide whether a letter is provisional. It also never shows a half-typed syllable as if it were stored text.

## Key bindings during composition

While a composition is active, the host does not run plugin commands that are bound to printable keys. A command bound to the letter `j` therefore does not run when the user types `j` as preedit. Its invocation count stays at 0 until the composition is committed or cancelled.

This rule covers printable keys only.

## Paste, bracketed paste and width

The terminal sends a paste between the bracketed paste markers. The host delivers a bracketed paste to the input as one text-input change event, with a value of up to 1 MiB (1,048,576 bytes). A larger paste is truncated to 1 MiB. When truncation happens, the host records one notice with the code `input.paste-truncated`, so the user and the author can find out why text is missing.

Width is measured in terminal cells, not in characters. A wide character, such as the kanji 漢, occupies two cells. Typing 漢 into an empty input moves the cursor from cell index 0 to cell index 2.

## Cursor movement and limits in grapheme clusters

A grapheme cluster is what a reader sees as one character. It can contain more than one code point. The letter e followed by a combining acute accent (U+0301) is one cluster, even though it is two code points and two bytes or more.

The cursor moves by whole clusters. Each press of Left or Right moves the cursor across exactly one grapheme cluster. Take the text `e` plus combining acute followed by `x`, which is two clusters, and put the cursor after `x`. One Left press moves the cursor in front of `x`. A second Left press moves it to index 0, before the accented `e`.

An input `maxlength` counts grapheme clusters. It does not count bytes or code points. An input with `maxlength: 3` accepts the cluster e plus combining acute, then 漢, then `a`, which makes exactly three clusters. The next keystroke, `b`, is refused.

## Testing with the harness

The harness drives a plugin input with scripted keys, preedit and paste, and it reports the change events, notices, invocation counts and cursor cell index that a plugin would see. Each behaviour above has a matching harness check.

To test composition gating, bind a command to `j`, start a composition, and send `j` as preedit. Then check that the command's invocation count is 0 until the composition ends.

To test committed-only events, compose に from preedit `n` and `i`, then commit it. The harness must record exactly one change event for the commit.

To test paste, send 2 MiB inside bracketed paste markers. The harness must record one change event with a 1 MiB value and one notice with the code `input.paste-truncated`.

To test width, type 漢 into an empty input and read the cursor cell index. The index must be 2. The harness ships the fixture `fixtures/input-wide-cursor`, which encodes this check, and that fixture must pass.

To test limits and movement, run the `maxlength` and cursor sequences described above and compare the results with the expected cluster counts and cursor positions.

## Requirements

- **[PDT-006]** While a composition is active the host MUST NOT run plugin commands bound to printable keys.
- **[PDT-007]** A text-input change event MUST carry only the committed string, never preedit text.
- **[PDT-008]** A bracketed paste MUST arrive as one change event of up to 1 MiB, and a larger paste MUST be truncated to 1 MiB with the notice code `input.paste-truncated`.
- **[PDT-009]** A wide character MUST advance the cursor by two cells, and the harness MUST ship a fixture that checks this.
- **[PDT-010]** An input `maxlength` MUST be counted in grapheme clusters, not bytes or code points.
- **[PDT-011]** Cursor movement in a plugin text input MUST move exactly one grapheme cluster per Left or Right key press.
