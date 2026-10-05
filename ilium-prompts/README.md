# Application prompts

All application-authored model instructions, examples, tool descriptions, generated agent messages and their context layouts live in `templates/**/*.hbs`. Rust supplies typed state, serialization, clipping budgets and runtime user text.

The `naming`, `voice`, `agent`, `conversion` and `compaction` catalogs explicitly embed these files using `include_str!`. Editing a template changes the next compiled artifact; rebuild to use it. Both debug and release binaries render embedded text without reading template files, locating a source checkout or accepting a template-directory setting.

## Edit or add a prompt

1. Edit its `.hbs` file. Preserve deliberate whitespace: files do not automatically gain a trailing newline.
2. For a new template, add an embedded constant and a unique `feature/path` entry to that feature's `TEMPLATES` list. The name matches the file path below `templates`, without `.hbs`.
3. Use the constant directly for fixed text. Use `render(name, &typed_context)` when the caller can propagate errors. `render_value(name, &json_context)` is for fixed catalog names and JSON values at infallible message-producing boundaries.
4. Add or update tests for the affected variants and run `cargo test -p ilium-prompts`. Treat frozen parity hashes as migration evidence; an intentional wording change should update its hash with a reviewed explanation.

`build.rs` validates every template with the same Handlebars engine used at runtime, rejects duplicate names and checks that every `.hbs` file is registered. Missing included files fail compilation. The runtime registry is cached once and performs no file I/O.

## Variables and partials

Contexts are serializable Rust types or `serde_json::Value`. Preserve JSON encoding for untrusted naming evidence and serialize structured payloads in Rust. Display precision and Debug formatting stay at the caller when replacing a Rust formatting expression; template fields receive those formatted values. Migrated `v0`, `v1`, etc. preserve the original ordered format slots; inspect the caller listed in `CATALOG.md` for their meanings.

HTML escaping is disabled because these are plain-text prompts. Inserted values are literal: a user's `{{...}}` text is never interpreted recursively. Existing conditional/iteration behavior is preserved, including optional missing fields. Every catalog template is also registered for named partial use, such as `{{> naming/label-instructions}}`. Register reusable fragments once, and pass any required values explicitly. Keep instruction fragments separate from untrusted data.

## Verification

`tests/source_parity.rs` checks extracted bytes and formatting skeletons against hashes captured from the original sources. `format_parity.rs` checks 117 interpolated layouts; `template_parity.rs` compares 12 full naming/restructure variants with the original outputs. Rendering tests cover registration, invalid sources, missing catalog entries, named partials, Unicode and literal user text. Existing caller tests exercise naming styles, retries, voice control, chatroom generation, progress outcomes and conversion behavior.

The `prompt_probe` example accepts `--input /absolute/path/cases.jsonl`. Each input line contains `name` and `context`; it emits JSONL results with byte count and SHA256. An optional `baseline_source` field renders a captured original for migration comparison. Build with `cargo build -p ilium-prompts --release --example prompt_probe`, copy the artifact to an isolated directory and run it there to verify rendering without source-template access.

`CATALOG.md` maps every extracted call site to its new template. Machine identifiers, typed enum/state labels, user-authored data, provider responses and general UI/OS notification text retain their existing ownership. Demos, archived task briefs and development workflow prompts are outside this catalog.
