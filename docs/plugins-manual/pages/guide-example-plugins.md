---
layout: page
title: Example plugin catalogue
permalink: /pages/guide-example-plugins/
---

## Example plugin catalogue

The Ilium project publishes one official repository of example plugins. Each example is a complete project that you can clone, build and run. The tutorials show code inside prose, and the templates give empty skeletons. An example is the third kind of help: a working plugin that the project tests on every release, so you can trust that it shows the real contract.

Terms:

- **Example**: one plugin project in the examples repository, with its own folder, manifest, tests and README.
- **Side**: `server`, `client`, `both` or `scene`, as in [Project templates](/pages/reference-project-templates/).
- **Capability family**: a group of related capabilities in [Capabilities](/pages/capabilities/), for example the `fs-*` family or the `agent-*` family.
- **Supported host minor**: a host contract version that the current release supports, as listed in [Host conformance](/pages/reference-host-conformance/).
- **Manual snapshot**: the release of this manual that matches one Ilium release.

## You will need

- The `ilium` binary and a toolchain, as in [Set up a toolchain](/pages/getting-started-toolchain/).
- `git`, if you clone the repository instead of using `new`.
- Background reading: [Project templates](/pages/reference-project-templates/) and [CI recipes](/pages/guide-ci-recipes/).

## What the repository holds

The repository has one folder per example, named after the example. Each folder is a normal plugin project. It is not a workspace of examples, so you can copy one folder and it builds alone.

- Every capability family has at least one example. Every side has at least one example. An example may cover several families.
- Each example has a Rust version and a TypeScript version in two sub-folders, `rust/` and `typescript/`. Both versions pass the same fixtures, so you can compare the languages.
- Each example has a `README.md` that names the capabilities it requests, the tutorial it pairs with, and the lines to read first.
- The repository root has a table file, `examples.json`, with one record per example. The table below is generated from it. The record fields are `name`, `id`, `side`, `families`, `tutorial` and `sdk`.

## The catalogue

The table lists the examples of the current manual snapshot. The `name` column is the value you pass after `example:`. The tutorial column names the page that explains the same plugin step by step.

| Name | Plugin id | Side | Capability families | What it demonstrates | Pairs with |
|---|---|---|---|---|---|
| `agent-badge` | `example.agent-badge` | client | `agent-read`, `ui` | Reads the state of each agent and draws a badge on its pane title | [Agent badge](/pages/tutorial-01-agent-badge/) |
| `command-settings` | `example.command-settings` | both | `commands`, `settings` | A command, a keybinding and a settings group that the command reads | [Command, keybinding and settings](/pages/tutorial-02-command-and-settings/) |
| `table-panel` | `example.table-panel` | client | `ui` | A panel that shows a table and updates it on a timer | [A panel with a table](/pages/tutorial-03-panel/) |
| `finished-notice` | `example.finished-notice` | client | `agent-read`, `notify` | A notice and a toolbar button when an agent finishes | [Finished-agent notice](/pages/tutorial-04-finished-notice/) |
| `todo-scanner` | `example.todo-scanner` | both | `fs-read`, `fs-watch`, `jobs` | A scan of project files in a background job, with a server side that keeps the result | [TODO scanner](/pages/tutorial-05-todo-scanner/) |
| `notes-pane` | `example.notes-pane` | client | `ui`, `storage`, `dnd` | A pane kind with drag and drop and saved text | [Notes pane](/pages/tutorial-06-notes-pane/) |
| `ambient-scene` | `example.ambient-scene` | scene | `scene` | A background scene that follows the shared palette | [Ambient scene](/pages/tutorial-07-ambient-scene/) |
| `hello-cli` | `example.hello-cli` | server | `cli-contribute` | A subcommand of `ilium` with arguments and exit codes | [Your first CLI subcommand](/pages/tutorial-12-hello-cli/) |
| `counter-storage` | `example.counter-storage` | both | `storage` | A counter that survives a restart | [Remember a counter in storage](/pages/tutorial-16-remember-a-counter/) |
| `quiet-hours` | `example.quiet-hours` | server | `hooks` | A hook that stops a core action in a set time window | [Stop core actions with hooks](/pages/tutorial-19-stop-core-actions-with-hooks/) |
| `link-preview` | `example.link-preview` | both | `extensions-provide`, `extensions-contribute` | An extension point that another plugin fills | [Offer an extension point](/pages/tutorial-18-add-an-extension-point/) |
| `review-swarm` | `example.review-swarm` | server | `agent-control`, `jobs` | A plugin that starts and watches several agents | [Drive the fleet](/pages/tutorial-23-review-swarm/) |

- **[DEV-1480]** The examples repository MUST contain at least one example for every capability family in [Capabilities](/pages/capabilities/) and at least one example for each of the sides `server`, `client`, `both` and `scene`. A release MUST NOT be published while a family or a side has no example. Test: read the capability list and `examples.json`, then assert that each family and each side appears in at least one record.
- **[DEV-1481]** Every record of `examples.json` MUST have a folder with the same `name`, and every example folder MUST have a record. The `tutorial` field MUST name a page that exists in the manual snapshot, or be `null`. Test: compare the folder list, the record list and the manual page list.

