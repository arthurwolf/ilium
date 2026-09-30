//! Headless rendering helpers for tests and probes: drive a scene without a
//! terminal and get Braille text back.

use crate::raster::{threshold, DitherMode, Raster};
use crate::scene::{Frame, Scene};
use std::time::{Duration, SystemTime};

pub struct Rendered {
    pub width: u16,
    pub height: u16,
    pub raster: Raster,
    pub cell_colors: Vec<[u8; 3]>,
}

/// Render one frame of `scene` at animation time `time` (wall time equal).
pub fn render_frame(scene: &mut dyn Scene, width: u16, height: u16, time: Duration) -> Rendered {
    let mut raster = Raster::default();
    raster.resize(usize::from(width) * 2, usize::from(height) * 4);
    let mut cell_colors = vec![[0, 0, 0]; usize::from(width) * usize::from(height)];
    {
        let mut frame = Frame {
            raster: &mut raster,
            cell_colors: &mut cell_colors,
            width,
            height,
            time,
            wall: time,
            now: SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000) + time,
        };
        scene.render(&mut frame);
    }
    Rendered {
        width,
        height,
        raster,
        cell_colors,
    }
}

impl Rendered {
    /// Braille rows thresholded exactly like the client does.
    pub fn braille_lines(&self, density_percent: u16, dither: DitherMode) -> Vec<String> {
        const BITS: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
        let density = f32::from(density_percent) / 100.0;
        (0..usize::from(self.height))
            .map(|y| {
                (0..usize::from(self.width))
                    .map(|x| {
                        let mut cell = 0u8;
                        for (dy, row) in BITS.iter().enumerate() {
                            for (dx, bit) in row.iter().enumerate() {
                                let (px, py) = (x * 2 + dx, y * 4 + dy);
                                let value = self.raster.dots[py * self.raster.width + px];
                                if value * density > threshold(px, py, dither) {
                                    cell |= bit;
                                }
                            }
                        }
                        if cell == 0 {
                            ' '
                        } else {
                            char::from_u32(0x2800 + u32::from(cell)).unwrap_or(' ')
                        }
                    })
                    .collect()
            })
            .collect()
    }

    /// Number of dots brighter than 0.5, a coarse "is anything drawn" probe.
    pub fn lit_dots(&self) -> usize {
        self.raster.dots.iter().filter(|dot| **dot > 0.5).count()
    }
}
