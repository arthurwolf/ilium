//! Exact saved states; coordinates stay Minecraft [x, y, z]. No material conversion.

use super::{
    nbt::{self, Compound, Document, Tag, Text},
    region,
};
use std::collections::BTreeMap;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Region(#[from] region::Error),
    #[error("invalid chunk: {0}")]
    Invalid(&'static str),
    #[error("chunk resource limit: {0}")]
    Limit(&'static str),
    #[error("chunk decoding cancelled")]
    Cancelled,
    #[error("palette index {value} at cell {cell} exceeds palette length {length}")]
    PaletteIndex {
        cell: usize,
        value: usize,
        length: usize,
    },
    #[error("packed data has {actual} longs; expected {expected}")]
    PackedLength { actual: usize, expected: usize },
}
type DecodeResult<T> = Result<T, Error>;
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_sections: usize,
    pub max_palette_entries: usize,
    pub max_properties: usize,
    pub max_text_units: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sections: 64,
            max_palette_entries: 65536,
            max_properties: 65536,
            max_text_units: 2 << 20,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockState {
    pub name: String,
    pub properties: BTreeMap<String, String>,
}
impl BlockState {
    pub fn is_air(&self) -> bool {
        self.properties.is_empty()
            && matches!(
                self.name.as_str(),
                "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
            )
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
/// Eagerly validated palette storage. Private words and widths keep every
/// in-range lookup valid without allocating or normalizing block states.
pub struct Paletted<T> {
    palette: Vec<T>,
    words: Vec<u64>,
    bits: usize,
    count: usize,
}
impl<T> Paletted<T> {
    pub fn palette(&self) -> &[T] {
        &self.palette
    }
    pub fn value(&self, cell: usize) -> Option<&T> {
        if cell >= self.count {
            return None;
        }
        if self.bits == 0 {
            return self.palette.first();
        }
        let per_word = 64 / self.bits;
        let shift = (cell % per_word) * self.bits;
        let value = ((self.words[cell / per_word] >> shift) & ((1u64 << self.bits) - 1)) as usize;
        self.palette.get(value)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub block_states: Option<Paletted<BlockState>>,
    pub biomes: Option<Paletted<String>>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedChunk {
    pub identity: region::Identity,
    pub status: Option<String>,
    pub sections_present: bool,
    pub sections: BTreeMap<i8, Section>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockSample<'a> {
    State(&'a BlockState),
    MissingSection,
    MissingBlockStates,
    OutsideChunk,
    OutsideSectionRange,
}

/// Stored biome identity for renderer tint lookup. Tour attractions are
/// independently inferred from blocks, never from this metadata sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BiomeSample<'a> {
    Name(&'a str),
    MissingSectionList,
    MissingSection,
    MissingBiomes,
    OutsideChunk,
    OutsideSectionRange,
}
impl DecodedChunk {
    /// Byte-denominated logical admission charge for retained storage. This
    /// includes actual String/Vec capacities and a deliberately conservative
    /// sixteen-slot allowance per tree entry. std owns its node layout, so this
    /// is neither an exact portable heap-size API nor an allocator/RSS bound.
    /// Temporary NBT/decompression and caller-held extra Arcs are separate.
    pub(crate) fn storage_charge(&self, cancel: &dyn Fn() -> bool) -> DecodeResult<usize> {
        let mut total = std::mem::size_of::<Self>();
        add_storage(&mut total, self.status.as_ref().map_or(0, String::capacity))?;
        add_storage(
            &mut total,
            tree_storage::<i8, Section>(self.sections.len())?,
        )?;
        for section in self.sections.values() {
            check(cancel)?;
            if let Some(blocks) = &section.block_states {
                add_storage(&mut total, blocks.storage_charge()?)?;
                for state in &blocks.palette {
                    check(cancel)?;
                    add_storage(&mut total, state.name.capacity())?;
                    add_storage(
                        &mut total,
                        tree_storage::<String, String>(state.properties.len())?,
                    )?;
                    for (key, value) in &state.properties {
                        check(cancel)?;
                        add_storage(&mut total, key.capacity())?;
                        add_storage(&mut total, value.capacity())?;
                    }
                }
            }
            if let Some(biomes) = &section.biomes {
                add_storage(&mut total, biomes.storage_charge()?)?;
                for name in &biomes.palette {
                    check(cancel)?;
                    add_storage(&mut total, name.capacity())?;
                }
            }
        }
        check(cancel)?;
        Ok(total)
    }

    pub fn biome_at(&self, position: [i32; 3]) -> BiomeSample<'_> {
        let [x, y, z] = position;
        if [x.div_euclid(16), z.div_euclid(16)] != self.identity.position {
            return BiomeSample::OutsideChunk;
        }
        let Ok(section_y) = i8::try_from(y.div_euclid(16)) else {
            return BiomeSample::OutsideSectionRange;
        };
        if !self.sections_present {
            return BiomeSample::MissingSectionList;
        }
        let Some(section) = self.sections.get(&section_y) else {
            return BiomeSample::MissingSection;
        };
        let Some(biomes) = &section.biomes else {
            return BiomeSample::MissingBiomes;
        };
        // Each stored sample covers a 4x4x4 block cell, x fastest, then z, y.
        // Euclidean remainders preserve the same layout at negative coordinates.
        let cell =
            (y.rem_euclid(16) / 4 * 16 + z.rem_euclid(16) / 4 * 4 + x.rem_euclid(16) / 4) as usize;
        match biomes.value(cell) {
            Some(name) => BiomeSample::Name(name),
            None => BiomeSample::MissingBiomes,
        }
    }
    pub fn is_full(&self) -> bool {
        matches!(self.status.as_deref(), Some("full" | "minecraft:full"))
    }
    pub fn has_full_coverage(&self, min_y: i8, max_y: i8) -> bool {
        if min_y > max_y || !self.is_full() || !self.sections_present {
            return false;
        }
        (i16::from(min_y)..=i16::from(max_y)).all(|y| {
            self.sections
                .get(&(y as i8))
                .is_some_and(|s| s.block_states.is_some())
        })
    }
    pub fn block_at(&self, position: [i32; 3]) -> BlockSample<'_> {
        let [x, y, z] = position;
        if [x.div_euclid(16), z.div_euclid(16)] != self.identity.position {
            return BlockSample::OutsideChunk;
        }
        let Ok(section_y) = i8::try_from(y.div_euclid(16)) else {
            return BlockSample::OutsideSectionRange;
        };
        let Some(decoded_section) = self.sections.get(&section_y) else {
            return BlockSample::MissingSection;
        };
        let Some(blocks) = &decoded_section.block_states else {
            return BlockSample::MissingBlockStates;
        };
        let cell = (y.rem_euclid(16) * 256 + z.rem_euclid(16) * 16 + x.rem_euclid(16)) as usize;
        // Local remainders guarantee cell <4096; decoding validates every cell
        // before constructing the private block palette.
        BlockSample::State(
            blocks
                .value(cell)
                .expect("validated 4096-cell block palette"),
        )
    }
}
struct Budget {
    palettes: usize,
    properties: usize,
    units: usize,
}

fn add_storage(total: &mut usize, amount: usize) -> DecodeResult<()> {
    *total = total
        .checked_add(amount)
        .ok_or(Error::Limit("storage charge overflow"))?;
    Ok(())
}

fn tree_storage<K, V>(entries: usize) -> DecodeResult<usize> {
    let slot = std::mem::size_of::<K>()
        .checked_add(std::mem::size_of::<V>())
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<usize>()))
        .ok_or(Error::Limit("storage charge overflow"))?;
    entries
        .checked_mul(16)
        .and_then(|count| count.checked_mul(slot))
        .ok_or(Error::Limit("storage charge overflow"))
}

impl<T> Paletted<T> {
    fn storage_charge(&self) -> DecodeResult<usize> {
        let mut total = self
            .palette
            .capacity()
            .checked_mul(std::mem::size_of::<T>())
            .ok_or(Error::Limit("storage charge overflow"))?;
        add_storage(
            &mut total,
            self.words
                .capacity()
                .checked_mul(std::mem::size_of::<u64>())
                .ok_or(Error::Limit("storage charge overflow"))?,
        )?;
        Ok(total)
    }
}
fn charge(left: &mut usize, amount: usize, reason: &'static str) -> DecodeResult<()> {
    *left = left.checked_sub(amount).ok_or(Error::Limit(reason))?;
    Ok(())
}
fn check(cancel: &dyn Fn() -> bool) -> DecodeResult<()> {
    if cancel() {
        return Err(Error::Cancelled);
    }
    Ok(())
}
fn compound(tag: &Tag) -> DecodeResult<&Compound> {
    if let Tag::Compound(value) = tag {
        return Ok(value);
    }
    Err(Error::Invalid("expected compound"))
}
fn text(value: &Text, remaining: &mut Budget) -> DecodeResult<String> {
    charge(&mut remaining.units, value.0.len(), "text units")?;
    value
        .to_utf8()
        .map_err(|_| Error::Invalid("unpaired UTF-16 surrogate in state/status text"))
}
fn string(tag: &Tag, remaining: &mut Budget) -> DecodeResult<String> {
    let Tag::String(value) = tag else {
        return Err(Error::Invalid("expected string"));
    };
    text(value, remaining)
}
fn identifier(tag: &Tag, remaining: &mut Budget) -> DecodeResult<String> {
    let name = string(tag, remaining)?;
    let Some((namespace, path)) = name.split_once(':') else {
        return Err(Error::Invalid("identifier needs explicit namespace"));
    };
    if namespace.is_empty() || path.is_empty() || path.contains(':') {
        return Err(Error::Invalid("invalid namespace:path identity"));
    }
    Ok(name)
}
fn read_state(tag: &Tag, remaining: &mut Budget) -> DecodeResult<BlockState> {
    let fields = compound(tag)?;
    let name = identifier(
        nbt::get(fields, "Name").ok_or(Error::Invalid("state has no Name"))?,
        remaining,
    )?;
    let mut properties = BTreeMap::new();
    if let Some(tag) = nbt::get(fields, "Properties") {
        let fields = compound(tag)?;
        charge(&mut remaining.properties, fields.len(), "properties")?;
        for (key, value) in fields {
            properties.insert(text(key, remaining)?, string(value, remaining)?);
        }
    }
    Ok(BlockState { name, properties })
}
fn read_palette<T>(
    tag: &Tag,
    count: usize,
    min_bits: usize,
    kind: u8,
    remaining: &mut Budget,
    cancel: &dyn Fn() -> bool,
    read: fn(&Tag, &mut Budget) -> DecodeResult<T>,
) -> DecodeResult<Paletted<T>> {
    let fields = compound(tag)?;
    let Some(Tag::List {
        kind: actual_kind,
        values,
    }) = nbt::get(fields, "palette")
    else {
        return Err(Error::Invalid("missing or non-list palette"));
    };
    if *actual_kind != kind || values.is_empty() || values.len() > count {
        return Err(Error::Invalid("invalid palette kind or length"));
    }
    charge(&mut remaining.palettes, values.len(), "palette entries")?;
    let mut palette = Vec::new();
    palette
        .try_reserve_exact(values.len())
        .map_err(|_| Error::Limit("palette allocation"))?;
    for value in values {
        check(cancel)?;
        palette.push(read(value, remaining)?);
    }
    let data = match nbt::get(fields, "data") {
        None => None,
        Some(Tag::LongArray(words)) => Some(words.as_slice()),
        Some(_) => return Err(Error::Invalid("data is not a LongArray")),
    };
    if palette.len() == 1 && data.is_none_or(|words| words.is_empty()) {
        return Ok(Paletted {
            palette,
            words: Vec::new(),
            bits: 0,
            count,
        });
    }
    let bits = (usize::BITS - (palette.len() - 1).leading_zeros()) as usize;
    let bits = bits.max(min_bits);
    let per_word = 64 / bits;
    let expected = count.div_ceil(per_word);
    let data = data.ok_or(Error::Invalid("multi-entry palette has no data"))?;
    if data.len() != expected {
        return Err(Error::PackedLength {
            actual: data.len(),
            expected,
        });
    }
    let mask = (1u64 << bits) - 1;
    for cell in 0..count {
        if cell % 64 == 0 {
            check(cancel)?;
        }
        let value =
            (((data[cell / per_word] as u64) >> ((cell % per_word) * bits)) & mask) as usize;
        if value >= palette.len() {
            return Err(Error::PaletteIndex {
                cell,
                value,
                length: palette.len(),
            });
        }
    }
    let mut words = Vec::new();
    words
        .try_reserve_exact(data.len())
        .map_err(|_| Error::Limit("packed allocation"))?;
    words.extend(data.iter().map(|word| *word as u64));
    Ok(Paletted {
        palette,
        words,
        bits,
        count,
    })
}
pub fn decode(
    document: &Document,
    expected: [i32; 2],
    decode_limits: Limits,
    cancel: &dyn Fn() -> bool,
) -> DecodeResult<DecodedChunk> {
    check(cancel)?;
    let identity = region::verify_identity(document, expected)?;
    let (_, layout, body) = region::chunk_body(document)?;
    if decode_limits.max_sections > 256 {
        return Err(Error::Limit("section limit exceeds signed-byte domain"));
    }
    let mut remaining = Budget {
        palettes: decode_limits.max_palette_entries,
        properties: decode_limits.max_properties,
        units: decode_limits.max_text_units,
    };
    let status = nbt::get(body, "Status")
        .map(|tag| string(tag, &mut remaining))
        .transpose()?;
    let (key, wrong_key) = match layout {
        region::Layout::Level => ("Sections", "sections"),
        region::Layout::Root => ("sections", "Sections"),
    };
    if nbt::get(body, wrong_key).is_some() {
        return Err(Error::Invalid("wrong-case or ambiguous outer section list"));
    }
    let mut decoded = DecodedChunk {
        identity,
        status,
        sections_present: false,
        sections: BTreeMap::new(),
    };
    let Some(tag) = nbt::get(body, key) else {
        check(cancel)?;
        return Ok(decoded);
    };
    let Tag::List { kind, values } = tag else {
        return Err(Error::Invalid("sections is not a list"));
    };
    if *kind != 10 && !(*kind == 0 && values.is_empty()) {
        return Err(Error::Invalid("sections must contain compounds"));
    }
    if values.len() > decode_limits.max_sections {
        return Err(Error::Limit("sections"));
    }
    decoded.sections_present = true;
    for value in values {
        check(cancel)?;
        let fields = compound(value)?;
        let Some(Tag::Byte(y)) = nbt::get(fields, "Y") else {
            return Err(Error::Invalid("section Y must be a signed Byte"));
        };
        if decoded.sections.contains_key(y) {
            return Err(Error::Invalid("duplicate section Y"));
        }
        if ["Palette", "BlockStates"]
            .iter()
            .any(|key| nbt::get(fields, key).is_some())
        {
            return Err(Error::Invalid(
                "legacy section palette schema is outside this adapter",
            ));
        }
        let block_states = nbt::get(fields, "block_states")
            .map(|tag| read_palette(tag, 4096, 4, 10, &mut remaining, cancel, read_state))
            .transpose()?;
        let biomes = nbt::get(fields, "biomes")
            .map(|tag| read_palette(tag, 64, 1, 8, &mut remaining, cancel, identifier))
            .transpose()?;
        decoded.sections.insert(
            *y,
            Section {
                block_states,
                biomes,
            },
        );
    }
    check(cancel)?;
    Ok(decoded)
}

/// Check the persisted world-generation gate without decoding sections.
/// Projected rendering still runs the full decoder after this inexpensive
/// candidate filter has established that Minecraft marked the chunk complete.
pub fn has_full_generation_status(document: &Document, expected: [i32; 2]) -> DecodeResult<bool> {
    region::verify_identity(document, expected)?;
    let (_, _, body) = region::chunk_body(document)?;
    let Some(Tag::String(status)) = nbt::get(body, "Status") else {
        return Ok(false);
    };
    Ok(status.0 == Text::from("full").0 || status.0 == Text::from("minecraft:full").0)
}
