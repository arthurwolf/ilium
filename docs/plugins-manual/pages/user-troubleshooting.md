---
layout: page
title: Troubleshooting plugins
permalink: /pages/user-troubleshooting/
---

## Troubleshooting plugins

This page helps you when a plugin does not install, does not start, keeps stopping, slows Ilium down, floods the screen, or stops Ilium from starting. It starts with a table of symptoms, then explains safe mode, the recovery page, how to find the plugin that causes a problem, how to read a plugin's log, and how to report a problem.

Contract version: `ilium:plugin@0.1.0`. Requirement ids have the form `[USR-nnn]`, collected in [User and administrator requirements](/pages/reference-requirements-user-admin/).

<div class="note">
<p>Decision status. The platform is not implemented. The name of the safe mode start flag, <code>--safe-mode</code>, is a working name that the platform owner confirms. The behaviour is specified in <a href="/pages/reference-plugin-lifecycle-states/">Plugin lifecycle states</a>.</p>
</div>

### Contents

- Find your symptom
- Safe mode
- The recovery page
- Find the plugin that causes a problem
- Read the plugin log
- Restart rules you will see
- Protective limits you will see
- Roll back and repair
- Report a problem
- Errors and edge cases
- Requirements

### Find your symptom

| What you see | Likely cause | What to do |
|---|---|---|
| "Cannot install <name> <version>." | You are on Windows or macOS, and the plugin is third-party | Use Linux, or use only bundled plugins |
| Install fails with a code such as `manifest.file-hash` | The file is damaged or was changed | Get the file again and compare its checksum with the publisher's |
| Install fails with `policy.blocked` | Your administrator limits plugins | Ask your administrator. See [Administration guide](/pages/guide-administration/) |
| A plugin shows **Failed** | It failed 3 times within 10 minutes | Read its log, then turn it on again (see below, and [Plugin health for users](/pages/user-plugin-health/)) |
| A plugin shows **Refused** | Ilium checked it at start and refused to run it | Read the code in the row. Reinstall the package or get a new version |
| A plugin shows **Blocked by policy** | Your administrator forbids it | Ask your administrator |
| A plugin shows **Blocked** with `trust.key-revoked` or `registry.takedown` | The publisher key was revoked as compromised, or the registry took the version down | Do not turn it on unless you accept the risk. Install an update from the publisher, or run `ilium plugin enable <id> --accept-risk`. See [Install and manage plugins](/pages/user-install-and-manage/) |
| A plugin shows **Waiting for <owner>** | It depends on a plugin that is off or removed | Turn on or install the owner |
| A panel or pane is empty | A permission was declined, or the plugin is off | Check the Capabilities tab and the log |
| A pane says "Plugin <name> is not installed" | The plugin that made the pane was removed | Install it again, or close the pane |
| A key does nothing | Another binding wins, or the plugin is off | See Settings > Keys. Your own remap wins over plugins |
| Lines of toasts appear | A plugin shows many messages | Ilium shows at most 3 per 10 s per plugin. Turn the plugin off |
| Your keyboard is unresponsive for a moment | A plugin locks input | The lock ends after 2 s. Revoke `terminal.input-lock` |
| Ilium is slow | A plugin uses much processor or memory | Run `ilium plugin stats <id>`. Turn it off |
| Settings does not open | The settings plugin failed | Use the recovery page |
| Ilium does not start well after you installed a plugin | A plugin breaks startup | Start in safe mode |
| An update shows "Update failed, rolled back" | The new version failed to start | The old version is back. Read the log of the new package's id |

### Safe mode

Safe mode starts Ilium with every third-party plugin (T2 and T3) switched off for this run. Core and bundled plugins run. Nothing is deleted, nothing is written to the registry, and the next normal start has every plugin as before.

Start it in one of three ways:

1. From a terminal: `ilium --safe-mode`.
2. From the recovery page, press the Safe mode action. Ilium restarts the client with third-party plugins off.
3. If Ilium fails to start twice in a row while a plugin starts, Ilium offers safe mode at the next start. (Pending decision: the offer after two failed starts is proposed.)

In safe mode, a banner stays at the top of the screen: "Safe mode: third-party plugins are off." The Plugins tab still works. Every third-party row shows **Off** with the reason `safe-mode`. You can read logs, uninstall plugins and use the bisect tool. You cannot turn a third-party plugin on until you leave safe mode.

