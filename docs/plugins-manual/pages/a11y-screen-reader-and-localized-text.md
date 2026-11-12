---
layout: page
title: Screen reader and localized text in plugin views
permalink: /pages/a11y-screen-reader-and-localized-text/
---

## Screen reader and localized text in plugin views

A plugin view reaches a screen reader as text. The terminal has no accessibility tree, so when the user sets `[ui] screen_reader = true`, the host draws each focusable element on its own line, in focus order, with a role word and its state. The plugin supplies the words of the element: its label, its status and the text of its value. It does not supply the role word, and it does not translate the host's words. This page covers what the plugin writes for that mode, how to check it, and how to keep the text of a view translatable. The mode itself is owned by [USR-305](/pages/reference-requirements-user-admin/) and the accessibility baseline is in [API: ui](/pages/api-ui/#accessibility-baseline). Message catalogues and the pseudo-locale check are owned by [Internationalisation](/pages/guide-internationalisation/). This page adds the plugin side of both and does not repeat their rules.

### You will need

- **A view with focusable widgets.** Keyboard use of the view is covered in [Keyboard operation and focus](/pages/a11y-keyboard-and-focus/).
- **The screen reader setting.** `[ui] screen_reader`, read by the host and changed by the user in [Settings plugin](/pages/core-plugin-settings/). The test host sets it through its settings, under the key `ui.screen_reader` (proposed key name, owner to confirm).
- **A message catalogue.** Only if the plugin ships more than one language. The file layout is in [USR-310](/pages/reference-requirements-user-admin/) and the lookup order in [USR-312](/pages/reference-requirements-user-admin/).

## What the linear mode prints

In screen reader mode the host prints one line per focusable element, in focus order. Each line holds the label of the element, its role word, its state where it has one, and its position where it belongs to a set. Box-drawing characters and spinner frames are not printed ([USR-305](/pages/reference-requirements-user-admin/)).

The exact form of the line is not in the contract yet. The proposed form below is the one this page checks against. The owner must confirm it before the form is tested.

Proposed, owner to confirm: `<label>, <role word>[, <state>][, <N> of <M>]`, with the state as the words `checked`, `unchecked`, `selected`, `disabled` or `locked by your administrator`, and the position only for a row of a list, a table or a tab strip.

<figure>
<pre class="term"><span class="t-dim">Screen reader mode, settings view at 48 columns</span>
Search, text field, empty
Only failing, toggle, unchecked
Run again, button, disabled, the agent is busy
Agents, tab, selected, 1 of 3
Enable plugin, toggle, unchecked, locked by your administrator
</pre>
<figcaption>Proposed form of the linear mode for one view. The host prints each line; the plugin wrote only the labels, the values and the disabled reason.</figcaption>
</figure>

