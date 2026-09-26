//! Relays typed "spoken" sentences from `ilium voice say` to the one client
//! that hosts the voice session.
//!
//! The voice session (microphone, provider connection, tool executor) lives
//! in an attached TUI client, not in this server, so the server is only a
//! broker. A client that can host voice registers its connection
//! ([`VoiceTextRelay::register_receiver`]); a `SubmitVoiceText` request is
//! then offered to registered clients newest first, one at a time, and the
//! first that accepts wins. Offering to a single client at a time is what
//! keeps two attached TUIs from both acting on the same sentence, and keeps
//! `--start` from switching voice on in every one of them.
//!
//! Registration is explicit because every connection, the one-shot CLI
//! included, performs the same interactive attach handshake: the server
//! cannot otherwise tell a TUI from a short-lived command.

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use ilium_ipc::{
    normalize_voice_sentences, ServerEvent, VoiceTextRejection, VoiceTextRejectionCode,
    VoiceTextResult,
};
use tokio::sync::{mpsc, oneshot};

/// How long one client may take to answer an offer. Answering includes
/// starting the voice session when `start_voice` is set, which opens audio
/// devices and may pause media players over D-Bus.
const OFFER_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct VoiceTextRelay {
    /// Direct-reply channels of connections that host a voice session, in
    /// registration order (the newest is the most likely to be the TUI the
    /// user is looking at).
    receivers: Mutex<Vec<mpsc::Sender<ServerEvent>>>,
    /// Request ids currently being brokered, so a reused id cannot steal
    /// another request's answer.
    active_requests: Mutex<HashSet<u64>>,
    /// The answer channel of the offer currently outstanding for a request.
    waiters: Mutex<std::collections::HashMap<u64, oneshot::Sender<VoiceTextResult>>>,
    offer_timeout: Duration,
}

impl Default for VoiceTextRelay {
    fn default() -> Self {
        Self::with_offer_timeout(OFFER_TIMEOUT)
    }
}

/// Removes a request's bookkeeping however the brokering future ends,
/// including being dropped when the requesting connection goes away.
struct ActiveRequestGuard<'relay> {
    relay: &'relay VoiceTextRelay,
    request_id: u64,
}

impl Drop for ActiveRequestGuard<'_> {
    fn drop(&mut self) {
        lock(&self.relay.active_requests).remove(&self.request_id);
        lock(&self.relay.waiters).remove(&self.request_id);
    }
}

impl VoiceTextRelay {
    pub(crate) fn with_offer_timeout(offer_timeout: Duration) -> Self {
        Self {
            receivers: Mutex::new(Vec::new()),
            active_requests: Mutex::new(HashSet::new()),
            waiters: Mutex::new(std::collections::HashMap::new()),
            offer_timeout,
        }
    }

    /// Records a connection as able to host voice. Idempotent per connection;
    /// closed connections are dropped at the same time.
    pub(crate) fn register_receiver(&self, receiver: mpsc::Sender<ServerEvent>) {
        let mut receivers = lock(&self.receivers);
        receivers.retain(|existing| !existing.is_closed() && !existing.same_channel(&receiver));
        receivers.push(receiver);
    }

    /// Delivers a voice client's answer to the offer waiting for it. An
    /// answer with no waiting offer (it timed out, or the id is unknown) is
    /// dropped: silence is not consent and a late answer changes nothing.
    pub(crate) fn answer(&self, request_id: u64, result: VoiceTextResult) {
        let waiter = lock(&self.waiters).remove(&request_id);
        match waiter {
            Some(waiter) => {
                let _ = waiter.send(result);
            }
            None => tracing::debug!(request_id, "voice text answer without a waiting offer"),
        }
    }