<figure>
<pre class="term"><span class="t-hi">╭─ Plugins (safe mode) ────────────────────────╮</span>
│ <span class="t-warn">Safe mode: third-party plugins are off.</span>      │
│                                              │
│ ilium.core.tree <span class="t-dim">…………………………………………………………………</span> <span class="t-ok">On</span> │
│ ilium.core.layout <span class="t-dim">……………………………………………………………</span> <span class="t-ok">On</span> │
│ com.example.notes (T2) <span class="t-dim">………………</span> <span class="t-dim">Off, safe mode</span> │
│ com.example.old (T3) <span class="t-dim">……………………</span> <span class="t-dim">Off, safe mode</span> │
│                                              │
│ Restart Ilium without --safe-mode to leave.  │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The Plugins tab in safe mode.</figcaption>
</figure>

- [USR-030] Safe mode MUST switch off every T2 and T3 plugin for the run, MUST leave core and bundled plugins on, and MUST NOT write the state to the registry. Test: start in safe mode, stop, start normally and assert every plugin is as before.
- [USR-031] Safe mode MUST show a banner on every screen and MUST label every third-party row as off with the reason `safe-mode`. Test: render the tree and the Plugins tab in safe mode.

### The recovery page

The recovery page is a screen that belongs to Ilium itself, not to any plugin. No plugin can replace it, hide it or change it. It works even if the Settings plugin has failed. It lists plugins that failed or are off first, and offers these actions: enable, disable, open the raw configuration file, uninstall, reset failures, forget settings and safe mode.

<figure>
<pre class="term"><span class="t-hi">╭─ Recovery ───────────────────────────────────╮</span>
│ Failed or disabled plugins come first.       │
│                                              │
│ failed   example.demo 0.1.0 3 in 10 min      │
│ enabled  ilium.cost-view 0.1.0               │
│ enabled  ilium.animations 0.1.0              │
│                                              │
│ e enable   d disable   o open config file    │
│ u uninstall   r reset failures               │
│ f forget settings   s safe mode   q close    │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The recovery page.</figcaption>
</figure>

Open the recovery page from the **Open Plugins page** link on any disabled plugin's rows, or from a failed-start notice, or with `Ctrl+B` then `:` and the Recovery entry when Settings is not available. The recovery page has no direct key of its own; those three routes are the only ones.

| Action | Effect | Changes |
|---|---|---|
| Enable | Turns the plugin on and clears its failure count | the on or off flag, the failure window |
| Disable | Turns the plugin off | the on or off flag |
| Reset failures | Clears the failure count without starting the plugin | the failure window |
| Forget settings | Deletes this plugin's saved settings for you and starts it with defaults | settings values only |
| Uninstall | Removes the plugin as in [Install and manage plugins](/pages/user-install-and-manage/) | the plugin |
| Open config file | Opens `config.toml` in an editor | nothing by itself |
| Safe mode | Restarts with third-party plugins off | nothing stored |

- [USR-032] The recovery page MUST stay reachable when the Settings plugin is off or failed, and MUST NOT be replaceable by a plugin. Test: turn off `ilium.core.settings`, open the recovery page, and re-enable it.
- [USR-033] Forget settings MUST delete only that plugin's settings values and MUST NOT delete permissions or data. Test: set a value, forget, and compare the grant file and data folder.

### Find the plugin that causes a problem

If you do not know which plugin is at fault, use the bisect tool. It turns half of your third-party plugins off, asks you whether the problem is still there, and narrows down until one plugin is left.

```sh
ilium plugin bisect start
# check the problem, then answer
ilium plugin bisect good     # the problem is gone with the current set off
ilium plugin bisect bad      # the problem is still there
ilium plugin bisect reset    # stop and put everything back
```

Bisect changes nothing on disk. The plugins it turns off are off for this run, as in safe mode, and `reset` or a restart brings them back. When bisect has found the plugin, it names it and offers to turn it off for good or to open its log.

1. Run `ilium plugin bisect start`.
2. Look at whether the problem is still there. Answer `good` when it is gone, `bad` when it is still there.
3. Repeat until Ilium names one plugin. With 16 plugins this takes at most 4 rounds.
4. Run `ilium plugin bisect reset`.

### Read the plugin log

Every plugin has a log that holds its own lines and the lines that Ilium writes about it: refused calls, failures, restarts and process runs. The log has four columns: time, level, plugin id and message.

```sh
ilium plugin logs com.example.notes --tail 50
ilium plugin logs com.example.notes --level warn --since "2026-10-12 08:00:00"
ilium plugin logs com.example.notes --follow
```

Lines you will often see:

