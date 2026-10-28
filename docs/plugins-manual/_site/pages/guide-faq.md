# Author FAQ

## Author FAQ

Short answers to questions that plugin authors ask often. Each answer points to the page that holds the rule. This page adds no requirements. If an answer and the linked page differ, the linked page decides.

### Getting started

**Which language should I use?** Rust or TypeScript. Both build to the same kind of component. Rust gives smaller files. TypeScript is quicker for small views. See [Rust SDK](/pages/rust-sdk/) and [TypeScript SDK](/pages/typescript-sdk/).

**How do I start?** Run `ilium plugin new <dir> --id <id> --lang <language> --side <side>`, then follow [Tutorial step 1](/pages/tutorial-01-agent-badge/).

**Which side do I need?** Server for terminals and agents, client for panels and settings pages, scene for animated backgrounds. See [Porting from other plugin systems](/pages/guide-porting-from-other-plugin-systems/) for a table.

**Can one plugin have more than one side?** Yes. Each side is its own component with its own grants, and the two talk by messages.

**What is the contract version?** `ilium:plugin@0.1.0`. It is the only version. See [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/).

### Capabilities and limits

**Why does my call return `not-permitted`?** The grant is missing from the side that calls, or the user declined it. Optional grants can be refused at any time, so handle the error.

**Can I ask for a grant later?** No. Grants are in the manifest, and a new grant needs an update that the user reviews.

**How much memory can I use?** 32 MiB by default. The ceiling is 256 MiB for bundled (T1) plugins and 128 MiB for T2 and T3. See [Errors and limits](/pages/reference-errors-and-limits/).

**How long may a handler run?** Events 20 ms (T1: 50 ms), commands 50 ms, render 10 ms, `activate` 2 s, `deactivate` 500 ms. Longer work goes to jobs. See [Performance](/pages/performance/).

**What happens when I break a limit?** The instance traps and restarts after 1 s, then 5 s. The third failure in 10 minutes disables the plugin on that side.

**Can I use threads?** No. A component is single-threaded. Use jobs or requests for background work.

**Can I use the network?** Yes, with an `http:<host-pattern>` grant. A process started with `process.run` has no network.

### Data and state

**Where do I keep data?** In a storage bucket: `global`, `project` or `session`. Keys are 256 bytes at most and values 1 MiB. Use `shared` only for data you can rebuild. See [Patterns cookbook](/pages/guide-patterns-cookbook/).

**Is memory kept across restarts?** No. Keep what you need in storage or restore bytes.

**How do I change my data layout?** Raise `data_version` in the manifest and export `migrator`. See [Data migration](/pages/guide-data-migration/).

**Do timers survive a restart?** `timers.set` timers do not. Durable timers do. See [Clock and timers](/pages/api-clock-and-timers/).

**How do I get the user's time zone or language?** You cannot. The contract has no such call. See [Internationalisation](/pages/guide-internationalisation/).

### Interface

**Why must widgets have keys?** The host keeps the state of a widget (selection, scroll, text) by key. Without it the state is lost on each render.

**Can I draw my own colours?** Use the nine colour roles. They follow the theme. See [Accessibility and theming](/pages/accessibility-and-theming/).

**Can I show HTML?** No. A view is a tree of widgets that the host draws.

**How do I measure text?** With `display-width`. Do not count characters.

### Testing

**How do I test without a running Ilium?** Use the fake host. See [Testing plugins](/pages/testing/).

**Why does my timer not fire in a test?** The fake host never runs timers by itself. Call `advance_clock` and then `settle`.

**Can I test an upgrade?** Yes. Load a storage snapshot of the old version and run the new one. See [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/).

### Publishing

**How do I share a plugin?** Pack it, optionally sign it, and send the file. See [Publishing and support](/pages/guide-publishing-and-support/).

**Do I need to sign?** Not for a local plugin. A signed package (T2) shows a publisher and key fingerprint in the review dialog.

**Can I rename my plugin?** The id is permanent. A new id is a new plugin.

**Is there a registry?** The manual specifies an optional one, in [Registry reference](/pages/reference-registry/). The package file works without it.

**What if I lose my key?** You cannot sign new releases under it. See the key table in [Publishing and support](/pages/guide-publishing-and-support/).

**Does it run on Windows or macOS?** Only bundled (T1) plugins. T2 and T3 packages need Linux.

### Problems

**My plugin was disabled. Why?** Three failures in 10 minutes. Read the first trap in the log. See [Troubleshooting](/pages/guide-troubleshooting/).

**How do I see what my plugin did?** `ilium plugin logs <id>`, `trace`, `stats`. See [Debugging tools](/pages/reference-debugging-tools/).

**How do I check a release?** Use the [Quality checklist](/pages/guide-quality-checklist/).

## See also

- [Patterns cookbook](/pages/guide-patterns-cookbook/)
- [Glossary](/pages/glossary/)
- [Troubleshooting](/pages/guide-troubleshooting/)

API version checked: `ilium:plugin@0.1.0`.
