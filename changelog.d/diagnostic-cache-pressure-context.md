### Fixed
- Statistics cache-pressure reports identify the deferred pane, background or interactive request and bounded cache/read occupancy, and stop appearing when the capacity condition resolves. Retained operation failures are labelled as potentially historical instead of implying a current block; their first error is no longer duplicated as the explanation.
- Resolving a statistics warning also clears its owned status/F9 copy, while preserving a newer unrelated operation message.
