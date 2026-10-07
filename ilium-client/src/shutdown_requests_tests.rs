//! Actual admitted request slots, sender, CPU codec and FrameWriter lifecycle.
use super::*;
use crate::shutdown_requests::{DrainCause, FlushStatus, ShutdownRequests};
use std::{
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};
use tokio::io::AsyncWrite;

struct Bank(Option<ilium_execution::Execution>);
impl Bank {
    fn start() -> (Self, crate::ipc_preparation::IpcPreparation) {
        let (owner, codec) =
            crate::ipc_preparation::IpcPreparation::standalone().expect("actual codec bank");
        (Self(Some(owner)), codec)
    }
}
impl Drop for Bank {
    fn drop(&mut self) {
        let mut owner = self.0.take().expect("actual bank");
        owner.request_shutdown(ilium_execution::ShutdownMode::Cancel);
        let joined = owner
            .join_until_background(Instant::now() + Duration::from_secs(5))
            .expect("physical observation");
        assert!(
            joined.shutdown_complete,
            "native fixture bank still owns work: {joined:?}"
        );
    }
}
fn batch(client: &ilium_execution::Client) -> Vec<crate::ipc_preparation::AdmittedRequest> {
    ["actual-original-head", "actual-original-tail"]
        .into_iter()
        .map(|session| {
            crate::ipc_preparation::admit_request(
                client,
                ClientRequest::AttachInteractive {
                    session: session.to_owned(),
                },
            )
            .expect("actual input admission")
        })
        .collect()
}
fn identities(batch: &[crate::ipc_preparation::AdmittedRequest]) -> Vec<(usize, usize)> {
    batch
        .iter()
        .map(|request| {
            let ClientRequest::AttachInteractive { session } = request.view() else {
                panic!("fixture original");
            };
            // Same outbound enum slot (including opaque hold) and authored backing.
            (request as *const _ as usize, session.as_ptr() as usize)
        })
        .collect()
}
fn sender(
    codec: &crate::ipc_preparation::IpcPreparation,
    capacity: usize,
) -> (RequestSender, mpsc::Receiver<RequestCommand>) {
    let (sender, received) = mpsc::channel(capacity);
    (
        RequestSender {
            sender,
            admission: codec.outbound_client(),
        },
        received,
    )
}

