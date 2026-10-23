---
layout: page
title: Plugins without the SDK
permalink: /pages/guide-plugins-without-the-sdk/
---

## Plugins without the SDK

This guide is for an author who writes a plugin in a language that has no Ilium SDK, such as Go, C or Python, or who wants to build a component straight from the WIT files. It states what the host requires of the component, what the Rust and TypeScript SDKs write for you and you must now write yourself, what `ilium plugin build` does with an unknown language, and how to check the result. Two SDKs exist: [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/).

Requirement ids such as `DEV-302` are in [Requirements: developer tooling](/pages/reference-requirements-dev-tooling/). Ids such as `PKG-139` are in [Requirements: manifest and package](/pages/reference-requirements-manifest-package/).

### You will need

- The contract listing [WIT reference](/pages/reference-wit/) (`ilium:plugin@0.1.0`). It is the only source of the types and function names.
- A toolchain that turns WIT into a WebAssembly component: `wit-bindgen` for Go (TinyGo) or C, or `componentize-py` for Python.
- The Ilium CLI, for `ilium plugin test --conformance` ([Conformance suite](/pages/reference-conformance-suite/)).

## Support status

| Part | Status |
|---|---|
| The contract (WIT files, error record, limits, `ilium:api-version` section) | Supported. A component that meets it runs like an SDK plugin. |
| The Rust and TypeScript SDKs and their toolchains in `ilium plugin build` | Supported. |
| A language toolchain for Go, C, Python or any other language | Community-supported. The project does not test it, does not fix its bugs, and does not promise that a version of it keeps producing valid components. |

The host cannot tell what language made a component. It sees a WebAssembly component, its exports and its custom section. It applies the same checks, limits, deadlines and traps to every component ([Runtime model](/pages/reference-host-runtime/)).

## What the host requires of the component

