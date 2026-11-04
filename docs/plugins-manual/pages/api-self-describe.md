---
layout: page
title: Self description
permalink: /pages/api-self-describe/
---

## Self description

The `self-describe` import tells a plugin who it is and what it is allowed to do. A plugin reads its own id, its version, the `api` string from its manifest, the host version and the list of grants the user has allowed. A plugin with a refused grant can show a "needs grant" state at once, instead of making a call, receiving `not-permitted` and parsing the message.

The import only describes the calling plugin. It does not list other plugins or the grants of another side. The user manages grants in Settings > Plugins, as [help and plugins](/pages/api-help-and-plugins/) and [Capabilities and grants](/pages/capabilities/) describe.

**You will need:**

- No grant. `self-describe` is available to server and client plugins.
- Rust: the `ilium-plugin-sdk` crate, module `ilium_plugin_sdk::host::self_describe`.
- TypeScript: the `@ilium/plugin-sdk` package, subpath `@ilium/plugin-sdk/host/self-describe`.

<div class="note">
<p>The function is named <code>describe</code> and not <code>self</code>, because <code>self</code> is a reserved word in WIT. The Rust and TypeScript names are <code>describe</code> too.</p>
</div>

### Interface

Interface version: `ilium:host/self-describe@0.1.0`.

```wit
// Excerpt of ilium:host/self-describe in reference-wit
interface self-describe {
  use ilium:types/ids@0.1.0.{plugin-id};
  record self-description {
    plugin-id: plugin-id,
    version: string,
    api: string,
    host-version: string,
    grants: list<string>,
  }
  describe: func() -> self-description;
}
```

### Functions

| Function | Parameters | Returns | What it does |
|---|---|---|---|
| `describe` | none | `self-description` | Returns the identity of the calling plugin and its current grants. The call cannot fail, so it has no error channel. |

The record `self-description` has five fields:

| Field | Meaning | Example |
|---|---|---|
| `plugin-id` | The id from the manifest. | `com.example.reader` |
| `version` | The version of the installed package. | `0.3.1` |
| `api` | The `api` string of the manifest, the contract version the plugin was built for. | `ilium:plugin@0.1.0` |
| `host-version` | The version of the running Ilium host. It is the value of `ilium version --json`. | `0.9.0` |
| `grants` | The grants the user has allowed for the calling side, as the strings of the manifest. | `fs.read:data`, `services.use:com.example.titles/suggest` |

The list `grants` holds only grants that are granted now. A grant that the user has not allowed, or has refused, is not in the list. A grant that a plugin asks for but the user has not decided on is also not in the list.

### Example in Rust

The plugin shows a "needs grant" state for a feature whose grant is missing.

```rust
use ilium_plugin_sdk::host::self_describe;

fn has_grant(grant: &str) -> bool {
    self_describe::describe().grants.iter().any(|allowed| allowed == grant)
}

fn needs_file_grant() -> bool {
    !has_grant("fs.read:data")
}
```

### Example in TypeScript

The same check in TypeScript, with the version and host version read for a support line.

```ts
import { describe } from '@ilium/plugin-sdk/host/self-describe';

export function hasGrant(grant: string): boolean {
  return describe().grants.includes(grant);
}

export function supportLine(): string {
  const self = describe();
  return `${self.pluginId} ${self.version} on host ${self.hostVersion}`;
}
```

### Edge cases

- **A grant that is revoked** leaves the list at once after the user revokes it. The plugin learns of the change from the event that the host sends, and the next call to `describe` no longer lists the grant.
- **A grant that is refused** is never in the list. A refusal at install and a refusal at a prompt both leave the grant out.
- **A plugin restarted by the host** reads the same `plugin-id`, `version` and `api`. The list `grants` is read again from the current decisions.

### What the user sees

The user sees the same grants in Settings > Plugins. A plugin that reads `describe` shows nothing by itself. A "needs grant" message is written by the plugin, in its own panel or notice.

### See also

- [help and plugins](/pages/api-help-and-plugins/) for the `plugins` import, which reads other plugins.
- [Capabilities and grants](/pages/capabilities/) for how a grant is requested and decided.
- [Self limits](/pages/api-self-limits/) for the budgets the plugin can read.
- [Packages and manifests](/pages/packages/) for the `api` field.

## Requirement statements

- **[API-226]** The host MUST provide a read-only `describe()` import that returns the calling plugin's id, its version, its manifest `api` string, the host version, and the list of grants currently granted to the calling side.

Test: call `describe` from a plugin, then assert each field against the manifest and against the output of `ilium version --json`.

- **[API-227]** The grant list returned by `describe()` MUST change within one event delivery after a grant is granted or revoked, and it MUST NOT include a grant that was refused.

Test: revoke a grant at runtime, wait for the event, and assert that `describe()` no longer lists it; refuse a second grant and assert that it never appears in the list.
