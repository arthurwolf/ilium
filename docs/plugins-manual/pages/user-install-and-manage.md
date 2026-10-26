---
layout: page
title: Install and manage plugins
permalink: /pages/user-install-and-manage/
---

## Install and manage plugins

This page shows how to install a plugin from a file, trust its publisher, turn it on and off, update it, remove it, and see what it is doing. It covers the Plugins tab in Settings and the matching `ilium plugin` commands. The exact rules for each step are in [Install, update and uninstall](/pages/reference-install-update-uninstall/) and the [CLI reference](/pages/reference-plugin-cli/); this page keeps to what you see and what you decide.

Contract version: `ilium:plugin@0.1.0`. Requirement ids have the form `[USR-nnn]`, collected in [User and administrator requirements](/pages/reference-requirements-user-admin/).

<div class="note">
<p>Decision status. The platform is not implemented. Ilium ships no store, marketplace or registry; a registry is optional (decision D27). A plugin is a local <code>.iliumpkg</code> file that someone gave you.</p>
</div>

### Contents

- Before you start
- Install from a file
- Trust a publisher
- Review the permissions
- First start of a bundled plugin
- Turn a plugin on or off
- Update a plugin
- Remove a plugin
- Look at a plugin
- Settings and the recovery page
- Errors
- Edge cases
- Requirements

### Before you start

You need a package file, for example `notes.iliumpkg`. Check three things first.

- You are on Linux. On Windows and macOS only the bundled plugins run, and the install is refused with "Cannot install <name> <version>."
- You know who made the file. Ilium cannot tell whether a plugin is useful or honest. It can only tell whether the file is intact and whether a key you trust signed it.
- Your administrator has not locked plugins on this machine. If they have, the install says "Blocked by policy". See [Administration guide](/pages/guide-administration/).

Installing needs no network access and does not need administrator rights. Everything is stored in your own folders. See [Plugin files on disk](/pages/reference-plugin-files/).

### Install from a file

There are two ways. Both run the same checks.

1. In the tree, press `Ctrl+B` then `:` to open Settings. Choose **Plugins**, select **Install from file**, and enter the path of the file. Press `Enter`.
2. In a terminal: `ilium plugin install notes.iliumpkg`.

Ilium checks the file before any code in it runs. A file that fails a check is not installed, and the error names the check. The checks run in this order, and the first failure stops the install:

1. The file is a plugin package and every file inside has the length and hash that the package lists.
2. The signature, if any, verifies, and the signer is a key you trust or that you are now asked to trust.
3. The version of the plugin interface fits this version of Ilium.
4. The description of the plugin (the manifest) is valid.
5. You see the permission review and answer it.
6. Your administrator's policy allows this plugin.
7. The files are unpacked and prepared, then recorded.

