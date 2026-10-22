---
layout: page
title: Shell completion and man pages
permalink: /pages/reference-cli-completion/
---

## Shell completion and man pages

The `ilium` command line has commands from three sources: the kernel, the core plugins, and the plugins you install. Tab completion and offline man pages let a user or an operator find each of them without a network connection and without a running server. Today the only built-in aid is `ilium --help`, which lists the commands but does not complete them in a shell. This page states the rules for completion scripts and man pages, and which subcommands they must cover.

Related pages: [CLI compatibility](/pages/reference-cli-compatibility/), [cli contribution capability](/pages/capability-cli-contribute/), [plugin command-line reference](/pages/reference-plugin-cli/), [manifest reference](/pages/reference-manifest/).

### Terms

- **Command table**: the one list of commands that `ilium --help`, the completion scripts and the man pages read. It holds the kernel commands, the core plugin commands and the commands that installed plugins declare under `contributes.cli` ([COR-040](/pages/reference-cli-compatibility/)).
- **Kernel command**: a command the kernel owns. A plugin cannot take its name; the install fails with `manifest.cli-name-reserved`.
- **Core plugin command**: a command that a core plugin declares, for example `ilium chat` from `ilium.core.chatroom`. See [CLI compatibility](/pages/reference-cli-compatibility/).
- **Contributed subcommand**: a command that a plugin you installed declares under `contributes.cli` with the fields `name`, `summary`, `aliases` and `output`.
- **Completion script**: plain text for one shell that completes command names, aliases and flags.
- **Man page**: an offline manual page for one command.
- **Installed set**: the plugins that are installed, with their enabled or disabled state, at the moment a script or page is generated.

### Shells and command table

Completion scripts are plain text that the user loads into a shell. Each script completes the command names, the aliases and the flags that `ilium --help` lists. The scripts and the help text are two views of one command table, so a command that works in one view is never missing from the other.

- [DEV-855] Completion scripts MUST be provided for bash, zsh, fish and PowerShell. Each script MUST complete every subcommand that `ilium --help` lists, and it MUST be generated from the same command table that `ilium --help` reads.

- [DEV-869] A completion script MUST complete the flags of each command that the command table lists, and MUST NOT complete a flag that the table does not list. For a contributed subcommand that declares `argument_groups: ["pane-selector"]`, the script MUST complete `--pane` and `--all-panes`.
- [DEV-870] A completion script and a man page MUST be generated from the installed set without starting a plugin, without a running server and without network access. When no session is running, generation MUST still succeed and MUST use the manifests on disk.

Decision needed: the command that prints a completion script, and where the script is installed. The repository does not yet name this command. The default this page assumes is a command of the form `ilium <command> <shell>` that prints the script to standard output, so the user redirects it into the directory their shell reads. The default needs no root access, writes no file outside the user's choice and works offline. Replace it with a fixed install path only if the project owner wants `ilium` to write into shell directories itself.

### Contributed subcommands

A plugin adds a subcommand through the `cli` key of its manifest, under `contributes.cli`. The key holds the name, the one-line summary, the output form and any aliases. The completion scripts and the man pages read these fields from each installed plugin. See [capability cli.contribute](/pages/capability-cli-contribute/) for the full key list.

- [DEV-856] A subcommand that an installed and enabled plugin declares under `contributes.cli` MUST appear in every completion script and every man page, with its `name`, each entry of its `aliases` list and its `summary`. A name the install refused with `manifest.cli-name-reserved` or `manifest.cli-name-taken` MUST NOT appear, because the install did not succeed.

- [DEV-871] Generation MUST be deterministic. The same installed set and the same release MUST give byte-identical script and page text, with commands sorted by name, so that a diff of two generations shows only real changes.

The installed plugins decide the list. A user who installs or removes a plugin changes the list, so the completion script that the user loaded earlier no longer matches the installed set until it is generated again.

Decision needed: whether a completion script regenerates itself when a plugin is installed, enabled, disabled or removed. The default this page assumes is no. Regeneration is a manual step, because a shell profile that changes without a visible command is hard to audit. The user runs the generation command again after each change.

Subcommands owned by a disabled plugin are a separate case. The kernel answers a call to one of them with exit code 1 before any plugin runs, with the error `cli.owner-disabled`. Decision needed: whether completion hides these subcommands. The default this page assumes is to hide them, because a completion that offers a command which always fails misleads the user.

### Man pages

