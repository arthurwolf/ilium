---
layout: page
title: Versioning
permalink: /pages/versioning/
---

## Versioning

The plugin contract has a version. The host and each plugin declare the version they use, and the host decides at install whether it can load a package. This page explains the version identity of a package, the markers on each item of the contract, the unstable features a plugin must opt into, the frozen snapshots the host keeps, the window of versions it loads, and the rule that governs changes after version 0.1.0.

## You will need

- A package whose `api` value matches the SDK it was built with, for example `ilium:plugin@0.1.0`.
- [Packages and manifests](/pages/packages/) for the manifest layout and the install checks.
- For an unstable feature, either a T2 or T3 package installed in development mode with `ilium plugin dev`, or a T1 bundled plugin.

## The version identity

The `api` key in the manifest names the contract version. Each component also carries an `ilium:api-version` custom section with the same version as three big-endian `u16` values: major, minor and patch. The host compares the two at install. They must match exactly, patch number included. A mismatch fails with `manifest.api-version` before any code loads.

```json
{
  "id": "com.example.notes-caller",
  "name": "Notes caller",
  "version": "0.1.0",
  "publisher": "Example Corp",
  "api": "ilium:plugin@0.1.0",
  "unstable": ["plugin-services"],
  "requires": { "services": ["com.example.titles/suggest@0.1.0"] }
}
```

The example shows only the keys that this page discusses. A real manifest also carries the keys described in [Packages and manifests](/pages/packages/).

The `version` key is the version of the plugin itself. The `api` key is the version of the contract the plugin was built for. The two are independent: a plugin at version 0.3.0 may target `ilium:plugin@0.1.0`.

## The markers on each item

Every item in the contract carries one of two markers:

| Marker | Meaning | Who can use it |
|---|---|---|
| `@since(version = 0.1.0)` | The item first appears in that version and keeps its shape in later minor versions. | Every plugin built for that version or later. |
| `@unstable(feature = <name>)` | The item may change or disappear inside 0.x. | Only plugins whose `unstable` key lists `<name>`. |

Version 0.1.0 has four unstable features. The manifest `unstable` key accepts only these names:

