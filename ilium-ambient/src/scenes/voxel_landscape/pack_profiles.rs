//! Eleven explicit private-test full-pack selectors. Reviews are local research
//! records, never parsed from a pack's own metadata or claims about permission.
use super::assets::{
    error::{AssetError, Result},
    identity::{Label, ResourceId},
    review::{AssetPhase, FullPackReview, PackScope, SourceEdition},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackSourceKind {
    Java,
    ExtractedJavaArt,
    Bedrock,
}

#[derive(Clone, Copy, Debug)]
pub struct PackProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub release: &'static str,
    pub edition: SourceEdition,
    pub source_kind: PackSourceKind,
    pub author: &'static str,
    pub license: &'static str,
    pub restriction: &'static str,
    pub known_missing: &'static str,
    pub evidence: &'static str,
}

pub const FULL_PACKS: [PackProfile; 11] = [
    PackProfile { id:"jicklus", name:"Jicklus", release:"Java 201", edition:SourceEdition::Java,
        source_kind:PackSourceKind::Java, author:"Jicklus by Jack", license:"Author personal-use / all-rights-reserved source record",
        restriction:"Private temporary testing only; no public redistribution", known_missing:"Release-specific modern state/model coverage must be measured",
        evidence:"pack-research full-pack-profile-catalog; supplied Jicklus release 201" },
    PackProfile { id:"f8thful", name:"F8thful", release:"Java 2.3.0", edition:SourceEdition::Java,
        source_kind:PackSourceKind::Java, author:"F8thful authors", license:"All rights reserved source record",
        restriction:"Private temporary testing only", known_missing:"Release-specific state coverage must be measured",
        evidence:"pack-research full-pack-profile-catalog; supplied 2.3.0" },
    PackProfile { id:"whimscape", name:"Whimscape", release:"Java 26.1-26.3_r2", edition:SourceEdition::Java,
        source_kind:PackSourceKind::Java, author:"Whimscape author", license:"All rights reserved source record",
        restriction:"Author no-port clause retained; private test only, no distribution", known_missing:"Selected 26.3 coverage requires exact report",
        evidence:"pack-research full-pack-profile-catalog; supplied 26.1-26.3_r2" },
    PackProfile { id:"goodvibes", name:"GoodVibes / Acaitart", release:"VoxelAssets extracted subtree", edition:SourceEdition::ExtractedWorldArt,
        source_kind:PackSourceKind::ExtractedJavaArt, author:"Acaitart", license:"CC BY 4.0 source README",
        restriction:"Credit Acaitart; extracted art is not a ready Java pack", known_missing:"No authored blockstates/models/animation sidecars; explicit compatibility geometry and timing needed",
        evidence:"supplied GoodVibes tree and native component render 002" },
    PackProfile { id:"programmerart", name:"deathcap ProgrammerArt", release:"v3.0 / Minecraft 1.9", edition:SourceEdition::Java,
        source_kind:PackSourceKind::Java, author:"deathcap", license:"CC BY 4.0 source record",
        restriction:"Not Mojang built-in Programmer Art", known_missing:"Entity art absent; modern states/models need explicit compatibility",
        evidence:"pack-research full-pack-profile-catalog; supplied v3.0" },
    PackProfile { id:"textureless", name:"Textureless", release:"v115 / 32px LabPBR", edition:SourceEdition::Java,
        source_kind:PackSourceKind::Java, author:"Textureless author", license:"CC0 source record",
        restriction:"Optional official models add-on is an internal layer, not a selector", known_missing:"Water absent; normal/specular maps required for intended look",
        evidence:"pack-research full-pack-profile-catalog; supplied v115 plus model add-on" },
    PackProfile { id:"plasticator", name:"Plasticator", release:"Java 1.0 / Bedrock 2.4", edition:SourceEdition::Java,
        source_kind:PackSourceKind::Java, author:"Plasticator author", license:"MIT LICENSE.md; conflicting generic ARR footer retained",
        restriction:"Java and Bedrock sources require separately selected mount/evidence", known_missing:"Java entity PNGs absent; explicit Bedrock paths and models needed",
        evidence:"pack-research full-pack-profile-catalog; supplied Java and Bedrock variants" },
    PackProfile { id:"pixelperfectionce", name:"PixelPerfectionCE", release:"v4.2 beta1", edition:SourceEdition::Java,
        source_kind:PackSourceKind::Java, author:"XSSheep and community", license:"CC BY-SA 4.0 source record",
        restriction:"Retain author/community credit and share-alike record", known_missing:"Optifine features and modern states need explicit support reports",
        evidence:"pack-research full-pack-profile-catalog; supplied no-sounds nested archive" },
    PackProfile { id:"faithful32", name:"Faithful32", release:"September 2026 / 26.3", edition:SourceEdition::Java,
        source_kind:PackSourceKind::Java, author:"Faithful team", license:"Faithful License v4 2026-07-31",
        restriction:"Private temporary test; retain custom terms", known_missing:"No assumed source coverage without selected archive report",
        evidence:"pack-research full-pack-profile-catalog; supplied 26.3" },
    PackProfile { id:"faithful64", name:"Faithful64", release:"26.3 Release 15", edition:SourceEdition::Java,
        source_kind:PackSourceKind::Java, author:"Faithful team", license:"Faithful License v4 2026-07-31",
        restriction:"Private temporary test; retain custom terms", known_missing:"No assumed source coverage without selected archive report",
        evidence:"pack-research full-pack-profile-catalog; supplied release 15" },
    PackProfile { id:"antumbra", name:"Antumbra", release:"1.20-26.1-pre-1", edition:SourceEdition::Java,
        source_kind:PackSourceKind::Java, author:"Antumbra publisher", license:"MIT publisher declaration",
        restriction:"Private temporary test; preserve actual archive provenance", known_missing:"26.3 state/model coverage not established",
        evidence:"pack-research additional-collections/antumbra-inspected-0.json" },
];
impl PackProfile {
    pub fn review(self) -> Result<FullPackReview> {
        let label = |text: &str| Label::new(text);
        let review = FullPackReview {
            pack: ResourceId::parse(&format!("ilium-pack:{}", self.id))?,
            release: label(self.release)?,
            edition: self.edition,
            scope: PackScope::FullWorld,
            phase: AssetPhase::PrivateTestPlaceholder,
            evidence: label(self.evidence)?,
            author_credit: label(self.author)?,
            license_record: label(self.license)?,
            restrictions: vec![label(self.restriction)?],
            known_missing: vec![label(self.known_missing)?],
        };
        review.validate()?;
        Ok(review)
    }
}
pub fn profile(index: usize) -> Result<&'static PackProfile> {
    FULL_PACKS
        .get(index)
        .ok_or_else(|| AssetError::InvalidReview("unknown full-pack selection".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_eleven_reviews_are_full_pack_private_placeholders_with_distinct_ids() {
        let mut ids = std::collections::BTreeSet::new();
        for entry in FULL_PACKS {
            let review = entry.review().unwrap();
            assert_eq!(review.phase, AssetPhase::PrivateTestPlaceholder);
            assert!(ids.insert(review.pack));
        }
        assert_eq!(ids.len(), 11);
    }
}
