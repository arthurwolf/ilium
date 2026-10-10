//! Fixed parent-native backlog: one refused original and one lookahead.
//! No allocation, cloning, coalescing or semantic disposal happens here.
#[derive(Debug)]
pub(crate) struct InputBacklog<T> {
    head: Option<T>,
    lookahead: Option<T>,
}
pub(crate) struct RestoreRefused<T> {
    pub original: Option<T>,
    pub lookahead: Option<T>,
}
impl<T> Default for InputBacklog<T> {
    fn default() -> Self {
        Self {
            head: None,
            lookahead: None,
        }
    }
}
impl<T> InputBacklog<T> {
    pub(crate) fn is_empty(&self) -> bool {
        self.head.is_none()
    }
    pub(crate) fn front(&self) -> Option<&T> {
        self.head.as_ref()
    }
    pub(crate) fn take_front(&mut self) -> Option<T> {
        let result = self.head.take();
        self.head = self.lookahead.take();
        result
    }
    /// Prepend the refused original before its already-read lookahead and any
    /// untouched retained successor. Refusal is atomic and returns both inputs.
    pub(crate) fn restore(
        &mut self,
        original: Option<T>,
        lookahead: Option<T>,
    ) -> Result<(), RestoreRefused<T>> {
        let present = usize::from(self.head.is_some()) + usize::from(self.lookahead.is_some());
        let incoming = usize::from(original.is_some()) + usize::from(lookahead.is_some());
        if present + incoming > 2 {
            return Err(RestoreRefused {
                original,
                lookahead,
            });
        }
        for next in [lookahead, original].into_iter().flatten() {
            self.lookahead = self.head.take();
            self.head = Some(next);
        }
        Ok(())
    }
}

/// Serve already-read original input before receiving any later native bytes.
pub(super) struct BacklogReady<'a, S: crate::ReadyInput> {
    pub backlog: &'a mut InputBacklog<S::Item>,
    pub upstream: &'a mut S,
}
impl<S: crate::ReadyInput> crate::ReadyInput for BacklogReady<'_, S> {
    type Item = S::Item;
    fn try_next(&mut self) -> Result<Self::Item, tokio::sync::mpsc::error::TryRecvError> {
        match self.backlog.take_front() {
            Some(original) => Ok(original),
            None => self.upstream.try_next(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn put<T>(backlog: &mut InputBacklog<T>, first: Option<T>, second: Option<T>) {
        assert!(backlog.restore(first, second).is_ok());
    }
    #[test]
    fn simultaneous_refusal_and_lookahead_retain_both_original_allocations_in_order() {
        let first = Box::new(11);
        let second = Box::new(22);
        let first_pointer = &*first as *const i32;
        let second_pointer = &*second as *const i32;
        let mut backlog = InputBacklog::default();
        put(&mut backlog, Some(first), Some(second));
        let first = backlog.take_front().unwrap();
        let second = backlog.take_front().unwrap();
        assert_eq!(&*first as *const i32, first_pointer);
        assert_eq!(&*second as *const i32, second_pointer);
        assert_eq!((*first, *second), (11, 22));
        assert!(backlog.is_empty());
    }
    #[test]
    fn refused_front_returns_before_untouched_retained_tail() {
        let mut backlog = InputBacklog::default();
        put(&mut backlog, Some(1), Some(2));
        let refused = backlog.take_front();
        put(&mut backlog, refused, None);
        assert_eq!(backlog.take_front(), Some(1));
        assert_eq!(backlog.take_front(), Some(2));
    }
    #[test]
    fn overflow_returns_both_incoming_values_without_mutating_existing_backlog() {
        let mut backlog = InputBacklog::default();
        put(&mut backlog, Some(1), Some(2));
        let rejected = backlog.restore(Some(3), Some(4)).err().unwrap();
        assert_eq!((rejected.original, rejected.lookahead), (Some(3), Some(4)));
        assert_eq!(backlog.take_front(), Some(1));
        assert_eq!(backlog.take_front(), Some(2));
    }
    #[test]
    fn every_two_slot_shape_preserves_fifo_without_dropping_values() {
        for initial in 0..=2 {
            for incoming in 0..=2 {
                let mut backlog = InputBacklog::default();
                put(
                    &mut backlog,
                    (initial > 0).then_some(3),
                    (initial > 1).then_some(4),
                );
                let result =
                    backlog.restore((incoming > 0).then_some(1), (incoming > 1).then_some(2));
                if initial + incoming > 2 {
                    assert!(result.is_err());
                } else {
                    assert!(result.is_ok());
                    let mut expected = Vec::new();
                    if incoming > 0 {
                        expected.push(1);
                    }
                    if incoming > 1 {
                        expected.push(2);
                    }
                    if initial > 0 {
                        expected.push(3);
                    }
                    if initial > 1 {
                        expected.push(4);
                    }
                    let actual: Vec<_> = std::iter::from_fn(|| backlog.take_front()).collect();
                    assert_eq!(actual, expected);
                }
            }
        }
    }
}
