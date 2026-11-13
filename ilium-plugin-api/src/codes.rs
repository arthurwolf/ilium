//! String constants for every namespaced code of the manual: the install and package
//! catalogue of reference-manifest-schema.md ("Error catalogue", retired codes excluded),
//! the trap, extension and settings codes of reference-errors-and-limits.md, plus
//! host.capability-not-requested (ABI-023) and component.wasi-forbidden (reference-host-runtime.md, ABI-024).
//! Generated once from those tables; tests/abi/codes.rs checks that it still matches them.

/// `package.*` codes (reference-manifest-schema.md).
pub mod package {
    /// `package.zip-invalid`.
    pub const ZIP_INVALID: &str = "package.zip-invalid";
    /// `package.zip64`.
    pub const ZIP64: &str = "package.zip64";
    /// `package.zip-compressed`.
    pub const ZIP_COMPRESSED: &str = "package.zip-compressed";
    /// `package.zip-encrypted`.
    pub const ZIP_ENCRYPTED: &str = "package.zip-encrypted";
    /// `package.zip-descriptor`.
    pub const ZIP_DESCRIPTOR: &str = "package.zip-descriptor";
    /// `package.zip-extra`.
    pub const ZIP_EXTRA: &str = "package.zip-extra";
    /// `package.zip-comment`.
    pub const ZIP_COMMENT: &str = "package.zip-comment";
    /// `package.zip-trailing`.
    pub const ZIP_TRAILING: &str = "package.zip-trailing";
    /// `package.header-mismatch`.
    pub const HEADER_MISMATCH: &str = "package.header-mismatch";
    /// `package.crc-mismatch`.
    pub const CRC_MISMATCH: &str = "package.crc-mismatch";
    /// `package.path-invalid`.
    pub const PATH_INVALID: &str = "package.path-invalid";
    /// `package.path-duplicate`.
    pub const PATH_DUPLICATE: &str = "package.path-duplicate";
    /// `package.path-case-collision`.
    pub const PATH_CASE_COLLISION: &str = "package.path-case-collision";
    /// `package.entry-directory`.
    pub const ENTRY_DIRECTORY: &str = "package.entry-directory";
    /// `package.entry-symlink`.
    pub const ENTRY_SYMLINK: &str = "package.entry-symlink";
    /// `package.entry-order`.
    pub const ENTRY_ORDER: &str = "package.entry-order";
    /// `package.entry-unexpected`.
    pub const ENTRY_UNEXPECTED: &str = "package.entry-unexpected";
    /// `package.entry-limit`.
    pub const ENTRY_LIMIT: &str = "package.entry-limit";
    /// `package.size-limit`.
    pub const SIZE_LIMIT: &str = "package.size-limit";
    /// `package.manifest-size`.
    pub const MANIFEST_SIZE: &str = "package.manifest-size";
    /// `package.manifest-missing`.
    pub const MANIFEST_MISSING: &str = "package.manifest-missing";
    /// `package.old-format`.
    pub const OLD_FORMAT: &str = "package.old-format";
    /// `package.precompiled-forbidden`.
    pub const PRECOMPILED_FORBIDDEN: &str = "package.precompiled-forbidden";
    /// `package.component-invalid`.
    pub const COMPONENT_INVALID: &str = "package.component-invalid";
    /// `package.api-section-missing`.
    pub const API_SECTION_MISSING: &str = "package.api-section-missing";
    /// `package.api-section-repeated`.
    pub const API_SECTION_REPEATED: &str = "package.api-section-repeated";
    /// `package.api-section-invalid`.
    pub const API_SECTION_INVALID: &str = "package.api-section-invalid";
    /// `package.api-unsupported`.
    pub const API_UNSUPPORTED: &str = "package.api-unsupported";
    /// `package.signature-format`.
    pub const SIGNATURE_FORMAT: &str = "package.signature-format";
    /// `package.publisher-key-changed`.
    pub const PUBLISHER_KEY_CHANGED: &str = "package.publisher-key-changed";
    /// `package.platform-unsupported`.
    pub const PLATFORM_UNSUPPORTED: &str = "package.platform-unsupported";
    /// `package.downgrade`.
    pub const DOWNGRADE: &str = "package.downgrade";
    /// `package.version-reused`.
    pub const VERSION_REUSED: &str = "package.version-reused";
    /// `package.blocked`.
    pub const BLOCKED: &str = "package.blocked";
    /// `package.corrupt`.
    pub const CORRUPT: &str = "package.corrupt";
    /// `package.cache-missing`.
    pub const CACHE_MISSING: &str = "package.cache-missing";
}

