//! Unit tests for the public surface of `ilium-transport`: endpoint naming,
//! the bind, connect, probe and cleanup decisions, accept-error
//! classification, error messages, and the stream halves.
//!
//! Async behaviour runs on a small current-thread runtime inside plain
//! `#[test]` functions. The permission-based cases (read-only and
//! unsearchable directories, mode-000 sockets) assume the test process is not
//! root, because root bypasses the kernel checks they depend on.

use std::error::Error as _;
use std::future::Future;
use std::io::{self, IoSlice};
use std::time::Duration;

use ilium_transport::{
    is_transient_accept_error, Liveness, SessionEndpoint, SessionStream, TransportError,
};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build the test runtime")
        .block_on(future)
}

fn temp_endpoint(name: &str) -> (tempfile::TempDir, SessionEndpoint) {
    let directory = tempfile::tempdir().expect("create temp dir");
    let endpoint = SessionEndpoint::from_path(directory.path().join(name));
    (directory, endpoint)
}

async fn connected_pair(endpoint: &SessionEndpoint) -> (SessionStream, SessionStream) {
    let mut listener = endpoint.bind().await.expect("bind test endpoint");
    let (client, accepted) = tokio::join!(endpoint.connect(), listener.accept());
    (
        client.expect("connect client"),
        accepted.expect("accept client"),
    )
}

fn assert_static_and_send<T: Send + 'static>(_value: &T) {}

#[cfg(unix)]
fn set_mode(path: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .expect("set permissions on test path");
}

/// Leaves behind what a crashed server leaves: a socket file with no listener.
#[cfg(unix)]
fn leave_stale_socket(endpoint: &SessionEndpoint) {
    let listener = std::os::unix::net::UnixListener::bind(endpoint.as_path())
        .expect("bind a socket to leave behind");
    drop(listener);
}

#[cfg(unix)]
#[test]
fn display_returns_the_socket_path_unchanged() {
    let endpoint = SessionEndpoint::from_path("/run/user/1000/ilium/session.sock");

    assert_eq!(endpoint.display(), "/run/user/1000/ilium/session.sock");
}

#[cfg(unix)]
#[test]
fn display_of_a_non_utf8_path_is_lossy_and_keeps_readable_parts() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let raw = OsStr::from_bytes(b"/tmp/session-\xff.sock");
    let displayed = SessionEndpoint::from_path(raw).display();

    assert!(displayed.starts_with("/tmp/session-"), "got {displayed:?}");
    assert!(displayed.ends_with(".sock"), "got {displayed:?}");
}

#[cfg(unix)]
#[test]
fn display_of_an_empty_path_is_an_empty_string() {
    assert_eq!(SessionEndpoint::from_path("").display(), "");
}

#[test]
fn bind_with_a_missing_parent_directory_returns_bind_error_naming_endpoint() {
    let directory = tempfile::tempdir().expect("create temp dir");
    let endpoint = SessionEndpoint::from_path(directory.path().join("missing").join("s.sock"));
    let expected_display = endpoint.display();

    let error = block_on(endpoint.bind()).err().expect("bind must fail");

    let TransportError::Bind {
        endpoint: named,
        source,
    } = error
    else {
        panic!("expected a Bind error for a missing parent directory");
    };
    assert_eq!(named, expected_display);
    assert_eq!(source.kind(), io::ErrorKind::NotFound);
}

#[test]
fn bind_with_a_path_longer_than_the_socket_address_limit_returns_bind_error() {
    let directory = tempfile::tempdir().expect("create temp dir");
    let long_name = format!("{}.sock", "a".repeat(120));
    let endpoint = SessionEndpoint::from_path(directory.path().join(long_name));

    let error = block_on(endpoint.bind()).err().expect("bind must fail");

    assert!(
        matches!(error, TransportError::Bind { .. }),
        "expected a Bind error, got {error}"
    );
}

