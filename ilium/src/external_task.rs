//! Explicit loopback client for sequence-fenced external tasks.
use clap::{Args, Subcommand};
use serde_json::{json, Value};
use std::time::Duration;

use crate::CliError;

#[derive(Debug, Args)]
pub(crate) struct ExternalTaskArgs {
    /// Loopback HTTP port of the target Ilium server.
    #[arg(long)]
    port: u16,
    /// Read or act token; alternatively set ILIUM_CONTROL_TOKEN.
    #[arg(long)]
    token: Option<String>,
    #[command(subcommand)]
    command: ExternalTaskCommand,
}

#[derive(Debug, Subcommand)]
enum ExternalTaskCommand {
    List,
    Upsert {
        producer: String,
        job: String,
        sequence: u64,
        title: String,
        status: String,
    },
    Remove {
        producer: String,
        job: String,
        sequence: u64,
    },
}

/// Use the same sequence when retrying an update with an uncertain response.
pub(crate) async fn run(args: ExternalTaskArgs) -> Result<(), CliError> {
    match execute(args).await {
        Ok(value) => {
            println!("{}", json!({"type":"result", "result":value}));
            Ok(())
        }
        Err(error) => {
            println!("{}", json!({"type":"error", "message":error}));
            Err(CliError::ExitStatus(3))
        }
    }
}

async fn execute(args: ExternalTaskArgs) -> Result<Value, String> {
    let token = args
        .token
        .or_else(|| std::env::var("ILIUM_CONTROL_TOKEN").ok())
        .filter(|value| !value.is_empty())
        .ok_or("an access token is required")?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| error.to_string())?;
    let url = format!("http://127.0.0.1:{}/v1/external-tasks", args.port);
    let request = match args.command {
        ExternalTaskCommand::List => client.get(url),
        ExternalTaskCommand::Upsert { producer, job, sequence, title, status } => client.post(url).json(&json!({"key":{"producer":producer,"job":job},"observation":{"sequence":sequence,"value":{"title":title,"status":status}}})),
        ExternalTaskCommand::Remove { producer, job, sequence } => client.post(url).json(&json!({"key":{"producer":producer,"job":job},"observation":{"sequence":sequence,"value":null}})),
    };
    let mut response =
        request.bearer_auth(token).send().await.map_err(|error| {
            format!("request outcome unknown; retry the same sequence: {error}")
        })?;
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("response incomplete; retry the same sequence: {error}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > 8 * 1024 * 1024 {
            return Err("response exceeds limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid response: {error}"))?;
    if !status.is_success() {
        return Err(format!("server returned {status}: {value}"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_ordered_removal_without_starting_a_session() {
        let cli = crate::Cli::try_parse_from([
            "ilium",
            "external-task",
            "--port",
            "32191",
            "--token",
            "test-token",
            "remove",
            "fixture",
            "job-1",
            "7",
        ])
        .unwrap();
        let Some(crate::Command::ExternalTask(args)) = cli.command else {
            panic!("wrong command")
        };
        assert_eq!(args.port, 32191);
        assert!(matches!(
            args.command,
            ExternalTaskCommand::Remove { sequence: 7, .. }
        ));
    }
}
