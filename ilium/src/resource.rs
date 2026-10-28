//! Thin JSONL command client for the server-owned shared resource authority.
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{Args, Subcommand};
use ilium_client::connection::Connection;
use ilium_core::{resource_queue::*, NodeId};
use ilium_ipc::resource::{ResourceOperation, ResourceReply, ResourceRequest};
use ilium_ipc::{ClientRequest, ServerEvent};
use serde_json::{json, Value};

use crate::{next_progress_request_id, pane_identity_from_env, session, CliError};

#[derive(Args, Debug)]
pub(crate) struct ResourceArgs {
    /// Select the project's existing server. Does not start a server.
    #[arg(long, global = true)]
    project: Option<PathBuf>,
    #[arg(long, global = true)]
    session: Option<String>,
    /// Pane number or session/pane number; never an arbitrary display label.
    #[arg(long, global = true)]
    agent: Option<String>,
    /// Reuse the exact time-bearing key when retrying a mutation.
    #[arg(long, global = true)]
    request_id: Option<String>,
    #[command(subcommand)]
    command: ResourceCommandArgs,
}

#[derive(Subcommand, Debug)]
enum ResourceCommandArgs {
    List {
        #[arg(long)]
        all: bool,
    },
    Show {
        resource: String,
    },
    /// Reserve SLUG DURATION, or AGENT SLUG DURATION. Estimates never expire ownership.
    Reserve {
        #[arg(num_args=2..=3)]
        values: Vec<String>,
    },
    /// Observe an existing reservation until acquired. Interrupting preserves the claim.
    Wait {
        reservation: String,
    },
    Status {
        reservation: Option<String>,
    },
    Release {
        reservation: String,
    },
    Cancel {
        reservation: String,
        #[arg(long, requires = "expected_revision")]
        reason: Option<String>,
        #[arg(long, requires = "reason")]
        expected_revision: Option<u64>,
    },
    Estimate {
        reservation: String,
        duration: String,
    },
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        slug: String,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long, default_value = "")]
        rules: String,
        #[arg(long, default_value = "generic")]
        kind: String,
        #[arg(long)]
        device_identity: Option<String>,
    },
    Link {
        resource: String,
        #[arg(long)]
        slug: String,
    },
    Edit {
        resource: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        rules: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long, conflicts_with = "clear_device_identity")]
        device_identity: Option<String>,
        #[arg(long)]
        clear_device_identity: bool,
    },
    Unlink {
        resource: String,
    },
    Archive {
        resource: String,
    },
    /// Rebind a stable project identity after an explicit directory move.
    Rebind {
        project_id: String,
        project_path: PathBuf,
        #[arg(long)]
        expected_revision: u64,
    },
    /// Retry managed instruction synchronization for a linked resource.
    Repair {
        resource: String,
    },
    /// Guarded user recovery. Does not stop workloads or reset devices.
    Recover {
        resource: String,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long)]
        reason: String,
    },
    /// Pause or mark unavailable without releasing physical occupancy.
    Gate {
        resource: String,
        #[arg(value_parser=["open","paused","unavailable"])]
        state: String,
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Debug, PartialEq)]
struct ReserveInput {
    agent: Option<String>,
    resource: String,
    seconds: u64,
}
fn reserve_input(values: &[String], flag_agent: Option<&str>) -> Result<ReserveInput, String> {
    let (agent, resource, duration) = match values {
        [resource, duration] => (flag_agent.map(str::to_owned), resource, duration),
        [agent, resource, duration] if flag_agent.is_none() => {
            (Some(agent.clone()), resource, duration)
        }
        [_, _, _] => return Err("Use either positional AGENT or --agent, not both".into()),
        _ => return Err("Expected SLUG DURATION or AGENT SLUG DURATION".into()),
    };
    let seconds = parse_resource_duration(duration).map_err(|error| error.to_string())?;
    if resource.is_empty() || resource.chars().any(char::is_whitespace) {
        return Err("Resource must be a project slug or resource identifier".into());
    }
    Ok(ReserveInput {
        agent,
        resource: resource.clone(),
        seconds,
    })
}

