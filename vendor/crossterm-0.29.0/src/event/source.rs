use std::{io, time::Duration};

#[cfg(feature = "event-stream")]
use super::sys::Waker;
use super::InternalEvent;

#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;

/// An interface for trying to read an `InternalEvent` within an optional `Duration`.
pub(crate) trait EventSource: Sync + Send {
    /// Tries to read an `InternalEvent` within the given duration.
    ///
    /// # Arguments
    ///
    /// * `timeout` - `None` block indefinitely until an event is available, `Some(duration)` blocks
    ///   for the given timeout
    ///
    /// Returns `Ok(None)` if there's no event available and timeout expires.
    fn try_read(&mut self, timeout: Option<Duration>) -> io::Result<Option<InternalEvent>>;

    /// Query ownership changes parsing only for the duration of that query.
    fn set_terminal_query_mode(&mut self, _active: bool) {}

    fn take_native_storage(&mut self) -> Option<Box<dyn super::native::NativeStorageLease>> {
        None
    }

    // Sources that cannot prove their original state is empty retain custody.
    fn native_has_retained_input(&self) -> bool {
        true
    }

    fn native_retained_original(&self) -> (&[u8], &[u8]) {
        (&[], &[])
    }

    /// Returns a `Waker` allowing to wake/force the `try_read` method to return `Ok(None)`.
    #[cfg(feature = "event-stream")]
    fn waker(&self) -> Waker;
}
