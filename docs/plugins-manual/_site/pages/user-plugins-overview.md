# Plugins for users

## Plugins for users

This page is the start of the user and administrator manual for plugins. It explains in plain words what a plugin is, which plugins you already have, what you can do with them, and what can and cannot go wrong. If you write plugins, start with [What a plugin is](/pages/getting-started-overview/) instead.

Contract version: `ilium:plugin@0.1.0`. Requirement ids have the form `[USR-nnn]`, collected in [User and administrator requirements](/pages/reference-requirements-user-admin/).

<div class="note">
<p>Decision status. The platform is not implemented yet. This page describes the behaviour that the manual specifies. The text is the specification that tests are derived from.</p>
</div>

### Contents

- What a plugin is
- The plugins you already have
- Where plugins come from
- Where plugins run
- What a plugin can do to you
- Where to manage plugins
- Common tasks
- Errors and edge cases
- Requirements

### What a plugin is

A plugin is a small program that adds to Ilium. It can add a pane, a setting, a command, a sound, a background scene or a step in something Ilium already does. A plugin is not a separate application. It runs inside Ilium, in a walled-off space, and it can only do what you allow.

Every feature that Ilium has today is also a plugin. Ilium ships and maintains 30 of them, called core plugins, and their ids start with `ilium.core.`. When you read "tree", "layout" or "Smart Copy" in the Ilium documentation, you are reading about a core plugin. You use them exactly as before.

- [USR-001] The user documentation MUST state that all 30 core plugins ship with Ilium and need no install. Test: search the page for the count 30 and the phrase "ship with Ilium".

### The plugins you already have

| Kind | Id starts with | Who made it | Install needed | Runs where | How trusted |
|---|---|---|---|---|---|
| Core plugin | `ilium.core.` | Ilium | no | inside Ilium | fully (T1) |
| Bundled plugin | `ilium.` (not `ilium.core.`) | Ilium | no | inside Ilium | fully (T1) |
| Signed plugin | any other id | a publisher you trust | yes | in a sandbox | by signature (T2) |
| Unsigned plugin | any other id | anyone | yes | in a sandbox | not trusted (T3) |

The tier names T1, T2 and T3 appear in the Plugins screen and in `ilium plugin list`. A T1 plugin is made by the Ilium project. It is not sandboxed, because it is part of the program, but the same permission rules apply to the things it asks you for. A T2 plugin carries a signature from a publisher key that you chose to trust. A T3 plugin has no signature. Ilium shows a `dev` badge on it and asks you to confirm the install with `--allow-unsigned`.

Read [Core plugins](/pages/user-core-plugins/) for the list of the 30 and what each one does.

### Where plugins come from

Ilium ships with no store, marketplace or registry (decision D27); you may add a registry. A plugin is a single file that ends in `.iliumpkg`. Someone gives it to you: a colleague, a website, a repository. You install the file on your own machine. Ilium never searches for plugins, never downloads one by itself and never updates one by itself.

- [USR-002] Ilium MUST NOT fetch, list, recommend or update plugins from the network on its own. Test: run Ilium with all network traffic logged for one hour with two plugins installed and assert that no request is made for a plugin package or a plugin list.

Because the file comes from someone else, Ilium checks it before anything runs. See [Install and manage plugins](/pages/user-install-and-manage/).

### Where plugins run

Third-party plugins (T2 and T3) run only on Linux, in a sandbox made of three parts: a private view of the file system (bubblewrap), a limit on processor and memory use (cgroups) and a filter on system calls (seccomp). On Windows and macOS the install is refused, and only the bundled plugins run. The refusal reads "Cannot install <name> <version>." and gives the reason.

| System | Core and bundled plugins | Signed and unsigned plugins |
|---|---|---|
| Linux | yes | yes |
| macOS | yes | refused at install |
| Windows | yes | refused at install |

### What a plugin can do to you

Think of a plugin as a guest in a house with locked rooms. The guest can sit in the hall (it can show a panel and store its own notes). Every other room has a lock, and you hold the key. The locks are called grants. A grant is one specific ability, for example "read the text of a terminal pane" or "talk to the host api.example.com". Ilium shows the list when you install the plugin. You can leave a box unchecked. The plugin still installs, and the ability stays locked.