fn request_key() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!(
        "{millis}:{}-{}",
        std::process::id(),
        next_progress_request_id()
    )
}
fn error(
    operation: &str,
    key: &str,
    code: &str,
    message: impl std::fmt::Display,
    exit: u8,
) -> CliError {
    println!(
        "{}",
        json!({"type":"error","operation":operation,"request_id":key,"code":code,"message":message.to_string()})
    );
    CliError::ExitStatus(exit)
}
fn parse_agent(reference: &str) -> Result<(Option<String>, NodeId), String> {
    let (session, pane) = reference
        .rsplit_once('/')
        .map_or((None, reference), |(session, pane)| {
            (Some(session.to_owned()), pane)
        });
    let pane = pane
        .parse::<u64>()
        .ok()
        .filter(|pane| *pane != 0)
        .ok_or("Agent reference must be a nonzero pane number or session/pane number")?;
    Ok((session, NodeId(pane)))
}

/// One correlated request; only an explicit Busy refusal permits resubmission.
/// Blocking resource waits retain their unlimited observation lifetime.
async fn exchange(
    connection: &mut Connection,
    request: ResourceRequest,
    key: &str,
    operation: &str,
    wait: bool,
) -> Result<ResourceReply, CliError> {
    let request_id = next_progress_request_id();
    connection
        .requests
        .send(ClientRequest::Resource {
            request_id,
            request: request.clone(),
        })
        .await
        .map_err(|e| error(operation, key, "outcome_unknown", e, 4))?;
    let receive = async {
        let mut reported_busy = false;
        loop {
            let received = connection.events.recv().await.ok_or_else(|| error(operation,key,"outcome_unknown","Server disconnected; retain the reservation and retry status/wait with the same identifier",4))?;
            let (event, retention) = received.into_parts();
            if let ServerEvent::ResourceResult {
                request_id: response_id,
                result,
            } = event
            {
                if response_id == request_id {
                    if let Err(refusal) = &result {
                        if refusal.code == "busy" && !reported_busy {
                            eprintln!(
                                "Resource {operation} is retrying a busy request: {}",
                                refusal.message
                            );
                            reported_busy = true;
                        }
                    }
                    if matches!(&result, Err(error) if error.code == "busy") {
                        // Explicit Busy permits a same-key retry. Keep both the
                        // durable key and wire correlation; never retry an
                        // uncertain outcome or revision conflict automatically.
                        drop(retention);
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        connection
                            .requests
                            .send(ClientRequest::Resource {
                                request_id,
                                request: request.clone(),
                            })
                            .await
                            .map_err(|e| error(operation, key, "outcome_unknown", e, 4))?;
                        continue;
                    }
                    return result.map(|reply| *reply).map_err(|e| {
                        let exit = match e.code.as_str() {
                            "cancelled"
                            | "revoked"
                            | "reservation_cancelled"
                            | "reservation_revoked" => 3,
                            "not_found" | "reservation_not_found" | "receipt_expired" => 5,
                            "outcome_unknown" | "observation_failed" | "store_error" => 4,
                            _ => 2,
                        };
                        error(operation, key, &e.code, e.message, exit)
                    });
                }
            }
        }
    };
    tokio::pin!(receive);
    tokio::select! {
        result = &mut receive => result,
        signal = tokio::signal::ctrl_c() => {
            let message = if signal.is_ok() { "Observer interrupted; reservation retained. Use status or wait on the same reservation.".to_owned() } else { "Signal observation failed; reservation outcome unknown".to_owned() };
            Err(error(operation,key,"interrupted",message,130))
        }
        _ = tokio::time::sleep(Duration::from_secs(45)), if !wait => Err(error(operation,key,"outcome_unknown","No acknowledgement; retry with the same request identifier",4)),
    }
}

