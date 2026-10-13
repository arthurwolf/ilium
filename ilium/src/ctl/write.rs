//! `ilium ctl` commands that change a session.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ilium_control::operation::{AgentProvider, PaneLaunch, QueueDelivery, StepDirection};
use ilium_control::{resolve_project, Operation, SelectError};
use ilium_core::{NodeId, RestructurePlan, ROOT_ID};
use ilium_ipc::{ClientRequest, ServerEvent, WorkspaceCreateSpec};
use serde_json::{json, Value};

use super::args::{CtlCommand, NewPaneArgs, Target};
use super::link::{self, owning_project, Link};
use super::{print_result, CtlFailure};

pub(super) async fn run(command: CtlCommand, target: Target, cwd: &Path) -> Result<(), CtlFailure> {
    if let CtlCommand::NewPane(args) = command {
        return new_pane(args, &target, cwd).await;
    }
    let mut link = link::open(&target, cwd, false).await?;
    let result = apply_command(command, &mut link).await;
    link.detach().await;
    let (name, details) = result?;
    print_result(name, details);
    Ok(())
}

async fn apply_command(
    command: CtlCommand,
    link: &mut Link,
) -> Result<(&'static str, Value), CtlFailure> {
    let operation = to_operation(command, link)?;
    let details = link.apply(&operation).await?;
    Ok((operation.name(), details))
}

fn project_id(link: &Link, selector: &str) -> Result<u64, CtlFailure> {
    Ok(resolve_project(link.tree(), selector).map(|id| id.0)?)
}

fn read_plan(path: &Path) -> Result<RestructurePlan, CtlFailure> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| CtlFailure::usage(format!("cannot read {}: {error}", path.display())))?;
    serde_json::from_str(&text).map_err(|error| {
        CtlFailure::usage(format!(
            "{} is not a restructure plan: {error}",
            path.display()
        ))
    })
}

fn read_operation_json(argument: &str) -> Result<Operation, CtlFailure> {
    let text = if argument == "-" {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(|error| CtlFailure::usage(format!("cannot read standard input: {error}")))?;
        text
    } else {
        argument.to_owned()
    };
    serde_json::from_str(&text).map_err(|error| {
        CtlFailure::usage(format!(
            "not a valid operation: {error}; see `ilium ctl ops`"
        ))
    })
}

fn container(parent: Option<u64>, project: Option<String>, link: &Link) -> Result<u64, CtlFailure> {
    match (parent, project) {
        (Some(parent), _) => Ok(parent),
        (None, Some(selector)) => project_id(link, &selector),
        (None, None) => Err(CtlFailure::usage("give --project or --parent")),
    }
}

fn to_operation(command: CtlCommand, link: &Link) -> Result<Operation, CtlFailure> {
    Ok(match command {
        CtlCommand::Rename { id, title } => Operation::Rename { node: id, title },
        CtlCommand::Close { id, worktree } => Operation::Close { pane: id, worktree },
        CtlCommand::Move {
            id,
            up,
            down,
            to,
            index,
        } => match (up, down, to) {
            (true, _, _) => Operation::MoveStep {
                node: id,
                direction: StepDirection::Up,
            },
            (_, true, _) => Operation::MoveStep {
                node: id,
                direction: StepDirection::Down,
            },
            (_, _, Some(parent)) => Operation::Reparent {
                node: id,
                parent,
                index,
            },
            _ => return Err(CtlFailure::usage("give --up, --down or --to PARENT")),
        },
        CtlCommand::NewProject { path } => Operation::NewProject { path },
        CtlCommand::NewGroup {
            name,
            project,
            parent,
        } => Operation::NewGroup {
            parent: container(parent, project, link)?,
            name,
        },
        CtlCommand::NewFolder {
            path,
            project,
            parent,
        } => Operation::NewFolder {
            parent: container(parent, project, link)?,
            path,
        },
        CtlCommand::ChangeFolder { project, path } => Operation::ChangeProjectFolder {
            project: project_id(link, &project)?,
            path,
        },
        CtlCommand::Send { id, text, no_enter } => Operation::SendText {
            pane: id,
            text,
            enter: !no_enter,
        },
        CtlCommand::Keys { id, keys } => Operation::SendKeys { pane: id, keys },
        CtlCommand::Schedule {
            id,
            delay_seconds,
            text,
            no_enter,
        } => Operation::Schedule {
            pane: id,
            delay_seconds,
            text,
            enter: !no_enter,
        },
        CtlCommand::Queue {
            id,
            text,
            times,
            forever,
        } => Operation::Enqueue {
            pane: id,
            text,
            delivery: match (times, forever) {
                (_, true) => QueueDelivery::Forever,
                (Some(runs), _) => QueueDelivery::Times { runs },
                _ => QueueDelivery::Once,
            },
        },
        CtlCommand::QueueClear { id } => Operation::ClearQueue { pane: id },
        CtlCommand::Bookmark { id, off } => Operation::Bookmark { node: id, on: !off },
        CtlCommand::Expand { id } => Operation::Expand {
            node: id,
            expanded: true,
        },
        CtlCommand::Collapse { id } => Operation::Expand {
            node: id,
            expanded: false,
        },
        CtlCommand::LockClosed { id, off } => Operation::LockClosed {
            node: id,
            locked: !off,
        },
        CtlCommand::Terminate { id } => Operation::Terminate { pane: id },
        CtlCommand::Restructure { plan, project } => Operation::ApplyRestructure {
            plan: read_plan(&plan)?,
            project: project
                .map(|selector| project_id(link, &selector))
                .transpose()?,
        },
        CtlCommand::RevertRestructure { project } => match project {
            Some(selector) => Operation::RevertProjectRestructure {
                project: project_id(link, &selector)?,
            },
            None => Operation::RevertRestructure,
        },
        CtlCommand::KillSession => Operation::KillSession,
        CtlCommand::Op { json } => read_operation_json(&json)?,
        _ => return Err(CtlFailure::usage("not a mutating command")),
    })
}

