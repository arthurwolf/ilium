# API gaps of the attention feature tests

Items the tests call that the skeleton lacks, or manual details the tests had to assume.

- None yet: the crate is a skeleton and has no feature tests.

- Port status: `src/item.rs` (kinds, briefing, stable FNV-1a fingerprint; 5 kernel unit tests) and `src/parse.rs` (lenient reading of the LLM attention list; 7 kernel unit tests) are ported with their tests. The queue, instant and turbo evaluators, settings, briefing, panel and the `next`, `panel` and `overview` commands are not: the commands need a focus intent and `MarkAttention`/`AckAttention` (G3), the briefing needs the `inference` import (G10), the panel needs paging and text entry (G8), and the activity topic does not cross sides (G4). The manifest now declares the commands, keybindings, panel and settings of the page; the plugin does not handle the commands yet.
