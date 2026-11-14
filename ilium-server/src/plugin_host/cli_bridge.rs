//! The server side of plugin CLI commands (api-cli, capability-cli-contribute).
//!
//! `ilium <name>` connects to the session, sends `CliInvoke` and reads `CliOutput` and
//! `CliExit`. The bridge starts the command on the runtime thread (`HostRuntime::start_cli`),
//! and a forwarding task per invocation turns what the plugin printed into those envelopes on
//! the invoking connection only. The runtime thread never waits for a socket: it pushes into
//! an unbounded channel the task drains, so a slow CLI cannot stall plugins.
//!
//! A CLI that goes away (its connection closes) or sends `CliCancel` ends the invocation: the
//! plugin gets `cli-cancelled` and the route is dropped, so nothing it writes afterwards
//! reaches anybody.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ilium_ipc::plugin::{CliStream, ControlBody, EnvelopeBody, PluginEnvelope};
use ilium_ipc::ServerEvent;
use ilium_plugin_host::imports::cli::CliChunk;
use ilium_plugin_host::runtime::cli::{CliStart, CliStartError};
use tokio::sync::{mpsc, oneshot};

use super::{log_errors, PluginHostHandle, Publisher, RuntimeSlot, SUBMISSION_WAIT};
use crate::ipc::DirectEventSender;

/// Exit code when the command line was wrong (unknown command, bad pane selector).
const EXIT_USAGE: i32 = 2;
/// Exit code when the command could not run.
const EXIT_FAILED: i32 = 1;

/// Where the output of one running invocation goes.
struct Route {
    /// The number the CLI chose; the answers quote it.
    wire_id: u64,
    /// The connection that invoked the command.
    origin: DirectEventSender,
    /// Feeds the forwarding task.
    sender: mpsc::UnboundedSender<CliChunk>,
}

/// The running invocations by the runtime's invocation id. Shared between the publisher (on the
/// runtime thread, which routes chunks) and the connection tasks.
#[derive(Default)]
pub(super) struct CliRoutes {
    routes: Mutex<HashMap<u64, Route>>,
}

impl CliRoutes {
    fn register(&self, invocation: u64, route: Route) {
        if let Ok(mut routes) = self.routes.lock() {
            routes.insert(invocation, route);
        }
    }

    fn forget(&self, invocation: u64) {
        if let Ok(mut routes) = self.routes.lock() {
            routes.remove(&invocation);
        }
    }

    /// The runtime invocation a connection started under its own number.
    fn find(&self, wire_id: u64, origin: &DirectEventSender) -> Option<u64> {
        let routes = self.routes.lock().ok()?;
        routes
            .iter()
            .find(|(_, route)| route.wire_id == wire_id && route.origin.same_channel(origin))
            .map(|(invocation, _)| *invocation)
    }

    /// Hands one chunk of plugin output to the task of its invocation. The exit chunk also ends
    /// the route, so the task finishes after it. A chunk for an invocation nobody listens to
    /// any more is dropped.
    pub(super) fn route(&self, chunk: CliChunk) {
        let Ok(mut routes) = self.routes.lock() else {
            return;
        };
        let (invocation, last) = match &chunk {
            CliChunk::Stdout { invocation, .. } | CliChunk::Stderr { invocation, .. } => {
                (*invocation, false)
            }
            CliChunk::Exit { invocation, .. } => (*invocation, true),
        };
        let delivered = routes
            .get(&invocation)
            .is_some_and(|route| route.sender.send(chunk).is_ok());
        if last || !delivered {
            routes.remove(&invocation);
        }
    }
}

/// What a CLI asked for with `CliInvoke`.
pub(super) struct CliInvocation {
    /// The number the CLI chose for this invocation.
    pub wire_id: u64,
    /// The plugin that owns the command.
    pub plugin: String,
    /// The command name.
    pub command: String,
    /// The words after the name.
    pub args: Vec<String>,
    /// Bytes for standard input.
    pub stdin: Vec<u8>,
    /// The CLI's working directory.
    pub cwd: PathBuf,
}

/// Ends one invocation from outside the plugin: the route goes first so no output follows,
/// then the plugin is told on the runtime thread.
struct Canceller {
    routes: Arc<CliRoutes>,
    runtime: RuntimeSlot,
    publisher: Publisher,
    invocation: u64,
}

impl Canceller {
    fn cancel(&self) {
        self.routes.forget(self.invocation);
        let invocation = self.invocation;
        let publisher = self.publisher.clone();
        let posted = self.runtime.lock().ok().is_some_and(|slot| {
            slot.as_ref().is_some_and(|thread| {
                thread.post(move |runtime| {
                    if let Err(error) = runtime.cancel_cli(invocation) {
                        tracing::debug!("cli cancel not delivered: {error}");
                    }
                    log_errors("pump", runtime.pump());
                    publisher.flush(runtime);
                })
            })
        });
        if !posted {
            tracing::debug!("cli cancel dropped: the plugin host stopped");
        }
    }
}

