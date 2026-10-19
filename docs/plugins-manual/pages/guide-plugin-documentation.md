---
layout: page
title: Plugin documentation guide
permalink: /pages/guide-plugin-documentation/
---

## Plugin documentation guide

This guide says what documentation a plugin ships and how to write it. Good plugin documentation is short and exact. The `lint` command checks the parts it can check. See [Check and lint rules](/pages/reference-check-and-lint-rules/).

Terms:

- **README**: the file a person reads first, in the repository and on the package page.
- **Help topic**: a Markdown file inside the package that Ilium shows in its help viewer.
- **Changelog**: the list of changes per version.

## You will need

- A plugin project. `ilium plugin new` writes the skeletons.
- The manifest keys `description`, `license` and `contributes.help`. See [Manifest reference](/pages/reference-manifest/).

## What a plugin ships

| File | Required | Checked by | Purpose |
|---|---|---|---|
| `README.md` | yes | `lint.readme-missing` | First read: what, how to build, which grants |
| `CHANGELOG.md` | yes | `lint.changelog-missing` | Changes per version |
| `help/<id>.md` | one per `contributes.help` entry | `lint.help-topic-missing` | In-app help |
| `docs/` | no | none | Longer guides for authors |
| `LICENSE` | when `license` is set | `lint.license-file-missing` | The license text |

- **[DEV-780]** A plugin MUST ship a README, a changelog and one help topic for each `contributes.help` entry. A project from `new` already does.
- **[DEV-781]** The README MUST list every requested grant with one sentence on why the plugin needs it. The manifest has no field for a reason, so the README is the place. `lint.grant-unexplained` warns when a grant name does not appear in the README.
- **[DEV-782]** The README MUST give the commands to build and test the plugin.
- **[DEV-783]** The changelog MUST follow Keep a Changelog headings: `Added`, `Changed`, `Fixed`, `Removed`. The top section of a release MUST match `version` in the manifest, or be `Unreleased`.

## Writing a help topic

A help topic is Markdown with these rules.

| Rule | Value |
|---|---|
| Format | Markdown, headings level 1 to 3 |
| Title | First line, `# Title`, same as the manifest `title` |
| Length | At most 20 KiB |
| Images | Not supported. Use text. |
| Links | Other topics as `help:<topic-id>`. Web links open in the browser. |

- **[DEV-785]** A help topic MUST start with a level 1 heading equal to the `title` in the manifest entry. `lint.help-title-mismatch` warns otherwise.
- **[DEV-786]** A help topic MUST NOT exceed 20 KiB. `lint.help-topic-too-large` warns for a larger file.
- **[DEV-787]** A link `help:<topic-id>` to a topic that the plugin does not ship MUST be a `lint.help-link-broken` warning.
- **[DEV-788]** A help topic SHOULD name each command, setting and view the plugin adds, so a search in the help viewer finds them.

## Style

Follow the style of this manual.

- Short sentences. One idea each.
- Sentence case for headings.
- Name each thing the same way every time.
- Show a command with its exact flags, and show the exact text it prints when that helps.
- Put a warning before the step it applies to, in words, not in symbols.

## A good README, step by step

1. Write one paragraph that says what the plugin adds to Ilium.
2. List the requested grants and why.
3. Show the install command for a package: `ilium plugin install <file>`.
4. Show how to build and test.
5. Say which sides the plugin has and which Ilium versions it supports.
6. Link the changelog and the license.

An example:

```markdown
# Agent tools

Marks agent panes that are working with a badge in the tree.

## Grants

- `tree.read`: reads agent activity.
- `tree.decorate`: sets the badge.

## Build and test

    ilium plugin check
    ilium plugin lint --deny warnings
    ilium plugin test
    ilium plugin build

## Install

    ilium plugin install dist/example.agent-tools-0.1.0.iliumpkg
```

## Documenting settings and commands

- **[DEV-790]** Every setting row MUST have a `help` text in the settings schema or a help topic that names the row.
- **[DEV-791]** Every command in `contributes.commands` MUST have a `title` in sentence case with no final period.

## Docs for extension points

A plugin that declares an extension point MUST describe it in a document: the point id, its kind, the payload, the reply rules and the deadline. `core-style` templates write `docs/EXTENDING.md` for this.

- **[DEV-795]** A declared extension point MUST appear in the README or in `docs/EXTENDING.md`. `lint.extension-point-undocumented` warns when its id is absent from both.

## A complete changelog

```markdown
# Changelog

## [0.2.0] - 2030-02-01

### Added

- A command that clears all badges.

### Changed

- The badge now shows for agents that wait for approval.

### Fixed

- A badge no longer stays after the agent pane closes.

## [0.1.0] - 2030-01-01

### Added

- First version.
```

- **[DEV-796]** Dates in the changelog MUST use `YYYY-MM-DD`.
- **[DEV-797]** The newest version MUST come first.

## A complete help topic

```markdown
# Agent tools

Agent tools marks agent panes that are working.

## Commands

- Clear badges: removes every badge the plugin set.

## Settings

- Show badge for approval: also marks agents that wait for approval.

## Grants

The plugin reads agent activity and sets decorations. It needs no
other grants.
```

## Review checklist

| Question | Where it is checked |
|---|---|
| Does the README say what the plugin does in one paragraph? | A person |
| Does every requested grant have a sentence? | `lint.grant-unexplained` |
| Does every command have a help line? | A person |
| Do help titles match the manifest? | `lint.help-title-mismatch` |
| Do help links resolve? | `lint.help-link-broken` |
| Does the changelog top section match the version? | A person, or a CI shell step |
| Is the license text present? | `lint.license-file-missing` |

## Edge cases

- **Translated help.** Ship `help/<id>.<lang>.md` for another language. The viewer falls back to the base file.
- **Very long READMEs.** The package page shows the first 64 KiB.
- **No help topic.** A plugin with `contributes.help` empty passes lint but shows no help entry.

## See also

- [Check and lint rules](/pages/reference-check-and-lint-rules/)
- [Project templates](/pages/reference-project-templates/)
- [Manifest reference](/pages/reference-manifest/)
- [CI recipes](/pages/guide-ci-recipes/)

API checked against `ilium:plugin@0.1.0`.