A short list of abilities needs no grant, because they cannot reach anything of yours: writing its own log, reading the clock, using timers, keeping its own settings and notes, and reading and writing its own data and cache folders. The full list and the plain meaning of each grant is in [Permissions explained](/pages/user-permissions-explained/).

What a plugin can never do, whatever you grant:

- Read or write a file outside the folders you granted.
- Start a program that you did not name in a grant.
- Reach the network except hosts you named.
- Draw outside its own panels, or draw the install dialog.
- Stop Ilium from showing you the recovery page.

- [USR-003] Ilium MUST show every grant a plugin requests before the plugin runs, and a grant the user does not give MUST stay locked. Test: install a plugin that requests five grants, leave two unchecked, and assert that calls that need those two return `not-permitted`.

### Where to manage plugins

Open **Settings** with `Ctrl+B` then `:`, or press the gear in the tree footer. Choose the **Plugins** tab. You see one row for each plugin. From there you can turn a plugin on or off, read its permissions and change them, see its log, update it and remove it. The same actions exist on the command line as `ilium plugin ...`. See [Install and manage plugins](/pages/user-install-and-manage/) and [Settings screens](/pages/settings-screens/).

<figure>
<pre class="term"><span class="t-hi">╭─ Plugins ────────────────────────────────────╮</span>
│ Filter: all plugins                          │
│                                              │
│ ilium.core.tree <span class="t-dim">…………………………………………………………………</span> <span class="t-ok">On</span> │
│ ilium.core.layout <span class="t-dim">……………………………………………………………</span> <span class="t-ok">On</span> │
│ com.example.notes (T2) <span class="t-dim">………………………………………………</span> <span class="t-ok">On</span> │
│ com.example.old (T3) <span class="t-dim">…………………………………………………</span> <span class="t-dim">Off</span> │
│                                              │
│ Enter details  Space on or off  d remove     │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The Plugins tab with three plugins.</figcaption>
</figure>

### Common tasks

| I want to | Do this | Page |
|---|---|---|
| Add a plugin from a file | `ilium plugin install <file>` or Settings > Plugins > Install from file | [Install and manage plugins](/pages/user-install-and-manage/) |
| See what a plugin may do | Select it, then the Capabilities tab | [Permissions explained](/pages/user-permissions-explained/) |
| Turn a plugin off without removing it | Space on its row, or `ilium plugin disable <id>` | [Install and manage plugins](/pages/user-install-and-manage/) |
| Remove a plugin | `ilium plugin uninstall <id>` | [Install and manage plugins](/pages/user-install-and-manage/) |
| Fix Ilium after a bad plugin | Start with `ilium --safe-mode` | [Troubleshooting plugins](/pages/user-troubleshooting/) |
| Change the order of things plugins add | Settings > Plugins > Extensions | [The Extensions tab](/pages/user-extensions-tab/) |
| Know what is sent where | Read the privacy page | [Plugin privacy](/pages/user-privacy/) |
| Lock plugins on a shared machine | Write a policy file | [Administration guide](/pages/guide-administration/) |
| Find a quick answer | Questions and answers | [Plugin FAQ](/pages/user-faq/) |

### Errors and edge cases

| Situation | What happens |
|---|---|
| You install on Windows or macOS | The install is refused: "Cannot install <name> <version>." Core and bundled plugins keep working |
| A plugin crashes | Ilium restarts it after 1 s, then 5 s, and turns it off at the third failure in 10 minutes |
| You decline a grant | The plugin keeps running and the calls that need the grant return `not-permitted` |
| You remove a plugin that others depend on | The others show "waiting for <owner>" and wake up again when the owner returns |
| A plugin hangs or floods the screen | Ilium limits it (toasts, input lock, log lines) and you can turn it off from the Plugins tab |

### See also

- [Install and manage plugins](/pages/user-install-and-manage/)
- [Permissions explained](/pages/user-permissions-explained/)
- [Core plugins](/pages/user-core-plugins/)
- [The Extensions tab](/pages/user-extensions-tab/)
- [Troubleshooting plugins](/pages/user-troubleshooting/)
- [Plugin privacy](/pages/user-privacy/)
- [Plugin FAQ](/pages/user-faq/)
- [Administration guide](/pages/guide-administration/)
- [Plugin files on disk](/pages/reference-plugin-files/)

API version checked: `ilium:plugin@0.1.0`.
