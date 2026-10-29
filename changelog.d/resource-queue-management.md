### Added
- Shared same-machine resource queues for GPU and USB-board coordination, project creation/linking, agent reservation commands and blocking waits with native queue progress.
- Resource tree rows, waiting badges, optional display-only agent grouping, queue details, managed agent instructions and guarded recovery for uncertain hardware ownership.

### Fixed
- Explicit resource command project directories now take precedence over the enclosing Ilium pane, preventing mutations in the wrong project.
- Plugin verification fixtures now initialize the optional installer, and local install errors retain grant-store context.
- Resource catalog I/O retains an admitted payload lease without exhausting the general job-result budget, so logging and attaching requests cannot deadlock queue snapshots.
- Retry temporary allocation-ledger contention inside resource operations while continuing to reject exhausted quota without waiting on retained catalog memory.
- Resource catalog workspace retains its full storage admission without monopolizing the general input budget used by attaching clients and other I/O.
- Resource queue tree actions support Home/End, reject virtual-row rename requests and cancel invalid stale drag releases while preserving real pane targets.
- Resource commands retry explicit temporary catalog-lock contention with the same request identity, while uncertain outcomes and revision conflicts remain explicit errors.
- Resource observers leave a full interval after slow catalog work instead of bursting through missed ticks; attached waits survive temporary catalog contention.
- Resource catalog requests queue fairly within a server before acquiring the shared cross-project lock, without consuming worker capacity while waiting.
- Cross-project owner checks distinguish server generations when pane numbers coincide, and resource rows refresh when the project tree arrives after the resource catalog.
- Blocking waits report cancellation and revocation distinctly when a queued reservation ends.
- Resource instruction-change notices wait until the project's managed instruction projection is synchronized.
- Resource forms retain a submit made while the authoritative catalog is loading, and report a failed load without discarding the form.
- Resource forms preserve deferred Submit through temporary catalog contention, retrying only the catalog read before sending the original mutation identity.
- Unchanged resource catalog reads reuse a verified open-file durability proof, avoiding redundant filesystem flushes while reconciling every replacement before use.
- Resource metadata snapshots report temporary allocation-ledger contention as retryable busy, while exhausted capacity remains an explicit admission refusal.
- Cross-project resource participants retain a bounded readable pane title instead of a generic pane number; invocation fencing remains independent of labels.

- Keep resource metadata autosave usable when a newer queue update arrives before its successful save reply, while retaining conflicts for concurrent metadata edits.

- Retry interrupted final-unlink instruction/settings cleanup while the project is open and when it is reopened, including after server restart.

- Reject older terminal protocol layouts after integrating screen previews, preventing event misinterpretation between mismatched client and server builds.
- Close the resource link form after a successful save when a newer catalog update arrives first, while retaining the form if the binding was subsequently removed.
- Retry initial resource snapshots after temporary shared-catalog contention so project resource rows recover without a reconnect.
