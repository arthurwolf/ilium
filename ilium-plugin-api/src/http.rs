//! The HTTP answer shape that a registry serves and a client reads (reference-registry.md).

/// One HTTP answer: status, content type, optional content encoding and body (PKG-818).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// The status code.
    pub status_code: u16,
    /// The `Content-Type` header value.
    pub content_type_header: String,
    /// The `Content-Encoding` header value, if any.
    pub content_encoding: Option<String>,
    /// The body bytes as sent.
    pub body_bytes: Vec<u8>,
}

impl HttpResponse {
    /// An answer with the given parts (PKG-818).
    pub fn new(
        status: u16,
        content_type: &str,
        content_encoding: Option<&str>,
        body: Vec<u8>,
    ) -> HttpResponse {
        HttpResponse {
            status_code: status,
            content_type_header: content_type.to_owned(),
            content_encoding: content_encoding.map(str::to_owned),
            body_bytes: body,
        }
    }

    /// An answer whose body is `decoded` compressed with gzip (PKG-818).
    pub fn with_gzip_body(status: u16, content_type: &str, decoded: &[u8]) -> HttpResponse {
        HttpResponse::new(status, content_type, Some("gzip"), gzip_encode(decoded))
    }

    /// The status code.
    pub fn status(&self) -> u16 {
        self.status_code
    }

    /// The content type.
    pub fn content_type(&self) -> &str {
        &self.content_type_header
    }

    /// The body bytes as sent.
    pub fn body(&self) -> &[u8] {
        &self.body_bytes
    }

    /// The `Content-Encoding` value, if any.
    pub fn content_encoding(&self) -> Option<&str> {
        self.content_encoding.as_deref()
    }
}

/// CRC-32 (IEEE) of `bytes`, as used by gzip and ZIP.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

struct BitWriter {
    bytes: Vec<u8>,
    bit_buffer: u32,
    bit_count: u32,
}

impl BitWriter {
    fn write_bits(&mut self, value: u32, count: u32) {
        self.bit_buffer |= value << self.bit_count;
        self.bit_count += count;
        while self.bit_count >= 8 {
            self.bytes.push((self.bit_buffer & 0xFF) as u8);
            self.bit_buffer >>= 8;
            self.bit_count -= 8;
        }
    }

    /// Huffman codes are written most significant bit first.
    fn write_code(&mut self, code: u32, length: u32) {
        let mut reversed = 0;
        for bit in 0..length {
            reversed |= ((code >> bit) & 1) << (length - 1 - bit);
        }
        self.write_bits(reversed, length);
    }

