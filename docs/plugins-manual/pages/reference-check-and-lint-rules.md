---
layout: page
title: Check and lint rules
permalink: /pages/reference-check-and-lint-rules/
---

## Check and lint rules

`ilium plugin check` finds mistakes that would stop an install or trap at run time. `ilium plugin lint` finds mistakes and weak spots that do not stop anything but make a plugin worse for its users. Both read files only and run offline. This page defines the rules, their codes, their severity, the input each one reads, the record each one prints, how to fix each one and how to suppress a lint rule.

Every rule has a stable code. A test or a CI job cites the code. The codes of `check` are the codes the install path uses, so a package that passes `check` passes the install checks (it can still fail trust or the review dialog, which need a person). Command flags and exit codes are in [Plugin command-line reference](/pages/reference-plugin-cli/).

Terms:

- **Finding**: one problem reported by a rule.
- **Severity**: `error` stops a build gate, `warn` is advisory, `info` is a hint.
- **Key path**: a JSON Pointer into `manifest.json`, for example `/capabilities/client/2`. A finding about the manifest carries one.

## You will need

- A project folder with `manifest.json`. For the component rules, a built `dist/` folder.
- No toolchain for `check` on a package file. A built `dist/` for the rules that read components.
- Background: [Packages and manifests](/pages/packages/), [Manifest reference](/pages/reference-manifest/) and [Capabilities](/pages/reference-capabilities/).

## What check does

- **[DEV-200]** `check` MUST run exactly the install checks of [Packages and manifests](/pages/packages/), in the same order, with the same codes, and MUST NOT run any plugin code. It reports the install codes `manifest.*`. The install path uses no `package.*` codes, so `check` has none either. The rules of DEV-201 add further codes.
- **[DEV-201]** `check` MUST add the component rules below, which report `component.*` and `signature.*` codes and two further `manifest.*` codes. They need built components and are skipped, with one `component.missing` finding per absent side, when `dist/` has no component for a declared side.
- **[DEV-202]** Every `check` finding is an `error`, except `signature.absent`, `check.stale-build` and `component.size-warning` from 8 to 32 MiB, which are `warn` (`component.size-warning` is an `error` above 32 MiB, DEV-206), and `signature.fingerprint`, which is `info`. The severity column of the rule tables is normative.
- **[DEV-203]** `check` MUST NOT check trust. Whether a signing key is trusted depends on the user's machine. `check` verifies that a signature is valid for the manifest bytes and states the fingerprint in an `info` finding named `signature.fingerprint`.

### Component rules

| Code | Severity | Reads | Fires when | Fix |
|---|---|---|---|---|
| `component.missing` | error | `dist/` | A declared side has no component | Run `ilium plugin build` |
| `component.invalid` | error | the component | The file is not a valid WebAssembly component | Rebuild |
| `component.world-mismatch` | error | component, manifest | The world of the component differs from `world` in the manifest | Fix the world in the manifest or the entry |
| `manifest.api-version` | error | component | The `ilium:api-version` section is absent, malformed or differs from `api` | Rebuild with the current SDK |
| `manifest.exports-mismatch` | error | component, manifest | The exports differ from the required set plus the listed optional exports | Fix the `exports` list |
| `component.import-ungranted` | error | component, manifest | The component imports a host function whose grant the side does not request | Add the grant, or remove the call |
| `component.import-unknown` | error | component | The component imports an interface that is not in the contract | Use the SDK imports only |
| `component.size-warning` | warn | component | A component is larger than 8 MiB | Strip dependencies |
| `signature.absent` | warn | package | A `.iliumpkg` has no `signature` entry | Sign it, or accept tier T3 |
| `check.stale-build` | warn | `dist/build.json`, sources | The input hash in `dist/build.json` differs from the current sources | Run `ilium plugin build` |
| `signature.fingerprint` | info | package | A valid signature exists | none |

- **[DEV-204]** `component.import-ungranted` MUST fire for each imported function whose required grant is missing from the `capabilities` list of the side that the component serves. A function of an always-granted interface MUST NOT fire. This is the static form of the trap that [Debugging](/pages/debugging/) describes for a call to an import that the manifest never requested.
- **[DEV-205]** `component.world-mismatch` and `manifest.api-version` MUST be reported from the component's own embedded data, never from the file name.
- **[DEV-206]** `component.size-warning` MUST fire above 8 MiB. Above 32 MiB the finding MUST be an `error` with the same code.

A finding record:

```json
{"type":"check","rule":"component.import-ungranted","severity":"error","message":"server.wasm imports ilium:host/fs.read-all but the server side does not request fs.read","file":"dist/server.wasm","key_path":"/capabilities/server"}
```

## What lint does

