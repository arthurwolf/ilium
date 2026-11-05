/// REQ: PDT-022
#[test]
fn pdt_022_language_sources_applied_in_order() {
    // Observable: Start with `LANG=fr_FR.UTF-8`, `ILIUM_LANG=de` and `--language ja`: `ilium plugin i18n-coverage --json` reports `language: "ja"`; without the flag it reports `de`.
    panic!(
        "PDT-022 not implemented: Language sources MUST be applied in the order `--language`, `ILIUM_LANG`, `[ui] language`, `LC_ALL`, `LC_MESSAGES`, `LANG`, then `en`."
    );
}

/// REQ: PDT-023
#[test]
fn pdt_023_i18n_coverage_prints_one_jsonl_line_per_plugin() {
    // Observable: Run the command with two plugins installed: stdout has two JSON lines, each with those four keys, and the exit code is 0.
    panic!(
        "PDT-023 not implemented: `ilium plugin i18n-coverage` MUST print one JSONL line per installed plugin with `plugin_id`, `language`, `message_count` and `missing_count`, and MUST exit 0."
    );
}

/// REQ: PDT-024
#[test]
fn pdt_024_oversized_catalogue_refused_plugin_loads_in_english() {
    // Observable: Install a plugin with a 300 KiB `de` catalogue and start with `ILIUM_LANG=de`: the log contains `i18n.catalogue-too-large` and the plugin's messages render in English.
    panic!(
        "PDT-024 not implemented: A catalogue larger than 256 KiB MUST be refused with `i18n.catalogue-too-large`, and the plugin MUST still load with English messages."
    );
}

/// REQ: PDT-025
#[test]
fn pdt_025_language_change_rerenders_open_plugin_views_within_500_ms() {
    // Observable: Open two plugin views and change `[ui] language` from `en` to `fr`: the harness observes both views redrawn with French messages within 500 ms.
    panic!(
        "PDT-025 not implemented: A language change MUST re-render every open plugin view within 500 ms."
    );
}

/// REQ: PDT-026
#[test]
fn pdt_026_language_locked_policy_pins_language_for_all_plugins() {
    // Observable: Set `language_locked = "en"` in the system policy and start with `--language fr`: `ilium plugin i18n-coverage --json` reports `language: "en"` for every plugin.
    panic!(
        "PDT-026 not implemented: The administrator policy key `[ui] language_locked` MUST pin the language for all plugins, overriding every other source."
    );
}
