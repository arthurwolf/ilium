# Secrets

## Secrets

A **secret** is a value that gives access to something: an API key, a token, a password. This page says where Ilium keeps secrets, who can read them, how they are kept out of logs, and what plugin authors must do. The rules are scattered across the settings pages; this page is the single normative source and the other pages link here.

The attacker classes are A1, A3 and A8 in the [Threat model](/pages/reference-threat-model/). Secrets that never reach the host, such as a publisher's private signing key, are covered by [Trust and supply chain](/pages/reference-trust-and-supply-chain/).

### You will need

- The settings row types in [Settings schema](/pages/reference-settings-schema/), in particular `secret`.
- The settings API in [API: settings](/pages/api-settings/).

## What counts as a secret

**[SEC-240]** The host treats these as secrets: the value of any settings row declared with `"secret": true`; the keys in the built-in `api` settings tab (model provider keys); and webhook URLs of notification channels that contain a token. Publisher private keys are not handled by the host at all.

## Storage at rest

**[SEC-241]** Secret values are never written to `config.toml`, to a project `.ilium/` folder, to session snapshots, to exports or to the pane journal. They are stored in the **secrets store**: the OS keyring when one is available (Secret Service on Linux, Keychain on macOS, Credential Manager on Windows), and otherwise `secrets.json` in the user state directory, mode 0600, in the deny set ([SEC-060]). The file has the form `{ "version": 1, "plugins": { "<id>": { "<key>": "<value>" } }, "kernel": { "<tab>.<key>": "<value>" } }` and is written atomically. When the keyring is used, the store holds only an index with no values.

**[SEC-242]** A settings row with `secret` together with `scope: "project"` is refused at install with `manifest.secret-scope`. A secret never comes from a project file. A secret value found in a project file is ignored and audited.

## Who can read a secret

**[SEC-244]** Only the plugin side that owns the row reads its secrets, through `settings.get-json`. `settings.read-all` and `settings.read-kernel` return an empty string for a plugin secret and `<redacted>` for a kernel secret. A secret does not appear in events, in the `settings-changed` payload (the event names the key only), in the review dialog or in `ilium plugin ... --json` output.

**[SEC-245]** The host never forwards a secret value to another plugin: not through `messages.peer`, services, topics or extension points. A plugin that sends its own secret to another plugin does so by its own code; the host cannot stop that, which is a residual risk of inter-plugin channels ([SEC-007]).

**[SEC-250]** Snapshots and exports exclude secrets. After a restore the secret rows are empty and the plugin is told with a `settings-changed` event.

## Redaction

**[SEC-243]** The host replaces secrets in text it writes. Algorithm:

1. Collect the current secret values of all plugins and the kernel that are at least 6 characters long. Shorter values are not redacted (the settings page shows the fixed hint `Short secrets are not redacted in logs` under a `secret` row while its stored value is shorter than 6 characters; the value is still accepted).
2. For each value, build the variants: the raw bytes, the base64 form (standard and URL-safe, with and without padding), the percent-encoded form, and the hexadecimal form.
3. Replace every occurrence of each variant, case-sensitively, with `<redacted>`. Longer values are replaced first.

The step applies to plugin log lines, `process-run` log lines and audit records, trap messages, HTTP request logs, diagnostics report sections ([SEC-249]) and error strings returned to plugins. Limits, stated honestly: a plugin can transform a secret (reverse it, split it, encrypt it) and then write it; redaction stops accidents, not malice.

**[SEC-249]** Diagnostics reports (`ilium.core.diagnostics/report-sections`) pass through redaction before they are shown or saved. A section that contains a secret in a form the algorithm cannot match is the contributor's defect.

**[SEC-246]** Rotation. When a secret changes, the host overwrites the old value in the store, forgets it for redaction purposes after 60 s (so late log lines are still redacted), and keeps no copy.

## Guidance for plugin authors

**[SEC-247]** The following is guidance and has no automated test, except that a secret in a log line or a query string is a review item. A plugin SHOULD: never log a secret or a request that carries one; send secrets in headers, not in query strings (URLs are logged on errors and shown in consent prompts); keep a secret in memory only as long as a request needs it and overwrite buffers it controls; avoid placing a secret in a place a command line shows; and name the row clearly so the user knows what to enter. Rust and TypeScript examples follow.

**Rust**

```rust
use ilium_plugin_sdk::host::{http::{self, Request}, settings};
use ilium_plugin_sdk::prelude::*;

/// Calls an API with a bearer token read just in time. The token never reaches a log line.
fn call_api() -> Result<Vec<u8>, PluginError> {
    let mut token = settings::get_json("api_token")?.into_bytes();
    let mut request = Request::get("https://api.example.com/v1/me");
    request.header("Authorization", &format!("Bearer {}", String::from_utf8_lossy(&token)));
    let result = http::send(request).map(|response| response.body);
    token.iter_mut().for_each(|byte| *byte = 0); // best effort: shorten the secret's lifetime
    result
}
```

**TypeScript**

```ts
import { get } from '@ilium/plugin-sdk/host/settings';
import { send } from '@ilium/plugin-sdk/host/http';

/** Reads the token only when needed and sends it as a header, never in the URL. */
export async function callApi(): Promise<Uint8Array> {
  const token = await get('api_token');
  const response = await send({
    method: 'GET',
    url: 'https://api.example.com/v1/me',
    headers: { Authorization: `Bearer ${token}` },
  });
  return response.body;
}
```

## What the user sees

**[SEC-248]** A secret row is masked with `••••`; `reveal_last` shows up to 8 trailing characters. Settings > Plugins > Secrets lists which plugins hold secrets (names only) and offers **Forget secrets** per plugin and for all. Forgetting removes the values from the store and the keyring and sends the plugin a `settings-changed` event.

## Edge cases

| Situation | Result |
|---|---|
| Keyring is locked or unavailable at start | The file store is used; a notice appears once |
| Secret shorter than 6 characters | Stored and masked, not redacted in logs |
| Package declares `secret` with `scope: "project"` | `manifest.secret-scope` at install |
| Plugin uninstalled | Its secrets are removed from the store with its grants |
| Plugin logs a secret split in two lines | Not matched; this is the plugin's defect |

### See also

- [Settings schema](/pages/reference-settings-schema/)
- [API: settings](/pages/api-settings/)
- [Audit log](/pages/reference-audit-log/)
- [Diagnostics core plugin](/pages/core-plugin-diagnostics/)
- [Debugging](/pages/debugging/)

Contract: `ilium:plugin@0.1.0`.
