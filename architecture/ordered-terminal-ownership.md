## Ordered terminal ownership

Terminal viewports reserve configured toolbar, prompt and progress space before
input. Metadata content and expiry change visibility within those slots; outer
size, explicit settings and split changes remain geometry inputs.

The PTY owner orders bounded commands and output. It acquires parser mutation
ownership before changing native geometry, verifies the result and preserves the
parser on failure. Query replies complete before a following resize; output may
advance during an ordinary pending input, but replies cannot split that payload.
`PtyInput` receipts distinguish admission from delivery and exact prefixes from
unconfirmed writes. Async callers clone that lifetime-bound handle under brief
registry access and await outside shared tree/pane locks. Final mutations verify
the same runtime again.

Screen presentation uses a nonblocking read and a consistent retained frame
captured immediately before native resize. Ordinary output does not clone a
full fallback screen. Automated readiness and input use current-only reads;
parser contention cannot authorize input from retained text. Paste-mode and
Text Trigger reads await screen/status changes outside registry locks with a
bounded wait and a session identity fence. A missed trigger chunk marks its
shadow for authoritative full-screen resynchronization before further scanning.

A terminal failure closes admission, settles active receipts and retains native
control/writer custody until session close. The existing server output forwarder
also observes owner status so failures surface without another keypress, while
already-published bytes remain available. Every worker has a bounded supervisor
slot and a retained join ticket; timeout reports pending ownership, never a false
join. An uninterruptible native resize remains an explicit hard-deadline limit.

Queued prompts persist an attempt fence before body or Enter. An uncertain
attempt retains authored text and blocks automatic replay; successful recurring
entries rotate with a fresh unattempted state. Auto-answer permits at most one
retry, only for proven zero delivery with the same process, dialog and settings.

