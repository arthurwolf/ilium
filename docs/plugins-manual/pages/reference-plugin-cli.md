---
layout: page
title: Plugin command-line reference
permalink: /pages/reference-plugin-cli/
---

## The ilium plugin command line

This page is the normative reference for every `ilium plugin` command: what it reads, what it writes, which flags it takes, which records it prints and which exit code it returns. Other pages show short tables of commands. When a short table and this page disagree, this page wins.

The `ilium plugin` commands are part of the `ilium` binary. Nothing else is installed. A plugin author uses them to create, build, check, test, pack, sign and run a plugin. A user or administrator uses them to install, list, inspect, grant, enable, disable and remove plugins. A test author uses the output formats on this page to write golden tests for the command line itself.

Rules on this page carry stable ids such as `[DEV-004]`. A test cites the id. The full list is in [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/). The words MUST, SHOULD and MAY have their usual meaning: MUST is a requirement, SHOULD is a requirement unless there is a stated reason, MAY is an option.

Terms used on this page:

- **Session**: one running Ilium server together with its clients. A session owns the loaded plugins. See [Architecture](/pages/architecture/).
- **Offline command**: a command that works with no running session, so a continuous-integration job can run it.
- **JSONL**: JSON Lines. One JSON object per line, UTF-8, no pretty printing.
- **Record**: one JSON object that a command prints. Its `type` field names its kind.

## Conventions

- **[DEV-001]** `<dir>` MUST default to the current working directory. A command that takes `<dir>` MUST treat it as the project directory, the one that holds `manifest.json`.
- **[DEV-002]** A command that builds MUST detect the language from the project files. A `Cargo.toml` means Rust. A `package.json` means TypeScript. Both files together fail with `toolchain.language-ambiguous`. Neither file fails with `toolchain.language-unknown`.
- **[DEV-003]** Every command MUST accept the global flags in the table below. A flag that does not apply to a command MUST be accepted and ignored, except `--session`, which a command that never contacts a session MUST also ignore without error.

| Flag | Type | Default | Meaning |
|---|---|---|---|
| `--session <name>` | string | `default` | Names the session that a session command talks to. |
| `--cwd <path>` | path | the current directory | The project directory that identifies the session, as for the bare `ilium` command. It is not `<dir>`. |
| `--quiet` | switch | off | Suppresses `progress` records and, in text mode, all output except errors and the final line. |
| `--no-color` | switch | off | Prints text output with no colour. The environment variable `NO_COLOR` has the same effect. |
| `--json` | switch | off | Selects JSONL output for a command whose default output is text. |
| `--help` | switch | off | Prints usage to standard output and exits with 0. |

- **[DEV-108]** `ilium plugin --help` MUST list every subcommand with a one-line meaning, `ilium plugin <command> --help` MUST list every flag of that command's flag table with its type, default and meaning, and both MUST end with the line `Codes: ilium plugin explain <code>   Environment: ilium plugin doctor` (proposed, owner to confirm).

- **[DEV-004]** A session command MUST find its session this way: the session named by `--session`, in the project that `--cwd` names, defaulting to the session called `default` of the current directory. No session found at that address MUST fail with `session.unreachable` and exit code 6. A command MUST NOT start a session itself. The one exception to the exit code is `kill`, which with no session writes the kill state file and exits 0 ([SCU-003](/pages/security-containment-states/)); it still does not start a session.
- **[DEV-005]** The commands in the offline column of the table below MUST work with no running session, no network and no home-directory configuration beyond the trust store.
- **[DEV-006]** A session command that cannot reach its session MUST print one `error` record with the code `session.unreachable`, then `summary`, then `result`, and exit with 6. It MUST NOT retry.

- **[DEV-091]** The pipeline commands `new`, `check`, `lint`, `test` (without `install_core(...).real()`), `build`, `pack`, `sign`, `keygen`, `inspect <file>`, `doctor`, `schema` and `--version` MUST run from the `ilium` executable alone, with the embedded test host, the WIT package, the schemas and the templates compiled in, and MUST NOT read any other file of the host installation, so a single downloaded file is enough on a CI runner; only `real()` MAY need the bundled core plugin set of DEV-483 (proposed, owner to confirm).

| Command | Needs a session | Notes |
|---|---|---|
| `new` | no | Writes files only. |
| `build` | no | Needs the language toolchain. |
| `check` | no | Reads files only. |
| `lint` | no | Reads files only. |
| `test` | no | Native fixtures and `--component` both run in an embedded test host. |
| `pack` | no | Needs a built `dist/`. |
| `keygen` | no | Writes a key pair. |
| `sign` | no | Reads a key and edits a package file. |
| `trust` | no | Edits the trust store file. A running session reloads it on change. |
| `publish` | no | Needs a registry. Defined in [Distribution commands](/pages/reference-plugin-distribution-cli/). |
| `inspect <file>` | no | Reads a package or a component. |
| `inspect <id>` | yes | Reads an installed plugin. |
| `doctor` | no | The session check reports `warn` when no session is found. |
| `install`, `uninstall`, `enable`, `disable`, `grants`, `list` | yes | They change or read the loaded set. |
| `dev`, `logs`, `log-level`, `trace`, `profile` | yes | They read or drive a live instance. |
| `kill <id> --reason <text>` | no | Stops a plugin and keeps it from activating until `unkill` ([HST-516], [HST-517]). It works in safe mode and without a network. With no session, or with no server reply within 2 s, it writes the kill state file, prints `path: offline` and exits 0 ([SCU-003](/pages/security-containment-states/)). |
| `unkill <id>` | yes | Clears an admin kill. It works in safe mode and without a network. With no session it exits 6 and changes nothing ([SCU-003](/pages/security-containment-states/)). See [If you suspect a plugin is malicious](/pages/user-plugin-security-and-privacy/). |

