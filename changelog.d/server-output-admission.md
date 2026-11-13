### Fixed
- Keep queued server replies and terminal recovery draining when shared event storage is full, while retaining real flush acknowledgments, bounded encoding and CPU retirement. Ordinary slow peers cannot consume the visible terminal's reserved output storage.
- Preserve large sparse-event input capacity and concurrent encoding of small frames while reserving output storage before encoding.