fn resolve_resource(
    reply: &ResourceReply,
    name: &str,
    project_path: &str,
) -> Result<ResourceId, String> {
    if reply
        .catalog
        .resources
        .contains_key(&ResourceId(name.into()))
    {
        return Ok(ResourceId(name.into()));
    }
    let matches: Vec<_> = reply
        .catalog
        .links
        .iter()
        .filter(|link| link.slug == name && link.project_path == project_path)
        .collect();
    match matches.as_slice() {
        [link] => Ok(link.resource_id.clone()),
        [] => Err(format!(
            "Resource {name:?} is not linked in the selected project"
        )),
        _ => Err(format!(
            "Resource slug {name:?} is ambiguous; use its identifier"
        )),
    }
}
fn find_reservation<'a>(reply: &'a ResourceReply, id: &str) -> Option<&'a Reservation> {
    reply
        .catalog
        .resources
        .values()
        .flat_map(|r| &r.reservations)
        .find(|r| r.id.0 == id)
}
fn result_json(
    reply: &ResourceReply,
    operation: &str,
    key: &str,
    reservation: Option<&Reservation>,
) -> Value {
    let resource_id = reservation
        .map(|r| &r.resource_id)
        .or_else(|| reply.result.as_ref().and_then(|r| r.resource_id.as_ref()));
    let mut value = json!({"type":"result","operation":operation,"request_id":key,"revision":reply.catalog.revision,"resource_id":resource_id,"result":reply.result,"message":reply.message,"reused":reply.result.as_ref().is_some_and(|r|r.reused)});
    if let Some(reservation) = reservation {
        let snapshot = reply
            .catalog
            .resources
            .get(&reservation.resource_id)
            .map(|r| project_resource_queue(r, &reply.clock));
        let entry = snapshot
            .as_ref()
            .and_then(|s| s.queue.iter().find(|q| q.reservation.id == reservation.id));
        let state = match reservation.state {
            ReservationState::Queued => "queued",
            ReservationState::Owned => "owned",
            ReservationState::NeedsAttention { .. } => "needs_attention",
            ReservationState::Released => "released",
            ReservationState::Cancelled => "cancelled",
            ReservationState::Revoked { .. } => "revoked",
        };
        value["reservation_id"] = json!(reservation.id);
        value["resource_slug"] = json!(reply
            .catalog
            .links
            .iter()
            .find(|link| link.project_id == reservation.agent.project_id
                && link.resource_id == reservation.resource_id)
            .map(|link| link.slug.as_str()));
        value["state"] = json!(state);
        value["actor"] = json!(reservation.agent);
        value["ownership_generation"] = json!(reservation.ownership_generation);
        value["queue_position"] = json!(entry.map(|q| q.position));
        value["ahead_count"] = json!(entry.map(|q| q.position.saturating_sub(1)
            + usize::from(snapshot.as_ref().is_some_and(|s| s.owner.is_some()))));
        value["estimated_wait_seconds"] = json!(entry.and_then(|q| q.estimate.wait_seconds));
        value["estimate_reason"] = json!(entry.and_then(|q| q.estimate.reason.as_deref()));
        value["estimate_confidence"] = json!(if entry
            .is_some_and(|q| q.estimate.wait_seconds.is_some())
        {
            "estimated"
        } else {
            "unknown"
        });
    }
    value
}

fn stable_id(prefix: &str, key: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{prefix}-{:x}", Sha256::digest(key.as_bytes()))
}

