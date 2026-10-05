//! Shared blocking ureq transport policy for already-admitted Ilium workers.
//!
//! ureq's default resolver creates an untracked helper thread when a timeout
//! is finite. This resolver keeps native DNS on the calling worker, so the
//! worker's existing bank or supervisor retains its actual physical custody.
//! Native getaddrinfo cannot be forcibly cancelled: a blocked lookup may
//! outlive the HTTP deadline and delay owner shutdown. The deadline is
//! reported once the lookup returns; no helper is abandoned at timeout.
use std::time::Instant;
use ureq::unversioned::{
    resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver},
    transport::{time::Duration as ResolverDuration, DefaultConnector, NextTimeout},
};

#[derive(Debug, Default)]
pub struct OwnedResolver;

fn resolve_owned(
    resolver: &impl Resolver,
    uri: &ureq::http::Uri,
    config: &ureq::config::Config,
    timeout: NextTimeout,
) -> Result<ResolvedSocketAddrs, ureq::Error> {
    let started = Instant::now();
    if matches!(timeout.after, ResolverDuration::Exact(duration) if duration.is_zero()) {
        return Err(ureq::Error::Timeout(timeout.reason));
    }
    // NotHappening selects ureq's synchronous getaddrinfo path. The outer
    // request still owns its normal connect/body timeouts and configuration.
    let resolved = resolver.resolve(
        uri,
        config,
        NextTimeout {
            after: ResolverDuration::NotHappening,
            reason: timeout.reason,
        },
    );
    if matches!(timeout.after, ResolverDuration::Exact(duration) if started.elapsed() >= duration) {
        return Err(ureq::Error::Timeout(timeout.reason));
    }
    resolved
}

impl Resolver for OwnedResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        resolve_owned(&DefaultResolver::default(), uri, config, timeout)
    }
}

/// Use only while an existing finite bank or supervised worker owns the call.
/// All caller-supplied proxy, redirect, status and timeout config is retained.
pub fn agent(config: ureq::config::Config) -> ureq::Agent {
    ureq::Agent::with_parts(config, DefaultConnector::default(), OwnedResolver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fmt,
        sync::{mpsc, Mutex},
        thread,
        time::Duration,
    };

    struct BlockingResolver {
        entered: mpsc::SyncSender<(thread::ThreadId, bool)>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl fmt::Debug for BlockingResolver {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("BlockingResolver").finish_non_exhaustive()
        }
    }
    impl Resolver for BlockingResolver {
        fn resolve(
            &self,
            _uri: &ureq::http::Uri,
            _config: &ureq::config::Config,
            timeout: NextTimeout,
        ) -> Result<ResolvedSocketAddrs, ureq::Error> {
            self.entered
                .send((
                    thread::current().id(),
                    matches!(timeout.after, ResolverDuration::NotHappening),
                ))
                .map_err(|_| ureq::Error::HostNotFound)?;
            self.release
                .lock()
                .map_err(|_| ureq::Error::HostNotFound)?
                .recv_timeout(Duration::from_secs(2))
                .map_err(|_| ureq::Error::HostNotFound)?;
            Err(ureq::Error::HostNotFound)
        }
    }

    #[test]
    fn elapsed_dns_deadline_keeps_the_owned_caller_blocked_until_resolver_returns() {
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let (finished_tx, finished_rx) = mpsc::sync_channel(1);
        let handle = thread::spawn(move || {
            let resolver = BlockingResolver {
                entered: entered_tx,
                release: Mutex::new(release_rx),
            };
            let uri = "https://example.org/".parse().expect("valid test URI");
            let result = resolve_owned(
                &resolver,
                &uri,
                &ureq::config::Config::default(),
                NextTimeout {
                    after: ResolverDuration::Exact(Duration::from_millis(1)),
                    reason: ureq::Timeout::Global,
                },
            );
            finished_tx
                .send(matches!(
                    result,
                    Err(ureq::Error::Timeout(ureq::Timeout::Global))
                ))
                .expect("test receiver exists");
        });
        let (resolver_thread, synchronous) = entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("injected resolver entered");
        assert_eq!(resolver_thread, handle.thread().id());
        assert!(
            synchronous,
            "ureq must receive NotHappening, not spawn a DNS helper"
        );
        assert!(matches!(
            finished_rx.recv_timeout(Duration::from_millis(20)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        release_tx.send(()).expect("owned caller still waiting");
        assert_eq!(
            finished_rx.recv_timeout(Duration::from_secs(2)),
            Ok(true),
            "elapsed DNS deadline reported after the owned resolver returns"
        );
        handle.join().expect("test resolver thread joined");
    }

    #[test]
    fn expired_before_dns_does_not_invoke_resolver() {
        struct NeverResolver;
        impl fmt::Debug for NeverResolver {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("NeverResolver")
            }
        }
        impl Resolver for NeverResolver {
            fn resolve(
                &self,
                _: &ureq::http::Uri,
                _: &ureq::config::Config,
                _: NextTimeout,
            ) -> Result<ResolvedSocketAddrs, ureq::Error> {
                panic!("expired DNS must never invoke resolver")
            }
        }
        let uri = "https://example.org/".parse().expect("valid test URI");
        assert!(matches!(
            resolve_owned(
                &NeverResolver,
                &uri,
                &ureq::config::Config::default(),
                NextTimeout {
                    after: ResolverDuration::Exact(Duration::ZERO),
                    reason: ureq::Timeout::Global,
                },
            ),
            Err(ureq::Error::Timeout(ureq::Timeout::Global))
        ));
    }
}