The output default of each command is in [Which commands print text](#which-commands-print-text). Every subcommand, with its session need, output mode, records and exit codes, is in the [Command inventory](#command-inventory).

## Exit codes

Codes 7 (policy) and 8 (network) are added by [Distribution commands](/pages/reference-plugin-distribution-cli/#exit-codes). Code 75 (busy) is returned only by `mirror sync` when another run holds the mirror lock (`mirror.sync-busy`, [PRY-035]).

- **[DEV-007]** Every command MUST exit with one of the codes below, and the `exit_code` field of its `result` record MUST equal the process exit code.

| Code | Name | Meaning |
|---|---|---|
| 0 | ok | The command did what was asked. Warnings do not change the code. |
| 1 | failed | The operation failed: a build error, a refused install, a write error. |
| 2 | usage | A flag or argument is wrong, or a reserved command was used. |
| 3 | findings | `check`, `lint` or `test` ran to the end and found problems. |
| 4 | environment | A tool, target, SDK or version that the command needs is missing or too old, or the install lock is held (`install.locked`). |
| 5 | not found | The plugin id, package, key or publisher does not exist. |
| 6 | no session | A session command found no reachable session. |
| 130 | interrupted | The user pressed Ctrl+C. |

- **[DEV-008]** Exit code 0 MUST be returned only when `result.ok` is `true`.
- **[DEV-009]** Exit code 1 MUST be returned for any failure that is not covered by a more specific code.
- **[DEV-010]** Exit code 2 MUST be returned for a usage error before any work starts. The command MUST NOT write files before it returns 2.

- **[DEV-093]** `ilium plugin <word>` where `<word>` is not a subcommand, and `ilium plugin` with no subcommand, MUST fail with `usage.unknown-command` and exit 2 before any work starts; the `error` message MUST name `<word>` and, when a subcommand is within Damerau-Levenshtein distance 2 (proposed, owner to confirm) of it, the `hint` MUST read `Did you mean <subcommand>?`, otherwise `Run ilium plugin --help for the list of commands.` (proposed, owner to confirm).
- **[DEV-011]** Exit code 3 MUST be returned when a `check` finding of severity `error` exists, when `lint --deny warnings` has any finding, when a lint rule of severity `error` fires, or when at least one test case does not pass. A finding of severity `warn` under `check` or plain `lint` returns 0.
- **[DEV-012]** Exit code 4 MUST be returned when a `toolchain.*` code stops the command, and when `doctor` reports any item with status `fail`.
- **[DEV-013]** Exit code 5 MUST be returned for `plugin.not-found`, `trust.not-found` and `inspect.id-unknown`.

- **[DEV-094]** An `error` record with code `plugin.not-found` for an id argument MUST carry a `hint`: when an installed id is within Damerau-Levenshtein distance 3 (proposed, owner to confirm) of the argument the hint MUST read `Did you mean <id>?`, otherwise `Run ilium plugin list for the installed ids.` (proposed, owner to confirm).
- **[DEV-014]** Exit code 6 MUST be returned for `session.unreachable` and `session.not-found`.
- **[DEV-015]** Exit code 130 MUST be returned after an interrupt. The command MUST print `summary` and `result` with `ok` set to `false` before it exits, and MUST leave no half-written output file (see the atomic write rule below).

## Output model

### The JSONL envelope

- **[DEV-016]** A JSONL command MUST print one JSON object per line on standard output, with the field `type` as the first key. Human text, if any, MUST go to standard error.
- **[DEV-017]** A JSONL command MUST print exactly one `summary` record and then exactly one `result` record, in that order, as its last two lines. This fixes the older disagreement between pages. The `summary` comes first, and the `result` is always the final line, so a script can read the last line to get the outcome.
- **[DEV-018]** Every size MUST be an unsigned integer in bytes. Every duration MUST be an unsigned integer in milliseconds, and its key MUST end in `_ms`. Every hash MUST be 64 lowercase hexadecimal characters of SHA-256. Every path MUST use `/` and be relative to the project directory unless the key says `absolute`.
- **[DEV-019]** A record MUST NOT contain a field that this page does not define for its type, except `details`, an optional object that a record MAY carry. A consumer MUST ignore fields it does not know, so later minor versions can add fields.
- **[DEV-020]** `--quiet` MUST suppress `progress` records and nothing else.

### The record catalogue

| `type` | Fields | Printed by |
|---|---|---|
| `progress` | `step`, `message`, optional `current`, `total` | long commands |
| `warning` | `code`, `message`, optional `file`, `line`, `column` | all |
| `error` | `code`, `message`, optional `file`, `line`, `column`, `key_path`, `hint` | all |
| `artifact` | `kind`, `path`, `sha256`, `bytes` | `build`, `pack`, `sign`, `keygen`, `test --record` |
| `check` | `rule`, `severity`, `message`, optional `file`, `line`, `column`, `key_path` | `check` |
| `lint` | `rule`, `severity`, `message`, optional `file`, `line`, `column`, `key_path`, `fix`, `suppressed` | `lint` |
| `test-case` | `name`, `fixture`, `side`, `status`, `duration_ms`, optional `diff`, `reason` | `test` |
| `diff` | `subject`, `expected`, `actual`, optional `first_difference` | `test`, `inspect --verify` |
| `submission` | `index`, `correlation`, `intent`, optional `dedup_key` | `test`, `trace` |
| `outcome` | `correlation`, `outcome`, optional `code` | `test`, `trace` |
| `reload` | `plugin`, `side`, `version`, `restored_bytes`, `restore_dropped`, `build_ms` | `dev` |
| `log` | `at`, `plugin`, `side`, `level`, `message` | `dev`, `logs --json` |
| `timing` | `plugin`, `side`, `export`, `calls`, `p50_ms`, `p99_ms`, `max_ms`, `deadline_ms` | `dev --timings`, `profile` |
| `trace` | `at`, `plugin`, `side`, `kind`, `detail` | `trace --json` |
| `plugin` | `id`, `version`, `tier`, `state`, `dev`, `sides`, `grants_granted`, `grants_requested` | `list --json` |
| `preview` | `locale`, `name`, `description`, `publisher`, `grants`, `license`, `contact`, `readme`, `screenshots` | `preview --json` |
| `grant` | `plugin`, `grant`, `risk`, `state` | `grants --json` |
| `inspect` | `section`, `data` | `inspect --json` |
| `doctor-check` | `item`, `status`, `message`, `fix` | `doctor` |
| the types of [Records added by later requirements](#records-added-by-later-requirements) | see that table | the commands named there |
| `summary` | `duration_ms`, `warnings`, `errors` | all JSONL commands |
| `result` | `ok`, `exit_code` | all JSONL commands |

- **[DEV-021]** The `step` field of `progress` MUST be one of `toolchain`, `compile`, `bundle`, `component`, `api-version`, `strip`, `inventory`, `pack`, `sign`, `check`, `lint`, `test`, `install`, `watch`, `download`, `verify`, `migrate`, `backup`, `restore`, `export`, `import`, `publish`.
- **[DEV-022]** The `kind` field of `artifact` MUST be one of `component`, `package`, `source-map`, `build-info`, `key`, `public-key`, `signature`, `replay`, `manifest`, `report`, `docs`, `sbom`, `bundle`, `support-bundle`, `debug-endpoint`, `export`, `backup`, `data`.
- **[DEV-023]** The `severity` field of `check` and `lint` MUST be `error` or `warn`. A `lint` record MAY also use `info`.
- **[DEV-024]** The `status` field of `test-case` MUST be `pass`, `fail`, `trap` or `skipped`. The `status` field of `doctor-check` MUST be `ok`, `warn` or `fail`.
- **[DEV-025]** The `side` field MUST be `server`, `client`, `scene` or `host`. `host` marks an entry that the host itself wrote.

A build that succeeds prints:

```json
{"type":"progress","step":"compile","message":"Compiling agent-tools","current":1,"total":3}
{"type":"progress","step":"component","message":"Writing server.wasm","current":2,"total":3}
{"type":"progress","step":"api-version","message":"Embedding ilium:api-version 0.1.0","current":3,"total":3}
{"type":"artifact","kind":"component","path":"dist/server.wasm","sha256":"3f9a0c51d7b8e2a4c6f1903b5d7e8a2c4b6d8f0a1c3e5a7b9d1f3a5c7e9b1d3f","bytes":412806}
{"type":"artifact","kind":"build-info","path":"dist/build.json","sha256":"7a1b9c3d5e7f1a3b5c7d9e1f3a5b7c9d1e3f5a7b9c1d3e5f7a9b1c3d5e7f9a1b","bytes":611}
{"type":"summary","duration_ms":8120,"warnings":0,"errors":0}
{"type":"result","ok":true,"exit_code":0}
```

A failed command prints its `error` records, then `summary` with `errors` greater than 0, then `result` with `ok` false:

```json
{"type":"error","code":"toolchain.rust-target","message":"the wasm32-wasip2 target is not installed","hint":"run: rustup target add wasm32-wasip2"}
{"type":"summary","duration_ms":41,"warnings":0,"errors":1}
{"type":"result","ok":false,"exit_code":4}
```

### Which commands print text

- **[DEV-026]** A command listed as JSONL in the table below MUST print JSONL always. `--json` on such a command MUST be accepted and have no effect.
- **[DEV-027]** A command listed as text MUST print text by default for a person and JSONL with `--json`. The JSONL form MUST end with `summary` and `result` records, as for any JSONL command.
- **[DEV-028]** Text output MUST be stable enough to read but is not a contract. Scripts MUST use `--json`.

| Command | Default output | `--json` form |
|---|---|---|
| `new`, `build`, `pack`, `keygen`, `sign`, `trust add`, `trust remove`, `install`, `uninstall`, `enable`, `disable`, `test`, `dev` | JSONL | the same |
| `check`, `lint` | text | `check` or `lint` records |
| `list`, `trust list`, `grants` | text | `plugin` or `grant` records |
| `logs`, `trace`, `profile` | text | `log`, `trace` or `timing` records |
| `inspect`, `doctor` | text | `inspect` or `doctor-check` records |
| `preview` | text | one `preview` record, and one `lint` record per finding |

Contributed subcommands of the form `ilium <name>` are not `ilium plugin` commands. They are declared in `contributes.cli` and described in [Contributed command-line commands](/pages/api-cli/). There is no `ilium plugin run`. The page on the workspace API uses the contributed form.

### Environment variables

| Variable | Default | Effect |
|---|---|---|
| `NO_COLOR` | unset | Any non-empty value turns colour off, like `--no-color`. |
| `ILIUM_PLUGIN_LOG_LEVEL` | `info` | Read by a session server at start. Sets the default log threshold of every plugin. Values: `trace`, `debug`, `info`, `warn`, `error`. |
| `ILIUM_PLUGIN_UPDATE_GOLDEN` | unset | The value `1` makes `test` and the harness rewrite golden files instead of comparing. Same as `--update`. |
| `ILIUM_PLUGIN_TEST_SEED` | `1` | The seed for the harness random source. Same as `--seed`. |
| `ILIUM_TRUST_STORE` | per-user path | Replaces the trust store path. A CI job uses it to keep keys in a temporary folder. |
| `ILIUM_PLUGIN_SIGNING_KEY` | unset | Path of a secret key file for `sign` when `--key` is not given. |
| `ILIUM_PLUGIN_OFFLINE` | unset | The value `1` is the same as `--offline` on `build`. |

- **[DEV-029]** A flag MUST override the environment variable that has the same effect.
- **[DEV-030]** Output files that a command replaces (`dist/*.wasm`, the package, the signature, golden files, replay files) MUST be written to a temporary file in the same folder and renamed into place, so an interrupt never leaves a half-written file.

## new

Creates a project from a template. The templates, their files and their guarantees are in [Project templates](/pages/reference-project-templates/).

`ilium plugin new <dir> --id <plugin-id> --lang <rust|typescript> (--template <name> | --side <name>) [flags]`

| Flag | Type | Default | Meaning |
|---|---|---|---|
| `<dir>` | path | none, required | The folder to create or fill. |
| `--id` | string | none, required | The plugin id. It must match `[a-z0-9-]+(\.[a-z0-9-]+)+`. |
| `--lang` | `rust` or `typescript` | none, required | The language. |
| `--template` | name | none | `server`, `client`, `both`, `scene`, `pane-kind`, `core-style`, `service-provider` or `extension`. |
| `--side` | name | none | An alias of `--template` for `server`, `client`, `both` and `scene`. |
| `--name` | string | the last label of the id | The display name. |
| `--publisher` | string | the operating system user name | The `publisher` manifest value. |
| `--first-party` | switch | off | Allows an id that starts with `ilium.`. Needed for `core-style`. |
| `--ci` | `github`, `gitlab` or `none` | `none` | Adds a CI file to the project. |
| `--force` | switch | off | Writes into a non-empty folder. It never deletes a file. |

- **[DEV-040]** `new` MUST require `--id`, `--lang` and one of `--template` or `--side`. If both are given, they MUST name the same template, or the command fails with `usage.conflict`.
- **[DEV-041]** `new` MUST fail with `new.dir-not-empty` when `<dir>` exists and holds any file, unless `--force` is given. With `--force` it MUST overwrite only files that the template writes and MUST leave every other file alone.
- **[DEV-042]** `new` MUST fail with `new.id-invalid` for an id that does not match the pattern, and with `new.id-reserved` for an `ilium.` id without `--first-party`.
- **[DEV-043]** `new` MUST fail with `new.template-unknown` for an unknown template name, before it writes any file.
- **[DEV-044]** `new` MUST print one `progress` record with step `inventory` for each file it writes, and the message MUST be the relative path of the file. It needs no network and no toolchain, and it writes no `artifact` record.
- **[DEV-045]** A fresh project MUST pass `check`, `lint --deny warnings`, `test` and `build` with zero warnings, as the template guarantees state.

## build

Compiles the project into one component per declared side. The stages are in [The build pipeline](#the-build-pipeline).

`ilium plugin build [<dir>] [--side <side>] [--debug | --release] [--watch] [--locked] [--offline]`

| Flag | Type | Default | Conflicts | Meaning |
|---|---|---|---|---|
| `--side` | `server`, `client` or `scene` | every declared side | none | Builds one side. |
| `--debug` | switch | off | `--release` | Keeps debug information. |
| `--release` | switch | on | `--debug` | Strips debug information. |
| `--watch` | switch | off | none | Rebuilds after a change. It never installs. |
| `--locked` | switch | off | none | Fails when the lockfile would change. |
| `--offline` | switch | off | none | Uses only cached dependencies. |

- **[DEV-050]** `build` MUST read `manifest.json`, build every declared side and write `dist/<side>.wasm` for each. With `--side`, a side that the manifest does not declare MUST fail with `build.side-undeclared`.
- **[DEV-051]** `build` MUST print one `artifact` record of kind `component` per side, one of kind `build-info` for `dist/build.json`, and, for a `--debug` build, one of kind `source-map` per side for TypeScript.
- **[DEV-052]** `build --watch` MUST build once, then watch the project and rebuild 300 ms after the last change. It MUST NOT install, activate or contact a session. The watch set and its ignore rules are the ones of `dev`. A failed rebuild MUST print its errors and keep watching. `--watch` ends only on interrupt, with exit code 130.
- **[DEV-053]** `--locked` MUST fail with `build.lockfile-missing` when `Cargo.lock` or `bun.lock` is missing, and with `build.lockfile-outdated` when building would change it.
- **[DEV-054]** `--offline` MUST fail with `build.offline-missing` when a dependency is not in the local cache.
- **[DEV-055]** `build` MUST exit with 4 when the toolchain is missing, and with 1 for a compile, bundle or component failure.

- **[DEV-066]** `build` of a TypeScript side MUST type-check the project with its `tsconfig.json` before the `bundle` stage and MUST fail with `build.type-error` (exit 1) on any type error, printing one `error` record per error with `file`, `line` and `column`; `bun build` alone, which does not report type errors, MUST NOT be the only check (proposed, owner to confirm).

There is no separate `watch` command. `ilium plugin watch` is not defined. Use `build --watch` to build without installing, and `dev` to build and run.

## check

Runs the install checks without installing. The rules are in [Check and lint rules](/pages/reference-check-and-lint-rules/).

`ilium plugin check [<dir> | <file.iliumpkg>] [--side <side>] [--json]`

`ilium plugin check --print-schema`

The second form takes no directory or archive. It writes the manifest JSON Schema that ships with the host to standard output and exits with 0; it never reads the network ([reference-manifest-schema](/pages/reference-manifest-schema/), PKG-563).

- **[DEV-070]** `check` MUST run offline and MUST report the same rule codes (`manifest.*`) that the install path reports, plus the component and signature rules (`component.*`, `signature.*`).
- **[DEV-071]** `check` on a project directory MUST validate the source manifest and the built components in `dist/`. A missing `dist/` for a declared side MUST be reported as `component.missing`, not as an error of the command.
- **[DEV-072]** `check` on a `.iliumpkg` file MUST validate the archive as the install path does, without checking trust.
- **[DEV-073]** `check` MUST exit with 3 when any finding has severity `error`.
- **[CMP-022]** `check --against <host-version>` and `test --host-contract <host-version>` MUST report `package.api-unsupported` and `manifest.api-version` with the same code and message that install stage 3 reports, when the manifest `requires` range or the built component lies outside the selected host window. `check` MUST report the refusal as an `error` finding and exit 3, and `test` MUST refuse before any case runs and exit 1.

- **[DEV-035]** The project MUST publish, for every host minor inside the current window and every minor still inside its support period, a headless host archive listed in that release's `SHA256SUMS`, and `ilium plugin check --against <host-version>` and `ilium plugin preflight --target <host-version>` MUST fetch and verify it from there or fail with `usage.host-unavailable` naming the version (proposed, owner to confirm).

## lint

Reports advisory problems.

`ilium plugin lint [<dir>] [--deny warnings] [--rule <id>]... [--fix [--dry-run]] [--json]`

- **[DEV-075]** `lint` MUST run offline and MUST report only `lint.*` rules.
- **[DEV-076]** `--deny warnings` MUST turn every `warn` finding into an `error` for the exit code. Without it a `warn` finding returns 0.
- **[DEV-077]** `--rule <id>` MUST restrict the run to the named rules. An unknown id MUST fail with `usage.invalid-value`.
- **[DEV-078]** A suppressed finding MUST be printed with `suppressed: true` and MUST NOT count toward the exit code. Suppression is described in the rules page.
- **[DEV-208]** `lint --fix [--dry-run]` MUST rewrite only the lines matched by rows of the `Removed and renamed` table of a migration note that carry the `mechanical` marking. It MUST exit 0 when every rewrite succeeds, and it MUST exit 1 with `lint.fix-dirty-tree` and change no file when a file it would edit has uncommitted changes. With `--dry-run` it MUST print the same records and write nothing.

Every code that `lint` prints is listed, with its severity, whether `lint --fix` applies it and its owner page, in [Check and lint rules](/pages/reference-check-and-lint-rules/#every-code-and-its-owner-page). That table is the one lint catalogue.

- **[DEV-234]** The lint section of the command reference MUST name one catalogue of every lint code, and the catalogue MUST give each code its severity, whether `lint --fix` can apply it and the page that owns the rule.

- **[DEV-235]** `_tools/check.py` MUST fail when a lint code printed by the CLI is missing from the catalogue.

## test

Runs fixtures against the plugin. The harness behind it is in [Test harness reference](/pages/reference-test-harness/). The fixture and expectation formats are in the same page.

`ilium plugin test [<dir>] [--fixture <glob>]... [--expect <glob>] [--grants <list> | --grant-manifest] [--side <side>] [--component <file>] [--replay <file>] [--update] [--record <file>] [--seed <n>] [--host-contract <version>] [--conformance [--subject <subject>] [--case <id>]] [--coverage [--min-coverage <percent>]]`

| Flag | Type | Default | Meaning |
|---|---|---|---|
| `--fixture` | glob | `tests/fixtures/*.json` | The fixtures to run. Repeatable. |
| `--expect` | glob | `tests/fixtures/<name>.expect.json` | Expectation files. A fixture without one is replayed with no assertion. |
| `--grants` | list | none | Grants for fixtures that have no `grants` key. |
| `--grant-manifest` | switch | on | Takes grants from the manifest. |
| `--side` | side | the only declared side | Needed when the package has two sides. |
| `--component` | file | `dist/<side>.wasm` | Runs the built component in component mode. Without it, `test` uses the component in `dist/`. |
| `--replay` | file | none | Replays a replay file. Conflicts with `--fixture`. |
| `--update` | switch | off | Writes missing or different expectation and golden files. |
| `--record` | file | none | With `--fixture`, writes the replay file of the run and prints an `artifact` of kind `replay` ([DEV-088]). |
| `--seed` | integer | 1 | The seed of the harness random source. |
| `--host-contract` | version | The newest contract that the installed CLI embeds | Runs component mode on the embedded host contract of that version, so a plugin can be tested against an older host. The value `list` prints the embedded versions and exits 0. |
| `--conformance` | switch | off | Runs the [conformance suite](/pages/reference-conformance-suite/) instead of the fixtures. `--subject` and `--case` narrow it. |
| `--coverage` | switch | off | Also runs the project's own tests (`cargo test`, `bun test`) under a coverage tool and writes `.ilium/coverage/`. |
| `--min-coverage` | percent | none | With `--coverage`, exit 3 when line coverage is lower. |

- **[DEV-080]** `test` MUST run in component mode: it loads the built component into the embedded test host, with the real linker, limits and deadlines. A project with no `dist/` component for the side MUST fail with `test.component-missing`.
- **[DEV-081]** `test` MUST print for each fixture one `test-case` record, and for each submission and outcome produced a `submission` and an `outcome` record, in delivery order, before the `test-case` record of that fixture.
- **[DEV-082]** If an expectation file exists, `test` MUST compare it with the result using the match modes of the harness page. On a mismatch it MUST print a `diff` record, set the `test-case` status to `fail`, and exit with 3.
- **[DEV-083]** A fixture with no expectation file MUST pass unless the component traps. It MUST print `warning` code `test.no-expectation`.
- **[DEV-084]** A component trap during a fixture MUST set the status `trap`, print the trap reason in `reason`, and exit with 3.
- **[DEV-085]** A package with a `server` and a `client` side MUST be given `--side`, or `test` MUST fail with `usage.missing-argument`. A project with one side needs no `--side`.
- **[DEV-086]** `--update` MUST rewrite only the expectation and golden files that differ, and MUST print a `warning` with the code `test.updated` for each file rewritten.
- **[DEV-087]** `test` MUST exit with 0 when every case passes, with 3 when a case fails or traps, and with 1 when a fixture cannot be read (`test.fixture-invalid`).
- **[DEV-089]** `test` MUST NOT run the project's own unit tests. Those run with `cargo test` or `bun test` in native mode. `test --coverage` is the one case where the command starts them, and it MUST then print their exit status in the `summary`.
- **[DEV-088]** `--record <file>` MAY be given with `--fixture`. It MUST write the replay file of the run and print an `artifact` of kind `replay`.
- **[DEV-194]** The flag table of `test` MUST list `--host-contract <version>` with its type, default and meaning, and `test` MUST exit 2 with `usage.host-contract-unavailable` before any case runs when the version is not embedded in the installed CLI. The message MUST list the embedded versions.

## pack

Writes the `.iliumpkg` file.

`ilium plugin pack [<dir>] [--out <file>] [--include-src] [--allow-debug]`

- **[DEV-095]** `pack` MUST refuse a missing or stale `dist/`. It reads `dist/build.json` and compares the input hash recorded there with the current sources. A difference MUST fail with `pack.stale-build`. A missing component MUST fail with `pack.not-built`.
- **[DEV-096]** `pack` MUST refuse a debug build with `pack.debug-build`, unless `--allow-debug` is given, in which case it MUST print a `warning` with the same code. A debug build is one whose `build.json` has `profile` `debug`.
- **[DEV-097]** `pack` MUST write the `files` list into the packed manifest, with the length and lowercase SHA-256 of every entry other than `manifest.json` and `signature`.
- **[DEV-098]** `pack` MUST NOT store `src/**` unless `--include-src` is given. With it, `pack` MUST store the sources named by the project's `src/` folder and the source maps of a debug build under `src/`.
- **[DEV-099]** `pack` MUST be reproducible. Two runs on the same inputs MUST produce byte-identical files. The archive format already fixes entry order, timestamps and compression (see [Packages](/pages/packages/)).
- **[DEV-100]** `pack` MUST NOT sign. The default output path is `dist/<id>-<version>.iliumpkg`, and the command prints one `artifact` of kind `package`.
- **[DEV-101]** `pack` MUST fail with `package.size-limit` when the package is larger than 300 MiB (the package format cap, PKG-130), and MUST print a `warning` with the separate code `pack.size-cap` at 100 MiB. See `lint.package-size-near-cap`.
- **[DEV-102]** `pack --out <file>` MUST refuse to overwrite a file that is not a package of the same id, with `pack.out-exists`.
- **[AUT-956]** `pack` MUST refuse with `pack.requires-untested` and exit 1 when any host contract version that the manifest `requires` range admits is missing from `tested_hosts` in `dist/build.json`. The message MUST list every missing version.

## keygen, sign and trust

`ilium plugin keygen --publisher-id <id> --publisher "<name>" --out <dir>`

- **[DEV-110]** `keygen` MUST create `publisher.key` (PKCS#8 PEM, mode 0600) and `publisher.pub` (`ilium-pub/1` JSON) with Ed25519, as defined in [Signing and trust](/pages/reference-signing-and-trust/). It MUST refuse to overwrite either file with `trust.key-exists`.
- **[DEV-111]** The file forms of the secret and public key are defined only in [Signing and trust](/pages/reference-signing-and-trust/) (requirements PKG-210 to PKG-215).
- **[DEV-112]** The fingerprint of a key is its `key_id`, the lowercase hexadecimal SHA-256 of the 32 raw public-key bytes (64 characters). The display form has 16 groups of 4 characters. The short form is the first 8 digits, for example `3f2a91c0`, shown only beside the full form or in `trust list` (SEC-177).

`ilium plugin sign <file.iliumpkg> [--key <file>] [--replace]`

- **[DEV-115]** `sign` MUST read the key from `--key`, or from `ILIUM_PLUGIN_SIGNING_KEY`. With neither, it MUST fail with `usage.missing-argument`.
- **[DEV-116]** `sign` MUST fail with `sign.key-permissions` when the key file is readable by group or others on a system that has such permissions, and with `sign.key-unreadable` when the key cannot be parsed.
- **[DEV-117]** `sign` MUST add the `signature` entry atomically, as an `ilium-sig/1` document over the exact bytes of `manifest.json`, as defined in [Signing and trust](/pages/reference-signing-and-trust/). The package stays deterministic: signing twice with the same key gives the same bytes.
- **[DEV-118]** `sign` MUST fail with `sign.publisher-mismatch` when the `publisher` in the manifest differs from the key's publisher, and with `sign.already-signed` when a signature exists and `--replace` is not given.
- **[DEV-119]** `sign` MUST NOT print the secret key, and the key MUST NOT appear in any record.

`ilium plugin trust add <publisher.pub> | list | remove <key_id> | export <key_id> | revoke <key_id>`

- **[DEV-125]** The trust store is the file `trust.json` (format `ilium-trust/1`) described in [Signing and trust](/pages/reference-signing-and-trust/), at `$XDG_STATE_HOME/ilium/trust.json` (macOS and Windows: the state folder of [SEC-112]), or at the path in `ILIUM_TRUST_STORE` when set.
- **[DEV-126]** `trust add` MUST print the fingerprint. It MUST fail with `trust.key-malformed` for a malformed key file, and with `trust.already-trusted` for a key already listed (exit code 1).
- **[DEV-127]** `trust remove <key_id>` MUST fail with `trust.not-found` (exit code 5) when no such key is listed, and with `trust.in-use` when installed plugins depend on it and `--force` is absent. Removing a key does not uninstall plugins signed with it. The plugins stay installed and show `untrusted` in `list`.
- **[DEV-128]** `trust list` MUST print the publisher, the short fingerprint and the date, one per line, in text mode.

## publish

- **[DEV-135]** `ilium plugin publish <file> --registry <url>` uploads a signed package to a registry. It is defined in [Distribution commands](/pages/reference-plugin-distribution-cli/) (decision D27). Without `--registry` it MUST fail with `usage.missing-argument` and exit 2.
- **[DEV-136]** A package file, the file `publisher.pub` and the SHA-256 of the package remain a complete way to share a plugin without a registry. An update from a file is found when the user installs a package of the same id with a higher version.

## preview

`ilium plugin preview <package-or-dir> [--locale <tag>] [--json]`

`preview` shows an author what users will see, before anything is published or installed. It reads a package file or a project folder that `pack` would accept. It prints two blocks: the store detail view ([Plugin store](/pages/user-plugin-store/#read-before-you-install)) and the install review dialog ([Install and manage plugins](/pages/user-install-and-manage/#review-the-permissions)). It works offline, needs no session, installs nothing, runs no plugin code and writes no file.

The shown fields are the name and the description in the chosen locale ([PKG-706](/pages/reference-manifest/)), the publisher, every requested grant with its risk level, the SPDX licence expression or `No licence declared`, the support contact (`contact`), the README text drawn with the rules of [PRM-202](/pages/user-plugin-store/#the-readme-the-changelog-and-the-screenshots), and one line per screenshot with its alt text and pixel size. Without `--locale` the command uses the unlocalized values. A `--locale` tag that the manifest does not provide falls back as PKG-706 says, and the output names the tag that was used. With `--json` the command prints one `preview` record whose fields are `locale`, `name`, `description`, `publisher`, `grants` (a list of `grant` and `risk`), `license`, `contact`, `readme` and `screenshots` (a list of `alt`, `width` and `height`).

`preview` reports a finding for each shown field that a user would miss:

| Code | Finding |
|---|---|
| `lint.preview-field-missing` | `name`, `description`, `license`, `contact`, `README.md` or the alt text of a screenshot is absent or empty. The `key_path` or `file` of the record names it |
| `lint.preview-truncated` | `README.md` or `CHANGELOG.md` is longer than the 64 KiB that the detail view shows. The `file` of the record names it |

Each finding is one `lint` record with the code as its `rule` and severity `error`. With a finding the command still prints the preview and exits 3, as a lint rule of severity `error` does ([DEV-011]). With none it exits 0. A path that is neither a package nor a project folder fails with `usage.missing-argument` or `plugin.not-found` as for `pack`.

The requirement is PRM-203, at the end of this page.

## install, uninstall, enable, disable

`ilium plugin install <source> [--allow-unsigned] [--allow-downgrade] [--trust <key_id>] [--channel stable|beta|dev] [--yes] [--grant <grant>]... [--locked] [--dry-run] [--registry <url>] [--from-bundle <dir>] [--no-backup] [--unskip]`

- **[DEV-140]** `install` MUST run the validation pipeline of [Package format](/pages/reference-package-format/#the-validation-order) before it runs any plugin code, print every failure as an `error` with the code of that pipeline, and exit with the code of [Distribution commands](/pages/reference-plugin-distribution-cli/#exit-codes). Sources other than a file are defined there.
- **[DEV-141]** `install` MUST show the review dialog in the attached client unless `--yes` is given. With `--yes`, it MUST grant every requested grant whose risk is not high, and MUST grant a high-risk grant only when it is named with `--grant`.
- **[DEV-142]** `install` with no attached client and no `--yes` MUST fail with `install.review-required`.
- **[DEV-143]** A declined review MUST fail with `install.review-declined`. A declined key trust MUST fail with `trust.declined`. An unsigned package without `--allow-unsigned` MUST fail with `install.unsigned`. A lower version without `--allow-downgrade` MUST fail with `package.downgrade`.
- **[DEV-144]** A successful `install` MUST print one `plugin` record for the installed plugin and exit with 0. The plugin is enabled after install.

The `plugin` record of `install` and `update` carries the field `license`, the SPDX expression of the manifest or `null`, and `license_previous` when an update changed it ([PRM-206](/pages/user-install-and-manage/#review-the-permissions)).

`ilium plugin uninstall <id> [--purge | --keep-data]`

- **[DEV-155]** `uninstall` MUST deactivate the plugin with reason `disabled`, withdraw its contributions and remove the package. Without `--purge` the `data` folder, the storage and the settings values MUST stay on disk for a later reinstall. `--keep-data` is accepted, means the same as the default, and MUST NOT ask a question; it excludes `--purge`, and giving both fails with `usage.conflict` and exit 2 ([USR-209], [LIF-082]).
- **[DEV-156]** `uninstall --purge` MUST also delete the `data` and `cache` folders, the storage buckets, the settings values and the plugin log. It MUST print each removed item as a `warning` with code `uninstall.purged`.
- **[DEV-157]** `uninstall` of an unknown id MUST fail with `plugin.not-found` and exit 5.

`ilium plugin reinstall <id>`

`reinstall` repairs the store entry of an installed or registry-listed plugin from the cached package and prints one `plugin` record. It exits 0 on success, 5 for an unknown id, and 1 with `package.cache-missing` when no cached package exists ([LIF-093]).

`ilium plugin enable <id>` and `ilium plugin disable <id>`

- **[DEV-160]** `disable` MUST deactivate the plugin with reason `disabled` and keep its data. `enable` MUST reset the failure count to zero, as the Settings > Plugins toggle does, and activate the plugin.
- **[DEV-161]** Both commands MUST be idempotent: the state they ask for already holding is success with exit code 0.

## list and grants

`ilium plugin list [--json]` prints the installed plugins.

- **[DEV-165]** `list` text mode MUST print one line per plugin with the id, the version, the tier, the state and the grant count. The state word maps from the lifecycle states: `running` is `installed-enabled` with an active instance, `needs-setup` is `installed-enabled` with the health `needs-setup` ([PRM-201](/pages/user-install-and-manage/#a-plugin-that-needs-setup)), `stopped` is `installed-enabled` with no active instance for any other reason (including `stopped-by-failure`), `disabled` is `installed-disabled-by-user` or `disabled-by-policy`, and `untrusted` is `disabled-untrusted` or `disabled-blocked`. `list --json` MUST print the lifecycle state name itself (LIF-001). A `[dev]` badge MUST mark a plugin installed by `dev`.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin list
example.agent-tools 0.1.0 T3 [dev] running
                    grants 2 of 2
ilium.badges        0.1.0 T1 running
                    grants 3 of 3
ilium.tasks         0.1.0 T1 disabled
                    grants 0 of 1
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The default text form of list, wrapped for the width. The dev badge marks a development install.</figcaption>
</figure>

`ilium plugin grants <id> [--grant <grant>]... [--revoke <grant>]... [--json]`

- **[DEV-170]** `grants` with no change flag MUST list every requested grant with its risk and state (`granted`, `declined`, `revoked`).
- **[DEV-171]** `--revoke` MUST withdraw the grant, withdraw the contributions that need it and restart the plugin with the smaller set. The revoke MUST work for T1 plugins.
- **[DEV-172]** `--grant` MUST grant a requested grant that is currently declined or revoked, and MUST fail with `grants.not-requested` for a grant the manifest does not request. A high-risk grant MUST ask for confirmation in the attached client, and without one MUST fail with `install.review-required`.

## dev

Builds the plugin, installs it as a development copy, and rebuilds and reloads it after each change.

`ilium plugin dev [<dir>] [--side <side>] [--timings] [--release] [--grant-all] [--replace-installed] [--purge-on-exit]`

- **[DEV-180]** `dev` MUST talk to the session found by the rule [DEV-004] and MUST NOT start one. With no session it MUST exit with 6.
- **[DEV-181]** `dev` MUST install the plugin as tier T3 with the `dev` badge, with a data folder of its own that is kept apart from any real install of the same id. A later real install of the same id MUST start with its own data folder.
- **[DEV-182]** `dev` MUST build with debug information by default, so traps show `file:line`. `--release` builds without it.
- **[DEV-183]** If a non-dev copy of the same id is installed, `dev` MUST fail with `dev.id-installed`. With `--replace-installed` it MUST disable the installed copy for the time `dev` runs and enable it again on exit.
- **[DEV-184]** The first install by `dev` MUST show the review dialog. A rebuilt manifest that adds grants MUST show the dialog again for the new grants only. `--grant-all` MUST grant every non-high-risk grant without a dialog. A high-risk grant MUST always ask.
- **[DEV-185]** `dev` MUST watch the project directory and MUST ignore `dist/`, `target/`, `node_modules/`, `.ilium/`, `.git/` and the paths in `.gitignore`. It MUST start a rebuild 300 ms after the last change.
- **[DEV-186]** A change that arrives during a build MUST be queued. When the build ends, `dev` MUST run exactly one more build for all queued changes.
- **[DEV-187]** On a successful rebuild, `dev` MUST reload each rebuilt side in turn. The server side reloads first, then every attached client side. Each reload deactivates the old instance with the reason `upgrade`, activates the new one with the restore bytes and prints one `reload` record. A failed rebuild MUST keep the running instance, print the compiler errors, and send no `deactivate`.
- **[DEV-188]** When restore bytes exceed 1 MiB they are dropped and the `reload` record MUST have `restore_dropped` true and `restored_bytes` 0.
- **[DEV-189]** On Ctrl+C `dev` MUST deactivate the plugin with the reason `disabled`, uninstall the development copy, keep its data folder unless `--purge-on-exit` is given, restore any plugin that `--replace-installed` disabled, print `summary` and `result`, and exit with 130.
- **[DEV-196]** With `--timings`, `dev` MUST print a `timing` record for each export every 10 seconds while calls happen. See [Debugging tools](/pages/reference-debugging-tools/).

## logs and log-level

`ilium plugin logs <id> [--tail <n>] [--follow] [--level <level>] [--side <side>] [--since <time>] [--json]`

| Flag | Type | Default | Meaning |
|---|---|---|---|
| `--tail` | integer | all | The last `n` entries. |
| `--follow` | switch | off | Keeps the command open and prints new entries. |
| `--level` | level | `trace` | Shows entries at this level or higher. |
| `--side` | side | all | Shows one side. `host` shows only entries the host wrote. |
| `--since` | time | none | A time `YYYY-MM-DD HH:mm:ss`, or a duration such as `10m` or `2h`. |
| `--json` | switch | off | Prints `log` records. |

- **[DEV-190]** `logs` MUST print oldest entry first. With `--follow` it MUST print the entries in the file, then new ones as they arrive, until interrupted, and exit with 130.
- **[DEV-191]** `logs` of an id that is not installed and has no kept log MUST fail with `plugin.not-found` and exit 5. The text format is fixed in [Debugging tools](/pages/reference-debugging-tools/), the single place for the log format.
- **[DEV-192]** `ilium plugin log` (singular) is not a command. A page that shows it is wrong.

`ilium plugin log-level <id> [<level> | --reset]`

- **[DEV-195]** `log-level` with no argument MUST print the current threshold. With a level it MUST set the threshold for that plugin and all its sides until the session stops. `--reset` MUST return to the session default.

## status

`ilium plugin status [<id>] --json` prints one line per plugin with its health, the reason for it and the numbers behind it. Without `<id>` it lists every plugin; with `<id>` it prints that plugin only. Without `--json` it prints a table with the columns ID, HEALTH, P99 and NEAR. The record fields, health states and the meaning of each reason are defined in [Health and watchdog](/pages/reference-health-and-watchdog/).

## trace, profile, inspect and doctor

These commands print text by default and JSONL with `--json`. Their output content is defined in [Debugging tools](/pages/reference-debugging-tools/).

`ilium plugin trace <id> [--last <n>] [--follow] [--kinds <list>] [--side <side>] [--record <file>] [--unredacted] [--json]`

- **[DEV-820]** `trace` MUST need a session and MUST fail with `plugin.not-found` for an unknown id.
- **[DEV-821]** `--last` MUST default to 50 records. `--kinds` MUST accept a comma list of the kinds in the debugging tools page.
- **[DEV-822]** `--record <file>` MUST write a replay file of the recorded events and MUST print an `artifact` record of kind `replay`.
- **[DEV-823]** `--unredacted` MUST be refused with `trace.unredacted-denied` for a plugin that is not a `dev` install.

`ilium plugin profile <id> [--duration <seconds>] [--side <side>] [--json]`

- **[DEV-830]** `profile` MUST default to `--duration 10`, MUST accept 1 to 600, and MUST print one `timing` record per export that ran, then a summary.

- **[DEV-048]** Each record of `ilium plugin profile <id> --duration <seconds>` MUST be a `timing` record with `export`, `started_at`, `queue_us`, `run_us`, `imports`, `outcome` and `synthetic: false`, using the same field meanings as the spans of HST-609, and the command MUST end with one `summary` record giving the call count and the p99 of `run_us` per export (proposed, owner to confirm).

- **[DEV-047]** Each `timing` record of `ilium plugin bench` MUST carry `peak_memory_bytes`, the highest linear memory plus table size reached during the measured iterations of that export, and bench MUST exit 3 when any export's `peak_memory_bytes` exceeds the effective `memory_mib` of the manifest (proposed, owner to confirm).

`ilium plugin inspect (<file.iliumpkg> | <dist-component.wasm> | <id>) [--verify] [--view <target>] [--memory] [--json]`

- **[DEV-800]** `inspect <file>` MUST work offline. `inspect <id>` needs a session.
- **[DEV-801]** `--verify` MUST recompute every hash of the package, compare it with the `files` list and print a `diff` record per difference. A package with a difference MUST exit with 1 and the code `inspect.hash-mismatch`.
- **[DEV-802]** `--view <target>` and `--memory` need a session and the content of the sections is in the debugging tools page.
- **[DEV-803]** `inspect` of an unreadable file MUST fail with `inspect.file-unreadable`, and of a file that is neither a package nor a component with `inspect.not-a-package`.

`ilium plugin support-bundle <id> --out <file>`

- **[DEV-831]** `support-bundle` MUST write the archive that [USR-221] defines to `<file>` and MUST print one `support-bundle` record with the path and the size. It needs a session.
- **[DEV-832]** A `<file>` that already exists MUST NOT be overwritten. The command MUST fail with `support-bundle.exists` and exit with 2.
- **[DEV-833]** An archive over 8 MiB MUST be trimmed as [USR-224] says, and the `support-bundle` record MUST say so.

- **[DEV-046]** The `support-bundle` record of [DEV-831] MUST also carry `sha256` (the hash of the whole zip file), `scope` and `dropped`, and the command MUST print the hash on a line of its own after the record, for the per-plugin bundle exactly as for `--all` (proposed, owner to confirm).

`ilium plugin doctor [--json]`

- **[DEV-810]** `doctor` MUST work offline and MUST print one `doctor-check` record per item in the table below, in this order.
- **[DEV-811]** `doctor` MUST exit with 4 when any item is `fail`, and with 0 otherwise. A `warn` never changes the exit code.

| Item | ok when | Status when not | Fix text |
|---|---|---|---|
| `rustc` | rustc 1.90 or later | `fail` if rustc is missing or older, `warn` if no Rust project is in the current directory | `rustup update stable` |
| `rust-target` | `wasm32-wasip2` installed | `fail` | `rustup target add wasm32-wasip2` |
| `bun` | Bun 1.2 or later | `fail` if missing or older, `warn` outside a TypeScript project | Install or upgrade Bun |
| `sdk-pin` | the project SDK matches the pin in the host | `fail` on a major or minor mismatch, `warn` on a patch mismatch | Update the SDK version |
| `wit-pins` | `wit-bindgen` and `wit-component` match the host | `fail` | Update the SDK |
| `host-version` | the `ilium` binary supports `ilium:plugin@0.1.0` | `fail` | Update Ilium |
| `platform-tiers` | the platform allows T2 and T3 | `warn` where T3 is disabled by policy | Ask the administrator |
| `trust-store` | the store file is readable and valid | `fail` if corrupt, `ok` if absent | Restore or delete the file |
| `session` | a session answers | `warn` | Start `ilium` |
| `disk` | at least 512 MiB free in the project folder | `fail` below 128 MiB, `warn` below 512 MiB | Free space |

## The build pipeline

The build has fixed stages. Each stage has an input, an output and an error code.

| Stage | `step` | Rust | TypeScript | Output | Error |
|---|---|---|---|---|---|
| 1 | `toolchain` | Check rustc, target | Check Bun, SDK | none | `toolchain.*` |
| 2 | `compile` | `cargo build --target wasm32-wasip2` | none | a core module or component under `target/` | `build.compile-failed` |
| 3 | `bundle` | none | `bun build` to one ES module, `ilium:*` and `wasi:*` external | `.ilium/cache/<side>.js` | `build.bundle-failed` |
| 4 | `component` | wrap and validate | `componentize-js` | `.ilium/cache/<side>.component.wasm` | `build.component-failed` |
| 5 | `api-version` | embed | embed | custom section `ilium:api-version` | `build.world-mismatch` |
| 6 | `strip` | remove DWARF on release | remove names of non-exports and the map on release | `dist/<side>.wasm` | none |

- **[DEV-300]** `build` MUST run the stages in this order and MUST print a `progress` record at the start of each stage.
- **[DEV-301]** `dist/server.wasm`, `dist/client.wasm` and `dist/scene.wasm` MUST be the only component names, one per declared side, matching the `entry` of the manifest.
- **[DEV-302]** Stage 5 MUST write the custom section `ilium:api-version` as three big-endian `u16` values: major, minor, patch. The build MUST fail with `build.world-mismatch` when the component's world differs from the world in the manifest.
- **[DEV-303]** Under `--release` the component MUST carry no DWARF information and, for TypeScript, no source map. It MUST keep the function names of the exports, so traps read `at handle_batch`. Under `--debug` it MUST keep DWARF (Rust) or write a source map to `dist/<side>.wasm.map` (TypeScript).
- **[DEV-304]** `build` MUST write `dist/build.json` with the keys `profile`, `api`, `input_hash`, `sides` (name, bytes, sha256), `toolchain` (versions of rustc or Bun and the SDK), and `cached`.
- **[DEV-305]** The `input_hash` MUST be the SHA-256 over the sorted list of project source files, the lockfile, the manifest and the toolchain versions. An unchanged `input_hash` MUST reuse the cached component and set `cached` true.
- **[DEV-306]** With a pinned toolchain and a locked dependency set, two builds of the same sources MUST produce byte-identical components. Without `--locked` the guarantee does not hold.
- **[DEV-307]** The build cache MUST live in `<dir>/.ilium/cache/`. `target/` and `node_modules/` keep their usual places. A project `.gitignore` MUST list `dist/`, `.ilium/`, `target/` and `node_modules/`.
- **[DEV-207]** `build` MUST write `dist/build.json` with the key `tested_hosts`, a list of the host contract versions for which a `test --conformance` run passed on the component with the same `component_sha256` as this build. Each passing `test --conformance` run MUST be recorded in `.ilium/conformance.json` with its contract version and component hash. `tested_hosts` MUST be an empty list when no such record exists.

## Error catalogue

These codes are in addition to the `manifest.*` codes of [Packages](/pages/packages/) and the rule codes of [Check and lint rules](/pages/reference-check-and-lint-rules/). The exit column is the exit code of the command when this code stops it.

| Code | Cause | Fix | Exit |
|---|---|---|---|
| `usage.unknown-flag` | A flag does not exist | Check `--help` | 2 |
| `usage.missing-argument` | A required flag or argument is missing | Add it | 2 |
| `usage.conflict` | Two flags contradict each other. This is the only code for a refused flag combination, including those of the [install flag matrix](/pages/install-flag-matrix/) | Remove one | 2 |
| `usage.invalid-value` | A flag value has the wrong form or an unknown rule id | Fix the value | 2 |
| `usage.unknown-command` | The subcommand is not in the [Command inventory](#command-inventory) ([DEV-093]) | Check `ilium plugin --help` | 2 |
| `usage.unknown-kind` | `schema <kind>` names a kind that does not exist ([PKG-005]) | Pick a kind from the printed list | 2 |
| `usage.file-not-found` | An argument that is a file path names nothing, or names a directory ([Distribution commands](/pages/reference-plugin-distribution-cli/)) | Fix the path | 2 |
| `usage.out-of-range` | A numeric flag value is outside its documented range, such as `--timeout-ms` above 600,000 | Use a value inside the range | 2 |
| `usage.side-required` | `debug` ran without `--side` on a plugin that declares several sides ([Plugin debugging](/pages/reference-plugin-debugging/)) | Add `--side <side>` | 2 |
| `usage.host-unavailable` | `check --against` or `preflight --target` names a host version whose archive cannot be fetched and verified | Use a supported host version, or run it where the archive is reachable | 2 |
| `usage.rules-version-invalid` | `--rules-version` is not `MAJOR.MINOR` ([Check and lint rules](/pages/reference-check-and-lint-rules/)) | Write two integers separated by a dot | 2 |
| `usage.rules-version-unavailable` | `--rules-version` names a version this CLI does not embed | Pick a version the message lists | 2 |
| `usage.suite-unavailable` | `--suite-version` names a conformance catalogue this CLI does not embed ([Conformance suite](/pages/reference-conformance-suite/)) | Pick an embedded version | 2 |
| `usage.subject-unavailable` | `test --conformance` names the subject `sdk` or `host` in a normal install ([DEV-601]) | Use the subject `plugin` | 2 |
| `usage.release-build-required` | `--conformance` ran on a debug build without `--allow-debug` | Build with `--release` | 2 |
| `session.unreachable` | No session answers | Start `ilium` in the project, or pass `--session` | 6 |
| `session.not-found` | The named session does not exist | Use a name from `ilium sessions` | 6 |
| `session.version-mismatch` | The session runs an older contract | Restart the session | 1 |
| `plugin.not-found` | The id is not installed and no log is kept | Check `list` | 5 |
| `toolchain.rustc-missing` | `rustc` is not on the path | Install Rust | 4 |
| `toolchain.rust-version` | `rustc` is older than 1.90 | `rustup update stable` | 4 |
| `toolchain.rust-target` | `wasm32-wasip2` is missing | `rustup target add wasm32-wasip2` | 4 |
| `toolchain.bun-missing` | Bun is not on the path | Install Bun | 4 |
| `toolchain.bun-version` | Bun is older than 1.2 | Upgrade Bun | 4 |
| `toolchain.sdk-missing` | The TypeScript SDK is not installed | `bun install` | 4 |
| `toolchain.sdk-mismatch` | The SDK is not supported by the host | Update the SDK pin | 4 |
| `toolchain.language-ambiguous` | Both `Cargo.toml` and `package.json` exist | Keep one | 4 |
| `toolchain.language-unknown` | Neither file exists | Run in the project folder | 4 |
| `new.dir-not-empty` | The folder holds files | Use `--force` or another folder | 1 |
| `new.id-invalid` | The id fails the pattern | Fix the id | 1 |
| `new.id-reserved` | An `ilium.` id without `--first-party` | Use another id | 1 |
| `new.template-unknown` | The template does not exist | Use a listed template | 2 |
| `build.manifest-missing` | No `manifest.json` | Run in the project folder | 1 |
| `build.manifest-invalid` | The manifest does not parse | Fix the JSON | 1 |
| `build.side-undeclared` | `--side` names a side the manifest lacks | Declare it | 1 |
| `build.entry-missing` | The source entry of a side is missing | Create the entry | 1 |
| `build.compile-failed` | The compiler failed | Read the compiler text | 1 |
| `build.bundle-failed` | The bundler failed | Read the bundler text | 1 |
| `build.component-failed` | The component step failed | Check the world | 1 |
| `build.world-mismatch` | The component world differs from the manifest | Fix the world | 1 |
| `build.lockfile-missing` | `--locked` and no lockfile | Create the lockfile | 1 |
| `build.lockfile-outdated` | `--locked` and the lockfile would change | Update and commit it | 1 |
| `build.offline-missing` | `--offline` and a dependency is not cached | Build once online | 1 |
| `pack.not-built` | A component is missing from `dist/` | Run `build` | 1 |
| `pack.stale-build` | `dist/` does not match the sources | Run `build` | 1 |
| `pack.debug-build` | The build is a debug build | Build with `--release` | 1 |
| `package.size-limit` | The package is over 300 MiB | Remove assets | 1 |
| `pack.size-cap` (warning) | The package is over 100 MiB | Remove assets | 0 |
| `pack.out-exists` | `--out` names a different file | Choose another path | 1 |
| `pack.requires-untested` | A host contract version admitted by `requires` is missing from `tested_hosts` | Run `test --conformance` on each listed version | 1 |
| `lint.fix-dirty-tree` | `lint --fix` would edit a file with uncommitted changes | Commit or stash the file, then run again | 1 |
| `lint.preview-field-missing` | `preview` found a shown field that is absent or empty | Add the field that the record names | 3 |
| `lint.preview-truncated` | `preview` found a README or CHANGELOG above 64 KiB | Shorten the file, or move the detail to the repository | 3 |
| `usage.host-contract-unavailable` | `--host-contract` names a version the CLI does not embed | Run `test --host-contract list` and pick a listed version | 2 |
| `sign.key-unreadable` | The key file does not parse | Check the file | 1 |
| `sign.key-permissions` | The key is readable by others | `chmod 600` | 1 |
| `sign.publisher-mismatch` | Manifest and key publishers differ | Fix one | 1 |
| `sign.already-signed` | A signature exists | Use `--replace` | 1 |
| `trust.key-malformed` | The public key file is malformed | Regenerate it | 1 |
| `trust.already-trusted` | The key is listed | none | 1 |
| `trust.not-found` | The publisher is not listed | Check `trust list` | 5 |
| `test.fixture-invalid` | A fixture does not match the schema | Fix the fixture | 1 |
| `test.component-missing` | There is no built component | Run `build` | 1 |
| `test.no-expectation` | A fixture has no expectation (warning) | Add one | 0 |
| `test.updated` | A file was rewritten by `--update` (warning) | Review the change | 0 |
| `test.mismatch` | A result differs from the expectation | Fix the plugin or update | 3 |
| `test.trap` | The component trapped | Read the reason | 3 |
| `dev.id-installed` | A real copy of the id is installed | `--replace-installed` | 1 |
| `dev.build-failed` | A rebuild failed (record only) | Fix the error | none |
| `install.review-required` | A review is needed and no client is attached | Attach or use `--yes` | 1 |
| `install.review-declined` | The user declined | none | 1 |
| `trust.declined` | The user declined the key | none | 1 |
| `install.unsigned` | An unsigned package without `--allow-unsigned` | Sign it | 1 |
| `package.downgrade` | A lower version without `--allow-downgrade` | Use the flag | 1 |
| `grants.not-requested` | The grant is not in the manifest | Request it | 1 |
| `uninstall.purged` | An item was removed (warning) | none | 0 |
| `trace.unredacted-denied` | `--unredacted` on a non-dev plugin | Use a dev install | 1 |
| `inspect.file-unreadable` | The file cannot be read | Check the path | 1 |
| `inspect.not-a-package` | The file is not a package or component | Check the file | 1 |
| `inspect.hash-mismatch` | A hash differs from the list | Repack | 1 |
| `inspect.id-unknown` | The installed id is unknown | Check `list` | 5 |

[Errors and limits](/pages/reference-errors-and-limits/) lists the runtime codes. The codes on this page are command codes, printed by the command line and never returned by a host import.

- **[DEV-107]** Every `error` record whose code has a catalogue entry ([USR-347]) MUST carry a non-empty `hint` equal to the next-step sentence of that entry, and in text mode the command MUST print `error: <message>` on one line and `fix: <hint>` on the next line (proposed, owner to confirm).

## What the user sees

A person who runs `ilium plugin doctor` on a machine without the Rust target sees one line per item:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin doctor
<span class="t-ok">ok</span>   rustc        1.90.0
<span class="t-err">fail</span> rust-target  wasm32-wasip2 missing
     fix: rustup target add wasm32-wasip2
<span class="t-ok">ok</span>   bun          1.2.4
<span class="t-ok">ok</span>   sdk-pin      0.1.0
<span class="t-warn">warn</span> session      no session found
<span class="t-ok">ok</span>   disk         41 GiB free
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The doctor text form. The exit code is 4 because one item failed.</figcaption>
</figure>

## Edge cases

- **Two commands at once.** Two `build` runs in one project would write the same files. The second MUST fail with `build.compile-failed` and the message `another build holds .ilium/lock`. The lock file is `.ilium/lock`.
- **Interrupt during pack.** The package is written to a temporary file and renamed, so an interrupt leaves no partial package.
- **Read-only folder.** `new` and `build` fail with exit code 1 and a `build.compile-failed` or file error that names the path.
- **Unicode paths.** Paths are UTF-8 and compared byte for byte. Case-insensitive file systems are not special-cased.
- **No terminal.** When standard output is not a terminal, text commands print no colour and no cursor movement.

## See also

- [Distribution commands](/pages/reference-plugin-distribution-cli/)
- [CLI output stability](/pages/reference-cli-output-stability/)

- [Set up a toolchain](/pages/getting-started-toolchain/)
- [Packages and manifests](/pages/packages/)
- [Check and lint rules](/pages/reference-check-and-lint-rules/)
- [Project templates](/pages/reference-project-templates/)
- [Test harness reference](/pages/reference-test-harness/)
- [Debugging tools](/pages/reference-debugging-tools/)
- [CI recipes](/pages/guide-ci-recipes/)
- [Package and share](/pages/tutorial-09-package-and-share/)

API checked against `ilium:plugin@0.1.0`.

## Production additions: T5-devtools

- **[DEV-900]** `ilium plugin clean [<dir>] [--all]` MUST delete `dist/` and the plugin build cache of the project and print one `artifact` record per removed path; with `--all` it MUST also delete `target/` or `node_modules/.cache`. It MUST NOT delete `src/`, `manifest.json`, lock files, `publisher.key` or golden files, MUST be idempotent (exit 0 when nothing exists) and MUST work offline.

- **[DEV-901]** Two commands that write `dist/` in the same project (`build`, `pack`, `sign`, `test --update`, `dev`) MUST serialize through an advisory lock file `dist/.lock`; a second command MUST wait up to 30,000 ms (proposed, owner to confirm) then fail with `build.locked` (exit 1) naming the holder PID. A lock whose PID is dead MUST be broken automatically.

- **[DEV-902]** Every project command MUST accept `--manifest-path <file>` naming a `manifest.json`; the project directory is then that file's parent and `<dir>` MUST NOT also be given (else exit 2 `usage.conflict`). Relative paths in output records MUST be relative to the project directory, never the caller's working directory.

- **[DEV-903]** A project MAY contain `ilium-plugin.toml` in its root. The CLI MUST read only the keys `[build] profile`, `[build] extra-args`, `[test] timeout_ms`, `[lint] deny`, `[dev] ignore` and MUST fail with `config.unknown-key` (exit 2) on any other key. A command-line flag MUST override the file, and the file MUST override defaults but never the manifest.

- **[DEV-904]** The CLI reference MUST list every environment variable the CLI reads (at least `ILIUM_SESSION`, `ILIUM_PLUGIN_SIGNING_KEY`, `NO_COLOR`, `ILIUM_PLUGIN_OFFLINE`) with type, default and the flag that overrides it. The CLI MUST ignore any `ILIUM_PLUGIN_*` variable not in that list and `doctor` MUST emit a `warn` doctor-check named `env.unknown` for each such variable set.

- **[DEV-906]** `ilium plugin --version` MUST print one `version` record (JSONL) or one text line with the CLI version, the plugin contract version, the supported WIT package versions and the embedded test host version, and exit 0 without a session or toolchain.

- **[DEV-908]** `lint --fix` MUST apply only rules whose record carries a non-empty `fix` field, MUST rewrite files atomically (write temp file then rename), MUST print one `artifact` record per modified file, and MUST refuse with `lint.dirty-tree` (exit 1) when the project is in a Git repository with uncommitted changes in a file it would modify, unless `--allow-dirty` is given. A second run MUST change zero files.

- **[DEV-911]** `check` MUST emit lint/check rule `size.component-over-budget` with severity `warn` when a built component exceeds 2 MiB (proposed, owner to confirm) and `error` when it exceeds the package component limit of the package format; the budget MAY be lowered by `[build] max_component_bytes` in the project config and MUST NOT be raised above the format limit.

- **[DEV-912]** `ilium plugin upgrade-sdk [<dir>] [--to <version>] [--dry-run]` MUST update the SDK dependency and the pinned WIT version in the project, MUST print each edit as a `diff` record, MUST be a no-op with exit 0 when already current, and MUST fail with `upgrade.no-path` (exit 1) when the target version is older than the current one or crosses a major SDK version without a published migration note.

- **[DEV-913]** `ilium plugin check --deps` MUST report, offline from the lock file, the SDK version in use, the newest SDK version known to the installed CLI, and a `warn` record `deps.sdk-outdated` when the project's SDK minor is more than 2 minors (proposed, owner to confirm) behind the CLI.

- **[DEV-914]** Every offline command MUST behave identically on Linux, macOS and Windows: paths in records MUST use forward slashes, line endings of generated files MUST be LF, key files MUST be created with owner-only permissions (mode 0600, or an ACL granting only the current user on Windows), and the file watcher MUST fall back to polling every 500 ms (proposed, owner to confirm) when native events are unavailable.

- **[DEV-915]** The CLI MUST write standard output and files as UTF-8 regardless of locale, MUST NOT localise `code` or `rule` fields or JSONL keys, and MUST print text-mode messages in English only. Invalid UTF-8 in a source manifest MUST fail with `manifest.json-invalid` and the byte offset.

- **[DEV-916]** An interrupt (SIGINT, SIGTERM or Windows Ctrl+C) during `pack` or `sign` MUST leave no partial output: the target file MUST either be unchanged or fully written, no temp file named `*.tmp` may remain, and the command MUST exit 130 after printing `summary` and `result`.

- **[DEV-920]** `test` MUST accept `--timeout-ms <n>` (default 10,000 per case, max 600,000 (proposed, owner to confirm); larger MUST exit 2 `usage.out-of-range`) and `--filter <glob>` matching fixture names. A case over its timeout MUST get status `fail` with reason `timeout`, and a filter matching zero fixtures MUST exit 2 `test.no-match`.

- **[DEV-922]** `ilium plugin bench [--fixture <name>] [--iterations <n>]` MUST call the built component in the embedded test host with a real clock, default 100 iterations (proposed, owner to confirm) after 10 warm-up iterations, and print one `timing` record per export with `p50_ms`, `p99_ms` and `max_ms`; it MUST exit 3 when any export p99 exceeds its manifest `deadline_ms` and MUST label the record `synthetic: true`.

- **[DEV-931]** `dev --reset-data` MUST delete the development copy's data folder, cache folder and storage buckets before the first install and print one `warning` record naming them. Without it, data from the previous `dev` run MUST be kept across runs and across rebuilds.

- **[DEV-939]** `ilium plugin diff <old.iliumpkg> <new.iliumpkg>` MUST work offline and print `diff` records for: added, removed and changed grants, contributions, file list, version, and declared limits; it MUST exit 0 when identical and 3 when they differ, and it MUST NOT extract files to disk.

- **[DEV-940]** `pack --sbom` MUST write `dist/<id>-<version>.sbom.json` in CycloneDX 1.5 JSON listing every locked dependency with name, version and SHA-256 or registry checksum from the lock file, MUST be reproducible (DEV-099), and MUST print an `artifact` record of kind `sbom`.

- **[DEV-941]** `ilium plugin doctor --bundle <file>` MUST write a zip with exactly `doctor.txt` and `host.txt`, the doctor subset of the whole-install support bundle ([USR-104](/pages/core-plugin-diagnostics/#the-three-bundle-commands)), and MUST work without a session. `doctor.txt` MUST hold the doctor records, the CLI and host versions and the lock file hash. Both entries MUST redact values of environment variables whose names contain `KEY`, `TOKEN` or `SECRET`, and the archive MUST NOT include a manifest, plugin log lines, source files, key files or the data folder.

## A session that connects but does not answer

[DEV-006] covers a session that cannot be reached. A hung server is different: it accepts the connection and never replies, which is the input-lock symptom in [Troubleshooting](/pages/guide-troubleshooting/). Without a rule, every diagnostic command would block at the moment the operator needs it most.

- **[DEV-109]** (unresponsive session) A session command whose connection succeeds, but whose request receives no reply within the response timeout, MUST print one `error` record with the code `session.unresponsive`, then `summary`, then `result`, and exit with 6. The record MUST name the socket path and the server process ID, and MUST suggest `ilium plugin doctor` and `ilium plugin support-bundle --all`. The command MUST NOT retry and MUST NOT send the request a second time. The response timeout is 10 s by default (proposed, owner to confirm) and `--timeout <seconds>` (proposed) overrides it; a value of 0 or above 600 MUST exit 2 `usage.out-of-range`.
- **[DEV-113]** (offline fallback) On `session.unresponsive`, a command listed as "falls back" in the table below MUST continue by reading on-disk state, print a `warning` record with the code `session.unresponsive-fallback`, mark every record it derived from disk with `source: "disk"`, and exit with the code its result would have. A command listed as "refuses" MUST stop at the error and exit 6.

| Command | When the session is unresponsive | Source on disk |
|---|---|---|
| `logs` | falls back | The plugin log files. Lines written since the server hung are absent. |
| `doctor` | falls back | Files, trust store and toolchain. The session check reports `fail` with `session.unresponsive`. |
| `recovery` | falls back | The recovery files of the data folder ([HST-617], [HST-618]). |
| `list` | falls back | `installed.json`. Each row carries `stale: true`, because the loaded state is unknown. |
| `kill <id>` | falls back | Writes the entry to the kill state file `<data>/plugins/kill.json` and sends `cgroup.kill` to the helper cgroup leaf; the server applies the entry when it answers again or at its next start ([HST-516], [PDZ-007]). |
| `unkill <id>` | refuses | Only a running server can check the policy kill and the block that may still hold the plugin ([SCU-003](/pages/security-containment-states/)). |
| `log-level`, `trace`, `profile`, `dev` | refuses | They need the live instance ([DEV-014]). |
| `install`, `uninstall`, `enable`, `disable`, `grants` | refuses | They change the loaded set. |

- **[DEV-114]** A command that refuses MUST NOT edit `installed.json` or any other state file as a substitute for the live call.

The user-facing steps for a hung session are in [Safe mode and troubleshooting](/pages/user-safe-mode-and-troubleshooting/) ([USR-336]).

## Language of command-line messages

Decision: the command line prints human-readable text in English only, and `--json` is the stable form for anything a program reads. The command line does not follow `[ui] language` or `ILIUM_LANG`.

- **[DEV-164]** (decision) The CLI MUST print text-mode messages in English whatever the interface language is. It MUST NOT read `[ui] language` or `ILIUM_LANG` to choose message text. Codes, rule names, keys and JSONL field names are never translated, so a script or a search for an error code works the same in every language.
- **[AUT-623]** Plugin authors MUST NOT depend on the wording of a message. They match on `code` or `rule`.
- **[USR-077]** Pages for users, `ilium plugin explain` and the store MUST state that command-line messages are in English only, and that the Settings screens and plugin dialogs follow the interface language. [USR-347] covers the store listing.
- **[DEV-166]** (test) A CI test MUST assert that every error code in the [Error catalogue](#error-catalogue) is the same under each launch language, and that every human-readable CLI message either has an entry in the English message catalogue or is listed as English-only. A message in neither place MUST fail the build.

## dev: non-code changes, dependents and several plugins

[DEV-187] and [DEV-879] define the reload of a rebuilt side. This section states the rest.

- **[DEV-121]** A change to `manifest.json`, to a file under `help/`, to `settings.schema` or to any other non-source file in the project MUST reinstall the development copy without a component rebuild. The reinstall MUST validate the manifest as `check` does; on failure it MUST keep the running instance and print the diagnostics, as in [DEV-879].
- **[DEV-120]** A manifest change that adds a grant MUST show the review dialog for the new grants only. Until the user answers, the running instance MUST stay as it is. A change that removes a grant takes effect at the reload.
- **[DEV-122]** The reload for a non-source change MUST print one `reload` record with `reason: "manifest"` and, for a change under `help/` or to `settings.schema`, `reason: "resources"`. A settings schema change MUST keep stored values that still validate and report the rest as `settings.value-dropped` warnings. The `restore_dropped` and `restored_bytes` fields of [DEV-188] apply unchanged.
- **[DEV-158]** When the reloaded plugin provides a service or contributes to an extension point:
  - A service call in flight at the moment of deactivation MUST complete as `failed(unavailable)`, or wait for the new instance up to the call deadline, and never run on both instances.
  - Contributions of the old instance MUST be withdrawn when it deactivates and registered again when the new instance activates. Consumers MUST NOT see a half-registered set.
  - Subscribers of the plugin MUST get one `plugin-changed` event after the new instance is active.
- **[DEV-124]** A plugin that depends on the reloaded one MUST NOT be reloaded itself. It stays active and sees the sequence above.
- **[DEV-159]** Two `dev` commands MAY run at once in the same session, for example a provider and its consumer in two projects. Each reloads only its own plugin. Each MUST use its own file watcher and its own lock ([DEV-901]), and the reloads MUST NOT be coalesced. The second `dev` for the same id MUST fail with `dev.id-installed`.
- **[DEV-123]** (scene side) When the reloaded side is a scene, the running ambient animation MUST restart from the new instance's initial state, using the saved restore bytes when the scene declares support for them, and MUST keep the shared look settings. The animation MUST show the last frame during the reload and MUST NOT blank the background.

## Published schemas and editor support

Authored files are validated at run time with path-based errors ([DEV-511], [DEV-231], [DEV-903]). Each one also has a published JSON Schema, so an editor can complete and check it.

| File | Command | Notes |
|---|---|---|
| `manifest.json` | `ilium plugin schema manifest` | Exists. See [Manifest schema](/pages/reference-manifest-schema/). |
| Fixture | `ilium plugin schema fixture` | Test input files ([DEV-511]). |
| Expectation | `ilium plugin schema expectation` | Golden and expected-output files. |
| Replay | `ilium plugin schema replay` | Recorded sessions for `test`. |
| Test report | `ilium plugin schema test-report` | The `--report` output of `test`. |
| Conformance report | `ilium plugin schema conformance-report` | Output of the conformance run. |
| `lint.toml` | `ilium plugin schema lint` | Includes `[[suppress]]` ([DEV-231]). |
| `ilium-plugin.toml` | `ilium plugin schema project` | The keys of [DEV-903]. |
| `ilium-toolchain.json` | `ilium plugin schema toolchain` | Toolchain pins. |
| Settings schema | `ilium plugin schema settings` | The format of `settings.schema`. |

- **[PKG-005]** `ilium plugin schema <kind>` MUST print the schema for the running CLI version to standard output, work offline, and exit 0. An unknown `<kind>` MUST exit 2 `usage.unknown-kind` and list the kinds.
- **[DEV-937]** Each schema MUST have the URL form `https://schemas.ilium.dev/plugin/<host-version>/<kind>.schema.json` (proposed, owner to confirm) and the schema's `$id` MUST equal that URL. The page for each kind MUST name the host version it was checked against.
- **[DEV-990]** `new` MUST write the `$schema` key into every generated file whose format allows it, set to the URL of the CLI's own version. `check` MUST report a `$schema` that names a newer host version than the CLI as `schema.version-newer`, and a file that fails its schema with the same path-based error as run time.
- **[DEV-168]** (editor support) The project supports one editor integration: a Visual Studio Code extension (proposed, owner to confirm) that runs `check` and `lint` on save, shows their records as editor diagnostics, and reads the schemas above. Its status is "supported for the current and previous host version". Other editors get the schemas and the `--json` records and have no support commitment.

## Data movement commands: export, import, backup, restore, data, export-data, erase-data

Every command here follows the output model above. Synopses are in the table; shared rules follow it. The procedures are in [Backup and restore](/pages/guide-backup-and-restore/).

| Command | Needs a session | Purpose |
|---|---|---|
| `ilium plugin export <id> --out <file>` | yes | One plugin's data and non-secret settings to an archive ([LIF-290]). |
| `ilium plugin import <id> --from <file>` | yes | Read such an archive into an installed plugin ([LIF-291], [LIF-292]). |
| `ilium plugin backup <file.tar.zst> [--include-grants] [--include-secrets] [--force]` | no | All plugin state to one archive ([HST-652]). |
| `ilium plugin restore <file> [--dry-run] [--merge\|--replace] [--force]` | no | Restore a backup; the session server must be stopped ([HST-653]). |
| `ilium plugin data export <id> <dir>` | yes | The plugin's data as plain files ([HST-656]). |
| `ilium plugin data import <id> <dir>` | yes | Read plain files back ([HST-656]). |
| `ilium plugin data prune --older-than <days>` | yes | Delete old plugin data ([HST-674]). |
| `ilium plugin du [<id>] [--json]` | no | Bytes used per plugin and per folder ([HST-651]). |
| `ilium plugin export-data`, `erase-data`, `data-report`, `import-data` | yes | The user's own data, as in the privacy flow ([USR-245], [USR-246], [USR-247], [USR-248], [USR-262]). |
| `ilium session forget-plugin-state` | yes | Remove saved plugin state of a session ([HST-691]). |
| `ilium plugin admin purge-user` | no | Remove a user's plugin data on a shared machine ([HST-689]). |

- **[LIF-293]** Exit codes: 0 on success; 1 on a failed operation; 2 on a usage error; 3 when `restore --dry-run` finds a difference it would apply; 6 when a session command cannot reach its session; 130 on interrupt. An interrupted `export`, `backup` or `data export` MUST leave no partial output, as in [DEV-916].
- **[LIF-199]** Each command MUST print `progress` records while it runs, one `artifact` record per file it writes (kind `export`, `backup` or `data`), then `summary` and `result`. `restore` MUST print one `plan` record per plugin before it changes anything. `du` MUST print one `usage` record per plugin with integer byte fields for store, data, storage, cache, logs and backups.
- **[HST-176]** `backup` leaves out secrets, cache, logs, staging and the compile cache. `--include-grants` adds `grants.json` and `consents.json`. `--include-secrets` adds the secret values in plaintext, and the command MUST print a `warning` record saying the archive is not encrypted. `--force` replaces an existing target file.

| Code | Command | Exit | Meaning |
|---|---|---|---|
| `export.exists` | `export` | 1 | The `--out` file exists. |
| `import.id-mismatch` | `import` | 1 | The archive was written for another plugin id. |
| `import.format-unsupported` | `import` | 1 | The archive format is not known to this host. |
| `restore.server-running` | `restore` | 1 | A session server is running; stop it first. |
| `restore.format-unsupported` | `restore` | 1 | The archive has an unknown major format ([HST-654]). |
| `restore.host-newer` | `restore` | 1 | The archive was written by a newer host ([HST-654]). |
| `backup.inside-live-root` | `backup` | 1 | The target lies inside a folder the backup would read. |

The four commands whose names contain "export" write four different things. Each has one spelling:

| Command | Writes | Read back with |
|---|---|---|
| `export <id> --out <file>` | One plugin's data and non-secret settings, as one archive ([LIF-290]) | `import <id> --from <file>` |
| `data export <id> <dir>` | One plugin's data as plain files in a folder ([HST-656]). It takes a folder, never `--out <file>` | `data import <id> <dir>` |
| `export-data` | The user's own data across all plugins, for the privacy flow ([USR-245]) | `import-data <folder>` |
| `export-bundle <dir> <id>...` | Signed packages and registry metadata for an offline machine, with no plugin data ([PKG-412](/pages/reference-registry/)) | `install --from-bundle <dir> <id>` |

- **[PRM-009]** The system must list, in the Command inventory of this page, every `ilium plugin <word>` subcommand that any page of the manual quotes as a command to run, and must list, in the error table of this page, every `usage.<code>` error code that any page cites. The words that the inventory names as not existing (`run`, `watch`, `package`, bare `bundle`, `diagnostics`) and a deliberately misspelt word in a test of [DEV-093] are exempt, and so is a `usage.` string that is an SDK field and not an error code, such as `usage.spend`. Test: a lint test extracts every backticked `ilium plugin <word>` and every `usage.<code>` string from all manual pages, removes the exempt list, and fails naming each word that no inventory row starts with or lists after a `/` or `|`, and each code that has no row in the error table.

- **[HST-691]** The commands in the "yes" rows of the table MUST follow the unreachable and unresponsive rules in [DEV-006].

## Command inventory

This section lists every `ilium plugin` subcommand. A subcommand that is not in the table below does not exist: `ilium plugin run`, `watch`, `package`, `bundle` (without `create` or `import`) and `diagnostics --out` fail with `usage.unknown-command` and exit 2. Every command also takes the global flags of [Conventions](#conventions), and every command that prints text by default prints its records with `--json`. The exit codes are those of [Exit codes](#exit-codes) and of [Distribution commands](/pages/reference-plugin-distribution-cli/#exit-codes), except where a row gives another meaning. The section or requirement in the last column holds the rules of the command.

| Command (after `ilium plugin`) | Needs a session | Default output | JSONL record types | Exit codes | Defined in |
|---|---|---|---|---|---|
| `new <dir> [--id <id>] [--lang rust\|ts] [--side <side>]... [--template <name>] [--ci <system>] [--license <spdx-id>] [--sdk-channel stable\|beta]` | no | JSONL | `progress`, `artifact` | 0, 1, 2, 4 | [new](#new) |
| `build [<dir>] [--release\|--debug] [--side <side>] [--watch] [--manifest-path <file>]` | no | JSONL | `progress`, `artifact` | 0, 1, 2, 4 | [build](#build) |
| `check [<dir>] [--release] [--deps] [--baseline [<package>]] [--against <host-version>] [--rules-version <v>] [--print-schema] [--allow-secret-path <path>]... [--all]` | no | text | `check` | 0, 2, 3, 4 | [check](#check) |
| `lint [<dir>] [--deny warnings] [--fix] [--rule <id>]... [--rules-version <v>] [--all]` | no | text | `lint` | 0, 2, 3 | [lint](#lint) |
| `fmt [<dir>] [--check]` | no | text | `artifact` (kind `manifest`) | 0, 1 with `--check` when a file would change, 2 | PRP-005 |
| `test [<dir>] [--component] [--fixture <name>] [--update] [--record] [--replay <file>] [--report <file>] [--coverage] [--min-coverage <n>] [--conformance] [--host-contract <host-version>] [--migrate-from <version>] [--colour-depth 8\|16\|256\|truecolor] [--all]` | no | JSONL | `test-case`, `diff`, `submission`, `outcome`, `artifact` | 0, 2, 3, 4 | [test](#test) |
| `test-report <report.json> --format junit\|json [--out <path>]` | no | text | `artifact` (kind `report`) | 0, 1, 2 | [CI test report formats](/pages/guide-ci-test-report-formats/) |
| `bench [<dir>] [--fixture <name>] [--iterations <n>] [--baseline <file>] [--max-regression <percent>]` | no | JSONL | `timing` | 0, 2, 3, 4 | DEV-922 |
| `pack [<dir>] [--out <file>] [--include-src] [--allow-debug] [--allow-secret-path <path>]... [--sbom] [--signed-at <time>] [--all]` | no | JSONL | `progress`, `artifact` | 0, 1, 2 | [pack](#pack) |
| `keygen --publisher-id <id> --publisher <name> --out <path>` | no | JSONL | `artifact` | 0, 1, 2 | [keygen, sign and trust](#keygen-sign-and-trust) |
| `sign <package> (--key <file> \| --signer <command> --public-key <file>) [--signed-at <time>]` | no | JSONL | `artifact` | 0, 1, 2, 5 | [keygen, sign and trust](#keygen-sign-and-trust) |
| `trust add\|remove\|list\|revoke [<file>] [--tier T2\|T3] [--system] [--fingerprint <key_id>] [--rotation <file>] [--additional] [--statement <file>] [--reason <text>] [--force]` | no | JSONL (`trust list`: text) | `trust` | 0, 1, 2, 5, 7 | [Signing and trust](/pages/reference-signing-and-trust/) |
| `publish <file> [--registry <url>] [--channel stable\|beta] [--notes <file>] [--verify] [--allow-unstable-sdk]` | no | JSONL | `progress`, `publish` | 0, 1, 2, 5, 7, 8 | [Distribution commands](/pages/reference-plugin-distribution-cli/) |
| `verify-published <id>[@<version>] [--registry <url>]` | no | JSONL | `progress`, `verify` | 0, 1 with `publish.verify-failed`, 2, 5, 8 | PRP-002 |
| `verify <package>` | no | text | `verify` | 0, 1, 2, 5 | [Distribution commands](/pages/reference-plugin-distribution-cli/) |
| `verify-build <package>` | no | JSONL | `progress`, `diff` | 0, 1 with `package.build-mismatch`, 2, 4 | PKG-715 |
| `verify-reproducible [<dir>]` | no | JSONL | `progress`, `diff` | 0, 1 with `build.not-reproducible`, 2, 4 | DEV-892 |
| `verify-conformance <report> [<artefact>]` | no | text | `verify` | 0, 2, 3 | PDT-043 |
| `preview [<dir>] [--locale <tag>]` | no | text | `inspect` | 0, 1, 2 | [preview](#preview) |
| `docs [<dir>] [--out <dir>]` | no | JSONL | `artifact` (kind `docs`) | 0, 1, 2 | DEV-938 |
| `schema manifest\|fixture\|expectation\|replay\|policy\|record` | no | text (the schema) | none | 0, 2 | [Published schemas](#published-schemas-and-editor-support) |
| `clean [<dir>] [--all]` | no | JSONL | `artifact` | 0, 1, 2 | DEV-900 |
| `diff <old.iliumpkg> <new.iliumpkg>` | no | JSONL | `diff` | 0, 2, 3 when the packages differ | DEV-939 |
| `upgrade-sdk [<dir>] [--to <version>] [--dry-run] [--sdk-channel stable\|beta]` | no | JSONL | `diff` | 0, 1, 2, 4 | DEV-912 |
| `template verify <path\|git-url> [--lang <lang>]` | no | JSONL | `check` | 0, 2, 3 | [Project templates](/pages/reference-project-templates/) |
| `completions bash\|zsh\|fish\|powershell` | no | text (the script) | none | 0, 2 | DEV-907 |
| `explain <code>` | no | text | `explain` | 0, 2 for an unknown code | USR-347 |
| `symbolize <trap.json> --debug-info <file>` | no | text | `frame` | 0, 1 with `debug.hash-mismatch`, 2 | DEV-878 |
| `doctor [--fix] [--sandbox] [--bundle <file>] [--clear-crash-marks]` | no | text | `doctor-check`, `check` with `--sandbox` | 0, 4 | [doctor](#trace-profile-inspect-and-doctor) |
| `inspect <file\|id> [--view <section>] [--verify] [--memory]` | file: no; id: yes | text | `inspect`, `diff` | 0, 1, 2, 5, 6 | [inspect](#trace-profile-inspect-and-doctor) |
| `install <source> [--allow-unsigned] [--allow-downgrade] [--trust <key_id>] [--channel stable\|beta\|dev] [--yes] [--grant <grant>]... [--locked] [--dry-run] [--registry <url>] [--from-bundle <dir>] [--no-backup] [--unskip] [--reinstall] [--system] [--accept-stale-revocation]` | yes | JSONL | `progress`, `plugin` | 0, 1, 2, 4, 5, 6, 7, 8 | [Distribution commands](/pages/reference-plugin-distribution-cli/) |
| `reinstall <id>` | yes | JSONL | `progress`, `plugin` | 0, 1, 5, 6 | LIF-064 |
| `uninstall <id> [--keep-data\|--purge] [--yes]` | yes | JSONL | `progress`, `plugin` | 0, 1, 5, 6 | [install, uninstall, enable, disable](#install-uninstall-enable-disable) |
| `enable <id> [--scope <scope>] [--accept-risk] / enable --all-third-party` | yes | JSONL | `plugin` | 0, 1, 5, 6, 7 | [install, uninstall, enable, disable](#install-uninstall-enable-disable) |
| `disable <id> [--scope <scope>] [--reason <text>] / disable --all-third-party --reason <text>` | yes | JSONL | `plugin` | 0, 1, 5, 6 | [install, uninstall, enable, disable](#install-uninstall-enable-disable) |
| `restart <id>` | yes | JSONL | `plugin` | 0, 1, 5, 6 | COR-109 |
| `kill <id> --reason <text>` | yes; offline it writes `kill.json` and prints `path: offline` | JSONL | `kill` | 0, 2, 5 | SCU-003 |
| `unkill <id>` | yes | JSONL | `kill` | 0, 5, 6, 7 | SCU-003 |
| `reset-failures <id>` | yes | JSONL | `plugin` | 0, 5, 6 | LIF-036 |
| `mute <id> [--off]` | yes | JSONL | `plugin` | 0, 5, 6 | HST-641 |
| `switch <old-id> --to <successor-id> [--yes]` | yes | JSONL | `progress`, `switch` | 0, 1, 2, 5, 6, 7 | [Install and update](/pages/reference-install-and-update/) |
| `update [<id>\|--all] [--dry-run] [--dry-run-migrate] [--channel <c>] [--from-registry] [--yes]` | yes | JSONL | `progress`, `plugin` | 0, 1, 2, 5, 6, 7, 8 | [Distribution commands](/pages/reference-plugin-distribution-cli/) |
| `update-cancel <id>` | yes | JSONL | `plugin` | 0, 1 with `update.in-progress`, 5, 6 | LIF-707 |
| `outdated [<id>] [--channel <c>]` | yes | text | `outdated` | 0, 6, 8 | PKG, [Registry](/pages/reference-registry/) |
| `rollback <id> [--restore-backup\|--discard-data]` | yes | JSONL | `progress`, `plugin` | 0, 1, 5, 6 | [Distribution commands](/pages/reference-plugin-distribution-cli/) |
| `pin <id>@<version> / unpin <id> / channel <id> <channel>` | yes | JSONL | `plugin` | 0, 1, 5, 6 | [Distribution commands](/pages/reference-plugin-distribution-cli/) |
| `lock [--out <file>] / sync --locked <lockfile> [--from-bundle <dir>] [--registry <url>]` | yes | JSONL | `progress`, `plugin` | 0, 1 with `lock.mismatch`, 2, 6, 7, 8 | [Distribution commands](/pages/reference-plugin-distribution-cli/) |
| `search <words> [--category <c>] [--limit <n>] [--offset <n>] / info <id>` | no | text | `plugin` | 0, 2, 5, 8 | [Distribution commands](/pages/reference-plugin-distribution-cli/) |
| `registry add\|remove\|list\|show [--root-key <file>] / mirror <url> [--allow <glob>]... / mirror sync <dir> --registry <url> [--allow <id>...] [--channel stable\|beta]` | no | text | `registry` | 0, 1, 2, 5, 8, and 75 when `mirror sync` finds the mirror lock held (`mirror.sync-busy`) | [Distribution commands](/pages/reference-plugin-distribution-cli/) |
| `yank <id>@<version> --reason <text> [--confirm-dependents] / unyank / unlist / deprecate <id> --message <text> [--successor <id>] / rollout <id>@<version> --percent <n> / owner add\|remove\|list / transfer <id> --to <publisher> --new-key <file> / advisory publish <id> --file <osv.json>` | no | JSONL | `publish` | 0, 1, 2, 5, 7, 8 | [Distribution commands](/pages/reference-plugin-distribution-cli/) |
| `export-bundle <dir> <id>[@range]... [--with-deps] / bundle create --out <file> [<id>...] [--with-deps] / bundle import <file>` | no | JSONL | `progress`, `artifact` (kind `bundle`) | 0, 1 on a hash or signature mismatch (`registry.hash-mismatch`, `registry.signature-invalid`, `registry.rollback`, `bundle.root-not-pinned`), 2, 5, 8 | PKG-412, [Air-gapped transfer](/pages/guide-air-gapped-transfer/) (PDV-001, PDV-002) |
| `list [--inventory] [--refused-abi]` | yes | text | `plugin` | 0, 6 | [list and grants](#list-and-grants) |
| `grants <id> [--grant <grant>]... [--revoke <grant>]... [--usage] / grant <id> <grant> --until <timestamp>` | yes | text | `grant` | 0, 2 with `grant.expiry-invalid`, 5, 6, 7 | [list and grants](#list-and-grants) |
| `permissions <id>` | yes | text | `grant` | 0, 5, 6 | COR-221 |
| `consents <id> [--withdraw <target>]` | yes | text | `consent` | 0, 5, 6 | [Grant review and consent](/pages/reference-grant-review-and-consent/) |
| `exposure <id> [--since <time>]` | no (reads the audit log and the grant history) | text | `exposure` | 0, 2, 5 | PRN-018 |
| `policy show / policy check <candidate-file> [--user-file <path>]` | no | JSONL | `policy` | 0, 1 when a system file is ignored or malformed, 2 | [Plugin policy](/pages/reference-admin-policy/) |
| `audit [--plugin <id>] [--since <time>] [--event <name>] [--kind <kind>] [--limit <n>] [--verify [<file>]] [--export <file>]` | no | JSONL | `audit`, `summary` | 0, 2, 3 on a chain or schema finding | [Audit log](/pages/reference-audit-log/) |
| `status [<id>] [--safe-mode] [--startup] [--days <n>]` | yes | text | `plugin`, `crash-mark` with `--safe-mode` | 0, 5, 6 | [status](#status) |
| `host-status` | no (reads state files; a running session adds live counts) | text | `host-status` | 0 healthy, 1 degraded, 2 critical | PRO-022 |
| `health [<id>]` | yes | text on a terminal, else JSONL | `health` | 0, 1 when any plugin is `degraded`, 2 when any is `failed`, 6 | HST-617 |
| `alerts [--ack <alert-id>\|--ack --all]` | yes | text | `alert` | 0, 5, 6 | [Health and watchdog](/pages/reference-health-and-watchdog/) |
| `history [<id>] [--since <time>]` | no | text | `history` | 0, 5 | PRP-017 |
| `crashes [<id>] [--host]` | no | text | `crash` | 0 | [Audit log](/pages/reference-audit-log/) |
| `top` | yes | text, refreshed at most once per second | `metric` | 0, 6, 130 | HST-634 |
| `stats [<id>] [--host] [--cache] / metrics [--host] [--out <file>] / limits [<id>]` | yes | text | `metric`, `limit` | 0, 1, 5, 6 | [Host telemetry](/pages/reference-host-telemetry/) |
| `logs [<id>] [--follow] [--tail <n>] [--since <time>] [--level <level>] [--side <side>] [--op <operation-id>] [--verify] [--clear <id>] / log-level <id> <level>\|--reset` | yes (`logs` reads the files when no session runs) | text | `log` | 0, 2, 5, 6, and 7 when `log-level` is refused with `policy.locked` | [logs and log-level](#logs-and-log-level) |
| `dev [<dir>] [--side <side>] [--release] [--timings] [--grant-all] [--replace-installed] [--purge-on-exit] [--scratch-session]` | yes; with `--scratch-session` it starts its own disposable session | JSONL | `progress`, `reload`, `log`, `timing` | 0, 1, 2, 4, 6, 130 | [dev](#dev) |
| `debug <dir> [--adapter]` | yes | JSONL | `artifact` (kind `debug-endpoint`), `diagnostic` | 0, 1 with `debug.not-loopback`, 6, 130 | DEV-877 |
| `trace [<id>] [--follow] [--last <n>] [--kinds <list>] [--record <file>] [--unredacted] / profile <id> [--duration <s>] [--side <side>]` | yes | text | `trace`, `timing`, `submission`, `outcome` | 0, 2, 5, 6 | [trace, profile, inspect and doctor](#trace-profile-inspect-and-doctor) |
| `preflight --target <host-version>` | yes | text | `check` | 0, 3, 6 | DEV-992 |
| `deprecations` | no | text | `deprecation` | 0 | DEV-969 |
| `conformance <id>` | yes | text | `test-case` | 0, 3, 5, 6 | COR-205 |
| `bisect start\|good\|bad\|cancel` | yes | text | `bisect` | 0, 1 with `bisect.state-corrupt`, 6 | USR-346 |
| `recovery` | no | text | `recovery-item` | 0, 78 while an item is unresolved | HST-660 |
| `support-bundle <id>\|--all --out <file> [--dry-run] [--submit-to-author]` | yes | JSONL | `artifact` (kind `support-bundle`) | 0, 1, 2 with `support-bundle.exists`, 5, 6 | [Diagnostics](/pages/core-plugin-diagnostics/) |
| `report <id> [--submit] [--rule <rule>] / report --status <report-id>` | no | text | `report` | 0, 2, 5, 8 | [Reporting a plugin](/pages/user-reporting-a-plugin/) |
| `incident check <file>` | no | text | `check` | 0, 3 | [Bad release recall](/pages/guide-bad-release-recall-playbook/) |
| `i18n-coverage [--json] [--language <tag>]` | no | text | bare lines without `type` (the one exception to [DEV-016] and [DEV-017]: exactly one line per installed plugin, no `summary` and no `result` record) | 0, 2 | [Language resolution and coverage](/pages/reference-language-resolution-and-coverage/#coverage-command) (PDT-023) |
| `export\|import\|backup\|restore\|data\|export-data\|import-data\|erase-data\|du\|config export\|config import` | see the section | JSONL | `progress`, `artifact`, `plan`, `usage`, `restore-summary`, `erase` | 0, 1, 2, 3, 5, 6 | [Data movement commands](#data-movement-commands-export-import-backup-restore-data-export-data-erase-data) |
| `data-report [<id>]` | no | text | `data-kind` | 0, 5 | USR-322 |
| `gc --kept-data --older-than <days> [--dry-run]` | no | JSONL | `erase` | 0, 2 | PRO-024 |
| `log monitor` | no | JSONL | `finding` (code `unexpected-release`), `summary` | 0, 1 when an entry has no matching release record, 2, 8 | [Publisher workflow](/pages/publish-publisher-workflow/) (PRW-039) |
| `purge-all --yes / admin purge-user <user> --yes` | no; refused with `purge.server-running` or `purge.user-active` while a session server runs | JSONL | `erase` | 0, 1, 2 without `--yes`, 3 with `erase.incomplete` | USR-024, HST-689 |
| `secret rotate <id> <name> / token rotate [--http\|--lock <id>]` | yes | JSONL | `rotation` | 0, 1, 5, 6 | SEC-451, SEC-452 |

### Flags added by later requirements

| Command | Flag | Type and range | Default | Meaning |
|---|---|---|---|---|
| `logs` | `--op <operation-id>` | string, 26 characters | none | Prints only the records that carry this operation id (PRN, [Diagnostics and logs](/pages/ops-diagnostics-and-logs/)) |
| `support-bundle` | `--submit-to-author` | switch | off | Prints the exact crash payload and, after the user confirms, sends it to the contact address of the signed manifest ([PRR-012]) |
| `new, upgrade-sdk` | `--sdk-channel stable\|beta` | enum | `stable` | `beta` lets the command select an SDK pre-release ([PRO-012]) |
| `test` | `--migrate-from <version>` | released version of the same plugin | none | Seeds storage with data of that version and runs `migrate`; fails with `test.migration-incomplete`, exit 3 ([PRP-001]) |
| `publish` | `--allow-unstable-sdk` | switch | off | Lets a project that uses experimental SDK items publish to a stable channel. Without it the refusal is `lint.sdk-unstable-used`, exit 3 |
| `doctor` | `--clear-crash-marks` | switch | off | Clears the automatic safe mode crash counter ([PRP-021]) |
| `gc` | `--kept-data` | switch | required | Selects the kept data of uninstalled plugins. `gc` without it exits 2 with `usage.missing-argument` |
| `gc` | `--older-than <days>` | integer, 1 to 3,650 | required | Only kept data older than this many days is deleted |
| `dev` | `--scratch-session` | switch | off | Runs the plugin in a disposable session with its own state, runtime and socket folders ([PRO-009]) |
| `install, update` | `--accept-stale-revocation` | switch | off | Confirms an install while the revocation data is stale. Refused under the policy key `packages.require_fresh_revocation` |
| `check, pack` | `--allow-secret-path <path>` | path, repeatable, no globs | none | Accepts one file that looks like a secret |
| `check` | `--baseline [<package>]` | file, optional | the highest stable version in the registry | Reports the grant and public-surface delta ([PRQ-024], [PRO-010], [PRO-011]) |
| `check` | `--against <host-version>` | version | the installed host | Checks against the headless host archive of that release |
| `check, lint` | `--rules-version <v>` | version | the newest embedded catalogue | Runs exactly the rule catalogue of that version |
| `check` | `--deps`, `--print-schema` | switches | off | Report the SDK dependency state offline; print the manifest schema |
| `bench` | `--iterations <n>`, `--baseline <file>`, `--max-regression <percent>` | integer 1 to 100,000; file; integer 0 to 1,000 | 100; none; 20 | Iterations after 10 warm-up calls; a baseline to compare with; the regression that makes `bench` exit 3 |
| `test` | `--colour-depth 8\|16\|256\|truecolor` | enum | `truecolor` | Colour depth of the test host, written into the report |
| `new` | `--license <spdx-id>` | SPDX identifier | none | Writes `LICENSE` and the manifest `license`; else `new.license-unknown` |
| `pack` | `--sbom`, `--signed-at <time>` | switch; RFC 3339 UTC time | off; the current time | Write a CycloneDX file; fix the time for a reproducible archive |
| `sign` | `--signer <command>`, `--public-key <file>` | string; file | none | External signer and its public key (`sign.public-key-required`) |
| `keygen` | `--publisher-id <id>` | publisher id | required | The id written into the key file |
| `trust add` | `--tier T2\|T3`, `--system`, `--fingerprint <key_id>`, `--rotation <file>`, `--additional` | enum; switch; key id; file; switch | `T3`; off; none; none; off | See [Signing and trust](/pages/reference-signing-and-trust/) |
| `trust revoke` | `--statement <file>` | file | none | Imports an `ilium-rev/1` revocation statement |
| `install` | `--system`, `--channel stable\|beta\|dev`, `--unskip` | switch; enum; switch | off; `stable`; off | System-wide install as root; release channel; clear the `skip` mark of this version |
| `enable` | `--accept-risk` | switch | off | Needed to enable a `retired` plugin |
| `enable, disable` | `--all-third-party`, `--scope <scope>` | switch; `user`, `project` or `session` | off; `user` | Every third-party plugin at once; where the choice is stored |
| `list` | `--inventory`, `--refused-abi` | switches | off | One object per plugin with hash and key id; only plugins refused for their `api` |
| `grants` | `--usage` | switch | off | Adds the last use and the use count of each grant |
| `grant` | `--until <timestamp>` | RFC 3339 UTC time, at most 365 days ahead | none | Expiry of the grant; else `grant.expiry-invalid`, exit 2 |
| `consents` | `--withdraw <target>` | string | none | Removes one stored decision |
| `audit` | `--event <name>`, `--plugin <id>`, `--limit <n>`, `--export <file>` | string; id; integer 1 to 100,000; file | none; none; 1,000; none | Filters, and export with an `anchor` line |
| `alerts` | `--ack [<alert-id>]` | string, optional | none | Acknowledges one alert, or all with `--all` |
| `data` | `--bucket <scope>`, `--project <path>`, `--older-than <days>` | string; path; integer 1 to 3,650 | none | Filters of `data list` and `data prune` |
| `logs` | `--clear <id>` | plugin id | none | Deletes the log files of one plugin |
| `status` | `--safe-mode`, `--startup`, `--days <n>` | switch; switch; integer 1 to 30 | off; off; none | Live crash marks; startup phase times; daily summaries |
| `stats` | `--cache` | switch | off | Size and hit rate of the host caches |
| `doctor` | `--sandbox`, `--bundle <file>` | switch; file | off; none | Sandbox prerequisites; a zip with `doctor.txt` and `host.txt` ([DEV-941]) |
| `debug` | `--adapter` | switch | off | Starts the stdio-to-endpoint bridge for a DAP client |
| `deprecate` | `--message <text>`, `--successor <id>` | string of at most 200 characters; plugin id | required; none | See [Registry](/pages/reference-registry/) |
| `publish` | `--channel stable\|beta`, `--notes <file>` | enum; file | `stable`; the top section of `CHANGELOG.md` | Release channel and release notes |
| `report` | `--submit`, `--rule <rule>`, `--status <report-id>` | switch; string; string | off; none; none | See [Reporting a plugin](/pages/user-reporting-a-plugin/) |
| `restore` | `--merge`, `--replace` | switches, one of them | `--merge` | Keep or remove the plugins that the archive does not hold |
| `rollback` | `--restore-backup`, `--discard-data` | switches, at most one | off | What to do with data newer than the target version |
| `search` | `--category <c>`, `--limit <n>`, `--offset <n>` | string; integer 1 to 100; integer 0 to 10,000 | none; 20; 0 | Search filters |
| `import` | `--from-id <id>` | plugin id | none | Must equal the id inside the archive, else `import.from-id-mismatch` |
| `policy check` | `--user-file <path>` | path | the user file of this profile | The user file merged with the candidate |
| `preflight` | `--target <host-version>` | version | required | The host version to test against |
| `token rotate` | `--http`, `--lock <id>` | switch; lock id | one is required | Which token is rotated |
| `update` | `--dry-run-migrate`, `--from-registry` | switches | off | Report the migration result without applying; adopt a side-loaded plugin from the registry |

### Records added by later requirements

These types extend [the record catalogue](#the-record-catalogue). Every time is `YYYY-MM-DD HH:mm:ss` in UTC.

| `type` | Fields | Printed by |
|---|---|---|
| `alert` | `id`, `severity` (1 to 3), `plugin`, `code`, `raised_at`, `acknowledged` | `alerts --json` |
| `audit` | the fields of [SEC-221](/pages/reference-audit-log/) | `audit` |
| `bisect` | `round`, `rounds_max`, `disabled` (list of ids), `suspect` (id or `null`) | `bisect` |
| `consent` | `plugin`, `version`, `grant`, `decision` (`granted`, `declined`), `at`, `surface` (`dialog`, `cli`, `policy`) | `consents --json` |
| `crash` | `at`, `plugin`, `side`, `reason`, `export`, `file` | `crashes --json` |
| `crash-mark` | `at`, `boot_id`, `phase` | `status --safe-mode --json` |
| `data-kind` | `plugin`, `kind`, `bytes`, `files`, `newest_at` | `data-report --json` |
| `deprecation` | `kind`, `name`, `since`, `remove_in`, `owner` | `deprecations --json` |
| `diagnostic` | `code`, `message` | `debug` |
| `erase` | `kind`, `files`, `bytes`, `remaining` | `erase-data`, `purge-all`, `admin purge-user`, `gc` |
| `explain` | `code`, `stage`, `exit_code`, `cause`, `fix` | `explain --json` |
| `exposure` | `plugin`, `from`, `to` (or `null` while enabled), `grants`, `file_roots`, `pane_kinds`, `agent_config_dirs`, `egress_http`, `egress_inference`, `secrets` (names only) | `exposure --json`, one per interval |
| `frame` | `index`, `function`, optional `file`, `line` | `symbolize --json` |
| `health` | `id`, `version`, `side`, `state`, `reason`, `restarts_10m` | `health --json` |
| none (bare line, no `type` key) | `plugin_id`, `language` (the language used for that plugin in this run), `message_count`, `missing_count` (unsigned integers); exactly these four keys | `i18n-coverage --json` ([PDT-023]) |
| `history` | `plugin`, `at`, `op`, `version`, `outcome`, optional `prev_version`, `delta` | `history --json` |
| `host-status` | `safe_mode`, `crash_marks_in_window`, `disk_step`, `revocation_age_seconds`, `plugins_by_health` (object of counts), `update_pending`, optional `last_backup_age_seconds` | `host-status --json`, exactly one |
| `kill` | `plugin`, `state` (`killed`, `released`), `path` (`session`, `offline`) | `kill`, `unkill` |
| `limit` | `plugin`, `name`, `requested`, `applied`, `source` (`default`, `manifest`, `user`, `policy`), `usage` | `limits --json` |
| `metric` | `plugin`, `side`, `name`, `value`, optional `limit`, `source`, `bucket_at` | `stats --json`, `metrics`, `top --json` |
| `outdated` | `id`, `installed`, `latest`, `channel`, `grants_added` (list of `{ grant, risk }`) | `outdated --json` |
| `plan` | `plugin`, `action` (`restore`, `skip`, `replace`, `remove`), `data_bytes`, `grants`, `secrets` | `restore`, before any change |
| `policy` | `file`, `status` (`used`, `ignored`, `malformed`, `absent`), optional `key`, `value`, `source` | `policy show`, `policy check` |
| `publish` | `id`, `version`, `channel`, `action`, `url` | `publish` and the registry owner commands |
| `recovery-item` | `path`, `code`, `actions` (list of `rebuild`, `restore`, `start-without`) | `recovery --json` |
| `registry` | `url`, `name`, `state`, `index_age_seconds` | `registry list --json` |
| `report` | `report_id`, `status` (`pending`, `accepted`, `rejected`), optional `decision` | `report --json` |
| `restore-summary` | `plugins`: list of `{ id, data_restored, grants_to_review, secrets_to_enter, trust_state (`kept`, `tampered-renamed`), left_disabled }` | `restore` and `restore --dry-run`, exactly one, before `summary` |
| `rotation` | `plugin`, `name`, `kind` (`secret`, `http-token`, `lock-token`) | `secret rotate`, `token rotate` |
| `switch` | `from`, `to`, `migrated_keys`, `unmatched_keys` | `switch` |
| `trust` | `key_id`, `publisher_id`, `tier`, `store` (`user`, `system`), `state` | `trust list --json` |
| `usage` | `plugin`, `store_bytes`, `data_bytes`, `storage_bytes`, `cache_bytes`, `logs_bytes`, `backups_bytes`, and one final record with `class_totals` (`active`, `backups`, `kept-uninstalled`, `moved-aside`, `caches`) | `du --json` |
| `verify` | `subject`, `step`, `ok`, optional `code` | `verify`, `verify-published`, `verify-conformance` |

The enumerations of DEV-021, DEV-022 and DEV-024 hold the values that these commands use. The `status` values of a record in this table are the ones listed in its row.

- **[PRM-106]** The system must list every `ilium plugin` subcommand in the Command inventory of this page with its synopsis, its flags with type, default and meaning, whether it needs a session, its default output mode, its JSONL record types with their fields, and its exit codes. A test must compare the subcommands and flags of `ilium plugin --help` and of every `ilium plugin <command> --help` with this inventory and must fail naming each subcommand or flag that is in only one of them.

- **[PRM-107]** The system must print, from every `ilium plugin` command, only JSONL record types that the record catalogue of this page lists, with only the listed fields and enumerated values. The golden-output test of each command must validate each printed record against its catalogue entry and must fail on an unlisted record type, field or enumerated value.

## Production additions (round 10)

- **[PRR-010]** The system must, when ilium plugin check, lint, test, pack or publish runs with --all at a repository root holding several plugin projects, process projects in sorted plugin-id order, print one summary record per project and one overall summary, continue after a failing project, exit with the highest project exit code, and publish nothing when any project check or lint failed (also documented in reference-project-templates).

## Production additions (round 11)

- **[PRQ-024]** The system must provide ilium plugin check --baseline that classifies each public-surface difference as breaking, additive or neutral and fails with lint.semver-bump-too-small (exit 3) when the version bump is too small, and publish must refuse the same way unless --allow-semver-violation with a reason is given and audited.

## Production additions (round 12)

- **[PRP-005]** The system must provide ilium plugin fmt that rewrites ilium-plugin.toml into one canonical layout preserving comments, is idempotent and never changes the parsed manifest value, and with --check exits 1 listing files that would change.

## Production additions (round 13)

- **[PRO-009]** The system must offer ilium plugin dev --scratch-session, which starts a disposable session with its own state, runtime and socket folders and a fixture tree of 3 panes, installs the plugin only there, never contacts or reads any other session, and removes the session on exit; the first dev run in a project with no --session must print a notice that the plugin runs in the live session and name this flag.
- **[PRO-011]** The system must resolve --baseline from an explicit package file, or else from the highest non-yanked stable version of the same id in the configured registry. With no baseline it must exit 0 with baseline.none-first-release, when the registry is unreachable it must fail with registry.unreachable unless a file was given, and the resolved baseline version and SHA-256 must appear in the summary record.

## Production additions (round 14)

- **[PRN-006]** The system must publish a signed, machine-readable feed of known host defects. Each entry must carry affected host versions, the error codes or log events that signal it, a workaround and the fixing version. `ilium plugin doctor` must print every entry that matches the installed host and the project's last failing run, and must label the entry as a host defect rather than a plugin defect.

## Production additions (round 15)

- **[PRM-203]** The system must provide `ilium plugin preview <package-or-dir> [--locale <tag>] [--json]`, which prints, offline and without installing, what the store detail view and the install review dialog would show (name and description in the chosen locale, grants with risk levels, README text, screenshot alt text, support contact, licence expression), and exits 3 with `lint.preview-field-missing` or `lint.preview-truncated` when a shown field is missing or would be truncated.
