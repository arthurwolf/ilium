//! Synthetic-only decoder controls. No test opens a user's save or starts Minecraft.
use super::{
    chunk::{self, BlockSample, Error},
    nbt::{self, Compound, Document, Tag, Text},
    region,
};
use flate2::{
    write::{GzEncoder, ZlibEncoder},
    Compression,
};
use std::{cell::Cell, fs, io::Write, path::Path};
fn fields(entries: Vec<(&str, Tag)>) -> Compound {
    entries
        .into_iter()
        .map(|(key, value)| (Text::from(key), value))
        .collect()
}
fn string(value: &str) -> Tag {
    Tag::String(Text::from(value))
}
fn list(kind: u8, values: Vec<Tag>) -> Tag {
    Tag::List { kind, values }
}
fn state(name: &str, properties: Vec<(&str, &str)>) -> Tag {
    Tag::Compound(fields(vec![
        ("Name", string(name)),
        (
            "Properties",
            Tag::Compound(fields(
                properties
                    .into_iter()
                    .map(|(key, value)| (key, string(value)))
                    .collect(),
            )),
        ),
    ]))
}
fn container(palette: Vec<Tag>, kind: u8, data: Option<Tag>) -> Tag {
    let mut value = fields(vec![("palette", list(kind, palette))]);
    if let Some(data) = data {
        value.insert(Text::from("data"), data);
    }
    Tag::Compound(value)
}
fn section_tag(y: i8, blocks: Option<Tag>) -> Tag {
    let mut value = fields(vec![("Y", Tag::Byte(y))]);
    if let Some(blocks) = blocks {
        value.insert(Text::from("block_states"), blocks);
    }
    Tag::Compound(value)
}
fn singleton(name: &str, data: Option<Tag>) -> Tag {
    container(vec![state(name, vec![])], 10, data)
}
fn fixture(version: i32, sections: Vec<Tag>) -> Document {
    let wrapped = version <= 2836;
    let key = if wrapped { "Sections" } else { "sections" };
    let body = fields(vec![
        ("xPos", Tag::Int(-1)),
        ("zPos", Tag::Int(-2)),
        ("Status", string("full")),
        (key, list(10, sections)),
    ]);
    let mut root = if wrapped {
        fields(vec![("Level", Tag::Compound(body))])
    } else {
        body
    };
    root.insert(Text::from("DataVersion"), Tag::Int(version));
    Document {
        name: Text::from("synthetic"),
        root,
    }
}
fn compound_mut(tag: &mut Tag) -> &mut Compound {
    let Tag::Compound(fields) = tag else {
        panic!("fixture requires compound");
    };
    fields
}
fn body_mut(doc: &mut Document) -> &mut Compound {
    if doc.root.contains_key(&Text::from("Level")) {
        return compound_mut(doc.root.get_mut(&Text::from("Level")).unwrap());
    }
    &mut doc.root
}
fn decode(doc: &Document) -> Result<chunk::DecodedChunk, Error> {
    chunk::decode(doc, [-1, -2], chunk::Limits::default(), &|| false)
}
fn pack(values: &[usize], bits: usize) -> Vec<i64> {
    let mut result = Vec::new();
    let (mut word, mut shift) = (0u64, 0usize);
    for value in values {
        if shift + bits > 64 {
            if shift < 64 {
                word |= !0u64 << shift;
            }
            result.push(word as i64);
            word = 0;
            shift = 0;
        }
        word |= (*value as u64) << shift;
        shift += bits;
    }
    if shift != 0 {
        if shift < 64 {
            word |= !0u64 << shift;
        }
        result.push(word as i64);
    }
    result
}
fn palette(count: usize) -> Vec<Tag> {
    (0..count)
        .map(|i| state(&format!("fixture:block_{i}"), vec![]))
        .collect()
}
fn tag_kind(tag: &Tag) -> u8 {
    match tag {
        Tag::Byte(_) => 1,
        Tag::Short(_) => 2,
        Tag::Int(_) => 3,
        Tag::Long(_) => 4,
        Tag::FloatBits(_) => 5,
        Tag::DoubleBits(_) => 6,
        Tag::ByteArray(_) => 7,
        Tag::String(_) => 8,
        Tag::List { .. } => 9,
        Tag::Compound(_) => 10,
        Tag::IntArray(_) => 11,
        Tag::LongArray(_) => 12,
    }
}
fn put_text(out: &mut Vec<u8>, value: &Text) {
    let mut data = Vec::new();
    for unit in &value.0 {
        match *unit {
            1..=127 => data.push(*unit as u8),
            0..=2047 => {
                data.extend_from_slice(&[(0xc0 | (unit >> 6)) as u8, (0x80 | (unit & 63)) as u8])
            }
            _ => data.extend_from_slice(&[
                (0xe0 | (unit >> 12)) as u8,
                (0x80 | ((unit >> 6) & 63)) as u8,
                (0x80 | (unit & 63)) as u8,
            ]),
        }
    }
    out.extend_from_slice(&u16::try_from(data.len()).unwrap().to_be_bytes());
    out.extend(data);
}
fn put_payload(out: &mut Vec<u8>, tag: &Tag) {
    match tag {
        Tag::Byte(value) => out.push(*value as u8),
        Tag::Short(value) => out.extend_from_slice(&value.to_be_bytes()),
        Tag::Int(value) => out.extend_from_slice(&value.to_be_bytes()),
        Tag::Long(value) => out.extend_from_slice(&value.to_be_bytes()),
        Tag::FloatBits(value) => out.extend_from_slice(&value.to_be_bytes()),
        Tag::DoubleBits(value) => out.extend_from_slice(&value.to_be_bytes()),
        Tag::String(value) => put_text(out, value),
        Tag::ByteArray(values) => {
            out.extend_from_slice(&(values.len() as i32).to_be_bytes());
            out.extend(values.iter().map(|value| *value as u8));
        }
        Tag::IntArray(values) => {
            out.extend_from_slice(&(values.len() as i32).to_be_bytes());
            for value in values {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        Tag::LongArray(values) => {
            out.extend_from_slice(&(values.len() as i32).to_be_bytes());
            for value in values {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        Tag::List { kind, values } => {
            out.push(*kind);
            out.extend_from_slice(&(values.len() as i32).to_be_bytes());
            for value in values {
                put_payload(out, value);
            }
        }
        Tag::Compound(fields) => {
            for (name, value) in fields {
                out.push(tag_kind(value));
                put_text(out, name);
                put_payload(out, value);
            }
            out.push(0);
        }
    }
}
fn encode(doc: &Document) -> Vec<u8> {
    let mut out = vec![10];
    put_text(&mut out, &doc.name);
    put_payload(&mut out, &Tag::Compound(doc.root.clone()));
    out
}
fn compress(kind: u8, bytes: &[u8]) -> Vec<u8> {
    if kind == 3 {
        return bytes.to_vec();
    }
    if kind == 1 {
        let mut writer = GzEncoder::new(Vec::new(), Compression::default());
        writer.write_all(bytes).unwrap();
        return writer.finish().unwrap();
    }
    assert_eq!(kind, 2);
    let mut writer = ZlibEncoder::new(Vec::new(), Compression::default());
    writer.write_all(bytes).unwrap();
    writer.finish().unwrap()
}
fn save_records(directory: &Path, records: &[([i32; 2], u8, Vec<u8>, bool)]) -> std::path::PathBuf {
    let coordinates = region::region_of(records[0].0);
    let mut bytes = vec![0u8; 8192];
    for (position, kind, payload, external) in records {
        assert_eq!(region::region_of(*position), coordinates);
        let offset = bytes.len() / 4096;
        let length = if *external { 1 } else { payload.len() + 1 };
        let sectors = (length + 4).div_ceil(4096);
        assert!(sectors <= 255);
        let slot = region::slot(*position);
        bytes[slot * 4..slot * 4 + 4]
            .copy_from_slice(&(((offset as u32) << 8) | sectors as u32).to_be_bytes());
        bytes[4096 + slot * 4..4100 + slot * 4].copy_from_slice(&123u32.to_be_bytes());
        bytes.resize(bytes.len() + sectors * 4096, 0);
        let start = offset * 4096;
        bytes[start..start + 4].copy_from_slice(&(length as u32).to_be_bytes());
        bytes[start + 4] = *kind | if *external { 128 } else { 0 };
        if *external {
            fs::write(
                directory.join(format!("c.{}.{}.mcc", position[0], position[1])),
                payload,
            )
            .unwrap();
        } else {
            bytes[start + 5..start + 5 + payload.len()].copy_from_slice(payload);
        }
    }
    let path = directory.join(format!("r.{}.{}.mca", coordinates[0], coordinates[1]));
    fs::write(&path, bytes).unwrap();
    path
}
#[test]
fn observed_wrappers_preserve_names_properties_and_negative_coordinates() {
    for version in [2834, 2835, 2836, 3218] {
        let blocks = container(
            vec![state(
                "unknown_mod:machine",
                vec![("zeta", "true"), ("axis", "z")],
            )],
            10,
            None,
        );
        let original = fixture(
            version,
            vec![
                section_tag(-4, Some(blocks)),
                section_tag(19, Some(singleton("minecraft:stone", None))),
            ],
        );
        let parsed = nbt::parse(&encode(&original), nbt::Limits::default()).unwrap();
        let decoded = decode(&parsed).unwrap();
        assert_eq!(decoded.identity.data_version, version);
        assert_eq!(
            decoded.identity.layout,
            if version <= 2836 {
                region::Layout::Level
            } else {
                region::Layout::Root
            }
        );
        assert!(decoded.is_full());
        assert_eq!(
            decoded.sections.keys().copied().collect::<Vec<_>>(),
            vec![-4, 19]
        );
        for position in [[-1, -64, -17], [-16, -49, -32]] {
            let BlockSample::State(state) = decoded.block_at(position) else {
                panic!("missing exact state");
            };
            assert_eq!(state.name, "unknown_mod:machine");
            assert_eq!(
                state
                    .properties
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                vec!["axis", "zeta"]
            );
            assert_eq!(state.properties["axis"], "z");
            assert_eq!(state.properties["zeta"], "true");
        }
        assert!(
            matches!(decoded.block_at([-1, 319, -17]), BlockSample::State(state) if state.name == "minecraft:stone")
        );
        assert_eq!(decoded.block_at([0, -64, -17]), BlockSample::OutsideChunk);
    }
}
#[test]
fn padded_palettes_match_independent_all_cell_oracle() {
    for (count, bits) in [(16, 4), (17, 5), (33, 6)] {
        let values: Vec<_> = (0..4096).map(|cell| cell % count).collect();
        let words = pack(&values, bits);
        assert_eq!(
            words.len(),
            match bits {
                4 => 256,
                5 => 342,
                _ => 410,
            }
        );
        assert!(words.iter().any(|word| *word < 0));
        let doc = fixture(
            2835,
            vec![section_tag(
                -4,
                Some(container(palette(count), 10, Some(Tag::LongArray(words)))),
            )],
        );
        let decoded = decode(&doc).unwrap();
        for (cell, expected) in values.iter().enumerate() {
            let position = [
                -16 + (cell % 16) as i32,
                -64 + (cell / 256) as i32,
                -32 + ((cell / 16) % 16) as i32,
            ];
            assert!(
                matches!(decoded.block_at(position), BlockSample::State(state) if state.name == format!("fixture:block_{expected}")),
                "bits={bits}, cell={cell}"
            );
        }
    }
}
#[test]
fn literal_five_bit_word_boundary_ignores_padding() {
    let mut words = vec![0i64; 342];
    words[0] = (0xf000_0000_0000_0000u64 | 16 | (3u64 << 55)) as i64;
    words[1] = 7;
    let decoded = decode(&fixture(
        3218,
        vec![section_tag(
            -4,
            Some(container(palette(17), 10, Some(Tag::LongArray(words)))),
        )],
    ))
    .unwrap();
    let blocks = decoded.sections[&-4].block_states.as_ref().unwrap();
    for (cell, name) in [
        (0, "fixture:block_16"),
        (11, "fixture:block_3"),
        (12, "fixture:block_7"),
    ] {
        assert_eq!(blocks.value(cell).unwrap().name, name);
    }
    assert!(blocks.value(4096).is_none());
}
#[test]
fn rejects_invalid_indices_at_first_and_last_cells() {
    for cell in [0, 4095] {
        let mut values = vec![0; 4096];
        values[cell] = 31;
        let doc = fixture(
            3218,
            vec![section_tag(
                -4,
                Some(container(
                    palette(17),
                    10,
                    Some(Tag::LongArray(pack(&values, 5))),
                )),
            )],
        );
        assert!(
            matches!(decode(&doc), Err(Error::PaletteIndex { cell: actual, value: 31, length: 17 }) if actual == cell)
        );
    }
}
#[test]
fn singleton_forms_validate_redundant_data_and_preserve_air() {
    for data in [
        None,
        Some(Tag::LongArray(vec![])),
        Some(Tag::LongArray(vec![0; 256])),
    ] {
        for name in [
            "minecraft:air",
            "minecraft:cave_air",
            "minecraft:void_air",
            "minecraft:water",
            "mod:air",
        ] {
            let decoded = decode(&fixture(
                3218,
                vec![section_tag(-4, Some(singleton(name, data.clone())))],
            ))
            .unwrap();
            let BlockSample::State(state) = decoded.block_at([-1, -49, -17]) else {
                panic!("singleton missing");
            };
            assert_eq!(state.name, name);
            assert_eq!(
                state.is_air(),
                matches!(
                    name,
                    "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
                )
            );
        }
    }
    for data in [
        Tag::Int(0),
        Tag::LongArray(vec![0]),
        Tag::LongArray(vec![1; 256]),
    ] {
        assert!(decode(&fixture(
            3218,
            vec![section_tag(
                -4,
                Some(singleton("minecraft:air", Some(data)))
            )]
        ))
        .is_err());
    }
}
#[test]
fn rejects_missing_short_long_and_straddling_arrays() {
    for data in [
        None,
        Some(Tag::Int(0)),
        Some(Tag::LongArray(vec![0; 320])),
        Some(Tag::LongArray(vec![0; 341])),
        Some(Tag::LongArray(vec![0; 343])),
    ] {
        assert!(decode(&fixture(
            2836,
            vec![section_tag(-4, Some(container(palette(17), 10, data)))]
        ))
        .is_err());
    }
    for blocks in [
        container(vec![], 10, None),
        container(vec![string("minecraft:air")], 8, None),
        container(vec![Tag::Compound(Compound::new())], 10, None),
    ] {
        assert!(decode(&fixture(3218, vec![section_tag(-4, Some(blocks))])).is_err());
    }
}
#[test]
fn full_proto_unknown_and_missing_remain_distinct() {
    for status in [
        Some("full"),
        Some("minecraft:full"),
        Some("noise"),
        Some("minecraft:features"),
        Some("custom:full"),
        None,
    ] {
        let mut doc = fixture(
            3218,
            vec![
                section_tag(-4, Some(singleton("minecraft:air", None))),
                section_tag(-3, None),
            ],
        );
        body_mut(&mut doc).remove(&Text::from("Status"));
        if let Some(status) = status {
            body_mut(&mut doc).insert(Text::from("Status"), string(status));
        }
        let decoded = decode(&doc).unwrap();
        let full = matches!(status, Some("full" | "minecraft:full"));
        assert_eq!(
            chunk::has_full_generation_status(&doc, [-1, -2]).unwrap(),
            full
        );
        assert_eq!(decoded.status.as_deref(), status);
        assert_eq!(decoded.is_full(), full);
        assert_eq!(decoded.has_full_coverage(-4, -4), full);
        assert!(!decoded.has_full_coverage(-4, -3));
        assert!(!decoded.has_full_coverage(-4, 19));
        assert!(!decoded.has_full_coverage(19, -4));
        assert!(
            matches!(decoded.block_at([-1, -64, -17]), BlockSample::State(state) if state.is_air())
        );
        assert_eq!(
            decoded.block_at([-1, -48, -17]),
            BlockSample::MissingBlockStates
        );
        assert_eq!(
            decoded.block_at([-1, -32, -17]),
            BlockSample::MissingSection
        );
        assert_eq!(
            decoded.block_at([-1, 2048, -17]),
            BlockSample::OutsideSectionRange
        );
    }
    let mut doc = fixture(3218, vec![]);
    let present = decode(&doc).unwrap();
    body_mut(&mut doc).remove(&Text::from("sections"));
    let absent = decode(&doc).unwrap();
    assert!(present.sections_present);
    assert!(!absent.sections_present);
    assert!(!absent.has_full_coverage(-4, 19));
    body_mut(&mut doc).insert(Text::from("Status"), Tag::Int(1));
    assert!(decode(&doc).is_err());
}
#[test]
fn complete_signed_span_and_byte_extremes_are_addressable() {
    let sections = (-4..=19)
        .map(|y| section_tag(y, Some(singleton("minecraft:stone", None))))
        .collect();
    let decoded = decode(&fixture(2834, sections)).unwrap();
    assert!(decoded.has_full_coverage(-4, 19));
    assert!(!decoded.has_full_coverage(-5, 19));
    let decoded = decode(&fixture(
        3218,
        vec![
            section_tag(-128, Some(singleton("minecraft:stone", None))),
            section_tag(127, Some(singleton("minecraft:stone", None))),
        ],
    ))
    .unwrap();
    for y in [-2048, 2047] {
        assert!(matches!(
            decoded.block_at([-1, y, -17]),
            BlockSample::State(_)
        ));
    }
    assert_eq!(
        decoded.block_at([-1, -2049, -17]),
        BlockSample::OutsideSectionRange
    );
}
#[test]
fn biome_palettes_decode_separately_and_validate_indices() {
    let names = vec![
        string("minecraft:plains"),
        string("mod:other_biome"),
        string("minecraft:forest"),
    ];
    let values: Vec<_> = (0..64).map(|i| i % 3).collect();
    let mut value = section_tag(-4, None);
    compound_mut(&mut value).insert(
        Text::from("biomes"),
        container(names.clone(), 8, Some(Tag::LongArray(pack(&values, 2)))),
    );
    let decoded = decode(&fixture(2836, vec![value])).unwrap();
    let biomes = decoded.sections[&-4].biomes.as_ref().unwrap();
    for (cell, index) in values.iter().enumerate() {
        assert_eq!(
            biomes.value(cell).unwrap().as_str(),
            ["minecraft:plains", "mod:other_biome", "minecraft:forest"][*index]
        );
    }
    let mut value = section_tag(-4, None);
    compound_mut(&mut value).insert(
        Text::from("biomes"),
        container(names, 8, Some(Tag::LongArray(vec![-1; 2]))),
    );
    assert!(matches!(
        decode(&fixture(3218, vec![value])),
        Err(Error::PaletteIndex { length: 3, .. })
    ));
    let mut value = section_tag(-4, None);
    compound_mut(&mut value).insert(
        Text::from("biomes"),
        container(vec![string("mod:single_biome")], 8, None),
    );
    let decoded = decode(&fixture(3218, vec![value])).unwrap();
    assert_eq!(
        decoded.sections[&-4]
            .biomes
            .as_ref()
            .unwrap()
            .value(63)
            .unwrap(),
        "mod:single_biome"
    );
}
#[test]
fn malformed_state_identity_properties_and_surrogates_are_rejected() {
    for entry in [
        state("stone", vec![]),
        state(":stone", vec![]),
        state("mod:", vec![]),
        state("mod:a:b", vec![]),
    ] {
        assert!(decode(&fixture(
            3218,
            vec![section_tag(-4, Some(container(vec![entry], 10, None)))]
        ))
        .is_err());
    }
    for properties in [
        Tag::Int(1),
        Tag::Compound(fields(vec![("axis", Tag::Int(1))])),
        Tag::Compound(fields(vec![("axis", Tag::String(Text(vec![0xd800])))])),
    ] {
        let mut entry = state("mod:machine", vec![]);
        compound_mut(&mut entry).insert(Text::from("Properties"), properties);
        assert!(decode(&fixture(
            3218,
            vec![section_tag(-4, Some(container(vec![entry], 10, None)))]
        ))
        .is_err());
    }
}
#[test]
fn structural_corruption_is_not_missing_terrain() {
    for version in [2834, 3218] {
        let mut doc = fixture(version, vec![]);
        let (key, other) = if version == 2834 {
            ("Sections", "sections")
        } else {
            ("sections", "Sections")
        };
        let sections = body_mut(&mut doc).remove(&Text::from(key)).unwrap();
        body_mut(&mut doc).insert(Text::from(other), sections);
        assert!(decode(&doc).is_err());
    }
    let mut ambiguous = fixture(2834, vec![]);
    ambiguous
        .root
        .insert(Text::from("sections"), list(10, vec![]));
    assert!(decode(&ambiguous).is_err());
    let value = section_tag(-4, None);
    assert!(decode(&fixture(3218, vec![value.clone(), value])).is_err());
    for y in [None, Some(Tag::Int(-4))] {
        let mut value = Tag::Compound(Compound::new());
        if let Some(y) = y {
            compound_mut(&mut value).insert(Text::from("Y"), y);
        }
        assert!(decode(&fixture(3218, vec![value])).is_err());
    }
    let mut value = section_tag(-4, None);
    compound_mut(&mut value).insert(Text::from("Palette"), list(10, vec![]));
    assert!(decode(&fixture(2836, vec![value])).is_err());
}
#[test]
fn version_floor_ceiling_and_coordinate_identity_are_explicit() {
    for version in [0, 1631, 2230, 2529, 2833, 3219, i32::MAX] {
        assert!(
            matches!(decode(&fixture(version, vec![])), Err(Error::Region(region::Error::DataVersion(actual))) if actual == version)
        );
    }
    let doc = fixture(2835, vec![]);
    assert!(matches!(
        chunk::decode(&doc, [0, -2], chunk::Limits::default(), &|| false),
        Err(Error::Region(region::Error::Coordinates {
            expected: [0, -2],
            actual: [-1, -2]
        }))
    ));
    let mut doc = fixture(3218, vec![]);
    doc.root.remove(&Text::from("DataVersion"));
    assert!(decode(&doc).is_err());
    body_mut(&mut doc).insert(Text::from("DataVersion"), Tag::Long(3218));
    assert!(decode(&doc).is_err());
}
#[test]
fn chunk_budgets_and_cancellation_fail_closed() {
    let doc = fixture(
        3218,
        vec![section_tag(
            -4,
            Some(container(
                vec![state("mod:block", vec![("key", "value")])],
                10,
                None,
            )),
        )],
    );
    for read_limits in [
        chunk::Limits {
            max_sections: 0,
            ..Default::default()
        },
        chunk::Limits {
            max_palette_entries: 0,
            ..Default::default()
        },
        chunk::Limits {
            max_properties: 0,
            ..Default::default()
        },
        chunk::Limits {
            max_text_units: 0,
            ..Default::default()
        },
        chunk::Limits {
            max_sections: 257,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            chunk::decode(&doc, [-1, -2], read_limits, &|| false),
            Err(Error::Limit(_))
        ));
    }
    assert!(matches!(
        chunk::decode(&doc, [-1, -2], chunk::Limits::default(), &|| true),
        Err(Error::Cancelled)
    ));
    let doc = fixture(
        3218,
        vec![section_tag(
            -4,
            Some(container(
                palette(17),
                10,
                Some(Tag::LongArray(vec![0; 342])),
            )),
        )],
    );
    let calls = Cell::new(0);
    assert!(matches!(
        chunk::decode(&doc, [-1, -2], chunk::Limits::default(), &|| {
            calls.set(calls.get() + 1);
            calls.get() == 25
        }),
        Err(Error::Cancelled)
    ));
}
#[test]
fn nbt_mutf8_preserves_nul_non_bmp_and_unpaired_surrogates() {
    let bytes = [
        10, 0, 9, b'A', 0xc0, 0x80, 0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80, 0,
    ];
    let parsed = nbt::parse(&bytes, nbt::Limits::default()).unwrap();
    assert_eq!(parsed.name.to_utf8().unwrap(), "A\0\u{1f600}");
    let doc = Document {
        name: Text::from("world\0\u{1f600}"),
        root: fields(vec![
            ("LevelName", string("name\0\u{1f600}")),
            ("key\0\u{1f600}", string("value")),
        ]),
    };
    assert_eq!(
        nbt::parse(&encode(&doc), nbt::Limits::default()).unwrap(),
        doc
    );
    let parsed = nbt::parse(&[10, 0, 3, 0xed, 0xa0, 0x80, 0], nbt::Limits::default()).unwrap();
    assert_eq!(parsed.name.0, vec![0xd800]);
    assert!(parsed.name.to_utf8().is_err());
    for encoded in [
        vec![0],
        vec![0xf0, 0x9f, 0x98, 0x80],
        vec![0xc1, 0x81],
        vec![0xed, 0xa0],
        vec![0xc2, 0x20],
    ] {
        let mut bytes = vec![10, 0, encoded.len() as u8];
        bytes.extend(encoded);
        bytes.push(0);
        assert!(nbt::parse(&bytes, nbt::Limits::default()).is_err());
    }
}
#[test]
fn nbt_all_types_round_trip_and_truncation_is_rejected() {
    let doc = Document {
        name: Text::from("types"),
        root: fields(vec![
            ("byte", Tag::Byte(-128)),
            ("short", Tag::Short(-32768)),
            ("int", Tag::Int(i32::MIN)),
            ("long", Tag::Long(i64::MIN)),
            ("float", Tag::FloatBits(0xffc0_1234)),
            ("double", Tag::DoubleBits(0xfff8_0000_0000_1234)),
            ("bytes", Tag::ByteArray(vec![-128, -1, 0, 127])),
            ("ints", Tag::IntArray(vec![i32::MIN, -1, i32::MAX])),
            ("longs", Tag::LongArray(vec![i64::MIN, -1, i64::MAX])),
            ("text", string("x")),
            ("list", list(2, vec![Tag::Short(-2)])),
            ("empty", list(0, vec![])),
            ("compound", Tag::Compound(Compound::new())),
        ]),
    };
    let bytes = encode(&doc);
    assert_eq!(nbt::parse(&bytes, nbt::Limits::default()).unwrap(), doc);
    for end in 0..bytes.len() {
        assert!(
            nbt::parse(&bytes[..end], nbt::Limits::default()).is_err(),
            "cut={end}"
        );
    }
    let mut trailing = bytes;
    trailing.push(0);
    assert!(nbt::parse(&trailing, nbt::Limits::default()).is_err());
}
#[test]
fn nbt_malformed_structure_budgets_and_cancellation() {
    for bytes in [
        vec![10, 0, 0, 13, 0],
        vec![10, 0, 0, 7, 0, 0, 255, 255, 255, 255, 0],
        vec![10, 0, 0, 9, 0, 0, 0, 0, 0, 0, 1, 0],
        vec![10, 0, 0, 1, 0, 1, b'x', 1, 1, 0, 1, b'x', 2, 0],
    ] {
        assert!(nbt::parse(&bytes, nbt::Limits::default()).is_err());
    }
    let doc = Document {
        name: Text::from("root"),
        root: fields(vec![(
            "nested",
            Tag::Compound(fields(vec![("ints", Tag::IntArray(vec![1, 2]))])),
        )]),
    };
    let bytes = encode(&doc);
    for read_limits in [
        nbt::Limits {
            max_bytes: bytes.len() - 1,
            ..Default::default()
        },
        nbt::Limits {
            max_depth: 1,
            ..Default::default()
        },
        nbt::Limits {
            max_string_bytes: 1,
            ..Default::default()
        },
        nbt::Limits {
            max_collection_len: 1,
            ..Default::default()
        },
        nbt::Limits {
            max_nodes: 1,
            ..Default::default()
        },
        nbt::Limits {
            max_elements: 1,
            ..Default::default()
        },
        nbt::Limits {
            max_depth: 129,
            ..Default::default()
        },
    ] {
        assert!(nbt::parse(&bytes, read_limits).is_err());
    }
    assert_eq!(
        nbt::parse_checked(&bytes, nbt::Limits::default(), &|| true)
            .unwrap_err()
            .reason,
        "cancelled"
    );
}
#[test]
fn compression_modes_require_complete_bounded_streams() {
    let bytes = encode(&fixture(3218, vec![]));
    for (marker, kind) in [
        (1, region::Compression::Gzip),
        (2, region::Compression::Zlib),
        (3, region::Compression::Raw),
    ] {
        let encoded = compress(marker, &bytes);
        assert_eq!(
            region::decompress(kind, &encoded, bytes.len(), &|| false).unwrap(),
            bytes
        );
        assert!(matches!(
            region::decompress(kind, &encoded, bytes.len() - 1, &|| false),
            Err(region::Error::Limit(_))
        ));
        assert!(matches!(
            region::decompress(kind, &encoded, bytes.len(), &|| true),
            Err(region::Error::Cancelled)
        ));
        if marker == 3 {
            continue;
        }
        assert!(
            region::decompress(kind, &encoded[..encoded.len() - 1], bytes.len(), &|| false)
                .is_err()
        );
        let mut corrupt = encoded.clone();
        let last = corrupt.len() - 1;
        corrupt[last] ^= 0x80;
        assert!(region::decompress(kind, &corrupt, bytes.len(), &|| false).is_err());
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(region::decompress(kind, &trailing, bytes.len(), &|| false).is_err());
        let mut concatenated = encoded.clone();
        concatenated.extend(encoded);
        assert!(region::decompress(kind, &concatenated, bytes.len() * 2, &|| false).is_err());
    }
    let expanded = vec![0u8; 65536];
    let encoded = compress(2, &expanded);
    assert!(matches!(
        region::decompress(region::Compression::Zlib, &encoded, 1024, &|| false),
        Err(region::Error::Limit(_))
    ));
}
#[test]
fn region_modes_external_payloads_and_read_only_behavior() {
    let doc = fixture(
        2835,
        vec![section_tag(-4, Some(singleton("minecraft:stone", None)))],
    );
    let bytes = encode(&doc);
    for marker in [1, 2, 3] {
        for external in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let payload = compress(marker, &bytes);
            let path = save_records(
                directory.path(),
                &[([-1, -2], marker, payload.clone(), external)],
            );
            let before = fs::read(&path).unwrap();
            let index = region::read_index(directory.path(), [-1, -1], &|| false).unwrap();
            assert_eq!(
                index.entries[region::slot([-1, -2])].unwrap().timestamp,
                123
            );
            let loaded = region::read_chunk(
                directory.path(),
                [-1, -2],
                region::Limits::default(),
                &|| false,
            )
            .unwrap()
            .unwrap();
            assert_eq!(loaded.document, doc);
            assert_eq!(loaded.external, external);
            assert_eq!(loaded.identity.position, [-1, -2]);
            assert_eq!(loaded.timestamp, 123);
            assert!(decode(&loaded.document).unwrap().is_full());
            assert_eq!(fs::read(&path).unwrap(), before);
            if external {
                assert_eq!(
                    fs::read(directory.path().join("c.-1.-2.mcc")).unwrap(),
                    payload
                );
            }
            assert!(region::read_chunk(
                directory.path(),
                [-2, -2],
                region::Limits::default(),
                &|| false
            )
            .unwrap()
            .is_none());
            let read_limits = region::Limits {
                max_compressed_bytes: payload.len() - 1,
                ..Default::default()
            };
            assert!(matches!(
                region::read_chunk(directory.path(), [-1, -2], read_limits, &|| false),
                Err(region::Error::Limit(_))
            ));
        }
    }
}
#[test]
fn mixed_versions_and_protochunks_do_not_inherit_world_status() {
    let full = fixture(
        2835,
        vec![section_tag(-4, Some(singleton("minecraft:air", None)))],
    );
    let mut proto = fixture(2836, vec![]);
    body_mut(&mut proto).insert(Text::from("xPos"), Tag::Int(-2));
    body_mut(&mut proto).insert(Text::from("Status"), string("noise"));
    let directory = tempfile::tempdir().unwrap();
    save_records(
        directory.path(),
        &[
            ([-1, -2], 2, compress(2, &encode(&full)), false),
            ([-2, -2], 2, compress(2, &encode(&proto)), false),
        ],
    );
    for (position, version, full) in [([-1, -2], 2835, true), ([-2, -2], 2836, false)] {
        let loaded = region::read_chunk(
            directory.path(),
            position,
            region::Limits::default(),
            &|| false,
        )
        .unwrap()
        .unwrap();
        let decoded = chunk::decode(
            &loaded.document,
            position,
            chunk::Limits::default(),
            &|| false,
        )
        .unwrap();
        assert_eq!(decoded.identity.data_version, version);
        assert_eq!(decoded.is_full(), full);
        assert_eq!(decoded.has_full_coverage(-4, -4), full);
    }
}
#[test]
fn region_header_bounds_overlap_and_negative_slot_math() {
    assert_eq!(region::region_of([-1, -33]), [-1, -2]);
    assert_eq!(region::slot([-1, -33]), 1023);
    assert_eq!(region::slot([i32::MIN, i32::MIN]), 0);
    let mut header = [0u8; 8192];
    assert!(region::Index::parse(&header, 8192, [0, 0])
        .unwrap()
        .entries
        .iter()
        .all(Option::is_none));
    for len in [0, 8191, 8193] {
        assert!(region::Index::parse(&header, len, [0, 0]).is_err());
    }
    for location in [0x00000101u32, 0x00000200, 0x00000301] {
        header[..4].copy_from_slice(&location.to_be_bytes());
        assert!(region::Index::parse(&header, 12288, [0, 0]).is_err());
    }
    header[..4].copy_from_slice(&0x00000201u32.to_be_bytes());
    header[4..8].copy_from_slice(&0x00000201u32.to_be_bytes());
    assert!(region::Index::parse(&header, 12288, [0, 0]).is_err());
}
#[test]
fn region_record_failures_are_errors_not_absence() {
    let bytes = encode(&fixture(3218, vec![]));
    let directory = tempfile::tempdir().unwrap();
    for marker in [0, 4, 127] {
        save_records(
            directory.path(),
            &[([-1, -2], marker, bytes.clone(), false)],
        );
        assert!(
            matches!(region::read_chunk(directory.path(), [-1, -2], region::Limits::default(), &|| false), Err(region::Error::UnsupportedCompression(actual)) if actual == marker)
        );
    }
    let path = save_records(directory.path(), &[([-1, -2], 3, bytes.clone(), true)]);
    fs::remove_file(directory.path().join("c.-1.-2.mcc")).unwrap();
    assert!(matches!(
        region::read_chunk(
            directory.path(),
            [-1, -2],
            region::Limits::default(),
            &|| false
        ),
        Err(region::Error::Io(_))
    ));
    let mut malformed = fs::read(&path).unwrap();
    malformed[8192..8196].copy_from_slice(&2u32.to_be_bytes());
    fs::write(&path, malformed).unwrap();
    assert!(matches!(
        region::read_chunk(
            directory.path(),
            [-1, -2],
            region::Limits::default(),
            &|| false
        ),
        Err(region::Error::Invalid(_))
    ));
    for length in [0u32, 4093] {
        let path = save_records(directory.path(), &[([-1, -2], 3, bytes.clone(), false)]);
        let mut malformed = fs::read(&path).unwrap();
        malformed[8192..8196].copy_from_slice(&length.to_be_bytes());
        fs::write(path, malformed).unwrap();
        assert!(matches!(
            region::read_chunk(
                directory.path(),
                [-1, -2],
                region::Limits::default(),
                &|| false
            ),
            Err(region::Error::Invalid(_))
        ));
    }
    save_records(directory.path(), &[([-2, -2], 3, bytes, false)]);
    assert!(matches!(
        region::read_chunk(
            directory.path(),
            [-2, -2],
            region::Limits::default(),
            &|| false
        ),
        Err(region::Error::Coordinates {
            expected: [-2, -2],
            actual: [-1, -2]
        })
    ));
}
#[test]
fn region_detects_change_between_observations() {
    let directory = tempfile::tempdir().unwrap();
    let bytes = encode(&fixture(3218, vec![]));
    let path = save_records(directory.path(), &[([-1, -2], 3, bytes, false)]);
    let calls = Cell::new(0);
    let result = region::read_chunk(
        directory.path(),
        [-1, -2],
        region::Limits::default(),
        &|| {
            calls.set(calls.get() + 1);
            if calls.get() == 4 {
                let mut bytes = fs::read(&path).unwrap();
                let offset = 4096 + region::slot([-1, -2]) * 4;
                bytes[offset..offset + 4].copy_from_slice(&124u32.to_be_bytes());
                fs::write(&path, bytes).unwrap();
            }
            false
        },
    );
    assert!(calls.get() >= 4);
    assert!(matches!(result, Err(region::Error::Changed)));
}

#[test]
fn renderer_biome_samples_use_exact_four_cell_volume_coordinates() {
    use chunk::BiomeSample;
    for version in [2834, 2835, 2836, 3218] {
        let mut section = section_tag(-4, Some(singleton("minecraft:air", None)));
        compound_mut(&mut section).insert(
            Text::from("biomes"),
            container(
                (0..64)
                    .map(|index| string(&format!("fixture:biome_{index}")))
                    .collect(),
                8,
                Some(Tag::LongArray(pack(&(0..64).collect::<Vec<_>>(), 6))),
            ),
        );
        let decoded = decode(&fixture(version, vec![section])).unwrap();
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    let expected = format!("fixture:biome_{}", y / 4 * 16 + z / 4 * 4 + x / 4);
                    assert_eq!(
                        decoded.biome_at([-16 + x, -64 + y, -32 + z]),
                        BiomeSample::Name(&expected)
                    );
                }
            }
        }
        assert_eq!(decoded.biome_at([-17, -64, -32]), BiomeSample::OutsideChunk);
        assert_eq!(
            decoded.biome_at([-16, -65, -32]),
            BiomeSample::MissingSection
        );
        assert_eq!(
            decoded.biome_at([-16, i32::MIN, -32]),
            BiomeSample::OutsideSectionRange
        );
        let mut decoded = decoded;
        decoded.sections.get_mut(&-4).unwrap().biomes = None;
        assert_eq!(
            decoded.biome_at([-16, -64, -32]),
            BiomeSample::MissingBiomes
        );
        decoded.sections_present = false;
        assert_eq!(
            decoded.biome_at([-16, -64, -32]),
            BiomeSample::MissingSectionList
        );
    }
}
