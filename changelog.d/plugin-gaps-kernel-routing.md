### Added
- Plugin topics and service calls cross between the server and the client runtime. A service provider on the other side answers within 5 seconds or the call ends with `timed-out`; the provider sees the caller as `remote:<plugin id>`.
- Server plugins can send signals to a pane process (`signal-pane`) and open or close panels, settings pages, onboarding steps and popups in every attached client.
- The plugin wire protocol carries CLI invocation records, forwarded intents, a pane signal request and a plugin pane kind.

### Changed
- The plugin wire protocol version is now 2. A client built before this change is refused with `ipc.protocol-mismatch` until it is updated.
