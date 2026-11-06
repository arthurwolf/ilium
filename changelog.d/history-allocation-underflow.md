### Fixed
- Replaying a large terminal history no longer panics with an arithmetic underflow while trimming: the replay's reserved history segment is now counted in the history allocation total.
