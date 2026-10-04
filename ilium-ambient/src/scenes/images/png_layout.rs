//! Pure borrowed PNG allocation inventory proposal, not production wiring.
//! CRC/order/semantic decoding remains the pinned PNG backend's responsibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutRefusal {
    EncodedLimit,
    Framing,
    Header,
    Dimensions,
    Overflow,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Layout {
    pub width: u32,
    pub native_pixel_bytes: usize,
    pub height: u32,
    pub raw_bytes: usize,
    pub largest_raw_row: usize,
    pub copied_metadata_payload: usize,
    pub largest_exif_payload: usize,
    pub text_payload: usize,
    pub text_records: [usize; 3],
}

fn add(target: &mut usize, amount: usize) -> Result<(), LayoutRefusal> {
    *target = target.checked_add(amount).ok_or(LayoutRefusal::Overflow)?;
    Ok(())
}
fn word(bytes: &[u8], position: usize) -> Result<u32, LayoutRefusal> {
    let end = position.checked_add(4).ok_or(LayoutRefusal::Overflow)?;
    Ok(u32::from_be_bytes(
        bytes
            .get(position..end)
            .ok_or(LayoutRefusal::Framing)?
            .try_into()
            .map_err(|_| LayoutRefusal::Framing)?,
    ))
}
fn pass_extent(extent: u32, start: u32, stride: u32) -> usize {
    if extent <= start {
        0
    } else {
        ((extent - start - 1) / stride + 1) as usize
    }
}
fn pass(
    width: usize,
    height: usize,
    bits_per_pixel: usize,
) -> Result<(usize, usize), LayoutRefusal> {
    if width == 0 || height == 0 {
        return Ok((0, 0));
    }
    let row = width
        .checked_mul(bits_per_pixel)
        .and_then(|bits| bits.checked_add(7))
        .map(|bits| bits / 8)
        .and_then(|bytes| bytes.checked_add(1))
        .ok_or(LayoutRefusal::Overflow)?;
    Ok((row.checked_mul(height).ok_or(LayoutRefusal::Overflow)?, row))
}

