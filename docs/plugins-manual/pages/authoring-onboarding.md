---
layout: page
title: Author onboarding
permalink: /pages/authoring-onboarding/
---

## Author onboarding

This page is the path a new plugin author walks: create a project, check the layout it produced, run the local development loop, run the tests, and find out why a plugin failed. It does not repeat the command reference. Each step links to the page that owns the rule, and the requirement IDs here cover only the order of steps and the behaviour an author relies on in the first days.

## You will need

- The Ilium plugin command line (`ilium plugin`), as set up in [Set up a toolchain](/pages/getting-started-toolchain/).
- The toolchain for your language: Rust with the WebAssembly target, or TypeScript with Bun. The exact versions are in [Set up a toolchain](/pages/getting-started-toolchain/).
- A running Ilium session in the project directory for the `dev` step. The `dev` command never starts one ([DEV-180]).
- About 30 minutes for the first pass through this page.

## The five steps

| Step | Command | Owner page | Done when |
|---|---|---|---|
| 1. Create | `ilium plugin new` | [Project templates](/pages/reference-project-templates/) | The folder holds the template files and passes `check` |
| 2. Verify | `check`, `lint`, `test`, `build` | [Plugin CLI](/pages/reference-plugin-cli/) | Each command exits with 0 |
| 3. Develop | `ilium plugin dev` | [Plugin CLI](/pages/reference-plugin-cli/#dev) | The dev copy runs and reloads after a save |
| 4. Test | `ilium plugin test` | [Testing plugins](/pages/testing/) and [Test harness reference](/pages/reference-test-harness/) | Every fixture passes |
| 5. Debug | log, trace and inspector | [Debugging](/pages/debugging/) and [Debugging tools](/pages/reference-debugging-tools/) | The trap or wrong output has a `file:line` and a reason |

## Step 1: create the project

A project is created from one template. The template chooses which sides exist. The example below creates a server-side Rust plugin for the id `example.hello`.

    ilium plugin new ~/src/hello-ilium --id example.hello --lang rust --template server

- **[ONB-001]** `plugin new` MUST create the common files listed in [Project templates](/pages/reference-project-templates/#common-files) and the source files of the chosen template, and nothing else outside `<dir>`.
- **[ONB-002]** The publisher of a new project MUST be the operating system user name unless `--publisher` is given, and the display name MUST be the last label of the id unless `--name` is given.

The layout a new author sees:

| Path | Who writes it | Purpose |
|---|---|---|
| `manifest.json` | Author | Id, sides, grants, and the help and settings entries of the plugin ([Plugin manifest](/pages/reference-manifest/)) |
| `Cargo.toml` or `package.json` | Author | Marks the language. The build reads it to choose the toolchain ([DEV-002]) |
| `Cargo.lock` or `bun.lock` | Toolchain | Pins the dependencies. `build --locked` needs it ([DEV-053]) |
| `src/` | Author | Source of each side |
| `help/overview.md` | Author | Help topic shown in the help screen |
| `tests/fixtures/*.json` | Author | Inputs for `test` |
| `tests/fixtures/*.expect.json` | Author | Expected results for the matching fixture |
| `tests/golden/*.txt` | `test --update` | Expected renders, for templates with a view |
| `dist/<side>.wasm`, `dist/build.json` | `build` | Built components and build record. Never edit them by hand |
| `.ilium/` | Ilium | Coverage output (`test --coverage`) and other tool output. Listed in the template `.gitignore` |

- **[ONB-003]** The author MUST NOT edit files under `dist/` by hand. `build` MUST be the only writer of `dist/<side>.wasm` and `dist/build.json` ([DEV-050], [DEV-051]), and `test --coverage` MUST write its output under `.ilium/coverage/`.

## Step 2: verify the fresh project

A fresh project passes four commands with zero warnings. Run them in this order, as the README of the template lists them.

    ilium plugin check
    ilium plugin lint --deny warnings
    ilium plugin test
    ilium plugin build

`test` needs the built component, so on a fresh clone `build` MUST run before `test` ([DEV-080]).

- **[ONB-004]** A fresh project MUST pass the four commands above with exit code 0 and no warning, in that order. The template guarantees this state ([DEV-045]). If a fresh project fails one of these commands, the author SHOULD treat it as a template or toolchain problem and report it, rather than edit the template files (proposed, owner to confirm).
- **[ONB-005]** `test` on a project with no built component for its side MUST fail with `test.component-missing` and exit with 1. The author MUST run `build` and rerun `test`.

### Language detection

- **[ONB-006]** A project with only `Cargo.toml` MUST build as Rust, and a project with only `package.json` MUST build as TypeScript. A project with both files MUST fail with `toolchain.language-ambiguous`, and a project with neither MUST fail with `toolchain.language-unknown` ([DEV-002]). The author MUST keep one language per project.

### Toolchain missing

- **[ONB-007]** When a toolchain is missing, `build` MUST stop with a `toolchain.*` code and exit with 4 ([DEV-012], [DEV-055]). The author installs the named tool and reruns the command. Running `doctor` lists every missing item with status `fail` and also exits with 4.

### Local checks that stay offline

- **[ONB-008]** `check` and `lint` MUST run without a network connection. `check --print-schema` MUST also run offline and print the manifest schema ([DEV-070], [DEV-075]).

## Step 3: the development loop

`dev` builds the plugin, installs it as a development copy, rebuilds after each save and reloads the running instance. Start the session first: open `ilium` in the project directory, so that the session named `default` of that directory is the one `dev` finds ([DEV-004]).

    cd ~/src/hello-ilium
    ilium plugin dev

The loop has these states. The arrows are the only transitions the author can cause or observe.

    Idle --save--> Queued --300 ms quiet--> Building
    Building --ok--> Reloading --> Running
    Building --error--> Running (old instance kept, errors printed)
    Running --Ctrl+C--> Stopped (exit 130)

A save during a build is queued, and one more build then runs for all queued saves ([DEV-186]).

- **[ONB-009]** `dev` MUST start a rebuild 300 ms after the last file change, and MUST ignore changes under `dist/`, `target/`, `node_modules/`, `.ilium/`, `.git/` and the paths in `.gitignore` ([DEV-185]).
- **[ONB-010]** On a successful rebuild, `dev` MUST reload the server side first and then each attached client side. Each reload MUST print one `reload` record. A failed rebuild MUST print the compiler errors, keep the running instance, and send no `deactivate` ([DEV-187]). The author fixes the error and saves again; no restart is needed.
- **[ONB-011]** The development copy MUST be installed as tier T3 with the `dev` badge and with a data folder of its own. A real install of the same id MUST not share that data folder ([DEV-181]). If a real install of the id is already present, `dev` MUST fail with `dev.id-installed` and exit with 1, unless `--replace-installed` is given ([DEV-183]).
- **[ONB-012]** `dev` MUST build with debug information by default, so that a trap names `file:line`. `--release` MUST omit it ([DEV-182]).
- **[ONB-013]** Ctrl+C in `dev` MUST deactivate the plugin with reason `disabled`, uninstall the development copy, keep its data folder unless `--purge-on-exit` is given, and exit with 130 ([DEV-189]).
- **[ONB-014]** Restore bytes above 1 MiB MUST be dropped on reload, and the `reload` record MUST show `restore_dropped` true with `restored_bytes` 0 ([DEV-188]). An author who sees this MUST treat the reload as having lost its restore state and MUST NOT rely on that state surviving a reload (proposed, owner to confirm: the page does not yet name the storage API an author should use for large state).

Worked example of one save. The author edits `src/lib.rs` at 10:02:00.000 and saves again at 10:02:00.200. `dev` queues both saves and starts one build at 10:02:00.500. That build passes. `dev` reloads the server side, prints one `reload` record with `restore_dropped` false, and the session shows the new behaviour. Had the build failed, the old instance would stay active and the compiler errors would appear in the `dev` output.

## Step 4: the test loop

A fixture is a JSON file of events and grants. An expectation file with the same base name, `<name>.expect.json`, states what the outcome must be. The full fixture and match-mode format is in [Test harness reference](/pages/reference-test-harness/).

    ilium plugin test
    ilium plugin test --fixture 'tests/fixtures/*.json' --seed 7

- **[ONB-015]** A fixture with no expectation file MUST pass and print the warning `test.no-expectation`. The author SHOULD add an expectation for every fixture before the first CI run (proposed, owner to confirm: the warning is advisory today, and this page sets the team expectation, not a rule of the host).
- **[ONB-016]** `test` MUST exit with 0 when every case passes, 3 when a case fails or traps, and 1 when a fixture cannot be read ([DEV-087]). A mismatch MUST print a `diff` record.
- **[ONB-017]** A package with two sides MUST be given `--side` for `test`, or the command fails with `usage.missing-argument` ([DEV-085]). A one-side project needs no flag.
- **[ONB-018]** `test --update` MUST rewrite only the expectation and golden files that differ and MUST print one `test.updated` warning for each file rewritten ([DEV-086]). The author reads the diff in version control before committing the rewrite.
- **[ONB-019]** `test` MUST NOT run the project's own unit tests. Those run with `cargo test` or `bun test` in native mode, and `test --coverage` is the only form of `test` that starts them ([DEV-089]).

## Step 5: when something fails

Start with the exit code of the command, then the record it printed. The table maps each code to the first place to look.

| Exit | Meaning for an author | First step |
|---|---|---|
| 0 | Done | None |
| 1 | Build, install or read failure | Read the `error` record and its code |
| 2 | Wrong flag or argument. No files were written | Run the command with `--help` |
| 3 | Findings, a failing case or a trap | Read the `diff`, `finding` or trap reason |
| 4 | Missing tool or failed `doctor` item | Install the named tool |
| 5 | Plugin, package or key not found | Check the id and the path |
| 6 | No session reachable | Start `ilium` in the project directory |
| 130 | Interrupted by Ctrl+C | Rerun the command |

- **[ONB-020]** An author who sees exit code 3 from `test` MUST read the first `diff` or trap record before changing the code, and MUST NOT update the expectation file to hide a trap ([DEV-084]). `test --update` is for an intended change of output only.
- **[ONB-021]** An author who sees a trap MUST use the `file:line` of the trap and the reason in its record. For a trap in a release build, which has no debug information, the author MUST rebuild with `dev` or `build --debug` before reading the position ([DEV-182]).

For the causes behind each code, and for the logs, traces and inspector views that show runtime state, read [Debugging](/pages/debugging/) and then [Debugging tools](/pages/reference-debugging-tools/). The failure modes of `dev` itself are in [Failure modes of the dev command](/pages/reference-plugin-cli-dev-failure-modes/).

## Decisions this page does not settle

- The command that launches a session for `dev` is `ilium` in the project directory. The page does not add a separate onboarding command, because none is defined in the command reference. Proposed, owner to confirm: keep it that way until a `doctor`-style first-run check is specified.
- CI pipelines should run `build --locked` so that a stale lockfile fails the build ([DEV-053]). The rule is stated in the command reference; the requirement to use it in CI is proposed, owner to confirm, and belongs in [CI recipes](/pages/guide-ci-recipes/).

## Edge cases

- A project directory that already holds files: `new` fails with `new.dir-not-empty` unless `--force` is given, and `--force` never deletes a file ([DEV-041]).
- A plugin id that starts with `ilium.`: `new` fails with `new.id-reserved` unless `--first-party` is given ([DEV-042]).
- `dev` in a project whose id is already installed as a real plugin: see [ONB-011].
- A save while `dev` is building: the save is queued, not dropped ([DEV-186]).

## See also

- [Your first plugin in five minutes](/pages/getting-started-first-plugin/)
- [Set up a toolchain](/pages/getting-started-toolchain/)
- [Project templates](/pages/reference-project-templates/)
- [Plugin command line](/pages/reference-plugin-cli/)
- [Testing plugins](/pages/testing/)
- [Test harness reference](/pages/reference-test-harness/)
- [Debugging](/pages/debugging/)
