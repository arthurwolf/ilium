//! Continuous page-relative scrolling, independent of fetch and frame clocks.

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slow_scroll_visits_bottom_and_dwells_before_rotating() {
        let mut scroll = PageScroll::new(10);
        assert!(!scroll.advance(10.0, 20.0, 2, 10));
        assert_eq!(scroll.position(), 0.0);
        assert!(!scroll.advance(50.0, 20.0, 2, 10));
        assert!((scroll.position() - 10.0).abs() < 0.001);
        assert!(!scroll.advance(50.0, 20.0, 2, 10));
        assert_eq!(scroll.position(), 20.0);
        assert!(!scroll.advance(9.0, 20.0, 2, 10));
        assert!(scroll.advance(1.0, 20.0, 2, 10));
    }
    #[test]
    fn paused_pages_never_rotate_even_when_short() {
        let mut scroll = PageScroll::new(2);
        assert!(!scroll.advance(1e9, 0.0, 0, 2));
        assert_eq!(scroll.position(), 0.0);
        assert!(scroll.advance(4.0, 0.0, 2, 2));
    }
    #[test]
    fn speed_edits_do_not_jump_and_resize_clamps_position() {
        let mut scroll = PageScroll::new(2);
        scroll.advance(12.0, 100.0, 10, 2);
        assert_eq!(scroll.position(), 10.0);
        scroll.advance(1.0, 100.0, 20, 2);
        assert_eq!(scroll.position(), 12.0);
        scroll.advance(0.0, 5.0, 20, 2);
        assert_eq!(scroll.position(), 5.0);
    }

    #[test]
    fn a_slow_fractional_speed_remains_motion_rather_than_rounding_to_pause() {
        let mut scroll = PageScroll::new(2);
        assert!(!scroll.advance_with_speed(4.0, 100.0, 2, 25, 2));
        assert!((scroll.position() - 0.1).abs() < 0.000_001);
        assert!(!scroll.advance_with_speed(100.0, 100.0, 2, 0, 2));
        assert!((scroll.position() - 0.1).abs() < 0.000_001);
    }

    #[test]
    fn a_reflowed_longer_page_receives_a_full_new_bottom_dwell() {
        let mut scroll = PageScroll::new(2);
        assert!(!scroll.advance(12.0, 10.0, 10, 2));
        assert!(!scroll.advance(1.0, 10.0, 10, 2));
        assert!(!scroll.advance(10.0, 20.0, 10, 2));
        assert!(!scroll.advance(1.0, 20.0, 10, 2));
        assert!(scroll.advance(1.0, 20.0, 10, 2));
    }
}

#[derive(Debug)]
pub struct PageScroll {
    position: f64,
    top_remaining: f64,
    bottom_remaining: Option<f64>,
}

impl PageScroll {
    pub fn new(dwell_seconds: u16) -> Self {
        Self {
            position: 0.0,
            top_remaining: f64::from(dwell_seconds),
            bottom_remaining: None,
        }
    }

    pub fn position(&self) -> f64 {
        self.position
    }

    /// Return true only after the whole page and both dwell periods.
    #[cfg(test)]
    pub fn advance(
        &mut self,
        delta: f64,
        maximum: f64,
        scroll_tenths: u16,
        dwell_seconds: u16,
    ) -> bool {
        self.advance_with_speed(delta, maximum, scroll_tenths, 100, dwell_seconds)
    }

    pub fn advance_with_speed(
        &mut self,
        delta: f64,
        maximum: f64,
        scroll_tenths: u16,
        speed_percent: u16,
        dwell_seconds: u16,
    ) -> bool {
        let maximum = if maximum.is_finite() {
            maximum.max(0.0)
        } else {
            0.0
        };
        self.position = self.position.min(maximum);
        if scroll_tenths == 0 || speed_percent == 0 || !delta.is_finite() {
            return false;
        }
        let mut remaining = delta.max(0.0);
        let top = remaining.min(self.top_remaining);
        self.top_remaining -= top;
        remaining -= top;
        if self.top_remaining > 0.0 {
            return false;
        }
        let rate = f64::from(scroll_tenths) / 10.0 * f64::from(speed_percent) / 100.0;
        let travel_seconds = ((maximum - self.position) / rate).max(0.0);
        // Resize/zoom can reveal more content after the old bottom dwell
        // started. The new bottom needs its own complete reading pause.
        if travel_seconds > 0.0 {
            self.bottom_remaining = None;
        }
        if remaining < travel_seconds {
            self.position += remaining * rate;
            self.bottom_remaining = None;
            return false;
        }
        self.position = maximum;
        remaining -= travel_seconds;
        let bottom = self
            .bottom_remaining
            .get_or_insert(f64::from(dwell_seconds));
        *bottom = (*bottom - remaining).max(0.0);
        *bottom == 0.0
    }
}
