//! File-install transport. Admission and durable effects remain installer-owned.
use std::{io::Read, path::Path, time::Instant};

use serde_json::json;

use super::{offline, CliOutput};
use crate::install::{Consent, Installer, TrustedInstallContext};

const MAX_PACKAGE_BYTES: u64 = 300 * 1024 * 1024;

/// Host-owned install service. Implementations supply actual trust, policy,
/// consent and durable effects to `install_file`; the CLI never supplies them.
/// Services shared through `Arc` must serialize their mutable grant-store
/// context and preserve the installer's lock ordering.
pub trait InstallCommandBackend: Send + Sync {
    fn install(&self, path: &Path) -> CliOutput;
}

/// The flags of `ilium plugin install` (reference-plugin-distribution-cli.md PKG-425).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallFlags {
    /// `--yes`: skip the review dialog.
    pub yes: bool,
    /// `--allow-unsigned`.
    pub allow_unsigned: bool,
    /// `--allow-downgrade`.
    pub allow_downgrade: bool,
    /// `--locked`.
    pub locked: bool,
    /// `--dry-run`.
    pub dry_run: bool,
    /// `--offline`.
    pub offline: bool,
    /// `--no-backup`.
    pub no_backup: bool,
    /// `--unskip`.
    pub unskip: bool,
    /// `--trust <key_id>`.
    pub trust: Option<String>,
    /// `--channel <stable|beta|dev>`.
    pub channel: Option<String>,
    /// `--registry <url>`.
    pub registry: Option<String>,
    /// `--from-bundle <dir>`.
    pub from_bundle: Option<String>,
    /// `--grant <grant>` (repeatable).
    pub grants: Vec<String>,
}

/// Splits install arguments into flags and positional operands.
pub fn parse_install_flags(args: &[String]) -> Result<(InstallFlags, Vec<String>), CliOutput> {
    let mut flags = InstallFlags::default();
    let mut positional = Vec::new();
    let mut iterator = args.iter();
    while let Some(arg) = iterator.next() {
        let mut value = |name: &str| {
            iterator.next().cloned().ok_or_else(|| {
                offline::failure("usage.missing-argument", format!("{name} needs a value"), 2)
            })
        };
        match arg.as_str() {
            "--json" | "--quiet" | "--no-color" => {}
            "--yes" => flags.yes = true,
            "--allow-unsigned" => flags.allow_unsigned = true,
            "--allow-downgrade" => flags.allow_downgrade = true,
            "--locked" => flags.locked = true,
            "--dry-run" => flags.dry_run = true,
            "--offline" => flags.offline = true,
            "--no-backup" => flags.no_backup = true,
            "--unskip" => flags.unskip = true,
            "--trust" => flags.trust = Some(value("--trust")?),
            "--registry" => flags.registry = Some(value("--registry")?),
            "--from-bundle" => flags.from_bundle = Some(value("--from-bundle")?),
            "--grant" => flags.grants.push(value("--grant")?),
            "--channel" => {
                let channel = value("--channel")?;
                if !matches!(channel.as_str(), "stable" | "beta" | "dev") {
                    return Err(offline::failure(
                        "usage.invalid-value",
                        format!("{channel} is not stable, beta or dev"),
                        2,
                    ));
                }
                flags.channel = Some(channel);
            }
            flag if flag.starts_with('-') => {
                return Err(offline::failure(
                    "usage.unknown-flag",
                    format!("Unknown install flag: {flag}"),
                    2,
                ))
            }
            _ => positional.push(arg.clone()),
        }
    }
    Ok((flags, positional))
}

/// Parse install arguments before invoking any service or reading a file. Registry sources and
/// updates require separate admission contexts. Consent and unsigned admission are decided by
/// the caller that built `backend` from the parsed flags.
pub fn run(args: &[String], cwd: &Path, backend: &dyn InstallCommandBackend) -> CliOutput {
    let (flags, positional) = match parse_install_flags(args) {
        Ok(parsed) => parsed,
        Err(output) => return output,
    };
    let mut source = None;
    for arg in &positional {
        if source.replace(arg.as_str()).is_some() {
            return offline::failure(
                "usage.argument-unexpected",
                "Only one package file is accepted",
                2,
            );
        }
    }
    let Some(source) = source else {
        return offline::failure("usage.argument-missing", "A package file is required", 2);
    };
    if source.is_empty() {
        return offline::failure("usage.argument-missing", "A package file is required", 2);
    }
    // PKG-426 classifies URLs before file paths and bare registry identifiers.
    if source.contains("://")
        || !(source.contains('/')
            || source.contains('\\')
            || source.starts_with('.')
            || source.starts_with('~')
            || source.ends_with(".iliumpkg"))
    {
        return offline::failure("install.context-required", "This adapter requires a local package file; registry and URL sources need their admission service", 1);
    }
    if !cwd.is_absolute() {
        return offline::failure(
            "install.context-required",
            "An absolute command working directory is required",
            1,
        );
    }
    if flags.dry_run {
        return dry_run(&cwd.join(source));
    }
    backend.install(&cwd.join(source))
}

