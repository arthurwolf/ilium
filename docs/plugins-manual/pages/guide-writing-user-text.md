---
layout: page
title: Writing user-visible text
permalink: /pages/guide-writing-user-text/
---

## Writing user-visible text: notices, errors, titles and help

This guide says how a plugin words the text a person reads: command titles, notices, error messages, tooltips, footer hints and help. With every core feature a plugin, the words of plugins are the words of the product. Kernel codes already follow fixed rules ([USR-347], [USR-171], [ABI-082]). This page gives plugin text the same discipline. For the documentation files a plugin ships, see [Plugin documentation guide](/pages/guide-plugin-documentation/). For translation mechanics, see [Internationalisation](/pages/guide-internationalisation/).

Terms:

- **Surface**: a place where text appears: a title, a toast, a footer, a tooltip, a help topic.
- **Notice**: a message the plugin shows with a toast or a status line.
- **Cell**: one terminal column. Limits on this page count cells, not bytes or characters, so a wide character counts 2.

## You will need

- A plugin project with at least one `contributes.commands` entry or one notice.
- `ilium plugin lint`. See [Check and lint rules](/pages/reference-check-and-lint-rules/).

## Which rules are already requirements

| Rule | Id | Source |
|---|---|---|
| A command `title` is sentence case with no final period | DEV-791 | [Plugin documentation guide](/pages/guide-plugin-documentation/) |
| A refused install starts `Cannot install <name> <version>.` | USR-171 | [Security](/pages/security/) |
| Every user-visible error code has a cause sentence and a next-step sentence | USR-347 | [Plugin store](/pages/user-plugin-store/) |
| Only the code and the reason-word prefix of a host message are stable | ABI-082 | [Errors and limits](/pages/reference-errors-and-limits/) |
| A missing translation falls back to one default language and never shows a message id | AUT-608 | [Internationalisation](/pages/guide-internationalisation/) |

The rest of this page applies the same shape to text that a plugin writes itself. The words MUST and SHOULD below describe what `lint.text-*` and review enforce; they do not add requirement ids.

## Titles

A title names a command, a view, a setting row or a help topic.

- Use sentence case: `Clear badges`, not `Clear Badges`. Keep proper names as they are (`Git`, `Markdown`).
- No final period. No exclamation mark. No trailing colon.
- Start with a verb for a command (`Turn on badges`), and with a noun for a view or a setting (`Badge style`).
- At most 48 cells.
- Do not repeat the plugin name. The command palette already shows it beside the title.

| Do | Do not |
|---|---|
| `Clear badges` | `Clear Badges.` |
| `Badge style` | `badgeStyleSetting` |
| `Remove all bookmarks` | `Bookmarks: remove all!` |

## Notices

A notice has up to two sentences of body.

1. **What changed.** Say what happened to the thing the user cares about, in plain words. Name the thing.
2. **What the user can do.** Say the next step, or say that nothing is needed.

Write for the user's view of the world, never for what the code did.

| Do | Do not |
|---|---|
| `Removed 3 bookmarks from notes.md.` | `deleteBookmarks() returned 3.` |
| `Badges are on for all agent panes.` | `Setting badges.enabled set to true.` |
| `Nothing to clear. No badges are showing.` | `Operation completed with no effect.` |

- Do not put an identifier, a code or a path in the first sentence. A code or a path MAY follow in the second sentence or in the details view.
- Do not blame the user: write `The file is not valid JSON.`, not `You gave an invalid file.`
- No exclamation marks. No jokes, wordplay or idioms: they do not translate and age badly.
- Use the same verb for the same action everywhere (see below).

## Error notices

An error notice has the shape of [USR-347]: a cause sentence, then a next-step sentence, and it names the thing it is about.

```text
Cannot export notes.md. The folder is read-only.
Choose another folder, or change its permissions.
```

- The first sentence says what could not be done and to what. For an install, the host text already starts `Cannot install <name> <version>.` ([USR-171]); a plugin that shows its own failure uses the same form `Cannot <verb> <thing>.`
- The cause is one sentence in plain words. It states the reason, not the exception type.
- The next step is one sentence the user can act on. If nothing can be done, say so: `Try again later; nothing is needed from you.`
- A plugin error code MAY be shown after the two sentences, as `Code: <CODE>`. Every code a user can see MUST have a cause sentence and a next-step sentence that `ilium plugin explain <code>` prints ([USR-347]).
- Never branch on, or copy, the text of a host message. Only its code and reason-word prefix are stable ([ABI-082]). Show your own sentence next to the host code instead.

| Do | Do not |
|---|---|
| `Cannot save settings. The disk is full. Free some space, then try again.` | `Error 28!` |
| `Cannot connect to the server. Check your network, then try again.` | `Failed. You must have a bad connection.` |