A man page gives the same facts as the [CLI reference](/pages/reference-plugin-cli/): the summary, the arguments, the output form and the exit codes. A man page is read offline, so its generation must not depend on the network or on a running session.

- [DEV-857] Man pages MUST be generated offline. The generation MUST NOT contact a registry, a mirror or a running server, and it MUST write one page for each kernel command, each core plugin command and each contributed subcommand.

Decision needed: whether the man pages ship in the release package, or are generated on the user's machine. The default this page assumes is generation on the user's machine into a directory the user chooses. A shipped set of man pages would have to be rebuilt for every combination of installed plugins, and the packages are signed as a fixed set of files.

- [DEV-872] Each man page MUST give the command name, each alias, the `summary`, the arguments, the output form (`jsonl`, `text` or `mixed`) and the exit codes of the [CLI reference](/pages/reference-plugin-cli/#exit-codes). A contributed subcommand that declares an `arguments_schema` MUST have its arguments described from that schema.

### Inputs and outputs

| Item | Content |
|---|---|
| Input | The kernel command table, the command tables of the core plugins and the `contributes.cli` fields of each installed plugin read from disk. The target shell for a script. The output directory for man pages. |
| Output of a script | The script text on standard output. Nothing is written to a shell directory. |
| Output of man pages | One file per command in the directory the user names. |
| Exit code | 0 when the script or all pages were written. 2 for an unknown shell name or a missing flag, before any work starts. 1 for a write failure. These are the codes of [DEV-007](/pages/reference-plugin-cli/#exit-codes). |

- [DEV-873] Generation MUST return exit code 2 for a shell other than bash, zsh, fish or PowerShell, and MUST write nothing. It MUST return exit code 1 and name the path when the output directory cannot be written, and it MUST NOT leave a half-written page.

### Errors and refusals

Completion and man pages do not run a plugin. They read the manifests of the installed plugins and the kernel command table. The codes below are the ones the install and the kernel already return for a subcommand, and they decide what the scripts and pages contain.

| Code | When | Effect on completion and man pages |
|---|---|---|
| `manifest.cli-name-reserved` | The name is a kernel command | The install is refused, so the name is not listed |
| `manifest.cli-name-taken` | Another installed plugin owns the name | The install is refused, so the name is not listed |
| `cli.owner-disabled` | The owner plugin is disabled | The kernel answers with exit code 1; see the open decision above on hiding the name |

- [DEV-874] A plugin whose manifest cannot be read when a script or page is generated MUST be skipped with one warning that names the plugin id. The skip MUST NOT stop the generation, and the plugin MUST NOT appear in the output.

### Limits

| Limit | Value | Source |
|---|---|---|
| Subcommand name and each alias | `[a-z][a-z0-9-]{0,31}`, at most 32 characters | [capability cli.contribute](/pages/capability-cli-contribute/) |
| Plugin command output lines | 64 KiB each | [capability cli.contribute](/pages/capability-cli-contribute/) |

A name that does not match is refused at install, so it never reaches a completion script or a man page.

- [DEV-875] A script and a page MUST NOT contain a command name or alias that fails the name rule, and MUST NOT contain text from a plugin other than the `name`, `aliases`, `summary` and the argument description from `arguments_schema`.

Decision needed: the maximum size of a generated completion script and the time generation may take. No page or source settles them, so this page states no number. The default is no fixed limit until the benchmark on the build host names one.

### Examples

Print a bash script and keep it (default form, see the decision above):

```text
ilium <command> bash > ~/.local/share/bash-completion/completions/ilium
```

A plugin that declares `name: "notes"` with `aliases: ["nt"]` and `summary: "Search notes"` gives the lines `notes`, `nt` and `Search notes` in the script and a page named for `notes`. After the plugin is removed, the next generation has none of them.

### What this page does not cover

- The fields of `contributes.cli` and the output forms: [capability cli.contribute](/pages/capability-cli-contribute/).
- The exit codes, flags and records of each `ilium plugin` command: [plugin command-line reference](/pages/reference-plugin-cli/).
- The stability of output and the deprecation rules: [CLI output stability](/pages/reference-cli-output-stability/).
- How a disabled owner answers a command: [CLI compatibility](/pages/reference-cli-compatibility/).

### See also

- [CLI compatibility](/pages/reference-cli-compatibility/)
- [Capability cli.contribute](/pages/capability-cli-contribute/)
- [Plugin command-line reference](/pages/reference-plugin-cli/)