/// `manifest.*` codes (reference-manifest-schema.md).
pub mod manifest {
    /// `manifest.json-invalid`.
    pub const JSON_INVALID: &str = "manifest.json-invalid";
    /// `manifest.duplicate-key`.
    pub const DUPLICATE_KEY: &str = "manifest.duplicate-key";
    /// `manifest.schema`.
    pub const SCHEMA: &str = "manifest.schema";
    /// `manifest.unknown-key`.
    pub const UNKNOWN_KEY: &str = "manifest.unknown-key";
    /// `manifest.key-missing`.
    pub const KEY_MISSING: &str = "manifest.key-missing";
    /// `manifest.id-invalid`.
    pub const ID_INVALID: &str = "manifest.id-invalid";
    /// `manifest.id-reserved`.
    pub const ID_RESERVED: &str = "manifest.id-reserved";
    /// `manifest.version-invalid`.
    pub const VERSION_INVALID: &str = "manifest.version-invalid";
    /// `manifest.display-name`.
    pub const DISPLAY_NAME: &str = "manifest.display-name";
    /// `manifest.data-version`.
    pub const DATA_VERSION: &str = "manifest.data-version";
    /// `manifest.schema-keyword`.
    pub const SCHEMA_KEYWORD: &str = "manifest.schema-keyword";
    /// `manifest.requires-invalid`.
    pub const REQUIRES_INVALID: &str = "manifest.requires-invalid";
    /// `manifest.requires-self`.
    pub const REQUIRES_SELF: &str = "manifest.requires-self";
    /// `manifest.when-syntax`.
    pub const WHEN_SYNTAX: &str = "manifest.when-syntax";
    /// `manifest.when-key`.
    pub const WHEN_KEY: &str = "manifest.when-key";
    /// `manifest.visible-when`.
    pub const VISIBLE_WHEN: &str = "manifest.visible-when";
    /// `manifest.contribution-limit`.
    pub const CONTRIBUTION_LIMIT: &str = "manifest.contribution-limit";
    /// `manifest.row-format`.
    pub const ROW_FORMAT: &str = "manifest.row-format";
    /// `manifest.cli-field`.
    pub const CLI_FIELD: &str = "manifest.cli-field";
    /// `manifest.menu-depth`.
    pub const MENU_DEPTH: &str = "manifest.menu-depth";
    /// `manifest.scene-prerender`.
    pub const SCENE_PRERENDER: &str = "manifest.scene-prerender";
    /// `manifest.settings-kind`.
    pub const SETTINGS_KIND: &str = "manifest.settings-kind";
    /// `manifest.settings-id`.
    pub const SETTINGS_ID: &str = "manifest.settings-id";
    /// `manifest.settings-row-type`.
    pub const SETTINGS_ROW_TYPE: &str = "manifest.settings-row-type";
    /// `manifest.settings-row-field`.
    pub const SETTINGS_ROW_FIELD: &str = "manifest.settings-row-field";
    /// `manifest.settings-default`.
    pub const SETTINGS_DEFAULT: &str = "manifest.settings-default";
    /// `manifest.settings-limit`.
    pub const SETTINGS_LIMIT: &str = "manifest.settings-limit";
    /// `manifest.settings-key`.
    pub const SETTINGS_KEY: &str = "manifest.settings-key";
    /// `manifest.settings-help`.
    pub const SETTINGS_HELP: &str = "manifest.settings-help";
    /// `manifest.settings-command`.
    pub const SETTINGS_COMMAND: &str = "manifest.settings-command";
    /// `manifest.settings-glyph`.
    pub const SETTINGS_GLYPH: &str = "manifest.settings-glyph";
    /// `manifest.settings-source`.
    pub const SETTINGS_SOURCE: &str = "manifest.settings-source";
    /// `manifest.sound-format`.
    pub const SOUND_FORMAT: &str = "manifest.sound-format";
    /// `manifest.sound-id-taken`.
    pub const SOUND_ID_TAKEN: &str = "manifest.sound-id-taken";
    /// `manifest.entry-invalid`.
    pub const ENTRY_INVALID: &str = "manifest.entry-invalid";
    /// `manifest.grant-unknown`.
    pub const GRANT_UNKNOWN: &str = "manifest.grant-unknown";
    /// `manifest.grant-argument`.
    pub const GRANT_ARGUMENT: &str = "manifest.grant-argument";
    /// `manifest.grant-argument-missing`.
    pub const GRANT_ARGUMENT_MISSING: &str = "manifest.grant-argument-missing";
    /// `manifest.grant-wildcard`.
    pub const GRANT_WILDCARD: &str = "manifest.grant-wildcard";
    /// `manifest.grant-side`.
    pub const GRANT_SIDE: &str = "manifest.grant-side";
    /// `manifest.grant-dependency`.
    pub const GRANT_DEPENDENCY: &str = "manifest.grant-dependency";
    /// `manifest.grant-scope`.
    pub const GRANT_SCOPE: &str = "manifest.grant-scope";
    /// `manifest.grant-program`.
    pub const GRANT_PROGRAM: &str = "manifest.grant-program";
    /// `manifest.grant-missing-extension`.
    pub const GRANT_MISSING_EXTENSION: &str = "manifest.grant-missing-extension";
    /// `manifest.settings-id-clash`.
    pub const SETTINGS_ID_CLASH: &str = "manifest.settings-id-clash";
    /// `manifest.settings-title-clash`.
    pub const SETTINGS_TITLE_CLASH: &str = "manifest.settings-title-clash";
    /// `manifest.settings-key-clash`.
    pub const SETTINGS_KEY_CLASH: &str = "manifest.settings-key-clash";
    /// `manifest.settings-target-unknown`.
    pub const SETTINGS_TARGET_UNKNOWN: &str = "manifest.settings-target-unknown";
    /// `manifest.settings-target-closed`.
    pub const SETTINGS_TARGET_CLOSED: &str = "manifest.settings-target-closed";
    /// `manifest.settings-target-grant`.
    pub const SETTINGS_TARGET_GRANT: &str = "manifest.settings-target-grant";
    /// `manifest.settings-anchor`.
    pub const SETTINGS_ANCHOR: &str = "manifest.settings-anchor";
    /// `manifest.settings-tab-anchor`.
    pub const SETTINGS_TAB_ANCHOR: &str = "manifest.settings-tab-anchor";
    /// `manifest.settings-depends`.
    pub const SETTINGS_DEPENDS: &str = "manifest.settings-depends";
    /// `manifest.settings-owner`.
    pub const SETTINGS_OWNER: &str = "manifest.settings-owner";
    /// `manifest.cli-name-reserved`.
    pub const CLI_NAME_RESERVED: &str = "manifest.cli-name-reserved";
    /// `manifest.cli-name-taken`.
    pub const CLI_NAME_TAKEN: &str = "manifest.cli-name-taken";
    /// `manifest.contribution-target`.
    pub const CONTRIBUTION_TARGET: &str = "manifest.contribution-target";
    /// `manifest.catalog-unknown`.
    pub const CATALOG_UNKNOWN: &str = "manifest.catalog-unknown";
    /// `manifest.client-request-unknown`.
    pub const CLIENT_REQUEST_UNKNOWN: &str = "manifest.client-request-unknown";
    /// `manifest.extension-point-duplicate`.
    pub const EXTENSION_POINT_DUPLICATE: &str = "manifest.extension-point-duplicate";
    /// `manifest.extension-point-invalid`.
    pub const EXTENSION_POINT_INVALID: &str = "manifest.extension-point-invalid";
    /// `manifest.extension-entry-invalid`.
    pub const EXTENSION_ENTRY_INVALID: &str = "manifest.extension-entry-invalid";
    /// `manifest.extension-side`.
    pub const EXTENSION_SIDE: &str = "manifest.extension-side";
    /// `manifest.extension-limit`.
    pub const EXTENSION_LIMIT: &str = "manifest.extension-limit";
    /// `manifest.unstable-unknown`.
    pub const UNSTABLE_UNKNOWN: &str = "manifest.unstable-unknown";
    /// `manifest.files-order`.
    pub const FILES_ORDER: &str = "manifest.files-order";
    /// `manifest.file-self`.
    pub const FILE_SELF: &str = "manifest.file-self";
    /// `manifest.file-unlisted`.
    pub const FILE_UNLISTED: &str = "manifest.file-unlisted";
    /// `manifest.file-missing`.
    pub const FILE_MISSING: &str = "manifest.file-missing";
    /// `manifest.file-length`.
    pub const FILE_LENGTH: &str = "manifest.file-length";
    /// `manifest.file-hash`.
    pub const FILE_HASH: &str = "manifest.file-hash";
    /// `manifest.asset-size`.
    pub const ASSET_SIZE: &str = "manifest.asset-size";
    /// `manifest.api-version`.
    pub const API_VERSION: &str = "manifest.api-version";
    /// `manifest.exports-mismatch`.
    pub const EXPORTS_MISMATCH: &str = "manifest.exports-mismatch";
    /// `manifest.export-grant`.
    pub const EXPORT_GRANT: &str = "manifest.export-grant";
    /// `manifest.import-not-requested`.
    pub const IMPORT_NOT_REQUESTED: &str = "manifest.import-not-requested";
    /// `manifest.unstable-missing`.
    pub const UNSTABLE_MISSING: &str = "manifest.unstable-missing";
    /// `manifest.signature-invalid`.
    pub const SIGNATURE_INVALID: &str = "manifest.signature-invalid";
    /// `manifest.id-publisher`.
    pub const ID_PUBLISHER: &str = "manifest.id-publisher";
    /// `manifest.publisher-mismatch`.
    pub const PUBLISHER_MISMATCH: &str = "manifest.publisher-mismatch";
    /// `manifest.grant-tier`.
    pub const GRANT_TIER: &str = "manifest.grant-tier";
    /// `manifest.limits-tier`.
    pub const LIMITS_TIER: &str = "manifest.limits-tier";
    /// `manifest.unstable-tier`.
    pub const UNSTABLE_TIER: &str = "manifest.unstable-tier";
    /// `manifest.replaces-tier`.
    pub const REPLACES_TIER: &str = "manifest.replaces-tier";
    /// `manifest.secret-scope`.
    pub const SECRET_SCOPE: &str = "manifest.secret-scope";
    /// `manifest.requires-cycle`.
    pub const REQUIRES_CYCLE: &str = "manifest.requires-cycle";
    /// `manifest.service-name`.
    pub const SERVICE_NAME: &str = "manifest.service-name";
    /// `manifest.service-version`.
    pub const SERVICE_VERSION: &str = "manifest.service-version";
    /// `manifest.service-cycle`.
    pub const SERVICE_CYCLE: &str = "manifest.service-cycle";
    /// `manifest.replaces-unknown`.
    pub const REPLACES_UNKNOWN: &str = "manifest.replaces-unknown";
    /// `manifest.replaces-not-replaceable`.
    pub const REPLACES_NOT_REPLACEABLE: &str = "manifest.replaces-not-replaceable";
    /// `manifest.point-risk`.
    pub const POINT_RISK: &str = "manifest.point-risk";
    /// `manifest.replaces-surface-incomplete`.
    pub const REPLACES_SURFACE_INCOMPLETE: &str = "manifest.replaces-surface-incomplete";
    /// `manifest.replaces-cycle`.
    pub const REPLACES_CYCLE: &str = "manifest.replaces-cycle";
}

