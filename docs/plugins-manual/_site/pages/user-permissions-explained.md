# Permissions explained

## Permissions explained

A plugin can only do what you let it do. Each ability is a permission, called a grant in the technical pages. This page explains in plain words how permissions work, what every permission means, which ones are risky, and what happens when you say no. Read it before you install a plugin that you did not write.

Contract version: `ilium:plugin@0.1.0`. Requirement ids have the form `[USR-nnn]`, collected in [User and administrator requirements](/pages/reference-requirements-user-admin/). The technical table of every grant is [Capability reference](/pages/reference-capabilities/).

<div class="note">
<p>Decision status. The platform is not implemented. The wording of each permission in the review dialog is a working text that the platform owner confirms.</p>
</div>

### Contents

- How permissions work
- What needs no permission
- Risk levels
- Every permission in plain words
- Questions that are not permissions
- Change a permission later
- What happens when a call is refused
- Limits that protect you
- Errors and edge cases
- Requirements

### How permissions work

- A permission belongs to one plugin and one side of it. A plugin has up to three sides: the server side runs in the background, the client side draws things on your screen, and the scene side draws animated backgrounds. A permission given to the server side does not carry to the client side.
- A permission belongs to you, not to a project. You give it once. It applies in every project you open. Permissions are stored in your own Ilium data, never inside a project folder, so a project you share does not share your permissions. See [Plugin files on disk](/pages/reference-plugin-files/).
- A plugin lists what it wants in its manifest. You see that list in the review dialog when you install. A plugin cannot get a permission that it did not list.
- You can say yes to some rows and no to others. The plugin installs either way.
- You can change your mind at any time in the **Capabilities** tab of the plugin, or with `ilium plugin grants`.

- [USR-020] A permission MUST apply to one plugin and one side, MUST be stored per user, and MUST NOT be shared through a project folder. Test: grant a permission to the client side only, then call it from the server side and assert `not-permitted`.
- [USR-021] A plugin MUST NOT obtain a permission that its manifest does not list. Test: call a host function whose permission is not in the manifest and assert that the call traps with `host.capability-not-requested`.

### What needs no permission

Some abilities cannot reach anything that is yours, so they need no permission and never appear in the dialog. Every plugin has them on every side that carries them:

- Writing lines to its own log, and reading the clock.
- Timers that call back into the plugin.
- Keeping its own notes (storage, up to 16 MiB in each bucket) and its own settings values.
- Compression, image decoding, hashing and text rasterizing.
- Reading and writing its own data folder and cache folder, and reading its own package files.
- Sending messages between its own server side and client side.
- Asking Ilium to draw one of its own views again.

Limits apply even to these: a plugin may write at most 200 log lines per second (extra lines are dropped), and at most 16 MiB to each storage bucket. See [Always-granted capabilities](/pages/capability-always-granted/).

### Risk levels

The dialog marks each row as normal or HIGH. HIGH means the ability could do harm or reveal private information if the plugin were careless or dishonest.

| Level | What it means | Start state in the dialog |
|---|---|---|
| Normal | Works inside Ilium, or reads little of yours | checked |
| HIGH | Can read private text, type for you, run programs, reach other sessions or change many things | unchecked |

A quick way to judge a row: ask what the plugin is for. A note-taking plugin that asks to run programs, or a theme that asks to read your terminal history, is asking for more than it should need. Leave the row unchecked, or do not install the plugin.

- [USR-022] The review dialog MUST start every HIGH row unchecked and every normal row checked. Test: open the dialog for a package with both and assert the initial states.

### Every permission in plain words

Names in the first column are the strings you see in the dialog and in `ilium plugin grants`. A name with `<...>` takes an argument: the dialog shows the real value. The last column says what happens if you say no. In all cases the plugin keeps running; only the calls that need the permission fail.

#### The project tree and panes