/// `new-pane`: placement, launch kind, working directory, optional worktree.
async fn new_pane(args: NewPaneArgs, target: &Target, cwd: &Path) -> Result<(), CtlFailure> {
    let mut link = link::open(target, cwd, true).await?;
    let result = new_pane_on(&mut link, args).await;
    link.detach().await;
    let (name, details) = result?;
    print_result(name, details);
    Ok(())
}

async fn new_pane_on(
    link: &mut Link,
    args: NewPaneArgs,
) -> Result<(&'static str, Value), CtlFailure> {
    let parent = resolve_parent(link, &args).await?;
    let provider = args.agent.as_deref().map(parse_provider).transpose()?;
    if args.worktree {
        let provider = provider.ok_or_else(|| CtlFailure::usage("--worktree needs --agent"))?;
        let branch = args.branch.clone().unwrap_or_default();
        let details = create_worktree_agent(link, parent, provider, &branch, &args).await?;
        return Ok(("new_pane", details));
    }
    let launch = match (provider, args.editor.clone(), args.command.is_empty()) {
        (Some(provider), _, _) => PaneLaunch::Agent {
            provider,
            prompt: args.prompt.clone(),
        },
        (None, Some(path), _) => PaneLaunch::Editor { path },
        (None, None, false) => PaneLaunch::Command {
            command: crate::shell_join(&args.command),
        },
        (None, None, true) => PaneLaunch::Shell,
    };
    let operation = Operation::NewPane {
        parent,
        launch,
        cwd: args.workdir.clone(),
        keep_open: args.keep_open,
    };
    let details = link.apply(&operation).await?;
    Ok((operation.name(), details))
}

fn parse_provider(name: &str) -> Result<AgentProvider, CtlFailure> {
    AgentProvider::parse(name).ok_or_else(|| {
        CtlFailure::usage(format!("unknown agent {name:?}; use claude, codex or agy"))
    })
}

/// The container the new pane goes in. An absolute path that is not yet a
/// project is added as one first.
async fn resolve_parent(link: &mut Link, args: &NewPaneArgs) -> Result<Option<u64>, CtlFailure> {
    if let Some(parent) = args.parent {
        return Ok(Some(parent));
    }
    let Some(selector) = &args.project else {
        return Ok(None);
    };
    match resolve_project(link.tree(), selector) {
        Ok(id) => Ok(Some(id.0)),
        Err(SelectError::NotFound { .. }) if is_existing_absolute_directory(selector) => {
            let operation = Operation::NewProject {
                path: PathBuf::from(selector),
            };
            let details = link.apply(&operation).await?;
            let created = details["created"].as_u64().ok_or_else(|| {
                CtlFailure::rejected("the project was added but its id could not be read")
            })?;
            link.refresh().await?;
            Ok(Some(created))
        }
        Err(error) => Err(error.into()),
    }
}

