### Fixed

- Fixed the native sandbox qualification step rejecting its checksum-verified test binary and helper because downloading the workflow artifact drops the execute permission; the runner now restores it after verifying the checksums.