fn control_event(next_envelope: &AtomicU64, body: ControlBody) -> ServerEvent {
    ServerEvent::Plugin(PluginEnvelope {
        plugin: String::new(),
        seq: next_envelope.fetch_add(1, Ordering::Relaxed),
        body: EnvelopeBody::Control(body),
    })
}

/// The wire form of one chunk and whether it ends the invocation.
fn wire_of(wire_id: u64, chunk: CliChunk) -> (ControlBody, bool) {
    match chunk {
        CliChunk::Stdout { data, .. } => (
            ControlBody::CliOutput {
                invocation_id: wire_id,
                stream: CliStream::Stdout,
                data,
            },
            false,
        ),
        CliChunk::Stderr { data, .. } => (
            ControlBody::CliOutput {
                invocation_id: wire_id,
                stream: CliStream::Stderr,
                data,
            },
            false,
        ),
        CliChunk::Exit { code, .. } => (
            ControlBody::CliExit {
                invocation_id: wire_id,
                code,
            },
            true,
        ),
    }
}

/// Forwards the chunks of one invocation to its connection until the exit chunk, the end of the
/// route or the loss of the connection.
async fn forward(
    wire_id: u64,
    mut chunks: mpsc::UnboundedReceiver<CliChunk>,
    origin: DirectEventSender,
    next_envelope: Arc<AtomicU64>,
    canceller: Canceller,
) {
    loop {
        tokio::select! {
            chunk = chunks.recv() => {
                let Some(chunk) = chunk else {
                    return;
                };
                let (body, last) = wire_of(wire_id, chunk);
                if origin.send(control_event(&next_envelope, body)).await.is_err() {
                    canceller.cancel();
                    return;
                }
                if last {
                    return;
                }
            }
            () = origin.closed() => {
                canceller.cancel();
                return;
            }
        }
    }
}

impl PluginHostHandle {
    /// Answers a command that could not start: the message on standard error, then the exit.
    async fn cli_refuse(
        &self,
        wire_id: u64,
        code: i32,
        message: &str,
        direct_tx: &DirectEventSender,
    ) {
        let error = control_event(
            &self.publisher.next_envelope,
            ControlBody::CliOutput {
                invocation_id: wire_id,
                stream: CliStream::Stderr,
                data: format!("{message}\n").into_bytes(),
            },
        );
        let exit = control_event(
            &self.publisher.next_envelope,
            ControlBody::CliExit {
                invocation_id: wire_id,
                code,
            },
        );
        for event in [error, exit] {
            if direct_tx.send(event).await.is_err() {
                return;
            }
        }
    }

    /// Starts a plugin command for a connection and spawns the task that streams its output.
    pub(super) async fn cli_invoke(&self, invoke: CliInvocation, direct_tx: &DirectEventSender) {
        let wire_id = invoke.wire_id;
        if self.cli.find(wire_id, direct_tx).is_some() {
            self.cli_refuse(
                wire_id,
                EXIT_USAGE,
                "ilium: this invocation number is already running",
                direct_tx,
            )
            .await;
            return;
        }
        let (chunks, receiver) = mpsc::unbounded_channel();
        let (started, outcome) = oneshot::channel::<Result<u64, CliStartError>>();
        let routes = Arc::clone(&self.cli);
        let origin = direct_tx.clone();
        let start = CliStart {
            plugin: invoke.plugin,
            command: invoke.command,
            args: invoke.args,
            stdin: invoke.stdin,
            cwd: invoke.cwd.to_string_lossy().into_owned(),
            ..CliStart::default()
        };
        let posted = self.post(move |runtime| {
            let result = runtime.start_cli(start);
            // Registered inside the job that started the command: the flush that follows it
            // already finds the route, so no early output is lost.
            if let Ok(invocation) = &result {
                routes.register(
                    *invocation,
                    Route {
                        wire_id,
                        origin,
                        sender: chunks,
                    },
                );
            }
            log_errors("pump", runtime.pump());
            // The CLI may have gone away already; its reply channel is then closed.
            let _ = started.send(result);
        });
        if !posted {
            self.cli_refuse(
                wire_id,
                EXIT_FAILED,
                "ilium: the plugin host is stopping",
                direct_tx,
            )
            .await;
            return;
        }
        let invocation = match tokio::time::timeout(SUBMISSION_WAIT, outcome).await {
            Ok(Ok(Ok(invocation))) => invocation,
            Ok(Ok(Err(error))) => {
                let code = match error {
                    CliStartError::Unknown(_) | CliStartError::Usage(_) => EXIT_USAGE,
                    CliStartError::Runtime(_) => EXIT_FAILED,
                };
                self.cli_refuse(wire_id, code, &format!("ilium: {error}"), direct_tx)
                    .await;
                return;
            }
            Ok(Err(_)) | Err(_) => {
                self.cli_refuse(
                    wire_id,
                    EXIT_FAILED,
                    "ilium: the plugin host did not start the command",
                    direct_tx,
                )
                .await;
                return;
            }
        };
        let canceller = Canceller {
            routes: Arc::clone(&self.cli),
            runtime: Arc::clone(&self.runtime),
            publisher: self.publisher.clone(),
            invocation,
        };
        let task = forward(
            wire_id,
            receiver,
            direct_tx.clone(),
            Arc::clone(&self.publisher.next_envelope),
            canceller,
        );
        if let Ok(mut tasks) = self.cli_tasks.lock() {
            // Finished forwarders are reaped here so the set only holds live invocations.
            while tasks.try_join_next().is_some() {}
            tasks.spawn(task);
        }
    }

