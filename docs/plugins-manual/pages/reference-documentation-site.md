---
layout: page
title: Reference, documentation site
permalink: /pages/reference-documentation-site/
---

## Documentation site

This page states the rules of the site that serves this manual: who may reuse its text, what happens to a page that is renamed or removed, how to get the manual without the network, how to report a mistake, how accessible the site is, and in which languages it is published. The snapshot paths and retention are defined on [Reference, documentation versions](/pages/reference-documentation-versions/) and the correction process on [Reference, documentation lifecycle](/pages/reference-documentation-lifecycle/). This page does not repeat them.

None of the rules below has a requirement id of its own yet. Where an existing id already fixes part of a rule, the rule cites it. The rules that have no id are marked "(proposed, owner to confirm)" and are listed in the notes of the change that added them, so that ids can be allocated in the requirements files.

## You will need

- The permalink of the page and the host version you read it for (`ilium version --json`), see [LIF-961].
- For the offline copy: the release archive directory and its `SHA256SUMS` file, see [SEC-428].

## Licence and reuse of the manual

The manual is published for machine and agent consumption as well as for people, so its reuse terms are stated here and not left to the reader.

| Material | Licence (SPDX) | Covers |
|---|---|---|
| Prose, tables, diagrams and figures | `CC-BY-4.0` | Every page under `/docs/latest/` and under each `/docs/X.Y.Z/` snapshot. |
| Code samples | `Apache-2.0` | Every fenced code block, manifest, WIT fragment and configuration sample. |
| `llms.txt`, `llms-full.txt` and the agent kit | Same as the material they are built from | A prose line is `CC-BY-4.0`, a code sample inside them is `Apache-2.0`. They are generated copies, they carry no licence of their own. |

(proposed, owner to confirm)

- A code sample may be pasted into a plugin of any licence. `Apache-2.0` was chosen over a copyleft licence for this reason. The sample needs no attribution beyond keeping the licence of the file it lands in.
- A reuse of prose MUST give attribution: the name "Ilium plugin manual", the page permalink, the snapshot version from [LIF-962], and a statement that changes were made when the text was edited. A link to the licence text is enough for the licence notice.
- An agent that quotes a rule to a user is a reuse of prose. It cites the permalink and the snapshot version, which is also what [LIF-974] asks of a support answer.
- Excluded from both licences: the name "Ilium" and its logo (a reuse MUST NOT suggest that the project endorses the result), and any screenshot or recording of a third-party plugin, which stays under the licence of that plugin.
- Every generated file carries the SPDX identifiers in its header, so a tool that reads only `llms.txt` still sees the terms. A snapshot keeps the licence it was published with, even if the current manual later changes it ([LIF-965]).

## Renamed and removed pages in the current manual

[LIF-966] already covers a removed snapshot. The rules here cover the current manual (proposed, owner to confirm).

1. A page whose `permalink` changes MUST keep a redirect from the old permalink for at least the snapshot retention period of [LIF-964], 24 months after the next minor release.
2. A page that is removed MUST be replaced by a stub that names its successor and the date of removal, in the same form as the stub of [LIF-966]. The stub returns HTTP 410 for crawlers and shows the successor link to people.
3. An anchor cited by a requirement row or by an entry of the error-code catalogue MUST NOT be renamed without a redirect to the new anchor. The same holds for the links printed by `ilium plugin explain <code>` ([USR-347]), which MUST resolve in both the snapshot of the running version and the current manual.
4. The build MUST fail on a broken internal link, on a redirect that points at a redirect, and on a redirect whose target does not exist.

A redirect is declared in the front matter of the successor page, not in a separate table:

```yaml
---
layout: page
title: Reference, documentation site
permalink: /pages/reference-documentation-site/
redirect_from:
  - /pages/old-name/
---
```

The key `redirect_from` is a list of old permalinks. The navigation generator skips them, so an old permalink never appears in the menu. The `llms.txt` builder lists only the current permalink and writes each old permalink into the `Aliases:` line of that entry, so an agent holding an old link finds the page. A removed page is declared the same way on its stub with `redirect_to` and `removed: YYYY-MM-DD`. Both keys are read by the same build step, so a page cannot be listed in the menu and in the aliases at once.

## Offline copy of the manual

Everything the manual says about air-gapped and mirrored installs (see [PKG-467] for the registry transport and [PKG-950] for the daily mirror refresh) is of little use if the documentation is reachable only online. Man pages cover the command line only ([DEV-870]).

- **Archive.** Each snapshot is published as one archive, `ilium-docs-X.Y.Z.tar.gz`, in the same directory as the release archives of host `X.Y.Z`. It holds the static site of `/docs/X.Y.Z/`, `llms.txt`, `llms-full.txt` and the agent kit.
- **Integrity.** The archive is listed in the `SHA256SUMS` file of the release with its SHA-256 hash, one line per archive ([SEC-428]). Check it as for any other release archive, see [Reference, host release verification](/pages/reference-host-release-verification/).
- **Use without the binary.** Unpack it and either open `index.html` from the file system or serve the folder with any static file server. All links are relative, and the site loads no script, font or image from another host.
- **Use with the binary.** `ilium docs --offline` serves the copy that matches the running host version on `127.0.0.1` on a free port, prints the address and exits 0 (proposed, owner to confirm). It reads the archive from the directory named by `[docs] offline_path` and exits 2 with `docs.offline-missing` when there is none or the hash does not match.
- **Mirrored and air-gapped deployments.** The operator copies the archive of every host minor version in use next to the private registry mirror and gives its address to users. A user on an old host then reads the snapshot of that host ([LIF-974]) without any outbound connection. When the mirror is refreshed ([PKG-476] applies to what is already installed, not to documentation), the operator also refreshes the archive.

