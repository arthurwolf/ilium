//! Explicit access selection for the local automation client.
use super::{args::Target, CtlFailure};
use ilium_control::access::{AccessLevel, Tokens};
use std::io::Read;
use std::path::Path;

fn requested(target: &Target) -> Result<AccessLevel, CtlFailure> {
    match target.access.as_deref() {
        Some(value) => AccessLevel::parse(value)
            .ok_or_else(|| CtlFailure::usage("access must be read, act or admin")),
        None if target.token.is_some() => Ok(AccessLevel::Admin),
        None if crate::pane_identity_from_env().is_ok() => Ok(AccessLevel::Act),
        None => Ok(AccessLevel::Read),
    }
}

pub(super) fn require_write_grant(target: &Target) -> Result<(), CtlFailure> {
    if requested(target)? == AccessLevel::Read {
        return Err(CtlFailure::rejected("outside callers default to read; explicitly grant --access act or --access admin for writes"));
    }
    Ok(())
}

pub(super) fn for_operation(
    target: &Target,
    socket: &Path,
    operation: &ilium_control::Operation,
) -> Result<String, CtlFailure> {
    let desired = requested(target)?;
    desired.authorize(operation).map_err(CtlFailure::rejected)?;
    // A supplied credential is authenticated by the server. Reading all roles
    // here would require authority the holder of one token need not possess.
    if let Some(token) = &target.token {
        return Ok(token.clone());
    }
    let file =
        ilium_platform::secure_fs::open_regular_file(&socket.with_extension("ctl-tokens.json"))
            .map_err(|error| {
                CtlFailure::rejected(format!("cannot open session control tokens: {error}"))
            })?;
    ilium_platform::secure_fs::validate_private_open_file(&file)
        .map_err(|error| CtlFailure::rejected(error.to_string()))?;
    let mut text = String::new();
    file.take(16 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|error| CtlFailure::rejected(error.to_string()))?;
    if text.len() > 16 * 1024 {
        return Err(CtlFailure::rejected("control token file exceeds limit"));
    }
    let tokens: Tokens =
        serde_json::from_str(&text).map_err(|error| CtlFailure::rejected(error.to_string()))?;
    let token = tokens.token(desired).to_owned();
    let granted = tokens
        .authenticate(Some(&token))
        .map_err(CtlFailure::rejected)?;
    desired
        .min(granted)
        .authorize(operation)
        .map_err(CtlFailure::rejected)?;
    Ok(token)
}
