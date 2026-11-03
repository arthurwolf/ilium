---
layout: page
title: Benchmark workflow
permalink: /pages/perf-benchmark-workflow/
---

## Benchmark workflow

A plugin passes its tests and can still drift toward a deadline release by release. This page is the author's workflow for finding out how fast a handler really is, deciding whether that is fast enough, and keeping it there. It uses four commands: `ilium plugin bench` for a synthetic timing of each export, `ilium plugin profile` for timings from a running plugin, `ilium plugin dev --timings` while you edit, and `ilium plugin stats` for memory and bucket use. The CI gate that compares every build with the last release is in [CI recipes](/pages/guide-ci-recipes/#performance-regression-gate). This page covers the local work that comes before that gate.

The host's own benchmarks, such as the 1 µs host call and the 2 ms batch to 30 plugins, are in [Performance budgets and benchmarks](/pages/reference-host-budgets/). They measure the host, not your plugin.

### You need

- A release build of the plugin. `ilium plugin build --release` produces it. A debug build is slower by a large factor, so its timings do not describe the plugin you ship.
- At least one fixture for the export you want to time. Fixtures are the inputs that `test` and `bench` use. The fixture format is in [Test harness reference](/pages/reference-test-harness/).
- The deadlines of your tier, from [Performance](/pages/performance/#deadlines-and-budgets). The defaults are 20 ms for `on-events` and 10 ms for `views.render`.

### Two kinds of timing

Two commands time the plugin, and they answer different questions. Do not mix their numbers.

| Command | What it times | Clock and host | Use it for |
|---|---|---|---|
| `ilium plugin bench` | Each export of the built component, many iterations | Real clock, embedded test host, every record marked `synthetic: true` | Comparing two builds of the same plugin on the same machine |
| `ilium plugin profile <id> --duration <seconds>` | Each export that ran in a live session, one record per call | The running host | Checking what a real session costs |

`bench` tells you how fast the handler is in isolation. `profile` tells you how often the handler runs and how long it took in use. A handler that is fast in `bench` can still be slow in a session if it runs very often, which is why both are needed.

### What bench prints and how it exits

`ilium plugin bench [--fixture <name>] [--iterations <n>]` calls the built component after 10 warm-up iterations. It runs the measured iterations, 100 by default (proposed, owner to confirm), and prints one `timing` record per export with `p50_ms`, `p99_ms` and `max_ms` ([Plugin command-line reference](/pages/reference-plugin-cli/)). The exit code says what to do next.

| Exit | Meaning | Action |
|---|---|---|
| 0 | Every export's p99 is within its manifest `deadline_ms` | Continue. Compare with a baseline if you have one. |
| 3 | One or more exports have a p99 above their `deadline_ms` | Find the cause and fix it. Do not raise the deadline to pass. |
| 1 or 2 | A usage or setup error, such as a missing build or a bad flag | Fix the command. The timing is not valid. |

<pre class="term"><span class="t-prompt">$</span> ilium plugin build --release
<span class="t-prompt">$</span> ilium plugin bench --fixture typical-batch --iterations 200
<span class="t-dim">{"type":"timing","export":"on-events","p50_ms":1.8,"p99_ms":6.4,"max_ms":9.1,"deadline_ms":20,"synthetic":true}</span>
<span class="t-dim">{"type":"timing","export":"render","p50_ms":0.4,"p99_ms":1.1,"max_ms":2.0,"deadline_ms":10,"synthetic":true}</span>
<span class="t-dim">{"type":"summary","exports":2,"over_deadline":0}</span>
</pre>

Illustrative, not generated. Both exports are well under their deadlines. The `synthetic` field is always set, so a bench result is never mistaken for a session timing.

<div class="note">
<p>Bench uses the embedded test host, not a live session. A live session also runs other plugins, the detection loop and the terminal, so its timings can differ from bench. Use <code>profile</code> to see the real session.</p>
</div>

- **[PERF-020]** `ilium plugin bench` MUST label every `timing` record `synthetic: true`, and an author MUST NOT present a bench result as a session timing. Test: run bench on a fixture and assert that every `timing` record has `synthetic` set to true.
- **[PERF-021]** `ilium plugin bench` MUST exit 3 when any export's p99 is above its manifest `deadline_ms`, and MUST exit 0 when every export's p99 is within it. Test: a fixture whose handler sleeps 25 ms exits 3 for `on-events`, and a fixture that sleeps 5 ms exits 0.
- **[PERF-022]** A plugin author MUST NOT raise a `deadline_ms` in the manifest, or a `limits` value above the default of its tier, to make a bench or a CI gate pass unless the new value follows from a measured need. The commit that makes the change MUST name the measured numbers. Test: a review checklist item requires the measured numbers in the commit message.

### Reading the numbers

The p99 is the value that the deadline applies to. A deadline breach happens on the calls that take longest, so the tail matters more than the median. The p50 shows the typical cost. The max is a single outlier and is noisy on a shared machine, so use it only to find a call that is much slower than the rest.

- **[PERF-023]** A handler SHOULD have a bench p99 of at most half its deadline, which is 10 ms for `on-events` and 5 ms for `views.render` at the defaults. (proposed, owner to confirm: the half-of-deadline target matches the warning colour in the handler budget meter in [Performance](/pages/performance/#measuring-a-handler), but no specification line requires it.) Test: a bench run of the reference fixture reports a p99 under 10 ms for `on-events`.

### Measuring the spread

A benchmark on a laptop can vary by 10 percent or more between runs. A change that looks like a 15 percent gain can be noise. Measure the spread before you trust a difference.

1. Build the current commit in release mode.
2. Run `bench` ten times in a row, with the same `--iterations`, and write each p99 down.
3. The spread is the largest p99 minus the smallest, divided by the smallest. Keep the record with the commit so it can be read later.
4. Make a change, build it, and run the same ten runs. Compare the two sets. A difference smaller than the spread is not a result.

The CI gate uses a tolerance of 20 percent as a start, and recommends a tolerance above the spread ([CI recipes](/pages/guide-ci-recipes/#performance-regression-gate)).

- **[PERF-024]** A timing claim in a commit message, a changelog entry or a review SHOULD give the p99 of at least ten runs and the spread of those runs, so that a reader can tell a change from noise. Test: a review checklist item checks the message for both numbers.

### Comparing with a baseline

The CI gate reads a committed file, `bench/baseline.json`, and fails when a metric is more than the tolerance above it. The same comparison works on your machine, with `--baseline` and `--max-regression`.

<pre class="term"><span class="t-prompt">$</span> ilium plugin bench --iterations 200 --baseline bench/baseline.json --max-regression 20
<span class="t-dim">{"type":"bench.regression","export":"on-events","metric":"p99_ms","baseline":5.9,"measured":7.4,"percent":25.4}</span>
</pre>

Illustrative, not generated. The p99 of `on-events` is 25.4 percent above the baseline, so the command exits 3.

- **[PERF-025]** A baseline MUST be refreshed only in a commit of its own, after a release whose slower numbers were reviewed and accepted, or after a change of runner. Test: a commit that changes `bench/baseline.json` and any other file is refused by the review checklist.

Run bench on a dedicated runner, or on a runner with the same size for every run. Shared runners add scheduling noise, so a baseline from a quiet machine fails on a busy one for no reason of the plugin's.

### Measuring in a live session

`ilium plugin profile <id> --duration <seconds>` records the timings of every export that runs in the session during the time you give. It prints one timing record per call. Use it after a change to see how often each export runs and whether the real calls are close to the deadline.

Run the profile while you use the plugin in the way a user would. A profile of an idle plugin shows almost nothing, and a profile of a busy plugin shows where the time goes. For the timings while you edit, `ilium plugin dev --timings` prints the time per export call during a development session.

- **[PERF-026]** `ilium plugin profile` MUST print one timing record for each export call that ran during the profile, and MUST NOT change the call time of the plugin by more than 5 percent. This is the rule in [Performance targets](/pages/performance/#performance-targets) (DEV-850). Test: profile a fixture plugin with and without profiling and compare the call times.

### Memory alongside time

A handler can be fast and still use too much memory. Read `ilium plugin stats <id>` after the same run. The memory line shows the use and the limit, and the sizing rules are in [Sizing memory and CPU](/pages/perf-sizing-memory-and-cpu/#measuring-memory). Measure the peak after the largest realistic input, not after the first small one.

### Worked example: a handler that creeps toward its deadline

A plugin's `on-events` handler filters a list of notes on every batch. The team added a feature that makes the list 40 percent longer, and the bench p99 rises from 6 ms to 14 ms.

1. The author runs `ilium plugin build --release`, then `ilium plugin bench --iterations 200`. The p99 is 14 ms, above the 10 ms half-of-deadline target in PERF-023, but still under the 20 ms deadline. Bench exits 0.
2. The author runs the spread measurement from the section above. The spread is 4 percent, so a change of 8 ms is a real change.
3. The author checks the live profile. The handler runs 30 times a second while the user types, so the work adds up to about 420 ms per second. That is above the 250 ms per second average in PERF-004, so the bucket is at risk.
4. The fix moves the filter into a job that runs when the user stops typing for 300 ms. The handler now only records the query.
5. The author runs bench again. The p99 of `on-events` is 1.2 ms. A new profile shows the handler's spend falls well below the 250 ms per second average of PERF-004.
6. The change goes in a commit that names the numbers from steps 1, 2, 3 and 5. The baseline is not refreshed in the same commit, as PERF-025 requires.

### Limits of this page

- The fixture format and the `bench` fixture names are defined in [Test harness reference](/pages/reference-test-harness/). This page does not restate them. (Gap: the page does not state how a fixture is selected by name when several exist. Confirm with the owner of the test harness reference.)
- The default of 100 measured iterations for bench is marked proposed in [Plugin command-line reference](/pages/reference-plugin-cli/).
- The output of `profile` is a stream of records. This page shows the command and its purpose, but not the record fields, because they are not fixed in the pages read for this manual.

### See also

- [Performance](/pages/performance/) for the deadlines, the budgets and the measuring commands in brief.
- [CI recipes](/pages/guide-ci-recipes/#performance-regression-gate) for the regression gate that runs the baseline comparison on every build.
- [Sizing memory and CPU](/pages/perf-sizing-memory-and-cpu/) for the memory and bucket budgets that a timing must fit inside.
- [Backpressure and retries](/pages/perf-backpressure-and-retries/) for what a plugin does when a timing is fine but a call is still refused.
- [Performance budgets and benchmarks](/pages/reference-host-budgets/) for the host's own benchmarks.
