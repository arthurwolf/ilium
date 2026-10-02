//! Named-root, big-endian Java NBT. No compression or filesystem access.
use std::collections::BTreeMap;

/// Java strings are UTF-16, including possible unpaired surrogates.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Text(pub Vec<u16>);
impl From<&str> for Text {
    fn from(s: &str) -> Self {
        Self(s.encode_utf16().collect())
    }
}
impl Text {
    /// Never substitutes U+FFFD. Callers must handle non-Unicode Java strings.
    pub fn to_utf8(&self) -> std::result::Result<String, std::string::FromUtf16Error> {
        String::from_utf16(&self.0)
    }
}
pub type Compound = BTreeMap<Text, Tag>;
pub fn get<'a>(c: &'a Compound, name: &str) -> Option<&'a Tag> {
    c.get(&Text::from(name))
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    /// IEEE-754 bits, retaining signed zero and NaN payloads exactly.
    FloatBits(u32),
    DoubleBits(u64),
    ByteArray(Vec<i8>),
    String(Text),
    List {
        kind: u8,
        values: Vec<Tag>,
    },
    Compound(Compound),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub name: Text,
    pub root: Compound,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_depth: usize,
    pub max_string_bytes: usize,
    pub max_collection_len: usize,
    pub max_nodes: usize,
    /// Aggregate array elements, list elements, and compound entries.
    pub max_elements: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 16 << 20,
            max_depth: 64,
            max_string_bytes: 65535,
            max_collection_len: 1 << 20,
            max_nodes: 262144,
            max_elements: 2 << 20,
        }
    }
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("NBT at byte {offset}: {reason}")]
pub struct Error {
    pub offset: usize,
    pub reason: &'static str,
}
type Result<T> = std::result::Result<T, Error>;

