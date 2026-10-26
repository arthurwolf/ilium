### Added
- Shared same-machine resource queues for GPU and USB-board coordination, project creation/linking, agent reservation commands and blocking waits with native queue progress.
- Resource tree rows, waiting badges, optional display-only agent grouping, queue details, managed agent instructions and guarded recovery for uncertain hardware ownership.

### Fixed
- Plugin verification fixtures now initialize the optional installer, and local install errors retain grant-store context.
- Resource catalog I/O retains an admitted payload lease without exhausting the general job-result budget, so logging and attaching requests cannot deadlock queue snapshots.
- Retry temporary allocation-ledger contention inside resource operations while continuing to reject exhausted quota without waiting on retained catalog memory.
