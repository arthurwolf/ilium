//! Small dependency-free helpers shared by the parsers and the engines.

/// FNV-1a 64-bit hash, used to dedupe message and response identifiers without
/// storing the strings.
pub(crate) fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Whether `haystack` contains `needle`.
///
/// The scan anchors on the first non-quote byte of the needle, because `"` is
/// by far the most common byte in JSON lines and would make every position a
/// candidate.
pub(crate) fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    find_bytes(haystack, needle).is_some()
}

/// Index of the first occurrence of `needle` in `haystack`.
pub(crate) fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if haystack.len() < needle.len() {
        return None;
    }
    let anchor = needle.iter().position(|&byte| byte != b'"').unwrap_or(0);
    let anchor_byte = needle[anchor];
    let last_start = haystack.len() - needle.len();
    let mut search_from = anchor;
    while search_from < haystack.len() {
        let relative = haystack[search_from..]
            .iter()
            .position(|&byte| byte == anchor_byte)?;
        let anchor_position = search_from + relative;
        let start = anchor_position - anchor;
        if start > last_start {
            return None;
        }
        if &haystack[start..start + needle.len()] == needle {
            return Some(start);
        }
        search_from = anchor_position + 1;
    }
    None
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.fff](Z|+HH:MM|-HH:MM)` into Unix milliseconds.
pub(crate) fn parse_iso8601_ms(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let number = |from: usize, to: usize| -> Option<i64> {
        let slice = text.get(from..to)?;
        if !slice.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        slice.parse::<i64>().ok()
    };
    if bytes[4] != b'-' || bytes[7] != b'-' || !matches!(bytes[10], b'T' | b't' | b' ') {
        return None;
    }
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    let mut position = 19;
    let mut millis = 0_i64;
    if bytes.get(position) == Some(&b'.') {
        position += 1;
        let digits_start = position;
        while bytes.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
        let digits = &text[digits_start..position];
        let mut scaled = 0_i64;
        for (index, digit) in digits.bytes().take(3).enumerate() {
            scaled += i64::from(digit - b'0') * 10_i64.pow(2 - index as u32);
        }
        millis = scaled;
    }
    let offset_minutes = match bytes.get(position) {
        None | Some(b'Z') | Some(b'z') => 0,
        Some(sign @ (b'+' | b'-')) => {
            let hours = number(position + 1, position + 3)?;
            let minutes = number(position + 4, position + 6).unwrap_or(0);
            let total = hours * 60 + minutes;
            if *sign == b'+' {
                total
            } else {
                -total
            }
        }
        Some(_) => return None,
    };
    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second - offset_minutes * 60;
    Some(seconds * 1_000 + millis)
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Saturating conversion of a JSON token count into the compact `u32` domain.
pub(crate) fn saturate_u32(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_needles_with_leading_quotes() {
        let line = br#"{"type":"assistant","message":{"usage":{"input_tokens":3}}}"#;
        assert!(contains_bytes(line, br#""usage""#));
        assert!(!contains_bytes(line, br#""compacted""#));
        assert_eq!(find_bytes(line, b"usage"), Some(32));
        assert!(contains_bytes(b"", b""));
        assert!(!contains_bytes(b"ab", b"abc"));
        assert!(contains_bytes(b"xxabc", b"abc"));
        assert!(!contains_bytes(b"xxab", b"abc"));
    }

    #[test]
    fn parses_timestamps() {
        assert_eq!(parse_iso8601_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_iso8601_ms("2026-10-05T08:56:13.011Z"),
            Some(1_791_190_573_011)
        );
        assert_eq!(
            parse_iso8601_ms("2026-10-05T10:56:13.011+02:00"),
            parse_iso8601_ms("2026-10-05T08:56:13.011Z")
        );
        assert_eq!(
            parse_iso8601_ms("2024-02-29T00:00:00Z"),
            Some(1_709_164_800_000)
        );
        assert_eq!(parse_iso8601_ms("garbage"), None);
        assert_eq!(parse_iso8601_ms("2026-13-05T08:56:13Z"), None);
    }
}
