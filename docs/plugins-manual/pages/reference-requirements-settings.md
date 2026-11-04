---
layout: page
title: Requirements for settings
permalink: /pages/reference-requirements-settings/
---

## Requirements for settings

Normative behaviour of settings tabs, sections, rows, values, validation and saving.

The rows follow the format and the observation channels of the [requirements index](/pages/reference-requirements/). The id prefix is `SET`.

| Id | Requirement | Source | Observable test |
|---|---|---|---|
| [SET-001] | A plugin MUST have at most 2 settings tabs, 8 sections per tab, 24 rows per section (a group counts as one row plus its children) and 12 children per group. | [limits](/pages/reference-settings-schema/#limits) | (install-error) 25 rows in a section -> manifest.settings-limit. |
| [SET-002] | A plugin MUST have at most 128 value keys. A value key MUST be unique inside the plugin. | [limits](/pages/reference-settings-schema/#limits) | (install-error) two rows with key "a" -> manifest.settings-key-clash. |
| [SET-003] | Tab, section and row titles MUST be at most 32, 40 and 48 characters. A description MUST be at most 200, info text 120, a button label 20. | [limits](/pages/reference-settings-schema/#limits) | (install-error) a 49 character row title -> manifest.settings-limit. |
| [SET-004] | A first-party plugin (limits_tier first-party) MAY raise the settings caps. Others MUST NOT. | [limits](/pages/reference-settings-schema/#limits) | (install-error) 9 tabs as T2 -> manifest.settings-limit. |
| [SET-005] | Plugin sections beyond 6 in one built-in tab MUST be dropped with settings.section-dropped. Plugin tabs beyond 16 in the rail MUST be dropped with settings.tab-dropped. | [limits](/pages/reference-settings-schema/#limits) | (load-warning) 7 plugin sections in one tab -> one is dropped and the warning is shown. |
| [SET-010] | Every default and option MUST be checked at install. | [validation and saving](/pages/reference-settings-schema/#validation-and-saving) | (install-error) a number default above max -> a manifest.settings-* error naming the JSON path. |
| [SET-011] | A stored value that no longer matches its row MUST be ignored and the default MUST apply. | [validation and saving](/pages/reference-settings-schema/#validation-and-saving) | (event-delivery) restore a stored value 99 with max 10 -> the value read is the default. |
| [SET-020] | The host MUST NOT clamp a stored value. A number typed outside min and max MUST be refused with the row note and the stored value MUST stay. The stepper MUST stop at the bounds. | [validation and saving](/pages/reference-settings-schema/#validation-and-saving) | (screen-text) type 99 in a number with max 10 -> the row note shows and the stored value is unchanged. |
| [SET-021] | Saving MUST run in this order: schema check, settings-validate, the write, settings-changed. | [validation and saving](/pages/reference-settings-schema/#validation-and-saving) | (event-delivery) save a valid value -> the plugin sees settings-validate before settings-changed. |
| [SET-022] | A settings-validate answer MUST arrive within 100 ms. On timeout the save MUST be refused. | [limits](/pages/reference-settings-schema/#limits) | (completion) answer in 150 ms -> save refused and the old value stays. |
| [SET-023] | Toggle, choice and dialog confirm MUST be written at once. Number steps and typed numbers or text MUST be written 600 ms after the last change or after debounce_ms. A text-area MUST be written 600 ms after the last keystroke, or 1 s when its text is above 10,000 bytes; debounce_ms overrides both. | [validation and saving](/pages/reference-settings-schema/#validation-and-saving) | (event-delivery) type three characters in 300 ms -> one settings-changed, 600 ms after the last. |
| [SET-024] | A text value MUST be at most 4,096 characters, a text-area value 65,536, and all values of one plugin 256 KiB. Above that save MUST fail with invalid-argument. | [limits](/pages/reference-settings-schema/#limits) | (sync-return) set-json with 300 KiB -> invalid-argument. |
| [SET-031] | set-json MUST write at once. | [validation and saving](/pages/reference-settings-schema/#validation-and-saving) | (event-delivery) set-json -> settings-changed without delay. |
| [SET-032] | The host MUST show the saving state on a row from the user's edit until the write completes or is refused: the trailing marker `Saving…` is visible while the debounce runs or a `set-json` is pending, and gone afterwards. | [settings screens](/pages/settings-screens/) | (event-delivery) type into a text row -> marker visible until the write at 600 ms, then gone. |
| [SET-050] | The calls set-info, set-choices, set-row-note, set-row-state, set-item-states, set-choice-items, set-info-rich and set-meter MUST be limited to 10 per second per plugin together. A call over the rate MUST return rate-limited and the previous state MUST stay. | [limits](/pages/reference-settings-schema/#limits) | (sync-return) 11 calls in a second -> the 11th returns rate-limited. |
| [SET-051] | set-info and set-row-note text MUST be at most 120 characters. Longer MUST return invalid-argument. | [limits](/pages/reference-settings-schema/#limits) | (sync-return) 121 characters -> invalid-argument. |
| [SET-060] | A choice or list MUST have at most 64 options or items; a map at most 64 entries; a meter at most 8 segments; a notice text at most 200 characters. | [limits](/pages/reference-settings-schema/#limits) | (install-error) 65 options -> manifest.settings-limit. |
| [SET-061] | A row whose depends_on names a missing key MUST fail with manifest.settings-depends. | [error codes](/pages/reference-settings-schema/#error-codes) | (install-error) depends_on "nope" -> manifest.settings-depends. |
| [SET-062] | A row key that is not a valid key MUST fail with manifest.settings-key. | [error codes](/pages/reference-settings-schema/#error-codes) | (install-error) key "A b" -> manifest.settings-key. |
| [SET-063] | A button command, glyph, help id or tab anchor that does not exist MUST fail with manifest.settings-command, settings-glyph, settings-help or settings-tab-anchor. | [error codes](/pages/reference-settings-schema/#error-codes) | (install-error) a button naming an unknown command -> manifest.settings-command. |
| [SET-064] | A settings schema that uses a keyword outside the supported subset MUST fail with manifest.schema-keyword. | [error codes](/pages/reference-settings-schema/#error-codes) | (install-error) use "oneOf" in a schema -> manifest.schema-keyword. |
| SET-000 | The Settings language row MUST offer a first value that leaves `[ui] language` unset, so the environment order of [USR-311] applies, then `en` and each launch language of [AUT-624], each shown by its own name in that language (`Deutsch`, `Français`) and never translated into the current language. The row MUST NOT offer `en-XA`, which is accepted only from `--language`, `ILIUM_LANG` or a hand-written config value (proposed, owner to confirm). | [core-plugin-settings](/pages/core-plugin-settings/) | Render the User Interface tab and assert the language row's options are the unset value, `English`, `Deutsch` and `Français` in every active language and that `en-XA` is absent; pick the unset value with `LANG=de_DE.UTF-8` and assert the next session starts in `de`; start with `--language en-XA` and assert the pseudo-locale is active. |
| SET-006 | A `[ui] language` value that is not a well-formed BCP 47 tag MUST be rejected when the config loads, with a message that names the key and the bad value. The host MUST then continue with the next source of [USR-311] as if the setting were unset, and `ilium plugin doctor` MUST print the rejected value next to the chosen tag (proposed, owner to confirm). | [core-plugin-settings](/pages/core-plugin-settings/) | Write `[ui] language = "english!"` with `LANG=fr_FR.UTF-8`, start the host and assert a notice naming `[ui] language` and `english!`, that the session runs in `fr`, and that `ilium plugin doctor` prints both the rejected value and `fr` from `LANG`. |

### See also

- [Requirements index](/pages/reference-requirements/)
- [Error codes and limits](/pages/reference-errors-and-limits/)
