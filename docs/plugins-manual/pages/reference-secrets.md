---
layout: page
title: Secrets
permalink: /pages/reference-secrets/
---

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

**[SEC-958]** (proposed, owner to confirm) Plugin data is stored as plain bytes, and the host does not encrypt it at rest. This covers the plugin data and bucket folders, the pre-migration backups, the session snapshot files, the plugin logs and the archives that `ilium plugin backup` writes. The only protection the host gives these files is the owner-only access of [USR-206] and the encryption, if any, that the user or administrator applies to the disk or the home folder. A plugin that holds personal data it must not expose at rest MUST encrypt that data itself and keep any key it needs in a secret row ([SEC-240]). This version offers no host-side encryption option for plugin data. Test: review against the page text, and read the stored files of a test plugin to confirm that the values are readable.

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

## Keyring changes and restores

**[SEC-959]** (proposed, owner to confirm) When the keyring becomes available again after a start that used the file store ([SEC-241]), the host MUST move every value in `secrets.json` into the keyring at the next start. A value written while the file store was in use is the newest value, so it MUST overwrite any older keyring value for the same key. The host MUST remove the file copy only after the keyring returns the value on readback. Test: start with the keyring locked, store one secret, unlock the keyring, restart, and assert that the keyring holds the value and that `secrets.json` no longer holds it.

**[SEC-963]** (proposed, owner to confirm) When the keyring is reset or emptied, or a secret value is missing from the store for any other reason, the host MUST treat that secret as unset. `settings.get-json` MUST then read the key as unset, as it does for a key with no stored value in [API: settings](/pages/api-settings/). At the next start the host MUST send the plugin a `settings-changed` event for each such key, so that the plugin can tell the user that the credential is missing. Test: delete one keyring entry, restart, and assert that `get-json` reads the key as unset and that the event arrives once.

**[SEC-964]** (proposed, owner to confirm) A secret lost by a keyring change ([SEC-963]) or by a restore ([SEC-250]) is re-entered by the user, one secret row at a time, on the settings tab of the plugin that owns the row. The host MUST NOT offer a bulk import of secrets, and it MUST NOT export a secret to make re-entry easier ([SEC-250]). A plugin SHOULD show a clear message that names the missing row when its secret is unset, and MUST NOT retry a request with an empty credential. Test: restore a snapshot that holds a plugin secret, assert that the row is empty, re-enter the value, and assert that `get-json` returns the value to the plugin.

## Secrets in host memory and in child processes

[SEC-247] gives only plugin guidance. This section states what the host itself does with a secret while it holds it. The rules are host requirements and carry the IDs of the rules they extend; where no ID exists yet the row says so.

**[SEC-247]** (host side) The kernel reads a secret from the keyring just in time, that is when `settings.get-json` or an internal consumer such as a provider call needs it, and holds the plaintext in a buffer whose type zeroises on drop (Rust `zeroize`). The buffer is zeroised when the request ends, on success and on error. The kernel keeps no long-lived plaintext cache of a secret. The only other plaintext copy is the redaction list of [SEC-243], and a rotated value stays there for no longer than the 60 s window of [SEC-246].

**[SEC-033]** (host side) The host never places a secret value in the argv or the environment of a `process.run` child or of a pane command. `/proc/<pid>/cmdline` and `/proc/<pid>/environ` can be read by other processes of the same user, and redaction ([SEC-243], [SEC-066]) only hides a value in text the host writes afterwards. A plugin that needs a secret in a child sends it on stdin or in a request header. A call that carries a stored secret value in `args` or `env` is not detected by value; a plugin that does this by its own code is the plugin's defect, as for [SEC-245]. The host side of the rule is that no host feature (a `{secret:key}` style substitution, a provider helper, a pane launcher) writes a secret into argv or environment.

**[SEC-244]** (host side) The server never includes a secret value in an IPC frame to the client, except to the side that owns the row: the settings editor of the owning plugin or of the kernel tab receives the value only when the user opens that row with reveal, and then only up to `reveal_last` characters ([SEC-248]). All other frames, including the render cache and `settings-changed`, carry the key name only.

**[SEC-445]** (scope) Core dumps and crash reports are covered by [SEC-445] and [SEC-446] in the [Audit log](/pages/reference-audit-log/).

Accepted limit: the host cannot control the swap area or the hibernation image of the operating system, so plaintext that was in memory when the system swapped or hibernated can persist on disk. Users who need to rule this out use encrypted swap or disable hibernation. This limit is the same as for data at rest in [SEC-958].

| Test | Setup | Pass condition |
|---|---|---|
| Argv and environment | Store a secret, run a `process.run` child and a pane command through every host feature that can read settings, then read `/proc/<pid>/cmdline` and `/proc/<pid>/environ` of each | The secret value (raw and the variants of [SEC-243]) appears in neither file |
| Zeroise after request | Call `settings.get-json` for a secret in a debug build with a memory probe | The buffer reads as zero after the call returns |
| IPC frames | Capture all frames sent to a client while a secret is stored and rotated | No frame holds the value except the reveal frame of the owner |

## Secrets in a whole-host archive versus one-at-a-time re-entry

Two pages answer how secrets survive a restore: the [Backup and restore](/pages/guide-backup-and-restore/) guide describes `ilium plugin backup --include-secrets` ([HST-652], [HST-960]), and [SEC-964] says a lost secret is re-entered one row at a time. The exact rule is this.

**[HST-652]** (secrets) `ilium plugin backup` leaves secrets out unless `--include-secrets` is given. With the flag, the archive holds the secret values in the plaintext form of [SEC-958]; the archive is not encrypted, so the user protects it like the keyring itself (mode 0600, encrypted disk or storage).

**[SEC-250]** (restore rules) A restore of an archive made with `--include-secrets`:

| Restore target | Result |
|---|---|
| Same machine, same keyring | The secrets are written back to the keyring (or to `secrets.json` under [SEC-241]) and the plugins read them with `settings.get-json`. |
| Another machine, or a keyring that was reset | The values are not usable there; each secret is treated as unset ([SEC-963]), the plugin gets a `settings-changed` event, and the user re-enters each row ([SEC-964]). |
| Archive made without `--include-secrets` | Every secret row is empty after the restore ([SEC-250]); the user re-enters them. |

There is no bulk import and no bulk export of secrets in any case: `--include-secrets` is an archive of a whole host for disaster recovery, not an import path for a single row ([SEC-964]).

**[LIF-290]** (secrets) `ilium plugin export <id> --out <file>` never includes a secret, with or without any other option. The settings values in that archive are the non-secret rows only; a secret row appears as unset. Test: store a secret, export the plugin, list the archive and search every entry for the value and its [SEC-243] variants; none is found.

An operator planning recovery therefore records, outside Ilium, where each credential comes from, and uses `--include-secrets` only when the restore target is the same machine and keyring.

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
