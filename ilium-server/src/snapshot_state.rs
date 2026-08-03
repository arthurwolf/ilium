//! The lifecycle of the crash-recovery snapshot's "does the file on disk
//! still match memory?" question, as one atomic state machine.
//!
//! This exists as its own type because the question has three answers, not
//! two, and the third one is absorbing: a session that has been killed must
//! never have a snapshot written for it again, however many still-attached
//! connections go on requesting one during `crate::run`'s shutdown grace
//! period (see `ipc::handlers::handle_kill_session`).
//!
//! It was previously two independent `AtomicBool`s -- `snapshot_dirty` and
//! `session_killed` -- sequenced by hand at each call site. That cannot be
//! made correct by any amount of re-checking: two separate atomics have no
//! common modification order, so "is it killed?" and "mark it dirty" can
//! always interleave with a concurrent "mark killed, then clear dirty" in an
//! order that leaves the flag stuck dirty for a killed session. Re-reading
//! the kill flag afterward only narrows the window; it never closes it,
//! because that read is itself just another separately-ordered operation.
//!
//! One atomic has one modification order, so every transition below is a
//! single indivisible step and `Killed` is unconditionally terminal. That is
//! the whole reason this is one `AtomicU8` rather than two flags, and the
//! reason no `pub` field lets a caller sequence the steps by hand again.

use std::sync::atomic::{AtomicU8, Ordering};

/// In sync with the on-disk snapshot; the writer has nothing to do.
const CLEAN: u8 = 0;

/// Memory has moved on from the file; the background writer owes a write.
const DIRTY: u8 = 1;

/// The session was killed and its snapshot deleted. Terminal: no transition
/// out of this state exists, so nothing can resurrect the file afterward.
const KILLED: u8 = 2;

/// Whether the crash-recovery snapshot needs rewriting, and whether the
/// session it belongs to still wants one at all.
pub struct SnapshotState {
    state: AtomicU8,
}

impl SnapshotState {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(CLEAN),
        }
    }

    /// Records that memory no longer matches the file. Returns whether the
    /// snapshot is now dirty -- `false` means the session was killed and the
    /// request was correctly ignored, so the caller must not wake the writer.
    ///
    /// Deliberately reports `true` for an already-dirty state rather than
    /// only for a `Clean` -> `Dirty` transition: the caller's wakeup is
    /// coalescing (`tokio::sync::Notify` holds one permit), and notifying on
    /// every accepted request keeps "can a wakeup be lost?" from being a
    /// question anyone has to reason about.
    pub fn mark_dirty(&self) -> bool {
        self.state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current != KILLED).then_some(DIRTY)
            })
            .is_ok()
    }

    /// Claims a pending write, returning whether this caller won it. The
    /// claim is a compare-exchange from `Dirty` specifically, not a swap to
    /// `Clean`: a swap would clobber `Killed` back to a writable state and
    /// re-enable exactly the resurrection this type exists to prevent.
    ///
    /// Consuming the flag *before* the write means a request that lands
    /// mid-write re-dirties it for the writer's next pass rather than being
    /// silently absorbed into a write that had already read the tree.
    pub fn take_dirty(&self) -> bool {
        self.state
            .compare_exchange(DIRTY, CLEAN, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Marks the session unrecoverable, discarding any pending write in the
    /// same step. Terminal, and the only writer of `KILLED`.
    pub fn mark_session_killed(&self) {
        self.state.store(KILLED, Ordering::Release);
    }

    /// Observers, for tests only: acting on either answer outside a test
    /// would be reintroducing the check-then-act this type exists to remove.
    #[cfg(test)]
    pub fn is_dirty(&self) -> bool {
        self.state.load(Ordering::Acquire) == DIRTY
    }

    #[cfg(test)]
    pub fn is_session_killed(&self) -> bool {
        self.state.load(Ordering::Acquire) == KILLED
    }
}

impl Default for SnapshotState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn an_ordinary_request_dirties_the_snapshot_and_the_writer_claims_it_once() {
        let state = SnapshotState::new();

        assert!(!state.take_dirty(), "a clean snapshot owes no write");
        assert!(state.mark_dirty(), "a live session accepts a request");
        assert!(state.is_dirty());
        assert!(state.take_dirty(), "the writer claims the pending write");
        assert!(
            !state.take_dirty(),
            "the same pending write must not be claimed twice"
        );
    }

    #[test]
    fn a_killed_session_refuses_every_later_request_and_write() {
        let state = SnapshotState::new();
        state.mark_dirty();
        state.mark_session_killed();

        assert!(state.is_session_killed());
        assert!(!state.is_dirty(), "the kill discards the pending write");
        assert!(
            !state.mark_dirty(),
            "a still-attached connection must not resurrect a killed session"
        );
        assert!(
            !state.take_dirty(),
            "and the writer must find nothing to do"
        );
        assert!(
            state.is_session_killed(),
            "a refused request must leave the session killed, not clean"
        );
    }

    /// The writer's claim must not be able to lift a session back out of
    /// `Killed`. Written as its own test because the tempting implementation
    /// -- `swap(CLEAN)`, mirroring the `AtomicBool::swap(false)` this
    /// replaced -- passes every other test here while silently re-enabling
    /// snapshot writes for a killed session.
    #[test]
    fn a_writers_claim_never_lifts_a_killed_session_back_to_writable() {
        let state = SnapshotState::new();
        state.mark_session_killed();

        assert!(!state.take_dirty());
        assert!(
            state.is_session_killed(),
            "the writer's claim must leave a killed session killed"
        );
        assert!(!state.mark_dirty());
    }

    /// Stresses a genuinely concurrent request and kill on real OS threads.
    /// Under the previous two-flag design this is the interleaving that could
    /// leave a killed session marked dirty; under one atomic the outcome is
    /// `Killed` regardless of which thread wins, because `mark_dirty`'s read
    /// and write are the same indivisible operation as the kill's store.
    ///
    /// Asserts the state is `Killed` rather than merely "not dirty": `Clean`
    /// is also not dirty, and would mean a request had erased the kill.
    #[test]
    fn a_concurrent_request_and_kill_always_settle_on_killed() {
        for _ in 0..1_000 {
            let contended = Arc::new(SnapshotState::new());

            let requester = Arc::clone(&contended);
            let requester_thread = std::thread::spawn(move || requester.mark_dirty());

            let killer = Arc::clone(&contended);
            let killer_thread = std::thread::spawn(move || killer.mark_session_killed());

            let accepted = requester_thread.join().expect("requester must not panic");
            killer_thread.join().expect("killer must not panic");

            assert!(
                contended.is_session_killed(),
                "a concurrent kill must always win, whatever the interleaving"
            );
            assert!(
                !contended.is_dirty(),
                "a killed session must never be left owing a write"
            );
            if !accepted {
                assert!(
                    !contended.take_dirty(),
                    "a request that reported itself refused must not have left work behind"
                );
            }
        }
    }
}
