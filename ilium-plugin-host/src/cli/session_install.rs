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
    let mut allow_unsigned = false;
    let mut yes = false;
    let mut remaining = Vec::new();
    for argument in args {
        match argument.as_str() {
            "--allow-unsigned" => allow_unsigned = true,
            "--yes" => yes = true,
            "--allow-downgrade" => {}
            _ => remaining.push(argument.clone()),
        }
    }
    let Some(session) = &env.session else {
        return offline::failure(
            "command.context-required",
            "No trusted installation service was supplied",
            1,
        );
    };
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
    if output.exit_code != 0 {
        return output;
    }
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
    }
    output
}