    /// The CLI asked to stop a running command. Nothing happens for an unknown number.
    pub(super) fn cli_cancel(&self, wire_id: u64, direct_tx: &DirectEventSender) {
        let Some(invocation) = self.cli.find(wire_id, direct_tx) else {
            return;
        };
        Canceller {
            routes: Arc::clone(&self.cli),
            runtime: Arc::clone(&self.runtime),
            publisher: self.publisher.clone(),
            invocation,
        }
        .cancel();
    }

    /// Ends every forwarding task; called when the host stops.
    pub(super) fn cli_stop(&self) {
        if let Ok(mut tasks) = self.cli_tasks.lock() {
            tasks.abort_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route_for(invocation: u64) -> (CliRoutes, mpsc::UnboundedReceiver<CliChunk>) {
        let routes = CliRoutes::default();
        let (origin, _receiver) = DirectEventSender::channel(4);
        let (sender, receiver) = mpsc::unbounded_channel();
        routes.register(
            invocation,
            Route {
                wire_id: 7,
                origin,
                sender,
            },
        );
        (routes, receiver)
    }

    /// REQ: API-005
    #[test]
    fn chunks_reach_their_route_in_order_and_the_exit_ends_it() {
        let (routes, mut receiver) = route_for(3);
        routes.route(CliChunk::Stdout {
            invocation: 3,
            data: b"a".to_vec(),
        });
        routes.route(CliChunk::Stderr {
            invocation: 3,
            data: b"b".to_vec(),
        });
        routes.route(CliChunk::Exit {
            invocation: 3,
            code: 4,
        });
        // After the exit the route is gone, so later chunks have nowhere to go.
        routes.route(CliChunk::Stdout {
            invocation: 3,
            data: b"late".to_vec(),
        });
        let mut seen = Vec::new();
        while let Ok(chunk) = receiver.try_recv() {
            seen.push(chunk);
        }
        assert_eq!(
            seen,
            vec![
                CliChunk::Stdout {
                    invocation: 3,
                    data: b"a".to_vec()
                },
                CliChunk::Stderr {
                    invocation: 3,
                    data: b"b".to_vec()
                },
                CliChunk::Exit {
                    invocation: 3,
                    code: 4
                },
            ]
        );
    }

    /// REQ: API-005
    #[test]
    fn a_chunk_for_a_forgotten_invocation_is_dropped() {
        let (routes, mut receiver) = route_for(3);
        routes.forget(3);
        routes.route(CliChunk::Stdout {
            invocation: 3,
            data: b"x".to_vec(),
        });
        routes.route(CliChunk::Stdout {
            invocation: 99,
            data: b"y".to_vec(),
        });
        assert!(receiver.try_recv().is_err());
    }

    /// REQ: API-005
    #[test]
    fn an_invocation_is_found_by_its_number_on_its_own_connection_only() {
        let routes = CliRoutes::default();
        let (first, _first_receiver) = DirectEventSender::channel(4);
        let (second, _second_receiver) = DirectEventSender::channel(4);
        let (sender, _chunks) = mpsc::unbounded_channel();
        routes.register(
            5,
            Route {
                wire_id: 1,
                origin: first.clone(),
                sender,
            },
        );
        assert_eq!(routes.find(1, &first), Some(5));
        assert_eq!(
            routes.find(1, &second),
            None,
            "another connection has its own numbers"
        );
        assert_eq!(routes.find(2, &first), None);
    }

    /// REQ: API-005
    #[test]
    fn the_wire_form_of_a_chunk_quotes_the_cli_number() {
        let (body, last) = wire_of(
            9,
            CliChunk::Stderr {
                invocation: 1,
                data: b"e".to_vec(),
            },
        );
        assert_eq!(
            body,
            ControlBody::CliOutput {
                invocation_id: 9,
                stream: CliStream::Stderr,
                data: b"e".to_vec()
            }
        );
        assert!(!last);
        let (body, last) = wire_of(
            9,
            CliChunk::Exit {
                invocation: 1,
                code: 130,
            },
        );
        assert_eq!(
            body,
            ControlBody::CliExit {
                invocation_id: 9,
                code: 130
            }
        );
        assert!(last);
    }
}
