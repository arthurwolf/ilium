//! Package-gap lane: red tests for the package and registry requirements of
//! reference-requirements-manifest-package.md that had no test (PKG-800..828, the planned ids
//! only). Groups: reference-registry (PKG-800, 801, 802, 810, 811, 812, 813, 814, 816, 817, 818,
//! 819, 821, 822, 823, 824), reference-dependency-resolution (PKG-803, 804, 828),
//! reference-package-format (PKG-825) and reference-manifest (PKG-805).
//! Every test is red until its feature exists. The manual is the spec.

mod reference_dependency_resolution;
mod reference_manifest;
mod reference_package_format;
mod reference_registry;
mod support;
