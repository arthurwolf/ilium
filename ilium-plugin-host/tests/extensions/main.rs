//! Lane extensions-services: extension points, contributions, hook chains, menus, core plugin
//! replacement, services, topics, peer messages and extension security (test plan 2.2 and 4.2).
//!
//! One test binary; one module per manual page. Part 1 of 2 wrote the modules below; part 2
//! appends its own `mod` lines at the end.

mod support;

mod api_commands;
mod api_extension_points;
mod api_extension_points_chains;
mod api_extension_points_limits;
mod api_menu_provider;
mod api_messages_and_topics;
mod api_services;
mod extending_core_plugins;
mod reference_core_extension_points;
mod reference_core_replacement_matrix;
mod reference_extension_security;
mod reference_wit_exports;
mod wit_extensions_register;