    fn write_literal_or_length(&mut self, symbol: u32) {
        match symbol {
            0..=143 => self.write_code(0x30 + symbol, 8),
            144..=255 => self.write_code(0x190 + symbol - 144, 9),
            256..=279 => self.write_code(symbol - 256, 7),
            _ => self.write_code(0xC0 + symbol - 280, 8),
        }
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Deflate with the fixed Huffman code and greedy LZ77 over a 3-byte hash.
fn deflate_fixed(input: &[u8]) -> Vec<u8> {
    let mut writer = BitWriter {
        bytes: Vec::new(),
        bit_buffer: 0,
        bit_count: 0,
    };
    writer.write_bits(1, 1);
    writer.write_bits(1, 2);
    let mut last_seen: std::collections::HashMap<[u8; 3], usize> = std::collections::HashMap::new();
    let mut position = 0;
    while position < input.len() {
        let mut best: Option<(usize, usize)> = None;
        if position + 3 <= input.len() {
            let key = [input[position], input[position + 1], input[position + 2]];
            if let Some(&earlier) = last_seen.get(&key) {
                let distance = position - earlier;
                if distance <= 32768 {
                    let mut length = 0;
                    while length < 258
                        && position + length < input.len()
                        && input[earlier + length] == input[position + length]
                    {
                        length += 1;
                    }
                    if length >= 3 {
                        best = Some((length, distance));
                    }
                }
            }
        }
        match best {
            Some((length, distance)) => {
                let length_index = LENGTH_BASE
                    .iter()
                    .rposition(|&base| usize::from(base) <= length)
                    .unwrap_or(0);
                writer.write_literal_or_length(257 + length_index as u32);
                let extra = u32::from(LENGTH_EXTRA[length_index]);
                if extra > 0 {
                    writer.write_bits((length - usize::from(LENGTH_BASE[length_index])) as u32, extra);
                }
                let distance_index = DISTANCE_BASE
                    .iter()
                    .rposition(|&base| usize::from(base) <= distance)
                    .unwrap_or(0);
                writer.write_code(distance_index as u32, 5);
                let extra = u32::from(DISTANCE_EXTRA[distance_index]);
                if extra > 0 {
                    writer.write_bits(
                        (distance - usize::from(DISTANCE_BASE[distance_index])) as u32,
                        extra,
                    );
                }
                for offset in 0..length {
                    let at = position + offset;
                    if at + 3 <= input.len() {
                        last_seen.insert([input[at], input[at + 1], input[at + 2]], at);
                    }
                }
                position += length;
            }
            None => {
                if position + 3 <= input.len() {
                    last_seen.insert(
                        [input[position], input[position + 1], input[position + 2]],
                        position,
                    );
                }
                writer.write_literal_or_length(u32::from(input[position]));
                position += 1;
            }
        }
    }
    writer.write_literal_or_length(256);
    if writer.bit_count > 0 {
        writer.bytes.push((writer.bit_buffer & 0xFF) as u8);
    }
    writer.bytes
}

/// A gzip stream (RFC 1952) holding `decoded`.
pub(crate) fn gzip_encode(decoded: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 0xFF];
    out.extend(deflate_fixed(decoded));
    out.extend(crc32(decoded).to_le_bytes());
    out.extend((decoded.len() as u32).to_le_bytes());
    out
}

struct BitReader<'a> {
    bytes: &'a [u8],
    position: usize,
    bit_buffer: u32,
    bit_count: u32,
}

impl BitReader<'_> {
    fn bits(&mut self, count: u32) -> Option<u32> {
        while self.bit_count < count {
            let byte = *self.bytes.get(self.position)?;
            self.position += 1;
            self.bit_buffer |= u32::from(byte) << self.bit_count;
            self.bit_count += 8;
        }
        let value = if count == 0 {
            0
        } else {
            self.bit_buffer & ((1u32 << count) - 1)
        };
        self.bit_buffer >>= count;
        self.bit_count -= count;
        Some(value)
    }
}

/// Canonical Huffman table: symbol counts per length and the symbols sorted by code.
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Huffman {
        let mut counts = [0u16; 16];
        for &length in lengths {
            counts[usize::from(length)] += 1;
        }
        counts[0] = 0;
        let mut offsets = [0u16; 16];
        for length in 1..15 {
            offsets[length + 1] = offsets[length] + counts[length];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                symbols[usize::from(offsets[usize::from(length)])] = symbol as u16;
                offsets[usize::from(length)] += 1;
            }
        }
        Huffman { counts, symbols }
    }

    fn decode(&self, reader: &mut BitReader<'_>) -> Option<u16> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for length in 1..16 {
            code |= reader.bits(1)? as i32;
            let count = i32::from(self.counts[length]);
            if code - count < first {
                return self.symbols.get((index + (code - first)) as usize).copied();
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        None
    }
}

