### Fixed
- A very large terminal replay no longer stalls the pane: the history memory counter now includes the space reserved when a replay starts, so trimming old output can no longer underflow it.
