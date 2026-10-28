# API gaps of the voice port twins (svc01)

The twins in `tests/port/{studio,device_startup,prep,ui_ports,submit}.rs` are adapted to the plugin API; the monolith tests reached host internals (byte banks, admission, pointer identity). What the plugin cannot observe is stated per item.

1. Sound studio: assumed to be the onboarding step `sound`; numbers are kernel settings `sound` / `design.{waveform,pitch_hz,sweep,pulse_rate}` (pulse rate in tenths of Hz, 0..160); numeric entry is a `Submitted` action with text; focus is marked by a `▸` row prefix; the numeric open glyph is `*` and the slider track `━` (not in the manual).
2. Studio preview: a job of kind `sound-preview` whose JSON input has `studio` (identity) and `revision`; "Preview ready" is the assumed status text.
3. `FakeHost::set_preview_revision(u64)` is a local trait stub (`unimplemented!`) in studio.rs; revision wrap (port_004) needs it. The min/max round-trip of every control, and the (3,2) offset reachability case, are only partly expressible (clamping is checked, exact ranges only for pulse rate).
4. port_014 uses the import name `audio.devices` for the refusal.
5. port_015 to port_017 cover the observable result of the byte audit, cancelled observation and catalogue move (names reach the capture unchanged); the byte accounting itself is not observable.
6. Voice preparation (port_024 to port_029): the byte bank, recharge and source ownership are host internals; the twins assert system prompt, retry, fencing and cancel outcomes.
7. UI (port_020 to port_023): popup keys, the strip dot glyph and `Danger` role for "red" are assumed; "black when disabled" is checked as no strip content.
8. port_045: the real PTY reader is host behaviour; the twin asserts the `send-input` intent with `submitted` delivery. `set_active_agent` takes the pane name.
