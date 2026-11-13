//! Text-to-RGBA adapter for the plugin codec import.
use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Wrap};
use ilium_plugin_api::generated::host::codec::{DecodedImage, RasterText};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};

// Use the same redistributable font as the client, without relying on host fonts.
static DEFAULT_FONT: &[u8] =
    include_bytes!("../../../ilium-client/assets/fonts/CascadiaCode-Regular.otf");
const MAX_TEXT_BYTES: usize = 64 * 1024;
// Match the documented default session memory ceiling for a single raster.
const MAX_RASTER_BYTES: usize = 1024 * 1024 * 1024;

fn invalid() -> PluginError {
    PluginError::new(
        ErrorCode::InvalidArgument,
        "invalid text rasterization request",
    )
}

fn too_large() -> PluginError {
    PluginError::new(ErrorCode::QuotaExceeded, "text raster exceeds memory limit")
}

pub(super) fn rasterize(request: &RasterText) -> Result<DecodedImage, PluginError> {
    if request.text.len() > MAX_TEXT_BYTES
        || !(1..=512).contains(&request.size_px)
        || !(1..=16_384).contains(&request.max_width_px)
    {
        return Err(invalid());
    }

    let mut database = cosmic_text::fontdb::Database::new();
    database.load_font_data(DEFAULT_FONT.to_vec());
    let family = database
        .faces()
        .next()
        .and_then(|face| face.families.first())
        .map(|(name, _)| name.clone())
        .ok_or_else(|| PluginError::new(ErrorCode::Internal, "bundled font is unavailable"))?;
    let mut fonts = FontSystem::new_with_locale_and_db("en-US".to_owned(), database);
    let size = f32::from(request.size_px);
    let mut buffer = Buffer::new(&mut fonts, Metrics::new(size, (size * 1.25).ceil()));
    buffer.set_size(&mut fonts, Some(request.max_width_px as f32), None);
    buffer.set_wrap(&mut fonts, Wrap::WordOrGlyph);
    buffer.set_text(
        &mut fonts,
        &request.text,
        &Attrs::new().family(Family::Name(&family)),
        Shaping::Advanced,
    );
    buffer.shape_until_scroll(&mut fonts, false);

    let (width, height) = buffer
        .layout_runs()
        .fold((1.0f32, 1.0f32), |(width, height), run| {
            (
                width.max(run.line_w.ceil()),
                height.max((run.line_top + run.line_height).ceil()),
            )
        });
    let width = (width as u32).min(request.max_width_px).max(1);
    let height = (height as u32).max(1);
    let bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|bytes| *bytes <= MAX_RASTER_BYTES)
        .ok_or_else(too_large)?;
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(bytes).map_err(|_| too_large())?;
    rgba.resize(bytes, 0);
    let mut cache = SwashCache::new();
    buffer.draw(
        &mut fonts,
        &mut cache,
        Color::rgb(255, 255, 255),
        |x, y, pixel_width, pixel_height, color| {
            let left = i64::from(x).max(0);
            let top = i64::from(y).max(0);
            let right = (i64::from(x) + i64::from(pixel_width)).min(i64::from(width));
            let bottom = (i64::from(y) + i64::from(pixel_height)).min(i64::from(height));
            for row in top..bottom {
                for column in left..right {
                    let offset = ((row as usize * width as usize) + column as usize) * 4;
                    let old = u16::from(rgba[offset + 3]);
                    let ink = u16::from(color.a());
                    rgba[offset..offset + 3].fill(255);
                    rgba[offset + 3] = (old + ink - old * ink / 255) as u8;
                }
            }
        },
    );
    Ok(DecodedImage {
        width,
        height,
        rgba,
    })
}