/// `trust.*` codes (reference-manifest-schema.md).
pub mod trust {
    /// `trust.declined`.
    pub const DECLINED: &str = "trust.declined";
    /// `trust.key-unknown`.
    pub const KEY_UNKNOWN: &str = "trust.key-unknown";
    /// `trust.key-revoked`.
    pub const KEY_REVOKED: &str = "trust.key-revoked";
    /// `trust.bundled-tampered`.
    pub const BUNDLED_TAMPERED: &str = "trust.bundled-tampered";
    /// `trust.key-malformed`.
    pub const KEY_MALFORMED: &str = "trust.key-malformed";
    /// `trust.key-exists`.
    pub const KEY_EXISTS: &str = "trust.key-exists";
    /// `trust.already-trusted`.
    pub const ALREADY_TRUSTED: &str = "trust.already-trusted";
    /// `trust.not-found`.
    pub const NOT_FOUND: &str = "trust.not-found";
    /// `trust.in-use`.
    pub const IN_USE: &str = "trust.in-use";
    /// `trust.system-entry`.
    pub const SYSTEM_ENTRY: &str = "trust.system-entry";
    /// `trust.store-invalid`.
    pub const STORE_INVALID: &str = "trust.store-invalid";
    /// `trust.publisher-conflict`.
    pub const PUBLISHER_CONFLICT: &str = "trust.publisher-conflict";
    /// `trust.publisher-reserved`.
    pub const PUBLISHER_RESERVED: &str = "trust.publisher-reserved";
    /// `trust.rotation-invalid`.
    pub const ROTATION_INVALID: &str = "trust.rotation-invalid";
    /// `trust.collision`.
    pub const COLLISION: &str = "trust.collision";
    /// `trust.unreadable`.
    pub const UNREADABLE: &str = "trust.unreadable";
    /// `trust.removed`.
    pub const REMOVED: &str = "trust.removed";
}

/// `sign.*` codes (reference-manifest-schema.md).
pub mod sign {
    /// `sign.key-permissions`.
    pub const KEY_PERMISSIONS: &str = "sign.key-permissions";
    /// `sign.key-unreadable`.
    pub const KEY_UNREADABLE: &str = "sign.key-unreadable";
    /// `sign.publisher-mismatch`.
    pub const PUBLISHER_MISMATCH: &str = "sign.publisher-mismatch";
    /// `sign.already-signed`.
    pub const ALREADY_SIGNED: &str = "sign.already-signed";
}

/// `policy.*` codes (reference-manifest-schema.md).
pub mod policy {
    /// `policy.blocked`.
    pub const BLOCKED: &str = "policy.blocked";
    /// `policy.third-party-denied`.
    pub const THIRD_PARTY_DENIED: &str = "policy.third-party-denied";
    /// `policy.tier-denied`.
    pub const TIER_DENIED: &str = "policy.tier-denied";
    /// `policy.publisher-denied`.
    pub const PUBLISHER_DENIED: &str = "policy.publisher-denied";
    /// `policy.id-denied`.
    pub const ID_DENIED: &str = "policy.id-denied";
    /// `policy.grant-forbidden`.
    pub const GRANT_FORBIDDEN: &str = "policy.grant-forbidden";
    /// `policy.source-denied`.
    pub const SOURCE_DENIED: &str = "policy.source-denied";
    /// `policy.replacement`.
    pub const REPLACEMENT: &str = "policy.replacement";
    /// `policy.invalid`.
    pub const INVALID: &str = "policy.invalid";
    /// `policy.managed-plugin`.
    pub const MANAGED_PLUGIN: &str = "policy.managed-plugin";
}

