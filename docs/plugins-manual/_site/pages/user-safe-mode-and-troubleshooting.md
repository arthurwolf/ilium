# Safe mode and recovery

## Safe mode and recovery

When a plugin makes Ilium hard to use, start in safe mode. Safe mode turns plugins off without removing anything, so you can find the cause and fix it. This page also covers the bisect tool and the crash-loop guard.

Contract version: `ilium:plugin@0.1.0`.

### Start in safe mode

| Command | Effect |
|---|---|
| `ilium --safe-mode` | The same as `--safe-mode=third-party`. |
| `ilium --safe-mode=third-party` | Runs the bundled plugins only. T2 and T3 plugins are not loaded. `replaces` is ignored. |
| `ilium --safe-mode=all-optional` | Runs only the essential set below. Every other plugin is off. |
| `ILIUM_SAFE_MODE=third-party` | Same as the flag. The flag wins when both are set. The variable also accepts `all-optional`, `1` (the same as `third-party`) and `0` (off). Any other value exits 2 with `usage.invalid-value`. |

The older switches stay as aliases: `--safe-plugins` is `--safe-mode=third-party`, and `--no-plugins` starts no plugin host at all (the kernel only). When more than one is given, the strictest wins.

Safe mode changes nothing on disk. Installed packages, grants, settings and data stay as they were. The footer shows "Safe mode: third-party plugins are off."

- [LIF-180] `--safe-mode=third-party` MUST load no T2 or T3 plugin and ignore `replaces`, and `--safe-mode=all-optional` MUST load only the essential set. Neither MUST change files on disk. Test: start both and compare the profile before and after.
- [LIF-181] The environment variable MUST behave like the flag, and the flag MUST win. Test: set both to different values.
- [LIF-182] Safe mode MUST show an indicator in the footer. Test: capture the footer.

### The essential set

The essential set is the core plugins that the screen needs to stay usable. A plugin outside the set can be turned off in safe mode.

| Plugin | Why essential |
|---|---|
| `ilium.core.tree` | draws the tree |
| `ilium.core.layout` | keeps the split layout |
| `ilium.core.settings` | opens Settings and the recovery page |
| `ilium.core.sessions` | writes the session snapshot |

Core plugins that are not in this table can be disabled. See the column "Can be disabled" in [Core plugins](/pages/core-plugins/).

- [LIF-183] The essential set MUST be exactly the four plugins above in 0.1.0, and MUST load in `all-optional` mode. Test: list running plugins in that mode.

### Find the plugin that causes a problem

`ilium plugin bisect` halves the set of enabled third-party plugins until it finds the one at fault.

| Command | Meaning |
|---|---|
| `ilium plugin bisect start` | Saves the enabled set, then enables half of it for this session. |
| `ilium plugin bisect good` | The problem is gone with this half. Test the other half next. |
| `ilium plugin bisect bad` | The problem remains. Narrow this half. |
| `ilium plugin bisect reset` | Restores the saved set. |

State lives in `<data>/plugins/bisect.json`. Each command prints JSONL records of type `bisect` with the enabled list and the remaining candidates. When one plugin remains, the record has `found` set to its id. The bisect changes the session level only, so it never alters your saved choices.

- [LIF-184] Bisect MUST change only the session-level enabled state and MUST restore the saved set on `reset`. Test: run to the end and compare `installed.json`.
- [LIF-185] Bisect MUST finish in at most `ceil(log2(n))` steps for `n` plugins. Test: 16 plugins, 4 steps.

### The crash-loop guard

If the server stops with a crash 3 times within 5 minutes during start, the next start runs in `third-party` safe mode and opens the recovery page. The guard clears after one start that stays up for 5 minutes.

- [LIF-186] Three crashes during start within 5 minutes MUST cause the next start to use `third-party` safe mode. Test: write three crash marks.

### Recovery steps

1. Start with `ilium --safe-mode=third-party`.
2. Open Settings, then Plugins ([Managing plugins](/pages/user-managing-plugins/)).
3. Turn plugins on one at a time, or run `ilium plugin bisect start`.
4. When you find the plugin, update it, disable it, or uninstall it.
5. Start normally.

<figure>
<pre class="term"><span class="t-warn">Safe mode: third-party plugins are off.</span>
Plugins
 ilium.core.tree       on
 com.example.notes     off  (safe mode)
 com.example.cursor    off  (safe mode)
[b] bisect  [r] leave safe mode
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The Plugins list in safe mode with third-party plugins held off.</figcaption>
</figure>

### See also

- [Managing plugins](/pages/user-managing-plugins/)
- [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/)
- [Host architecture](/pages/reference-host-architecture/)
- [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/)

API version checked: `ilium:plugin@0.1.0`.
