//! The OpenStreetMap attribution a map scene must carry, and the geometry reserved for it.
//!
//! Ported from `ilium-client/src/layout.rs` (`OSM_ATTRIBUTION`, `osm_attribution_area`). The
//! caption owns complete rows above the status and voice footer. When three workspace rows cannot
//! coexist with it, the map is suppressed rather than shown without its attribution. Pure: no host
//! call; the compositor that paints the strip stays with the client.

use crate::hover::Rect;
use crate::rows::OPENSTREETMAP_ID;

/// ASCII keeps the attribution's measured width equal to its byte length. The source URL is in the
/// map scene's settings help.
pub const OSM_ATTRIBUTION: &str = "(c) OpenStreetMap contributors / ODbL";

/// The licence page the settings help links.
pub const OSM_COPYRIGHT_URL: &str = "https://www.openstreetmap.org/copyright";

/// The caveat text of the map scene's help topic: who is credited, under which licence, where.
pub fn help_caveat() -> String {
    format!("Map data: OpenStreetMap contributors / ODbL. Licence and credit: {OSM_COPYRIGHT_URL}")
}

/// Rows reserved for the caption on a screen of `screen_width` columns.
pub fn attribution_rows(screen_width: u16) -> u16 {
    if screen_width == 0 {
        return 0;
    }
    (OSM_ATTRIBUTION.len() as u16).div_ceil(screen_width)
}

/// The strip directly above the footer row, or an empty rectangle when it does not fit.
pub fn attribution_area(screen: Rect) -> Rect {
    if screen.width == 0 {
        return Rect::default();
    }
    let rows = attribution_rows(screen.width);
    if screen.height < rows.saturating_add(4) {
        return Rect::default();
    }
    Rect::new(screen.x, screen.bottom() - 1 - rows, screen.width, rows)
}

/// The strip reserved while `scene` is displayed: only an enabled OSM scene carries one.
pub fn area_for_scene(scene: &str, background_enabled: bool, screen: Rect) -> Rect {
    if background_enabled && scene == OPENSTREETMAP_ID {
        attribution_area(screen)
    } else {
        Rect::default()
    }
}

/// Whether the map is displayed at all: it is suppressed when its credit cannot be shown.
pub fn map_is_displayable(screen: Rect) -> bool {
    !attribution_area(screen).is_empty()
}