/// `dependency.*` codes (reference-manifest-schema.md).
pub mod dependency {
    /// `dependency.unresolved`.
    pub const UNRESOLVED: &str = "dependency.unresolved";
    /// `dependency.conflict`.
    pub const CONFLICT: &str = "dependency.conflict";
    /// `dependency.cycle`.
    pub const CYCLE: &str = "dependency.cycle";
    /// `dependency.host-version`.
    pub const HOST_VERSION: &str = "dependency.host-version";
    /// `dependency.in-use`.
    pub const IN_USE: &str = "dependency.in-use";
    /// `dependency.lock-mismatch`.
    pub const LOCK_MISMATCH: &str = "dependency.lock-mismatch";
    /// `dependency.unsatisfied`.
    pub const UNSATISFIED: &str = "dependency.unsatisfied";
}

/// `install.*` codes (reference-manifest-schema.md).
pub mod install {
    /// `install.locked`.
    pub const LOCKED: &str = "install.locked";
    /// `install.state-corrupt`.
    pub const STATE_CORRUPT: &str = "install.state-corrupt";
    /// `install.registry-format`.
    pub const REGISTRY_FORMAT: &str = "install.registry-format";
    /// `install.io`.
    pub const IO: &str = "install.io";
    /// `install.no-space`.
    pub const NO_SPACE: &str = "install.no-space";
    /// `install.write-failed`.
    pub const WRITE_FAILED: &str = "install.write-failed";
    /// `install.notify-failed`.
    pub const NOTIFY_FAILED: &str = "install.notify-failed";
    /// `install.not-installed`.
    pub const NOT_INSTALLED: &str = "install.not-installed";
    /// `install.no-previous`.
    pub const NO_PREVIOUS: &str = "install.no-previous";
    /// `install.bundled`.
    pub const BUNDLED: &str = "install.bundled";
    /// `install.scope-unsupported`.
    pub const SCOPE_UNSUPPORTED: &str = "install.scope-unsupported";
    /// `install.unsigned`.
    pub const UNSIGNED: &str = "install.unsigned";
    /// `install.review-required`.
    pub const REVIEW_REQUIRED: &str = "install.review-required";
    /// `install.review-declined`.
    pub const REVIEW_DECLINED: &str = "install.review-declined";
    /// `install.needs-input`.
    pub const NEEDS_INPUT: &str = "install.needs-input";
    /// `install.cancelled`.
    pub const CANCELLED: &str = "install.cancelled";
    /// `install.compile-failed`.
    pub const COMPILE_FAILED: &str = "install.compile-failed";
    /// `install.source-scheme`.
    pub const SOURCE_SCHEME: &str = "install.source-scheme";
    /// `install.insecure-url`.
    pub const INSECURE_URL: &str = "install.insecure-url";
    /// `install.download-failed`.
    pub const DOWNLOAD_FAILED: &str = "install.download-failed";
    /// `install.size`.
    pub const SIZE: &str = "install.size";
    /// `install.already-installed`.
    pub const ALREADY_INSTALLED: &str = "install.already-installed";
    /// `install.ok`.
    pub const OK: &str = "install.ok";
}

/// `rollback.*` codes (reference-manifest-schema.md).
pub mod rollback {
    /// `rollback.no-backup`.
    pub const NO_BACKUP: &str = "rollback.no-backup";
}

/// `migration.*` codes (reference-manifest-schema.md).
pub mod migration {
    /// `migration.data-newer`.
    pub const DATA_NEWER: &str = "migration.data-newer";
    /// `migration.interrupted`.
    pub const INTERRUPTED: &str = "migration.interrupted";
    /// `migration.failed`.
    pub const FAILED: &str = "migration.failed";
    /// `migration.backup-too-large`.
    pub const BACKUP_TOO_LARGE: &str = "migration.backup-too-large";
    /// `migration.io`.
    pub const IO: &str = "migration.io";
    /// `migration.skipped`.
    pub const SKIPPED: &str = "migration.skipped";
    /// `migration.no-backup-failed`.
    pub const NO_BACKUP_FAILED: &str = "migration.no-backup-failed";
}

/// `plugin.*` codes (reference-manifest-schema.md).
pub mod plugin {
    /// `plugin.not-found`.
    pub const NOT_FOUND: &str = "plugin.not-found";
    /// `plugin.not-removable`.
    pub const NOT_REMOVABLE: &str = "plugin.not-removable";
    /// `plugin.already-installed`.
    pub const ALREADY_INSTALLED: &str = "plugin.already-installed";
    /// `plugin.server-missing`.
    pub const SERVER_MISSING: &str = "plugin.server-missing";
    /// `plugin.version-skew`.
    pub const VERSION_SKEW: &str = "plugin.version-skew";
    /// `plugin.blocked`.
    pub const BLOCKED: &str = "plugin.blocked";
    /// `plugin.not-running`.
    pub const NOT_RUNNING: &str = "plugin.not-running";
}

/// `grants.*` codes (reference-manifest-schema.md).
pub mod grants {
    /// `grants.not-requested`.
    pub const NOT_REQUESTED: &str = "grants.not-requested";
    /// `grants.unreadable`.
    pub const UNREADABLE: &str = "grants.unreadable";
    /// `grants.unsafe-permissions`.
    pub const UNSAFE_PERMISSIONS: &str = "grants.unsafe-permissions";
}

/// `update.*` codes (reference-manifest-schema.md).
pub mod update {
    /// `update.ok`.
    pub const OK: &str = "update.ok";
    /// `update.rolled-back`.
    pub const ROLLED_BACK: &str = "update.rolled-back";
}

/// `uninstall.*` codes (reference-manifest-schema.md).
pub mod uninstall {
    /// `uninstall.ok`.
    pub const OK: &str = "uninstall.ok";
    /// `uninstall.purged`.
    pub const PURGED: &str = "uninstall.purged";
    /// `uninstall.recovered`.
    pub const RECOVERED: &str = "uninstall.recovered";
}

