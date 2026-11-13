# Agent monitoring and local control

The server owns a bounded event log and session-lifetime consumer cursors. `ilium-control` owns pure filtering, paging, digest ranking, evidence acknowledgement, brief shaping, wait selection and access policy. IPC and loopback HTTP adapt those contracts; neither requires a screen read for event monitoring. The shared attention kind and fingerprint live in `ilium-core` and are also used by the client's existing attention queue.

Provider text is an adapter concern. The server captures the process and transcript source, performs a bounded tail read on the finite I/O lane, and revalidates process birth, held descriptor, file identity and current invocation before publishing text. New-turn records invalidate earlier messages. Unknown evidence is explicit.

Pane briefs observe state under the tree lock and recheck it after transcript I/O. State age comes from a monotonic server observation and survives identical tree refreshes. Restored state without a recorded transition has unknown age.

The runtime token file has distinct read, act and admin roles. The CLI applies the shared policy before requesting a durable server audit preflight; HTTP authenticates every request and authorizes mutations before dispatch. Existing user-owned UI IPC stays in the same operating-system trust domain: this is an automation safety boundary, not isolation from another process owned by that user. A preflight audit is an attempt record, not an execution receipt. A malicious same-user raw IPC caller can bypass the CLI, just as it can read every token.

Audit appends run on the finite I/O lane. An owned mutex guard remains with the physical writer if the caller is cancelled; the writer synchronizes the file and parent before acknowledging authorization. Writes fail closed when the audit cannot be committed. The log rotates independently from the best-effort lifecycle queue, whose drop policy is unsuitable for authorization auditing.


Cross-session discovery and cancellation live in the CLI adapter. Reads connect directly to each known endpoint, without rediscovering the socket set per request. Monitoring reads never attach to the UI stream. Every result and cursor is scoped by full socket identity; a global budget admits complete session pages only and preserves the input cursor for omitted pages. Follow owns its reader task set and cancels remaining connections after a successful response. Tree/find and pane-selection waits reuse the existing attached document reader. No cross-session write fanout exists.

Acknowledgement evidence and static context belong to an exact bounded delivery receipt, not a sequential cursor alone. Priority ordering can deliver different pages at the same safe sequence; only the returned receipt commits that page. Federation may omit pages without committing their evidence. Pending receipt eviction is explicit and requires a fresh read. Context fingerprints are shared across event and digest readers for each consumer and server lifetime.

Credential and audit admission opens nonblocking, no-follow private regular handles before accessing data. Optional credential failure leaves ordinary server startup available and automation authorization unavailable. Unsupported ownership or durability primitives never become an implicit success.