/// Inflates a raw deflate stream; `None` for a corrupt stream or one that decodes past `limit`.
fn inflate(input: &[u8], limit: usize) -> Option<(Vec<u8>, usize)> {
    let mut reader = BitReader {
        bytes: input,
        position: 0,
        bit_buffer: 0,
        bit_count: 0,
    };
    let mut out: Vec<u8> = Vec::new();
    loop {
        let is_final = reader.bits(1)?;
        match reader.bits(2)? {
            0 => {
                reader.bit_buffer = 0;
                reader.bit_count = 0;
                let header = input.get(reader.position..reader.position + 4)?;
                let length = usize::from(u16::from_le_bytes([header[0], header[1]]));
                reader.position += 4;
                out.extend_from_slice(input.get(reader.position..reader.position + length)?);
                reader.position += length;
            }
            kind @ (1 | 2) => {
                let (literals, distances) = if kind == 1 {
                    let mut lengths = [8u8; 288];
                    lengths[144..256].fill(9);
                    lengths[256..280].fill(7);
                    (Huffman::new(&lengths), Huffman::new(&[5u8; 30]))
                } else {
                    let literal_count = reader.bits(5)? as usize + 257;
                    let distance_count = reader.bits(5)? as usize + 1;
                    let code_length_count = reader.bits(4)? as usize + 4;
                    const ORDER: [usize; 19] =
                        [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                    let mut code_lengths = [0u8; 19];
                    for &slot in ORDER.iter().take(code_length_count) {
                        code_lengths[slot] = reader.bits(3)? as u8;
                    }
                    let code_table = Huffman::new(&code_lengths);
                    let mut lengths = vec![0u8; literal_count + distance_count];
                    let mut index = 0;
                    while index < lengths.len() {
                        let symbol = code_table.decode(&mut reader)?;
                        let (value, repeat) = match symbol {
                            0..=15 => (symbol as u8, 1),
                            16 => (*lengths.get(index.checked_sub(1)?)?, 3 + reader.bits(2)? as usize),
                            17 => (0, 3 + reader.bits(3)? as usize),
                            _ => (0, 11 + reader.bits(7)? as usize),
                        };
                        if index + repeat > lengths.len() {
                            return None;
                        }
                        lengths[index..index + repeat].fill(value);
                        index += repeat;
                    }
                    (
                        Huffman::new(&lengths[..literal_count]),
                        Huffman::new(&lengths[literal_count..]),
                    )
                };
                loop {
                    let symbol = literals.decode(&mut reader)?;
                    match symbol {
                        0..=255 => out.push(symbol as u8),
                        256 => break,
                        _ => {
                            let index = usize::from(symbol) - 257;
                            let length = usize::from(*LENGTH_BASE.get(index)?)
                                + reader.bits(u32::from(LENGTH_EXTRA[index]))? as usize;
                            let distance_index = usize::from(distances.decode(&mut reader)?);
                            let distance = usize::from(*DISTANCE_BASE.get(distance_index)?)
                                + reader.bits(u32::from(DISTANCE_EXTRA[distance_index]))? as usize;
                            if distance > out.len() {
                                return None;
                            }
                            for _ in 0..length {
                                out.push(out[out.len() - distance]);
                            }
                        }
                    }
                    if out.len() > limit {
                        return None;
                    }
                }
            }
            _ => return None,
        }
        if out.len() > limit {
            return None;
        }
        if is_final == 1 {
            return Some((out, reader.position));
        }
    }
}

/// Decodes a gzip stream; `None` when it is corrupt, its checksum fails or it decodes to more
/// than `limit` bytes.
pub(crate) fn gzip_decode(bytes: &[u8], limit: usize) -> Option<Vec<u8>> {
    if bytes.len() < 18 || bytes[0] != 0x1F || bytes[1] != 0x8B || bytes[2] != 8 {
        return None;
    }
    let flags = bytes[3];
    let mut position = 10;
    if flags & 4 != 0 {
        let extra = usize::from(u16::from_le_bytes([*bytes.get(position)?, *bytes.get(position + 1)?]));
        position += 2 + extra;
    }
    for flag in [8u8, 16u8] {
        if flags & flag != 0 {
            while *bytes.get(position)? != 0 {
                position += 1;
            }
            position += 1;
        }
    }
    if flags & 2 != 0 {
        position += 2;
    }
    let (decoded, used) = inflate(bytes.get(position..)?, limit)?;
    let trailer = bytes.get(position + used..position + used + 8)?;
    let expected = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    (crc32(&decoded) == expected).then_some(decoded)
}
