### Fixed
- Keep terminal input and event handling responsive under sustained output by bounding client batches by elapsed time and avoiding repeated full projection scans for terminal chunks.
- Release failed IPC connections promptly, cancel blocked reader handoffs during shutdown, bound partial frame transfers, and size codec reservations from their audited allocation limits and actual inputs.
- Reduce PTY deadline-worker wakeups and queue allocation churn while preserving terminal command order and cancellation delivery.
- Retain frozen-screen restores through temporary parser pressure, bound their memory and shutdown lifecycle, and handle growing or interrupted snapshot reads safely.
- Prevent lost writer-completion wakeups in terminal operation and shutdown, so completed writes cannot leave input waiting until a deadline.
- Isolate deep request and repository-query polling in awaited cancellation-owned tasks to contain debug-server stack depth while retaining request order and reservations.
