# Minimum system requirements

This page lists what an Ilium host needs to run, and what the repository establishes about it. Where the repository does not establish a value, the value is an open decision and is listed at the end. No requirement on this page has a requirement ID yet, because the open values must be decided first.

See also: [Platform support](reference-platform-support.md), [Install and update](reference-install-and-update.md).

## Shipped platforms

The host ships for these operating system and architecture pairs. The sandbox tiers each one runs are listed in [Platform support](reference-platform-support.md).

| Platform | Architecture |
|----------|--------------|
| Linux | x86_64, aarch64 |
| macOS | aarch64 |
| Windows | x86_64 |

## Linux C library

Linux packages target glibc 2.35, the baseline of Ubuntu 22.04 and Debian 12 (`src/docs/installation.md`). A Linux system with an older glibc cannot run the packages. Release builds run on `ubuntu-22.04` in CI (`.github/workflows/release.yml`).

## Toolchain

The Rust toolchain is pinned to 1.96.1 in `rust-toolchain.toml`. This pin applies to building Ilium from source. It does not change what a running host needs.

## Open decisions

These values are not established by the repository. Each one must be decided before it can become a requirement.

- **Minimum macOS release.** Not stated anywhere in the manual. Consequence: a Mac on an older release has no documented support boundary.
- **Minimum Windows release.** Not stated anywhere in the manual. Consequence: the installer cannot check it yet. The CI image `windows-2025` is only the build machine, not a support floor.
- **Minimum RAM and free disk.** Not stated anywhere in the manual. The performance budgets in `reference-host-budgets.md` bound the memory the host adds with the bundled plugins (HST-309), not the memory the system needs.
- **Installer prerequisite checks.** The manual does not specify which checks the installer runs before installing. Consequence: an unsupported system fails late, or not at all.
- **Requirement IDs for this page.** The IDs HST-306 to HST-310 are already used by the performance budgets. A new range must be chosen, and the rows added together with tests, so the coverage check stays at zero uncovered IDs.