#[tokio::test]
async fn cancelled_empty_batch_retains_original_flush_receipt_for_cleanup() {
    let (_bank, codec) = Bank::start();
    let (sender, mut received) = sender(&codec, 1);
    let mut custody = ShutdownRequests::default();
    // Observe the actual FIFO barrier before canceling the borrowed finish
    // future. No timing assumption or request payload is needed to reach it.
    let acknowledgement = {
        let finish = custody.finish(&sender);
        tokio::pin!(finish);
        tokio::select! {
            command = received.recv() => match command.expect("original queued barrier") {
                RequestCommand::Flush(acknowledgement) => acknowledgement,
                RequestCommand::Request(_) => panic!("empty batch published request bytes"),
            },
            result = &mut finish => panic!("unacknowledged barrier completed: {result:?}"),
        }
    };
    assert_eq!(custody.accepted_prefix(), 0);
    assert_eq!(custody.flush_status(), FlushStatus::Pending);
    assert!(
        custody.has_unresolved_publication(),
        "final cleanup must retain the actual unresolved receipt even with zero new requests"
    );
    acknowledgement
        .send(())
        .expect("same original receiver retained");
    custody.finish(&sender).await.expect("observe same receipt");
    assert_eq!(custody.flush_status(), FlushStatus::Confirmed);
    assert!(!custody.has_unresolved_publication());
    assert!(matches!(
        received.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn closed_empty_batch_retains_original_flush_failure_for_cleanup() {
    let (_bank, codec) = Bank::start();
    let (sender, mut received) = sender(&codec, 1);
    let mut custody = ShutdownRequests::default();
    let acknowledgement = {
        let finish = custody.finish(&sender);
        tokio::pin!(finish);
        tokio::select! {
            command = received.recv() => match command.expect("original queued barrier") {
                RequestCommand::Flush(acknowledgement) => acknowledgement,
                RequestCommand::Request(_) => panic!("empty batch published request bytes"),
            },
            result = &mut finish => panic!("unacknowledged barrier completed: {result:?}"),
        }
    };
    drop(acknowledgement);
    assert_eq!(
        custody.finish(&sender).await,
        Err(DrainCause::FlushReceiptClosed)
    );
    assert_eq!(custody.accepted_prefix(), 0);
    assert_eq!(custody.flush_status(), FlushStatus::ReceiptClosed);
    assert!(
        custody.has_unresolved_publication(),
        "final cleanup must retain the original closed receipt even with zero new requests"
    );
    let error = custody.into_failure(DrainCause::FlushReceiptClosed);
    error.inspect(|custody| assert_eq!(custody.flush_status(), FlushStatus::ReceiptClosed));
    assert!(matches!(
        received.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn closed_writer_preserves_original_head_tail_and_guards() {
    let (_bank, codec) = Bank::start();
    let (sender, received) = sender(&codec, 1);
    drop(received);
    let original = batch(&sender.admission);
    let identity = identities(&original);
    let usage = sender.admission.usage();
    let mut custody = ShutdownRequests::from_batch(original);
    assert_eq!(
        custody.publish(&sender).await,
        Err(DrainCause::WriterClosed)
    );
    assert_eq!(
        identities(custody.pending_originals()),
        identity,
        "original iterator head/tail or SAME opaque guard slot was discarded"
    );
    assert_eq!(
        sender.admission.usage(),
        usage,
        "original admission released on refusal"
    );
    let error = custody.into_failure(DrainCause::WriterClosed);
    error.inspect(|custody| assert_eq!(identities(custody.pending_originals()), identity));
    let additional = batch(&sender.admission);
    let additional_identity = identities(&additional);
    let combined_usage = sender.admission.usage();
    let error = error.with_cleanup(
        additional,
        Some(crate::error::ClientError::TerminalSetup(
            std::io::Error::other("original terminal cleanup failure"),
        )),
    );
    let error = std::io::Error::other(error);
    let typed = error
        .get_ref()
        .and_then(|source| source.downcast_ref::<crate::shutdown_requests::ShutdownRequestsError>())
        .expect("original failure custody survives the public error wrapper");
    typed.inspect(|custody| assert_eq!(identities(custody.pending_originals()), identity));
    assert_eq!(
        identities(typed.additional_originals()),
        additional_identity
    );
    assert_eq!(
        sender.admission.usage(),
        combined_usage,
        "cleanup wrapping released an unpublished original's admission"
    );
    assert!(typed.to_string().contains("4 original requests retained"));
    assert!(std::error::Error::source(typed)
        .expect("previous cleanup error remains observable")
        .to_string()
        .contains("original terminal cleanup failure"));
    // Rejected later batches must be returned intact before the caller moves
    // them into the combined failure; never replace the earlier pending tail.
    let mut overlap = ShutdownRequests::from_batch(batch(&sender.admission));
    let later = batch(&sender.admission);
    let later_identity = identities(&later);
    let refused = overlap
        .adopt_batch(later)
        .expect_err("earlier tail is owned");
    assert_eq!(identities(&refused), later_identity);
    drop(refused);
    drop(overlap);
    drop(error);
}
#[tokio::test]
async fn full_queue_cancellation_preserves_actual_iterator_and_guard_debits() {
    let (_bank, codec) = Bank::start();
    let (sender, mut received) = sender(&codec, 1);
    sender
        .send(ClientRequest::UpdateDebugLogging { enabled: true })
        .await
        .expect("occupy actual queue");
    let original = batch(&sender.admission);
    let identity = identities(&original);
    let usage = sender.admission.usage();
    let mut custody = ShutdownRequests::from_batch(original);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), custody.publish(&sender))
            .await
            .is_err()
    );
    assert_eq!(
        identities(custody.pending_originals()),
        identity,
        "timeout destroyed an accepted semantic original"
    );
    assert_eq!(sender.admission.usage(), usage);
    drop(received.recv().await.expect("original occupied queue"));
    assert!(
        tokio::time::timeout(Duration::from_millis(20), custody.publish(&sender))
            .await
            .is_err()
    );
    assert_eq!(
        identities(custody.pending_originals()),
        identity[1..],
        "partial queue publication must keep the exact original tail"
    );
    assert_eq!(custody.accepted_prefix(), 1);
    drop(received);
    assert_eq!(
        custody.publish(&sender).await,
        Err(DrainCause::WriterClosed)
    );
    assert_eq!(identities(custody.pending_originals()), identity[1..]);
    drop(custody);
}
#[derive(Default)]
struct StreamState {
    bytes: Vec<u8>,
    released: bool,
    fail: bool,
    waker: Option<Waker>,
}
struct Stream {
    state: Arc<Mutex<StreamState>>,
    entered: Arc<tokio::sync::Notify>,
}
impl AsyncWrite for Stream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.state
            .lock()
            .expect("stream")
            .bytes
            .extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        let mut state = self.state.lock().expect("stream");
        if state.fail {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "native fixture stream flush failure",
            )));
        }
        if state.released {
            return Poll::Ready(Ok(()));
        }
        state.waker = Some(cx.waker().clone());
        self.entered.notify_one();
        Poll::Pending
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.poll_flush(cx)
    }
}
fn release(state: &Mutex<StreamState>, fail: bool) {
    let wake = {
        let mut state = state.lock().expect("stream");
        state.released = true;
        state.fail = fail;
        state.waker.take()
    };
    if let Some(wake) = wake {
        wake.wake();
    }
}
#[tokio::test]
async fn cancelled_flush_keeps_same_receipt_and_wire_order_without_replay() {
    let (_bank, codec) = Bank::start();
    let (sender, received) = sender(&codec, 4);
    let state = Arc::new(Mutex::new(StreamState::default()));
    let entered = Arc::new(tokio::sync::Notify::new());
    let writer = tokio::spawn(write_loop(
        Stream {
            state: state.clone(),
            entered: entered.clone(),
        },
        received,
        codec,
    ));
    let mut custody = ShutdownRequests::from_batch(batch(&sender.admission));
    assert!(
        tokio::time::timeout(Duration::from_millis(40), custody.finish(&sender))
            .await
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .expect("actual native CPU encoding reached stream flush");
    assert_eq!(custody.flush_status(), FlushStatus::Pending);
    assert_eq!(custody.accepted_prefix(), 2);
    release(&state, false);
    tokio::time::timeout(Duration::from_secs(5), custody.finish(&sender))
        .await
        .expect("actual stored receipt")
        .expect("physical stream flush");
    assert_eq!(custody.flush_status(), FlushStatus::Confirmed);
    assert_eq!(custody.accepted_prefix(), 2);
    let bytes = state.lock().expect("stream").bytes.clone();
    let mut reader = FrameReader::new(bytes.as_slice());
    for expected in ["actual-original-head", "actual-original-tail"] {
        let actual: ClientRequest = reader.read().await.expect("actual ordered wire frame");
        assert!(matches!(actual,ClientRequest::AttachInteractive{session} if session==expected));
    }
    assert!(
        matches!(reader.read::<ClientRequest>().await,
            Err(IpcError::Io(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof),
        "cancelled barrier replayed an accepted prefix"
    );
    drop(custody);
    drop(sender);
    writer.await.expect("actual writer physical task exit");
}

#[tokio::test]
async fn failed_stream_retains_closed_receipt_and_truthful_prefix_uncertainty() {
    let (_bank, codec) = Bank::start();
    let (sender, received) = sender(&codec, 4);
    let state = Arc::new(Mutex::new(StreamState::default()));
    let entered = Arc::new(tokio::sync::Notify::new());
    let writer = tokio::spawn(write_loop(
        Stream {
            state: state.clone(),
            entered: entered.clone(),
        },
        received,
        codec,
    ));
    let mut custody = ShutdownRequests::from_batch(batch(&sender.admission));
    assert!(
        tokio::time::timeout(Duration::from_millis(40), custody.finish(&sender))
            .await
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .expect("flush reached");
    release(&state, true);
    writer.await.expect("writer failure physically exits");
    assert_eq!(
        custody.finish(&sender).await,
        Err(DrainCause::FlushReceiptClosed)
    );
    assert_eq!(custody.flush_status(), FlushStatus::ReceiptClosed);
    assert_eq!(custody.accepted_prefix(), 2);
    assert_eq!(
        custody.finish(&sender).await,
        Err(DrainCause::FlushReceiptClosed)
    );
    assert_eq!(
        custody.accepted_prefix(),
        2,
        "closed receipt must not enqueue original prefix again"
    );
    let error = custody.into_failure(DrainCause::FlushReceiptClosed);
    assert!(error.to_string().contains("server acceptance unknown"));
    drop(error);
    drop(sender);
}
