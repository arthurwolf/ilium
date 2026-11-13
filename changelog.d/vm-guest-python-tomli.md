### Fixed

- Fixed the Ubuntu 22.04 VM package check failing on startup because its Python 3.10 lacks `tomllib`; the guest now installs `python3-tomli` and the release tooling falls back to it.
