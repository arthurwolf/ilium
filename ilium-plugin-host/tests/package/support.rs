//! Shared helpers of the package-signing lane: a byte-level ZIP builder that can write every
//! hostile variation of reference-package-format.md, CRC-32 and SHA-256 (so the tests need no
//! extra dependency), the manual's test vectors, and small wrappers over the host API.
//! Every wait is bounded; nothing here sleeps.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use ilium_plugin_api::manifest::{CheckMode, Diagnostic};
use ilium_plugin_api::Severity;
use ilium_plugin_host::cli::{CliEnv, CliOutput};
use ilium_plugin_host::install::{CheckReport, Installer};
use ilium_plugin_host::package::read_package;
use ilium_plugin_host::registry::PackageStore;
use ilium_plugin_host::registry_client::{FetchError, Fetcher};

// ---------------------------------------------------------------------------------------------
// Hashes
// ---------------------------------------------------------------------------------------------

/// CRC-32 (IEEE, reflected) of `data`.
pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for index in 0..256u32 {
        let mut value = index;
        for _ in 0..8 {
            value = if value & 1 != 0 {
                0xEDB8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
        }
        table[index as usize] = value;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        crc = table[((crc ^ u32::from(*byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 of `data`.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut padded = data.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in padded.chunks(64) {
        let mut w = [0u32; 64];
        for index in 0..16 {
            w[index] = u32::from_be_bytes([
                block[index * 4],
                block[index * 4 + 1],
                block[index * 4 + 2],
                block[index * 4 + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let mut v = state;
        for index in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let choice = (v[4] & v[5]) ^ ((!v[4]) & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(choice)
                .wrapping_add(SHA256_K[index])
                .wrapping_add(w[index]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let majority = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(majority);
            v[7] = v[6];
            v[6] = v[5];
            v[5] = v[4];
            v[4] = v[3].wrapping_add(t1);
            v[3] = v[2];
            v[2] = v[1];
            v[1] = v[0];
            v[0] = t1.wrapping_add(t2);
        }
        for index in 0..8 {
            state[index] = state[index].wrapping_add(v[index]);
        }
    }
    let mut out = [0u8; 32];
    for index in 0..8 {
        out[index * 4..index * 4 + 4].copy_from_slice(&state[index].to_be_bytes());
    }
    out
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&sha256(data))
}

pub fn unhex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    digits
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("00"), 16).unwrap_or(0))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Manual vectors (reference-package-format.md "Test vector", reference-signing-and-trust.md)
// ---------------------------------------------------------------------------------------------

pub const VECTOR_COMPONENT_SHA256: &str =
    "028fb8ab3e154570597f3cc09ad59782ab44d9a6873f1834490da625ef3c6cd8";
pub const VECTOR_MANIFEST_SHA256: &str =
    "350b5e47c5781e72a9bb7b3d6630335fca0390402d0f625a48857a234f99c99d";
pub const VECTOR_UNSIGNED_SHA256: &str =
    "58aa1172e083701a96593f7531f21c0f3668e2f66c3166899f9b90ee3fe5616a";
pub const VECTOR_SIGNED_SHA256: &str =
    "d2238076eea48c58ceb4416dbb50438c6ffee23028051d7e9a0cb1f9ae7fc408";
pub const VECTOR_SEED_HEX: &str =
    "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
pub const VECTOR_PUBLIC_KEY_HEX: &str =
    "03a107bff3ce10be1d70dd18e74bc09967e4d6309ba50d5f1ddc8664125531b8";
pub const VECTOR_KEY_ID: &str = "56475aa75463474c0285df5dbf2bcab73da651358839e9b77481b2eab107708c";
pub const VECTOR_SIGNATURE_HEX: &str =
    "8efe737031228839637cdfa2cb79d92ed0a049c99b4cc2683afd0cdd7f6bd03f539b876f76a138de0a441005bf72a56877a8984008de211f04d3ebc5c04a6507";
pub const VECTOR_SIGNATURE_B64URL: &str =
    "jv5zcDEiiDljfN-iy3nZLtCgScmbTMJoOv0M3X9r0D9Tm4dvdqE43gpEEAW_cqVod6iYQAjeIR8E0-vFwEplBw";
pub const VECTOR_SIGNED_AT: &str = "2026-10-12 10:00:00";

pub fn vector_manifest() -> Vec<u8> {
    include_bytes!("golden/minimal_manifest.json").to_vec()
}

pub fn vector_component() -> Vec<u8> {
    unhex(include_str!("golden/minimal_component.hex"))
}

/// The unsigned 658-byte package of the manual.
pub fn vector_unsigned() -> Vec<u8> {
    unhex(include_str!("golden/minimal_unsigned.hex"))
}

/// The signed 1,114-byte package of the manual.
pub fn vector_signed() -> Vec<u8> {
    unhex(include_str!("golden/minimal_signed.hex"))
}

pub fn vector_signature_doc() -> Vec<u8> {
    include_bytes!("golden/minimal_signature.json").to_vec()
}

pub fn vector_seed() -> Vec<u8> {
    unhex(VECTOR_SEED_HEX)
}

pub fn vector_public_key() -> Vec<u8> {
    unhex(VECTOR_PUBLIC_KEY_HEX)
}

// ---------------------------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------------------------

pub const COMPONENT_PREAMBLE: [u8; 8] = [0x00, 0x61, 0x73, 0x6D, 0x0D, 0x00, 0x01, 0x00];
pub const CORE_MODULE_PREAMBLE: [u8; 8] = [0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];

/// One custom section (id 0) with a name and a payload; every size here is below 128.
pub fn custom_section(name: &str, payload: &[u8]) -> Vec<u8> {
    let mut body = vec![name.len() as u8];
    body.extend_from_slice(name.as_bytes());
    body.extend_from_slice(payload);
    let mut section = vec![0x00, body.len() as u8];
    section.extend_from_slice(&body);
    section
}

pub fn api_section(payload: &[u8]) -> Vec<u8> {
    custom_section("ilium:api-version", payload)
}

pub fn component_with(sections: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = COMPONENT_PREAMBLE.to_vec();
    for section in sections {
        bytes.extend_from_slice(section);
    }
    bytes
}

pub fn core_module_with(sections: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = CORE_MODULE_PREAMBLE.to_vec();
    for section in sections {
        bytes.extend_from_slice(section);
    }
    bytes
}

// ---------------------------------------------------------------------------------------------
// Manifest text in the serialisation of PKG-145
// ---------------------------------------------------------------------------------------------

/// One `files` item: path, length, lowercase SHA-256.
pub type Listing = (String, usize, String);

pub fn listing_of(files: &[(&str, &[u8])]) -> Vec<Listing> {
    files
        .iter()
        .map(|(path, bytes)| (path.to_string(), bytes.len(), sha256_hex(bytes)))
        .collect()
}

/// The packed manifest of the vector shape: one side, `extra_top` lines (already indented and
/// ending in a comma) before the side object, and `files` last.
pub fn manifest_text(extra_top: &str, side: &str, entry: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    manifest_from_listing(extra_top, side, entry, &listing_of(files))
}

pub fn manifest_from_listing(
    extra_top: &str,
    side: &str,
    entry: &str,
    files: &[Listing],
) -> Vec<u8> {
    let mut text = String::new();
    text.push_str("{\n  \"api\": \"ilium:plugin@0.1.0\",\n  \"id\": \"example.minimal\",\n");
    text.push_str(
        "  \"name\": \"Minimal\",\n  \"version\": \"0.1.0\",\n  \"publisher\": \"Example Labs\",\n",
    );
    text.push_str(extra_top);
    text.push_str(&format!(
        "  \"{side}\": {{\n    \"entry\": \"{entry}\",\n    \"world\": \"ilium:{side}/base@0.1.0\",\n    \"exports\": []\n  }},\n"
    ));
    if files.is_empty() {
        text.push_str("  \"files\": []\n}\n");
    } else {
        text.push_str("  \"files\": [\n");
        for (index, (path, length, digest)) in files.iter().enumerate() {
            text.push_str(&format!(
                "    {{\n      \"path\": \"{path}\",\n      \"length\": {length},\n      \"sha256\": \"{digest}\"\n    }}"
            ));
            text.push_str(if index + 1 == files.len() {
                "\n"
            } else {
                ",\n"
            });
        }
        text.push_str("  ]\n}\n");
    }
    text.into_bytes()
}

// ---------------------------------------------------------------------------------------------
// ZIP builder
// ---------------------------------------------------------------------------------------------

/// One archive entry with every header field that a hostile archive can vary.
#[derive(Clone, Debug)]
pub struct Z {
    pub name: Vec<u8>,
    pub data: Vec<u8>,
    pub method: u16,
    pub local_method: Option<u16>,
    pub flags: u16,
    pub local_flags: Option<u16>,
    pub time: u16,
    pub date: u16,
    pub made: u16,
    pub needed: u16,
    pub disk: u16,
    pub internal: u16,
    pub external: u32,
    pub local_extra: Vec<u8>,
    pub central_extra: Vec<u8>,
    pub comment: Vec<u8>,
    /// Central CRC override.
    pub crc: Option<u32>,
    /// Local CRC override.
    pub local_crc: Option<u32>,
    /// Central size override (compressed and uncompressed).
    pub size: Option<u32>,
    /// Local size override (compressed and uncompressed).
    pub local_size: Option<u32>,
    pub local_name: Option<Vec<u8>>,
    pub offset: Option<u32>,
    /// Bytes written between this entry's local record and the next.
    pub gap_after: Vec<u8>,
}

impl Z {
    pub fn new(name: &str, data: &[u8]) -> Z {
        Z::raw(name.as_bytes(), data)
    }

    pub fn raw(name: &[u8], data: &[u8]) -> Z {
        Z {
            name: name.to_vec(),
            data: data.to_vec(),
            method: 0,
            local_method: None,
            flags: 0x0800,
            local_flags: None,
            time: 0,
            date: 0x0021,
            made: 0x0314,
            needed: 20,
            disk: 0,
            internal: 0,
            external: 0x81A4_0000,
            local_extra: Vec::new(),
            central_extra: Vec::new(),
            comment: Vec::new(),
            crc: None,
            local_crc: None,
            size: None,
            local_size: None,
            local_name: None,
            offset: None,
            gap_after: Vec::new(),
        }
    }

    /// An entry whose central header claims `claimed` bytes while the stored data is tiny, so a
    /// size cap can be tested without allocating the cap (the reader checks caps from the
    /// central directory, PKG-126..132).
    pub fn claiming(name: &str, claimed: u32) -> Z {
        let mut entry = Z::new(name, b"x");
        entry.size = Some(claimed);
        entry
    }
}

/// Archive-level knobs.
#[derive(Clone, Debug, Default)]
pub struct Tail {
    pub comment: Vec<u8>,
    pub trailing: Vec<u8>,
    pub leading: Vec<u8>,
    pub count: Option<u16>,
    pub cd_size: Option<u32>,
    pub cd_offset: Option<u32>,
    /// Keep only the first half of the central directory but leave the end record unchanged.
    pub cut_cd: bool,
    /// Order in which the central headers are written (indexes into the entries).
    pub cd_order: Option<Vec<usize>>,
    /// Insert a ZIP64 end-record locator just before the end record.
    pub zip64_locator: bool,
    /// Bytes inserted between the last local record and the central directory.
    pub gap_before_cd: Vec<u8>,
}

fn le16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn le32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub fn build(entries: &[Z], tail: &Tail) -> Vec<u8> {
    let mut out = tail.leading.clone();
    let mut offsets = Vec::new();
    for entry in entries {
        offsets.push(out.len() as u32);
        let real_crc = crc32(&entry.data);
        let real_size = entry.data.len() as u32;
        let name = entry.local_name.as_ref().unwrap_or(&entry.name);
        le32(&mut out, 0x0403_4B50);
        le16(&mut out, entry.needed);
        le16(&mut out, entry.local_flags.unwrap_or(entry.flags));
        le16(&mut out, entry.local_method.unwrap_or(entry.method));
        le16(&mut out, entry.time);
        le16(&mut out, entry.date);
        le32(&mut out, entry.local_crc.unwrap_or(real_crc));
        let local_size = entry.local_size.unwrap_or(real_size);
        le32(&mut out, local_size);
        le32(&mut out, local_size);
        le16(&mut out, name.len() as u16);
        le16(&mut out, entry.local_extra.len() as u16);
        out.extend_from_slice(name);
        out.extend_from_slice(&entry.local_extra);
        out.extend_from_slice(&entry.data);
        out.extend_from_slice(&entry.gap_after);
    }
    out.extend_from_slice(&tail.gap_before_cd);
    let central_start = out.len() as u32;
    let order: Vec<usize> = tail
        .cd_order
        .clone()
        .unwrap_or_else(|| (0..entries.len()).collect());
    let mut central = Vec::new();
    for index in &order {
        let entry = &entries[*index];
        let real_crc = crc32(&entry.data);
        let real_size = entry.data.len() as u32;
        le32(&mut central, 0x0201_4B50);
        le16(&mut central, entry.made);
        le16(&mut central, entry.needed);
        le16(&mut central, entry.flags);
        le16(&mut central, entry.method);
        le16(&mut central, entry.time);
        le16(&mut central, entry.date);
        le32(&mut central, entry.crc.unwrap_or(real_crc));
        let size = entry.size.unwrap_or(real_size);
        le32(&mut central, size);
        le32(&mut central, size);
        le16(&mut central, entry.name.len() as u16);
        le16(&mut central, entry.central_extra.len() as u16);
        le16(&mut central, entry.comment.len() as u16);
        le16(&mut central, entry.disk);
        le16(&mut central, entry.internal);
        le32(&mut central, entry.external);
        le32(&mut central, entry.offset.unwrap_or(offsets[*index]));
        central.extend_from_slice(&entry.name);
        central.extend_from_slice(&entry.central_extra);
        central.extend_from_slice(&entry.comment);
    }
    let central_len = central.len() as u32;
    if tail.cut_cd {
        central.truncate(central.len() / 2);
    }
    out.extend_from_slice(&central);
    if tail.zip64_locator {
        le32(&mut out, 0x0706_4B50);
        out.extend_from_slice(&[0u8; 16]);
    }
    le32(&mut out, 0x0605_4B50);
    le16(&mut out, 0);
    le16(&mut out, 0);
    let count = tail.count.unwrap_or(entries.len() as u16);
    le16(&mut out, count);
    le16(&mut out, count);
    le32(&mut out, tail.cd_size.unwrap_or(central_len));
    le32(&mut out, tail.cd_offset.unwrap_or(central_start));
    le16(&mut out, tail.comment.len() as u16);
    out.extend_from_slice(&tail.comment);
    out.extend_from_slice(&tail.trailing);
    out
}

/// Builds a valid-shape archive from files already in archive order.
pub fn zip_of(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let entries: Vec<Z> = files
        .iter()
        .map(|(name, data)| Z::new(name, data))
        .collect();
    build(&entries, &Tail::default())
}

// ---------------------------------------------------------------------------------------------
// Reading what the builder (or the host) wrote, without using the host reader
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Loc {
    pub name: String,
    pub local_off: usize,
    pub central_off: usize,
    pub data_off: usize,
    pub len: usize,
}

fn rd16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn rd32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// Walks the central directory of a well-formed archive (the last 22 bytes are the end record).
pub fn layout(bytes: &[u8]) -> Vec<Loc> {
    let end = bytes.len() - 22;
    let count = rd16(bytes, end + 10) as usize;
    let mut position = rd32(bytes, end + 16) as usize;
    let mut locs = Vec::new();
    for _ in 0..count {
        let name_len = rd16(bytes, position + 28) as usize;
        let extra_len = rd16(bytes, position + 30) as usize;
        let comment_len = rd16(bytes, position + 32) as usize;
        let local_off = rd32(bytes, position + 42) as usize;
        let len = rd32(bytes, position + 24) as usize;
        let name =
            String::from_utf8_lossy(&bytes[position + 46..position + 46 + name_len]).to_string();
        let local_name_len = rd16(bytes, local_off + 26) as usize;
        let local_extra_len = rd16(bytes, local_off + 28) as usize;
        locs.push(Loc {
            name,
            local_off,
            central_off: position,
            data_off: local_off + 30 + local_name_len + local_extra_len,
            len,
        });
        position += 46 + name_len + extra_len + comment_len;
    }
    locs
}

pub fn names_of(bytes: &[u8]) -> Vec<String> {
    layout(bytes).into_iter().map(|loc| loc.name).collect()
}

pub fn data_of(bytes: &[u8], name: &str) -> Vec<u8> {
    let loc = layout(bytes)
        .into_iter()
        .find(|loc| loc.name == name)
        .unwrap_or_else(|| panic!("entry {name} not in archive"));
    bytes[loc.data_off..loc.data_off + loc.len].to_vec()
}

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// Changes one data byte of an entry; with `repair_crc` both headers get the new CRC-32.
pub fn patch_data(
    bytes: &[u8],
    name: &str,
    index: usize,
    new_value: u8,
    repair_crc: bool,
) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let loc = layout(bytes)
        .into_iter()
        .find(|loc| loc.name == name)
        .unwrap_or_else(|| panic!("entry {name} not in archive"));
    out[loc.data_off + index] = new_value;
    if repair_crc {
        let crc = crc32(&out[loc.data_off..loc.data_off + loc.len]);
        put32(&mut out, loc.local_off + 14, crc);
        put32(&mut out, loc.central_off + 16, crc);
    }
    out
}

/// Header fields of one entry as written in the central directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CentralFields {
    pub made: u16,
    pub needed: u16,
    pub flags: u16,
    pub method: u16,
    pub time: u16,
    pub date: u16,
    pub extra_len: u16,
    pub comment_len: u16,
    pub disk: u16,
    pub internal: u16,
    pub external: u32,
    pub local_flags: u16,
    pub local_needed: u16,
    pub local_extra_len: u16,
}

pub fn central_fields(bytes: &[u8]) -> Vec<CentralFields> {
    layout(bytes)
        .into_iter()
        .map(|loc| CentralFields {
            made: rd16(bytes, loc.central_off + 4),
            needed: rd16(bytes, loc.central_off + 6),
            flags: rd16(bytes, loc.central_off + 8),
            method: rd16(bytes, loc.central_off + 10),
            time: rd16(bytes, loc.central_off + 12),
            date: rd16(bytes, loc.central_off + 14),
            extra_len: rd16(bytes, loc.central_off + 30),
            comment_len: rd16(bytes, loc.central_off + 32),
            disk: rd16(bytes, loc.central_off + 34),
            internal: rd16(bytes, loc.central_off + 36),
            external: rd32(bytes, loc.central_off + 38),
            local_flags: rd16(bytes, loc.local_off + 6),
            local_needed: rd16(bytes, loc.local_off + 4),
            local_extra_len: rd16(bytes, loc.local_off + 28),
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Packages built from the vector
// ---------------------------------------------------------------------------------------------

/// The vector files in archive order.
pub fn vector_entries() -> Vec<(String, Vec<u8>)> {
    vec![
        ("manifest.json".to_string(), vector_manifest()),
        ("server.wasm".to_string(), vector_component()),
    ]
}

/// The vector with extra entries added at their sorted position. The extras are listed in the
/// manifest `files` with correct length and hash, so only the extras' own path and location
/// rules can fail.
pub fn package_with(extras: &[(&str, &[u8])]) -> Vec<u8> {
    let component = vector_component();
    let mut listed: Vec<(&str, &[u8])> = vec![("server.wasm", component.as_slice())];
    listed.extend_from_slice(extras);
    listed.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    let manifest = manifest_text("", "server", "server.wasm", &listed);
    let mut all: Vec<(&str, &[u8])> = listed.clone();
    all.push(("manifest.json", manifest.as_slice()));
    all.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    let entries: Vec<Z> = all.iter().map(|(name, data)| Z::new(name, data)).collect();
    build(&entries, &Tail::default())
}

/// The vector with a replacement `server.wasm`; `files` and the CRC-32 are rebuilt (the
/// "rebuild with pack" rule of PKG-177..180).
pub fn package_with_component(component: &[u8]) -> Vec<u8> {
    let manifest = manifest_text("", "server", "server.wasm", &[("server.wasm", component)]);
    zip_of(&[
        ("manifest.json", manifest),
        ("server.wasm", component.to_vec()),
    ])
}

/// The vector plus entries added at their sorted position without listing them in `files`
/// (the hostile matrix rule: "The entry is not listed in `files` unless the row says so").
pub fn vector_plus_unlisted(extra: Vec<Z>) -> Vec<u8> {
    let mut entries = vec![
        Z::new("manifest.json", &vector_manifest()),
        Z::new("server.wasm", &vector_component()),
    ];
    entries.extend(extra);
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    build(&entries, &Tail::default())
}

// ---------------------------------------------------------------------------------------------
// Host API wrappers
// ---------------------------------------------------------------------------------------------

pub fn error_codes(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .map(|diagnostic| diagnostic.code.as_str().to_string())
        .collect()
}

pub fn warning_diagnostics(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Warning)
        .collect()
}

/// Error codes of `read_package`; empty when the archive reads.
pub fn read_codes(bytes: &[u8]) -> Vec<String> {
    match read_package(bytes) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => error_codes(&diagnostics),
    }
}

/// The `package.*` error codes of `read_package`, in print order.
pub fn stage1_codes(bytes: &[u8]) -> Vec<String> {
    read_codes(bytes)
        .into_iter()
        .filter(|code| code.starts_with("package."))
        .collect()
}

pub fn installer() -> (tempfile::TempDir, Installer) {
    let dir = tempfile::tempdir().expect("temporary store directory");
    let installer = Installer::new(PackageStore {
        root: dir.path().to_path_buf(),
    });
    (dir, installer)
}

pub fn check(bytes: &[u8]) -> CheckReport {
    let (_dir, installer) = installer();
    installer.check(bytes, CheckMode::Install)
}

/// Error codes of the install check, in print order.
pub fn check_codes(bytes: &[u8]) -> Vec<String> {
    error_codes(&check(bytes).diagnostics)
}

/// The `package.*` and `manifest.*` error codes of the check.
pub fn check_package_codes(bytes: &[u8]) -> Vec<String> {
    check_codes(bytes)
        .into_iter()
        .filter(|code| code.starts_with("package."))
        .collect()
}

pub fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

// ---------------------------------------------------------------------------------------------
// CLI wrappers
// ---------------------------------------------------------------------------------------------

/// A fetcher that counts calls; offline commands must never call it.
pub struct CountingFetcher(pub AtomicUsize);

impl Fetcher for CountingFetcher {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(FetchError::Unreachable)
    }
}

pub struct CliFixture {
    pub root: tempfile::TempDir,
    pub env: CliEnv,
    pub net: Arc<CountingFetcher>,
}

impl CliFixture {
    pub fn work(&self) -> PathBuf {
        self.root.path().join("work")
    }
    pub fn state(&self) -> PathBuf {
        self.root.path().join("state")
    }
    pub fn network_calls(&self) -> usize {
        self.net.0.load(Ordering::SeqCst)
    }
}

pub fn cli_fixture() -> CliFixture {
    let root = tempfile::tempdir().expect("temporary cli directory");
    for name in ["data", "state", "config", "work"] {
        std::fs::create_dir_all(root.path().join(name)).expect("create cli directory");
    }
    let net = Arc::new(CountingFetcher(AtomicUsize::new(0)));
    let env = CliEnv::new(
        root.path().join("data"),
        root.path().join("state"),
        root.path().join("config"),
        root.path().join("work"),
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_791_800_000),
        true,
        net.clone(),
    );
    CliFixture { root, env, net }
}

pub fn run_cli(fixture: &CliFixture, args: &[&str]) -> CliOutput {
    let owned: Vec<String> = args.iter().map(|argument| argument.to_string()).collect();
    ilium_plugin_host::cli::run(&owned, &fixture.env)
}

pub fn records<'a>(output: &'a CliOutput, kind: &str) -> Vec<&'a serde_json::Value> {
    output
        .stdout_jsonl
        .iter()
        .filter(|record| record.get("type").and_then(|value| value.as_str()) == Some(kind))
        .collect()
}

pub fn record_codes(output: &CliOutput, kind: &str) -> Vec<String> {
    records(output, kind)
        .iter()
        .filter_map(|record| record.get("code").and_then(|value| value.as_str()))
        .map(|code| code.to_string())
        .collect()
}

pub fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent directory");
    }
    std::fs::write(path, bytes).expect("write file");
}

