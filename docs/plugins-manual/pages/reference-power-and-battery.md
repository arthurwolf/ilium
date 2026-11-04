---
layout: page
title: Power, battery and thermal behaviour
permalink: /pages/reference-power-and-battery/
---

## Power, battery and thermal behaviour

A plugin that wakes up often uses CPU time, and on a laptop CPU time costs battery. This page states how the host changes its budgets when the machine runs on battery, when a power-saving mode is on, or when the CPU throttles for heat. It also says what a plugin does to stay inside the lower budgets.

<div class="warn">
<p>The rules on this page are proposed requirements with defaults. The host does not change its budgets for power state today. Each rule that is a decision to confirm is marked as proposed. Do not design a plugin that depends on a rule here until its requirement is shipped and its test passes.</p>
</div>

### What the host reads

The host reads one power value. It says whether the machine runs on battery and whether a power-saving mode is on. Plugins read that value from the host. A plugin does not probe the operating system for its own power state, because the differences between platforms are handled in one place, in the host. The name and shape of the call that returns the value are an open decision ([Limits of this page](#limits-of-this-page)).

### Budgets on battery

| Budget | On mains power | On battery or in a power-saving mode | Status |
|---|---|---|---|
| Scene frame rate | The `frame_rate_cap` that the scene asks for, up to 60 fps ([Scenes](/pages/api-scenes/)) | At most 30 fps | proposed |
| Ordinary timer interval | Under 50 ms is refused with `invalid-argument` ([Errors and limits](/pages/reference-errors-and-limits/)) | Not under 250 ms | proposed |
| Idle wake-ups of the whole host | At most 100 per second ([B24 in the budgets](/pages/reference-host-budgets/)) | Still at most 100 per second, and a plugin targets 10 | B24 is current; the plugin target is proposed |
| Execution bucket and deadlines | 2,000 ms capacity, refills at 500 ms per second, and the deadlines of [Performance](/pages/performance/) | Unchanged | current rule |

### Rules for power state

- **[PRF-032]** The host MUST give plugins one power value that says whether the machine is on battery or in a power-saving mode, and a plugin MUST NOT probe the operating system for that state itself. Test: a fake host that unplugs the machine changes the value that a plugin reads within 5 seconds, and the plugin reads the old value only before the change.
- **[PRF-033]** While the machine is on battery or in a power-saving mode, the host MUST cap every scene frame rate at 30 fps, whatever the `frame_rate_cap` of the scene. (proposed, owner to confirm: the 30 fps figure is a default.) Test: a scene that asks for 60 fps runs at 30 fps on battery and at 60 fps on mains power.
- **[PRF-034]** While the machine is on battery or in a power-saving mode, the host MUST NOT fire an ordinary timer at an interval under 250 ms. (proposed, owner to confirm: the 250 ms floor is a default.) Test: a timer armed at 50 ms fires no more often than every 250 ms on battery and every 50 ms on mains power.
- **[PRF-035]** The host MUST NOT scale deadlines, the execution bucket or its refill rate when the CPU throttles for heat or the operating system lowers the clock. A deadline missed during a throttle MUST count as an ordinary deadline breach. (proposed, owner to confirm: scaling deadlines was rejected because it would change the budgets of [Performance budgets](/pages/reference-host-budgets/).) Test: a fixture that burns 25 ms of CPU in a handler is trapped at the 20 ms deadline, with or without a throttled core.
- **[PRF-036]** A plugin SHOULD keep its idle wake-ups, counted as timer expiries and poll wake-ups with no event to handle, at 10 per second or fewer, so that a set of plugins stays inside the B24 budget on battery. Test: a plugin with one 250 ms timer and no events shows 4 or fewer wake-ups per second in a 60 second run.

### Limits of this page

- Decision needed: whether the battery rules are on by default. Default: on. The switch that turns them off is not designed yet.
- Decision needed: the name and shape of the power value. Default: one value with two flags, one for battery and one for a power-saving mode.
- Decision needed: whether the host reads a thermal signal at all. Default: it does not. Only [PRF-035](#rules-for-power-state) states what happens to deadlines during a throttle.
- The 30 fps and 250 ms figures are defaults. A change to either one changes the budgets of plugins that are sized against them.

### See also

- [Sizing memory and CPU](/pages/perf-sizing-memory-and-cpu/) for the execution bucket and the wake-up budget.
- [Performance budgets and benchmarks](/pages/reference-host-budgets/) for B24, the idle cost budget.
- [Performance](/pages/performance/) for the deadlines and the single table of budgets.
- [Host scheduling](/pages/reference-host-scheduling/) for how timers and clock jumps are scheduled.
- [Scenes](/pages/api-scenes/) for `frame_rate_cap`.