| Permission | In plain words | Risk |
|---|---|---|
| `tree.read` | See the groups, panes and their names in your project tree | normal |
| `tree.write` | Add, rename or move items you did not ask it to touch | normal |
| `tree.metadata` | Attach its own notes to tree items | normal |
| `tree.decorate` | Add badges and colors to tree rows | normal |
| `tree.restructure` | Reorganize the tree on a large scale | HIGH |
| `tree.attention` | Mark items as needing your attention | normal |
| `panes.read` | See pane sizes, titles and focus | normal |
| `panes.create` | Open new panes | normal |
| `panes.signal` | Send signals to the programs in panes | HIGH |
| `panes.lifecycle` | Close, restart or kill panes | HIGH |
| `panes.provide-kind` | Offer its own kind of pane, like a notes pane | normal |

#### Terminals and agents

| Permission | In plain words | Risk |
|---|---|---|
| `terminal.read-screen` | Read the text you see in a terminal pane | normal |
| `terminal.read-cells` | Read the text with its colors and styles | normal |
| `terminal.history` | Read scrollback, the older lines that scrolled out of view | HIGH |
| `terminal.output-filter` | Change terminal output before you see it, such as hiding noise | normal |
| `terminal.write` | Type into a terminal as if you did | HIGH |
| `terminal.input-lock` | Block your keyboard for a moment while it acts | HIGH |
| `terminal.raw-input` | Receive every key you press in a terminal | HIGH |
| `terminal.pointer` | See mouse movement over a terminal | normal |
| `agent.read` | See which coding agents run, and whether each is working or idle | normal |
| `agent.read-prompts` | Read what you typed to an agent | HIGH |
| `agent.control` | Send commands to an agent | HIGH |
| `agent.signatures` | Add rules that tell Ilium how to recognize an agent | normal |
| `agent.transcript` | Read the saved conversation of an agent | HIGH |

Terminal text can hold passwords, tokens and private notes. Terminal rows are the ones to read with care.

#### Editors, screen and keys

| Permission | In plain words | Risk |
|---|---|---|
| `editor.read` | Read the text of open editor panes | normal |
| `editor.edit` | Change the text of open editor panes | normal |
| `editor.provide` | Offer its own editor, for a new file type | HIGH |
| `ui.panels` | Show its own panels and settings pages | normal |
| `ui.chrome` | Add items to menus, toolbars, status bar and tree rows | normal |
| `keys.intercept` | See your keys before Ilium does and swallow some | HIGH |
| `keymap.read` | Read your key bindings | normal |
| `commands.invoke:<pattern>` | Run Ilium commands. The pattern names which ones. A bare `*` means all | HIGH when `*`, otherwise normal |

#### Files and folders

| Permission | In plain words | Risk |
|---|---|---|
| `fs.read:<scope>` | Read files in one named place. The place is a scope, see below | normal; HIGH for agent transcripts |
| `fs.write:<scope>` | Create and change files in one named place | HIGH, except its own data and cache |
| `fs.watch:<scope>` | Be told when files in a place change | normal |
| `fs.pick` | Ask you to choose a file or folder, then read that one only | normal |

The scopes you will see: `project` (the project folder you launched Ilium in), `worktrees` (the folder of Git worktrees), `agent-sessions:<agent>` (an agent's saved conversations), `agent-config:<agent>` (an agent's settings), `minecraft-saves`, and `path:<template>` (a custom place built from your home, config, data, cache, project or worktree folders). Its own `data`, `cache` and `package` folders are always allowed. A plugin sees the granted place and nothing around it.

Reading the project folder is a normal-risk permission, because the plugin cannot read anything outside it. Writing there is HIGH.

#### Network, programs and background work

| Permission | In plain words | Risk |
|---|---|---|
| `http:<host-pattern>` | Talk to one named website or server | normal |
| `http:setting:<key>` | Talk to the host that you type into one of its settings | normal; asks you for each new host |
| `jobs.run` | Run work in the background | normal |
| `process.run:<program>` | Run one named program. Never a wildcard | HIGH |
| `process.long-lived` | Keep a started program running in the background. Needs `process.run` | HIGH |

A started program runs in the same sandbox as the plugin, with no network. Stopping the plugin ends the program.

#### AI, services and messages