/// `registry.*` codes (reference-manifest-schema.md).
pub mod registry {
    /// `registry.none-configured`.
    pub const NONE_CONFIGURED: &str = "registry.none-configured";
    /// `registry.root-key-missing`.
    pub const ROOT_KEY_MISSING: &str = "registry.root-key-missing";
    /// `registry.insecure-url`.
    pub const INSECURE_URL: &str = "registry.insecure-url";
    /// `registry.unreachable`.
    pub const UNREACHABLE: &str = "registry.unreachable";
    /// `registry.stale`.
    pub const STALE: &str = "registry.stale";
    /// `registry.metadata-expired`.
    pub const METADATA_EXPIRED: &str = "registry.metadata-expired";
    /// `registry.signature-invalid`.
    pub const SIGNATURE_INVALID: &str = "registry.signature-invalid";
    /// `registry.rollback`.
    pub const ROLLBACK: &str = "registry.rollback";
    /// `registry.hash-mismatch`.
    pub const HASH_MISMATCH: &str = "registry.hash-mismatch";
    /// `registry.not-found`.
    pub const NOT_FOUND: &str = "registry.not-found";
    /// `registry.yanked`.
    pub const YANKED: &str = "registry.yanked";
    /// `registry.size`.
    pub const SIZE: &str = "registry.size";
    /// `registry.changed`.
    pub const CHANGED: &str = "registry.changed";
    /// `registry.auth`.
    pub const AUTH: &str = "registry.auth";
    /// `registry.policy`.
    pub const POLICY: &str = "registry.policy";
    /// `registry.takedown`.
    pub const TAKEDOWN: &str = "registry.takedown";
    /// `registry.version-exists`.
    pub const VERSION_EXISTS: &str = "registry.version-exists";
    /// `registry.version-invalid`.
    pub const VERSION_INVALID: &str = "registry.version-invalid";
    /// `registry.id-reserved`.
    pub const ID_RESERVED: &str = "registry.id-reserved";
    /// `registry.namespace-unowned`.
    pub const NAMESPACE_UNOWNED: &str = "registry.namespace-unowned";
    /// `registry.confusable`.
    pub const CONFUSABLE: &str = "registry.confusable";
    /// `registry.category-unknown`.
    pub const CATEGORY_UNKNOWN: &str = "registry.category-unknown";
    /// `registry.namespace-not-dormant`.
    pub const NAMESPACE_NOT_DORMANT: &str = "registry.namespace-not-dormant";
}

/// `publish.*` codes (reference-manifest-schema.md).
pub mod publish {
    /// `publish.unsigned`.
    pub const UNSIGNED: &str = "publish.unsigned";
}

/// `usage.*` codes (reference-manifest-schema.md).
pub mod usage {
    /// `usage.missing-argument`.
    pub const MISSING_ARGUMENT: &str = "usage.missing-argument";
    /// `usage.unknown-flag`.
    pub const UNKNOWN_FLAG: &str = "usage.unknown-flag";
    /// `usage.invalid-value`.
    pub const INVALID_VALUE: &str = "usage.invalid-value";
    /// `usage.conflict`.
    pub const CONFLICT: &str = "usage.conflict";
    /// `usage.file-not-found`.
    pub const FILE_NOT_FOUND: &str = "usage.file-not-found";
    /// `usage.subject-unavailable`.
    pub const SUBJECT_UNAVAILABLE: &str = "usage.subject-unavailable";
    /// `usage.release-build-required`.
    pub const RELEASE_BUILD_REQUIRED: &str = "usage.release-build-required";
}

/// `session.*` codes (reference-manifest-schema.md).
pub mod session {
    /// `session.unreachable`.
    pub const UNREACHABLE: &str = "session.unreachable";
    /// `session.not-found`.
    pub const NOT_FOUND: &str = "session.not-found";
    /// `session.version-mismatch`.
    pub const VERSION_MISMATCH: &str = "session.version-mismatch";
    /// `session.memory-exceeded`.
    pub const MEMORY_EXCEEDED: &str = "session.memory-exceeded";
}

/// `toolchain.*` codes (reference-manifest-schema.md).
pub mod toolchain {
    /// `toolchain.bun-missing`.
    pub const BUN_MISSING: &str = "toolchain.bun-missing";
    /// `toolchain.bun-version`.
    pub const BUN_VERSION: &str = "toolchain.bun-version";
    /// `toolchain.rustc-missing`.
    pub const RUSTC_MISSING: &str = "toolchain.rustc-missing";
    /// `toolchain.rust-version`.
    pub const RUST_VERSION: &str = "toolchain.rust-version";
    /// `toolchain.rust-target`.
    pub const RUST_TARGET: &str = "toolchain.rust-target";
    /// `toolchain.sdk-missing`.
    pub const SDK_MISSING: &str = "toolchain.sdk-missing";
    /// `toolchain.sdk-mismatch`.
    pub const SDK_MISMATCH: &str = "toolchain.sdk-mismatch";
    /// `toolchain.language-ambiguous`.
    pub const LANGUAGE_AMBIGUOUS: &str = "toolchain.language-ambiguous";
    /// `toolchain.language-unknown`.
    pub const LANGUAGE_UNKNOWN: &str = "toolchain.language-unknown";
    /// `toolchain.lockfile-missing`.
    pub const LOCKFILE_MISSING: &str = "toolchain.lockfile-missing";
    /// `toolchain.network-needed`.
    pub const NETWORK_NEEDED: &str = "toolchain.network-needed";
    /// `toolchain.coverage-tool-missing`.
    pub const COVERAGE_TOOL_MISSING: &str = "toolchain.coverage-tool-missing";
}

/// `trap.*` codes (reference-errors-and-limits.md).
pub mod trap {
    /// `trap.deadline`.
    pub const DEADLINE: &str = "trap.deadline";
    /// `trap.memory`.
    pub const MEMORY: &str = "trap.memory";
    /// `trap.table`.
    pub const TABLE: &str = "trap.table";
    /// `trap.submission-batch`.
    pub const SUBMISSION_BATCH: &str = "trap.submission-batch";
    /// `trap.invalid-ui-tree`.
    pub const INVALID_UI_TREE: &str = "trap.invalid-ui-tree";
    /// `trap.abi`.
    pub const ABI: &str = "trap.abi";
    /// `trap.panic`.
    pub const PANIC: &str = "trap.panic";
}

