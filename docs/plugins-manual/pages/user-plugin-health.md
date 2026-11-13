---
layout: page
title: Plugin health for users
permalink: /pages/user-plugin-health/
---

## Plugin health for users

A plugin can slow Ilium down or stop working without telling you. Ilium watches every plugin and shows one health word for each. This page explains the words, how to see the reason, and what to do. It uses no technical background.

## You will need

- Ilium with at least one plugin installed. The core plugins that ship with Ilium appear here too.

## The health words

| Word | What it means | What you can do |
|---|---|---|
| healthy | The plugin works and answers on time | Nothing |
| dormant | The plugin is on and waits until something needs it | Nothing |
| needs-setup | The plugin is on, but a setting it cannot work without is empty. It has not started and has not failed | Open its settings and fill the rows marked as needed. See [A plugin that needs setup](/pages/user-install-and-manage/#a-plugin-that-needs-setup) |
| degraded | The plugin works but is slow, busy or was restarted once | Open the detail and read the reason. Often it needs no action |
| restarting | The plugin failed and Ilium will start it again in a moment | Wait a few seconds |
| failed | The plugin failed three times in ten minutes and Ilium turned it off | Read the log. Turn it on again with the Space key when you want to try again |
| disabled | You, or your administrator, turned the plugin off | Turn it on in the Plugins screen if the policy allows |
| refused | Ilium would not load the plugin, for example because it needs permissions that your policy forbids | Read the reason in the detail |

## See the health of every plugin

Open Settings, then Plugins. The Health column shows the word.

<figure>
<pre class="term">┌──────────────────────────────────────────────┐
│ Plugins                                      │
│ Name            State    Health              │
│ Notes           on       healthy             │
│ Tickets         on       degraded            │
│ Cursor trail    on       restarting          │
│ Sketch pad      on       failed              │
│                                              │
│ [Enter] detail   [R] restart   [Space] on/off│
└──────────────────────────────────────────────┘
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The Plugins list with a Health column.</figcaption>
</figure>

## See why a plugin is degraded

Select the plugin and press Enter. The detail view shows the reason with numbers.

<figure>
<pre class="term">┌──────────────────────────────────────────────┐
│ Tickets: health                              │
│ Health      degraded                         │
│ Why         near-misses (7 in 5 min)         │
│ Slowest     views.render 9.1 ms of 10 ms     │
│ Queue       812 of 1,024 events              │
│ Failures    0 in the last 10 minutes         │
│                                              │
│ [L] open log   [Esc] back                    │
└──────────────────────────────────────────────┘
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The health detail of one plugin.</figcaption>
</figure>

The reasons you can see are:

- **Near-misses.** The plugin answered, but close to the time limit, several times in the last five minutes. If Ilium feels slow while this plugin is busy, turn the plugin off to test.
- **Busy queue.** Many messages wait for the plugin. This passes when the plugin catches up.
- **Held by its quota.** The plugin used its share of time or data and Ilium made it wait. It continues on its own.
- **Waiting for memory.** Ilium has too many plugins running at once. Turn off one you do not use.
- **Restarted.** The plugin failed once and started again. The word changes to failed only after three failures within ten minutes.

- **[USR-064]** The detail view of a plugin in state `failed` or `refused` MUST show the sentence of its last failure (the export, the code or trap, and the time as `YYYY-MM-DD HH:mm:ss`) followed by the cause sentence and the next-step sentence of that code from the catalogue ([USR-347]), and MUST NOT show a WebAssembly offset, a backtrace, a host frame or a raw engine message; those go only to the plugin log (proposed, owner to confirm).
- **[USR-076]** The detail view of a plugin in state `refused` or `blocked` MUST show the code, its cause sentence and its next-step sentence ([USR-347]), and when the cause is a policy it MUST append the administrator's `notice.contact` text ([USR-343]); the state word in the row MUST NOT be the only explanation (proposed, owner to confirm).

## When a fix is available

A plugin that failed may already have a newer version that fixes the problem. Ilium tells you so in the place where you read about the failure, and it uses only what it already knows.

When the health of a plugin is `failed` or `restarting`, the failure notice and the plugin's detail view add the line `Version X.Y.Z is available. Update?` with the action **Update**. X.Y.Z is the newest version that Ilium could install for you now: the value of the field `update-available` of the plugin record ([Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/)), which comes from cached registry data only. The action runs the normal update, with the review dialog when the new version needs one ([Update a plugin](/pages/user-install-and-manage/#update-a-plugin)). If the update fails to start, Ilium puts the old version back, as for any update.

The line is shown only when all of this holds:

- The registry data that Ilium has cached lists a version higher than the installed one, on the channel you selected, that is not yanked.
- The cached data is not expired ([PKG-541](/pages/user-plugin-store/)).
- Your administrator's policy allows that version: it is not blocked, it matches a pin if there is one, and it is not held back ([Holding back new versions](/pages/reference-install-and-update/#holding-back-new-versions)).

Otherwise the line is absent, and the notice and the detail view read as before. Ilium makes no network request to find out whether to show the line. With `auto_update = off` it therefore appears only if you opened the store or ran a check yourself and the data is still cached. A plugin installed from a file has no registry data and never shows the line. The requirement is PRM-208, at the end of this page.

## What happens to what the plugin showed

When a plugin stops, the badges and rows that it added stay on screen for up to 60 seconds, dimmed as stale (HST-254, see [Rendering pipeline for plugins](/pages/reference-host-rendering/)). This lets you keep working. If the plugin comes back, the items refresh. If it does not, they disappear. A dimmed item that you select shows "plugin is restarting".

## What Ilium never does

Ilium does not send health data anywhere. The numbers stay on your computer. See [Telemetry and plugin health](/pages/reference-host-telemetry/) for what is measured. A health report that you share is made by you, with `ilium plugin status` or the Diagnostics report, and you choose what to send.

## If Ilium itself is slow or does not start

Start Ilium with `--safe-mode` (the alias `--safe-plugins` does the same) to run only the bundled plugins, or with `--no-plugins` to run none. See [Safe mode and troubleshooting](/pages/user-safe-mode-and-troubleshooting/). The recovery page lists the plugins that are not healthy first.

## See also

- [Managing plugins](/pages/user-managing-plugins/) for turning plugins on and off.
- [Troubleshooting](/pages/user-troubleshooting/) for common problems.
- [Health and watchdog](/pages/reference-health-and-watchdog/) for the exact rules.

API version checked: `ilium:plugin@0.1.0`.

## Production additions: T8-observability-perf

- **[HST-640]** The Plugins screen MUST show for each plugin the health, the last error code with its time, calls per minute, p95 call time, and memory, updating at most once per second, and MUST render this in at most 16 ms (proposed, owner to confirm) for 64 plugins.


## Production additions: T7-users-privacy-a11y

- **[USR-338]** When a plugin screen's view call fails or exceeds its 100 ms (proposed, owner to confirm) budget the host MUST keep the last good frame dimmed for at most 2 s and then show a placeholder naming the plugin and the code, with the keys Retry and Details, and MUST NOT blank any other window.

## Production additions (round 15)

- **[PRM-208]** The system must, when a plugin's health is `failed` or `restarting` and the cached, unexpired registry metadata lists a newer non-yanked version that policy allows, add the line `Version X.Y.Z is available. Update?` with an Update action to the failure notice and to the plugin detail view. Without such a cached version, or when policy blocks the update, the line must be absent, and no network request may be made only to produce it.