The last line shows the locked control described in [Enterprise deployment](/pages/guide-enterprise-deployment/#locked-items-as-text). The lock state comes from the host, not from the plugin.

### Role words

The role word tells the user what kind of control the line is. The host chooses it from the widget kind. The plugin never writes a role word into a label.

| Widget | Role word (proposed) |
|---|---|
| `button` | button |
| `toggle` | toggle, with `checked` or `unchecked` |
| `check-list` item | checkbox, with `checked`, `unchecked` or `mixed` |
| `choice`, `number`, `slider` | the widget name, with its current value as the state |
| `text-input`, `text-area` | text field, with `empty` or the value |
| `tabs` item | tab, with `selected` or nothing |
| `%list`, `table`, `tree-list` row | list item, with `selected` or nothing and the position |

## Labels: what the plugin writes

The label is the only text the plugin writes for a focusable widget. It must say what the control does. The value and the state are read after it, so a label that repeats the value makes the line hard to follow.

Give a control a label that names its action, such as `Max results`, and let the value carry the number. A label that only shows the value, such as `25`, leaves the user with no name for the control. An icon-only button needs a `label` as well ([USR-306](/pages/reference-requirements-user-admin/)).

Do not put meaning into a glyph or an emoji inside a label. A decorative symbol is read as a word, so a label such as a check mark drawn before `Saved` reads as the name of that mark followed by `Saved`. Put the state in words, and keep the glyph out of the label.

## Status changes are not announced

The host has no announcement path for a plugin. When the plugin changes a status text in a view, the screen reader does not say so. The user hears the new text only when they move focus onto the row, or when they read the view again. A plugin must not depend on an announcement. [API: notifications](/pages/api-notifications/) states the same for `notify` and `play-sound`.

If a change needs the user's attention, the plugin uses a toast or a desktop notice, and it keeps a persistent text signal in its view. A toast is read as one line in the linear mode, as [Toast notices](/pages/capability-notify-toast/#accessibility) describes, and the persistent twin rule of CAP-883 applies.

## Worked example: a label that reads well in both modes

The example is one button whose value changes with a run. It is shown as the tree the plugin returns and as the line the host prints for it in screen reader mode. The snippet uses the TypeScript builder names of the [TypeScript SDK](/pages/typescript-sdk/).

```ts
import { button } from '@ilium/plugin-sdk/ui/build';

// Good: the label names the action, and the reason for a disabled control is in words.
const runButton = button('Run again', { key: 'run-again', disabledReason: 'the agent is busy' });
```

The status line is a plain text row of the same view. Its text carries the last result in words, for example `Last run: failed, 2 checks`.

In screen reader mode the button reads as `Run again, button, disabled, the agent is busy`, and the status row reads as its own line. The plugin never adds `button` to the label, because the host prints the role word too. A label of `Button: Run again` would be read as `Button: Run again, button`.

## Localized text

The host has a language setting for its own text ([USR-311](/pages/reference-requirements-user-admin/)), and the plugin has its own text. Each one comes from its own catalogue.

- **Host text.** Role words, state words such as `locked by your administrator`, and the host's key hints come from the host catalogue, keyed by message id ([USR-259](/pages/reference-requirements-user-admin/)). A plugin cannot change them ([USR-317](/pages/reference-requirements-user-admin/)).
- **Plugin text.** Labels, values and disabled reasons come from the plugin's own messages, as [Your own strings](/pages/guide-internationalisation/#your-own-strings) describes. A plugin that ships translations puts them in `i18n/<tag>.json` ([USR-310](/pages/reference-requirements-user-admin/)).

A label that the user hears in a screen reader is a plugin message. It is translated like any other message, and its id is never shown. A message that is missing in the user's language falls back to English for that one message ([USR-260](/pages/reference-requirements-user-admin/)).

Proposed, owner to confirm: the role words and state words of the linear mode are host text, translated with the host catalogue and selected by the same language setting as the rest of the host's dialogs.

## Pseudo-locale check in screen reader mode

A translated label is often longer than the English one. The check of [USR-316](/pages/reference-requirements-user-admin/) uses the pseudo-locale `en-XA`, which lengthens each string by 40 per cent. A screen reader check repeats that test in the linear mode.

Set the language to `en-XA` and `[ui] screen_reader = true`, render the view at 48 columns, and read every line. Each label must be whole. A label that is cut with `…` is a failure, because the screen reader reads the cut text as the name of the control. The release gate that uses this check is listed in [Quality checklist](/pages/guide-quality-checklist/).

## Edge cases

| Situation | What the screen reader gets |
|---|---|
| A focusable widget with an empty label | The view is refused with `ui.label-missing`. The last good tree stays on screen, as [API: ui](/pages/api-ui/#accessibility-baseline) describes. |
| A `disabled-reason` longer than the row | The reason is cut with `…` in the normal view. The linear line keeps the full reason, so the plugin keeps the reason short. |
| A label with wide CJK text | The host measures its real cell width ([USR-315](/pages/reference-requirements-user-admin/)). The line is not split inside a character. |
| A translated label that the user's language lacks | The English text of that message is printed, and no message id is shown. |
| A value that changes without a focus change | The row shows the new value on its next render. The user hears it only after moving to the row. |

## Requirements

- **[ACC-012]** A plugin MUST give every focusable widget a label that names its action. The label MUST NOT contain the value, a state word or a role word that the host prints itself. Test: render the view in screen reader mode and assert that each line has the role word once.
- **[ACC-013]** A plugin MUST NOT put meaning in a decorative glyph or an emoji inside a label. The meaning MUST be a word. Test: scan the labels of the view for characters outside letters, digits, spaces and punctuation, and assert that each one carries no meaning that the words do not give.
- **[ACC-014]** A plugin MUST put a status change in the words of the row that shows it, not only in a colour, a glyph or a moving frame. Test: change the status in the fake host, render the view, and assert that the row text holds the new state word.
- **[ACC-015]** A plugin MUST NOT rely on the screen reader to announce a text change in its view. A change that needs the user's attention MUST also raise a toast or a desktop notice with a persistent text twin (CAP-883). Test: review the change path of the view for an event that needs attention and check that it has the twin.
- **[ACC-016]** A plugin that ships translations MUST build each accessible label from a message id of its own catalogue, and MUST NOT join two translated pieces into one label. Test: search the source for a label built with `+`, a format string with two translated parts, or a string concatenation.
- **[ACC-017]** A plugin MUST NOT write a role word, such as `button` or `toggle`, into a label or a value. Test: render the view in screen reader mode and assert that no line holds the role word twice.
- **[ACC-018]** A plugin MUST keep every label of its focusable widgets whole at 48 columns in the pseudo-locale `en-XA` with screen reader mode on. A label that is cut with `…` is a failure. Test: render the view with `en-XA` and `screen_reader` set in the test host, and assert that each linear line ends with its full label.
- **[ACC-019]** A plugin MUST keep a `disabled-reason` short enough to be read in one line at 48 columns, and MUST give the reason in words, not as a symbol. Test: render each disabled widget at 48 columns and assert that the reason is complete.
- **[ACC-020]** A plugin MUST NOT show a message id, a key of a setting or a catalogue key in any label or value of a view, in any language. Test: search the rendered screen text in each shipped language for a dotted identifier that the plugin defines.
- **[ACC-021]** A plugin MUST keep the same focusable widgets, with the same labels, in screen reader mode and in the normal view. A change of the mode MUST NOT remove a control or change its action. Test: render the view with the setting off and on, and assert the same set of keys.
- **[ACC-022]** A plugin test SHOULD check the linear output of each view once per release, with the screen reader setting on, and SHOULD compare its line order with the focus order of [Keyboard operation and focus](/pages/a11y-keyboard-and-focus/). Test: the test host reads the screen text and the focus order, and the assert compares both.
- **[ACC-023]** In linear mode the host MUST read each cell of a `table` row as `<column title>: <cell text>`, in column order, before the role word and the position. A plugin MUST give every table column a non-empty `title`. (proposed, owner to confirm)
- **[ACC-024]** In linear mode the host MUST print the `title` of a `block` or panel on its own line, before the first element inside it. A plugin MUST give a non-empty `title` to every `block` that contains a focusable widget. (proposed, owner to confirm)
- **[ACC-025]** In linear mode the host MUST print non-focusable text widgets (`paragraph`, `markdown`, `notice`, `leader-row`, `badge` and `key-hints`) as plain lines in tree order, between the focusable lines. It MUST read a `gauge`, `line-gauge`, `meter` or `progress-row` as its label followed by its percentage as digits and the word `percent`. (proposed, owner to confirm)
- **[ACC-026]** In linear mode the host MUST read a `tree-list` row with its depth as `level <N>`. For an expandable item it MUST also read the word `expanded` or `collapsed` after the position. (proposed, owner to confirm)
- **[ACC-027]** In linear mode the host MUST read a secret `text-input` as `filled` or `empty`, and MUST NOT print any character of its value. A plugin MUST NOT copy a secret value into a label, a status row or a tooltip. (proposed, owner to confirm)
- **[ACC-028]** After a re-render that keeps the focused element, the host MUST keep the cursor on the first cell of that element in linear mode. It MUST NOT move the cursor to the first line of the view. (proposed, owner to confirm)
- **[ACC-031]** A plugin MUST give every `spinner` a non-empty `label`. In linear mode the host MUST read a spinner as its label followed by the word `busy`, and the view validator MUST refuse a spinner without a label with `ui.label-missing`. (proposed, owner to confirm)
- **[ACC-033]** When `[ui] screen_reader` changes while plugin views are open, the host MUST render every open plugin view again in the new mode within one frame and MUST deliver `theme-changed`, so that a plugin needs no code and no restart to switch modes. (proposed, owner to confirm)

## Checklist before release

- Turn on the screen reader setting and read the view line by line. Each line must say what the control does and what state it is in.
- Switch the language to each shipped language and to `en-XA`. Each label must be whole, and no message id may appear.
- Change each status in the view and check that the row text changes in words.
- Confirm that a change that needs attention also has a toast or a desktop notice, and a text row that stays.

## See also

- [Accessibility and theming](/pages/accessibility-and-theming/)
- [Keyboard operation and focus](/pages/a11y-keyboard-and-focus/)
- [Internationalisation](/pages/guide-internationalisation/)
- [Test harness](/pages/reference-test-harness/)

## Production additions (round 3)

- **[PRY-004]** In linear mode the host MUST print at most 200 rows of a %list, table or tree-list per render, centred on the focused row, followed by one line of the form `rows <first> to <last> of <total>`, and a render of a 100000-row table MUST finish within the 10 ms view deadline of the plugin.

## Production additions (round 4)

- **[PRZ-002]** When the user changes the interface language while plugin views are open, the host MUST deliver locale-changed to each client plugin and MUST render every open plugin view again within 1 frame, and a test MUST assert that no label of the old language remains in the rendered rows after that frame.

## Production additions (round 7)

- **[PRU-009]** When the host truncates a label, title or row text with an ellipsis because it exceeds its box, the full untruncated text MUST remain available to the screen reader mode text output and to a reveal action (focus or hover detail), so no information is lost by truncation alone.