## Length limits per surface

Limits count cells. A text over its limit is cut by the host with `…` at the end; the cut is for safety, not a style to rely on.

| Surface | Limit | Notes |
|---|---|---|
| Title (command, view, setting row) | 48 cells | One line |
| Toast body | 3 lines | After wrapping at the toast width |
| Footer hint | 110 cells | One line, no line break |
| Tooltip | 1 line | Wrapped text is not shown |
| Setting `help` | 2 lines | Longer text belongs in a help topic |
| Help topic | 20 KiB | See [DEV-786] |

- Leave room for translation: text in other languages is often 30 percent longer. Keep English text at most 80 percent of a limit when it will be translated.
- A toast that needs more than three lines SHOULD link a help topic instead of growing.

## Consistent verbs

| Action | Use | Do not use |
|---|---|---|
| Switch a feature on or off | `Turn on`, `Turn off` | `Enable`, `Disable`, `Activate`, `Toggle` |
| Take something away | `Remove` | `Delete` for a plugin or entry the user can add again, `Uninstall` for a non-plugin |
| Add a plugin | `Install` | `Add`, `Get` |
| Close a view | `Close` | `Exit`, `Dismiss` |
| Confirm | `OK` | `Yes` for a non-question |

Use `Delete` only for data that cannot be recovered, and say so in the cause sentence.

## Text that survives translation

- Never join pieces of a sentence by code. `"Deleted " + count + " notes"` cannot be translated. Write one message with a named placeholder: `Deleted {count} notes`.
- Name every placeholder by meaning: `{count}`, `{name}`, `{folder}`. Never `{0}` or `{x}`.
- Do not put a placeholder at the start of a sentence if its value can begin with a lower-case letter.
- A message with a count MUST be a plural message with the CLDR categories; `other` is required ([USR-313]). Do not write `note(s)`. Do not add `s` in code.
- Do not build a sentence from a translated word plus another translated word. Give the translator the whole sentence.
- Give a context note to the translator for any short or ambiguous message (`Open` as a verb or as a state).
- A missing translation falls back to English and never shows a message id ([AUT-608]). A placeholder mismatch makes that message fall back too ([USR-312]).
- Keep numbers, dates and file sizes out of the message: pass them as placeholders and let the host format them.

## Lint rules for text

These rules are proposed additions to [Check and lint rules](/pages/reference-check-and-lint-rules/). Until the owner confirms them, review checks them by hand.

| Rule | Level | Fires when |
|---|---|---|
| `lint.text-title-case` | warning | A `title` is not sentence case (DEV-791) |
| `lint.text-final-period` | warning | A `title` ends with `.`, `:` or `!` |
| `lint.text-title-too-long` | warning | A `title` is over 48 cells |
| `lint.text-exclamation` | warning | A message contains `!` |
| `lint.text-concatenated` | warning | A message is built by joining strings with a number or a name |
| `lint.text-placeholder-unnamed` | warning | A placeholder is numeric or has a one-letter name |
| `lint.text-plural-missing-other` | error | A plural message has no `other` form |
| `lint.text-error-no-next-step` | warning | An error message has no second sentence |
| `lint.text-banned-verb` | warning | A title uses `Enable`, `Disable` or `Toggle` |

## Checklist for the quality page

Copy this list into the release review of [Quality checklist](/pages/guide-quality-checklist/).

- [ ] Every title is sentence case, has no final period, and is at most 48 cells.
- [ ] Every notice says what changed in the first sentence, with no code or identifier.
- [ ] Every error notice has a cause sentence and a next-step sentence, and names its subject.
- [ ] No message blames the user, uses `!`, or relies on a joke or an idiom.
- [ ] Verbs match the table: Turn on/off, Remove, Install.
- [ ] No message is built by string concatenation; every placeholder has a name; every count uses plural forms.
- [ ] Each surface is within its limit, with room for translation.
- [ ] The text works with the plugin's default language only, and a second language falls back without showing an id.

## Edge cases

- **A host message inside yours.** Show your sentence first and the host code after it. Do not paste the host sentence, because a minor release may reword it ([ABI-082]).
- **A right-to-left language.** Keep placeholders as whole words; the host handles direction. Do not insert direction marks by hand.
- **A very long name.** A placeholder value, such as a file name, MAY be shortened by the host in the middle with `…`. Do not rely on its full text being visible.
- **A message with no possible next step.** Write `Nothing is needed from you.` as the second sentence.

## See also

- [Plugin documentation guide](/pages/guide-plugin-documentation/)
- [Internationalisation](/pages/guide-internationalisation/)
- [Quality checklist](/pages/guide-quality-checklist/)
- [Check and lint rules](/pages/reference-check-and-lint-rules/)
- [Errors and limits](/pages/reference-errors-and-limits/)