The install check is the same for every component. It is defined in [Package format](/pages/reference-package-format/#the-validation-order). The rows that matter for a hand-built component:

| Id | Rule | Failure code |
|---|---|---|
| [PKG-141] | The file MUST be a component (layer 1 in the preamble), not a core module. `wit-bindgen` and `componentize-py` produce a core module first; wrap it as a component before you ship. | `package.component-invalid` |
| [PKG-142] | The component MUST validate against the world named in the manifest. Exports that the manifest does not list, or the reverse, are refused. | `package.component-invalid`, `manifest.exports-mismatch` |
| [PKG-138] | The component MUST contain exactly one custom section named `ilium:api-version`. | `package.api-section-missing`, `package.api-section-repeated` |
| [PKG-139] | The payload MUST be 6 bytes: major, minor and patch as three big-endian `u16`. For `ilium:plugin@0.1.0` it is `00 00 00 01 00 00`. | `package.api-section-invalid` |
| [PKG-140] | The three integers MUST equal the version in the manifest `api` key. | `manifest.api-version` |

The manifest `api` value is the string `ilium:plugin@0.1.0` ([Manifest reference](/pages/reference-manifest/)). It is also the value the host puts in `activation.api`, and the host compares versions before linking (ABI-021).

### The section the SDK writes

The SDK build writes the section at stage 5 of the build ([DEV-302](/pages/reference-plugin-cli/#the-build-pipeline)). Without the SDK you write it yourself, after the component exists, with any tool that adds a custom section to a component, such as `wasm-tools`. Add it once. Running the step twice on the same file makes two sections and the install fails with `package.api-section-repeated`. Check the result by reading the file back: the section name is exactly `ilium:api-version`, the payload is six bytes.

### Export names

A plugin component targets one of the worlds in [WIT reference](/pages/reference-wit/#9-worlds). The world `base` exports two interfaces and the others add one each:

| World | Exports |
|---|---|
| `base` | `ilium:exports/lifecycle@0.1.0` (`activate`, `deactivate`) and `ilium:exports/handler@0.1.0` (`on-events`) |
| `with-detector` | adds `ilium:exports/detector@0.1.0` (`identify`, `classify`) |
| `with-jobs` | adds `ilium:exports/job-runner@0.1.0` (`run`) |
| `with-service` | adds `ilium:exports/service-provider@0.1.0` |

The component must export exactly the interfaces its manifest lists (PKG-142). A generator names the functions after the WIT names, in the style of its language: `on-events` becomes `OnEvents` in Go and `on_events` in C and Python. The names in the component binary are always the WIT names.

## What the SDK does that you must now do

| Duty | What the SDK does | What you do |
|---|---|---|
| API section | Writes `ilium:api-version` at build | Write it once, see above |
| Error record | Turns `Result<T, PluginError>` or a thrown `PluginError` into `result<T, plugin-error>` | Return the WIT record: `code` (one of the 15 `error-code` values, in the order of the enum), `message` (at most 256 characters, plain text) and `retry-after-ms`. See [Errors and limits](/pages/reference-errors-and-limits/#the-plugin-error-record). A code outside the enum cannot be encoded, so the generator rejects it. |
| Restore bytes | Returns the bytes you give it from `deactivate` | Return `option<list<u8>>`. The host stores the bytes without reading them, drops anything over 1 MiB, and hands them back as `activation.restored` ([Persistence and restart](/pages/reference-persistence-and-restart/#restore-bytes), LIF-152). The layout is yours. Start it with a format byte and fall back to defaults when the first byte is unknown (AUT-206). |
| Limits | Rejects some bad calls before they leave the plugin | Keep to the caps yourself: at most 256 events in and 256 submissions out per `on-events` call, deadlines of 20 ms for `on-events`, 10 ms for `render`, 2000 ms for `activate` and 500 ms for `deactivate` ([Performance](/pages/performance/)). A breach traps the instance exactly as it does for an SDK plugin. |
| Async calls | Wraps a `request-id` and its `take-*` partner in a future | Call the `request-id` function, wait for the completion event in `on-events`, then call the `take-*` function. The rules are in the WIT comments of each function. |
| Pure `render` | Gives you the view target and area | `render` may not call a host function that submits intents. Return a `ui-tree`. |

Debug aid: a plugin that traps shows `at <function>` in the trap message only when the component keeps the names of its exports. Keep them in release builds (DEV-303).

## What `ilium plugin build` does with another language

`ilium plugin build` detects the language from the project files ([DEV-002](/pages/reference-plugin-cli/#build)). A `Cargo.toml` means Rust and a `package.json` means TypeScript. A project with neither fails with `toolchain.language-unknown`, exit 4, and nothing is written. A project with both fails with `toolchain.language-ambiguous`. There is no flag that selects a third toolchain, and `build` never runs `wit-bindgen`, `componentize-py` or any other tool you name.

Build the component with your own toolchain, outside `ilium plugin build`. Two points follow from the CLI reference:

- `ilium plugin pack` reads `dist/build.json` and refuses a missing or stale `dist/` with `pack.not-built` or `pack.stale-build` (DEV-095). The `build.json` record is written only by `build`.
- The CLI reference defines no `build --component` flag and no `pack --prebuilt` flag. Packing an externally built component is therefore not a supported CLI path in this contract version. Until the CLI documents one, treat a non-SDK plugin as testable but not packable with the project tools.

What you can do today is run the component through the same harness the SDK plugins use. `ilium plugin test --component <file>` loads the file you name instead of `dist/<side>.wasm` and runs the fixtures in component mode ([Plugin command-line reference](/pages/reference-plugin-cli/#test), DEV-080). The component is checked with the real linker, limits and deadlines.

## The acceptance check

`ilium plugin test --conformance` is the acceptance check for a plugin in any language. It does not look at the language, the toolchain or the SDK. It needs the component at `dist/<side>.wasm` (subject `plugin`), so copy the externally built file there first. It loads the component, drives it with the fixed cases of [Conformance suite](/pages/reference-conformance-suite/) and prints one `conformance` record per case and a `summary`. Any `fail` makes the exit code 3 (DEV-602). A pass means the component keeps to the contract limits. A case that needs a grant the manifest does not request is skipped with the reason `grant-not-requested` (DEV-600).

The subjects `sdk` and `host` are refused in a normal install with `usage.subject-unavailable` (DEV-601). They are for the Ilium source tree. A host author who wants to claim conformance uses the published corpus instead: each case has an input, an expected result and a matrix row, and a host may claim a version only when every case of that version passes (ABI-073 to ABI-075, [Host conformance](/pages/reference-host-conformance/)).

The suite does not check the look of a view, the speed on a given machine, or the quality of the language toolchain ([The edge of the suite](/pages/reference-conformance-suite/#the-edge-of-the-suite)). For parity of outcomes between the Rust and TypeScript SDKs there are separate cases (DEV-927). A third language has no such parity promise.

## Failure modes

| Symptom | Cause | Fix |
|---|---|---|
| `toolchain.language-unknown` | The project folder has neither `Cargo.toml` nor `package.json` | Build with your own toolchain, or keep a `Cargo.toml` or `package.json` only if you really use that SDK |
| `package.component-invalid` | A core module, or a component that does not fit the world | Wrap the module as a component against the world in the manifest |
| `manifest.exports-mismatch` | The component exports more or fewer interfaces than the manifest lists | Make the manifest and the component list the same interfaces |
| `package.api-section-missing` | The section was not added | Add `ilium:api-version` once |
| `package.api-section-repeated` | The step ran twice | Remove one section |
| `package.api-section-invalid` | The payload is not 6 bytes | Write three big-endian `u16` |
| `manifest.api-version` | The three numbers differ from the manifest `api` | Make them equal to `ilium:plugin@0.1.0` |
| A trap with `restore` warnings | Restore bytes over 1 MiB, or a layout the next version cannot read | Keep the bytes small and start them with a format byte |

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Conformance suite](/pages/reference-conformance-suite/)
- [WIT reference](/pages/reference-wit/)
- [Package format](/pages/reference-package-format/)
