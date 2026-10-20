# Plugin health for users

## Plugin health for users

A plugin can slow Ilium down or stop working without telling you. Ilium watches every plugin and shows one health word for each. This page explains the words, how to see the reason, and what to do. It uses no technical background.

## You will need

- Ilium with at least one plugin installed. The core plugins that ship with Ilium appear here too.

## The health words

| Word | What it means | What you can do |
|---|---|---|
| healthy | The plugin works and answers on time | Nothing |
| dormant | The plugin is on and waits until something needs it | Nothing |
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
- **Restarted.** The plugin failed once and started again. If it happens again, the word changes to failed.

## What happens to what the plugin showed

When a plugin stops, the badges and rows that it added stay on screen for up to a minute, dimmed. This lets you keep working. If the plugin comes back, the items refresh. If it does not, they disappear. A dimmed item that you select shows "plugin is restarting".

## What Ilium never does

Ilium does not send health data anywhere. The numbers stay on your computer. See [Telemetry and plugin health](/pages/reference-host-telemetry/) for what is measured. A health report that you share is made by you, with `ilium plugin status` or the Diagnostics report, and you choose what to send.

## If Ilium itself is slow or does not start

Start Ilium with `--safe-mode` (the alias `--safe-plugins` does the same) to run only the bundled plugins, or with `--no-plugins` to run none. See [Safe mode and troubleshooting](/pages/user-safe-mode-and-troubleshooting/). The recovery page lists the plugins that are not healthy first.

## See also

- [Managing plugins](/pages/user-managing-plugins/) for turning plugins on and off.
- [Troubleshooting](/pages/user-troubleshooting/) for common problems.
- [Health and watchdog](/pages/reference-health-and-watchdog/) for the exact rules.

API version checked: `ilium:plugin@0.1.0`.
