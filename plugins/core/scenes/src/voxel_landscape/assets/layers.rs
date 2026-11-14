//! Explicit mounts and cumulative Java overlays. All matching entries are applied
//! in authored order; the highest matching layer wins independently per resource.
use super::{
    budget::{ByteBudget, Cancel, Limits, Reservation},
    error::{AssetError, Result},
    identity::{AssetPath, BlobOrigin, Digest256, Label, OriginKind, ResourceId, SourceBlob},
    metadata::{self, array, object, required, text, uint, Document},
    review::FullPackReview,
    source::{check_limit, AssetSource},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::sync::Arc;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct PackFormat {
    pub major: u32,
    pub minor: u32,
}
impl PackFormat {
    pub fn new(major: u32, minor: u32) -> Result<Self> {
        if major > i32::MAX as u32 || minor > i32::MAX as u32 {
            return Err(metadata::invalid(
                "pack format exceeds signed format domain",
            ));
        }
        Ok(Self { major, minor })
    }
    fn endpoint(value: &Value, upper: bool) -> Result<Self> {
        let numbers = match value {
            Value::Array(values) => {
                if values.is_empty() || values.len() > 2 {
                    return Err(metadata::invalid("format tuple needs one or two integers"));
                }
                values.iter().map(uint).collect::<Result<Vec<_>>>()?
            }
            _ => vec![uint(value)?],
        };
        Self::new(
            numbers[0],
            *numbers
                .get(1)
                .unwrap_or(&if upper { i32::MAX as u32 } else { 0 }),
        )
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FormatRange {
    pub minimum: PackFormat,
    pub maximum: PackFormat,
}
impl FormatRange {
    fn new(minimum: PackFormat, maximum: PackFormat) -> Result<Self> {
        if minimum > maximum {
            return Err(metadata::invalid("reversed pack format interval"));
        }
        Ok(Self { minimum, maximum })
    }
    pub fn contains(self, target: PackFormat) -> bool {
        target >= self.minimum && target <= self.maximum
    }
    fn legacy(value: &Value) -> Result<Self> {
        match value {
            Value::Number(_) => Self::new(
                PackFormat::endpoint(value, false)?,
                PackFormat::endpoint(value, true)?,
            ),
            Value::Array(values) if values.len() == 2 => {
                let low = uint(&values[0])?;
                let high = uint(&values[1])?;
                Self::new(
                    PackFormat::new(low, 0)?,
                    PackFormat::new(high, i32::MAX as u32)?,
                )
            }
            Value::Object(values) => Self::new(
                PackFormat::endpoint(required(values, "min_inclusive")?, false)?,
                PackFormat::endpoint(required(values, "max_inclusive")?, true)?,
            ),
            _ => Err(metadata::invalid(
                "invalid legacy supported_formats/formats interval",
            )),
        }
    }
    fn fields(fields: &Map<String, Value>, legacy: &str, pack_fallback: bool) -> Result<Self> {
        let old = fields.get(legacy).map(Self::legacy).transpose()?;
        match (fields.get("min_format"), fields.get("max_format")) {
            (Some(minimum), Some(maximum)) => {
                // Modern tuples are authoritative; legacy fields remain in the original document.
                Self::new(
                    PackFormat::endpoint(minimum, false)?,
                    PackFormat::endpoint(maximum, true)?,
                )
            }
            (None, None) => {
                if let Some(old) = old {
                    return Ok(old);
                }
                if pack_fallback {
                    return Self::legacy(required(fields, "pack_format")?);
                }
                Err(metadata::invalid("overlay has no format interval")) // Preserve the explicit failure instead of treating it as absence.
            }
            _ => Err(metadata::invalid(
                "both min_format and max_format are required together",
            )),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct OverlayEntry {
    pub directory: AssetPath,
    pub formats: FormatRange,
    pub authored_index: usize,
}
#[derive(Debug)]
pub struct JavaPackMetadata {
    pub formats: FormatRange,
    pub overlays: Vec<OverlayEntry>,
    pub source: Document,
}
impl JavaPackMetadata {
    pub fn parse(source: Document) -> Result<Self> {
        let root = object(&source.value)?;
        let pack = object(required(root, "pack")?)?;
        let formats = FormatRange::fields(pack, "supported_formats", true)?;
        let mut overlays = Vec::new();
        if let Some(value) = root.get("overlays") {
            let entries = array(required(object(value)?, "entries")?)?;
            check_limit("pack overlays", entries.len() as u64, 256)?;
            for (authored_index, entry) in entries.iter().enumerate() {
                let fields = object(entry)?;
                overlays.push(OverlayEntry {
                    directory: AssetPath::parse(text(required(fields, "directory")?)?)?,
                    formats: FormatRange::fields(fields, "formats", false)?,
                    authored_index,
                });
            }
        }
        Ok(Self {
            formats,
            overlays,
            source,
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MountLayout {
    Java,
    ExtractedJava,
    Bedrock,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum ResourceKind {
    Texture,
    TextureMetadata,
    Model,
    Blockstate,
}
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ResourceKey {
    pub kind: ResourceKind,
    pub id: ResourceId,
}
impl ResourceKey {
    pub fn relative_path(&self, layout: MountLayout) -> Result<AssetPath> {
        if layout == MountLayout::Bedrock {
            return Err(AssetError::Unsupported(
                "Bedrock needs an explicit edition/resource mapping".into(),
            ));
        }
        let (namespace, path) = self.id.parts();
        let (directory, suffix) = match self.kind {
            ResourceKind::Texture => ("textures", ".png"),
            ResourceKind::TextureMetadata => ("textures", ".png.mcmeta"),
            ResourceKind::Model => ("models", ".json"),
            ResourceKind::Blockstate => ("blockstates", ".json"),
        };
        let prefix = if layout == MountLayout::Java {
            "assets/"
        } else {
            ""
        };
        AssetPath::parse(&format!("{prefix}{namespace}/{directory}/{path}{suffix}"))
    }
}
struct MountedLayer {
    source: Arc<dyn AssetSource>,
    root: Option<AssetPath>,
    label: Label,
    kind: OriginKind,
}
#[derive(Clone, Debug, Serialize)]
pub struct ResolutionAttempt {
    pub layer: Label,
    pub path: AssetPath,
    pub found: bool,
    pub source_sha256: Option<Digest256>,
}
#[derive(Debug)]
pub struct Resolution {
    pub blob: Option<SourceBlob>,
    pub requested: AssetPath,
    pub target: Option<PackFormat>,
    pub attempts: Vec<ResolutionAttempt>,
    pub(crate) reservations: Vec<Reservation>,
}
/// Mount rules are trusted local configuration, not declarations of ownership.
pub struct LayeredPack {
    review: FullPackReview,
    review_digest: Digest256,
    layers: Vec<MountedLayer>,
    layout: MountLayout,
    target: Option<PackFormat>,
    compatibility: Option<Label>,
    internal_compatibility: Vec<(Label, Label)>,
    metadata: Vec<JavaPackMetadata>,
    skipped_empty_overlays: Vec<Label>,
    budget: ByteBudget,
    limits: Limits,
    reservations: Vec<Reservation>,
    #[cfg(test)]
    fixture_members: Option<std::collections::BTreeMap<AssetPath, Vec<u8>>>,
    #[cfg(test)]
    fixture_member_reads: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    fixture_stop_after_reads: Option<(Arc<std::sync::atomic::AtomicBool>, usize)>,
}
pub fn join_root(root: Option<&AssetPath>, path: &AssetPath) -> Result<AssetPath> {
    match root {
        Some(root) => root.join(path),
        None => Ok(path.clone()),
    }
}
impl LayeredPack {
    #[cfg(test)]
    pub(crate) fn fixture_ground_members(
        members: std::collections::BTreeMap<AssetPath, Vec<u8>>,
        budget: ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        cancel.check()?;
        if members.is_empty() || members.len() > 4096 {
            return Err(metadata::invalid("synthetic ground member count"));
        }
        let limits = Limits::default();
        let mut bytes = 256 * 1024_u64;
        for (path, member) in &members {
            cancel.check()?;
            check_limit(
                "synthetic ground member bytes",
                member.len() as u64,
                limits.encoded_bytes,
            )?;
            bytes = bytes
                .checked_add(member.capacity() as u64 + path.as_str().len() as u64 + 512)
                .ok_or(AssetError::Allocation)?;
        }
        let reservation = budget.reserve(bytes, cancel)?;
        let review = super::review::fixture_review();
        review.validate()?;
        let review_digest = review.digest()?;
        Ok(Self {
            review,
            review_digest,
            layers: Vec::new(),
            layout: MountLayout::Java,
            target: None,
            compatibility: None,
            internal_compatibility: Vec::new(),
            metadata: Vec::new(),
            skipped_empty_overlays: Vec::new(),
            budget,
            limits,
            reservations: vec![reservation],
            fixture_members: Some(members),
            fixture_member_reads: std::sync::atomic::AtomicUsize::new(0),
            fixture_stop_after_reads: None,
        })
    }
    #[cfg(test)]
    pub(crate) fn fixture_cancel_on_read(
        &mut self,
        stop: Arc<std::sync::atomic::AtomicBool>,
        read: usize,
    ) {
        assert!(self.fixture_members.is_some() && read > 0);
        self.fixture_stop_after_reads = Some((stop, read));
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "mount provenance, capability, limits and cancellation are separate admission inputs"
    )]
    pub fn mount(
        review: FullPackReview,
        source: Arc<dyn AssetSource>,
        root: Option<AssetPath>,
        layout: MountLayout,
        role: OriginKind,
        limits: Limits,
        budget: ByteBudget,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        review.validate()?;
        let source_is_bedrock = review.edition == super::review::SourceEdition::Bedrock;
        if (layout == MountLayout::Bedrock) != source_is_bedrock
            && review.edition != super::review::SourceEdition::ArtistManifest
        {
            return Err(metadata::invalid(
                "mount layout contradicts reviewed source edition",
            ));
        }
        limits.validate()?;
        cancel.check()?;
        let review_digest = review.digest()?;
        let reservation = budget.reserve(256 * 1024, cancel)?;
        let mut value = Self {
            review,
            review_digest,
            layers: Vec::new(),
            layout,
            target: None,
            compatibility: None,
            internal_compatibility: Vec::new(),
            metadata: Vec::new(),
            skipped_empty_overlays: Vec::new(),
            budget,
            limits,
            reservations: vec![reservation],
            #[cfg(test)]
            fixture_members: None,
            #[cfg(test)]
            fixture_member_reads: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            fixture_stop_after_reads: None,
        };
        value.add_layer(source, root, Label::new("root")?, role, cancel)?;
        Ok(value)
    }
    fn add_layer(
        &mut self,
        source: Arc<dyn AssetSource>,
        root: Option<AssetPath>,
        label: Label,
        kind: OriginKind,
        cancel: Cancel<'_>,
    ) -> Result<()> {
        cancel.check()?;
        check_limit("mounted layers", self.layers.len() as u64 + 1, 512)?;
        let prefix = root.as_ref().map(|root| format!("{}/", root.as_str()));
        if !source
            .members()
            .keys()
            .any(|path| prefix.as_ref().is_none_or(|p| path.as_str().starts_with(p)))
        {
            return Err(metadata::invalid("mount contains no indexed file members"));
        }
        self.reservations.push(self.budget.reserve(4096, cancel)?);
        self.layers.push(MountedLayer {
            source,
            root,
            label,
            kind,
        });
        Ok(())
    }
    /// Apply root metadata once. Calling twice would incorrectly repeat overlays.
    pub fn with_java_overlays(
        mut self,
        target: PackFormat,
        compatibility: Option<Label>,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        if self.layout == MountLayout::Bedrock || self.target.is_some() {
            return Err(metadata::invalid(
                "invalid/repeated Java overlay initialization",
            ));
        }
        PackFormat::new(target.major, target.minor)?;
        let path = AssetPath::parse("pack.mcmeta")?;
        let resolved = self.read_path(&path, self.limits.metadata_bytes, cancel)?;
        let blob = resolved
            .blob
            .ok_or_else(|| metadata::invalid("Java pack.mcmeta is missing at explicit root"))?;
        let parsed =
            JavaPackMetadata::parse(Document::parse(&blob, &self.limits, &self.budget, cancel)?)?;
        if !parsed.formats.contains(target) && compatibility.is_none() {
            return Err(metadata::invalid(
                "target outside declared range; explicit compatibility provenance required",
            ));
        }
        let base = self
            .layers
            .first()
            .ok_or_else(|| metadata::invalid("missing root mount"))?;
        let source = Arc::clone(&base.source);
        let base_root = base.root.clone();
        let base_kind = base.kind;
        self.target = Some(target);
        self.compatibility = compatibility;
        for entry in &parsed.overlays {
            cancel.check()?;
            if !entry.formats.contains(target) {
                continue;
            }
            let root = join_root(base_root.as_ref(), &entry.directory)?;
            let label = Label::new(&format!(
                "overlay[{}]:{}",
                entry.authored_index,
                entry.directory.as_str()
            ))?;
            let kind = if base_kind == OriginKind::SelectedPack {
                OriginKind::OfficialInternalLayer
            } else {
                base_kind
            };
            let prefix = format!("{}/", root.as_str());
            if !source
                .members()
                .keys()
                .any(|path| path.as_str().starts_with(&prefix))
            {
                // Authored overlay declarations can refer to a directory absent
                // from this exact release (observed in Jicklus 201). Retain the
                // declaration and report its absence without inventing a layer.
                self.skipped_empty_overlays.push(label);
                continue;
            }
            self.add_layer(Arc::clone(&source), Some(root), label, kind, cancel)?;
        }
        self.metadata.push(parsed);
        Ok(self)
    }
    /// Extracted art without pack.mcmeta receives an explicit target, never invented authored overlays.
    pub fn with_external_target(
        mut self,
        target: PackFormat,
        evidence: Label,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        cancel.check()?;
        if self.layout != MountLayout::ExtractedJava || self.target.is_some() {
            return Err(metadata::invalid(
                "external target only initializes an extracted-art mount once",
            ));
        }
        PackFormat::new(target.major, target.minor)?;
        self.target = Some(target);
        self.compatibility = Some(evidence);
        Ok(self)
    }
    /// Consume an official internal addon transactionally, including all of its own matching overlays.
    /// A failure drops this candidate mount, leaving any previously published mount untouched.
    pub fn with_internal_pack(
        mut self,
        source: Arc<dyn AssetSource>,
        root: Option<AssetPath>,
        label: Label,
        compatibility: Option<Label>,
        cancel: Cancel<'_>,
    ) -> Result<Self> {
        cancel.check()?;
        if self.layout != MountLayout::Java {
            return Err(metadata::invalid(
                "internal Java pack requires a Java mount",
            ));
        }
        let target = self.target.ok_or_else(|| {
            metadata::invalid("initialize the base target before adding an internal pack")
        })?;
        let relative = AssetPath::parse("pack.mcmeta")?;
        let actual = join_root(root.as_ref(), &relative)?;
        let bytes = source
            .read(&actual, self.limits.metadata_bytes, &self.budget, cancel)?
            .ok_or_else(|| {
                metadata::invalid("internal pack metadata is absent at its explicit root")
            })?;
        let role = self
            .layers
            .first()
            .ok_or_else(|| metadata::invalid("missing base layer"))?
            .kind;
        let kind = if role == OriginKind::SelectedPack {
            OriginKind::OfficialInternalLayer
        } else {
            role
        };
        let origin = BlobOrigin {
            pack: self.review.pack.clone(),
            release: self.review.release.clone(),
            layer: label.clone(),
            path: actual,
            review_digest: self.review_digest,
            kind,
        };
        let blob = bytes.into_blob(origin, None, &self.limits, &self.budget, cancel)?;
        let parsed =
            JavaPackMetadata::parse(Document::parse(&blob, &self.limits, &self.budget, cancel)?)?;
        if !parsed.formats.contains(target) && compatibility.is_none() {
            return Err(metadata::invalid(
                "internal pack target mismatch requires explicit compatibility provenance",
            ));
        }
        self.add_layer(
            Arc::clone(&source),
            root.clone(),
            label.clone(),
            kind,
            cancel,
        )?;
        for entry in &parsed.overlays {
            cancel.check()?;
            if !entry.formats.contains(target) {
                continue;
            }
            let overlay_root = join_root(root.as_ref(), &entry.directory)?;
            let overlay_label = Label::new(&format!(
                "{}/overlay[{}]:{}",
                label.as_str(),
                entry.authored_index,
                entry.directory.as_str()
            ))?;
            self.add_layer(
                Arc::clone(&source),
                Some(overlay_root),
                overlay_label,
                kind,
                cancel,
            )?;
        }
        if let Some(evidence) = compatibility {
            self.internal_compatibility.push((label, evidence));
        }
        self.metadata.push(parsed);
        Ok(self)
    }
    /// An internal official addon is not a second full-pack selector. Its review
    /// is the selected parent's review; its original archive identity is retained.
    pub fn append_internal_layer(
        &mut self,
        source: Arc<dyn AssetSource>,
        root: Option<AssetPath>,
        label: Label,
        cancel: Cancel<'_>,
    ) -> Result<()> {
        let role = self
            .layers
            .first()
            .ok_or_else(|| metadata::invalid("missing root mount"))?
            .kind;
        let kind = if role == OriginKind::SelectedPack {
            OriginKind::OfficialInternalLayer
        } else {
            role
        };
        self.add_layer(source, root, label, kind, cancel)
    }
    pub(crate) fn uses_budget(&self, budget: &ByteBudget) -> bool {
        self.reservations
            .first()
            .is_some_and(|charge| charge.belongs_to(budget))
    } // Validate the mount account before preparing a bank.
    pub fn review(&self) -> &FullPackReview {
        &self.review
    }
    pub fn layout(&self) -> MountLayout {
        self.layout
    }
    pub fn target(&self) -> Option<PackFormat> {
        self.target
    }
    pub fn compatibility(&self) -> Option<&Label> {
        self.compatibility.as_ref()
    }
    pub fn internal_compatibility(&self) -> &[(Label, Label)] {
        &self.internal_compatibility
    }
    pub fn skipped_empty_overlays(&self) -> &[Label] {
        &self.skipped_empty_overlays
    }
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }
    pub fn budget(&self) -> &ByteBudget {
        &self.budget
    }
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
    pub fn read_resource(&self, key: &ResourceKey, cancel: Cancel<'_>) -> Result<Resolution> {
        let cap = if key.kind == ResourceKind::Texture {
            self.limits.encoded_bytes
        } else {
            self.limits.metadata_bytes
        };
        self.read_path(&key.relative_path(self.layout)?, cap, cancel)
    }
    pub fn read_path(&self, path: &AssetPath, cap: u64, cancel: Cancel<'_>) -> Result<Resolution> {
        cancel.check()?;
        #[cfg(test)]
        if let Some(members) = &self.fixture_members {
            let read = self
                .fixture_member_reads
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1;
            if let Some((stop, target)) = &self.fixture_stop_after_reads {
                if read >= *target {
                    stop.store(true, std::sync::atomic::Ordering::Release);
                }
            }
            cancel.check()?;
            let layer = Label::new("Synthetic selected ground boundary fixture")?;
            let member = members.get(path);
            let charge = self
                .budget
                .reserve(4096 + member.map_or(0, |bytes| bytes.len() as u64), cancel)?;
            let blob = match member {
                None => None,
                Some(bytes) => {
                    check_limit("synthetic ground read bytes", bytes.len() as u64, cap)?;
                    let mut copied = Vec::new();
                    copied
                        .try_reserve_exact(bytes.len())
                        .map_err(|_| AssetError::Allocation)?;
                    copied.extend_from_slice(bytes);
                    let origin = BlobOrigin {
                        pack: self.review.pack.clone(),
                        release: self.review.release.clone(),
                        layer: layer.clone(),
                        path: path.clone(),
                        review_digest: self.review_digest,
                        kind: OriginKind::SelectedPack,
                    };
                    Some(SourceBlob::new(
                        copied,
                        origin,
                        None,
                        &self.limits,
                        &self.budget,
                        cancel,
                    )?)
                }
            };
            return Ok(Resolution {
                blob,
                requested: path.clone(),
                target: self.target,
                attempts: vec![ResolutionAttempt {
                    layer,
                    path: path.clone(),
                    found: member.is_some(),
                    source_sha256: None,
                }],
                reservations: vec![charge],
            });
        }
        let mut attempts = Vec::new();
        let mut reservations = vec![self.budget.reserve(
            4096 + self.layers.len() as u64 * std::mem::size_of::<ResolutionAttempt>() as u64,
            cancel,
        )?];
        attempts
            .try_reserve_exact(self.layers.len())
            .map_err(|_| AssetError::Allocation)?;
        let mut blob = None;
        // Reverse lookup implements forward root+all-overlays last-wins without
        // eagerly decoding a shadowed resource or parsing unrelated GUI sidecars.
        for layer in self.layers.iter().rev() {
            cancel.check()?;
            let actual = join_root(layer.root.as_ref(), path)?;
            let bytes = layer.source.read(&actual, cap, &self.budget, cancel)?;
            reservations.push(self.budget.reserve(4096, cancel)?);
            attempts.push(ResolutionAttempt {
                layer: layer.label.clone(),
                path: actual.clone(),
                found: bytes.is_some(),
                source_sha256: layer.source.source_digest(),
            });
            let Some(bytes) = bytes else {
                continue;
            };
            let origin = BlobOrigin {
                pack: self.review.pack.clone(),
                release: self.review.release.clone(),
                layer: layer.label.clone(),
                path: actual,
                review_digest: self.review_digest,
                kind: layer.kind,
            };
            blob = Some(bytes.into_blob(origin, None, &self.limits, &self.budget, cancel)?);
            break;
        }
        Ok(Resolution {
            blob,
            requested: path.clone(),
            target: self.target,
            attempts,
            reservations,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scalar_upper_bound_includes_minors_but_tuple_upper_bound_is_exact() {
        let scalar = serde_json::json!({"min_format":70,"max_format":70});
        let tuple = serde_json::json!({"min_format":[70,1],"max_format":[70,2]});
        let a = FormatRange::fields(object(&scalar).unwrap(), "formats", false).unwrap();
        let b = FormatRange::fields(object(&tuple).unwrap(), "formats", false).unwrap();
        assert!(a.contains(PackFormat::new(70, 999).unwrap()));
        assert!(!b.contains(PackFormat::new(70, 0).unwrap()));
        assert!(b.contains(PackFormat::new(70, 2).unwrap()));
        assert!(!b.contains(PackFormat::new(70, 3).unwrap()));
    }
    #[test]
    fn tuples_supersede_legacy_fields_without_collapsing_minor_versions() {
        let value = serde_json::json!({"formats":[15,84],"min_format":[70,1],"max_format":[84,0]});
        let range = FormatRange::fields(object(&value).unwrap(), "formats", false).unwrap();
        assert!(!range.contains(PackFormat::new(70, 0).unwrap()));
        assert!(!range.contains(PackFormat::new(84, 1).unwrap()));
        assert!(range.contains(PackFormat::new(70, 1).unwrap()));
        let bad = serde_json::json!({"min_format":[3,1],"max_format":[3,0]});
        assert!(FormatRange::fields(object(&bad).unwrap(), "formats", false).is_err());
    }
    #[test]
    fn layouts_do_not_apply_global_legacy_or_bedrock_aliases() {
        let key = ResourceKey {
            kind: ResourceKind::Texture,
            id: ResourceId::parse("block/oak_log").unwrap(),
        };
        assert_eq!(
            key.relative_path(MountLayout::Java).unwrap().as_str(),
            "assets/minecraft/textures/block/oak_log.png"
        );
        assert_eq!(
            key.relative_path(MountLayout::ExtractedJava)
                .unwrap()
                .as_str(),
            "minecraft/textures/block/oak_log.png"
        );
        assert!(key.relative_path(MountLayout::Bedrock).is_err());
    }
}