#[test]
fn bind_over_a_live_server_fails_and_keeps_the_live_listener_reachable() {
    let (_directory, endpoint) = temp_endpoint("live.sock");

    block_on(async {
        let _live = endpoint.bind().await.expect("first bind");

        let second = endpoint.bind().await;

        assert!(
            second.is_err(),
            "a second bind must not take over a live session"
        );
        assert_eq!(endpoint.probe_liveness(), Liveness::Live);
        endpoint
            .connect()
            .await
            .expect("the live listener still accepts connections");
    });
}

#[cfg(unix)]
#[test]
fn bind_over_stale_debris_in_a_read_only_directory_reports_clear_stale() {
    let (directory, endpoint) = temp_endpoint("stale.sock");
    leave_stale_socket(&endpoint);
    set_mode(directory.path(), 0o555);

    let error = block_on(endpoint.bind()).err().expect("bind must fail");

    set_mode(directory.path(), 0o755);
    assert!(
        matches!(error, TransportError::ClearStale { .. }),
        "expected a ClearStale error, got {error}"
    );
    assert!(
        endpoint.as_path().exists(),
        "the stale socket must not be removed when unlink is refused"
    );
}

#[cfg(unix)]
#[test]
fn remove_stale_in_an_unwritable_directory_errors_and_keeps_the_file() {
    let (directory, endpoint) = temp_endpoint("debris.sock");
    leave_stale_socket(&endpoint);
    set_mode(directory.path(), 0o555);

    let result = endpoint.remove_stale();

    set_mode(directory.path(), 0o755);
    assert!(result.is_err(), "unlink in a read-only directory must fail");
    assert!(
        endpoint.as_path().exists(),
        "debris must remain after a failed removal"
    );
}

#[test]
fn connect_with_no_listener_and_no_file_returns_connect_error() {
    let (_directory, endpoint) = temp_endpoint("nothing.sock");

    let error = block_on(endpoint.connect())
        .err()
        .expect("connect must fail");

    let TransportError::Connect {
        endpoint: named,
        source,
    } = error
    else {
        panic!("expected a Connect error when nothing exists at the endpoint");
    };
    assert_eq!(named, endpoint.display());
    assert_eq!(source.kind(), io::ErrorKind::NotFound);
}

#[cfg(unix)]
#[test]
fn connect_to_a_stale_socket_returns_connect_error_with_refused_source() {
    let (_directory, endpoint) = temp_endpoint("stale-connect.sock");
    leave_stale_socket(&endpoint);

    let error = block_on(endpoint.connect())
        .err()
        .expect("connect must fail");

    let TransportError::Connect { source, .. } = error else {
        panic!("expected a Connect error for a stale socket");
    };
    assert_eq!(source.kind(), io::ErrorKind::ConnectionRefused);
}

#[cfg(unix)]
#[test]
fn connect_to_a_regular_file_is_a_connect_error_not_a_live_session() {
    let (_directory, endpoint) = temp_endpoint("plain.sock");
    std::fs::write(endpoint.as_path(), b"not a socket").expect("write a plain file");

    let error = block_on(endpoint.connect())
        .err()
        .expect("connect must fail");

    assert!(
        matches!(error, TransportError::Connect { .. }),
        "expected a Connect error, got {error}"
    );
    assert_eq!(endpoint.probe_liveness(), Liveness::StaleListener);
}

#[test]
fn connect_error_message_names_the_endpoint() {
    let (_directory, endpoint) = temp_endpoint("message.sock");

    let message = block_on(endpoint.connect())
        .err()
        .expect("connect must fail")
        .to_string();

    assert!(
        message.starts_with("failed to connect to the session endpoint "),
        "got {message}"
    );
    assert!(message.contains(&endpoint.display()), "got {message}");
}

#[test]
fn remove_stale_on_an_absent_path_succeeds() {
    let (_directory, endpoint) = temp_endpoint("absent.sock");

    endpoint
        .remove_stale()
        .expect("nothing to remove is not an error");

    assert_eq!(endpoint.probe_liveness(), Liveness::Absent);
}

