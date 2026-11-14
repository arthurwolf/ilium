### Added
- Plugin views edit text, numbers and scroll positions in the host: a text field, a text area and a number field keep their cursor, selection and scroll state, accept pasted text and keyboard editing, and send the edited value to the plugin.
- Plugins contribute editable settings rows: a switch, a choice and a number appear in the Settings plugin list and are written, after a schema check, to the plugin's section of `config.toml`.
- Plugin settings live in `[plugins."<id>"]` of `config.toml` under the shared file lock; `set-json` takes a merge patch (null resets to the default), an unchanged patch sends no event, and a plugin reads other plugins' settings only with the `settings.read-all` or `settings.write-all` grants.
- Plugins contribute terminal link detectors (`contributes.link_detectors`): the first match under the clicked cell, in manifest then plugin order, wins; invalid or oversized patterns are skipped, and the built-in link handling stays the fallback.

### Changed
- The Settings plugin list shows the effective value of a contributed setting (stored value over the default) and says whether Enter changes it.