/// `settings.*` codes (reference-errors-and-limits.md).
pub mod settings {
    /// `settings.section-dropped`.
    pub const SECTION_DROPPED: &str = "settings.section-dropped";
    /// `settings.tab-dropped`.
    pub const TAB_DROPPED: &str = "settings.tab-dropped";
    /// `settings.anchor-missing`.
    pub const ANCHOR_MISSING: &str = "settings.anchor-missing";
    /// `settings.value-ignored`.
    pub const VALUE_IGNORED: &str = "settings.value-ignored";
    /// `settings.invalid-value`.
    pub const INVALID_VALUE: &str = "settings.invalid-value";
    /// `settings.key-not-owned`.
    pub const KEY_NOT_OWNED: &str = "settings.key-not-owned";
    /// `settings.locked`.
    pub const LOCKED: &str = "settings.locked";
    /// `settings.unknown-key`.
    pub const UNKNOWN_KEY: &str = "settings.unknown-key";
}

/// `extension.*` codes (reference-errors-and-limits.md).
pub mod extension {
    /// `extension.unknown-point`.
    pub const UNKNOWN_POINT: &str = "extension.unknown-point";
    /// `extension.not-granted`.
    pub const NOT_GRANTED: &str = "extension.not-granted";
    /// `extension.schema-mismatch`.
    pub const SCHEMA_MISMATCH: &str = "extension.schema-mismatch";
    /// `extension.cap-exceeded`.
    pub const CAP_EXCEEDED: &str = "extension.cap-exceeded";
    /// `extension.reentrant`.
    pub const REENTRANT: &str = "extension.reentrant";
    /// `extension.not-dynamic`.
    pub const NOT_DYNAMIC: &str = "extension.not-dynamic";
    /// `extension.handler-timeout`.
    pub const HANDLER_TIMEOUT: &str = "extension.handler-timeout";
    /// `extension.reply-not-allowed`.
    pub const REPLY_NOT_ALLOWED: &str = "extension.reply-not-allowed";
    /// `extension.vetoed`.
    pub const VETOED: &str = "extension.vetoed";
    /// `extension.disabled`.
    pub const DISABLED: &str = "extension.disabled";
    /// `extension.chain-cap`.
    pub const CHAIN_CAP: &str = "extension.chain-cap";
    /// `extension.contributor-not-ready`.
    pub const CONTRIBUTOR_NOT_READY: &str = "extension.contributor-not-ready";
    /// `extension.depth-exceeded`.
    pub const DEPTH_EXCEEDED: &str = "extension.depth-exceeded";
    /// `extension.entry-rejected`.
    pub const ENTRY_REJECTED: &str = "extension.entry-rejected";
    /// `extension.veto-overridden`.
    pub const VETO_OVERRIDDEN: &str = "extension.veto-overridden";
}

/// `host.*` codes (reference-errors-and-limits.md).
pub mod host {
    /// `host.capability-not-requested`.
    pub const CAPABILITY_NOT_REQUESTED: &str = "host.capability-not-requested";
    /// `host.cache-unavailable`.
    pub const CACHE_UNAVAILABLE: &str = "host.cache-unavailable";
    /// `host.cwasm-incompatible`.
    pub const CWASM_INCOMPATIBLE: &str = "host.cwasm-incompatible";
    /// `host.engine-error`.
    pub const ENGINE_ERROR: &str = "host.engine-error";
    /// `host.engine-failed`.
    pub const ENGINE_FAILED: &str = "host.engine-failed";
    /// `host.grants-unreadable`.
    pub const GRANTS_UNREADABLE: &str = "host.grants-unreadable";
    /// `host.helper-missing`.
    pub const HELPER_MISSING: &str = "host.helper-missing";
    /// `host.import-panic`.
    pub const IMPORT_PANIC: &str = "host.import-panic";
    /// `host.late`.
    pub const LATE: &str = "host.late";
    /// `host.queue-timeout`.
    pub const QUEUE_TIMEOUT: &str = "host.queue-timeout";
    /// `host.shutdown-timeout`.
    pub const SHUTDOWN_TIMEOUT: &str = "host.shutdown-timeout";
    /// `host.slow-calls`.
    pub const SLOW_CALLS: &str = "host.slow-calls";
    /// `host.stack-overflow`.
    pub const STACK_OVERFLOW: &str = "host.stack-overflow";
    /// `host.start-slow`.
    pub const START_SLOW: &str = "host.start-slow";
    /// `host.trap`.
    pub const TRAP: &str = "host.trap";
}

/// `component.*` codes (reference-host-runtime.md).
pub mod component {
    /// `component.wasi-forbidden`.
    pub const WASI_FORBIDDEN: &str = "component.wasi-forbidden";
    /// `component.custom-section-too-large`.
    pub const CUSTOM_SECTION_TOO_LARGE: &str = "component.custom-section-too-large";
    /// `component.import-ungranted`.
    pub const IMPORT_UNGRANTED: &str = "component.import-ungranted";
    /// `component.import-unknown`.
    pub const IMPORT_UNKNOWN: &str = "component.import-unknown";
    /// `component.invalid`.
    pub const INVALID: &str = "component.invalid";
    /// `component.missing`.
    pub const MISSING: &str = "component.missing";
    /// `component.size-warning`.
    pub const SIZE_WARNING: &str = "component.size-warning";
    /// `component.too-large`.
    pub const TOO_LARGE: &str = "component.too-large";
    /// `component.world-mismatch`.
    pub const WORLD_MISMATCH: &str = "component.world-mismatch";
}

/// `config.*` codes (reference-errors-and-limits.md).
pub mod config {
    /// `config.parse-error`.
    pub const PARSE_ERROR: &str = "config.parse-error";
    /// `config.unknown-key`.
    pub const UNKNOWN_KEY: &str = "config.unknown-key";
}

/// `snapshot.*` codes (reference-errors-and-limits.md).
pub mod snapshot {
    /// `snapshot.section-too-large`.
    pub const SECTION_TOO_LARGE: &str = "snapshot.section-too-large";
    /// `snapshot.write-failed`.
    pub const WRITE_FAILED: &str = "snapshot.write-failed";
}