#[cfg(unix)]
#[test]
fn remove_stale_deletes_a_regular_file_at_the_endpoint() {
    let (_directory, endpoint) = temp_endpoint("regular.sock");
    std::fs::write(endpoint.as_path(), b"debris").expect("write a regular file");

    endpoint.remove_stale().expect("remove debris");

    assert!(!endpoint.as_path().exists());
}

#[cfg(unix)]
#[test]
fn remove_stale_deletes_a_socket_left_by_a_dead_server() {
    let (_directory, endpoint) = temp_endpoint("dead-server.sock");
    leave_stale_socket(&endpoint);
    assert_eq!(endpoint.probe_liveness(), Liveness::StaleListener);

    endpoint.remove_stale().expect("remove stale socket");

    assert!(!endpoint.as_path().exists());
    assert_eq!(endpoint.probe_liveness(), Liveness::Absent);
}

#[cfg(unix)]
#[test]
fn remove_stale_on_a_directory_errors_and_keeps_the_directory() {
    let (_directory, endpoint) = temp_endpoint("a-directory");
    std::fs::create_dir(endpoint.as_path()).expect("create a directory at the endpoint");

    let result = endpoint.remove_stale();

    assert!(result.is_err(), "remove_file must refuse a directory");
    assert!(
        endpoint.as_path().is_dir(),
        "the directory must not be removed"
    );
}

#[cfg(unix)]
#[test]
fn probe_with_an_unsearchable_parent_is_unreachable_not_absent() {
    let (directory, endpoint) = temp_endpoint("hidden.sock");
    leave_stale_socket(&endpoint);
    set_mode(directory.path(), 0o000);

    let liveness = endpoint.probe_liveness();

    set_mode(directory.path(), 0o755);
    assert_eq!(liveness, Liveness::Unreachable);
}

#[cfg(unix)]
#[test]
fn probe_through_a_symlink_loop_is_unreachable() {
    let directory = tempfile::tempdir().expect("create temp dir");
    let loop_link = directory.path().join("loop");
    std::os::unix::fs::symlink(&loop_link, &loop_link).expect("create a self-referencing link");
    let endpoint = SessionEndpoint::from_path(loop_link.join("session.sock"));

    assert_eq!(endpoint.probe_liveness(), Liveness::Unreachable);
}

#[cfg(unix)]
#[test]
fn bind_through_a_symlink_loop_reports_bind_and_never_clears_debris() {
    let directory = tempfile::tempdir().expect("create temp dir");
    let loop_link = directory.path().join("loop");
    std::os::unix::fs::symlink(&loop_link, &loop_link).expect("create a self-referencing link");
    let endpoint = SessionEndpoint::from_path(loop_link.join("session.sock"));

    let error = block_on(endpoint.bind()).err().expect("bind must fail");

    assert!(
        matches!(error, TransportError::Bind { .. }),
        "an unreachable endpoint must never be cleared, got {error}"
    );
}

#[cfg(unix)]
#[test]
fn socket_that_cannot_be_opened_is_unreachable_and_bind_keeps_it() {
    let (_directory, endpoint) = temp_endpoint("sealed.sock");
    leave_stale_socket(&endpoint);
    set_mode(endpoint.as_path(), 0o000);

    let liveness = endpoint.probe_liveness();
    let bind_error = block_on(endpoint.bind()).err();

    assert_eq!(liveness, Liveness::Unreachable);
    assert!(
        matches!(bind_error, Some(TransportError::Bind { .. })),
        "expected a Bind error, got {bind_error:?}"
    );
    assert!(
        endpoint.as_path().exists(),
        "the socket must not be deleted by a bind that could not reach it"
    );
}

