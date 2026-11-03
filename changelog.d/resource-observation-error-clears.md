### Fixed

- A transient resource catalog failure such as "Catalog busy" no longer leaves "State unknown" on every resource row until the catalog next changes: the server republishes the catalog after the first successful observation that follows a failure.
