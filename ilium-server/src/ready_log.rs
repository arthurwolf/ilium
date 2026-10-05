//! The one startup marker write uses the existing finite I/O bank.
use crate::{error::ServerError, execution::ExecutionClient, ReadyLogMetadata};
use ilium_execution::{JobCost, Lane, Retention, StorageAdmission};
use std::{fmt, io, path::PathBuf, sync::Arc};

#[derive(Debug)]
struct NativeFailure {
    path: PathBuf,
    source: io::Error,
}
impl fmt::Display for NativeFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path.display(), self.source)
    }
}

/// The path in ServerError is destroyed before this source releases its charge.
#[derive(Debug)]
struct ChargedError {
    source: io::Error,
    _result: Option<Retention>,
    _capture: Option<Arc<StorageAdmission>>,
}
impl fmt::Display for ChargedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.source, formatter)
    }
}
impl std::error::Error for ChargedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

pub(crate) async fn publish(
    client: &ExecutionClient,
    metadata: ReadyLogMetadata,
) -> Result<(), ServerError> {
    publish_with(client, metadata, write_native).await
}

async fn publish_with(
    client: &ExecutionClient,
    metadata: ReadyLogMetadata,
    write: impl FnOnce(&ReadyLogMetadata) -> Result<(), NativeFailure> + Send + 'static,
) -> Result<(), ServerError> {
    // Account for the two originals, temporary/error path copies, formatted
    // marker and backing growth in this finite native operation. This is a
    // cooperative allocation declaration, not a kernel/RSS limit.
    let Some(input_bytes) = metadata
        .active_log_path_file
        .capacity()
        .checked_add(metadata.log_path.capacity())
        .and_then(|bytes| bytes.checked_mul(16))
        .and_then(|bytes| bytes.checked_add(16 * 1024))
    else {
        return Err(ServerError::ReadyLogMetadata {
            path: metadata.active_log_path_file,
            source: io::Error::other("ready-log allocation declaration overflow"),
        });
    };
    let Some(result_bytes) = metadata
        .active_log_path_file
        .capacity()
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(4096))
    else {
        return Err(ServerError::ReadyLogMetadata {
            path: metadata.active_log_path_file,
            source: io::Error::other("ready-log result declaration overflow"),
        });
    };
    // Infrastructure failures need the exact original target even when a
    // cancelled/panicked job cannot return its input. Admit this sole fallback
    // copy before cloning, and retain it through the final error consumer.
    let capture = match client.try_reserve_storage(result_bytes) {
        Ok(storage) => storage,
        Err(reason) => {
            return Err(ServerError::ReadyLogMetadata {
                path: metadata.active_log_path_file,
                source: io::Error::other(format!("ready-log capture admission: {reason:?}")),
            });
        }
    };
    let reservation = match client
        .reserve(
            Lane::Io,
            JobCost {
                input_bytes,
                result_bytes,
            },
        )
        .await
    {
        Ok(reservation) => reservation,
        Err(reason) => {
            return Err(ServerError::ReadyLogMetadata {
                path: metadata.active_log_path_file,
                source: io::Error::other(ChargedError {
                    source: io::Error::other(format!("ready-log execution admission: {reason:?}")),
                    _result: None,
                    _capture: Some(capture),
                }),
            });
        }
    };
    let fallback_path = metadata.active_log_path_file.clone();
    let outcome = client
        .run_reserved(reservation, move |_context| write(&metadata))
        .await;
    match outcome {
        Ok(completion) => {
            drop(fallback_path);
            drop(capture);
            drop(completion);
            Ok(())
        }
        Err(crate::execution::ExecutionError::Failed(failure)) => {
            drop(fallback_path);
            drop(capture);
            let (failure, retention) = failure.into_parts();
            Err(ServerError::ReadyLogMetadata {
                path: failure.path,
                source: io::Error::new(
                    failure.source.kind(),
                    ChargedError {
                        source: failure.source,
                        _result: Some(retention),
                        _capture: None,
                    },
                ),
            })
        }
        Err(error) => Err(ServerError::ReadyLogMetadata {
            path: fallback_path,
            source: io::Error::other(ChargedError {
                source: io::Error::other(format!("ready-log worker failed: {error}")),
                _result: None,
                _capture: Some(capture),
            }),
        }),
    }
}

