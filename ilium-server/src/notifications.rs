//! The desktop notifier behind the `notify` plugin intent. The kernel decides nothing about
//! which events deserve a notice: the Notifications plugin (`ilium.core.notifications`) owns
//! the alert rules and texts. This module only shows one finished notice. [`PendingNotification`]
//! owns the presentation contract and [`send_admitted`] is the thin I/O adapter around `notify-rust`
//! that actually shows one.

/// A notice a plugin asked for, queued until the notifier lane takes it. A slow or unavailable
/// notification daemon must never hold up a plugin's turn or a client's tree access.
pub struct PendingNotification {
    session_name: String,
    plugin: String,
    title: String,
    body: Option<String>,
    /// A core plugin composes its whole text (it names the session and the pane itself), so
    /// the body is shown as given; any other plugin's body is followed by the sender line.
    verbatim: bool,
}

impl PendingNotification {
    /// A plugin's `notify` intent. `plugin` names the sender in the body of a third-party
    /// notice so a user can always tell a plugin alert from a kernel alert; `is_core` shows the
    /// body of a core plugin verbatim.
    pub fn for_plugin(
        session_name: String,
        plugin: String,
        title: String,
        body: Option<String>,
        is_core: bool,
    ) -> Self {
        Self {
            session_name,
            plugin,
            title,
            body,
            verbatim: is_core,
        }
    }

    /// Notification summary shown by the desktop shell.
    fn summary(&self) -> String {
        self.title.clone()
    }

    /// Notification body.
    fn body(&self) -> String {
        if self.verbatim {
            return self.body.clone().unwrap_or_default();
        }
        let sender = format!(
            "Session \"{}\": from plugin {}.",
            self.session_name, self.plugin
        );
        match &self.body {
            Some(body) if !body.is_empty() => format!("{body}\n\n{sender}"),
            _ => sender,
        }
    }
}

/// Shows a desktop notification for `pending`. `notify-rust`'s `show()` is
/// synchronous, blocking I/O (a D-Bus round trip via `zbus` on Linux), so
/// the actual call runs on the server's bounded I/O lane rather than inline
/// on a tokio worker thread. The returned
/// `NotificationHandle` is dropped inside the closure (not returned out of
/// it): on macOS's default `NSUserNotificationCenter` backend, `show()`
/// only stages the notification and the actual OS delivery call happens in
/// the handle's `Drop` impl, so `deliver` drops the handle before returning
/// from the I/O-lane callback.
///
/// Reports admission: `false` when the bounded I/O lane refused the job (the
/// text is too large or the lane is full). A delivery failure is never
/// propagated: no notification daemon/D-Bus session (this sandboxed
/// environment, most containers, some window managers) is a normal, expected
/// condition, not a bug -- logged and continued. Deliberately not unit tested
/// against a real daemon, which is exactly the environment-dependent
/// flakiness the injected `deliver` closure of [`send_with`] keeps out of the
/// test suite.
pub(crate) fn send_admitted(
    client: &crate::execution::ExecutionClient,
    pending: PendingNotification,
) -> bool {
    send_with(client, pending, deliver)
}

fn deliver(summary: &str, body: &str) -> Result<(), &'static str> {
    notify_rust::Notification::new()
        .summary(summary)
        .body(body)
        .show()
        .map(drop)
        .map_err(|error| {
            tracing::warn!(%error, "desktop notification backend rejected delivery");
            "desktop notification backend rejected delivery"
        })
}

fn send_with(
    client: &crate::execution::ExecutionClient,
    pending: PendingNotification,
    deliver: impl FnOnce(&str, &str) -> Result<(), &'static str> + Send + 'static,
) -> bool {
    let input_bytes = match pending
        .retained_text_bytes()
        .checked_mul(6)
        .and_then(|bytes| bytes.checked_add(2048))
    {
        Some(bytes) if bytes <= MAX_NOTIFICATION_JOB_BYTES => bytes,
        _ => {
            tracing::warn!(
                "desktop notification refused because its text exceeds the bounded job size"
            );
            return false;
        }
    };
    let job = notification_job(pending, deliver);
    let reservation = match client.foundation.try_reserve(
        ilium_execution::Lane::Io,
        ilium_execution::JobCost {
            input_bytes,
            result_bytes: 0,
        },
    ) {
        Ok(reservation) => reservation,
        Err(error) => {
            tracing::warn!(?error, "desktop notification was not admitted (continuing)");
            return false;
        }
    };
    let client = client.clone();
    tokio::spawn(async move {
        match client.run_reserved(reservation, job).await {
            Ok(result) => {
                if let Err(error) = result.view() {
                    tracing::warn!(%error, "desktop notification failed (no notification daemon? continuing)");
                }
            }
            Err(error) => tracing::warn!(?error, "desktop notification worker failed (continuing)"),
        }
    });
    true
}

/// An unavailable desktop backend is a delivery result, while the execution
/// bank reports cancellation, panic, and lost completion separately.
fn notification_job(
    pending: PendingNotification,
    deliver: impl FnOnce(&str, &str) -> Result<(), &'static str> + Send + 'static,
) -> impl ilium_execution::Job<Output = Result<(), &'static str>, Error = std::convert::Infallible>
{
    move |_context: ilium_execution::JobContext| Ok(deliver(&pending.summary(), &pending.body()))
}

const MAX_NOTIFICATION_JOB_BYTES: usize = 64 * 1024;

