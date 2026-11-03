//! Red tests of the install-deps lane: install, update, uninstall and dependency resolution
//! (PKG-260 .. PKG-348), derived from the manual pages. Items the skeleton lacks are listed in
//! API_GAPS.md. Part 2 of the lane extends the modules below.

mod support;

mod reference_dependency_resolution;
mod reference_dependency_resolution_p2;
mod reference_install_and_update;
mod inst_install_flags;
mod inst_locked_bundle;
mod inst_refusals;
mod inst_registry;
mod inst_rollback_uninstall;
mod inst_unskip_backup;