    /// Brokers one request to completion. Never returns before a client has
    /// accepted, every client has declined, or the offers have timed out.
    pub(crate) async fn submit(
        &self,
        request_id: u64,
        sentences: Vec<String>,
        start_voice: bool,
    ) -> VoiceTextResult {
        let sentences = normalize_voice_sentences(sentences)?;
        if !lock(&self.active_requests).insert(request_id) {
            return Err(VoiceTextRejection::new(
                VoiceTextRejectionCode::InvalidRequest,
                format!("request id {request_id} is already in progress"),
            ));
        }
        let _guard = ActiveRequestGuard {
            relay: self,
            request_id,
        };

        let candidates = self.live_receivers_newest_first();
        if candidates.is_empty() {
            return Err(VoiceTextRejection::new(
                VoiceTextRejectionCode::NoVoiceClient,
                "no interactive Ilium client is attached to host the voice session; \
                 attach with `ilium` and try again",
            ));
        }

        // First pass: sessions that are already running. Nothing is switched
        // on, so a client that has voice enabled always wins over one that
        // would have to start it.
        let mut declined = Vec::new();
        for candidate in &candidates {
            match self.offer(candidate, request_id, &sentences, false).await {
                Ok(accepted) => return Ok(accepted),
                Err(rejection) => declined.push((candidate, rejection)),
            }
        }

        // Second pass, only on request: ask the newest client that reported
        // voice as off or failed to start it. One client only -- starting a
        // second microphone because the first failed would be a surprise.
        if start_voice {
            let restartable = declined.iter().find(|(_, rejection)| {
                matches!(
                    rejection.code,
                    VoiceTextRejectionCode::VoiceOff | VoiceTextRejectionCode::VoiceUnavailable
                )
            });
            if let Some((candidate, _)) = restartable {
                return self.offer(candidate, request_id, &sentences, true).await;
            }
        }

        Err(most_informative_rejection(
            declined.into_iter().map(|(_, rejection)| rejection),
        ))
    }

    fn live_receivers_newest_first(&self) -> Vec<mpsc::Sender<ServerEvent>> {
        let mut receivers = lock(&self.receivers);
        receivers.retain(|receiver| !receiver.is_closed());
        receivers.iter().rev().cloned().collect()
    }

    async fn offer(
        &self,
        receiver: &mpsc::Sender<ServerEvent>,
        request_id: u64,
        sentences: &[String],
        start_voice: bool,
    ) -> VoiceTextResult {
        let (answer_tx, answer_rx) = oneshot::channel();
        lock(&self.waiters).insert(request_id, answer_tx);
        let offered = receiver
            .send(ServerEvent::VoiceTextOffered {
                request_id,
                sentences: sentences.to_vec(),
                start_voice,
            })
            .await;
        if offered.is_err() {
            lock(&self.waiters).remove(&request_id);
            return Err(unresponsive("the client disconnected before it was asked"));
        }
        match tokio::time::timeout(self.offer_timeout, answer_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(unresponsive("the client dropped the request")),
            Err(_) => {
                lock(&self.waiters).remove(&request_id);
                Err(unresponsive(&format!(
                    "the client did not answer within {} seconds",
                    self.offer_timeout.as_secs_f32()
                )))
            }
        }
    }
}

fn unresponsive(detail: &str) -> VoiceTextRejection {
    VoiceTextRejection::new(VoiceTextRejectionCode::ClientUnresponsive, detail)
}

/// A failed voice session explains more than a switched-off one, which in
/// turn explains more than a silent client.
fn most_informative_rejection(
    rejections: impl Iterator<Item = VoiceTextRejection>,
) -> VoiceTextRejection {
    let rank = |code: VoiceTextRejectionCode| match code {
        VoiceTextRejectionCode::VoiceUnavailable => 0,
        VoiceTextRejectionCode::VoiceOff => 1,
        VoiceTextRejectionCode::ClientUnresponsive => 2,
        VoiceTextRejectionCode::InvalidRequest | VoiceTextRejectionCode::NoVoiceClient => 3,
    };
    rejections
        .min_by_key(|rejection| rank(rejection.code))
        .unwrap_or_else(|| {
            VoiceTextRejection::new(
                VoiceTextRejectionCode::NoVoiceClient,
                "no interactive Ilium client is attached to host the voice session",
            )
        })
}

