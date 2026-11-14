//! Ken-Burns motion: easing, per-image poses and the visible image rectangle.
//!
//! The view is a rectangle in normalized image coordinates (0..1 on both
//! axes). For `Fill` and `Stretch` it always lies inside the image; for `Fit`
//! it may extend beyond the image along the letterboxed axis (the overflow is
//! the border) and is then centred on the image.

use super::settings::{Easing, FitMode, Motion};

/// Shape the raw progress 0..1 with the chosen easing.
pub fn ease(easing: Easing, progress: f32) -> f32 {
    let p = progress.clamp(0.0, 1.0);
    match easing {
        Easing::Linear => p,
        Easing::EaseInOut => p * p * (3.0 - 2.0 * p),
        Easing::EaseOut => 1.0 - (1.0 - p) * (1.0 - p),
    }
}

/// Triangle wave 0 -> 1 -> 0 with `one_way` seconds per direction.
pub fn ping_pong(seconds: f32, one_way: f32) -> f32 {
    if one_way <= 0.0 {
        return 0.0;
    }
    let phase = (seconds / one_way).rem_euclid(2.0);
    if phase <= 1.0 {
        phase
    } else {
        2.0 - phase
    }
}

/// splitmix64 finalizer: stable pseudo-random values from a seed.
pub fn hash64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// Uniform value in 0..1 derived from `variant` and a `salt`.
pub fn unit(variant: u64, salt: u64) -> f32 {
    (hash64(variant ^ salt.wrapping_mul(0xd6e8_feb8_6659_fd93)) >> 40) as f32 / (1u64 << 24) as f32
}

/// Camera pose: zoom (>= 1) and where in the free space the view sits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub zoom: f32,
    /// 0 = far left / top edge, 1 = far right / bottom edge of the free space.
    pub pan_x: f32,
    pub pan_y: f32,
}

impl Pose {
    pub const STILL: Self = Self {
        zoom: 1.0,
        pan_x: 0.5,
        pan_y: 0.5,
    };

    fn lerp(from: Self, to: Self, t: f32) -> Self {
        Self {
            zoom: from.zoom + (to.zoom - from.zoom) * t,
            pan_x: from.pan_x + (to.pan_x - from.pan_x) * t,
            pan_y: from.pan_y + (to.pan_y - from.pan_y) * t,
        }
    }
}

/// Pose at raw `progress` (0..=1) of one image's move.
///
/// * `strength` is the zoom amount / pan range (0..=0.5).
/// * `variant` selects the random path of `Drift` and `ZoomPan`.
/// * `reverse` flips pan direction on alternate slides.
pub fn pose_at(
    motion: Motion,
    strength: f32,
    easing: Easing,
    progress: f32,
    variant: u64,
    reverse: bool,
) -> Pose {
    let s = strength.clamp(0.0, 0.5);
    let p = ease(easing, progress);
    let directed = if reverse { 1.0 - p } else { p };
    match motion {
        Motion::None => Pose::STILL,
        Motion::ZoomIn => Pose {
            zoom: 1.0 + s * p,
            ..Pose::STILL
        },
        Motion::ZoomOut => Pose {
            zoom: 1.0 + s * (1.0 - p),
            ..Pose::STILL
        },
        Motion::PanHorizontal => Pose {
            zoom: 1.0 + s,
            pan_x: directed,
            pan_y: 0.5,
        },
        Motion::PanVertical => Pose {
            zoom: 1.0 + s,
            pan_x: 0.5,
            pan_y: directed,
        },
        Motion::Drift => {
            let from = Pose {
                zoom: 1.0 + s * (0.4 + 0.6 * unit(variant, 1)),
                pan_x: 0.1 + 0.8 * unit(variant, 2),
                pan_y: 0.1 + 0.8 * unit(variant, 3),
            };
            let to = Pose {
                zoom: 1.0 + s * (0.4 + 0.6 * unit(variant, 4)),
                pan_x: 0.1 + 0.8 * unit(variant, 5),
                pan_y: 0.1 + 0.8 * unit(variant, 6),
            };
            Pose::lerp(from, to, p)
        }
        Motion::ZoomPan => {
            let x = 0.15 + 0.2 * unit(variant, 7);
            let y = 0.15 + 0.2 * unit(variant, 8);
            let flip = unit(variant, 9) > 0.5;
            let (from_x, to_x) = if flip { (1.0 - x, x) } else { (x, 1.0 - x) };
            let from = Pose {
                zoom: 1.0 + s * 0.3,
                pan_x: from_x,
                pan_y: y,
            };
            let to = Pose {
                zoom: 1.0 + s,
                pan_x: to_x,
                pan_y: 1.0 - y,
            };
            Pose::lerp(from, to, p)
        }
    }
}