/// Writes the trust store with the vector key. The location is not fixed by the CLI page for an
/// injected state directory, so the file is written where either reading would find it.
pub fn write_trust_store(fixture: &CliFixture, status: &str, revoked_json: &str) {
    let document = format!(
        "{{\n  \"format\": \"ilium-trust/1\",\n  \"keys\": [\n    {{\n      \"key_id\": \"{VECTOR_KEY_ID}\",\n      \"publisher_id\": \"example\",\n      \"publisher\": \"Example Labs\",\n      \"public_key\": \"A6EHv_POEL4dcN0Y50vAmWfk1jCbpQ1fHdyGZBJVMbg\",\n      \"added\": \"2026-10-12 10:05:00\",\n      \"added_by\": \"user\",\n      \"status\": \"{status}\",\n      \"revoked\": {revoked_json}\n    }}\n  ]\n}}\n"
    );
    write_file(&fixture.state().join("trust.json"), document.as_bytes());
    write_file(
        &fixture.state().join("ilium").join("trust.json"),
        document.as_bytes(),
    );
}

/// A pack project: the source manifest of the vector (no `files`), the built component in
/// `dist/` and a `dist/build.json`. The input hash is a placeholder (see API_GAPS.md).
pub fn write_pack_project(dir: &Path) {
    let source = String::from_utf8(vector_manifest()).expect("vector manifest is UTF-8");
    let cut = source
        .find(",\n  \"files\"")
        .expect("vector manifest has files");
    let source_manifest = format!("{}\n}}\n", &source[..cut]);
    write_file(&dir.join("manifest.json"), source_manifest.as_bytes());
    write_file(&dir.join("dist").join("server.wasm"), &vector_component());
    let build_json = format!(
        "{{\"profile\":\"release\",\"api\":\"ilium:plugin@0.1.0\",\"input_hash\":\"{}\",\"sides\":[{{\"name\":\"server\",\"bytes\":34,\"sha256\":\"{VECTOR_COMPONENT_SHA256}\"}}],\"toolchain\":{{}},\"cached\":false}}\n",
        "0".repeat(64)
    );
    write_file(&dir.join("dist").join("build.json"), build_json.as_bytes());
}