## The rule that keeps examples working

An example that no longer builds teaches the wrong thing. The project therefore tests every example in its CI on every change to the repository and on every release candidate of Ilium.

- **[DEV-1482]** For every example, in every language, the project CI MUST run `ilium plugin check`, `ilium plugin lint --deny warnings`, `ilium plugin test` and `ilium plugin test --conformance`, and each command MUST exit 0. The CI MUST run them against every supported host minor, using `--host-contract <version>` for the oldest and the newest version that the manifest `requires` range admits, as [DEV-933] describes. A failure in any cell of this matrix MUST block the merge and the release. Test: the CI configuration lists the four commands, both languages and every supported minor, and a deliberately broken example turns the run red.
- **[DEV-1483]** A new supported host minor MUST NOT be released until every example passes the matrix for it. If an example needs a change, the change MUST land in the examples repository before the release. Test: add a host minor to the matrix in a test build and assert that the release job refuses while one example fails.
- A fresh clone of an example MUST also satisfy [DEV-270]: with no edits, it builds and passes `check`, `lint` and `test` on a machine where `ilium plugin doctor` reports no `fail`.

### Pinned SDK version

Every example pins the exact SDK version of the manual snapshot it belongs to. The Rust version pins it in `Cargo.toml`, and the TypeScript version pins it in `package.json` with no range operator. The `sdk` field of the record repeats the pin.

- **[DEV-1484]** Each example MUST pin the SDK version of the manual snapshot that lists it, and the repository MUST keep one branch or tag per manual snapshot. The page you read and the example you clone from the matching tag therefore agree. A pin that differs from the `sdk` field in `examples.json` MUST fail the repository CI. Test: change one pin and assert that the CI fails with the example name in the message.

When a new SDK version appears, the maintainers update the pins on the main branch, run the matrix, and tag the result. Old tags are kept, so an author who stays on an older SDK can still clone the matching examples.

## Licence

- **[DEV-1485]** The examples repository MUST be licensed under the MIT licence, the same licence as the Ilium source repository. Every example folder MUST contain a copy of the licence text and MUST set the manifest `license` field to `MIT`. You may copy, change and publish an example, including under your own licence, if you keep the copyright notice in the files you copied. Test: assert that each example has the licence file and the manifest field `license = "MIT"`.

An example uses the id prefix `example.`. A package that keeps this prefix is refused by the registry. Change the id before you publish, as [Package and share](/pages/tutorial-09-package-and-share/) explains.

## Copy an example

There are two ways. Both give you a project that builds with no further steps.

1. Use `new`: `ilium plugin new my-plugin --id acme.my-plugin --lang rust --template example:agent-badge`. The command copies the files of the example from the tag that matches the installed `ilium` version, replaces the placeholders, and sets the id, name and publisher. It needs network access to the repository, or a local clone given with `--examples-dir <path>`.
2. Clone: `git clone --branch <manual-snapshot-tag> <repository-url>` and copy the folder of one example. Change `id`, `name` and `publisher` in the manifest by hand.

- **[DEV-1486]** `ilium plugin new --template example:<name>` (proposed, owner to confirm) MUST use the same flags and the same rules as other templates in [Project templates](/pages/reference-project-templates/), including `new.dir-not-empty`, `new.id-invalid` and `new.id-reserved`. An `example.` id MUST be refused, because the registry reserves it for the examples. An unknown `<name>` MUST fail with `new.template-unknown` before any file is written. If the repository cannot be reached and no `--examples-dir` is given, `new` MUST fail with `new.examples-unavailable` and exit 2, and MUST write no file. Test: run each of the three failures and assert the code, exit status and an empty target folder.
- **[DEV-1487]** The output of `new --template example:<name>` MUST be byte-identical to the example folder at the matching tag, except for the placeholders `@@id@@`, `@@name@@` and `@@publisher@@`, the manifest `id`, `name` and `publisher` values, and the removal of the `examples.json` record. Test: copy an example, replace the three values, and compare the file trees.

## When something goes wrong

| Symptom | Cause | What to do |
|---|---|---|
| `new.template-unknown` for `example:foo` | The name is not in the `name` column of the current snapshot | Compare with the table above. The names change only with a manual snapshot. |
| `new.examples-unavailable` | No network, or the tag for your `ilium` version does not exist | Use `--examples-dir` with a clone, or install the `ilium` version whose tag you cloned. |
| `new.id-reserved` | The id starts with `example.` | Choose your own id, for example `acme.agent-badge`. |
| An example fails on your machine only | A toolchain older than the one in [Set up a toolchain](/pages/getting-started-toolchain/) | Run `ilium plugin doctor` and fix each `fail` line. Examples are tested on supported toolchains only. |
| An example fails on a host minor in your setup | You run a host older than the examples' `requires` range | Clone the examples from the tag of your SDK version. |

## See also

- [Project templates](/pages/reference-project-templates/)
- [CI recipes](/pages/guide-ci-recipes/)
- [Conformance suite](/pages/reference-conformance-suite/)
- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Package and share](/pages/tutorial-09-package-and-share/)

API checked against `ilium:plugin@0.1.0`.