| Permission | In plain words | Risk |
|---|---|---|
| `inference` | Use your configured AI providers to answer a prompt | normal |
| `inference.provide` | Offer itself as an AI provider | normal |
| `inference.embed` | Turn text into embeddings with your AI providers | normal |
| `services.use:<name>` | Call a named service of another plugin | normal |
| `services.provide:<name>` | Offer a named service to other plugins | normal |
| `messages.peer:<plugin-id>` | Exchange messages with one named other plugin | normal |
| `topics.subscribe:<pattern>` | Listen to a named stream of events | normal; HIGH for a bare `*` |
| `journal.read` | Read the host's record of recent events | normal |
| `extensions.provide:<id>/<point>` | Offer its own extension point | normal |
| `extensions.contribute:<id>/<point>` | Fill another plugin's extension point | normal; HIGH if the point is marked high |

#### Sound, alerts, links and the clipboard

| Permission | In plain words | Risk |
|---|---|---|
| `sound.play` | Play sounds | normal |
| `notify.desktop` | Send desktop notifications | normal |
| `notify.toast` | Show small messages inside Ilium | normal |
| `open-external` | Ask to open a link; you confirm each site | normal |
| `clipboard.read` | Read what you copied | HIGH |
| `clipboard.write` | Put text on the clipboard | normal |
| `audio.capture` | Use the microphone | HIGH |
| `audio.loopback` | Record the sound your computer plays | HIGH |
| `audio.playback` | Play audio streams | normal |
| `media.video` | Show and decode video | normal |
| `location.observer` | Know your approximate location for scenes | HIGH |
| `location.geocode` | Turn a place name into a position, through an online service | normal; asks you first |

#### Sessions, workspace and settings

| Permission | In plain words | Risk |
|---|---|---|
| `sessions.cross` | Reach other Ilium sessions on this machine | HIGH |
| `workspace.manage` | Create and remove worktrees and workspaces | HIGH |
| `cli.contribute` | Add `ilium` commands of its own | normal |
| `http-api.route:<path>` | Answer requests at one path of the local HTTP API | normal |
| `settings.contribute:<tab>` | Add rows to one named built-in Settings tab | normal |
| `settings.read-kernel:<tab>` | Read the values of one built-in Settings tab | normal; HIGH for the api tab |
| `settings.read-all`, `settings.write-all` | Read or change every setting. Bundled plugins only | HIGH |
| `settings.write-kernel:<tab>` | Change the values of one built-in Settings tab. Bundled plugins only, one holder per tab | HIGH |
| `plugins.manage` | Install, remove and turn plugins on and off. Bundled plugins only | HIGH |
| `help.read` | Read the Ilium help topics | normal |
| `gpu.kernels` | Run graphics code. Development builds only | normal |

Rows marked bundled-only or development-only are never shown to you for third-party plugins. A third-party package that asks for them fails to install with `manifest.grant-tier` for a bundled-only row, or with `manifest.grant-unknown` for `gpu.kernels`, which does not exist in a release build.

- [USR-080] The Capabilities tab MUST show the plain wording, the grant string, the risk and the state (granted, declined, revoked) of every permission the plugin requests. Test: open the tab for a plugin with ten requested grants and compare.
- [USR-081] A package that requests a bundled-only grant MUST fail to install with `manifest.grant-tier`. Test: request `plugins.manage` in a T2 package.
- [USR-082] A grant string MUST be shown exactly as it will be matched, including its argument, so that the user can see the host, program or place. Test: request `http:api.example.com` and assert the dialog shows that string.

### Questions that are not permissions

Four features ask you at the moment of first use, because Ilium only knows the target then. The answer is stored with the plugin and you can withdraw it in the Capabilities tab.

| Feature | When you are asked | If you say no |
|---|---|---|
| `open-external` | The first time a plugin opens a link to each website | The link does not open |
| `http:setting:<key>` | When the setting holds a host the plugin has not used before | The request fails and the host is not contacted |
| `location.geocode` | The first time a plugin looks up a place. The online service is Photon, with Nominatim as fallback | Calls return `not-permitted` |
| Bundled plugin with HIGH rows | Once, at its first start | It runs without them. You are asked again at the next start, at most 3 more times |

