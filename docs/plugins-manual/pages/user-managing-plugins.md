---
layout: page
title: Managing plugins
permalink: /pages/user-managing-plugins/
---

## Managing plugins

This page shows how to see what your plugins are doing, turn them on and off, deal with failures, and how an administrator restricts them.

Contract version: `ilium:plugin@0.1.0`.

### The Plugins screen

Open Settings, then Plugins. Each row shows the name, the tier, the version, the state and the health. Select a row to open its detail tabs.

| Tab | Shows |
|---|---|
| Overview | state, health, version, publisher, scope |
| Capabilities | grants, with a switch for each |
| Resources | memory, execution bucket, storage and disk use |
| Log | the last lines of the plugin log |
| Data | data size, buttons Clear cache and Delete data |

<figure>
<pre class="term"> Plugins
 Name              State    Health
 Notes             on       healthy
 Cursor trail      failed   failed
 Tickets           on       degraded
 [Enter] detail  [Space] on/off
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The Plugins list with a state and a health column.</figcaption>
</figure>

- [LIF-190] The Plugins screen MUST show state and health for each installed plugin, and a detail view with the five tabs. Test: render a fixture list.

### When a plugin fails

After a failure the plugin restarts after a short delay (see [Lifecycle](/pages/lifecycle/#failure-counting-and-restart)). After the third failure in a window it is turned off, and the row shows "failed". You see a notice with three buttons.

1. Enable again. This clears the failure count and starts the plugin.
2. Open log. Shows the lines that explain the failure.
3. Uninstall.

If the failed plugin replaced a built-in one, the built-in plugin is turned on again and a notice says so.

- [LIF-191] Enable on a failed plugin MUST clear the failure window. Test: fail, enable, fail twice more and assert it is not disabled.
- [LIF-192] A replacement disabled by failure MUST re-enable the built-in plugin with a notice. Test: fail a replacement.

### The recovery page

The recovery page lists plugins with state and last failure. It offers seven actions: enable, disable, reset failures, uninstall, forget settings, safe mode and open config file ([Safe mode](/pages/user-safe-mode-and-troubleshooting/)). Open it with `Ctrl+B`, then `:` and the Recovery entry when Settings is not available, or from the **Open Plugins page** link on a disabled plugin's row or a failed-start notice.

### Update, rollback and uninstall

Select Update on a row, or run `ilium plugin update <id>`. If the new version fails to start within the trial, Ilium puts the old version back. Uninstall asks whether to keep or delete the plugin data ([Plugin files](/pages/reference-plugin-files/)). Details are in [Install, update and uninstall](/pages/reference-install-update-uninstall/).

### Administrator policy

An administrator controls plugins for all users with `/etc/ilium/policy.toml` on Linux. Keys are in [Plugin policy](/pages/reference-plugin-policy/). A plugin that the policy blocks shows "Blocked by policy" and its data stays. A policy may forbid third-party plugins, limit tiers, allow or deny publishers and ids, and forbid grants. On macOS the file is in `/Library/Application Support/ilium/` and on Windows in `%PROGRAMDATA%\ilium\`.

The policy can also block exact versions, force safe mode, or pin a version. These per-version keys are planned and are not in the first policy format. A plugin blocked by id already shows the code of the specific rule as its disabled reason (for example `policy.id-denied` for `plugins.deny_ids`). Until the per-version keys exist, use `plugins.deny_ids` and `plugins.allow_ids`.

- [LIF-193] A plugin blocked by policy MUST show "Blocked by policy" with the policy code of the rule that blocked it (for example `policy.id-denied`, `policy.third-party-denied`; codes in [Plugin policy](/pages/reference-plugin-policy/)) and MUST keep its data. Test: add its id to `plugins.deny_ids` and assert the row text and the code.

### See also

- [Install and manage plugins](/pages/user-install-and-manage/)
- [Safe mode and recovery](/pages/user-safe-mode-and-troubleshooting/)
- [Health and watchdog](/pages/reference-health-and-watchdog/)
- [Security](/pages/security/)
- [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/)

API version checked: `ilium:plugin@0.1.0`.