impl PendingNotification {
    fn retained_text_bytes(&self) -> usize {
        self.session_name
            .capacity()
            .saturating_add(self.plugin.capacity())
            .saturating_add(self.title.capacity())
            .saturating_add(self.body.as_ref().map_or(0, String::capacity))
            .saturating_add(std::mem::size_of::<Self>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A core plugin's notice, with a short text.
    fn core_notice(title: &str) -> PendingNotification {
        PendingNotification::for_plugin(
            "session".into(),
            "ilium.core.notifications".into(),
            title.into(),
            Some("body".into()),
            true,
        )
    }

    #[test]
    fn a_plugin_notification_keeps_its_title_and_names_the_plugin() {
        let pending = PendingNotification::for_plugin(
            "work".into(),
            "acme.alerts".into(),
            "Build failed".into(),
            Some("3 tests failed".into()),
            false,
        );
        assert_eq!(pending.summary(), "Build failed");
        assert_eq!(
            pending.body(),
            "3 tests failed\n\nSession \"work\": from plugin acme.alerts."
        );
        let bare = PendingNotification::for_plugin(
            "work".into(),
            "acme.alerts".into(),
            "Ping".into(),
            None,
            false,
        );
        assert_eq!(bare.body(), "Session \"work\": from plugin acme.alerts.");
        assert!(bare.retained_text_bytes() > "Ping".len());
    }

    #[test]
    fn a_core_plugin_notification_shows_its_text_verbatim() {
        let pending = PendingNotification::for_plugin(
            "work".into(),
            "ilium.core.notifications".into(),
            "claude finished".into(),
            Some("Session \"work\": the agent in \"claude\" is done and waiting on you.".into()),
            true,
        );
        assert_eq!(pending.summary(), "claude finished");
        assert_eq!(
            pending.body(),
            "Session \"work\": the agent in \"claude\" is done and waiting on you."
        );
        let bare = PendingNotification::for_plugin(
            "work".into(),
            "ilium.core.notifications".into(),
            "Ping".into(),
            None,
            true,
        );
        assert_eq!(bare.body(), "");
    }

    #[tokio::test]
    async fn backend_refusal_remains_a_delivery_result() {
        let execution = crate::execution::ServerExecution::start().expect("execution bank");
        let reservation = execution
            .client
            .foundation
            .try_reserve(
                ilium_execution::Lane::Io,
                ilium_execution::JobCost {
                    input_bytes: 4096,
                    result_bytes: 0,
                },
            )
            .expect("notification admission");
        let pending = core_notice("pane");
        let result = execution
            .client
            .run_reserved(
                reservation,
                notification_job(pending, |_, _| Err("backend unavailable")),
            )
            .await
            .expect("the execution job completes despite backend refusal");
        assert_eq!(result.view(), &Err("backend unavailable"));
    }

    #[tokio::test]
    async fn notification_delivery_is_owned_by_the_bounded_execution_bank() {
        let execution = crate::execution::ServerExecution::start().expect("execution bank");
        let monitor = execution.test_monitor();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let admitted = send_with(&execution.client, core_notice("Task"), move |_, _| {
            let _ = entered_tx.send(std::thread::current().id());
            release_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("release notification delivery");
            Ok(())
        });
        assert!(admitted, "bounded I/O admission accepts the notification");

        let delivery_thread = tokio::time::timeout(Duration::from_secs(2), entered_rx)
            .await
            .expect("notification delivery starts")
            .expect("delivery reports its thread");
        let health = monitor.health();
        assert_eq!(health.quota.jobs, 1);
        assert!(health.quota.worker_threads > 0);
        assert_ne!(delivery_thread, std::thread::current().id());

        release_tx.send(()).expect("release worker");
        tokio::time::timeout(Duration::from_secs(2), async {
            while monitor.health().quota.jobs != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("notification receipt retires after delivery");
    }

    #[tokio::test]
    async fn oversized_notification_is_refused_before_execution_admission() {
        let execution = crate::execution::ServerExecution::start().expect("execution bank");
        let monitor = execution.test_monitor();
        let mut oversized_session = String::with_capacity(MAX_NOTIFICATION_JOB_BYTES);
        oversized_session.push('x');

        let admitted = send_with(
            &execution.client,
            PendingNotification::for_plugin(
                oversized_session,
                "acme.alerts".into(),
                "pane".into(),
                None,
                false,
            ),
            |_, _| panic!("oversized notification must not be delivered"),
        );

        assert!(!admitted, "oversized notification is refused");
        assert_eq!(monitor.health().quota.jobs, 0);
    }

    #[tokio::test]
    async fn notification_is_dropped_when_io_admission_is_saturated() {
        let execution = crate::execution::ServerExecution::start().expect("execution bank");
        let monitor = execution.test_monitor();
        let occupied = execution
            .client
            .foundation
            .try_reserve(
                ilium_execution::Lane::Io,
                ilium_execution::JobCost {
                    input_bytes: crate::execution::GENERAL_INPUT_BYTES,
                    result_bytes: 0,
                },
            )
            .expect("fill the shared input-byte admission budget");

        let admitted = send_with(&execution.client, core_notice("pane"), |_, _| {
            panic!("a refused notification must not reach the I/O lane")
        });

        assert!(!admitted, "admission pressure drops advisory notifications");
        assert_eq!(monitor.health().quota.jobs, 1);
        drop(occupied);
    }
}