- [USR-083] Ilium MUST ask the user before it opens a link from a plugin, for each target website and plugin, and MUST remember the answer. Test: open two links to the same site and one to another site and count prompts: two.
- [USR-084] Ilium MUST NOT contact a host from a `http:setting` grant until the user has confirmed that host. Test: set a new host in a setting, call, and assert no connection before the answer.

### Change a permission later

Open **Settings > Plugins**, select the plugin, choose **Capabilities**. Each row has a switch. Or run:

```sh
ilium plugin grants com.example.notes
ilium plugin grants com.example.notes --revoke clipboard.read
ilium plugin grants com.example.notes --grant clipboard.read
```

Turning a permission off takes effect at once. A call that is running when you turn it off returns `revoked`, and the plugin is started again with the smaller set, so its screens refresh. Turning a HIGH permission on asks you to confirm. A plugin cannot give itself a permission that its manifest does not list: `--grant` fails with `grants.not-requested`.

- [USR-085] A revoke MUST take effect for calls started after it, MUST make calls in flight return `revoked`, and MUST restart the plugin with the smaller set. Test: start a long call, revoke, and assert `revoked` and a restart in the log.
- [USR-086] A HIGH grant MUST ask for confirmation in the attached client. Test: run `ilium plugin grants <id> --grant terminal.write` with a client attached and assert the prompt.

### What happens when a call is refused

The plugin keeps running. The call returns `not-permitted` (declined), or `revoked` (you turned it off). The host writes one line into the plugin's log:

```text
2026-10-12 09:14:02 warn  com.example.notes refused tree.restructure by restructure (declined)
```

The form is `refused <grant> by <function> (<reason>)`, and the reason is one word such as `declined`, `revoked`, `not-granted`, `policy` or `offline`. You can read the log in the plugin's **Log** tab or with `ilium plugin logs <id>`. A plugin that depends on a declined permission may show an empty panel or a message; that is the plugin's own design, not an Ilium fault.

A call to something the plugin never listed is different. It is a bug in the plugin, not a refusal. The call traps, the plugin instance ends, and Ilium restarts it after a delay. Repeated traps turn the plugin off. See [Troubleshooting plugins](/pages/user-troubleshooting/).

### Limits that protect you

Even with permissions, Ilium limits what a plugin can do to your screen and session:

| Limit | Value |
|---|---|
| Toasts from one plugin | 3 per 10 s |
| Input lock (`terminal.input-lock`) | at most 2 s at a time |
| Clipboard use | a footer indicator stays 3 s after each use |
| Log lines | 200 per second |
| Storage | 16 MiB per bucket |

- [USR-087] Ilium MUST show a footer indicator for 3 s after a plugin reads or writes the clipboard. Test: read the clipboard from a test plugin and watch the footer.
- [USR-088] Ilium MUST drop toasts beyond 3 per 10 s per plugin and MUST end an input lock after 2 s. Test: send ten toasts at once, count the visible ones; lock input and time the release.

### Errors and edge cases

| Situation | What happens |
|---|---|
| You decline every row | The plugin installs with the always-granted set only |
| The plugin lists a permission that does not exist | Install fails with `manifest.grant-unknown` |
| The plugin asks for a permission its side cannot hold | Install fails with `manifest.grant-side` |
| An update asks for a new permission | Only the new row is shown |
| Your administrator forbids a permission | The grant row in the review dialog and in the Capabilities tab shows "Blocked by your administrator", off and locked. A whole plugin that the policy blocks shows "Blocked by policy" in the plugin list. See [Plugin policy](/pages/reference-plugin-policy/) |
| You uninstall, then reinstall | Permissions are asked again |

### See also

- [Plugins for users](/pages/user-plugins-overview/)
- [Install and manage plugins](/pages/user-install-and-manage/)
- [Plugin privacy](/pages/user-privacy/)
- [Capability reference](/pages/reference-capabilities/)
- [Security](/pages/security/)

API version checked: `ilium:plugin@0.1.0`.