- **[DEV-210]** `lint` MUST run only `lint.*` rules. It reads the manifest, the files listed by the manifest, `dist/` when present, and the fixtures in `tests/fixtures/` when present.
- **[DEV-211]** A rule that needs an input that is absent MUST be skipped without a finding. `lint` MUST print one `info` finding named `lint.skipped` for each rule it skipped, with the missing input in the message.
- **[DEV-212]** Rules that render a view or replay a fixture MUST do so in the embedded test host and MUST NOT contact a session.

### Lint rule catalogue

| Code | Severity | Reads | Fires when | Fix |
|---|---|---|---|---|
| `lint.grant-unused` | warn | manifest, component | A requested grant has no import or contribution that needs it | Remove the grant |
| `lint.widget-missing-key` | warn | rendered views | An interactive widget has no `key` | Give every interactive widget a `key` |
| `lint.render-trap` | error | rendered views | A declared view traps in the test host with an empty fixture | Fix the render |
| `lint.help-topic-missing` | warn | manifest | The plugin contributes commands, pane kinds, panels or settings pages and `contributes.help` is empty | Add a help topic |
| `lint.settings-help-missing` | warn | manifest | A settings row has no `help` id | Add a topic id, see [guide](/pages/guide-plugin-documentation/) |
| `lint.limit-above-ceiling` | warn | manifest | A `limits` value is above the ceiling of the tier, so the host clamps it | Lower the value |
| `lint.debug-info-in-release` | warn | `dist/`, `build.json` | A release build carries DWARF or a source map | Rebuild with `--release` |
| `lint.package-size-near-cap` | warn | files | The files sum to more than 100 MiB | Reduce assets |
| `lint.unstable-feature` | warn | manifest | `unstable` lists a name | Expect change before 1.0 |
| `lint.dedup-key-charset` | warn | replayed fixtures | A submission carries a dedup key with a character outside `[A-Za-z0-9._:/-]` or longer than 128 characters | Fix the key |
| `lint.icon-name-unknown` | warn | manifest | An `icon` is not a Tabler icon name that the host ships | Choose a listed name |
| `lint.extension-schema` | warn | manifest | An extension point has a schema outside the allowed subset, no `version`, or `max_handlers` of 0 | Fix the declaration |
| `lint.description-missing` | warn | manifest | `description` is empty or longer than 280 characters (the manifest limit of 500 characters is a separate hard error) | Write one short paragraph |
| `lint.license-missing` | info | manifest | `license` is empty | Add an SPDX expression |
| `lint.grant-unexplained` | warn | docs | A requested grant name does not appear in `README.md` | Explain the grant in the README |
| `lint.help-title-mismatch` | warn | docs | The first heading of a help topic differs from its manifest `title` | Make them equal |
| `lint.help-link-broken` | warn | docs | A `help:<id>` link names a topic the plugin does not ship | Fix the id or add the topic |
| `lint.help-topic-too-large` | warn | docs | A help topic is over 20 KiB | Split the topic |
| `lint.extension-point-undocumented` | warn | docs | A declared extension point id is in neither `README.md` nor `docs/EXTENDING.md` | Document the point |
| `lint.license-file-missing` | info | docs | `license` is set and no `LICENSE` file exists | Add the license text |
| `lint.activation-startup` | warn | manifest | `activation` lists `startup` and the plugin has no event handler that needs it | Use lazy activation |
| `lint.high-risk-grants` | info | manifest | A high-risk grant is requested | Confirm it is needed |
| `lint.src-in-package` | info | files | The package holds `src/**` entries, which reviewers can read but the host never loads | Pack without `--include-src` unless wanted |
| `lint.contribution-near-cap` | info | manifest | A contribution list is at 90 percent of its cap | Plan for the cap |
| `lint.notice-missing` | warn | project | The package bundles third-party code (`bundled-dependencies` in the manifest) and the project has no `THIRD-PARTY-NOTICES` or `NOTICE` file | Add the notice file |
| `lint.notice-incomplete` | warn | project | A bundled dependency named in the manifest has no entry in the notice file | Add its licence text |
| `lint.readme-missing` | info | project | No `README.md` in the project folder | Add one |
| `lint.changelog-missing` | info | project | No `CHANGELOG.md` in the project folder | Add one |
| `lint.suppression-reason-missing` | error | `lint.toml` | A suppression has no reason | Add a reason |
| `lint.suppression-unknown` | error | `lint.toml` | A suppression names an unknown rule | Fix the rule id |
| `lint.suppression-unused` | warn | `lint.toml` | A suppression matched no finding | Remove it |
| `lint.skipped` | info | none | A rule was skipped for a missing input | none |

The check `manifest.when-key` already catches an unknown `when` key at install time, so lint has no second rule for it.

