//! Lane `imports-ui`, part 1: the WIT host imports of the data, IO and UI interfaces
//! (audio, catalogs, cli, client-requests, clipboard, geocode, gpu, help, http-api, inference,
//! kernel-settings, keymap, keys, location, media, plugins, sessions, sounds, ui-host, vector,
//! workspace) and the `inference-provider` export. Spec: docs/plugins-manual/pages/api-*.md.
//! Skeleton gaps: API_GAPS.md.

mod api_audio;
mod api_catalogs;
mod api_cli;
mod api_client_requests;
mod api_clipboard;
mod api_geocode;
mod api_gpu;
mod api_help_and_plugins;
mod api_http_api;
mod api_inference;
mod api_kernel_settings_and_keymap;
mod api_keys;
mod api_location;
mod api_media;
mod api_sessions;
mod api_ui;
mod api_vector;
mod api_workspace;
mod capability_sound_play;
mod sound_notify_a11y;
mod common;
