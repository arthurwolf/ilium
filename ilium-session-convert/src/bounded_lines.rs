//! Reads a byte stream one line at a time into a reused buffer with a hard
//! cap on the length of a single line, so one pathological line can never
//! make the conversion allocate it whole. Every line is reported with the
//! byte offset of its first byte, which lets a later pass seek back to it.

use std::io::{self, BufRead, Read, Seek, SeekFrom};

/// One line from [`BoundedLineReader::next_line`].
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BoundedLine<'a> {
    /// A complete line without its trailing `\n` (a `\r` before it is kept;
    /// callers trim). `offset` is the position of the line's first byte.
    Line { offset: u64, bytes: &'a [u8] },
    /// A line longer than the cap. Its bytes were consumed and discarded;
    /// `length` counts them without the terminating newline.
    Oversized { offset: u64, length: u64 },
}

pub(crate) struct BoundedLineReader<R> {
    reader: R,
    /// Reused across lines; it never grows past `max_line_bytes + 1`.
    buffer: Vec<u8>,
    /// Stream position of the next line's first byte.
    next_offset: u64,
    max_line_bytes: usize,
}

impl<R: BufRead> BoundedLineReader<R> {
    /// Reads from the start of `reader`, whose first byte is at stream
    /// offset 0.
    pub(crate) fn new(reader: R, max_line_bytes: usize) -> Self {
        Self::starting_at(reader, 0, max_line_bytes)
    }

    /// Reads from `reader`, whose first byte sits at `offset` in the
    /// underlying stream (for a reader positioned by a seek).
    pub(crate) fn starting_at(reader: R, offset: u64, max_line_bytes: usize) -> Self {
        Self {
            reader,
            buffer: Vec::new(),
            next_offset: offset,
            max_line_bytes,
        }
    }

    /// Stream position after the last line returned (the next line's start).
    pub(crate) fn offset(&self) -> u64 {
        self.next_offset
    }

    /// Returns the next line, `None` at end of stream. A line over the cap
    /// is skipped without being buffered beyond the cap and reported as
    /// [`BoundedLine::Oversized`].
    pub(crate) fn next_line(&mut self) -> io::Result<Option<BoundedLine<'_>>> {
        self.buffer.clear();
        let offset = self.next_offset;
        // Reading at most cap + 1 bytes decides "over the cap" without ever
        // holding more than that; `read_until` keeps std's fast newline scan.
        let limit = self.max_line_bytes as u64 + 1;
        let read = (&mut self.reader)
            .take(limit)
            .read_until(b'\n', &mut self.buffer)?;
        if read == 0 {
            return Ok(None);
        }
        let terminated = self.buffer.last() == Some(&b'\n');
        if terminated || read <= self.max_line_bytes {
            self.next_offset += read as u64;
            if terminated {
                self.buffer.pop();
            }
            return Ok(Some(BoundedLine::Line {
                offset,
                bytes: &self.buffer,
            }));
        }
        // `cap + 1` bytes without a newline: discard what was buffered and
        // skip to the end of the line chunk by chunk.
        self.buffer.clear();
        let mut consumed = read as u64;
        let mut found_newline = false;
        loop {
            let chunk = self.reader.fill_buf()?;
            if chunk.is_empty() {
                break;
            }
            match chunk.iter().position(|byte| *byte == b'\n') {
                Some(index) => {
                    consumed += index as u64 + 1;
                    self.reader.consume(index + 1);
                    found_newline = true;
                    break;
                }
                None => {
                    let length = chunk.len();
                    consumed += length as u64;
                    self.reader.consume(length);
                }
            }
        }
        self.next_offset += consumed;
        Ok(Some(BoundedLine::Oversized {
            offset,
            length: consumed - u64::from(found_newline),
        }))
    }
}

impl<R: BufRead + Seek> BoundedLineReader<R> {
    /// Repositions the stream so the next line read starts at `offset`.
    pub(crate) fn seek_to(&mut self, offset: u64) -> io::Result<()> {
        self.reader.seek(SeekFrom::Start(offset))?;
        self.next_offset = offset;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn collect(input: &[u8], cap: usize) -> Vec<(u64, Result<Vec<u8>, u64>)> {
        let mut reader = BoundedLineReader::new(Cursor::new(input.to_vec()), cap);
        let mut lines = Vec::new();
        while let Some(line) = reader.next_line().unwrap() {
            lines.push(match line {
                BoundedLine::Line { offset, bytes } => (offset, Ok(bytes.to_vec())),
                BoundedLine::Oversized { offset, length } => (offset, Err(length)),
            });
        }
        lines
    }

    #[test]
    fn lines_keep_their_offsets_and_lose_only_the_newline() {
        let lines = collect(b"ab\n\ncd\r\nlast", 10);
        assert_eq!(
            lines,
            vec![
                (0, Ok(b"ab".to_vec())),
                (3, Ok(Vec::new())),
                (4, Ok(b"cd\r".to_vec())),
                (8, Ok(b"last".to_vec())),
            ]
        );
    }

    #[test]
    fn a_line_exactly_at_the_cap_is_kept_and_one_over_is_skipped() {
        let lines = collect(b"12345\n123456\nz\n", 5);
        assert_eq!(
            lines,
            vec![
                (0, Ok(b"12345".to_vec())),
                (6, Err(6)),
                (13, Ok(b"z".to_vec())),
            ]
        );
    }

    #[test]
    fn an_oversized_final_line_without_newline_is_counted_in_full() {
        // The oversized line spans several internal chunks of a small
        // BufReader, so the skip loop is exercised past one fill.
        let mut input = b"ok\n".to_vec();
        input.extend(std::iter::repeat_n(b'x', 50));
        let reader = std::io::BufReader::with_capacity(4, Cursor::new(input));
        let mut lines = BoundedLineReader::new(reader, 8);
        assert!(matches!(
            lines.next_line().unwrap(),
            Some(BoundedLine::Line {
                offset: 0,
                bytes: b"ok"
            })
        ));
        assert!(matches!(
            lines.next_line().unwrap(),
            Some(BoundedLine::Oversized {
                offset: 3,
                length: 50
            })
        ));
        assert_eq!(lines.offset(), 53);
        assert!(lines.next_line().unwrap().is_none());
    }

    #[test]
    fn seeking_rereads_the_line_at_an_offset() {
        let mut lines = BoundedLineReader::new(Cursor::new(b"one\ntwo\nthree\n".to_vec()), 100);
        lines.seek_to(4).unwrap();
        assert!(matches!(
            lines.next_line().unwrap(),
            Some(BoundedLine::Line {
                offset: 4,
                bytes: b"two"
            })
        ));
        assert_eq!(lines.offset(), 8);
        let reader = BoundedLineReader::starting_at(Cursor::new(b"x\n".to_vec()), 40, 100);
        assert_eq!(reader.offset(), 40);
    }
}
