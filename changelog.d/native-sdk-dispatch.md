### Fixed
- The plugin attribute accepts its documented renamed SDK crate argument.
- Native server/client plugin attribute callbacks forward activation, deactivation and event delivery to SDK traits, preserving callback results and errors. Scene callback execution remains unfinished.