| Line | Meaning |
|---|---|
| `refused <grant> by <function> (declined)` | You said no to that permission. Grant it in the Capabilities tab if you want the feature |
| `refused <grant> by <function> (revoked)` | You turned the permission off |
| `refused <grant> by <function> (not-granted)` | The plugin used a value outside what you granted, such as a host or folder you did not allow |
| `restart in 1 s` or `restart in 5 s` | The plugin stopped and Ilium will start it again |
| `disabled after 3 failures in 10 minutes` | The plugin is off and stays off until you turn it on |
| `dropped N log lines` | The plugin wrote more than 200 lines per second |

The log is plain text. Before you paste it into a bug report, read it. A plugin should not write secrets there, but check.

### Restart rules you will see

When a plugin stops with an error, Ilium starts it again after a delay: 1 s after the first failure, 5 s after the second. At the third failure within 10 minutes it stays off, the row shows **Failed**, and a notice names the plugin. Turning it on again, from the Plugins tab, the recovery page or `ilium plugin enable <id>`, starts counting from zero. `ilium plugin reset-failures <id>` clears the count without starting the plugin. See [Lifecycle](/pages/lifecycle/).

If you changed the plugin in the meantime, for example by updating it, the count also starts from zero.

- [USR-176] The troubleshooting table MUST tell a user whose plugin shows `Blocked` with `trust.key-revoked` or `registry.takedown` that the block is not a failure, that the plugin stays off until `enable --accept-risk` or a signed update, and that the failure counter does not apply. Test: read the page text and assert that the row names both reasons, `--accept-risk` and the update route; block a plugin and assert that `ilium plugin list` shows no failure count for it.
- [USR-100] The plugin MUST restart after 1 s, then after 5 s, and MUST stay off at the third failure within 10 minutes. Test: make a test plugin fail on start and measure.
- [USR-101] Turning a plugin on MUST clear its failure count. Test: reach the stop, turn on, fail once, and assert a restart after 1 s.

### Protective limits you will see

Ilium limits what a plugin can do to your screen, whatever you granted. If you see these effects, a plugin is probably very active:

- **Toasts:** at most 3 per 10 s per plugin. More are dropped. One summary line goes to the log.
- **Input lock:** a plugin can block your keyboard for 2 s at most.
- **Clipboard:** after a plugin reads or writes the clipboard, the footer shows an indicator for 3 s.
- **Log lines:** 200 per second.

### Roll back and repair

| Problem | Command | What it does |
|---|---|---|
| The new version is worse | `ilium plugin rollback <id>` | Activates the previous version |
| The new version cannot read old data | `ilium plugin rollback <id> --restore-backup` | Restores the data copy made before the update. What was written since is lost |
| Store files are damaged | `ilium plugin reinstall <id>` | Rebuilds the store entry from the cached package |
| You want a clean start for one plugin | `ilium plugin uninstall <id> --purge`, then install | Deletes its data, settings and log |
| Many checks at once | `ilium plugin doctor` | Checks the tools, trust store and free disk space |

`ilium plugin doctor` also checks whether the machine allows T2 and T3 plugins and whether the trust store file is readable.

### Report a problem

Give the maintainer of the plugin, not the Ilium project, the report first. Include:

1. The plugin id and version (`ilium plugin list`).
2. Your Ilium version and your system.
3. The last lines of the log, after you read them for secrets.
4. What you did and what you expected.

If you think Ilium itself is at fault, for example a plugin could do something it was not granted, report it to the Ilium project as a security issue and include the log lines.

### Errors and edge cases

| Situation | What happens |
|---|---|
| Safe mode and a policy that blocks plugins | Both apply. The row shows the policy reason first |
| Bisect with one third-party plugin | It names that plugin after the first answer |
| You leave safe mode | Plugins are back as they were. Failed plugins stay failed until you turn them on |
| The log file is missing | The log tab is empty. A new file starts at the next line |
| Two problems at once | Fix one, restart, then check again. Bisect finds one plugin at a time |
| The registry cannot be written | The action fails with `io`, and the state stays as it was |

### See also

- [Plugins for users](/pages/user-plugins-overview/)
- [Install and manage plugins](/pages/user-install-and-manage/)
- [Permissions explained](/pages/user-permissions-explained/)
- [Plugin FAQ](/pages/user-faq/)
- [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/)
- [Settings screens](/pages/settings-screens/)
- [CLI reference](/pages/reference-plugin-cli/)

API version checked: `ilium:plugin@0.1.0`.
