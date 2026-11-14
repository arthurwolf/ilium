use super::budget::{ByteBudget, Cancel, Limits, Reservation};
use super::error::{AssetError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// A semantic identifier, never a filesystem path. Deserialization validates it
/// too, so a saved profile cannot bypass the same checks as an interactive one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ResourceId(String);
impl ResourceId {
    pub fn parse(value: &str) -> Result<Self> {
        if value.len() > 512 {
            return Err(AssetError::InvalidId("identifier exceeds 512 bytes".into()));
        }
        let canonical = if value.contains(':') {
            value.to_owned()
        } else {
            format!("minecraft:{value}")
        };
        if canonical.len() > 512 {
            return Err(AssetError::InvalidId("identifier exceeds 512 bytes".into()));
        }
        let Some((namespace, path)) = canonical.split_once(':') else {
            return Err(AssetError::InvalidId(canonical));
        };
        let atom =
            |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'.');
        if namespace.is_empty()
            || namespace == "."
            || namespace == ".."
            || !namespace.bytes().all(atom)
            || path.is_empty()
            || !path.bytes().all(|b| atom(b) || b == b'/')
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(AssetError::InvalidId(canonical));
        }
        Ok(Self(canonical))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn parts(&self) -> (&str, &str) {
        // Construction above always inserts exactly one colon. A checked split
        // still avoids panicking if a future refactor changes that invariant.
        self.0.split_once(':').unwrap_or(("", ""))
    }
}
impl TryFrom<String> for ResourceId {
    type Error = AssetError;
    fn try_from(s: String) -> Result<Self> {
        Self::parse(&s)
    }
}
impl From<ResourceId> for String {
    fn from(id: ResourceId) -> Self {
        id.0
    }
}
impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Relative, case-sensitive archive/mount path. It is NOT an authorization to
/// follow filesystem symlinks: the directory adapter must separately contain
/// the opened file within its admitted root. Empty roots use Option<AssetPath>.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AssetPath(String);
impl AssetPath {
    pub fn parse(value: &str) -> Result<Self> {
        if value.is_empty()
            || value.len() > 1024
            || value
                .chars()
                .any(|c| c.is_control() || matches!(c, '\\' | ':'))
            || value
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == ".." || p.len() > 255)
        {
            return Err(AssetError::InvalidPath(super::error::summary(value)));
        }
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn join(&self, child: &AssetPath) -> Result<Self> {
        Self::parse(&format!("{}/{}", self.0, child.0))
    }
}
impl TryFrom<String> for AssetPath {
    type Error = AssetError;
    fn try_from(s: String) -> Result<Self> {
        Self::parse(&s)
    }
}
impl From<AssetPath> for String {
    fn from(path: AssetPath) -> Self {
        path.0
    }
}
impl fmt::Display for AssetPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Label(String);
impl Label {
    pub fn new(value: &str) -> Result<Self> {
        if value.trim().is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
            return Err(AssetError::InvalidMetadata(
                "empty, control-bearing or oversized evidence label".into(),
            ));
        }
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for Label {
    type Error = AssetError;
    fn try_from(s: String) -> Result<Self> {
        Self::new(&s)
    }
}
impl From<Label> for String {
    fn from(label: Label) -> Self {
        label.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Digest256([u8; 32]);
impl Digest256 {
    pub fn of(bytes: &[u8]) -> Self {
        let mut result = [0; 32];
        result.copy_from_slice(&Sha256::digest(bytes));
        Self(result)
    }
    pub fn of_checked(bytes: &[u8], cancel: Cancel<'_>) -> Result<Self> {
        let mut hasher = Sha256::new();
        for chunk in bytes.chunks(64 * 1024) {
            cancel.check()?;
            hasher.update(chunk);
        }
        cancel.check()?;
        let mut result = [0; 32];
        result.copy_from_slice(&hasher.finalize());
        Ok(Self(result))
    }
    pub fn bytes(self) -> [u8; 32] {
        self.0
    }
}
impl TryFrom<String> for Digest256 {
    type Error = AssetError;
    fn try_from(s: String) -> Result<Self> {
        if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(AssetError::InvalidMetadata(
                "SHA-256 must contain 64 hexadecimal digits".into(),
            ));
        }
        let digit = |b: u8| {
            if b.is_ascii_digit() {
                b - b'0'
            } else {
                b.to_ascii_lowercase() - b'a' + 10
            }
        };
        let mut out = [0; 32];
        for (dst, pair) in out.iter_mut().zip(s.as_bytes().chunks_exact(2)) {
            *dst = digit(pair[0]) * 16 + digit(pair[1]);
        }
        Ok(Self(out))
    }
}
impl fmt::Display for Digest256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}
impl From<Digest256> for String {
    fn from(d: Digest256) -> Self {
        d.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginKind {
    SelectedPack,
    OfficialInternalLayer,
    ExplicitFullPackFallback,
    CustomArtist,
    DiagnosticFixture,
    OriginalCompatibilityGeometry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlobOrigin {
    pub pack: ResourceId,
    pub release: Label,
    pub layer: Label,
    pub path: AssetPath,
    /// Digest of the trusted local review record, not an author signature.
    pub review_digest: Digest256,
    pub kind: OriginKind,
}

/// The adapter supplies original bytes without any image normalization. The
/// original-file SHA-256 remains distinct from PixelImage's decoded RGBA hash.
/// Constructor checks do not replace a bounded read in the upstream adapter.
#[derive(Debug)]
pub struct SourceBlob {
    bytes: Vec<u8>,
    origin: BlobOrigin,
    digest: Digest256,
    _reservation: Reservation,
}
impl SourceBlob {
    /// Transfer an adapter-owned bounded buffer and its live byte reservation.
    pub(crate) fn from_reserved(
        bytes: Vec<u8>,
        origin: BlobOrigin,
        expected: Option<Digest256>,
        limits: &Limits,
        reservation: Reservation,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        limits.validate()?;
        cancel.check()?;
        if !reservation.belongs_to(budget) || reservation.bytes() < bytes.capacity() as u64 {
            return Err(AssetError::InvalidMetadata(
                "invalid transferred source reservation".into(),
            ));
        }
        if bytes.len() as u64 > limits.encoded_bytes {
            return Err(AssetError::Limit {
                resource: "encoded bytes",
                requested: bytes.len() as u64,
                limit: limits.encoded_bytes,
            });
        }
        let digest = Digest256::of_checked(&bytes, cancel)?;
        if let Some(expected) = expected {
            if digest != expected {
                return Err(AssetError::Integrity {
                    expected: expected.to_string(),
                    actual: digest.to_string(),
                });
            }
        }
        Ok(Self {
            bytes,
            origin,
            digest,
            _reservation: reservation,
        })
    }
    pub fn new(
        bytes: Vec<u8>,
        origin: BlobOrigin,
        expected: Option<Digest256>,
        limits: &Limits,
        budget: &ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        limits.validate()?;
        cancel.check()?;
        let count = bytes.capacity() as u64;
        if count > limits.encoded_bytes {
            return Err(AssetError::Limit {
                resource: "encoded bytes",
                requested: count,
                limit: limits.encoded_bytes,
            });
        }
        let reservation = budget.reserve(count, cancel)?;
        let digest = Digest256::of_checked(&bytes, cancel)?;
        if let Some(expected) = expected {
            if digest != expected {
                return Err(AssetError::Integrity {
                    expected: expected.to_string(),
                    actual: digest.to_string(),
                });
            }
        }
        Ok(Self {
            bytes,
            origin,
            digest,
            _reservation: reservation,
        })
    }
    pub(crate) fn uses_budget(&self, budget: &ByteBudget) -> bool {
        self._reservation.belongs_to(budget)
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn origin(&self) -> &BlobOrigin {
        &self.origin
    }
    pub fn digest(&self) -> Digest256 {
        self.digest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_are_canonical_but_not_global_legacy_aliases() {
        assert_eq!(
            ResourceId::parse("block/oak_log").unwrap().as_str(),
            "minecraft:block/oak_log"
        );
        assert_eq!(
            ResourceId::parse("artist:wood/poplar").unwrap().parts(),
            ("artist", "wood/poplar")
        );
        for value in [
            "../escape",
            "minecraft:/foo",
            "minecraft:a//b",
            "a:b:c",
            "minecraft:A",
            "",
            "a\\b",
            ".:block/a",
            "..:block/a",
        ] {
            assert!(ResourceId::parse(value).is_err(), "{value}");
        }
        assert_ne!(
            ResourceId::parse("block/stone").unwrap(),
            ResourceId::parse("blocks/stone").unwrap()
        );
    }
    #[test]
    fn paths_reject_traversal_absolute_and_platform_ambiguity() {
        for path in [
            "/etc/passwd",
            "C:/foo",
            "a/../b",
            "a/./b",
            "a//b",
            "a\\b",
            "a/",
            "a\0b",
        ] {
            assert!(AssetPath::parse(path).is_err(), "{path:?}");
        }
        assert!(
            AssetPath::parse("PixelPerfectionCE/assets/minecraft/textures/blocks/stone.png")
                .is_ok()
        );
    }
    #[test]
    fn deserialization_cannot_bypass_identifier_or_label_checks() {
        assert!(serde_json::from_str::<ResourceId>(r#""minecraft:../x""#).is_err());
        assert!(serde_json::from_str::<AssetPath>(r#""a/../x""#).is_err());
        assert!(serde_json::from_str::<Label>(r#""bad\u001b[31m""#).is_err());
        let id = ResourceId::parse("artist:poplar/log").unwrap();
        assert_eq!(
            serde_json::from_str::<ResourceId>(&serde_json::to_string(&id).unwrap()).unwrap(),
            id
        );
    }
    #[test]
    fn sha256_is_a_real_digest_not_a_noncryptographic_cache_key() {
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(Digest256::of(b"abc").to_string(), expected);
        assert_eq!(
            Digest256::try_from(expected.to_uppercase()).unwrap(),
            Digest256::of(b"abc")
        );
        assert!(Digest256::try_from(String::from("xyz")).is_err());
    }
}