## Reporting a mistake in the manual

- **Where.** Open an issue with the template "Documentation error" in the project tracker. A security-relevant mistake goes through the private channel of [Reference, security response](/pages/reference-security-response/) instead.
- **What to include.** The page permalink, the snapshot version (the footer of every page shows it, [LIF-962]), the exact sentence, and the behaviour you observed, with the output of `ilium version --json`.
- **Classification.** The maintainers label the report as one of two classes. A wording-only correction fixes wording, spelling or a link ([LIF-968]). A rule correction changes a rule, a number, a code name or a procedure ([LIF-969]).
- **Where the fix lands.** In the current manual always. In every snapshot still online that makes the same wrong statement when the statement changes what a plugin or an operator does ([LIF-967]). A snapshot is never replaced by the current manual ([LIF-965]).
- **How you learn it was fixed.** A rule correction is listed in the release notes of the next patch release with the page, the old and the new statement and the snapshots that received it ([LIF-969]). A wording-only correction is not listed. The issue is closed with a link to the commit. A correction never changes the contract version ([LIF-970]).
- **Edit links.** Every page carries an "Edit this page" link at its foot. It opens the source file of the page in the repository under `docs/plugins-manual/pages/`, named by the last path part of the permalink. A snapshot page links to the file at the tag of its release.

## Accessibility of the documentation site

The product checks its own interface with a screen reader before every release ([USR-951], [USR-950]). The site that documents it holds itself to a comparable standard (proposed, owner to confirm).

- **Target.** WCAG 2.2 level AA.
- **Checked on each release of the site.** Keyboard-only navigation of the menu, search and page body, with a visible focus mark. A single `h1` per page and no skipped heading level. Contrast of text and of interface parts in both the light and the dark theme. A text alternative for every figure. Zoom to 200 percent and a 320 px wide viewport without horizontal scrolling of the text.
- **Diagrams.** Every diagram is inline SVG and keeps its `<title>` and its `<desc>`, which say what the diagram shows in full sentences. A diagram that lacks either fails the build.
- **Terminal captures.** Every terminal mock-up and command output is selectable text in a code block, never an image. Colour is never the only carrier of meaning in a capture.
- **Screen readers.** The site is checked with one screen reader on each desktop platform that the product release notes name under [USR-950]. The names are printed in the accessibility statement of the release. A platform with no named reader is not claimed as tested.
- **Known gaps.** The statement lists open gaps with the page and the date found. A gap is closed or restated at each release. The search box announces the result count but not the result order.
- **Complaints.** Write to the accessibility address in the footer of the site. A first answer is promised within 10 working days (proposed, owner to confirm), and an unresolved complaint is referred to the national enforcement body named in the statement.

## Languages and translation of the manual

- **Source language.** English. The English text is authoritative. When a translation differs from it, the English page of the same snapshot decides.
- **Translations.** Accepted as community contributions, reviewed by a maintainer who reads the language. They are published per snapshot under `/docs/X.Y.Z/<lang>/`, with `<lang>` a BCP 47 tag such as `de` or `pt-BR`. A language has no `/docs/latest/<lang>/` until its current page set is complete.
- **Never translated.** Requirement ids such as `LIF-961`, error codes such as `docs.offline-missing`, command and flag names, grant strings, settings keys, manifest fields, file names and code samples. Quoted text of a consent dialog is given in the language of the interface only where the locale catalogue of [Internationalisation](/pages/guide-internationalisation/) defines it. The exact wording of an English dialog stays in English next to it.
- **Out of date.** Each translated page records the git commit of the English page it was translated from. When the English page is newer, the translated page shows a banner at the top, "This translation is out of date. The English page is authoritative.", with a link to it. A page whose English source changed a rule or a number is withdrawn from the menu until updated, and shows the English page instead.
- **Corrections.** A correction of a rule ([LIF-969]) reaches a translation only after the English snapshot has it. The release notes list the English page only.
- **Machine consumption.** `llms.txt` and `llms-full.txt` are built from the English text only, so an agent never reads a translation as the contract.
- **Licence.** A translation is licensed as the prose of the manual (see "Licence and reuse of the manual").

## See also

- [Reference, documentation versions](/pages/reference-documentation-versions/)
- [Reference, documentation lifecycle](/pages/reference-documentation-lifecycle/)
- [Accessibility and theming](/pages/accessibility-and-theming/)
- [Agent documentation, llms.txt](/pages/agent-docs-llms-txt/)
- [Reference, host release verification](/pages/reference-host-release-verification/)
