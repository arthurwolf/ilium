//! Presentation-only pixels bridging asynchronous saved-route preparation.
//! No model, source, issued view, receipt or owner survives in this buffer.
use crate::{
    scene::Frame,
    voxel_landscape::assets::budget::{ByteBudget, Cancel, Reservation},
};
use std::mem::size_of;

pub(super) struct Display {
    geometry: [usize; 4],
    scale: u64,
    dots: Vec<f32>,
    colors: Vec<[u8; 3]>,
    valid: bool,
    _reservation: Reservation,
}

impl Display {
    fn geometry(frame: &Frame<'_>) -> [usize; 4] {
        [
            frame.raster.width,
            frame.raster.height,
            usize::from(frame.width),
            usize::from(frame.height),
        ]
    }

    pub(super) fn new(
        frame: &Frame<'_>,
        scale: f64,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self, String> {
        let geometry = Self::geometry(frame);
        let dots = geometry[0]
            .checked_mul(geometry[1])
            .ok_or("Saved display extent overflow")?;
        let cells = geometry[2]
            .checked_mul(geometry[3])
            .ok_or("Saved display cell extent overflow")?;
        if geometry.contains(&0)
            || dots != frame.raster.dots.len()
            || cells != frame.cell_colors.len()
            || !scale.is_finite()
            || scale <= 0.0
        {
            return Err("Saved display geometry is inconsistent".into());
        }
        // Charge before allocating, including a conservative vector/allocator
        // margin. Reject unexpected extra capacity rather than retaining it
        // outside the existing scene account.
        let payload = dots
            .checked_mul(size_of::<f32>())
            .and_then(|n| {
                cells
                    .checked_mul(size_of::<[u8; 3]>())
                    .and_then(|c| n.checked_add(c))
            })
            .ok_or("Saved display allocation overflow")?;
        let charged = payload
            .checked_add(1024)
            .and_then(|n| u64::try_from(n).ok())
            .ok_or("Saved display account overflow")?;
        let reservation = budget.reserve(charged, cancel).map_err(|e| e.to_string())?;
        let mut dot_buffer = Vec::new();
        dot_buffer
            .try_reserve_exact(dots)
            .map_err(|e| e.to_string())?;
        let mut color_buffer = Vec::new();
        color_buffer
            .try_reserve_exact(cells)
            .map_err(|e| e.to_string())?;
        let actual = dot_buffer
            .capacity()
            .checked_mul(size_of::<f32>())
            .and_then(|n| {
                color_buffer
                    .capacity()
                    .checked_mul(size_of::<[u8; 3]>())
                    .and_then(|c| n.checked_add(c))
            })
            .ok_or("Saved display capacity overflow")?;
        if actual > payload {
            return Err("Saved display allocation exceeded admitted capacity".into());
        }
        dot_buffer.resize(dots, 0.0);
        color_buffer.resize(cells, [0; 3]);
        Ok(Self {
            geometry,
            scale: scale.to_bits(),
            dots: dot_buffer,
            colors: color_buffer,
            valid: false,
            _reservation: reservation,
        })
    }

    pub(super) fn matches(&self, frame: &Frame<'_>, scale: f64) -> bool {
        self.geometry == Self::geometry(frame)
            && self.scale == scale.to_bits()
            && self.dots.len() == frame.raster.dots.len()
            && self.colors.len() == frame.cell_colors.len()
    }

    pub(super) fn invalidate(&mut self) {
        self.valid = false;
    }

    pub(super) fn capture(&mut self, frame: &Frame<'_>, scale: f64) {
        self.valid = self.matches(frame, scale);
        if self.valid {
            self.dots.copy_from_slice(&frame.raster.dots);
            self.colors.copy_from_slice(frame.cell_colors);
        }
    }

    pub(super) fn replay(&self, frame: &mut Frame<'_>, scale: f64) -> bool {
        if !self.valid || !self.matches(frame, scale) {
            return false;
        }
        frame.raster.dots.copy_from_slice(&self.dots);
        frame.cell_colors.copy_from_slice(&self.colors);
        frame.raster.owner_ids.fill(0);
        true
    }
}