pub fn parse(bytes: &[u8], limits: Limits) -> Result<Document> {
    parse_checked(bytes, limits, &|| false)
}
pub fn parse_checked(bytes: &[u8], limits: Limits, cancel: &dyn Fn() -> bool) -> Result<Document> {
    let mut p = Parser {
        bytes,
        pos: 0,
        limits,
        nodes: 0,
        elements: 0,
        cancel,
    };
    // A hard recursion ceiling also bounds recursive destruction on failure.
    if limits.max_depth > 128 {
        return Err(p.err("depth limit exceeds hard ceiling 128"));
    }
    if bytes.len() > limits.max_bytes {
        return Err(p.err("uncompressed byte limit"));
    }
    if p.byte()? != 10 {
        return Err(p.err("root must be a named compound"));
    }
    let name = p.text()?;
    // Tag kind 10 always produces Compound; parsing errors return before here.
    let Tag::Compound(root) = p.value(10, 0)? else {
        unreachable!()
    };
    if p.pos != bytes.len() {
        return Err(p.err("trailing bytes after root"));
    }
    Ok(Document { name, root })
}
struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    limits: Limits,
    nodes: usize,
    elements: usize,
    cancel: &'a dyn Fn() -> bool,
}
impl Parser<'_> {
    fn err(&self, reason: &'static str) -> Error {
        Error {
            offset: self.pos,
            reason,
        }
    }
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        if (self.cancel)() {
            return Err(self.err("cancelled"));
        }
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| self.err("length overflow"))?;
        let s = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| self.err("truncated input"))?;
        self.pos = end;
        Ok(s)
    }
    fn fixed<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut a = [0; N];
        a.copy_from_slice(self.take(N)?);
        Ok(a)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.fixed::<1>()?[0])
    }
    fn elements(&mut self, n: usize) -> Result<()> {
        if n > self.limits.max_elements.saturating_sub(self.elements) {
            return Err(self.err("aggregate element limit"));
        }
        self.elements += n;
        Ok(())
    }
    fn len(&mut self) -> Result<usize> {
        let n = i32::from_be_bytes(self.fixed()?);
        if n < 0 {
            return Err(self.err("negative collection length"));
        }
        let n = n as usize;
        if n > self.limits.max_collection_len {
            return Err(self.err("collection limit"));
        }
        self.elements(n)?;
        Ok(n)
    }
    fn vector<T>(&self, n: usize) -> Result<Vec<T>> {
        let mut v = Vec::new();
        v.try_reserve_exact(n)
            .map_err(|_| self.err("allocation failed"))?;
        Ok(v)
    }
    fn array_bytes(&mut self, n: usize, width: usize) -> Result<&[u8]> {
        let size = n
            .checked_mul(width)
            .ok_or_else(|| self.err("array length overflow"))?;
        self.take(size)
    }
    fn text(&mut self) -> Result<Text> {
        let n = usize::from(u16::from_be_bytes(self.fixed()?));
        if n > self.limits.max_string_bytes {
            return Err(self.err("string byte limit"));
        }
        // Verify all input bytes exist before reserving owned UTF-16 units.
        let start = self.pos;
        self.take(n)?;
        let b = &self.bytes[start..self.pos];
        let mut units = self.vector(n)?;
        let mut i = 0;
        while i < b.len() {
            let a = b[i];
            let (u, width) = match a {
                1..=127 => (u16::from(a), 1),
                0xc0..=0xdf => {
                    let t = *b.get(i + 1).ok_or_else(|| self.err("truncated MUTF-8"))?;
                    if t & 0xc0 != 0x80 {
                        return Err(self.err("invalid MUTF-8 continuation"));
                    }
                    let u = (u16::from(a & 31) << 6) | u16::from(t & 63);
                    if u < 128 && !(a == 0xc0 && t == 0x80) {
                        return Err(self.err("overlong MUTF-8"));
                    }
                    (u, 2)
                }
                0xe0..=0xef => {
                    let t = *b.get(i + 1).ok_or_else(|| self.err("truncated MUTF-8"))?;
                    let v = *b.get(i + 2).ok_or_else(|| self.err("truncated MUTF-8"))?;
                    if t & 0xc0 != 0x80 || v & 0xc0 != 0x80 {
                        return Err(self.err("invalid MUTF-8 continuation"));
                    }
                    let u =
                        (u16::from(a & 15) << 12) | (u16::from(t & 63) << 6) | u16::from(v & 63);
                    if u < 2048 {
                        return Err(self.err("overlong MUTF-8"));
                    }
                    (u, 3)
                }
                _ => return Err(self.err("invalid canonical Java MUTF-8")),
            };
            units.push(u);
            i += width;
        }
        Ok(Text(units))
    }
    fn value(&mut self, kind: u8, depth: usize) -> Result<Tag> {
        if (self.cancel)() {
            return Err(self.err("cancelled"));
        }
        if depth > self.limits.max_depth {
            return Err(self.err("depth limit"));
        }
        if self.nodes >= self.limits.max_nodes {
            return Err(self.err("node limit"));
        }
        self.nodes += 1;
        Ok(match kind {
            1 => Tag::Byte(self.byte()? as i8),
            2 => Tag::Short(i16::from_be_bytes(self.fixed()?)),
            3 => Tag::Int(i32::from_be_bytes(self.fixed()?)),
            4 => Tag::Long(i64::from_be_bytes(self.fixed()?)),
            5 => Tag::FloatBits(u32::from_be_bytes(self.fixed()?)),
            6 => Tag::DoubleBits(u64::from_be_bytes(self.fixed()?)),
            7 => {
                let n = self.len()?;
                self.array_bytes(n, 1)?;
                let mut v = self.vector(n)?;
                v.extend(self.bytes[self.pos - n..self.pos].iter().map(|b| *b as i8));
                Tag::ByteArray(v)
            }
            8 => Tag::String(self.text()?),
            9 => {
                let kind = self.byte()?;
                let n = self.len()?;
                if kind > 12 || (kind == 0 && n != 0) {
                    return Err(self.err("invalid list element tag"));
                }
                if n > self.limits.max_nodes.saturating_sub(self.nodes) {
                    return Err(self.err("list exceeds remaining node budget"));
                }
                let mut values = self.vector(n)?;
                for _ in 0..n {
                    values.push(self.value(kind, depth + 1)?);
                }
                Tag::List { kind, values }
            }
            10 => {
                let mut fields = Compound::new();
                loop {
                    let kind = self.byte()?;
                    if kind == 0 {
                        break;
                    }
                    if kind > 12 {
                        return Err(self.err("unknown compound tag"));
                    }
                    if fields.len() >= self.limits.max_collection_len {
                        return Err(self.err("compound entry limit"));
                    }
                    self.elements(1)?;
                    let name = self.text()?;
                    if fields.contains_key(&name) {
                        return Err(self.err("duplicate compound name"));
                    }
                    let value = self.value(kind, depth + 1)?;
                    fields.insert(name, value);
                }
                Tag::Compound(fields)
            }
            11 => {
                let n = self.len()?;
                self.array_bytes(n, 4)?;
                let mut v = self.vector(n)?;
                for b in self.bytes[self.pos - n * 4..self.pos].chunks_exact(4) {
                    v.push(i32::from_be_bytes([b[0], b[1], b[2], b[3]]));
                }
                Tag::IntArray(v)
            }
            12 => {
                let n = self.len()?;
                self.array_bytes(n, 8)?;
                let mut v = self.vector(n)?;
                for b in self.bytes[self.pos - n * 8..self.pos].chunks_exact(8) {
                    v.push(i64::from_be_bytes([
                        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                    ]));
                }
                Tag::LongArray(v)
            }
            _ => return Err(self.err("unknown or misplaced end tag")),
        })
    }
}
