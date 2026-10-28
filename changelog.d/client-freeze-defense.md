### Fixed

- Distinguish impossible incoming projection allocations from temporary memory pressure, retaining the original event and reporting the exact refusal.
- Record terminal presenter phases, frame and byte progress, acknowledgement age, and stalled-output warnings in local diagnostics without waiting for screen redraw.
- Include retained decoder ownership in the original admission envelope without reducing maximum frame/allocation capacity or increasing the combined declared storage ceiling.
