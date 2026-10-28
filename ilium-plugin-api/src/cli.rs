//! The `ilium plugin` command line against a registry model (PKG-801, PKG-823, PKG-824).

use crate::error::{ErrorCode, PluginError};
use crate::registry::{Registry, YankRequest};

/// What a command prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOutput {
    /// The text on standard output.
    pub text: String,
}

fn usage(code: &str, message: impl std::fmt::Display) -> PluginError {
    PluginError::coded(code, ErrorCode::InvalidArgument, message)
}

/// The value after `flag`, `None` when the flag is absent.
fn flag_value<'a>(args: &[&'a str], flag: &str) -> Result<Option<&'a str>, PluginError> {
    match args.iter().position(|argument| *argument == flag) {
        None => Ok(None),
        Some(index) => args
            .get(index + 1)
            .filter(|value| !value.starts_with("--"))
            .map(|value| Some(*value))
            .ok_or_else(|| usage("usage.missing-argument", format!("{flag} needs a value"))),
    }
}

fn split_target(target: &str) -> Result<(&str, &str), PluginError> {
    target
        .split_once('@')
        .filter(|(id, version)| !id.is_empty() && !version.is_empty())
        .ok_or_else(|| usage("usage.invalid-value", format!("`{target}` is not <id>@<version>")))
}

fn ranged(
    args: &[&str],
    flag: &str,
    low: u64,
    high: u64,
    default: u64,
) -> Result<u64, PluginError> {
    let Some(text) = flag_value(args, flag)? else {
        return Ok(default);
    };
    text.parse::<u64>()
        .ok()
        .filter(|value| (low..=high).contains(value))
        .ok_or_else(|| {
            usage(
                "usage.invalid-value",
                format!("{flag} must be a number from {low} to {high}"),
            )
        })
}

/// Runs `args`, for example `["plugin", "yank", "demo@1.0.0", "--reason", "broken"]`, against
/// `registry` as the owner of `signer_key`.
pub fn run(
    registry: &mut Registry,
    signer_key: &str,
    args: &[&str],
) -> Result<CliOutput, PluginError> {
    let rest = match args {
        ["plugin", rest @ ..] => rest,
        _ => return Err(usage("usage.invalid-value", "expected `plugin <command>`")),
    };
    let Some((command, rest)) = rest.split_first() else {
        return Err(usage("usage.missing-argument", "a plugin command is required"));
    };
    match *command {
        "yank" => {
            let target = rest
                .first()
                .ok_or_else(|| usage("usage.missing-argument", "yank needs <id>@<version>"))?;
            let (id, version) = split_target(target)?;
            let reason = flag_value(rest, "--reason")?
                .ok_or_else(|| usage("usage.missing-argument", "yank needs --reason"))?;
            let dependents = registry.dependents_of(id, version);
            let confirmed = rest.contains(&"--confirm-dependents");
            if dependents > 0 && !confirmed {
                return Err(PluginError::coded(
                    "registry.dependents",
                    ErrorCode::Conflict,
                    format!(
                        "yanking {id}@{version} leaves {dependents} plugin(s) without a satisfying release; pass --confirm-dependents"
                    ),
                ));
            }
            registry.yank(YankRequest::new(id, version, signer_key).with_reason(reason))?;
            Ok(CliOutput {
                text: format!("yanked {id}@{version}\n{dependents} plugin(s) depend only on it\n"),
            })
        }
        "unyank" => {
            let target = rest
                .first()
                .ok_or_else(|| usage("usage.missing-argument", "unyank needs <id>@<version>"))?;
            let (id, version) = split_target(target)?;
            registry.unyank(YankRequest::new(id, version, signer_key))?;
            Ok(CliOutput {
                text: format!("unyanked {id}@{version}\n"),
            })
        }
        "info" => {
            let target = rest
                .first()
                .ok_or_else(|| usage("usage.missing-argument", "info needs <id>@<version>"))?;
            let (id, version) = split_target(target)?;
            registry.note_request();
            let info = registry
                .own_versions(id)
                .into_iter()
                .find(|info| info.version == version)
                .ok_or_else(|| {
                    PluginError::coded(
                        "plugin.not-found",
                        ErrorCode::NotFound,
                        format!("no `{id}@{version}`"),
                    )
                })?;
            let mut text = format!(
                "{id}@{version}\nsha256: {}\nlength: {}\nchannel: {}\n",
                info.sha256, info.length, info.channel
            );
            if let Some(reason) = &info.yank_reason {
                text.push_str(&format!("yanked: {reason}\n"));
            }
            Ok(CliOutput { text })
        }
        "search" => {
            let query = rest
                .first()
                .filter(|value| !value.starts_with("--"))
                .ok_or_else(|| usage("usage.missing-argument", "search needs a query"))?;
            // Validate before any request is made (PKG-824).
            let limit = ranged(rest, "--limit", 1, 100, 20)? as usize;
            let offset = ranged(rest, "--offset", 0, 10_000, 0)? as usize;
            registry.note_request();
            let mut rows = registry.search_ids(query);
            rows.sort();
            let text: String = rows
                .iter()
                .skip(offset)
                .take(limit)
                .map(|(id, version)| format!("{id} {version}\n"))
                .collect();
            Ok(CliOutput { text })
        }
        other => Err(usage(
            "usage.invalid-value",
            format!("unknown plugin command `{other}`"),
        )),
    }
}
