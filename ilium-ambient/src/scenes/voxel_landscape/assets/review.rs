use super::error::{AssetError, Result};
use super::identity::{Digest256, Label, ResourceId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackScope {
    FullWorld,
    ItemOnly,
    LooseAssets,
    EnhancementOverlay,
    IncompleteDemo,
    Unreviewed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetPhase {
    PrivateTestPlaceholder,
    CustomArtist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceEdition {
    Java,
    Bedrock,
    ExtractedWorldArt,
    ArtistManifest,
}

/// Trusted LOCAL research configuration. A pack's own pack.mcmeta/manifest.json
/// MUST NOT be deserialized into this record or manufacture its scope/permission.
/// This preserves review evidence, it does not decide legal rights algorithmically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FullPackReview {
    pub pack: ResourceId,
    pub release: Label,
    pub edition: SourceEdition,
    pub scope: PackScope,
    pub phase: AssetPhase,
    pub evidence: Label,
    pub author_credit: Label,
    pub license_record: Label,
    pub restrictions: Vec<Label>,
    pub known_missing: Vec<Label>,
}
impl FullPackReview {
    pub fn validate(&self) -> Result<()> {
        if self.scope != PackScope::FullWorld {
            return Err(AssetError::InvalidReview(
                "source is not a reviewed full world-art pack".into(),
            ));
        }
        if self.restrictions.len() > 32 || self.known_missing.len() > 64 {
            return Err(AssetError::InvalidReview(
                "review exceeds bounded evidence inventory".into(),
            ));
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<Digest256> {
        self.validate()?;
        // Struct field order is stable; array order is deliberately meaningful.
        // This is not an interoperable arbitrary-JSON canonicalization scheme.
        let bytes = serde_json::to_vec(self)
            .map_err(|e| AssetError::InvalidReview(super::error::summary(&e.to_string())))?;
        Ok(Digest256::of(&bytes))
    }
}

#[cfg(test)]
pub(crate) fn fixture_review() -> FullPackReview {
    FullPackReview {
        pack: ResourceId::parse("fixture:world").unwrap(),
        release: Label::new("authored synthetic control; not a real selectable pack").unwrap(),
        edition: SourceEdition::ArtistManifest,
        scope: PackScope::FullWorld,
        phase: AssetPhase::PrivateTestPlaceholder,
        evidence: Label::new("synthetic test review, never application admission evidence")
            .unwrap(),
        author_credit: Label::new("consultation synthetic fixtures").unwrap(),
        license_record: Label::new("original fixture data").unwrap(),
        restrictions: vec![],
        known_missing: vec![],
    }
}
#[cfg(test)]
pub(crate) fn fixture_origin(kind: super::identity::OriginKind) -> super::identity::BlobOrigin {
    let review = fixture_review();
    super::identity::BlobOrigin {
        pack: review.pack.clone(),
        release: review.release.clone(),
        layer: Label::new("fixture layer").unwrap(),
        path: super::identity::AssetPath::parse("synthetic.png").unwrap(),
        review_digest: review.digest().unwrap(),
        kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excluded_sources_cannot_become_selectable_because_they_have_many_files() {
        for scope in [
            PackScope::ItemOnly,
            PackScope::LooseAssets,
            PackScope::EnhancementOverlay,
            PackScope::IncompleteDemo,
            PackScope::Unreviewed,
        ] {
            let mut review = fixture_review();
            review.scope = scope;
            assert!(review.validate().is_err());
        }
    }
    #[test]
    fn missing_entities_do_not_erase_full_base_art_scope_or_missing_evidence() {
        let mut review = fixture_review();
        review.known_missing.push(
            Label::new("entity textures absent; separate explicit full-pack fallback required")
                .unwrap(),
        );
        assert!(review.validate().is_ok());
        assert_ne!(review.digest().unwrap(), fixture_review().digest().unwrap());
        let roundtrip: FullPackReview =
            serde_json::from_str(&serde_json::to_string(&review).unwrap()).unwrap();
        assert_eq!(review, roundtrip);
    }
}