The complete list with every error code is in [Install, update and uninstall](/pages/reference-install-update-uninstall/#the-install-pipeline).

- [USR-010] Installing MUST NOT run any code of the package before the user has answered the permission review. Test: install a package whose start function writes a file, decline the review, and assert that the file does not exist.
- [USR-011] A failed install MUST leave the installed plugins and their settings unchanged and MUST print one error that names the failed check. Test: install a package with a bad file hash, list plugins before and after, and compare.

Without a client to show the review, for example in a script with no attached Ilium client, the install fails with `install.review-required` unless you pass `--yes`. With a client attached, `--yes` accepts all normal permissions, and a high-risk permission is accepted only if you name it with `--grant`.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin install notes.iliumpkg
Example Labs, signed (T2), 3F2A…9C01
Installed Notes 1.2.0.
It is on. 4 of 5 permissions given.
<span class="t-dim">Open Settings &gt; Plugins to change them.</span>
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A successful install on the command line.</figcaption>
</figure>

### Trust a publisher

A signed (T2) package carries the signature of a publisher. A publisher is a person or company with a key. Ilium shows the key's fingerprint, a short code that identifies the key. Trust a publisher only if you have checked the fingerprint with them by another route, for example on their website over HTTPS or by phone. A fingerprint on the same page as the download proves nothing.

<figure>
<pre class="term"><span class="t-hi">╭─ Trust this publisher? ──────────────────────╮</span>
│ Example Labs signed Notes 1.2.0.             │
│                                              │
│ Key fingerprint:                             │
│ 3F2A 91C0 7B44 D1E8 0A5C 6E92 B3F7 48AD      │
│ 5C10 E2B9 77D3 04FA 8B61 C9E5 2D38 9C01      │
│                                              │
│ You have not trusted this key before.        │
│ Trusting it lets you install and update      │
│ other packages that Example Labs signs.      │
│                                              │
│   [ Trust ]   [ Details ]   [ Cancel ]       │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The trust prompt for an unknown key.</figcaption>
</figure>

Choose **Details** to see the full fingerprint, 64 lowercase hexadecimal characters in 16 groups of 4. Choose **Cancel** and the install stops: nothing is installed. A trusted key is stored in your own trust list. You can manage the list with `ilium plugin trust list`, `ilium plugin trust add <publisher.pub>` and `ilium plugin trust remove <publisher>`. Removing a key does not remove the plugins that it signed. They stay installed and show `untrusted`; Ilium does not update them silently, because updates are always your decision.

- [USR-120] Ilium MUST show the full key fingerprint (64 lowercase hexadecimal characters in groups of 4) in the prompt, and the short form (the first 8 characters) only beside a full form or in the trust list (SEC-177), and MUST NOT trust a key without an explicit user answer. Test: install a package signed by an unknown key and assert that the prompt appears and that declining installs nothing.
- [USR-121] Removing a trusted key MUST NOT uninstall any plugin or change its files, grants or data. The plugins it signed stop loading at the next activation (SEC-144) and MUST be marked plugins that it signed as `untrusted` in `ilium plugin list`. Test: remove the key and list.
- [USR-122] An update of a T2 plugin MUST be signed by a trusted key. A key that signed the installed version MUST NOT be replaced by a different key unless the old key endorsed it with a rotation statement (SEC-142); otherwise the update is refused with `package.publisher-key-changed`. Test: install version 1 signed by key A, then offer version 2 signed by unendorsed key B, and assert that the update is refused with `package.publisher-key-changed`.

Unsigned packages (T3) carry no signature. They need `--allow-unsigned`. They get a `dev` badge in the list and the permissions of a T2 plugin at most. Install one only if you built it or reviewed it yourself.

### Review the permissions

After the checks, Ilium shows one dialog with every permission the plugin asks for. The dialog is drawn by Ilium. A plugin cannot draw it or change it. Rows marked HIGH start unchecked. Leave a row unchecked if you do not see why the plugin needs it. The plugin installs anyway, and calls that need that permission fail with `not-permitted`. The dialog and the wording of every row are in [Security](/pages/security/#verifying-a-package). The plain meaning of every row is in [Permissions explained](/pages/user-permissions-explained/).

A package that fills other plugins' extension points shows an extra group, **Adds to other plugins**. A package that replaces a core plugin asks one more question. Read both carefully: they change what Ilium does by itself. See [The Extensions tab](/pages/user-extensions-tab/).

- [USR-012] The review dialog MUST list every requested permission, MUST start HIGH rows unchecked, and MUST record exactly the checked rows. Test: install a package with three normal and two HIGH rows, accept the defaults, and assert that the grant file holds three granted and two declined entries.
- [USR-013] The plugin MUST be on after a successful install, unless the user chose to leave it off. Test: install, then run `ilium plugin list` and assert the state.

### First start of a bundled plugin

Bundled plugins (T1) need no install. A few of them ask for a high-risk ability the first time they need it, for example to read your terminal history. Ilium shows one prompt for the plugin with its high-risk rows.

<figure>
<pre class="term"><span class="t-hi">╭─ First start: Voice ─────────────────────────╮</span>
│ Voice wants these extra permissions:         │
│                                              │
│ [ ] Use the microphone <span class="t-dim">…………………</span> audio.capture │
│ [ ] Run the program say <span class="t-dim">…………</span> process.run:say │
│                                              │
│ You can change this later in Settings &gt;      │
│ Plugins &gt; Voice &gt; Capabilities.              │
│                                              │
│  Enter Allow selected      Esc Not now       │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The first-start prompt of a bundled plugin.</figcaption>
</figure>

**Allow selected** gives the checked rows. **Not now** gives none and asks again at the next start. It asks again at the next start, up to three **Not now** answers in total, then stops asking. The plugin works without those abilities, with those features unavailable. You can give them later in the Capabilities tab.

- [USR-014] The first-start prompt MUST appear at most once per start, MUST NOT block other plugins, and after **Not now** MUST stop appearing after the third **Not now** answer (three in total, counted per plugin, as in SEC-111). Test: answer Not now at three consecutive starts and assert that the fourth start shows no prompt.

### Turn a plugin on or off

Select the plugin in **Settings > Plugins** and press `Space`. Or run `ilium plugin disable <id>` and `ilium plugin enable <id>`. Turning a plugin off stops it, hides what it added, and keeps its permissions, settings and data. Turning it on starts it again and resets its failure count.

If a plugin keeps failing, Ilium turns it off by itself. It restarts a failing plugin after 1 s, then after 5 s, and at the third failure within 10 minutes it stays off and the row shows the reason. Turning it on again resets the count. See [Lifecycle](/pages/lifecycle/).

Row states in the Plugins tab are defined once, in [Settings, plugin rows](/pages/core-plugin-settings/#plugin-row-states). They are `On`, `Off`, `Failed`, `Restarting`, `Waiting for <owner>`, `Blocked by policy`, `Blocked` (key revoked or registry takedown) and `Refused`. A plugin that shows `Failed` is explained in [Plugin health for users](/pages/user-plugin-health/).

### A plugin that shows Blocked

A plugin shows **Blocked** when its publisher key was revoked as `compromised`, or when the registry took down the version you have installed. Ilium stops the plugin at once and keeps its permissions, settings and data. The row shows the reason: `trust.key-revoked` or `registry.takedown`, with the registry's reason text for a takedown. The state survives a restart.

To use the plugin again, run `ilium plugin enable <id> --accept-risk`, or install an update signed by another trusted key of the same publisher. A plain `enable` without `--accept-risk` is refused with `plugin.blocked`. There is no Install button for a blocked plugin in the store.

- [USR-174] A plugin whose key was revoked as `compromised`, or whose installed version a registry takedown names, MUST stop at once, MUST show `Blocked` with the reason `trust.key-revoked` or `registry.takedown`, and MUST keep its grants, settings and data. Test: revoke the signing key as `compromised`, assert that the plugin is stopped and the row says `Blocked`, then compare the grant file, settings and data folder before and after.
- [USR-175] `ilium plugin enable <id>` on a blocked plugin MUST fail with `plugin.blocked` and leave it blocked. `ilium plugin enable <id> --accept-risk` MUST turn it on and clear the block. Test: enable without the flag and assert the error and the state, then enable with the flag and assert `On`.

### Update a plugin

There is no automatic update. To update, get the new package file and install it: `ilium plugin install notes-1.3.iliumpkg`, or **Install from file** in the Plugins tab. Ilium sees that the id is installed and treats it as an update. The review dialog then shows only what is new. Permissions you already gave stay as they are.

- A newer version asks for new permissions: only the new rows appear. New HIGH rows start unchecked.
- The update fails to start: Ilium puts the old version back and shows `Update failed, rolled back`. Your data is restored from the copy made before the update, if a data migration ran.
- A lower version needs `--allow-downgrade`. Ilium refuses a downgrade if the stored data is newer than the old version can read.

`ilium plugin update <file>` does the same as install but fails if the id is not yet installed. `ilium plugin rollback <id>` returns to the previous version.

- [USR-123] An update MUST show only permissions that the installed version did not request, and MUST keep the answers already given. Test: update from a package with two grants to one with three and assert one new row.
- [USR-124] A failed update MUST restore the previous version and its data and MUST show "Update failed, rolled back". Test: update to a package whose activation fails and assert the version in `ilium plugin list`.
- [USR-125] Ilium MUST NOT check for, download or install updates by itself unless the user has set `[plugins] auto_update` to `notify` or `install`; the default is `off`. Test: with two plugins installed and the default setting, run for one hour with request logging and assert that no update request is made; then set `auto_update = notify` and assert that a check is made.

### Remove a plugin

Select the plugin and press `d`, or run `ilium plugin uninstall <id>`. Ilium asks what to do with the plugin's data when a terminal is attached.

<figure>
<pre class="term"><span class="t-hi">╭─ Remove Notes? ──────────────────────────────╮</span>
│ Notes 1.2.0 will be removed.                 │
│                                              │
│ Its permissions and cache are deleted.       │
│ Plugins that wait for it will pause.         │
│                                              │
│ What about its data and settings?            │
│  ( ) Keep them for a later reinstall         │
│  ( ) Delete them now (purge)                 │
│                                              │
│   [ Remove ]   [ Cancel ]                    │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The uninstall dialog.</figcaption>
</figure>

On the command line, `--keep-data` keeps the data and `--purge` deletes data, settings and log. The two flags exclude each other. Without a terminal and without a flag, the data is kept. A reinstall of the same id asks for permissions again and may read the kept data. The table of what each choice deletes is in [Plugin files on disk](/pages/reference-plugin-files/#what-each-action-leaves-behind).

To keep a copy of a plugin's data before you purge it or move to another machine, run `ilium plugin export <id> --out <file>`. On the new machine install the same plugin and run `ilium plugin import <id> --from <file>`. The archive holds data, storage and settings values, never permissions or keys. A purge in a terminal reminds you of this command first.

Removing a plugin has effects on other plugins. They stop being fed by it but they are not deleted:

| What depended on the removed plugin | What happens |
|---|---|
| A plugin that fills an extension point owned by the removed plugin | It becomes dormant and shows "Waiting for <owner>". It wakes up when the owner is installed again |
| A plugin that requires a service of the removed plugin | It shows "Waiting for <owner>" and its calls to the service return `unavailable` |
| An extension that the removed plugin filled in another plugin | It disappears from that point at once |
| A core plugin that the removed plugin replaced | The core plugin turns back on. Its own data was never touched |
| A pane of a kind the removed plugin provided | The pane stays in the tree and shows a placeholder: "Plugin <name> is not installed" with the kind and the plugin id |
| Keys, menu items, status segments and toolbar buttons the plugin added | They disappear |
| Your order of extensions for the plugin's points | The entries for the removed plugin are deleted |

- [USR-140] Uninstall MUST first mark the plugin as being removed, then stop it, then delete files, and after a crash the next start MUST finish the removal. Test: kill the host after the mark and start again.
- [USR-141] Uninstall MUST NOT delete the data of any other plugin and MUST NOT remove trusted keys. Test: compare the other plugins' folders before and after.
- [USR-142] A pane whose plugin is gone MUST stay in the tree with a placeholder and MUST come back to life when the plugin is installed again. Test: remove, check the placeholder, reinstall, check the pane.
- [USR-143] Plugins that depend on the removed plugin MUST become dormant, not failed, and MUST wake up when it returns. Test: remove the owner, check the state of the dependent plugin, reinstall the owner.

<figure>
<pre class="term"><span class="t-dim">Plugin Notes is not installed</span>
 
This pane was made by the plugin
com.example.notes (kind: notes).
 
Install the plugin again to see it,
or close this pane.
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A pane left behind by a removed plugin.</figcaption>
</figure>

### Look at a plugin

| I want to see | In Settings > Plugins | On the command line |
|---|---|---|
| All plugins and their states | the list | `ilium plugin list` |
| One plugin in detail | select it, `Enter` | `ilium plugin info <id>` |
| Its permissions and their state | the Capabilities tab | `ilium plugin grants <id>` |
| What it logged | the Log tab | `ilium plugin logs <id> --tail 50` |
| How much it uses | the Details tab | `ilium plugin stats <id>` |

`ilium plugin list` prints one line per plugin, with the id, version, tier, state and the count of permissions given. Add `--json` for scripts.

### Settings and the recovery page

Each plugin can have its own settings pages inside Settings. Their values are saved as you change them. If a plugin's settings page cannot open, or a bad value stops Ilium from starting the plugin, the recovery page lists the plugin with seven actions, among them **Disable** and **Forget settings**. [Settings screens](/pages/settings-screens/#the-kernel-recovery-page) lists all seven. Forget settings deletes that plugin's values for this user and starts the plugin again with its defaults. It does not touch permissions or data. See [Settings screens](/pages/settings-screens/).

### Errors

| What you see | Code | What to do |
|---|---|---|
| Cannot install <name> <version>. (on Windows or macOS) | `package.platform-unsupported` | Use Linux for third-party plugins |
| The file is damaged | `package.manifest-missing` or `package.entry-unexpected`, `manifest.file-hash` | Download the file again and compare its checksum |
| The signature does not verify | `manifest.signature-invalid` | Do not install. Ask the publisher |
| The publisher key is not trusted and you declined | `trust.declined` | Trust the key after checking its fingerprint |
| The package is unsigned | `install.unsigned` | Add `--allow-unsigned` only for packages you reviewed |
| The plugin is for another interface version | `package.api-unsupported` | Get a version for your Ilium |
| Blocked by policy | `policy.blocked` | Ask your administrator |
| You declined the review | `install.review-declined` | Run the install again and accept what you need |
| Another install is running | `install.locked` | Wait and try again |
| The disk is full | `install.io` | Free space. Nothing was changed |

### Edge cases

- **Installing the same file twice** does nothing and succeeds. A different file with the same version fails with `package.version-reused`.
- **Two Ilium windows** share the same plugins. A change in one appears in the other within 2 s.
- **The file moves while installing.** Ilium copies it first. Moving it does not harm the install.
- **A plugin id that starts with `ilium.`** is reserved. A third-party package with such an id is refused.
- **A plugin that wants a replaced core plugin** needs your confirmation. Declining installs the plugin without the replacement.

### See also

- [Plugins for users](/pages/user-plugins-overview/)
- [Permissions explained](/pages/user-permissions-explained/)
- [The Extensions tab](/pages/user-extensions-tab/)
- [Troubleshooting plugins](/pages/user-troubleshooting/)
- [Install, update and uninstall](/pages/reference-install-update-uninstall/)
- [CLI reference](/pages/reference-plugin-cli/)
- [Security](/pages/security/)
- [Plugin files on disk](/pages/reference-plugin-files/)

API version checked: `ilium:plugin@0.1.0`.
