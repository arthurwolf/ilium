---
layout: page
title: Plugin FAQ
permalink: /pages/user-faq/
---

## Plugin FAQ

Short answers to the questions users ask most about plugins. Each answer points to the page that explains it fully.

Contract version: `ilium:plugin@0.1.0`. Requirement ids have the form `[USR-nnn]`, collected in [User and administrator requirements](/pages/reference-requirements-user-admin/).

<div class="note">
<p>Decision status. The platform is not implemented. Answers describe specified behaviour.</p>
</div>

### Contents

- Basics
- Installing
- Permissions
- Safety and privacy
- When things go wrong
- Administrators
- Errors and edge cases
- Requirements

### Basics

**Do I have to use plugins?** No. All 30 core plugins ship with Ilium and work as before. Third-party plugins are optional. See [Plugins for users](/pages/user-plugins-overview/).

**Is there a plugin store?** Only if you set up a registry. Ilium ships none and runs no central store (decisions D3 and D27). See [Plugin store](/pages/user-plugin-store/). A plugin is a `.iliumpkg` file that someone gives you. Ilium never searches for plugins. See [Install and manage plugins](/pages/user-install-and-manage/).

**Do plugins work on Windows and macOS?** Only the bundled plugins. A third-party plugin is refused at install with "Cannot install <name> <version>." The sandbox that makes third-party plugins safe exists on Linux only. See [Plugins for users](/pages/user-plugins-overview/#where-plugins-run).

**What is the difference between T1, T2 and T3?** T1 are plugins from the Ilium project, built in. T2 are signed by a publisher you trust. T3 are unsigned and show a `dev` badge. T2 and T3 run in a sandbox.

**Can I write a plugin?** Yes. Start at [What a plugin is](/pages/getting-started-overview/). This FAQ is for users.

### Installing

**How do I install a plugin?** `ilium plugin install file.iliumpkg`, or Settings > Plugins > Install from file. See [Install and manage plugins](/pages/user-install-and-manage/).

**Do I need administrator rights?** No. Everything is stored in your own folders. A policy file, which only an administrator can write, may limit what you can install.

**What does "trust a publisher" mean?** You tell Ilium that packages signed with a key come from a person or company you believe. Check the fingerprint with them by a second route. Removing a key later does not remove the plugins.

**Why does the install ask so many questions?** Each question is one decision only you can make: trust a key, accept each permission, accept a replacement of a built-in feature.

**Can I install an unsigned plugin?** Yes, with `--allow-unsigned`, but only on Linux. Do it only for a plugin you built or read.

**Does Ilium update plugins by itself?** No. You get the new file and install it. The dialog shows only the new permissions. See [Install and manage plugins](/pages/user-install-and-manage/#update-a-plugin).

### Permissions

**What if I say no to a permission?** The plugin still installs and runs. The calls that need the permission fail, and the plugin may show less. See [Permissions explained](/pages/user-permissions-explained/).

**Can I change my mind later?** Yes, in the plugin's Capabilities tab or with `ilium plugin grants`. A permission you turn off takes effect at once.

**What does HIGH mean?** The permission can read private text, act for you, run programs or change much. HIGH rows start unchecked.

**Are permissions per project?** No. They belong to you and the plugin. Giving `fs.read:project` once lets the plugin read the project folder of whichever project is open.

**Why does a plugin ask for a permission it does not seem to need?** Ask the plugin's author. If there is no good answer, leave the row unchecked.

### Safety and privacy

**Can a plugin read my files?** Only the folders you granted, and its own folders. See [Plugin privacy](/pages/user-privacy/).

**Can a plugin send my terminal text to a website?** Only if you granted both a way to read it and a network host. Think about the combination.

**Does Ilium send anything about my plugins?** No. See [Plugin privacy](/pages/user-privacy/#what-ilium-itself-sends).

**Can a plugin change the install dialog?** No. Ilium draws it. A plugin cannot draw or restyle it.

**Can a plugin hide the recovery page?** No. The kernel recovery page can never be replaced.

### When things go wrong

**A plugin shows Failed.** It failed three times in 10 minutes. Read its log, then turn it on again. See [Troubleshooting plugins](/pages/user-troubleshooting/).

**Ilium misbehaves after I installed a plugin.** Start with `ilium --safe-mode`. All third-party plugins are off for the run. Then use `ilium plugin bisect start` to find the one.

**A pane says the plugin is not installed.** The plugin that made the pane was removed. Install it again to get the pane back, or close the pane.

**An update failed.** Ilium puts the old version back and says "Update failed, rolled back".

**A plugin says "Waiting for <owner>".** It adds to a plugin that is off or missing. Turn the owner on or install it.

**Can I get back my notes after I uninstalled?** Yes, if you did not use `--purge`. Reinstall the same plugin and it reads its data again.

**How do I back up one plugin's data or move it to another computer?** Run `ilium plugin export <id> --out <file>`, install the plugin on the other computer and run `ilium plugin import <id> --from <file>`. Do this before `--purge`, which cannot be undone.

### Administrators

**Can I stop users from installing plugins?** Yes, with the policy file. See [Administration guide](/pages/guide-administration/) and [Plugin policy](/pages/reference-plugin-policy/).

**Can I allow only signed plugins from my company?** Yes. Set `packages.allowed_tiers` and `publishers.allow` in the policy file ([Plugin policy](/pages/reference-plugin-policy/)).

**Can I see what was installed?** The audit log is always on. See [Audit log](/pages/reference-audit-log/) for where it is stored and how to read it.

### Errors and edge cases

| Situation | Answer |
|---|---|
| The question is not here | Read [Troubleshooting plugins](/pages/user-troubleshooting/) or the page for the topic |
| You have a plugin you no longer trust | Turn it off, then uninstall it with `--purge`, then remove the key with `ilium plugin trust remove` |
| You lose the package file | Installed plugins keep working. Keep the file if you want to reinstall on another machine |

### Requirements

- [USR-110] The documentation MUST answer, in this FAQ, whether a store exists, which systems run third-party plugins, what declining a permission does, and how to leave a bad plugin behind. Test: search the page for each of the four answers.

### See also

- [Plugins for users](/pages/user-plugins-overview/)
- [Install and manage plugins](/pages/user-install-and-manage/)
- [Permissions explained](/pages/user-permissions-explained/)
- [Plugin privacy](/pages/user-privacy/)
- [Troubleshooting plugins](/pages/user-troubleshooting/)
- [Administration guide](/pages/guide-administration/)

API version checked: `ilium:plugin@0.1.0`.