#[cfg(unix)]
#[test]
fn dangling_symlink_at_the_endpoint_is_debris_that_bind_replaces() {
    let (directory, endpoint) = temp_endpoint("dangling.sock");
    std::os::unix::fs::symlink(directory.path().join("nowhere"), endpoint.as_path())
        .expect("create a dangling symlink");
    assert_eq!(endpoint.probe_liveness(), Liveness::StaleListener);

    block_on(async {
        let result = endpoint.bind().await;

        assert!(result.is_ok(), "bind must replace the dangling symlink");
    });
}

#[test]
fn accept_with_no_pending_client_keeps_waiting() {
    let (_directory, endpoint) = temp_endpoint("waiting.sock");

    block_on(async {
        let mut listener = endpoint.bind().await.expect("bind");

        let waited = tokio::time::timeout(Duration::from_millis(100), listener.accept()).await;

        assert!(waited.is_err(), "accept returned with no client connected");
    });
}

#[cfg(unix)]
#[test]
fn accept_returns_a_stream_for_a_client_that_disconnected_before_accept() {
    let (_directory, endpoint) = temp_endpoint("early-hangup.sock");

    block_on(async {
        let mut listener = endpoint.bind().await.expect("bind");
        drop(endpoint.connect().await.expect("connect"));

        let mut accepted = listener.accept().await.expect("accept the queued client");
        let mut buffer = [0u8; 4];
        let read = accepted.read(&mut buffer).await.expect("read");

        assert_eq!(read, 0, "a hung-up client must read as end of stream");
    });
}

#[test]
fn transient_accept_kinds_are_retried() {
    let transient_kinds = [
        io::ErrorKind::ConnectionAborted,
        io::ErrorKind::ConnectionReset,
        io::ErrorKind::Interrupted,
        io::ErrorKind::WouldBlock,
    ];

    for kind in transient_kinds {
        let error = io::Error::new(kind, "transient");
        assert!(
            is_transient_accept_error(&error),
            "{kind:?} must be classified as transient"
        );
    }
}

