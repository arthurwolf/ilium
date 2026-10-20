---
layout: page
title: "Capability: settings.write-kernel:<kernel-tab-id>"
permalink: /pages/capability-settings-write-kernel/
---

## settings.write-kernel:<kernel-tab-id>

The `settings.write-kernel:<kernel-tab-id>` capability lets one first-party plugin write the values of one built-in Settings tab, for example the Terminal tab. It is how a core plugin such as the settings owner of that tab keeps the tab editable after the tab moved out of the kernel. It is high risk, T1 only, and always needs an explicit grant.

**You will need:**

- A `settings.write-kernel:<kernel-tab-id>` grant with exactly one tab id from the settings reference. A wildcard is refused with `manifest.grant-wildcard`.
- A first-party (T1) package. A T2 or T3 package that declares the grant is refused with `manifest.grant-tier`.
- To be the only installed plugin that holds the grant for that tab. A second holder is refused with `manifest.settings-owner`.

### Rules

- The host MUST accept `kernel-settings.write` for tab `T` only from the plugin that holds `settings.write-kernel:T`.
- A write without the grant, or after the user revoked it, MUST fail with `not-permitted`.
- A write to an unknown tab or key MUST fail with `not-found` and change nothing.
- A value that fails the tab schema, or lies outside the range of its row, MUST fail with `invalid-argument` and change nothing; the host logs `settings.invalid-value` for an out-of-range value. A write is all or nothing.
- A successful write MUST be persisted before the call returns and MUST produce one `kernel-settings-changed` event naming the tab and the changed keys.
- Keybindings are not part of any kernel tab. No grant writes a binding.
- A secret key MUST NOT be written through this call. The owner stores it through the secrets store ([secrets](/pages/reference-secrets/)).

### Errors

| Call | Code | Cause |
|---|---|---|
| `kernel-settings.write` | `not-permitted` | no grant for that tab, or revoked |
| `kernel-settings.write` | `not-found` | unknown tab or key |
| `kernel-settings.write` | `invalid-argument` | value fails the tab schema |
| manifest | `manifest.grant-wildcard` | wildcard tab id |
| manifest | `manifest.grant-argument-missing` | missing tab id |
| manifest | `manifest.grant-tier` | package is not T1 |
| manifest | `manifest.settings-owner` | another plugin already owns the tab |

See [Kernel settings and keymap](/pages/api-kernel-settings-and-keymap/) for the host interface and [settings.read-kernel](/pages/capability-settings-read-kernel/) for the read side.