/// No allocations, format hooks, decompression or unchecked declared lengths.
/// Resource limits are an explicit caller policy; this visitor is not a decoder.
pub(super) fn inspect(
    bytes: &[u8],
    max_file: usize,
    max_width: u32,
    max_height: u32,
    max_pixels: u64,
) -> Result<Layout, LayoutRefusal> {
    if bytes.len() > max_file {
        return Err(LayoutRefusal::EncodedLimit);
    }
    if bytes.get(..8) != Some(b"\x89PNG\r\n\x1a\n")
        || word(bytes, 8)? != 13
        || bytes.get(12..16) != Some(b"IHDR")
    {
        return Err(LayoutRefusal::Header);
    }
    let width = word(bytes, 16)?;
    let height = word(bytes, 20)?;
    if width == 0
        || height == 0
        || width > max_width
        || height > max_height
        || u64::from(width) * u64::from(height) > max_pixels
    {
        return Err(LayoutRefusal::Dimensions);
    }
    let header = bytes.get(24..29).ok_or(LayoutRefusal::Framing)?;
    let depth = header[0];
    let channels = match header[1] {
        0 if matches!(depth, 1 | 2 | 4 | 8 | 16) => 1,
        2 if matches!(depth, 8 | 16) => 3,
        3 if matches!(depth, 1 | 2 | 4 | 8) => 1,
        4 if matches!(depth, 8 | 16) => 2,
        6 if matches!(depth, 8 | 16) => 4,
        _ => return Err(LayoutRefusal::Header),
    };
    if header[2] != 0 || header[3] != 0 || header[4] > 1 {
        return Err(LayoutRefusal::Header);
    }
    let bits = channels * usize::from(depth);
    let (raw_bytes, largest_raw_row) = if header[4] == 0 {
        pass(width as usize, height as usize, bits)?
    } else {
        let mut total = 0;
        let mut largest = 0;
        // Adam7 starts/strides, exactly the seven disjoint passes.
        for (x, y, dx, dy) in [
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ] {
            let (bytes, row) = pass(pass_extent(width, x, dx), pass_extent(height, y, dy), bits)?;
            add(&mut total, bytes)?;
            largest = largest.max(row);
        }
        (total, largest)
    };
    let mut layout = Layout {
        width,
        native_pixel_bytes: if depth == 16 { 8 } else { 4 },
        height,
        raw_bytes,
        // Backend initializes row/shift policy from the full canvas, even if
        // the corresponding Adam7 pass has no rows (e.g. height=1).
        largest_raw_row: largest_raw_row.max(pass(width as usize, 1, bits)?.1),
        copied_metadata_payload: 0,
        largest_exif_payload: 0,
        text_payload: 0,
        text_records: [0; 3],
    };
    let mut position = 8usize;
    while position < bytes.len() {
        let payload =
            usize::try_from(word(bytes, position)?).map_err(|_| LayoutRefusal::Overflow)?;
        let kind_start = position.checked_add(4).ok_or(LayoutRefusal::Overflow)?;
        let payload_start = position.checked_add(8).ok_or(LayoutRefusal::Overflow)?;
        let end = payload_start
            .checked_add(payload)
            .and_then(|end| end.checked_add(4))
            .ok_or(LayoutRefusal::Overflow)?;
        let Some(kind) = bytes.get(kind_start..payload_start) else {
            break;
        };
        if end > bytes.len() {
            break;
        }
        match kind {
            b"PLTE" | b"sBIT" | b"tRNS" | b"eXIf" | b"bKGD" => {
                add(&mut layout.copied_metadata_payload, payload)?;
                if kind == b"eXIf" {
                    layout.largest_exif_payload = layout.largest_exif_payload.max(payload);
                }
            }
            b"tEXt" | b"zTXt" | b"iTXt" => {
                let index = match kind {
                    b"tEXt" => 0,
                    b"zTXt" => 1,
                    _ => 2,
                };
                add(&mut layout.text_payload, payload)?;
                add(&mut layout.text_records[index], 1)?;
            }
            _ => {}
        }
        position = end;
        // Backend finishes the PNG stream here; trailing bytes are untouched.
        if kind == b"IEND" {
            break;
        }
    }
    Ok(layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn header(width: u32, height: u32, interlaced: bool) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, u8::from(interlaced)]);
        bytes.extend_from_slice(&[0; 4]); // CRC remains authoritative backend work.
        bytes
    }
    fn chunk(bytes: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
        bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(payload);
        bytes.extend_from_slice(&[0; 4]);
    }
    #[test]
    fn exact_adam7_rows_include_only_nonempty_disjoint_passes() {
        let single = inspect(&header(1, 1, true), 1024, 100, 100, 10000).unwrap();
        assert_eq!((single.raw_bytes, single.largest_raw_row), (5, 5));
        let eight = inspect(&header(8, 8, true), 1024, 100, 100, 10000).unwrap();
        // 64RGBA pixels plus15 nonempty filter rows across seven passes.
        assert_eq!((eight.raw_bytes, eight.largest_raw_row), (271, 33));
        let plain = inspect(&header(8, 8, false), 1024, 100, 100, 10000).unwrap();
        assert_eq!((plain.raw_bytes, plain.largest_raw_row), (264, 33));
    }
    #[test]
    fn metadata_duplicates_count_original_payloads_without_heap_copy() {
        let mut bytes = header(1, 1, false);
        chunk(&mut bytes, b"eXIf", b"first");
        chunk(&mut bytes, b"eXIf", b"largest");
        chunk(&mut bytes, b"tEXt", b"a\0b");
        chunk(&mut bytes, b"iTXt", b"c\0d");
        chunk(&mut bytes, b"IEND", b"");
        bytes.extend_from_slice(b"backend ignores trailing");
        let pointer = bytes.as_ptr();
        let result = inspect(&bytes, 1024, 100, 100, 10000).unwrap();
        assert_eq!(result.copied_metadata_payload, 12);
        assert_eq!(result.largest_exif_payload, 7);
        assert_eq!((result.text_payload, result.text_records), (6, [1, 0, 1]));
        assert_eq!(bytes.as_ptr(), pointer);
    }
    #[test]
    fn incomplete_huge_declared_chunk_does_not_count_unparsed_metadata() {
        let mut bytes = header(1, 1, false);
        bytes.extend_from_slice(&u32::MAX.to_be_bytes());
        bytes.extend_from_slice(b"iTXt");
        let layout = inspect(&bytes, 1024, 100, 100, 10000).unwrap();
        assert_eq!(layout.text_payload, 0);
        assert_eq!(layout.text_records, [0; 3]);
    }
}