/// `--dry-run`: reads and parses the package and reports it without any effect.
fn dry_run(path: &Path) -> CliOutput {
    let bytes = match read_install_file(path) {
        Ok(bytes) => bytes,
        Err(output) => return output,
    };
    match crate::package::read_package(&bytes) {
        Ok(archive) => offline::finish(
            vec![json!({"type":"plugin","id":archive.manifest.id,
                "version":archive.manifest.version.to_string(),"state":"dry-run","dry_run":true})],
            0,
        ),
        Err(diagnostics) => {
            let first = diagnostics.first();
            offline::failure(
                first.map_or("package.invalid", |d| d.code.as_str()),
                first.map_or("invalid package", |d| d.message.as_str()),
                1,
            )
        }
    }
}

/// Read an actual regular package file with the PKG-130 bound, including growth
/// after opening. This performs no writes, trust decisions or package admission.
pub fn read_install_file(path: &Path) -> Result<Vec<u8>, CliOutput> {
    let file = ilium_platform::secure_fs::open_regular_file(path).map_err(|error| {
        let usage = error.kind() == std::io::ErrorKind::NotFound
            || error.kind() == std::io::ErrorKind::InvalidInput;
        offline::failure(
            if usage {
                "usage.file-not-found"
            } else {
                "install.file-unreadable"
            },
            error.to_string(),
            if usage { 2 } else { 1 },
        )
    })?;
    if file
        .metadata()
        .map_err(|error| offline::failure("install.file-unreadable", error.to_string(), 1))?
        .len()
        > MAX_PACKAGE_BYTES
    {
        return Err(offline::failure(
            "package.size-limit",
            "Package exceeds 300 MiB",
            1,
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_PACKAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| offline::failure("install.file-unreadable", error.to_string(), 1))?;
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(offline::failure(
            "package.size-limit",
            "Package exceeds 300 MiB",
            1,
        ));
    }
    Ok(bytes)
}

/// Install a fresh local package using actual caller-supplied trust, policy,
/// grant-store, consent and durable effects. There is no implicit approval or
/// session requirement. The caller must construct effects that durably audit
/// approval before commit and notify actual sessions after commit.
///
/// File provenance is derived from the supplied path; unsigned permission is
/// configured on `Installer`, never inferred from consent or command flags.
/// Existing installations are refused by the installer until an update context
/// is provided. A notification warning after commit remains a successful install.
pub fn install_file(
    path: &Path,
    installer: &Installer,
    context: &mut TrustedInstallContext<'_>,
    consent: &dyn Consent,
) -> CliOutput {
    let started = Instant::now();
    let bytes = match read_install_file(path) {
        Ok(bytes) => bytes,
        Err(output) => return output,
    };
    let absolute = match std::fs::canonicalize(path) {
        Ok(path) => path,
        Err(error) => return offline::failure("install.file-unreadable", error.to_string(), 1),
    };
    let Some(source) = absolute.to_str() else {
        return offline::failure("install.source-invalid", "Package path is not UTF-8", 1);
    };
    context.provenance = crate::install::InstallProvenance {
        source: source.to_owned(),
        kind: crate::policy::PolicySourceKind::File,
        channel: "file".into(),
    };
    let output = match installer.install_with_context(&bytes, context, consent) {
        Ok(report) => {
            let tier = match report.tier {
                ilium_plugin_api::Tier::T1 => "T1",
                ilium_plugin_api::Tier::T2 => "T2",
                ilium_plugin_api::Tier::T3 => "T3",
            };
            let mut records = vec![json!({"type":"plugin","id":report.installed.plugin,
                "version":report.installed.version,"tier":tier,"state":"enabled","dev":false})];
            records.extend(report.warnings.into_iter().map(|warning|
                json!({"type":"warning","code":warning.code.as_str(),"message":warning.message})));
            offline::finish(records, 0)
        }
        Err(error) => {
            let code = if error.code.as_str() == "install.declined" {
                "install.review-declined"
            } else {
                error.code.as_str()
            };
            offline::failure(code, error.message, install_exit_code(code))
        }
    };
    let mut records = output.stdout_jsonl;
    if let Some(summary) = records
        .iter_mut()
        .find(|record| record["type"] == "summary")
    {
        summary["duration_ms"] =
            json!(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));
    }
    offline::render(records, output.exit_code)
}

/// Distribution-command exit mapping (PKG-423/424), independent of transport.
pub fn install_exit_code(code: &str) -> i32 {
    match code {
        "install.locked" => 4,
        "plugin.not-found" | "trust.not-found" | "registry.not-found" => 5,
        "package.blocked" => 7,
        "registry.unreachable"
        | "registry.stale"
        | "registry.metadata-expired"
        | "install.download-failed" => 8,
        code if code.starts_with("policy.") => 7,
        _ => 1,
    }
}