/// The relay's mutexes guard plain collections that are never left
/// half-updated across a panic, so a poisoned lock is safe to keep using.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ilium_ipc::{VoiceTextAccepted, VoiceTextPhase};

    use super::*;

    fn accepted(started_voice: bool) -> VoiceTextResult {
        Ok(VoiceTextAccepted {
            sentence_count: 1,
            phase: VoiceTextPhase::Listening,
            started_voice,
        })
    }

    fn rejection(code: VoiceTextRejectionCode) -> VoiceTextResult {
        Err(VoiceTextRejection::new(code, "test"))
    }

    fn relay() -> Arc<VoiceTextRelay> {
        Arc::new(VoiceTextRelay::with_offer_timeout(Duration::from_millis(
            200,
        )))
    }

    /// One offer as a fake client saw it: `(sentences, start_voice)`.
    type RecordedOffer = (Vec<String>, bool);

    /// A registered fake client: answers each offer with what `reply` says
    /// and records every offer it saw as `(sentences, start_voice)`.
    struct FakeClient {
        offers: Arc<Mutex<Vec<RecordedOffer>>>,
        task: tokio::task::JoinHandle<()>,
    }

    fn register_client(
        relay: &Arc<VoiceTextRelay>,
        reply: impl Fn(bool) -> Option<VoiceTextResult> + Send + 'static,
    ) -> FakeClient {
        let (direct_tx, mut direct_rx) = mpsc::channel(8);
        relay.register_receiver(direct_tx);
        let offers = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&offers);
        let relay = Arc::clone(relay);
        let task = tokio::spawn(async move {
            while let Some(event) = direct_rx.recv().await {
                let ServerEvent::VoiceTextOffered {
                    request_id,
                    sentences,
                    start_voice,
                } = event
                else {
                    continue;
                };
                lock(&recorded).push((sentences, start_voice));
                if let Some(result) = reply(start_voice) {
                    relay.answer(request_id, result);
                }
            }
        });
        FakeClient { offers, task }
    }

    fn sentences() -> Vec<String> {
        vec!["open the settings".to_owned(), "close it".to_owned()]
    }

    #[tokio::test]
    async fn without_a_voice_client_the_request_is_rejected() {
        let relay = relay();
        let rejection = relay.submit(1, sentences(), false).await.unwrap_err();
        assert_eq!(rejection.code, VoiceTextRejectionCode::NoVoiceClient);
    }

    #[tokio::test]
    async fn a_running_session_receives_the_normalized_sentences() {
        let relay = relay();
        let client = register_client(&relay, |_| Some(accepted(false)));
        let result = relay
            .submit(2, vec!["  hello ".to_owned(), "world".to_owned()], false)
            .await;
        assert!(matches!(result, Ok(ref value) if value.sentence_count == 1));
        assert_eq!(
            *lock(&client.offers),
            [(vec!["hello".to_owned(), "world".to_owned()], false)]
        );
        client.task.abort();
    }

    #[tokio::test]
    async fn a_client_with_voice_running_wins_over_a_newer_one_that_is_off() {
        let relay = relay();
        let running = register_client(&relay, |_| Some(accepted(false)));
        let off = register_client(&relay, |_| {
            Some(rejection(VoiceTextRejectionCode::VoiceOff))
        });

        // `--start` must not switch the newer client on while the older one
        // already hosts a running session.
        relay.submit(3, sentences(), true).await.expect("accepted");
        assert_eq!(*lock(&off.offers), [(sentences(), false)]);
        assert_eq!(*lock(&running.offers), [(sentences(), false)]);
        running.task.abort();
        off.task.abort();
    }

    #[tokio::test]
    async fn start_is_offered_only_when_asked_for_and_only_to_one_client() {
        let relay = relay();
        let older = register_client(&relay, |_| {
            Some(rejection(VoiceTextRejectionCode::VoiceOff))
        });
        let newer = register_client(&relay, |start| {
            if start {
                Some(accepted(true))
            } else {
                Some(rejection(VoiceTextRejectionCode::VoiceOff))
            }
        });

        let refused = relay.submit(4, sentences(), false).await.unwrap_err();
        assert_eq!(refused.code, VoiceTextRejectionCode::VoiceOff);
        assert_eq!(*lock(&newer.offers), [(sentences(), false)]);

        let started = relay.submit(5, sentences(), true).await.expect("started");
        assert!(started.started_voice);
        // Newest client: first pass declined, then asked to start. The older
        // client only ever heard the non-starting offers.
        assert_eq!(
            *lock(&newer.offers),
            [
                (sentences(), false),
                (sentences(), false),
                (sentences(), true)
            ]
        );
        assert!(lock(&older.offers).iter().all(|(_, start)| !start));
        older.task.abort();
        newer.task.abort();
    }

    #[tokio::test]
    async fn a_failed_voice_session_is_reported_over_a_switched_off_one() {
        let relay = relay();
        let failed = register_client(&relay, |_| {
            Some(Err(VoiceTextRejection::new(
                VoiceTextRejectionCode::VoiceUnavailable,
                "OpenAI API key must not be empty",
            )))
        });
        let off = register_client(&relay, |_| {
            Some(rejection(VoiceTextRejectionCode::VoiceOff))
        });
        let rejection = relay.submit(6, sentences(), false).await.unwrap_err();
        assert_eq!(rejection.code, VoiceTextRejectionCode::VoiceUnavailable);
        assert!(rejection.message.contains("API key"));
        failed.task.abort();
        off.task.abort();
    }

    #[tokio::test]
    async fn a_silent_client_times_out_instead_of_hanging_the_request() {
        let relay = relay();
        let silent = register_client(&relay, |_| None);
        let rejection = relay.submit(7, sentences(), false).await.unwrap_err();
        assert_eq!(rejection.code, VoiceTextRejectionCode::ClientUnresponsive);
        // The late answer of a timed-out offer must not resurrect anything.
        relay.answer(7, accepted(false));
        silent.task.abort();
    }

    #[tokio::test]
    async fn malformed_requests_are_rejected_before_any_client_is_asked() {
        let relay = relay();
        let client = register_client(&relay, |_| Some(accepted(false)));
        let rejection = relay
            .submit(8, vec![" ".to_owned()], false)
            .await
            .unwrap_err();
        assert_eq!(rejection.code, VoiceTextRejectionCode::InvalidRequest);
        assert!(lock(&client.offers).is_empty());
        client.task.abort();
    }

    #[tokio::test]
    async fn a_request_id_cannot_be_reused_while_it_is_in_flight() {
        let relay = relay();
        let slow = register_client(&relay, |_| None);
        let first = {
            let relay = Arc::clone(&relay);
            tokio::spawn(async move { relay.submit(9, vec!["one".to_owned()], false).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        let duplicate = relay
            .submit(9, vec!["two".to_owned()], false)
            .await
            .unwrap_err();
        assert_eq!(duplicate.code, VoiceTextRejectionCode::InvalidRequest);
        assert!(first.await.expect("first request").is_err());
        // The id is free again once the first request finished.
        assert!(!lock(&relay.active_requests).contains(&9));
        slow.task.abort();
    }

    #[tokio::test]
    async fn disconnected_clients_are_forgotten() {
        let relay = relay();
        let (direct_tx, direct_rx) = mpsc::channel(1);
        relay.register_receiver(direct_tx);
        drop(direct_rx);
        let rejection = relay.submit(10, sentences(), false).await.unwrap_err();
        assert_eq!(rejection.code, VoiceTextRejectionCode::NoVoiceClient);
        assert!(lock(&relay.receivers).is_empty());
    }

    #[tokio::test]
    async fn registering_the_same_connection_twice_keeps_one_entry() {
        let relay = relay();
        let (direct_tx, _direct_rx) = mpsc::channel(1);
        relay.register_receiver(direct_tx.clone());
        relay.register_receiver(direct_tx);
        assert_eq!(lock(&relay.receivers).len(), 1);
    }
}