#[test]
fn fatal_accept_kinds_are_not_retried() {
    let fatal_kinds = [
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::InvalidInput,
        io::ErrorKind::BrokenPipe,
        io::ErrorKind::NotConnected,
    ];

    for kind in fatal_kinds {
        let error = io::Error::new(kind, "fatal");
        assert!(
            !is_transient_accept_error(&error),
            "{kind:?} must not be retried, or the accept loop would spin"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn raw_os_accept_errors_are_classified_like_their_kinds() {
    // Linux errno values: ECONNABORTED 103, ECONNRESET 104, EINTR 4 and
    // EAGAIN 11 describe a per-connection blip; EBADF 9, EACCES 13 and
    // EMFILE 24 say the listener or this process cannot continue.
    for transient_code in [103, 104, 4, 11] {
        let error = io::Error::from_raw_os_error(transient_code);
        assert!(
            is_transient_accept_error(&error),
            "errno {transient_code} ({error}) must be transient"
        );
    }
    for fatal_code in [9, 13, 24] {
        let error = io::Error::from_raw_os_error(fatal_code);
        assert!(
            !is_transient_accept_error(&error),
            "errno {fatal_code} ({error}) must not be transient"
        );
    }
}

#[test]
fn bind_error_message_is_exact_and_contains_the_source_text() {
    let error = TransportError::Bind {
        endpoint: "/run/x.sock".to_string(),
        source: io::Error::new(io::ErrorKind::AddrInUse, "address in use"),
    };

    assert_eq!(
        error.to_string(),
        "failed to bind the session endpoint /run/x.sock: address in use"
    );
}

#[test]
fn connect_error_message_is_exact_and_contains_the_source_text() {
    let error = TransportError::Connect {
        endpoint: "/run/x.sock".to_string(),
        source: io::Error::new(io::ErrorKind::ConnectionRefused, "refused"),
    };

    assert_eq!(
        error.to_string(),
        "failed to connect to the session endpoint /run/x.sock: refused"
    );
}

#[test]
fn clear_stale_message_is_distinct_from_bind_message() {
    let clear = TransportError::ClearStale {
        endpoint: "/run/x.sock".to_string(),
        source: io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
    };
    let bind = TransportError::Bind {
        endpoint: "/run/x.sock".to_string(),
        source: io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
    };

    assert_ne!(clear.to_string(), bind.to_string());
    assert_eq!(
        clear.to_string(),
        "failed to clear the stale session endpoint /run/x.sock before binding: denied"
    );
}

#[test]
fn transport_error_source_is_the_wrapped_io_error_for_every_variant() {
    let errors = [
        TransportError::Bind {
            endpoint: "a".to_string(),
            source: io::Error::new(io::ErrorKind::Other, "boom"),
        },
        TransportError::Connect {
            endpoint: "a".to_string(),
            source: io::Error::new(io::ErrorKind::Other, "boom"),
        },
        TransportError::ClearStale {
            endpoint: "a".to_string(),
            source: io::Error::new(io::ErrorKind::Other, "boom"),
        },
    ];

    for error in errors {
        let source = error
            .source()
            .expect("every variant must expose its io::Error as the source");
        assert_eq!(source.to_string(), "boom");
    }
}

#[test]
fn split_halves_are_owned_and_move_into_spawned_tasks() {
    let (_directory, endpoint) = temp_endpoint("split.sock");

    block_on(async {
        let (client, mut accepted) = connected_pair(&endpoint).await;
        let (_read_half, write_half) = client.into_split();
        assert_static_and_send(&write_half);

        let writer = tokio::spawn(async move {
            let mut write_half = write_half;
            write_half
                .write_all(b"spawn")
                .await
                .expect("write from the spawned task");
        });
        let mut received = [0u8; 5];
        accepted.read_exact(&mut received).await.expect("read");
        writer.await.expect("writer task");

        assert_eq!(&received, b"spawn");
    });
}

#[test]
fn bytes_written_on_the_write_half_are_read_by_the_peer() {
    let (_directory, endpoint) = temp_endpoint("halves.sock");

    block_on(async {
        let (client, mut accepted) = connected_pair(&endpoint).await;
        let (_read_half, mut write_half) = client.into_split();

        write_half.write_all(b"hello").await.expect("write");
        let mut received = [0u8; 5];
        accepted.read_exact(&mut received).await.expect("read");

        assert_eq!(&received, b"hello");
    });
}

#[test]
fn dropping_the_read_half_leaves_the_write_half_usable() {
    let (_directory, endpoint) = temp_endpoint("drop-half.sock");

    block_on(async {
        let (client, mut accepted) = connected_pair(&endpoint).await;
        let (read_half, mut write_half) = client.into_split();
        drop(read_half);

        write_half
            .write_all(b"after")
            .await
            .expect("write after drop");
        let mut received = [0u8; 5];
        accepted.read_exact(&mut received).await.expect("read");

        assert_eq!(&received, b"after");
    });
}

#[test]
fn shutdown_on_the_write_half_gives_the_peer_end_of_stream() {
    let (_directory, endpoint) = temp_endpoint("half-shutdown.sock");

    block_on(async {
        let (client, mut accepted) = connected_pair(&endpoint).await;
        let (_read_half, mut write_half) = client.into_split();

        write_half.shutdown().await.expect("shutdown");
        let mut buffer = [0u8; 8];
        let read = accepted.read(&mut buffer).await.expect("read");

        assert_eq!(read, 0, "the peer must observe end of stream");
    });
}

#[cfg(target_os = "linux")]
#[test]
fn peer_process_id_names_this_process_on_both_ends() {
    let (_directory, endpoint) = temp_endpoint("peer-pid.sock");

    block_on(async {
        let (client, accepted) = connected_pair(&endpoint).await;

        assert_eq!(client.peer_process_id(), Some(std::process::id()));
        assert_eq!(accepted.peer_process_id(), Some(std::process::id()));
    });
}

#[test]
fn is_write_vectored_reports_the_inner_stream_capability() {
    let (_directory, endpoint) = temp_endpoint("vectored-flag.sock");

    block_on(async {
        let (client, _accepted) = connected_pair(&endpoint).await;

        assert!(
            client.is_write_vectored(),
            "the wrapper must forward the stream's vectored support, not the default false"
        );
    });
}

#[test]
fn write_vectored_with_several_slices_delivers_bytes_in_order() {
    let (_directory, endpoint) = temp_endpoint("vectored-order.sock");
    let all = b"alpha-beta-gamma";

    block_on(async {
        let (mut client, mut accepted) = connected_pair(&endpoint).await;
        let slices = [
            IoSlice::new(b"alpha-"),
            IoSlice::new(b"beta-"),
            IoSlice::new(b"gamma"),
        ];

        let written = client
            .write_vectored(&slices)
            .await
            .expect("vectored write");
        assert!(written > 0, "the vectored write must make progress");
        client
            .write_all(&all[written..])
            .await
            .expect("write the remainder");
        let mut received = vec![0u8; all.len()];
        accepted.read_exact(&mut received).await.expect("read");

        assert_eq!(&received[..], &all[..]);
    });
}

#[test]
fn vectored_write_after_the_peer_closed_errors() {
    let (_directory, endpoint) = temp_endpoint("vectored-closed.sock");

    block_on(async {
        let (mut client, accepted) = connected_pair(&endpoint).await;
        drop(accepted);
        let slices = [IoSlice::new(b"x"), IoSlice::new(b"y")];

        let error = client
            .write_vectored(&slices)
            .await
            .expect_err("a write to a closed peer must fail");

        assert!(
            matches!(
                error.kind(),
                io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
            ),
            "got {:?}",
            error.kind()
        );
    });
}

#[test]
fn flush_delivers_written_bytes_to_the_peer() {
    let (_directory, endpoint) = temp_endpoint("flush.sock");

    block_on(async {
        let (mut client, mut accepted) = connected_pair(&endpoint).await;

        client.write_all(b"flushed").await.expect("write");
        client.flush().await.expect("flush");
        let mut received = [0u8; 7];
        accepted.read_exact(&mut received).await.expect("read");

        assert_eq!(&received, b"flushed");
    });
}

#[test]
fn shutdown_twice_keeps_the_peer_at_end_of_stream_without_panicking() {
    let (_directory, endpoint) = temp_endpoint("double-shutdown.sock");

    block_on(async {
        let (mut client, mut accepted) = connected_pair(&endpoint).await;

        client.shutdown().await.expect("first shutdown");
        let second = client.shutdown().await;

        // The second call is platform-defined: it may succeed as a no-op or
        // report that the socket is no longer connected. Either is acceptable.
        assert!(
            second.is_ok()
                || second
                    .as_ref()
                    .is_err_and(|error| error.kind() == io::ErrorKind::NotConnected),
            "unexpected result from a second shutdown: {second:?}"
        );
        let mut buffer = [0u8; 4];
        let read = accepted.read(&mut buffer).await.expect("read");
        assert_eq!(read, 0);
    });
}

#[test]
fn read_returns_zero_after_the_peer_closed() {
    let (_directory, endpoint) = temp_endpoint("peer-closed.sock");

    block_on(async {
        let (client, mut accepted) = connected_pair(&endpoint).await;
        drop(client);
        let mut buffer = [0u8; 4];

        let read = accepted.read(&mut buffer).await.expect("read");

        assert_eq!(read, 0);
    });
}

#[test]
fn read_into_a_zero_capacity_buffer_returns_without_blocking() {
    let (_directory, endpoint) = temp_endpoint("zero-read.sock");

    block_on(async {
        let (mut client, mut accepted) = connected_pair(&endpoint).await;
        client.write_all(b"data").await.expect("write");
        client.flush().await.expect("flush");
        let mut empty: [u8; 0] = [];

        let read = tokio::time::timeout(Duration::from_secs(1), accepted.read(&mut empty))
            .await
            .expect("a zero-length read must not block")
            .expect("read");

        assert_eq!(read, 0);
    });
}
