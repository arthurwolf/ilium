---
layout: page
title: Language resolution and translation coverage report
permalink: /pages/reference-language-resolution-and-coverage/
---

## About this page

This page states how Ilium chooses the language of its user interface and of every plugin view, from the setting, the command-line flag, the `ILIUM_LANG` variable, and the POSIX locale variables. It also covers how a language change reaches open views, how an operator lists the translation coverage of every installed plugin, what happens to a catalogue that is too large, and how a fleet policy pins the language. Plugin authors use it to know which language their messages are shown in. Operators and administrators use it to check which language a machine runs and to lock it. The message catalogue format is explained in [Internationalisation](/pages/guide-internationalisation/), and the policy file is explained in [Administration policy](/pages/reference-admin-policy/).

## Order of sources (setting, flag, ILIUM_LANG, POSIX variables)

Ilium reads the language from six sources, and it uses the first one that has a value. The order is fixed. The first source is the `--language` command-line flag. The second is the `ILIUM_LANG` environment variable. The third is the `language` key of the `[ui]` table in the configuration file, written as `[ui] language`. The fourth is `LC_ALL`, the fifth is `LC_MESSAGES`, and the sixth is `LANG`. When none of these has a value, Ilium uses `en`.

A flag always beats the environment, and the environment always beats the configuration file. For example, when the shell sets `LANG=fr_FR.UTF-8` and `ILIUM_LANG=de`, a run with no flag uses `de`. A run with `--language ja` uses `ja` and ignores both variables. The language that a run uses is shown by `ilium plugin i18n-coverage` (see [Coverage command](#coverage-command) below), so an operator can check the result without starting the interface.

## Change at run time

The language can also change while Ilium is running. When the user changes `[ui] language` from `en` to `fr`, every plugin view that is open is redrawn with French messages within 500 ms.

## Coverage command

The command `ilium plugin i18n-coverage` lists the translation coverage of every installed plugin. It prints one JSON object per line, which is JSON Lines, and one line describes one plugin. Each line has four keys. The key `plugin_id` is the identifier of the plugin. The key `language` is the language used for that plugin in this run, after the order of sources has been applied. The key `message_count` is the number of messages the plugin defines, and the key `missing_count` is the number of those messages that the catalogue for `language` does not contain. The command exits with status 0 when it runs, whatever the coverage is.

With two plugins installed, the output has two lines. An example line looks like this:

```json
{"plugin_id":"demo-plugin","language":"de","message_count":40,"missing_count":3}
```

The examples on this page use the `--json` flag, which selects this output. Run the command with `--language ja` to check what a machine would show in Japanese, and run it without the flag to check the language from the environment and the configuration file.

## Broken catalogue behaviour

A catalogue is a file that holds one plugin's messages for one language. A catalogue larger than 256 KiB, which is 262,144 bytes, is refused. Ilium writes the diagnostic `i18n.catalogue-too-large` to the log. The plugin still loads, and its messages are shown in English. To check this rule, install a plugin with a 300 KiB `de` catalogue and start Ilium with `ILIUM_LANG=de`. The log then contains `i18n.catalogue-too-large`, and the plugin's messages render in English.

## Fleet policy

An administrator pins the language for every machine with the key `[ui] language_locked` in the system policy. A pinned language overrides every other source, including the `--language` flag, `ILIUM_LANG`, the `[ui] language` setting, and the POSIX variables. For example, when the system policy sets `language_locked = "en"` and Ilium is started with `--language fr`, the command `ilium plugin i18n-coverage --json` reports `language: "en"` for every plugin. The administrator policy file as a whole is described in [Administration policy](/pages/reference-admin-policy/).

## Requirements

- **[PDT-022]** Language sources MUST be applied in the order `--language`, `ILIUM_LANG`, `[ui] language`, `LC_ALL`, `LC_MESSAGES`, `LANG`, then `en`.
- **[PDT-023]** `ilium plugin i18n-coverage` MUST print one JSONL line per installed plugin with `plugin_id`, `language`, `message_count` and `missing_count`, and MUST exit 0.
- **[PDT-024]** A catalogue larger than 256 KiB MUST be refused with `i18n.catalogue-too-large`, and the plugin MUST still load with English messages.
- **[PDT-025]** A language change MUST re-render every open plugin view within 500 ms.
- **[PDT-026]** The administrator policy key `[ui] language_locked` MUST pin the language for all plugins, overriding every other source.
