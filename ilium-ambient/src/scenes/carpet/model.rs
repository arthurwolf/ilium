//! Normalized ground geometry shared by all hidden-object simulations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Hunters,
    Snake,
    Life,
    AutoChess,
    LiveChess,
    Dvd,
    Orbits,
    DigitalClock,
    AnalogClock,
}

#[derive(Debug, Clone, Copy)]
pub struct Body {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub radius: f32,
    pub height: f32,
}