pub(crate) async fn run(args: ResourceArgs, cwd: &Path) -> Result<(), CliError> {
    let key = args.request_id.clone().unwrap_or_else(request_key);
    let operation = match &args.command {
        ResourceCommandArgs::List { .. } => "resource_list",
        ResourceCommandArgs::Show { .. } => "resource_show",
        ResourceCommandArgs::Reserve { .. } => "resource_reserve",
        ResourceCommandArgs::Wait { .. } => "resource_wait",
        ResourceCommandArgs::Status { .. } => "resource_status",
        ResourceCommandArgs::Release { .. } => "resource_release",
        ResourceCommandArgs::Cancel { .. } => "resource_cancel",
        ResourceCommandArgs::Estimate { .. } => "resource_estimate",
        ResourceCommandArgs::Create { .. } => "resource_create",
        ResourceCommandArgs::Link { .. } => "resource_link",
        ResourceCommandArgs::Edit { .. } => "resource_edit",
        ResourceCommandArgs::Unlink { .. } => "resource_unlink",
        ResourceCommandArgs::Archive { .. } => "resource_archive",
        ResourceCommandArgs::Recover { .. } => "resource_recover",
        ResourceCommandArgs::Gate { .. } => "resource_gate",
        ResourceCommandArgs::Rebind { .. } => "resource_rebind",
        ResourceCommandArgs::Repair { .. } => "resource_repair",
    };
    let reserve = if let ResourceCommandArgs::Reserve { values } = &args.command {
        Some(
            reserve_input(values, args.agent.as_deref())
                .map_err(|e| error(operation, &key, "invalid_input", e, 2))?,
        )
    } else {
        None
    };
    let explicit = reserve
        .as_ref()
        .and_then(|r| r.agent.as_deref())
        .or(args.agent.as_deref());
    let parsed_agent = explicit
        .map(parse_agent)
        .transpose()
        .map_err(|e| error(operation, &key, "invalid_identity", e, 2))?;
    let inherited = pane_identity_from_env().ok();
    let project = args.project.as_deref().unwrap_or(cwd);
    let project_path = ilium_platform::paths::canonicalize(project)
        .map_err(|e| error(operation, &key, "invalid_project", e, 2))?
        .to_string_lossy()
        .into_owned();
    let selected_session = args
        .session
        .as_deref()
        .or_else(|| parsed_agent.as_ref().and_then(|(s, _)| s.as_deref()));
    if let (Some(session), Some((Some(agent_session), _))) =
        (args.session.as_ref(), parsed_agent.as_ref())
    {
        if session != agent_session {
            return Err(error(
                operation,
                &key,
                "invalid_identity",
                "--session disagrees with agent reference",
                2,
            ));
        }
    }
    let use_inherited = args.project.is_none() && selected_session.is_none();
    let (socket, session_name, pane) = if use_inherited {
        if let Some(identity) = inherited.as_ref() {
            (
                identity.socket_path.clone(),
                identity.session_name.clone(),
                parsed_agent
                    .as_ref()
                    .map(|(_, p)| *p)
                    .or(Some(identity.pane_id)),
            )
        } else {
            let resolved = session::resolve_project_session(project, session::DEFAULT_SESSION_NAME)
                .map_err(|e| error(operation, &key, "invalid_project", e, 2))?;
            (
                resolved.socket_path,
                resolved.name,
                parsed_agent.as_ref().map(|(_, p)| *p),
            )
        }
    } else {
        let resolved = session::resolve_project_session(
            project,
            selected_session.unwrap_or(session::DEFAULT_SESSION_NAME),
        )
        .map_err(|e| error(operation, &key, "invalid_project", e, 2))?;
        (
            resolved.socket_path,
            resolved.name,
            parsed_agent.as_ref().map(|(_, p)| *p),
        )
    };
    let user_cancel = matches!(
        &args.command,
        ResourceCommandArgs::Cancel {
            reason: Some(_),
            ..
        }
    );
    let agent_command = !user_cancel
        && matches!(
            args.command,
            ResourceCommandArgs::Reserve { .. }
                | ResourceCommandArgs::Wait { .. }
                | ResourceCommandArgs::Release { .. }
                | ResourceCommandArgs::Cancel { .. }
                | ResourceCommandArgs::Estimate { .. }
        );
    if agent_command && pane.is_none() {
        return Err(error(
            operation,
            &key,
            "identity_required",
            "Outside an Ilium pane, supply --agent SESSION/PANE and select its project",
            2,
        ));
    }
    let mut connection = Connection::connect(&socket, session_name)
        .await
        .map_err(|e| error(operation, &key, "service_unavailable", e, 4))?;
    let mut snapshot_request = ResourceRequest::new(None, ResourceOperation::Snapshot);
    snapshot_request.pane_id = pane;
    let snapshot = exchange(&mut connection, snapshot_request, &key, operation, false).await?;
    let resolve = |name: &str| {
        resolve_resource(&snapshot, name, &project_path)
            .map_err(|e| error(operation, &key, "resource_not_found", e, 2))
    };
    let link_for = |resource_id: ResourceId, slug: String| ProjectResourceLink {
        project_id: ResourceProjectId("selected-project".into()),
        resource_id,
        slug,
        project_path: project_path.clone(),
        created_at_unix_millis: 0,
        desired_instruction_revision: 0,
        sync_debt: None,
    };
    let mut request = ResourceRequest::new(None, ResourceOperation::Snapshot);
    request.request_key = Some(key.clone());
    request.pane_id = if agent_command || matches!(args.command, ResourceCommandArgs::Status { .. })
    {
        pane
    } else {
        None
    };
    let mut selected_reservation = None;
    let command = match args.command {
        ResourceCommandArgs::List { all } => {
            let resources: Vec<_> = snapshot
                .catalog
                .resources
                .values()
                .filter(|record| {
                    all || snapshot.catalog.links.iter().any(|link| {
                        link.project_path == project_path
                            && link.resource_id == record.definition.id
                    })
                })
                .collect();
            println!(
                "{}",
                json!({"type":"result","operation":operation,"request_id":key,"revision":snapshot.catalog.revision,"resources":resources,"links":snapshot.catalog.links.iter().filter(|link|all||link.project_path==project_path).collect::<Vec<_>>()})
            );
            return Ok(());
        }
        ResourceCommandArgs::Show { resource } => {
            let id = resolve(&resource)?;
            let record = &snapshot.catalog.resources[&id];
            println!(
                "{}",
                json!({"type":"result","operation":operation,"request_id":key,"revision":snapshot.catalog.revision,"resource":record,"queue":project_resource_queue(record,&snapshot.clock)})
            );
            return Ok(());
        }
        ResourceCommandArgs::Status { reservation: None } => {
            let reservations: Vec<_> = snapshot
                .catalog
                .resources
                .values()
                .flat_map(|r| &r.reservations)
                .filter(|r| {
                    r.state.active()
                        && pane.is_none_or(|p| p == r.agent.pane_id)
                        && snapshot
                            .local_invocations
                            .iter()
                            .any(|local| local.same_invocation(&r.agent))
                })
                .collect();
            println!(
                "{}",
                json!({"type":"result","operation":operation,"request_id":key,"revision":snapshot.catalog.revision,"reservations":reservations})
            );
            return Ok(());
        }
        ResourceCommandArgs::Status {
            reservation: Some(id),
        } => {
            selected_reservation = Some(id.clone());
            request.operation = ResourceOperation::Status {
                reservation_id: ReservationId(id),
            };
            None
        }
        ResourceCommandArgs::Wait { reservation: id } => {
            selected_reservation = Some(id.clone());
            request.operation = ResourceOperation::Wait {
                reservation_id: ReservationId(id),
            };
            None
        }
        ResourceCommandArgs::Reserve { .. } => {
            let input = reserve.ok_or_else(|| {
                error(
                    operation,
                    &key,
                    "invalid_input",
                    "Missing parsed reserve arguments",
                    2,
                )
            })?;
            let id = stable_id("reservation", &key);
            selected_reservation = Some(id.clone());
            Some(ResourceCommand::Reserve {
                resource_id: resolve(&input.resource)?,
                reservation_id: ReservationId(id),
                estimate_seconds: input.seconds,
            })
        }
        ResourceCommandArgs::Release { reservation: id } => {
            let reservation = find_reservation(&snapshot, &id).ok_or_else(|| {
                error(
                    operation,
                    &key,
                    "reservation_not_found",
                    "Reservation missing; no release was sent",
                    5,
                )
            })?;
            selected_reservation = Some(id.clone());
            Some(ResourceCommand::Release {
                reservation_id: ReservationId(id),
                ownership_generation: reservation.ownership_generation,
            })
        }
        ResourceCommandArgs::Cancel {
            reservation: id,
            reason,
            expected_revision,
        } => {
            selected_reservation = Some(id.clone());
            request.expected_revision = expected_revision;
            if let Some(reason) = reason {
                if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
                    return Err(error(
                        operation,
                        &key,
                        "interactive_confirmation_required",
                        "User cancellation requires an interactive confirmation",
                        2,
                    ));
                }
                let resource_id = find_reservation(&snapshot, &id)
                    .ok_or_else(|| error(operation, &key, "not_found", "Reservation missing", 5))?
                    .resource_id
                    .clone();
                let prepared = exchange(
                    &mut connection,
                    ResourceRequest::new(None, ResourceOperation::PrepareRecovery { resource_id }),
                    &key,
                    operation,
                    false,
                )
                .await?;
                if expected_revision != Some(prepared.catalog.revision) {
                    return Err(error(
                        operation,
                        &key,
                        "stale_revision",
                        "Queue changed; inspect before cancelling",
                        2,
                    ));
                }
                eprint!("Cancel queued reservation {id}; reason: {reason}. Type CANCEL {id} to confirm: ");
                std::io::stderr()
                    .flush()
                    .map_err(|e| error(operation, &key, "confirmation_failed", e, 2))?;
                let mut confirmation = String::new();
                std::io::stdin()
                    .read_line(&mut confirmation)
                    .map_err(|e| error(operation, &key, "confirmation_failed", e, 2))?;
                if confirmation.trim() != format!("CANCEL {id}") {
                    return Err(error(
                        operation,
                        &key,
                        "confirmation_declined",
                        "Reservation was not cancelled",
                        2,
                    ));
                }
                request.recovery_token = prepared.recovery_token;
                request.pane_id = None;
                Some(ResourceCommand::CancelByUser {
                    reservation_id: ReservationId(id),
                    reason,
                })
            } else {
                Some(ResourceCommand::Cancel {
                    reservation_id: ReservationId(id),
                })
            }
        }
        ResourceCommandArgs::Estimate {
            reservation: id,
            duration,
        } => {
            selected_reservation = Some(id.clone());
            Some(ResourceCommand::Estimate {
                reservation_id: ReservationId(id),
                estimate_seconds: parse_resource_duration(&duration)
                    .map_err(|e| error(operation, &key, "invalid_duration", e, 2))?,
            })
        }
        ResourceCommandArgs::Create {
            name,
            slug,
            description,
            rules,
            kind,
            device_identity,
        } => {
            let id = ResourceId(stable_id("resource", &key));
            Some(ResourceCommand::Create {
                definition: ResourceDefinition {
                    id: id.clone(),
                    title: name,
                    description,
                    rules,
                    kind,
                    physical_identity: device_identity,
                    policy: ResourcePolicy::ExclusiveFifo,
                    created_at_unix_millis: 0,
                    updated_at_unix_millis: 0,
                    definition_revision: 0,
                    archived: false,
                },
                link: link_for(id, slug),
            })
        }
        ResourceCommandArgs::Link { resource, slug } => Some(ResourceCommand::Link {
            link: link_for(resolve(&resource)?, slug),
        }),
        ResourceCommandArgs::Edit {
            resource,
            name,
            description,
            rules,
            kind,
            device_identity,
            clear_device_identity,
        } => {
            let mut definition = snapshot.catalog.resources[&resolve(&resource)?]
                .definition
                .clone();
            if let Some(value) = name {
                definition.title = value;
            }
            if let Some(value) = description {
                definition.description = value;
            }
            if let Some(value) = rules {
                definition.rules = value;
            }
            if let Some(value) = kind {
                definition.kind = value;
            }
            if clear_device_identity {
                definition.physical_identity = None;
            } else if let Some(value) = device_identity {
                definition.physical_identity = Some(value);
            }
            request.expected_revision = Some(snapshot.catalog.revision);
            Some(ResourceCommand::Edit { definition })
        }
        ResourceCommandArgs::Unlink { resource } => Some(ResourceCommand::Unlink {
            project_id: ResourceProjectId("selected-project".into()),
            resource_id: resolve(&resource)?,
        }),
        ResourceCommandArgs::Archive { resource } => Some(ResourceCommand::Archive {
            resource_id: resolve(&resource)?,
        }),
        ResourceCommandArgs::Gate {
            resource,
            state,
            reason,
        } => {
            let gate = match state.as_str() {
                "open" => ResourceGate::Open,
                "paused" => ResourceGate::Paused {
                    reason: reason.ok_or_else(|| {
                        error(
                            operation,
                            &key,
                            "reason_required",
                            "A pause requires --reason",
                            2,
                        )
                    })?,
                },
                _ => ResourceGate::Unavailable {
                    reason: reason.ok_or_else(|| {
                        error(
                            operation,
                            &key,
                            "reason_required",
                            "Unavailability requires --reason",
                            2,
                        )
                    })?,
                },
            };
            request.expected_revision = Some(snapshot.catalog.revision);
            Some(ResourceCommand::SetGate {
                resource_id: resolve(&resource)?,
                gate,
            })
        }
        ResourceCommandArgs::Rebind {
            project_id,
            project_path,
            expected_revision,
        } => {
            let project_path = ilium_platform::paths::canonicalize(&project_path)
                .map_err(|e| error(operation, &key, "invalid_project", e, 2))?
                .to_string_lossy()
                .into_owned();
            request.expected_revision = Some(expected_revision);
            Some(ResourceCommand::RebindProject {
                project_id: ResourceProjectId(project_id),
                project_path,
            })
        }
        ResourceCommandArgs::Repair { resource } => {
            request.operation = ResourceOperation::RepairInstructions {
                resource_id: resolve(&resource)?,
            };
            None
        }
        ResourceCommandArgs::Recover {
            resource,
            expected_revision,
            reason,
        } => {
            let id = resolve(&resource)?;
            recover(&mut connection, id, expected_revision, reason, &key).await?;
            return Ok(());
        }
    };
    if let Some(command) = command {
        request.operation = ResourceOperation::Mutate(command);
    }
    let waiting = matches!(request.operation, ResourceOperation::Wait { .. });
    let reply = exchange(&mut connection, request, &key, operation, waiting).await?;
    let reservation = reply
        .reservation
        .as_ref()
        .or_else(|| {
            reply
                .result
                .as_ref()
                .and_then(|r| r.reservation_id.as_ref())
                .and_then(|id| find_reservation(&reply, &id.0))
        })
        .or_else(|| {
            selected_reservation
                .as_deref()
                .and_then(|id| find_reservation(&reply, id))
        });
    if waiting {
        return match reservation.map(|r| &r.state) {
            Some(ReservationState::Owned) => {
                println!("{}", result_json(&reply, operation, &key, reservation));
                Ok(())
            }
            Some(ReservationState::Cancelled | ReservationState::Revoked { .. }) => {
                println!("{}", result_json(&reply, operation, &key, reservation));
                Err(CliError::ExitStatus(3))
            }
            _ => Err(error(
                operation,
                &key,
                "outcome_unknown",
                "Wait returned without confirming current ownership; reservation retained",
                4,
            )),
        };
    }
    println!("{}", result_json(&reply, operation, &key, reservation));
    Ok(())
}

