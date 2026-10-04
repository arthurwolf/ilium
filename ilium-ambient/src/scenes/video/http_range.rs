//! Single byte ranges for the seekable, memory-only media endpoint.

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ByteRange {
    pub start: usize,
    pub end: usize,
    pub partial: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct UnsatisfiableRange;

pub(super) fn select_range(
    header: Option<&str>,
    length: usize,
) -> Result<ByteRange, UnsatisfiableRange> {
    let Some(header) = header else {
        return Ok(ByteRange {
            start: 0,
            end: length,
            partial: false,
        });
    };
    if length == 0 {
        return Err(UnsatisfiableRange);
    }
    let value = header.strip_prefix("bytes=").ok_or(UnsatisfiableRange)?;
    let (start, end) = value.split_once('-').ok_or(UnsatisfiableRange)?;
    let number = |value: &str| {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(UnsatisfiableRange);
        }
        value.parse::<usize>().map_err(|_| UnsatisfiableRange)
    };
    if start.is_empty() {
        let suffix = number(end)?;
        if suffix == 0 {
            return Err(UnsatisfiableRange);
        }
        return Ok(ByteRange {
            start: length.saturating_sub(suffix),
            end: length,
            partial: true,
        });
    }
    let start = number(start)?;
    if start >= length {
        return Err(UnsatisfiableRange);
    }
    let end = if end.is_empty() {
        length - 1
    } else {
        number(end)?.min(length - 1)
    };
    if end < start {
        return Err(UnsatisfiableRange);
    }
    Ok(ByteRange {
        start,
        end: end + 1,
        partial: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_closed_open_and_suffix_ranges_preserve_seeked_bytes() {
        for (header, start, end, partial) in [
            (None, 0, 10, false),
            (Some("bytes=2-4"), 2, 5, true),
            (Some("bytes=7-"), 7, 10, true),
            (Some("bytes=-3"), 7, 10, true),
            (Some("bytes=2-200"), 2, 10, true),
            (Some("bytes=-200"), 0, 10, true),
            (Some("bytes=0-0"), 0, 1, true),
        ] {
            assert_eq!(
                select_range(header, 10),
                Ok(ByteRange {
                    start,
                    end,
                    partial
                }),
                "{header:?}"
            );
        }
        assert_eq!(
            select_range(None, 0),
            Ok(ByteRange {
                start: 0,
                end: 0,
                partial: false
            })
        );
    }

    #[test]
    fn malformed_multiple_empty_and_overflow_ranges_never_select_bytes() {
        for header in [
            "bytes=10-",
            "bytes=4-2",
            "bytes=-0",
            "bytes=-",
            "bytes=",
            "items=1-2",
            "bytes=0-1,4-5",
            "bytes=+1-2",
            "bytes=1--2",
            "bytes=184467440737095516160-",
            "bytes=0-184467440737095516160",
        ] {
            assert_eq!(
                select_range(Some(header), 10),
                Err(UnsatisfiableRange),
                "{header}"
            );
        }
        assert_eq!(select_range(Some("bytes=0-"), 0), Err(UnsatisfiableRange));
    }
}
