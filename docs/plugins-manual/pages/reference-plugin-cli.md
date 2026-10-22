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

- **[DEV-004]** A session command MUST find its session this way: the session named by `--session`, in the project that `--cwd` names, defaulting to the session called `default` of the current directory. No session found at that address MUST fail with `session.unreachable` and exit code 6. A command MUST NOT start a session itself.
- **[DEV-005]** The commands in the offline column of the table below MUST work with no running session, no network and no home-directory configuration beyond the trust store.
- **[DEV-006]** A session command that cannot reach its session MUST print one `error` record with the code `session.unreachable`, then `summary`, then `result`, and exit with 6. It MUST NOT retry.

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

The output default of each command is in [Which commands print text](#which-commands-print-text).

## Exit codes

Codes 7 (policy) and 8 (network) are added by [Distribution commands](/pages/reference-plugin-distribution-cli/#exit-codes).

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
- **[DEV-011]** Exit code 3 MUST be returned when a `check` finding of severity `error` exists, when `lint --deny warnings` has any finding, when a lint rule of severity `error` fires, or when at least one test case does not pass. A finding of severity `warn` under `check` or plain `lint` returns 0.
- **[DEV-012]** Exit code 4 MUST be returned when a `toolchain.*` code stops the command, and when `doctor` reports any item with status `fail`.
- **[DEV-013]** Exit code 5 MUST be returned for `plugin.not-found`, `trust.not-found` and `inspect.id-unknown`.
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
| `grant` | `plugin`, `grant`, `risk`, `state` | `grants --json` |
| `inspect` | `section`, `data` | `inspect --json` |
| `doctor-check` | `item`, `status`, `message`, `fix` | `doctor` |
| `summary` | `duration_ms`, `warnings`, `errors` | all JSONL commands |
| `result` | `ok`, `exit_code` | all JSONL commands |

- **[DEV-021]** The `step` field of `progress` MUST be one of `toolchain`, `compile`, `bundle`, `component`, `api-version`, `strip`, `inventory`, `pack`, `sign`, `check`, `lint`, `test`, `install`, `watch`.
- **[DEV-022]** The `kind` field of `artifact` MUST be one of `component`, `package`, `source-map`, `build-info`, `key`, `public-key`, `signature`, `replay`.
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

## lint

Reports advisory problems.

`ilium plugin lint [<dir>] [--deny warnings] [--rule <id>]... [--json]`

- **[DEV-075]** `lint` MUST run offline and MUST report only `lint.*` rules.
- **[DEV-076]** `--deny warnings` MUST turn every `warn` finding into an `error` for the exit code. Without it a `warn` finding returns 0.
- **[DEV-077]** `--rule <id>` MUST restrict the run to the named rules. An unknown id MUST fail with `usage.invalid-value`.
- **[DEV-078]** A suppressed finding MUST be printed with `suppressed: true` and MUST NOT count toward the exit code. Suppression is described in the rules page.

## test

Runs fixtures against the plugin. The harness behind it is in [Test harness reference](/pages/reference-test-harness/). The fixture and expectation formats are in the same page.

`ilium plugin test [<dir>] [--fixture <glob>]... [--expect <glob>] [--grants <list> | --grant-manifest] [--side <side>] [--component <file>] [--replay <file>] [--update] [--record <file>] [--seed <n>] [--conformance [--subject <subject>] [--case <id>]] [--coverage [--min-coverage <percent>]]`

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

## install, uninstall, enable, disable

`ilium plugin install <source> [--allow-unsigned] [--allow-downgrade] [--trust <key_id>] [--channel stable|beta|dev] [--yes] [--grant <grant>]... [--locked] [--dry-run] [--registry <url>] [--from-bundle <dir>] [--no-backup] [--unskip]`

- **[DEV-140]** `install` MUST run the validation pipeline of [Package format](/pages/reference-package-format/#the-validation-order) before it runs any plugin code, print every failure as an `error` with the code of that pipeline, and exit with the code of [Distribution commands](/pages/reference-plugin-distribution-cli/#exit-codes). Sources other than a file are defined there.
- **[DEV-141]** `install` MUST show the review dialog in the attached client unless `--yes` is given. With `--yes`, it MUST grant every requested grant whose risk is not high, and MUST grant a high-risk grant only when it is named with `--grant`.
- **[DEV-142]** `install` with no attached client and no `--yes` MUST fail with `install.review-required`.
- **[DEV-143]** A declined review MUST fail with `install.review-declined`. A declined key trust MUST fail with `trust.declined`. An unsigned package without `--allow-unsigned` MUST fail with `install.unsigned`. A lower version without `--allow-downgrade` MUST fail with `package.downgrade`.
- **[DEV-144]** A successful `install` MUST print one `plugin` record for the installed plugin and exit with 0. The plugin is enabled after install.

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

- **[DEV-165]** `list` text mode MUST print one line per plugin with the id, the version, the tier, the state and the grant count. The state word maps from the lifecycle states: `running` is `installed-enabled` with an active instance, `stopped` is `installed-enabled` with no active instance (including `stopped-by-failure`), `disabled` is `installed-disabled-by-user` or `disabled-by-policy`, and `untrusted` is `disabled-untrusted` or `disabled-blocked`. `list --json` MUST print the lifecycle state name itself (LIF-001). A `[dev]` badge MUST mark a plugin installed by `dev`.

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

`ilium plugin inspect (<file.iliumpkg> | <dist-component.wasm> | <id>) [--verify] [--view <target>] [--memory] [--json]`

- **[DEV-800]** `inspect <file>` MUST work offline. `inspect <id>` needs a session.
- **[DEV-801]** `--verify` MUST recompute every hash of the package, compare it with the `files` list and print a `diff` record per difference. A package with a difference MUST exit with 1 and the code `inspect.hash-mismatch`.
- **[DEV-802]** `--view <target>` and `--memory` need a session and the content of the sections is in the debugging tools page.
- **[DEV-803]** `inspect` of an unreadable file MUST fail with `inspect.file-unreadable`, and of a file that is neither a package nor a component with `inspect.not-a-package`.

`ilium plugin support-bundle <id> --out <file>`

- **[DEV-831]** `support-bundle` MUST write the archive that [USR-221] defines to `<file>` and MUST print one `support-bundle` record with the path and the size. It needs a session.
- **[DEV-832]** A `<file>` that already exists MUST NOT be overwritten. The command MUST fail with `support-bundle.exists` and exit with 2.
- **[DEV-833]** An archive over 8 MiB MUST be trimmed as [USR-224] says, and the `support-bundle` record MUST say so.

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

## Error catalogue

These codes are in addition to the `manifest.*` codes of [Packages](/pages/packages/) and the rule codes of [Check and lint rules](/pages/reference-check-and-lint-rules/). The exit column is the exit code of the command when this code stops it.

| Code | Cause | Fix | Exit |
|---|---|---|---|
| `usage.unknown-flag` | A flag does not exist | Check `--help` | 2 |
| `usage.missing-argument` | A required flag or argument is missing | Add it | 2 |
| `usage.conflict` | Two flags contradict each other | Remove one | 2 |
| `usage.invalid-value` | A flag value has the wrong form or an unknown rule id | Fix the value | 2 |
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

- **[DEV-902]** Every project command MUST accept `--manifest-path <file>` naming a `manifest.json`; the project directory is then that file's parent and `<dir>` MUST NOT also be given (else exit 2 `usage.conflicting-flags`). Relative paths in output records MUST be relative to the project directory, never the caller's working directory.

- **[DEV-903]** A project MAY contain `ilium-plugin.toml` in its root. The CLI MUST read only the keys `[build] profile`, `[build] extra-args`, `[test] timeout_ms`, `[lint] deny`, `[dev] ignore` and MUST fail with `config.unknown-key` (exit 2) on any other key. A command-line flag MUST override the file, and the file MUST override defaults but never the manifest.

- **[DEV-904]** The CLI reference MUST list every environment variable the CLI reads (at least `ILIUM_SESSION`, `ILIUM_PLUGIN_SIGNING_KEY`, `NO_COLOR`, `ILIUM_PLUGIN_OFFLINE`) with type, default and the flag that overrides it. The CLI MUST ignore any `ILIUM_PLUGIN_*` variable not in that list and `doctor` MUST emit a `warn` doctor-check named `env.unknown` for each such variable set.

- **[DEV-906]** `ilium plugin --version` MUST print one `version` record (JSONL) or one text line with the CLI version, the plugin contract version, the supported WIT package versions and the embedded test host version, and exit 0 without a session or toolchain.

- **[DEV-908]** `lint --fix` MUST apply only rules whose record carries a non-empty `fix` field, MUST rewrite files atomically (write temp file then rename), MUST print one `artifact` record per modified file, and MUST refuse with `lint.dirty-tree` (exit 1) when the project is in a Git repository with uncommitted changes in a file it would modify, unless `--allow-dirty` is given. A second run MUST change zero files.

- **[DEV-911]** `check` MUST emit lint/check rule `size.component-over-budget` with severity `warn` when a built component exceeds 2 MiB (proposed, owner to confirm) and `error` when it exceeds the package component limit of the package format; the budget MAY be lowered by `[build] max_component_bytes` in the project config and MUST NOT be raised above the format limit.

- **[DEV-912]** `ilium plugin upgrade-sdk [<dir>] [--to <version>] [--dry-run]` MUST update the SDK dependency and the pinned WIT version in the project, MUST print each edit as a `diff` record, MUST be a no-op with exit 0 when already current, and MUST fail with `upgrade.no-path` (exit 1) when the target version is older than the current one or crosses a major SDK version without a published migration note.

- **[DEV-913]** `ilium plugin check --deps` MUST report, offline from the lock file, the SDK version in use, the newest SDK version known to the installed CLI, and a `warn` record `deps.sdk-outdated` when the project's SDK minor is more than 2 minors (proposed, owner to confirm) behind the CLI.

- **[DEV-914]** Every offline command MUST behave identically on Linux, macOS and Windows: paths in records MUST use forward slashes, line endings of generated files MUST be LF, key files MUST be created with owner-only permissions (mode 0600, or an ACL granting only the current user on Windows), and the file watcher MUST fall back to polling every 500 ms (proposed, owner to confirm) when native events are unavailable.

- **[DEV-915]** The CLI MUST write standard output and files as UTF-8 regardless of locale, MUST NOT localise `code` or `rule` fields or JSONL keys, and MUST print text-mode messages in English only. Invalid UTF-8 in a source manifest MUST fail with `manifest.encoding` and the byte offset.

- **[DEV-916]** An interrupt (SIGINT, SIGTERM or Windows Ctrl+C) during `pack` or `sign` MUST leave no partial output: the target file MUST either be unchanged or fully written, no temp file named `*.tmp` may remain, and the command MUST exit 130 after printing `summary` and `result`.

- **[DEV-920]** `test` MUST accept `--timeout-ms <n>` (default 10,000 per case, max 600,000 (proposed, owner to confirm); larger MUST exit 2 `usage.out-of-range`) and `--filter <glob>` matching fixture names. A case over its timeout MUST get status `fail` with reason `timeout`, and a filter matching zero fixtures MUST exit 2 `test.no-match`.

- **[DEV-922]** `ilium plugin bench [--fixture <name>] [--iterations <n>]` MUST call the built component in the embedded test host with a real clock, default 100 iterations (proposed, owner to confirm) after 10 warm-up iterations, and print one `timing` record per export with `p50_ms`, `p99_ms` and `max_ms`; it MUST exit 3 when any export p99 exceeds its manifest `deadline_ms` and MUST label the record `synthetic: true`.

- **[DEV-931]** `dev --reset-data` MUST delete the development copy's data folder, cache folder and storage buckets before the first install and print one `warning` record naming them. Without it, data from the previous `dev` run MUST be kept across runs and across rebuilds.

- **[DEV-939]** `ilium plugin diff <old.iliumpkg> <new.iliumpkg>` MUST work offline and print `diff` records for: added, removed and changed grants, contributions, file list, version, and declared limits; it MUST exit 0 when identical and 3 when they differ, and it MUST NOT extract files to disk.

- **[DEV-940]** `pack --sbom` MUST write `dist/<id>-<version>.sbom.json` in CycloneDX 1.5 JSON listing every locked dependency with name, version and SHA-256 or registry checksum from the lock file, MUST be reproducible (DEV-099), and MUST print an `artifact` record of kind `sbom`.

- **[DEV-941]** `ilium plugin doctor --bundle <file>` MUST write a zip of the doctor records, CLI and host versions, the manifest, the lock file hash and the last 200 log lines (proposed, owner to confirm) of the plugin, MUST redact values of environment variables whose names contain `KEY`, `TOKEN` or `SECRET`, and MUST NOT include source files, key files or the data folder.