async fn recover(
    connection: &mut Connection,
    resource_id: ResourceId,
    expected_revision: u64,
    reason: String,
    key: &str,
) -> Result<(), CliError> {
    const OP: &str = "resource_recover";
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return Err(error(
            OP,
            key,
            "interactive_confirmation_required",
            "Recovery requires an interactive terminal; no headless confirmation flag is supported",
            2,
        ));
    }
    if reason.trim().is_empty() {
        return Err(error(
            OP,
            key,
            "reason_required",
            "Recovery requires a nonempty reason",
            2,
        ));
    }
    let mut prepared = exchange(
        connection,
        ResourceRequest::new(
            None,
            ResourceOperation::PrepareRecovery {
                resource_id: resource_id.clone(),
            },
        ),
        key,
        OP,
        false,
    )
    .await?;
    if prepared.catalog.revision != expected_revision {
        return Err(error(
            OP,
            key,
            "stale_revision",
            format!(
                "Current revision is {}; inspect the resource before retrying",
                prepared.catalog.revision
            ),
            2,
        ));
    }
    let owner = prepared
        .catalog
        .resources
        .get(&resource_id)
        .and_then(|r| r.reservations.iter().find(|q| q.state.occupied()))
        .cloned();
    let action = if owner.is_some() { "REVOKE" } else { "CLEAR" };
    eprintln!("Resource {} recovery: current owner {}. Workloads may still run. Revocation pauses the queue; clearance is a separate recovery invocation after physical safety is established.",resource_id,owner.as_ref().map_or_else(||"none".into(),|q|format!("{} ({})",q.agent.label,q.id)));
    eprint!("Type {action} {} to confirm: ", resource_id);
    std::io::stderr()
        .flush()
        .map_err(|e| error(OP, key, "confirmation_failed", e, 2))?;
    let mut confirmation = String::new();
    std::io::stdin()
        .read_line(&mut confirmation)
        .map_err(|e| error(OP, key, "confirmation_failed", e, 2))?;
    if confirmation.trim() != format!("{action} {resource_id}") {
        return Err(error(
            OP,
            key,
            "confirmation_declined",
            "Recovery was not performed",
            2,
        ));
    }
    let command = match owner {
        Some(q) => ResourceCommand::Revoke {
            reservation_id: q.id,
            reason,
        },
        None => ResourceCommand::ConfirmClear {
            resource_id,
            reason,
        },
    };
    let mut request = ResourceRequest::new(None, ResourceOperation::Mutate(command));
    request.expected_revision = Some(expected_revision);
    request.recovery_token = prepared.recovery_token.take();
    request.request_key = Some(key.into());
    let reply = exchange(connection, request, key, OP, false).await?;
    println!(
        "{}",
        result_json(&reply, OP, key, reply.reservation.as_ref())
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    #[test]
    fn reserve_forms_have_identical_meaning() {
        let short = reserve_input(&["gpu".into(), "1m30s".into()], Some("default/4")).unwrap();
        let long =
            reserve_input(&["default/4".into(), "gpu".into(), "1m30s".into()], None).unwrap();
        assert_eq!(short, long);
        assert_eq!(short.seconds, 90);
        assert!(reserve_input(&["4".into(), "gpu".into(), "1m".into()], Some("5")).is_err());
    }
    #[test]
    fn invalid_duration_and_arity_are_rejected() {
        for value in ["0s", "12", "8d", "1.5h", "999999999999999999999999h"] {
            assert!(reserve_input(&["gpu".into(), value.into()], None).is_err());
        }
        assert!(reserve_input(&["gpu".into()], None).is_err());
        assert!(reserve_input(&["gpu".into(), "1m".into(), "x".into(), "y".into()], None).is_err());
    }
    #[test]
    fn parser_accepts_alias_and_both_reserve_forms() {
        for command in ["resource", "ressource"] {
            for values in [vec!["gpu", "5m"], vec!["default/4", "gpu", "5m"]] {
                let mut arguments = vec!["ilium", command, "reserve"];
                arguments.extend(values);
                assert!(crate::Cli::try_parse_from(arguments).is_ok());
            }
        }
        assert!(crate::Cli::try_parse_from(["ilium", "resource", "reserve", "gpu"]).is_err());
        assert!(
            crate::Cli::try_parse_from(["ilium", "resource", "reserve", "a", "b", "c", "d"])
                .is_err()
        );
    }
    #[test]
    fn agent_references_do_not_guess_display_names() {
        assert_eq!(
            parse_agent("default/42").unwrap(),
            (Some("default".into()), NodeId(42))
        );
        assert!(parse_agent("Claude").is_err());
        assert!(parse_agent("0").is_err());
    }
    #[test]
    fn retry_keys_produce_same_reservation_identity() {
        assert_eq!(
            stable_id("reservation", "123:abc"),
            stable_id("reservation", "123:abc")
        );
        assert_ne!(
            stable_id("reservation", "123:abc"),
            stable_id("reservation", "123:def")
        );
    }
}