| Feature name | What it opens | Why it is unstable |
|---|---|---|
| `ui-calendar` | The `calendar` widget, which mirrors the monthly calendar of ratatui. | No first-party plugin needed it for 0.1, so its shape is still open. |
| `plugin-services` | Plugin-defined services: `services.call`, and the `services.use` and `services.provide` grants for names such as `com.example.titles/suggest`. | The payload contract between two plugins is new. |
| `gpu-kernels` | The `gpu.kernels` grant and the `gpu.run` function. See [More unstable features](#more-unstable-features). | The kernel interface depends on graphics hardware that varies. |
| `websocket` | WebSocket connections. See [More unstable features](#more-unstable-features). | The streaming contract may change. |

The `chart` widget is stable in 0.1.0. Kernel-known typed services, such as `ilium:inference/provider`, are stable too.

## Frozen snapshots and the window

Each released minor version is kept as a frozen snapshot of the contract, in a folder named after it, for example `wit/since_v0_1/`. The host loads the current minor version and the previous one through small adapters written by hand. A plugin built for the previous minor keeps working until the window moves past it. Version 0.1.0 is the first release, so it has no previous minor, and a host at 0.1.0 loads only `ilium:plugin@0.1.0`.

The window is the current minor version and the previous one, counted by minor number. A package whose `api` falls outside the window is refused at install with `package.api-unsupported`. The message names both ends of the window:

<pre class="term">
<span class="t-prompt">$</span> ilium plugin install demo-plugin-0.3.0.iliumpkg
<span class="t-err">error</span> package.api-unsupported
<span class="t-dim">      Plugin demo-plugin 0.3.0 needs a plugin API</span>
<span class="t-dim">      that this Ilium does not support.</span>
<span class="t-dim">      This Ilium supports</span>
<span class="t-dim">      ilium:plugin@0.1.0.</span>
</pre>

Illustrative, not generated. A host with a previous minor version names both versions it supports.

<div class="note">
<p>The window is a rule about minor numbers only. The exact match between the `api` value and the `ilium:api-version` section still applies inside the window.</p>
</div>

## Unstable opt-in checks

The install checks for the `unstable` key run before any code loads:

| Code | When it fails | What the message says |
|---|---|---|
| `manifest.unstable-unknown` | A name in `unstable` is not an unstable feature of this host. | The name and the plugin API. |
| `manifest.unstable-tier` | A T2 or T3 package lists a feature outside development mode. | That the feature needs `ilium plugin dev`. |
| `manifest.unstable-missing` | The component imports an unstable item, but its manifest does not list the feature. | The feature name and the item that uses it. |

Development mode is the only way to install a T2 or T3 package that uses an unstable feature. Such a package is shown with a `dev` badge. A T1 bundled plugin may list unstable features in any build.

<pre class="term">
<span class="t-prompt">$</span> ilium plugin install notes-caller-0.1.0.iliumpkg
<span class="t-err">error</span> manifest.unstable-tier
<span class="t-dim">      notes-caller 0.1.0 lists plugin-services,</span>
<span class="t-dim">      which is unstable.</span>
<span class="t-dim">      Unstable features need development mode:</span>
<span class="t-dim">      ilium plugin dev.</span>
</pre>

Illustrative, not generated. The same package installs in development mode with `ilium plugin dev`.

When a later version removes or renames an unstable feature, a package that still lists the old name fails with `manifest.unstable-unknown` after the upgrade. Read the changelog before you upgrade Ilium with a plugin that uses an unstable feature.

## Worked example: a caller of an unstable service

The manifest above lists `plugin-services` and requires `com.example.titles/suggest@0.1.0`. The code below makes one call. It needs the `services.use:com.example.titles/suggest` grant on the caller side.

Rust, in a crate that depends on `ilium-plugin-sdk = "0.1"`:

**Rust**

```rust
use ilium_plugin_sdk::host::services;
use ilium_plugin_sdk::prelude::PluginError;

/// Asks the titles provider for a title. Unstable in 0.1: `plugin-services` must be listed.
pub async fn suggest_title(note: &str) -> Result<String, PluginError> {
    // Escape the note with a JSON library before you format it in a real plugin.
    let payload = format!(r#"{{"text":"{note}"}}"#);
    services::call("com.example.titles/suggest", "suggest", &payload).await
}
```

TypeScript, in a package that depends on `@ilium/plugin-sdk` at its pinned version:

**TypeScript**

```ts
import { call } from '@ilium/plugin-sdk/host/services';

/** Asks the titles provider for a title. Unstable in 0.1: `plugin-services` must be listed. */
export async function suggestTitle(note: string): Promise<string> {
  const payloadJson = JSON.stringify({ text: note });
  return call('com.example.titles/suggest', 'suggest', payloadJson);
}
```

Both functions return the reply string. If the provider is missing, the call fails with `not-found`, as it does for any service call.

## What the contract contains

The contract is one unit, `ilium:plugin@0.1.0`, and its items fall into three kinds.

| Kind | What it contains | Examples |
|---|---|---|
| New | Interfaces, functions, export fragments, and cases appended at the end of variants and enums | the host interfaces `catalogs`, `geocode`, `sessions`, `kernel-settings`, `keymap`, `client-requests` and `extensions`, the exports `catalog-provider`, `menu-provider`, `virtual-rows-provider` and `token-provider`, 18 events, 23 intents, 24 grants |
| Manifest | Keys and fields on manifest entries | nine `contributes` keys, `limits_tier`, 18 settings row types, 13 `manifest.*` codes |
| Shape | Fields of records, fixed by the first freeze | See [ABI evolution](/pages/reference-abi-evolution/#2-what-may-change). After the freeze a record never gains a field |

The widget catalogue has 51 widgets in seven families, there are 18 settings row types, 17 ui-action kinds and 12 action-value cases, and the outcome case `ok-epoch(u64)` is part of the outcome variant. A plugin that matches only `ok` still works, because `is_ok()` in Rust and `isOk()` in TypeScript cover both.

The render shows how plugins look in the install check.

<pre class="term">╭  ≡ ● · Install check─────────────────────────────╮
│ old-titler 1.2.0 ……… api ilium:plugin@0.1.0  ✓   │
│ future-x 1.0.0 …… api ilium:plugin@0.3.0  ✕      │
│   built for a newer minor than this host (0.1.0) │
╰──────────────────────────────────────────────────╯
</pre>

The first loads. The second is refused with `package.api-unsupported`, naming both versions. Illustrative, not generated.

### Which `api` string a plugin declares

The manifest key `api` must equal the version in the component's `ilium:api-version` section. A plugin declares `ilium:plugin@0.1.0` in both places. A plugin built for a newer minor than the host is refused. The SDKs write the version into the component for you.

The package manifest carries the string in the `api` key, shown here for both SDKs because the manifest is JSON:

```json
{ "id": "com.example.guard", "version": "0.4.0", "publisher": "Example Labs", "api": "ilium:plugin@0.1.0" }
```

Each SDK project file takes the contract version from the SDK dependency: `ilium-plugin-sdk = "0.1"` in Rust and `@ilium/plugin-sdk` at its pinned version in TypeScript.

### More unstable features

Two of the four unstable features (see [ABI evolution](/pages/reference-abi-evolution/#3-markers-and-unstable-features) for the one list) are described in this section: `gpu-kernels` (the `gpu.kernels` grant and the `gpu.run` function) and `websocket`. They are visible only to a plugin that opts in, and T2 and T3 plugins need developer mode for them. They may change in any release.

The manifest lists the opt-in, so the line is the same for Rust and TypeScript plugins:

```json
"unstable": ["gpu-kernels"]
```

## Stable public names

Some names are part of a plugin's public surface, and they keep their spelling across plugin versions. These are:

- pane kind ids,
- command ids,
- settings keys,
- decoration slots, and
- topic names.

The kernel stores them as strings in the snapshot of each user. A rename breaks the saved state of that user, so treat each name as permanent once a release has used it. To change a name, add the new name, keep the old one working and document both. Removing the old name is a change that is not allowed after version 0.1.0.

## The rule after version 0.1.0

Moving the editor and the boards into plugins after the first freeze may only add items marked `@since`. Shape items change the record layout, so they belong to the first freeze, and every later release only adds. Nothing that version 0.1.0 exports is removed or changed. The editor interface, the basic interface, the cases of the widget variant, and the events and intents keep their shape. New widget cases are appended at the end of the variant. See [Pane primitives](/pages/primitives/).

A later addition looks like this. Suppose a future minor adds a grant marked `@since(version = 0.4.0)`. A package built for `ilium:plugin@0.1.0` that names that grant fails at install with `manifest.grant-unknown`, because the grant is not known to its API.

## Versions of extension points

The contract version `ilium:plugin@0.1.0` is one thing. Each extension point has its own `version`, the semver of its payload, reply or entry schema. A contributor may write `"point": "<plugin-id>/<id>@<minimum-version>"`. The point satisfies it when the major version is the same and the minor version is at least as high. An owner that changes a schema incompatibly raises the major version, and contributions that name the old major version become dormant. See [extension points](/pages/api-extension-points/).

## Practical advice

- Set the manifest `api` value to the version your SDK was built for, and rebuild the package whenever you update the SDK.
- Use unstable features only in development. A feature may change or disappear inside 0.x.
- Do not rename a pane kind, command, setting or topic in a later release. Add the new name and leave the old one working.
- Expect a refusal for a package whose `api` is outside the window. Rebuild it with an SDK for a version the host supports.
- Read [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) before you rename anything, change a data layout or raise a version. The plugin MUST keep every name of the stable public list across its own releases. [AUT-214]

## See also

- [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) for the author side of versions
- [Packages and manifests](/pages/packages/)
- [Manifest reference](/pages/reference-manifest/)
- [Plugin services](/pages/services/)
- [Widgets: data display](/pages/widgets-data-display/)
- [Pane primitives](/pages/primitives/)
- [Worlds and sides](/pages/worlds-and-sides/)

API version checked: `ilium:plugin@0.1.0`.
