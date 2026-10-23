# Minimum system requirements

This page lists what an Ilium host needs to run, and what the repository establishes about it. Where the repository does not establish a value, the value is an open decision and is listed at the end. A requirement carries an ID only when its value is decided, or when it is marked "(proposed, owner to confirm)" until the owner accepts it.

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

## Operating system floor

The installer checks the operating system and architecture before it unpacks or writes anything. The floors below are proposed values; the owner must confirm them before the rows are treated as final.

- **[HST-955]** The installer MUST accept only the platform pairs listed under Shipped platforms. Any other operating system or architecture MUST stop the install with error code `install.unsupported-platform` (proposed, owner to confirm), naming the detected operating system and architecture.
- **[HST-956]** On macOS aarch64 the minimum release is macOS 14 Sonoma (proposed, owner to confirm). The installer MUST refuse an earlier release with `install.unsupported-platform`, naming the detected release and the minimum.
- **[HST-957]** On Windows x86_64 the minimum release is Windows 11, version 22H2, build 22621 (proposed, owner to confirm). The installer MUST refuse an earlier build with `install.unsupported-platform`, naming the detected build and the minimum. Windows 10 is not accepted, because Microsoft ended its mainstream support in October 2025.
- **[HST-958]** A refusal under HST-955 to HST-957 MUST happen before any file is written to the install location. The installer MUST exit with status 1, MUST print one line with the error code and the detected and minimum values, and MUST leave no installed or half-installed host, as [SEC-437] requires for a failed checksum.

## Open decisions

These values are not established by the repository. Each one must be decided before it can become a requirement.

- **Minimum RAM and free disk.** Not stated anywhere in the manual. The performance budgets in `reference-host-budgets.md` bound the memory the host adds with the bundled plugins (HST-309), not the memory the system needs.
- **Installer checks beyond the operating system.** HST-955 to HST-958 cover the operating system and architecture only. The manual does not yet specify a RAM or free-disk check. Consequence: a system short of either fails late, or not at all.
- **Requirement IDs for this page.** HST-306 to HST-310 are already used by the performance budgets. The operating system floor uses HST-955 to HST-958. Any new value on this page takes its own unused ID in the HST range, and its row is added together with its test, so the coverage check stays at zero uncovered IDs.
