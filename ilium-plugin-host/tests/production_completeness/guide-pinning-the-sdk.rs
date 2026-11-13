/// REQ: PDX-001
#[test]
fn pdx_001_commit_lockfile_or_fail_unpinned() {
    // Observable: Delete Cargo.lock from a Rust template project and run `ilium plugin check --release`: the exit code is non-zero and the output contains `build.unpinned-input`.
    panic!(
        "PDX-001 not implemented: A plugin project MUST commit its lockfile (`Cargo.lock` for Rust, `bun.lock` for TypeScript), and a release build or `ilium plugin check --release` without one MUST fail with `build.unpinned-input` as defined by [DEV-893]."
    );
}

/// REQ: PDX-002
#[test]
fn pdx_002_tilde_pin_and_open_range_warning() {
    // Observable: Change the SDK requirement in package.json to `^0.4.2` and run `ilium plugin check`: the output contains one `lint.sdk-range-open` warning naming package.json.
    panic!(
        "PDX-002 not implemented: The templates MUST pin the SDK to a tilde-style minor range (Rust `ilium-plugin-sdk = '~0.4.2'`, TypeScript `'~0.4.2'`), and `ilium plugin check` MUST warn with `lint.sdk-range-open` for any requirement that admits a higher minor, such as `*`, `^0.4` or a bare major."
    );
}

/// REQ: PDX-003
#[test]
fn pdx_003_build_json_sdk_host_range_mismatch() {
    // Observable: Set manifest `api` to a minor one above the resolved SDK's highest declared host minor and run `ilium plugin build`: the exit code is non-zero, the output contains `build.sdk-host-mismatch`, and no dist/ package is written.
    panic!(
        "PDX-003 not implemented: `dist/build.json` MUST record the resolved SDK version together with the lowest and highest host API minors that SDK declares, and `ilium plugin build` MUST fail with `build.sdk-host-mismatch` when the manifest `api` is outside that range."
    );
}

/// REQ: PDX-004
#[test]
fn pdx_004_mixed_sdk_line_fails_build() {
    // Observable: Pin the Rust SDK to `~0.4.2` and the TypeScript SDK to `~0.5.0` in one project and run `ilium plugin build`: the output contains `build.sdk-line-mixed` and the exit code is non-zero.
    panic!(
        "PDX-004 not implemented: When one plugin uses both the Rust and the TypeScript SDK, they MUST resolve to the same SDK line (same major.minor), otherwise `ilium plugin build` MUST fail with `build.sdk-line-mixed`."
    );
}

/// REQ: PDX-005
#[test]
fn pdx_005_patch_bump_keeps_exported_signatures() {
    // Observable: Resolve the SDK from 0.4.2 to 0.4.3 in the lockfile and rebuild without source changes: the build succeeds and the exported WIT interface hash in dist/build.json is unchanged.
    panic!(
        "PDX-005 not implemented: A patch bump within a pinned SDK line MUST NOT change any exported signature (per [API-083] and [API-086]), so an author MAY accept patch updates automatically, while a minor bump MUST be made in its own commit with `ilium plugin check` re-run."
    );
}

/// REQ: PDX-006
#[test]
fn pdx_006_deprecated_item_fails_check() {
    // Observable: Use an item whose removal minor is 0.5 and pin the SDK to `~0.5.0`: `ilium plugin check` exits non-zero and prints `lint.sdk-deprecated` with the item name and the removal minor.
    panic!(
        "PDX-006 not implemented: `ilium plugin check` MUST fail with `lint.sdk-deprecated` as an error when the resolved SDK minor equals or exceeds the removal minor of an item the plugin still uses ([DEV-997]), so the pin is moved only after the item is replaced."
    );
}
