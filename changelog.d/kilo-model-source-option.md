### Added
- Settings → Inference has a "Kilo model source" row: use the model-scanner's recommendation (default) or a manual Kilo model. Manual mode sends the model you choose and never reads the scanner; the catalog and model rows appear only in that mode.

### Fixed
- Kilo requests no longer fail with "no qualified base model-scanner recommendation" when the scanner publishes a newer benchmark version or a larger question suite; the recommendation is now validated for internal consistency instead of against a pinned version and question count.
