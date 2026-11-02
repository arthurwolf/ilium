### Fixed
- Run launch-directory validation and repository-boundary inspection on admitted I/O workers so slow filesystems do not block the asynchronous server runtime. Preserve the final directory identity fence and report admission and filesystem errors.