/// Every code of this module, in manual order.
pub const ALL: &[&str] = &[
    package::ZIP_INVALID,
    package::ZIP64,
    package::ZIP_COMPRESSED,
    package::ZIP_ENCRYPTED,
    package::ZIP_DESCRIPTOR,
    package::ZIP_EXTRA,
    package::ZIP_COMMENT,
    package::ZIP_TRAILING,
    package::HEADER_MISMATCH,
    package::CRC_MISMATCH,
    package::PATH_INVALID,
    package::PATH_DUPLICATE,
    package::PATH_CASE_COLLISION,
    package::ENTRY_DIRECTORY,
    package::ENTRY_SYMLINK,
    package::ENTRY_ORDER,
    package::ENTRY_UNEXPECTED,
    package::ENTRY_LIMIT,
    package::SIZE_LIMIT,
    package::MANIFEST_SIZE,
    package::MANIFEST_MISSING,
    package::OLD_FORMAT,
    package::PRECOMPILED_FORBIDDEN,
    manifest::JSON_INVALID,
    manifest::DUPLICATE_KEY,
    manifest::SCHEMA,
    manifest::UNKNOWN_KEY,
    manifest::KEY_MISSING,
    manifest::ID_INVALID,
    manifest::ID_RESERVED,
    manifest::VERSION_INVALID,
    manifest::DISPLAY_NAME,
    manifest::DATA_VERSION,
    manifest::SCHEMA_KEYWORD,
    manifest::REQUIRES_INVALID,
    manifest::REQUIRES_SELF,
    manifest::WHEN_SYNTAX,
    manifest::WHEN_KEY,
    manifest::VISIBLE_WHEN,
    manifest::CONTRIBUTION_LIMIT,
    manifest::ROW_FORMAT,
    manifest::CLI_FIELD,
    manifest::MENU_DEPTH,
    manifest::SCENE_PRERENDER,
    manifest::SETTINGS_KIND,
    manifest::SETTINGS_ID,
    manifest::SETTINGS_ROW_TYPE,
    manifest::SETTINGS_ROW_FIELD,
    manifest::SETTINGS_DEFAULT,
    manifest::SETTINGS_LIMIT,
    manifest::SETTINGS_KEY,
    manifest::SETTINGS_HELP,
    manifest::SETTINGS_COMMAND,
    manifest::SETTINGS_GLYPH,
    manifest::SETTINGS_SOURCE,
    manifest::SOUND_FORMAT,
    manifest::SOUND_ID_TAKEN,
    manifest::ENTRY_INVALID,
    manifest::GRANT_UNKNOWN,
    manifest::GRANT_ARGUMENT,
    manifest::GRANT_ARGUMENT_MISSING,
    manifest::GRANT_WILDCARD,
    manifest::GRANT_SIDE,
    manifest::GRANT_DEPENDENCY,
    manifest::GRANT_SCOPE,
    manifest::GRANT_PROGRAM,
    manifest::GRANT_MISSING_EXTENSION,
    manifest::SETTINGS_ID_CLASH,
    manifest::SETTINGS_TITLE_CLASH,
    manifest::SETTINGS_KEY_CLASH,
    manifest::SETTINGS_TARGET_UNKNOWN,
    manifest::SETTINGS_TARGET_CLOSED,
    manifest::SETTINGS_TARGET_GRANT,
    manifest::SETTINGS_ANCHOR,
    manifest::SETTINGS_TAB_ANCHOR,
    manifest::SETTINGS_DEPENDS,
    manifest::SETTINGS_OWNER,
    manifest::CLI_NAME_RESERVED,
    manifest::CLI_NAME_TAKEN,
    manifest::CONTRIBUTION_TARGET,
    manifest::CATALOG_UNKNOWN,
    manifest::CLIENT_REQUEST_UNKNOWN,
    manifest::EXTENSION_POINT_DUPLICATE,
    manifest::EXTENSION_POINT_INVALID,
    manifest::EXTENSION_ENTRY_INVALID,
    manifest::EXTENSION_SIDE,
    manifest::EXTENSION_LIMIT,
    manifest::UNSTABLE_UNKNOWN,
    manifest::FILES_ORDER,
    manifest::FILE_SELF,
    manifest::FILE_UNLISTED,
    manifest::FILE_MISSING,
    manifest::FILE_LENGTH,
    manifest::FILE_HASH,
    manifest::ASSET_SIZE,
    package::COMPONENT_INVALID,
    package::API_SECTION_MISSING,
    package::API_SECTION_REPEATED,
    package::API_SECTION_INVALID,
    manifest::API_VERSION,
    manifest::EXPORTS_MISMATCH,
    manifest::EXPORT_GRANT,
    manifest::IMPORT_NOT_REQUESTED,
    manifest::UNSTABLE_MISSING,
    package::API_UNSUPPORTED,
    package::SIGNATURE_FORMAT,
    manifest::SIGNATURE_INVALID,
    trust::DECLINED,
    trust::KEY_UNKNOWN,
    trust::KEY_REVOKED,
    trust::BUNDLED_TAMPERED,
    manifest::ID_PUBLISHER,
    manifest::PUBLISHER_MISMATCH,
    package::PUBLISHER_KEY_CHANGED,
    trust::KEY_MALFORMED,
    trust::KEY_EXISTS,
    trust::ALREADY_TRUSTED,
    trust::NOT_FOUND,
    trust::IN_USE,
    trust::SYSTEM_ENTRY,
    trust::STORE_INVALID,
    trust::PUBLISHER_CONFLICT,
    trust::PUBLISHER_RESERVED,
    trust::ROTATION_INVALID,
    trust::COLLISION,
    sign::KEY_PERMISSIONS,
    sign::KEY_UNREADABLE,
    sign::PUBLISHER_MISMATCH,
    sign::ALREADY_SIGNED,
    manifest::GRANT_TIER,
    manifest::LIMITS_TIER,
    manifest::UNSTABLE_TIER,
    manifest::REPLACES_TIER,
    manifest::SECRET_SCOPE,
    package::PLATFORM_UNSUPPORTED,
    package::DOWNGRADE,
    package::VERSION_REUSED,
    policy::BLOCKED,
    policy::THIRD_PARTY_DENIED,
    policy::TIER_DENIED,
    policy::PUBLISHER_DENIED,
    policy::ID_DENIED,
    policy::GRANT_FORBIDDEN,
    policy::SOURCE_DENIED,
    policy::REPLACEMENT,
    policy::INVALID,
    policy::MANAGED_PLUGIN,
    package::BLOCKED,
    dependency::UNRESOLVED,
    dependency::CONFLICT,
    dependency::CYCLE,
    dependency::HOST_VERSION,
    dependency::IN_USE,
    dependency::LOCK_MISMATCH,
    dependency::UNSATISFIED,
    manifest::REQUIRES_CYCLE,
    manifest::SERVICE_NAME,
    manifest::SERVICE_VERSION,
    manifest::SERVICE_CYCLE,
    manifest::REPLACES_UNKNOWN,
    manifest::REPLACES_NOT_REPLACEABLE,
    manifest::POINT_RISK,
    manifest::REPLACES_SURFACE_INCOMPLETE,
    manifest::REPLACES_CYCLE,
    install::LOCKED,
    install::STATE_CORRUPT,
    install::REGISTRY_FORMAT,
    install::IO,
    install::NO_SPACE,
    install::WRITE_FAILED,
    install::NOTIFY_FAILED,
    install::NOT_INSTALLED,
    install::NO_PREVIOUS,
    rollback::NO_BACKUP,
    migration::DATA_NEWER,
    migration::INTERRUPTED,
    migration::FAILED,
    migration::BACKUP_TOO_LARGE,
    install::BUNDLED,
    install::SCOPE_UNSUPPORTED,
    install::UNSIGNED,
    install::REVIEW_REQUIRED,
    install::REVIEW_DECLINED,
    install::NEEDS_INPUT,
    install::CANCELLED,
    install::COMPILE_FAILED,
    install::SOURCE_SCHEME,
    install::INSECURE_URL,
    install::DOWNLOAD_FAILED,
    install::SIZE,
    package::CORRUPT,
    package::CACHE_MISSING,
    plugin::NOT_FOUND,
    plugin::NOT_REMOVABLE,
    plugin::ALREADY_INSTALLED,
    plugin::SERVER_MISSING,
    plugin::VERSION_SKEW,
    grants::NOT_REQUESTED,
    install::ALREADY_INSTALLED,
    update::OK,
    update::ROLLED_BACK,
    uninstall::OK,
    uninstall::PURGED,
    uninstall::RECOVERED,
    registry::NONE_CONFIGURED,
    registry::ROOT_KEY_MISSING,
    registry::INSECURE_URL,
    registry::UNREACHABLE,
    registry::STALE,
    registry::METADATA_EXPIRED,
    registry::SIGNATURE_INVALID,
    registry::ROLLBACK,
    registry::HASH_MISMATCH,
    registry::NOT_FOUND,
    registry::YANKED,
    registry::SIZE,
    registry::CHANGED,
    registry::AUTH,
    registry::POLICY,
    registry::TAKEDOWN,
    registry::VERSION_EXISTS,
    registry::VERSION_INVALID,
    registry::ID_RESERVED,
    registry::NAMESPACE_UNOWNED,
    registry::CONFUSABLE,
    registry::CATEGORY_UNKNOWN,
    publish::UNSIGNED,
    usage::MISSING_ARGUMENT,
    usage::UNKNOWN_FLAG,
    usage::INVALID_VALUE,
    usage::CONFLICT,
    usage::FILE_NOT_FOUND,
    usage::SUBJECT_UNAVAILABLE,
    usage::RELEASE_BUILD_REQUIRED,
    session::UNREACHABLE,
    session::NOT_FOUND,
    session::VERSION_MISMATCH,
    session::MEMORY_EXCEEDED,
    toolchain::BUN_MISSING,
    toolchain::BUN_VERSION,
    toolchain::RUSTC_MISSING,
    toolchain::RUST_VERSION,
    toolchain::RUST_TARGET,
    toolchain::SDK_MISSING,
    toolchain::SDK_MISMATCH,
    toolchain::LANGUAGE_AMBIGUOUS,
    toolchain::LANGUAGE_UNKNOWN,
    toolchain::LOCKFILE_MISSING,
    toolchain::NETWORK_NEEDED,
    toolchain::COVERAGE_TOOL_MISSING,
    trap::DEADLINE,
    trap::MEMORY,
    trap::TABLE,
    trap::SUBMISSION_BATCH,
    trap::INVALID_UI_TREE,
    trap::ABI,
    trap::PANIC,
    settings::SECTION_DROPPED,
    settings::TAB_DROPPED,
    settings::ANCHOR_MISSING,
    settings::VALUE_IGNORED,
    extension::UNKNOWN_POINT,
    extension::NOT_GRANTED,
    extension::SCHEMA_MISMATCH,
    extension::CAP_EXCEEDED,
    extension::REENTRANT,
    extension::NOT_DYNAMIC,
    extension::HANDLER_TIMEOUT,
    extension::REPLY_NOT_ALLOWED,
    extension::VETOED,
    extension::DISABLED,
    extension::CHAIN_CAP,
    host::CAPABILITY_NOT_REQUESTED,
    component::WASI_FORBIDDEN,
    trust::UNREADABLE,
    trust::REMOVED,
    install::OK,
    migration::IO,
    migration::SKIPPED,
    migration::NO_BACKUP_FAILED,
    plugin::BLOCKED,
    plugin::NOT_RUNNING,
    registry::NAMESPACE_NOT_DORMANT,
    grants::UNREADABLE,
    grants::UNSAFE_PERMISSIONS,
    settings::INVALID_VALUE,
    settings::KEY_NOT_OWNED,
    settings::LOCKED,
    settings::UNKNOWN_KEY,
    extension::CONTRIBUTOR_NOT_READY,
    extension::DEPTH_EXCEEDED,
    extension::ENTRY_REJECTED,
    extension::VETO_OVERRIDDEN,
    host::CACHE_UNAVAILABLE,
    host::CWASM_INCOMPATIBLE,
    host::ENGINE_ERROR,
    host::ENGINE_FAILED,
    host::GRANTS_UNREADABLE,
    host::HELPER_MISSING,
    host::IMPORT_PANIC,
    host::LATE,
    host::QUEUE_TIMEOUT,
    host::SHUTDOWN_TIMEOUT,
    host::SLOW_CALLS,
    host::STACK_OVERFLOW,
    host::START_SLOW,
    host::TRAP,
    component::CUSTOM_SECTION_TOO_LARGE,
    component::IMPORT_UNGRANTED,
    component::IMPORT_UNKNOWN,
    component::INVALID,
    component::MISSING,
    component::SIZE_WARNING,
    component::TOO_LARGE,
    component::WORLD_MISMATCH,
    config::PARSE_ERROR,
    config::UNKNOWN_KEY,
    snapshot::SECTION_TOO_LARGE,
    snapshot::WRITE_FAILED,
];