/// Atomically records the exact log file for a server that has already bound
/// its listener. The temporary file lives beside the final marker so `rename`
/// is atomic; clients therefore observe either the previous healthy server's
/// marker or the newly ready server's marker, never a partial path.
fn write_native(metadata: &ReadyLogMetadata) -> Result<(), NativeFailure> {
    let temporary_path = metadata
        .active_log_path_file
        .with_extension(format!("ready-{}", std::process::id()));
    let log_path = metadata.log_path.to_str().ok_or_else(|| NativeFailure {
        path: metadata.active_log_path_file.clone(),
        source: std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "ready log path is not valid UTF-8",
        ),
    })?;
    let contents = format!("pid={}\nlog_path={log_path}\n", std::process::id());
    // Mirrors `persistence::save_snapshot`'s atomic-write convention: a stale
    // temp file left by a crashed predecessor that happened to reuse this pid
    // is cleared so `create_new` can succeed, and the temp file is created
    // owner-only from the start (`private_open_options`: mode 0600 plus
    // `O_NOFOLLOW`/`O_CLOEXEC` on Unix). The previous write-then-chmod
    // sequence here left a window where the file was world-readable and would
    // have written through a symlink pre-planted at this predictable path.
    let _ = std::fs::remove_file(&temporary_path);
    let write_result = ilium_platform::secure_fs::private_open_options()
        .write(true)
        .create_new(true)
        .open(&temporary_path)
        // The file handle is dropped inside this closure, before the rename
        // below runs -- required on Windows, where renaming a still-open file
        // fails.
        .and_then(|mut temporary_file| {
            std::io::Write::write_all(&mut temporary_file, contents.as_bytes())
        })
        .and_then(|()| std::fs::rename(&temporary_path, &metadata.active_log_path_file));
    if let Err(source) = write_result {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(NativeFailure {
            path: metadata.active_log_path_file.clone(),
            source,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::ServerExecution;
    use std::{sync::mpsc, time::Duration};

    #[tokio::test]
    async fn ready_marker_write_runs_on_bank_and_preserves_exact_readback() {
        let directory = tempfile::tempdir().expect("fixture directory");
        let metadata = ReadyLogMetadata {
            active_log_path_file: directory.path().join("active"),
            log_path: directory.path().join("server.log"),
        };
        let active = metadata.active_log_path_file.clone();
        let expected = format!(
            "pid={}\nlog_path={}\n",
            std::process::id(),
            metadata.log_path.display()
        );
        let owner = ServerExecution::start().expect("real bank");
        let caller = std::thread::current().id();
        let (observed, worker) = tokio::sync::oneshot::channel();
        publish_with(&owner.client, metadata, move |metadata| {
            observed.send(std::thread::current().id()).unwrap();
            write_native(metadata)
        })
        .await
        .expect("publish");
        assert_ne!(worker.await.unwrap(), caller);
        assert_eq!(std::fs::read_to_string(active).unwrap(), expected);
        owner.request_shutdown();
    }

    #[tokio::test]
    async fn cancelling_waiter_keeps_blocked_native_write_admitted_until_return() {
        let directory = tempfile::tempdir().expect("fixture directory");
        let metadata = ReadyLogMetadata {
            active_log_path_file: directory.path().join("active"),
            log_path: directory.path().join("server.log"),
        };
        let active = metadata.active_log_path_file.clone();
        let owner = ServerExecution::start().expect("real bank");
        let quota = owner.quota_group();
        let baseline = quota.snapshot().jobs;
        let client = owner.client.clone();
        let completed = client.completion_notification();
        let (entered, entry) = tokio::sync::oneshot::channel();
        let (release, blocked) = mpsc::sync_channel(1);
        let waiter = tokio::spawn(async move {
            publish_with(&client, metadata, move |metadata| {
                entered.send(()).unwrap();
                blocked.recv().unwrap();
                write_native(metadata)
            })
            .await
        });
        tokio::time::timeout(Duration::from_secs(5), entry)
            .await
            .expect("native entry")
            .unwrap();
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        assert_eq!(quota.snapshot().jobs, baseline + 1);
        assert!(!active.exists());
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let notified = completed.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if quota.snapshot().jobs == baseline {
                    break;
                }
                notified.await;
            }
        })
        .await
        .expect("physical callback retirement");
        assert!(active.exists(), "cancelled waiter does not imply rollback");
        owner.request_shutdown();
    }

    #[tokio::test]
    async fn native_failure_keeps_error_path_charge_and_previous_marker() {
        let directory = tempfile::tempdir().expect("fixture directory");
        let metadata = ReadyLogMetadata {
            active_log_path_file: directory.path().join("active"),
            log_path: directory.path().join("server.log"),
        };
        let active = metadata.active_log_path_file.clone();
        std::fs::write(&active, b"previous marker").unwrap();
        let owner = ServerExecution::start().expect("real bank");
        let quota = owner.quota_group();
        let baseline = quota.snapshot().jobs;
        let error = publish_with(&owner.client, metadata, |metadata| {
            Err(NativeFailure {
                path: metadata.active_log_path_file.clone(),
                source: io::Error::new(io::ErrorKind::PermissionDenied, "injected refusal"),
            })
        })
        .await
        .unwrap_err();
        assert!(matches!(
            &error,
            ServerError::ReadyLogMetadata { path, source }
                if path == &active && source.kind() == io::ErrorKind::PermissionDenied
        ));
        assert_eq!(std::fs::read(&active).unwrap(), b"previous marker");
        assert_eq!(quota.snapshot().jobs, baseline + 1);
        drop(error);
        assert_eq!(quota.snapshot().jobs, baseline);
        owner.request_shutdown();
    }
}