- **[DEV-213]** `lint.grant-unused` MUST consider a grant used when a component import needs it, a contribution needs it, or an `extensions` entry needs it. It MUST skip a grant that is named only in a feature that loads later through `requires`. It MUST read the component imports, so it is skipped when `dist/` is absent.
- **[DEV-214]** `lint.widget-missing-key` MUST render each declared view with an empty fixture at 40 columns by 12 rows and report each interactive node without a `key`. Interactive widgets are those with actions, such as buttons, inputs, lists and toggles.
- **[DEV-215]** `lint.limit-above-ceiling` MUST use the ceilings of tier T2 and T3 from [Errors and limits](/pages/reference-errors-and-limits/) for any package that is not a bundled T1 plugin.
- **[DEV-216]** `lint.package-size-near-cap` MUST fire at 100 MiB total and MUST match the `pack.size-cap` warning, so `pack` and `lint` agree.
- **[DEV-217]** `lint.icon-name-unknown` MUST compare against the Tabler icon names built into the host version, and the finding MUST name the closest known name when one is within an edit distance of 2.
- **[DEV-218]** `lint.dedup-key-charset` MUST replay each fixture of `tests/fixtures/` through the test host and inspect the submissions. Without fixtures it is skipped.
- **[DEV-219]** `lint.high-risk-grants` MUST list the grants by name in `message`, so a reviewer sees the risk summary at a glance.
- **[DEV-220]** Rules with scope `docs` MUST read `README.md`, `CHANGELOG.md`, `LICENSE`, `docs/` and the help files named by the manifest. A missing file is a finding of the rule for that file, not a crash. The rules are defined in [Plugin documentation guide](/pages/guide-plugin-documentation/).

A lint finding record:

```json
{"type":"lint","rule":"lint.settings-help-missing","severity":"warn","message":"settings row 'refresh_seconds' has no help id","file":"manifest.json","key_path":"/contributes/settings/0/schema/properties/refresh_seconds","fix":"add \"help\": \"<topic id from contributes.help>\" to the row","suppressed":false}
```

## Severity, deny and exit codes

- **[DEV-225]** Plain `lint` MUST exit with 0 when only `warn` and `info` findings exist, and with 3 when an `error` finding exists.
- **[DEV-226]** `lint --deny warnings` MUST exit with 3 when any unsuppressed `warn` or `error` finding exists. `info` findings never affect the exit code.
- **[DEV-227]** `check` MUST exit with 3 when any `error` finding exists, and `check` has no deny flag, because its three `warn` findings (`signature.absent` for an unsigned package, `component.size-warning` for a component from 8 to 32 MiB and `check.stale-build`) are normal during development.

## Suppression

A project suppresses a lint rule in a file named `lint.toml` in the project folder. Suppression is a per-project decision that a reviewer can read, so it carries a reason.

```toml
[[suppress]]
rule = "lint.grant-unused"
key_path = "/capabilities/client/2"
reason = "the optional exporter that needs this grant loads only after the first run"

[[suppress]]
rule = "lint.activation-startup"
reason = "scans the project once at startup to fill the tree decorations"
```

- **[DEV-230]** `lint.toml` MUST be the only place that suppresses a lint rule. The manifest MUST NOT carry a suppression key, because unknown manifest keys fail the install.
- **[DEV-231]** A `[[suppress]]` table MUST have `rule` and `reason`. `key_path` is optional and, when given, limits the suppression to findings with the same key path. A table without `reason` MUST produce `lint.suppression-reason-missing`.
- **[DEV-232]** `check` rules MUST NOT be suppressible. A `rule` that names a `check.*`, `manifest.*`, `component.*` or `signature.*` code MUST produce `lint.suppression-unknown`.
- **[DEV-233]** A suppressed finding MUST still be printed, with `suppressed` true, so a reviewer sees it. Suppressed findings do not count toward the exit code.

## Worked example

A TypeScript plugin requests `fs.read:project` but never calls a file function. The text form of `lint` prints:

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin lint
<span class="t-warn">warn</span> lint.grant-unused
     /capabilities/server/1 fs.read:project
     fix: remove the grant
<span class="t-warn">warn</span> lint.settings-help-missing
     row refresh_seconds has no help id
<span class="t-dim">info</span> lint.readme-missing
2 warnings, 0 errors, 1 hint
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The text form of lint. The exit code is 0, or 3 with --deny warnings.</figcaption>
</figure>

## Edge cases

- **Component older than the manifest.** A rule that reads the component can report a finding about a grant that was removed from the manifest after the build. `lint` and `check` MUST first compare `dist/build.json` with the sources and report a finding with rule `check.stale-build` and severity `warn` when they differ, and still run. `pack` fails on the same condition with `pack.stale-build`.
- **Two sides.** Every rule runs per side. A finding carries the side in its `file` path, such as `dist/client.wasm`.
- **Empty project.** A project with no `contributes` key passes every rule except `lint.description-missing` if the description is empty.
- **A TypeScript project with no `dist/`.** `check` reports `component.missing` for each side. `lint` skips the component rules and prints `lint.skipped`.

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Packages and manifests](/pages/packages/)
- [Manifest reference](/pages/reference-manifest/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Plugin documentation guide](/pages/guide-plugin-documentation/)

API checked against `ilium:plugin@0.1.0`.
