---
layout: page
title: Shell completion and man pages
permalink: /pages/reference-cli-completion/
---

## Shell completion and man pages

The `ilium` command line has commands from three sources: the kernel, the core plugins, and the plugins you install. Tab completion and offline man pages let a user or an operator find each of them without a network connection and without a running server. Today the only built-in aid is `ilium --help`, which lists the commands but does not complete them in a shell. This page states the rules for completion scripts and man pages, and which subcommands they must cover.

Related pages: [CLI compatibility](/pages/reference-cli-compatibility/), [cli contribution capability](/pages/capability-cli-contribute/), [plugin command-line reference](/pages/reference-plugin-cli/), [manifest reference](/pages/reference-manifest/).

### Shells and command table

Completion scripts are plain text that the user loads into a shell. Each script completes the command names, the aliases and the flags that `ilium --help` lists. The scripts and the help text are two views of one command table, so a command that works in one view is never missing from the other.

- [DEV-855] Completion scripts MUST be provided for bash, zsh, fish and PowerShell. Each script MUST complete every subcommand that `ilium --help` lists, and it MUST be generated from the same command table that `ilium --help` reads.

Decision needed: the command that prints a completion script, and where the script is installed. The repository does not yet name this command. The default this page assumes is a command of the form `ilium <command> <shell>` that prints the script to standard output, so the user redirects it into the directory their shell reads. The default needs no root access, writes no file outside the user's choice and works offline. Replace it with a fixed install path only if the project owner wants `ilium` to write into shell directories itself.

### Contributed subcommands

A plugin adds a subcommand through the `cli` key of its manifest, under `contributes.cli`. The key holds the name, the one-line summary, the output form and any aliases. The completion scripts and the man pages read these fields from each installed plugin. See [capability cli.contribute](/pages/capability-cli-contribute/) for the full key list.

- [DEV-856] A subcommand that an installed and enabled plugin declares under `contributes.cli` MUST appear in every completion script and every man page, with its `name`, each entry of its `aliases` list and its `summary`. A name the install refused with `manifest.cli-name-reserved` or `manifest.cli-name-taken` MUST NOT appear, because the install did not succeed.

The installed plugins decide the list. A user who installs or removes a plugin changes the list, so the completion script that the user loaded earlier no longer matches the installed set until it is generated again.

Decision needed: whether a completion script regenerates itself when a plugin is installed, enabled, disabled or removed. The default this page assumes is no. Regeneration is a manual step, because a shell profile that changes without a visible command is hard to audit. The user runs the generation command again after each change.

Subcommands owned by a disabled plugin are a separate case. The kernel answers a call to one of them with exit code 1 before any plugin runs, with the error `cli.owner-disabled`. Decision needed: whether completion hides these subcommands. The default this page assumes is to hide them, because a completion that offers a command which always fails misleads the user.

### Man pages

A man page gives the same facts as the [CLI reference](/pages/reference-plugin-cli/): the summary, the arguments, the output form and the exit codes. A man page is read offline, so its generation must not depend on the network or on a running session.

- [DEV-857] Man pages MUST be generated offline. The generation MUST NOT contact a registry, a mirror or a running server, and it MUST write one page for each kernel command, each core plugin command and each contributed subcommand.

Decision needed: whether the man pages ship in the release package, or are generated on the user's machine. The default this page assumes is generation on the user's machine into a directory the user chooses. A shipped set of man pages would have to be rebuilt for every combination of installed plugins, and the packages are signed as a fixed set of files.

### Errors and refusals

Completion and man pages do not run a plugin. They read the manifests of the installed plugins and the kernel command table. The codes below are the ones the install and the kernel already return for a subcommand, and they decide what the scripts and pages contain.

| Code | When | Effect on completion and man pages |
|---|---|---|
| `manifest.cli-name-reserved` | The name is a kernel command | The install is refused, so the name is not listed |
| `manifest.cli-name-taken` | Another installed plugin owns the name | The install is refused, so the name is not listed |
| `cli.owner-disabled` | The owner plugin is disabled | The kernel answers with exit code 1; see the open decision above on hiding the name |

### Limits

Each subcommand name follows the rule in the manifest reference: `[a-z][a-z0-9-]{0,31}`. A name that does not match is refused at install, so it never reaches a completion script or a man page.

### See also

- [CLI compatibility](/pages/reference-cli-compatibility/)
- [Capability cli.contribute](/pages/capability-cli-contribute/)
- [Plugin command-line reference](/pages/reference-plugin-cli/)