fn is_existing_absolute_directory(text: &str) -> bool {
    let path = Path::new(text);
    path.is_absolute() && path.is_dir()
}

/// The same worktree flow as `ilium new-pane --worktree`, addressed to a
/// project: query repository facts, expand the configured location template,
/// create the agent, and report the new pane and worktree path.
async fn create_worktree_agent(
    link: &mut Link,
    parent: Option<u64>,
    provider: AgentProvider,
    branch: &str,
    args: &NewPaneArgs,
) -> Result<Value, CtlFailure> {
    ilium_core::validate_branch_name(branch).map_err(|error| {
        CtlFailure::usage(format!("invalid worktree branch {branch:?}: {error}"))
    })?;
    let git_settings = ilium_platform::paths::config_dir()
        .map(|directory| ilium_client::config::load(&directory).map(|config| config.git))
        .transpose()
        .map_err(|error| {
            CtlFailure::usage(format!("cannot read the Ilium configuration: {error}"))
        })?
        .unwrap_or_default();
    let parent_group = parent.map_or(ROOT_ID, NodeId);
    let project = if parent_group == ROOT_ID {
        ROOT_ID
    } else {
        owning_project(link.tree(), parent_group).unwrap_or(parent_group)
    };
    let request_id = crate::next_progress_request_id();
    link.send(ClientRequest::QueryRepoFacts {
        request_id,
        project,
    })
    .await?;
    let facts =
        wait_for_event(
            link,
            Duration::from_secs(30),
            "repository facts",
            |event| match event {
                ServerEvent::RepoFactsReported {
                    request_id: reply,
                    result,
                    ..
                } if reply == request_id => Some(result.map_err(CtlFailure::rejected)),
                _ => None,
            },
        )
        .await??;
    let path =
        crate::default_workspace_path(&facts, branch, &git_settings.worktree_location_template)
            .map_err(|error| CtlFailure::rejected(error.to_string()))?;
    let base_ref = args
        .base
        .clone()
        .unwrap_or_else(|| facts.default_base_ref.clone());
    let setup_command = git_settings.setup_command.clone();
    let spec = if setup_command.trim().is_empty() {
        WorkspaceCreateSpec::New {
            branch: branch.to_owned(),
            base_ref: base_ref.clone(),
            path: path.clone(),
        }
    } else {
        WorkspaceCreateSpec::NewWithSetup {
            branch: branch.to_owned(),
            base_ref: base_ref.clone(),
            path: path.clone(),
            setup_command,
        }
    };
    link.send(ClientRequest::CreateAgentInWorkspace {
        request_id,
        parent_group,
        provider: provider.builtin(),
        spec,
        initial_input: args.prompt.clone().filter(|text| !text.trim().is_empty()),
    })
    .await?;
    let pane_id = wait_for_event(
        link,
        Duration::from_secs(180),
        "worktree creation",
        |event| match event {
            ServerEvent::WorkspaceCreated {
                request_id: reply,
                pane_id,
            } if reply == request_id => Some(Ok(pane_id)),
            ServerEvent::WorkspaceCreateFailed {
                request_id: reply,
                error,
            } if reply == request_id => Some(Err(CtlFailure::rejected(error))),
            _ => None,
        },
    )
    .await??;
    Ok(json!({
        "created": pane_id.0,
        "branch": branch,
        "base": base_ref,
        "worktree_path": path.to_string_lossy(),
    }))
}

/// Reads server events until `select` returns a value, a server error
/// arrives, or `timeout` passes.
async fn wait_for_event<T>(
    link: &mut Link,
    timeout: Duration,
    what: &str,
    mut select: impl FnMut(ServerEvent) -> Option<T>,
) -> Result<T, CtlFailure> {
    let wait = async {
        while let Some(event) = link.attached.connection.events.recv().await {
            let (event, _retention) = event.into_parts();
            if let ServerEvent::Error { message } = &event {
                return Err(CtlFailure::rejected(message.clone()));
            }
            if let Some(value) = select(event) {
                return Ok(value);
            }
        }
        Err(CtlFailure::no_session(format!(
            "the session closed before the {what} arrived"
        )))
    };
    tokio::time::timeout(timeout, wait).await.map_err(|_| {
        CtlFailure::rejected(format!(
            "timed out waiting for the {what}; inspect the session before retrying"
        ))
    })?
}