/// A rectangle in normalized image coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub x0: f32,
    pub y0: f32,
    pub width: f32,
    pub height: f32,
}

impl View {
    #[cfg(test)]
    pub fn is_inside_image(&self) -> bool {
        const EPS: f32 = 1e-4;
        self.x0 >= -EPS
            && self.y0 >= -EPS
            && self.x0 + self.width <= 1.0 + EPS
            && self.y0 + self.height <= 1.0 + EPS
    }
}

/// The visible rectangle for `pose`. `image_aspect` and `screen_aspect` are
/// width / height in square units (the raster's dots are square).
pub fn view_rect(fit: FitMode, image_aspect: f32, screen_aspect: f32, pose: Pose) -> View {
    let image_aspect = image_aspect.max(1e-3);
    let screen_aspect = screen_aspect.max(1e-3);
    let (base_width, base_height) = match fit {
        FitMode::Stretch => (1.0, 1.0),
        FitMode::Fill => {
            if image_aspect > screen_aspect {
                (screen_aspect / image_aspect, 1.0)
            } else {
                (1.0, image_aspect / screen_aspect)
            }
        }
        FitMode::Fit => {
            if image_aspect > screen_aspect {
                (1.0, image_aspect / screen_aspect)
            } else {
                (screen_aspect / image_aspect, 1.0)
            }
        }
    };
    let zoom = pose.zoom.max(1.0);
    let width = base_width / zoom;
    let height = base_height / zoom;
    let place = |extent: f32, pan: f32| {
        let free = 1.0 - extent;
        if free >= 0.0 {
            pan.clamp(0.0, 1.0) * free
        } else {
            free / 2.0
        }
    };
    View {
        x0: place(width, pose.pan_x),
        y0: place(height, pose.pan_y),
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOTIONS: [Motion; 7] = [
        Motion::None,
        Motion::ZoomIn,
        Motion::ZoomOut,
        Motion::PanHorizontal,
        Motion::PanVertical,
        Motion::Drift,
        Motion::ZoomPan,
    ];

    #[test]
    fn easing_endpoints_and_monotonic() {
        for easing in Easing::ALL {
            assert!(ease(*easing, 0.0).abs() < 1e-6);
            assert!((ease(*easing, 1.0) - 1.0).abs() < 1e-6);
            let mut previous = 0.0;
            for step in 0..=100 {
                let value = ease(*easing, step as f32 / 100.0);
                assert!(value >= previous - 1e-6);
                previous = value;
            }
        }
        assert!((ease(Easing::EaseInOut, 0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn ping_pong_is_continuous_and_bounded() {
        let mut previous = ping_pong(0.0, 10.0);
        for step in 1..500 {
            let value = ping_pong(step as f32 * 0.1, 10.0);
            assert!((0.0..=1.0).contains(&value));
            assert!((value - previous).abs() <= 0.011);
            previous = value;
        }
        assert!((ping_pong(10.0, 10.0) - 1.0).abs() < 1e-6);
        assert!(ping_pong(20.0, 10.0).abs() < 1e-6);
    }

    #[test]
    fn fill_and_stretch_views_stay_inside_the_image() {
        for fit in [FitMode::Fill, FitMode::Stretch] {
            for motion in MOTIONS {
                for (image_aspect, screen_aspect) in
                    [(1.5, 3.0), (3.0, 1.5), (1.0, 1.0), (0.4, 2.5), (5.0, 0.7)]
                {
                    for variant in [0u64, 7, 12345] {
                        for reverse in [false, true] {
                            for step in 0..=20 {
                                let pose = pose_at(
                                    motion,
                                    0.5,
                                    Easing::EaseInOut,
                                    step as f32 / 20.0,
                                    variant,
                                    reverse,
                                );
                                assert!(pose.zoom >= 1.0);
                                let view = view_rect(fit, image_aspect, screen_aspect, pose);
                                assert!(view.is_inside_image(), "{fit:?} {motion:?} {view:?}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn fill_view_has_the_screen_aspect_and_covers_one_axis() {
        let view = view_rect(FitMode::Fill, 2.0, 1.0, Pose::STILL);
        assert!((view.height - 1.0).abs() < 1e-6);
        assert!((view.width - 0.5).abs() < 1e-6);
        assert!(((view.width * 2.0) / view.height - 1.0).abs() < 1e-6);
        let tall = view_rect(FitMode::Fill, 0.5, 1.0, Pose::STILL);
        assert!((tall.width - 1.0).abs() < 1e-6 && (tall.height - 0.5).abs() < 1e-6);
    }

    #[test]
    fn fit_view_contains_the_whole_image_along_the_letterboxed_axis() {
        let view = view_rect(FitMode::Fit, 2.0, 1.0, Pose::STILL);
        assert!((view.width - 1.0).abs() < 1e-6);
        assert!(view.height > 1.0 && view.y0 < 0.0);
        assert!((view.y0 + view.height / 2.0 - 0.5).abs() < 1e-6);
        // Zooming far enough turns the border into a real crop.
        let zoomed = view_rect(
            FitMode::Fit,
            2.0,
            1.0,
            Pose {
                zoom: 3.0,
                ..Pose::STILL
            },
        );
        assert!(zoomed.is_inside_image());
    }

    #[test]
    fn motion_is_continuous_in_progress() {
        for motion in MOTIONS {
            for easing in Easing::ALL {
                let mut previous = pose_at(motion, 0.3, *easing, 0.0, 99, false);
                for step in 1..=200 {
                    let pose = pose_at(motion, 0.3, *easing, step as f32 / 200.0, 99, false);
                    assert!((pose.zoom - previous.zoom).abs() < 0.02);
                    assert!((pose.pan_x - previous.pan_x).abs() < 0.03);
                    assert!((pose.pan_y - previous.pan_y).abs() < 0.03);
                    previous = pose;
                }
            }
        }
    }

    #[test]
    fn motions_actually_move_and_none_does_not() {
        for motion in MOTIONS {
            let start = pose_at(motion, 0.3, Easing::Linear, 0.0, 5, false);
            let end = pose_at(motion, 0.3, Easing::Linear, 1.0, 5, false);
            let moved = (start.zoom - end.zoom).abs()
                + (start.pan_x - end.pan_x).abs()
                + (start.pan_y - end.pan_y).abs();
            if motion == Motion::None {
                assert_eq!(start, end);
            } else {
                assert!(moved > 0.05, "{motion:?} barely moves");
            }
        }
    }

    #[test]
    fn zero_strength_keeps_zoom_at_one() {
        for motion in [Motion::ZoomIn, Motion::ZoomOut, Motion::PanHorizontal] {
            let pose = pose_at(motion, 0.0, Easing::Linear, 0.7, 1, false);
            assert_eq!(pose.zoom, 1.0);
        }
    }

    #[test]
    fn drift_paths_differ_per_variant_and_repeat_per_variant() {
        let a = pose_at(Motion::Drift, 0.3, Easing::Linear, 0.5, 1, false);
        let b = pose_at(Motion::Drift, 0.3, Easing::Linear, 0.5, 2, false);
        let again = pose_at(Motion::Drift, 0.3, Easing::Linear, 0.5, 1, false);
        assert_ne!(a, b);
        assert_eq!(a, again);
    }

    #[test]
    fn reverse_flips_the_pan_direction() {
        let forward = pose_at(Motion::PanHorizontal, 0.2, Easing::Linear, 0.1, 0, false);
        let backward = pose_at(Motion::PanHorizontal, 0.2, Easing::Linear, 0.1, 0, true);
        assert!(forward.pan_x < 0.5 && backward.pan_x > 0.5);
    }
}
