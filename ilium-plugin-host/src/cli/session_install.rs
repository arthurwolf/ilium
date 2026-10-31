//! `install` over the session: builds the trusted local installer from the command flags and the
//! session's consent answers, then tells the session what the real installer admitted.
use std::sync::Arc;

use super::{
    install_command, local_install, offline, CliEnv, CliOutput, InstallRecord, SessionRequest,
    SessionResponse,
};
use crate::{
    audit::{AuditActor, AuditContext},
    install::{Consent, ConsentDecision, ConsentRequest, InstallOptions},
};

/// Review answered by the attached session, or approved by `--yes`.
struct SessionConsent {
    env: CliEnv,
    approve_without_prompt: bool,
}

impl Consent for SessionConsent {
    fn decide(&self, request: &ConsentRequest) -> ConsentDecision {
        if self.approve_without_prompt {
            return ConsentDecision::Approve(request.requested.clone());
        }
        let Some(session) = &self.env.session else {
            return ConsentDecision::Deny;
        };
        let grants = request
            .requested
            .iter()
            .map(|grant| grant.as_str().to_owned())
            .collect();
        match session.request(SessionRequest::Review { grants }) {
            Ok(SessionResponse::Decision(true)) => {
                ConsentDecision::Approve(request.requested.clone())
            }
            _ => ConsentDecision::Deny,
        }
    }
}

pub(super) fn run(args: &[String], env: &CliEnv) -> CliOutput {
    let (flags, remaining) = match install_command::parse_install_flags(args) {
        Ok(parsed) => parsed,
        Err(output) => return output,
    };
    let (allow_unsigned, yes) = (flags.allow_unsigned, flags.yes);
    if !yes && env.session.is_none() && env.is_ci && !flags.dry_run {
        return offline::failure(
            "install.needs-input",
            "The review needs an answer; pass --yes in a non-interactive run",
            1,
        );
    }
    let context = AuditContext {
        session: "cli".to_owned(),
        actor: AuditActor {
            kind: "cli".to_owned(),
            pane: None,
            plugin: None,
        },
    };
    let consent = Arc::new(SessionConsent {
        env: env.clone(),
        approve_without_prompt: yes,
    });
    let backend = match local_install::factory_with_options(
        env,
        consent,
        context,
        InstallOptions { allow_unsigned },
    ) {
        Ok(backend) => backend,
        Err(diagnostic) => {
            return offline::failure(diagnostic.code.as_str(), diagnostic.message, 1);
        }
    };
    let output = install_command::run(&remaining, &env.cwd, backend.as_ref());
    if output.exit_code != 0 || flags.dry_run {
        return output;
    }
    let Some(session) = &env.session else {
        return output;
    };
    let plugin = output
        .stdout_jsonl
        .iter()
        .find(|record| record["type"] == "plugin");
    if let Some(plugin) = plugin {
        let text = |key: &str| plugin[key].as_str().unwrap_or_default().to_owned();
        let id = text("id");
        let record = InstallRecord {
            data_dir: env.data_dir.join("plugins").join(&id),
            id,
            version: text("version"),
            tier: text("tier"),
            dev: false,
            grants: Vec::new(),
        };
        if let Err(message) = session.request(SessionRequest::RecordInstall {
            record,
            state: "enabled".to_owned(),
        }) {
            return offline::failure("session.unreachable", message, 6);
        }
        if let Err(message) = session.request(SessionRequest::Enable {
            id: text("id"),
        }) {
            return offline::failure("session.unreachable", message, 6);
        }
    }
    output
}
