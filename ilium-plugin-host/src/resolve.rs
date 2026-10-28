//! Dependency and service resolution across a set of packages (reference-manifest.md PKG-650..660).

use ilium_plugin_api::manifest::{Diagnostic, InstalledSummary, Manifest};
use ilium_plugin_api::ApiVersion;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// The order in which packages activate, with the service providers each consumer uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionPlan {
    /// Plugin ids in activation order.
    pub activation_order: Vec<String>,
}

/// Resolves plugin and service edges; cycles are `dependency.cycle` or
/// `manifest.service-cycle` (PKG-335, PKG-654).
pub fn resolve(
    packages: &[Manifest],
    installed: &[InstalledSummary],
    host: ApiVersion,
) -> Result<ResolutionPlan, Vec<Diagnostic>> {
    let incoming: Vec<PlanPackage> = packages
        .iter()
        .map(|package| PlanPackage {
            id: package.id.clone(),
            version: package.version.clone(),
            needs: package
                .requires
                .as_ref()
                .map(|r| {
                    r.plugins
                        .iter()
                        .map(|p| (p.id.clone(), p.version.clone(), p.optional))
                        .collect()
                })
                .unwrap_or_default(),
            host: package.requires.as_ref().and_then(|r| r.host.clone()),
        })
        .collect();
    let incoming = incoming
        .into_iter()
        .map(|mut package| {
            for (id, range, _) in &mut package.needs {
                let original_present =
                    packages.iter().any(|p| p.id == *id) || installed.iter().any(|p| p.id == *id);
                if !original_present {
                    if let Some(replacement) = packages
                        .iter()
                        .filter(|p| p.replaces.contains(id))
                        .find(|p| {
                            PluginVersion::parse(&p.version).is_ok_and(|v| {
                                VersionRange::parse(range).is_ok_and(|r| r.matches(&v, false))
                            })
                        })
                    {
                        *id = replacement.id.clone();
                    }
                }
            }
            package
        })
        .collect();
    let installed_packages = installed
        .iter()
        .filter(|p| !packages.iter().any(|incoming| incoming.id == p.id))
        .map(|p| PlanPackage {
            id: p.id.clone(),
            version: p.version.clone(),
            needs: Vec::new(),
            host: None,
        })
        .collect();
    let plan = resolve_plan(&ResolveInput {
        incoming,
        installed: installed_packages,
        sources: Vec::new(),
        channel: "stable".into(),
        host_release: format!("{}.{}.{}", host.major, host.minor, host.patch),
        is_update: false,
        pins: Vec::new(),
    })?;
    let mut graph: BTreeMap<String, BTreeSet<String>> = plan
        .steps
        .iter()
        .map(|step| (step.id.clone(), BTreeSet::new()))
        .collect();
    let mut providers: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for package in installed
        .iter()
        .filter(|p| !packages.iter().any(|incoming| incoming.id == p.id))
    {
        for (name, version) in &package.services {
            providers
                .entry(name.clone())
                .or_default()
                .push((package.id.clone(), version.clone()));
        }
    }
    for package in packages {
        if let Some(entries) = package
            .contributes
            .0
            .get("services")
            .and_then(|v| v.get("provides"))
            .and_then(serde_json::Value::as_array)
        {
            for entry in entries {
                let Some(name) = entry.get("name").and_then(serde_json::Value::as_str) else {
                    return Err(vec![diagnostic(
                        "manifest.requires-invalid",
                        "service provider missing name",
                    )]);
                };
                let Some(version) = entry.get("version").and_then(serde_json::Value::as_str) else {
                    return Err(vec![diagnostic(
                        "manifest.service-version",
                        "service provider missing version",
                    )]);
                };
                PluginVersion::parse(version).map_err(|e| vec![e])?;
                providers
                    .entry(name.into())
                    .or_default()
                    .push((package.id.clone(), version.into()));
            }
        }
    }
    let mut service_labels = BTreeMap::new();
    for package in packages {
        if let Some(requires) = &package.requires {
            if requires.services.len() > 32 {
                return Err(vec![diagnostic(
                    "manifest.requires-invalid",
                    "at most 32 required services are allowed",
                )]);
            }
            for need in requires.plugins.iter().filter(|p| !p.optional) {
                let target = if graph.contains_key(&need.id) {
                    need.id.clone()
                } else {
                    packages
                        .iter()
                        .find(|p| {
                            p.replaces.contains(&need.id)
                                && PluginVersion::parse(&p.version).is_ok_and(|v| {
                                    VersionRange::parse(&need.version)
                                        .is_ok_and(|r| r.matches(&v, false))
                                })
                        })
                        .map(|p| p.id.clone())
                        .unwrap_or_else(|| need.id.clone())
                };
                graph.entry(package.id.clone()).or_default().insert(target);
            }
            for text in &requires.services {
                let requirement = parse_service_requirement(text).map_err(|e| vec![e])?;
                let provider = providers.get(&requirement.interface).and_then(|options| {
                    options
                        .iter()
                        .filter_map(|(id, version)| {
                            PluginVersion::parse(version)
                                .ok()
                                .filter(|v| requirement.range.matches(v, false))
                                .map(|version| (id, version))
                        })
                        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))
                });
                if let Some((id, _)) = provider {
                    graph
                        .entry(package.id.clone())
                        .or_default()
                        .insert(id.clone());
                    service_labels.insert((package.id.clone(), id.clone()), requirement.interface);
                }
            }
        }
    }
    let order = topological_order(graph).map_err(|cycle| {
        let mut names: Vec<String> = cycle
            .windows(2)
            .filter_map(|edge| {
                service_labels
                    .get(&(edge[0].clone(), edge[1].clone()))
                    .cloned()
            })
            .collect();
        if let Some((index, _)) = names.iter().enumerate().min_by_key(|(_, name)| *name) {
            names.rotate_left(index);
        }
        vec![diagnostic(
            if names.is_empty() {
                "dependency.cycle"
            } else {
                "manifest.service-cycle"
            },
            format!(
                "cycle: {}",
                if names.is_empty() { cycle } else { names }.join(" -> ")
            ),
        )]
    })?;
    Ok(ResolutionPlan {
        activation_order: order,
    })
}

/// A plugin version: semver with build metadata ignored in comparisons
/// (reference-dependency-resolution.md PKG-310..312).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginVersion {
    /// Major.
    pub major: u64,
    /// Minor.
    pub minor: u64,
    /// Patch.
    pub patch: u64,
    /// Pre-release identifiers, empty for a release.
    pub pre_release: Vec<String>,
}

impl PluginVersion {
    /// Parses a version; a bad one is `manifest.version-invalid` (PKG-310).
    pub fn parse(text: &str) -> Result<PluginVersion, Diagnostic> {
        let parsed = semver::Version::parse(text).map_err(|_| {
            diagnostic(
                "manifest.version-invalid",
                format!("invalid version {text:?}"),
            )
        })?;
        if text.len() > 64
            || [parsed.major, parsed.minor, parsed.patch]
                .iter()
                .any(|number| *number > 2_147_483_647)
        {
            return Err(diagnostic(
                "manifest.version-invalid",
                format!("version outside limits: {text}"),
            ));
        }
        Ok(Self {
            major: parsed.major,
            minor: parsed.minor,
            patch: parsed.patch,
            pre_release: if parsed.pre.is_empty() {
                Vec::new()
            } else {
                parsed.pre.as_str().split('.').map(str::to_owned).collect()
            },
        })
    }
}

/// A version range of a requirement (reference-dependency-resolution.md PKG-313..316).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRange {
    comparators: Vec<semver::Comparator>,
}

impl VersionRange {
    /// Parses a range; a bad one is `manifest.requires-invalid` (PKG-313).
    pub fn parse(text: &str) -> Result<VersionRange, Diagnostic> {
        let invalid = || {
            diagnostic(
                "manifest.requires-invalid",
                format!("invalid range {text:?}"),
            )
        };
        if text.is_empty()
            || text.len() > 128
            || text.contains("||")
            || text.contains(',')
            || text.split(' ').any(str::is_empty)
            || text.chars().any(|c| c.is_whitespace() && c != ' ')
        {
            return Err(invalid());
        }
        let mut comparators = Vec::new();
        for token in text.split(' ') {
            if token == "*" {
                continue;
            }
            if token.contains('*') || token.contains('+') {
                return Err(invalid());
            }
            let comparator = semver::Comparator::parse(token).map_err(|_| invalid())?;
            if comparator.op == semver::Op::Wildcard {
                return Err(invalid());
            }
            if comparator.major > 2_147_483_647
                || comparator.minor.is_some_and(|v| v > 2_147_483_647)
                || comparator.patch.is_some_and(|v| v > 2_147_483_647)
            {
                return Err(invalid());
            }
            comparators.push(comparator);
        }
        Ok(Self { comparators })
    }

    /// Whether a version satisfies the range; pre-releases match only when allowed (PKG-315).
    pub fn matches(&self, version: &PluginVersion, allow_prerelease: bool) -> bool {
        if semver::Prerelease::new(&version.pre_release.join(".")).is_err() {
            return false;
        }
        let version = version.semver();
        if !version.pre.is_empty()
            && !allow_prerelease
            && !self.comparators.iter().any(|c| {
                !c.pre.is_empty()
                    && c.major == version.major
                    && c.minor == Some(version.minor)
                    && c.patch == Some(version.patch)
            })
        {
            return false;
        }
        self.comparators
            .iter()
            .all(|comparator| comparator_matches(comparator, &version))
    }
}

/// A parsed `requires.services` entry (PKG-320).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRequirement {
    /// The service interface.
    pub interface: String,
    /// The accepted range.
    pub range: VersionRange,
}

/// Parses `<interface>@<range>` (PKG-320).
pub fn parse_service_requirement(text: &str) -> Result<ServiceRequirement, Diagnostic> {
    let (interface, version) = text.rsplit_once('@').ok_or_else(|| {
        diagnostic(
            "manifest.service-version",
            "service requires a full version",
        )
    })?;
    ilium_plugin_api::grants::Grant::parse(&format!("services.use:{interface}"))?;
    PluginVersion::parse(version).map_err(|_| {
        diagnostic(
            "manifest.service-version",
            "service requires a full version",
        )
    })?;
    Ok(ServiceRequirement {
        interface: interface.to_owned(),
        range: VersionRange::parse(&format!("^{version}"))?,
    })
}

/// A parsed extension-point reference (PKG-321).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointReference {
    /// The owning plugin.
    pub plugin: String,
    /// The point name.
    pub point: String,
    /// The accepted range.
    pub range: VersionRange,
}

/// Parses `<plugin>/<point>@<range>` (PKG-321).
pub fn parse_point_reference(text: &str) -> Result<PointReference, Diagnostic> {
    let invalid = || {
        diagnostic(
            "manifest.requires-invalid",
            format!("invalid point reference {text:?}"),
        )
    };
    let (name, range) = text.rsplit_once('@').ok_or_else(invalid)?;
    let (plugin, point) = name.split_once('/').ok_or_else(invalid)?;
    ilium_plugin_api::ids::parse_plugin_id(plugin).map_err(|_| invalid())?;
    ilium_plugin_api::grants::Grant::parse(&format!("extensions.provide:{name}"))
        .map_err(|_| invalid())?;
    if point.is_empty()
        || !point
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(invalid());
    }
    Ok(PointReference {
        plugin: plugin.to_owned(),
        point: point.to_owned(),
        range: VersionRange::parse(range)?,
    })
}

/// One package in a resolution request (PKG-330..348).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanPackage {
    /// The plugin id.
    pub id: String,
    /// The version.
    pub version: String,
    /// Requirements as (plugin id, range, optional).
    pub needs: Vec<(String, String, bool)>,
    /// The host release range, when any.
    pub host: Option<String>,
}

/// One package a source offers (PKG-330..348).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePackage {
    /// The package.
    pub package: PlanPackage,
    /// The package digest.
    pub sha256: String,
    /// Whether the source yanked it.
    pub yanked: bool,
    /// The channel it is published on.
    pub channel: String,
    /// The rank of the source, lower wins.
    pub source_rank: usize,
}

/// A resolution request (PKG-330..348).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveInput {
    /// Packages being installed or updated.
    pub incoming: Vec<PlanPackage>,
    /// Packages already installed.
    pub installed: Vec<PlanPackage>,
    /// Packages the sources offer.
    pub sources: Vec<SourcePackage>,
    /// The selected channel.
    pub channel: String,
    /// The running host release.
    pub host_release: String,
    /// Whether the request is an update.
    pub is_update: bool,
    /// Pinned plugin ids.
    pub pins: Vec<String>,
}

/// What a plan step does (PKG-330).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanAction {
    /// Installs a package.
    Install,
    /// Updates an installed package.
    Update,
    /// Keeps an installed package.
    Keep,
}

/// One step of a resolved plan (PKG-330).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanStep {
    /// The action.
    pub action: PlanAction,
    /// The plugin id.
    pub id: String,
    /// The version.
    pub version: String,
}

/// A resolved plan (PKG-330..348).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPlan {
    /// The steps in activation order.
    pub steps: Vec<PlanStep>,
    /// The digest chosen for each plugin id.
    pub chosen_sources: Vec<(String, String)>,
}

impl ResolvedPlan {
    /// The digest of the source chosen for a plugin (PKG-340).
    pub fn chosen_source_sha(&self, id: &str) -> Option<String> {
        self.chosen_sources
            .iter()
            .find(|(plugin, _)| plugin == id)
            .map(|(_, sha)| sha.clone())
    }
}

/// Resolves a request into a plan; conflicts are diagnostics (PKG-330..348).
pub fn resolve_plan(input: &ResolveInput) -> Result<ResolvedPlan, Vec<Diagnostic>> {
    let host = PluginVersion::parse(&input.host_release).map_err(|error| vec![error])?;
    for packages in [&input.incoming, &input.installed] {
        let mut ids = BTreeSet::new();
        for package in packages {
            if !ids.insert(&package.id) {
                return Err(vec![diagnostic(
                    "dependency.conflict",
                    format!("multiple versions requested for {}", package.id),
                )]);
            }
        }
    }
    let mut candidates: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    for source in &input.sources {
        if source.yanked
            || !(source.channel == "stable" || input.channel == "beta" && source.channel == "beta")
        {
            continue;
        }
        let version = PluginVersion::parse(&source.package.version).map_err(|error| vec![error])?;
        if input.channel != "beta" && !version.pre_release.is_empty() {
            continue;
        }
        candidates
            .entry(source.package.id.clone())
            .or_default()
            .push(Candidate {
                package: source.package.clone(),
                version,
                sha: Some(source.sha256.clone()),
                rank: source.source_rank,
                installed: false,
            });
    }
    for package in &input.installed {
        candidates
            .entry(package.id.clone())
            .or_default()
            .push(Candidate::local(package, true).map_err(|e| vec![e])?);
    }
    for options in candidates.values_mut() {
        options.sort_by(|left, right| {
            let installed = if input.is_update {
                Ordering::Equal
            } else {
                right.installed.cmp(&left.installed)
            };
            installed
                .then_with(|| right.version.cmp(&left.version))
                .then_with(|| left.rank.cmp(&right.rank))
                .then_with(|| left.sha.cmp(&right.sha))
        });
    }
    for id in &input.pins {
        if let Some(installed) = input.installed.iter().find(|p| &p.id == id) {
            let version = PluginVersion::parse(&installed.version).map_err(|e| vec![e])?;
            if let Some(options) = candidates.get_mut(id) {
                options.retain(|c| c.version == version);
            }
            if input.incoming.iter().any(|p| {
                &p.id == id && PluginVersion::parse(&p.version).is_ok_and(|v| v != version)
            }) {
                return Err(vec![diagnostic(
                    "dependency.conflict",
                    format!("{id} is pinned to {}", installed.version),
                )]);
            }
        }
    }
    // Incoming file choices are fixed; dependencies may never silently replace them.
    for package in &input.incoming {
        candidates.insert(
            package.id.clone(),
            vec![Candidate::local(package, false).map_err(|e| vec![e])?],
        );
    }
    for options in candidates.values() {
        for candidate in options {
            validate_package(&candidate.package).map_err(|e| vec![e])?;
        }
    }
    let roots: BTreeSet<String> = input
        .installed
        .iter()
        .chain(&input.incoming)
        .map(|p| p.id.clone())
        .collect();
    let mut selected = BTreeMap::new();
    let mut failures = Vec::new();
    if !search(
        &roots,
        &candidates,
        &mut selected,
        &host,
        input.channel == "beta",
        &mut failures,
    ) {
        if failures.is_empty() {
            failures.push(diagnostic(
                "dependency.conflict",
                "no common version; use ilium plugin update --all",
            ));
        }
        return Err(failures);
    }
    let order = activation_order(&selected).map_err(|e| vec![e])?;
    let steps = order
        .into_iter()
        .map(|id| {
            let package = &selected[&id].package;
            let action = match input.installed.iter().find(|old| old.id == id) {
                None => PlanAction::Install,
                Some(old)
                    if PluginVersion::parse(&old.version).ok()
                        == Some(selected[&id].version.clone()) =>
                {
                    PlanAction::Keep
                }
                Some(_) => PlanAction::Update,
            };
            PlanStep {
                action,
                id,
                version: package.version.clone(),
            }
        })
        .collect();
    let chosen_sources = selected
        .iter()
        .filter_map(|(id, candidate)| candidate.sha.as_ref().map(|sha| (id.clone(), sha.clone())))
        .collect();
    Ok(ResolvedPlan {
        steps,
        chosen_sources,
    })
}

fn diagnostic(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(
        ilium_plugin_api::error::Code::from_static(code),
        "requires",
        message,
    )
}

impl PluginVersion {
    fn semver(&self) -> semver::Version {
        let mut version = semver::Version::new(self.major, self.minor, self.patch);
        // Public fields may be supplied by callers; malformed prereleases cannot match.
        if let Ok(pre) = semver::Prerelease::new(&self.pre_release.join(".")) {
            version.pre = pre;
        }
        version
    }
}
impl Ord for PluginVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        let core =
            (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch));
        if core != Ordering::Equal {
            return core;
        }
        match (self.pre_release.is_empty(), other.pre_release.is_empty()) {
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            _ => {}
        }
        for (left, right) in self.pre_release.iter().zip(&other.pre_release) {
            let numeric_left = left.bytes().all(|b| b.is_ascii_digit());
            let numeric_right = right.bytes().all(|b| b.is_ascii_digit());
            let order = match (numeric_left, numeric_right) {
                (true, true) => left.len().cmp(&right.len()).then_with(|| left.cmp(right)),
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                _ => left.cmp(right),
            };
            if order != Ordering::Equal {
                return order;
            }
        }
        self.pre_release.len().cmp(&other.pre_release.len())
    }
}
impl PartialOrd for PluginVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone)]
struct Candidate {
    package: PlanPackage,
    version: PluginVersion,
    sha: Option<String>,
    rank: usize,
    installed: bool,
}
impl Candidate {
    fn local(package: &PlanPackage, installed: bool) -> Result<Self, Diagnostic> {
        Ok(Self {
            package: package.clone(),
            version: PluginVersion::parse(&package.version)?,
            sha: None,
            rank: usize::MAX,
            installed,
        })
    }
}

fn validate_package(package: &PlanPackage) -> Result<(), Diagnostic> {
    let mut seen = BTreeSet::new();
    if package.needs.len() > 32 {
        return Err(diagnostic(
            "manifest.requires-invalid",
            "at most 32 plugin requirements are allowed",
        ));
    }
    for (id, range, _) in &package.needs {
        if id == &package.id {
            return Err(diagnostic(
                "manifest.requires-self",
                format!("{} requires itself", package.id),
            ));
        }
        if !seen.insert(id) {
            return Err(diagnostic(
                "manifest.requires-invalid",
                format!("duplicate requirement {id}"),
            ));
        }
        VersionRange::parse(range)?;
    }
    if let Some(range) = &package.host {
        VersionRange::parse(range)?;
    }
    Ok(())
}

fn constraints(selected: &BTreeMap<String, Candidate>, id: &str) -> Vec<(String, String)> {
    selected
        .values()
        .flat_map(|candidate| {
            candidate
                .package
                .needs
                .iter()
                .filter(move |(need, _, optional)| !optional && need == id)
                .map(move |(_, range, _)| (candidate.package.id.clone(), range.clone()))
        })
        .collect()
}

fn accepts(candidate: &Candidate, constraints: &[(String, String)], beta: bool) -> bool {
    constraints.iter().all(|(_, text)| {
        VersionRange::parse(text).is_ok_and(|range| range.matches(&candidate.version, beta))
    })
}

fn search(
    roots: &BTreeSet<String>,
    candidates: &BTreeMap<String, Vec<Candidate>>,
    selected: &mut BTreeMap<String, Candidate>,
    host: &PluginVersion,
    beta: bool,
    failures: &mut Vec<Diagnostic>,
) -> bool {
    // Every candidate's outgoing constraints are rechecked when a later node is assigned.
    for (id, candidate) in selected.iter() {
        if !accepts(candidate, &constraints(selected, id), beta) {
            *failures = vec![diagnostic(
                "dependency.conflict",
                format!(
                    "{id} constraints {:?}; available {:?}; use ilium plugin update --all",
                    constraints(selected, id),
                    candidates.get(id).map(|options| options
                        .iter()
                        .map(|c| &c.package.version)
                        .collect::<Vec<_>>())
                ),
            )];
            return false;
        }
    }
    let required: BTreeSet<String> = roots
        .iter()
        .cloned()
        .chain(selected.values().flat_map(|c| {
            c.package
                .needs
                .iter()
                .filter(|(_, _, optional)| !optional)
                .map(|(id, _, _)| id.clone())
        }))
        .collect();
    let missing: Vec<_> = required
        .iter()
        .filter(|id| !selected.contains_key(*id) && !candidates.contains_key(*id))
        .collect();
    if !missing.is_empty() {
        *failures = missing
            .into_iter()
            .map(|id| {
                diagnostic(
                    "dependency.unresolved",
                    format!("missing {id}: {:?}", constraints(selected, id)),
                )
            })
            .collect();
        return false;
    }
    let Some(id) = required.iter().find(|id| !selected.contains_key(*id)) else {
        return match activation_order(selected) {
            Ok(_) => true,
            Err(error) => {
                *failures = vec![error];
                false
            }
        };
    };
    let requirements = constraints(selected, id);
    let mut host_failure = None;
    for candidate in &candidates[id] {
        if !accepts(candidate, &requirements, beta) {
            continue;
        }
        if candidate.package.host.as_ref().is_some_and(|text| {
            !VersionRange::parse(text).is_ok_and(|range| range.matches(host, beta))
        }) {
            host_failure = Some(diagnostic(
                "dependency.host-version",
                format!(
                    "{} requires host {:?}, running {}.{}.{}",
                    id, candidate.package.host, host.major, host.minor, host.patch
                ),
            ));
            continue;
        }
        selected.insert(id.clone(), candidate.clone());
        if search(roots, candidates, selected, host, beta, failures) {
            return true;
        }
        selected.remove(id);
    }
    if let Some(error) = host_failure {
        *failures = vec![error];
    } else if failures.is_empty() {
        let all_constraints: Vec<_> = selected
            .values()
            .flat_map(|c| {
                c.package
                    .needs
                    .iter()
                    .map(move |(id, range, _)| format!("{} requires {id} {range}", c.package.id))
            })
            .collect();
        *failures = vec![diagnostic(
            "dependency.conflict",
            format!(
                "constraints: {all_constraints:?}; available: {:?}; use ilium plugin update --all",
                candidates
                    .iter()
                    .map(|(id, options)| (
                        id,
                        options
                            .iter()
                            .map(|c| &c.package.version)
                            .collect::<Vec<_>>()
                    ))
                    .collect::<Vec<_>>()
            ),
        )];
    }
    false
}

fn activation_order(selected: &BTreeMap<String, Candidate>) -> Result<Vec<String>, Diagnostic> {
    let graph = selected
        .iter()
        .map(|(id, c)| {
            (
                id.clone(),
                c.package
                    .needs
                    .iter()
                    .filter(|(_, _, optional)| !optional)
                    .map(|(id, _, _)| id.clone())
                    .collect(),
            )
        })
        .collect();
    topological_order(graph).map_err(|cycle| {
        diagnostic(
            "dependency.cycle",
            format!("hard dependency cycle: {}", cycle.join(" -> ")),
        )
    })
}

fn comparator_matches(comparator: &semver::Comparator, version: &semver::Version) -> bool {
    use semver::Op;
    let mut lower = semver::Version::new(
        comparator.major,
        comparator.minor.unwrap_or(0),
        comparator.patch.unwrap_or(0),
    );
    lower.pre = comparator.pre.clone();
    let upper = match comparator.op {
        Op::Caret if comparator.major > 0 || comparator.minor.is_none() => {
            semver::Version::new(comparator.major + 1, 0, 0)
        }
        Op::Caret if comparator.minor != Some(0) || comparator.patch.is_none() => {
            semver::Version::new(0, comparator.minor.unwrap_or(0) + 1, 0)
        }
        Op::Caret => semver::Version::new(0, 0, comparator.patch.unwrap_or(0) + 1),
        _ if comparator.minor.is_none() => semver::Version::new(comparator.major + 1, 0, 0),
        _ => semver::Version::new(comparator.major, comparator.minor.unwrap_or(0) + 1, 0),
    };
    match comparator.op {
        Op::Caret | Op::Tilde => version >= &lower && version < &upper,
        Op::Exact | Op::Wildcard => {
            if comparator.patch.is_some() {
                version == &lower
            } else {
                version >= &lower && version < &upper
            }
        }
        Op::Greater => {
            if comparator.patch.is_some() {
                version > &lower
            } else {
                version >= &upper
            }
        }
        Op::GreaterEq => version >= &lower,
        Op::Less => version < &lower,
        Op::LessEq => {
            if comparator.patch.is_some() {
                version <= &lower
            } else {
                version < &upper
            }
        }
        _ => false,
    }
}

fn topological_order(
    mut graph: BTreeMap<String, BTreeSet<String>>,
) -> Result<Vec<String>, Vec<String>> {
    let mut order = Vec::new();
    while !graph.is_empty() {
        let Some(id) = graph
            .iter()
            .find(|(_, needs)| needs.is_empty())
            .map(|(id, _)| id.clone())
        else {
            // Follow real graph edges so the diagnostic names a cycle, not downstream nodes.
            let Some(start) = graph.keys().next().cloned() else {
                return Ok(order);
            };
            let mut path = Vec::new();
            let mut current = start;
            loop {
                if let Some(index) = path.iter().position(|id| id == &current) {
                    let mut cycle = path[index..].to_vec();
                    if let Some((index, _)) = cycle.iter().enumerate().min_by_key(|(_, id)| *id) {
                        cycle.rotate_left(index);
                    }
                    if let Some(first) = cycle.first().cloned() {
                        cycle.push(first);
                    }
                    return Err(cycle);
                }
                path.push(current.clone());
                let Some(next) = graph.get(&current).and_then(|needs| needs.first()).cloned()
                else {
                    return Err(path);
                };
                current = next;
            }
        };
        graph.remove(&id);
        for needs in graph.values_mut() {
            needs.remove(&id);
        }
        order.push(id);
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::{
        parse_point_reference, parse_service_requirement, resolve, resolve_plan, PlanAction,
        PlanPackage, PlanStep, PluginVersion, ResolveInput, ResolvedPlan, SourcePackage,
        VersionRange,
    };
    use ilium_plugin_api::manifest::{parse_manifest, Diagnostic, InstalledSummary, Manifest};
    use ilium_plugin_api::ApiVersion;

    /// Parses a version that the manual calls valid.
    fn version(text: &str) -> PluginVersion {
        PluginVersion::parse(text).expect("fixture version should parse")
    }

    /// Returns the refusal code of a version text the manual calls invalid.
    fn version_refusal(text: &str) -> String {
        PluginVersion::parse(text)
            .expect_err("fixture version should be refused")
            .code
            .as_str()
            .to_string()
    }

    /// Parses a range that the manual calls valid.
    fn range(text: &str) -> VersionRange {
        VersionRange::parse(text).expect("fixture range should parse")
    }

    /// Returns the refusal code of a range text the manual calls invalid.
    fn range_refusal(text: &str) -> String {
        VersionRange::parse(text)
            .expect_err("fixture range should be refused")
            .code
            .as_str()
            .to_string()
    }

    /// Whether `version_text` satisfies `range_text` with the given prerelease permission.
    fn satisfies(range_text: &str, version_text: &str, allow_prerelease: bool) -> bool {
        range(range_text).matches(&version(version_text), allow_prerelease)
    }

    fn has_code(diagnostics: &[Diagnostic], expected_code: &str) -> bool {
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == expected_code)
    }

    fn assert_refused_with(diagnostics: &[Diagnostic], expected_code: &str) {
        let found: Vec<String> = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str().to_string())
            .collect();
        assert!(
            has_code(diagnostics, expected_code),
            "expected refusal code {expected_code}, found {found:?}"
        );
    }

    fn all_messages(diagnostics: &[Diagnostic]) -> String {
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<&str>>()
            .join("\n")
    }

    fn plan_package(
        id: &str,
        version: &str,
        needs: &[(&str, &str, bool)],
        host: Option<&str>,
    ) -> PlanPackage {
        PlanPackage {
            id: id.to_string(),
            version: version.to_string(),
            needs: needs
                .iter()
                .map(|&(need_id, need_range, optional)| {
                    (need_id.to_string(), need_range.to_string(), optional)
                })
                .collect(),
            host: host.map(str::to_string),
        }
    }

    fn source(
        id: &str,
        version: &str,
        sha256: &str,
        channel: &str,
        source_rank: usize,
        needs: &[(&str, &str, bool)],
    ) -> SourcePackage {
        SourcePackage {
            package: plan_package(id, version, needs, None),
            sha256: sha256.to_string(),
            yanked: false,
            channel: channel.to_string(),
            source_rank,
        }
    }

    fn request(
        incoming: Vec<PlanPackage>,
        installed: Vec<PlanPackage>,
        sources: Vec<SourcePackage>,
    ) -> ResolveInput {
        ResolveInput {
            incoming,
            installed,
            sources,
            channel: "stable".to_string(),
            host_release: "0.1.0".to_string(),
            is_update: false,
            pins: Vec::new(),
        }
    }

    fn step(action: PlanAction, id: &str, version: &str) -> PlanStep {
        PlanStep {
            action,
            id: id.to_string(),
            version: version.to_string(),
        }
    }

    /// Resolves a request that the manual says must succeed.
    fn expect_plan(input: &ResolveInput) -> ResolvedPlan {
        resolve_plan(input).expect("resolution should succeed")
    }

    /// Resolves a request that the manual says must be refused, returning its diagnostics.
    fn expect_refusal(input: &ResolveInput) -> Vec<Diagnostic> {
        match resolve_plan(input) {
            Ok(plan) => panic!("resolution should be refused, got {plan:?}"),
            Err(diagnostics) => diagnostics,
        }
    }

    fn api_version() -> ApiVersion {
        ApiVersion {
            major: 0,
            minor: 1,
            patch: 0,
        }
    }

    /// Builds a manifest for `id` whose `requires.plugins` lists each `(plugin id, optional)` pair.
    fn manifest_requiring(id: &str, needs: &[(&str, bool)]) -> Manifest {
        let plugins: Vec<serde_json::Value> = needs
            .iter()
            .map(|&(need_id, optional)| {
                serde_json::json!({ "id": need_id, "version": "*", "optional": optional })
            })
            .collect();
        let text = serde_json::json!({
            "api": "ilium:plugin@0.1.0",
            "id": id,
            "name": "Fixture",
            "version": "1.0.0",
            "publisher": "Example Labs",
            "client": { "entry": "client.wasm", "world": "ilium:client/base@0.1.0" },
            "requires": { "plugins": plugins }
        })
        .to_string();
        parse_manifest(&text).expect("fixture manifest should parse")
    }

    /// REQ: PKG-310
    #[test]
    fn version_parses_major_minor_patch() {
        let parsed = version("1.2.3");
        assert_eq!(parsed.major, 1);
        assert_eq!(parsed.minor, 2);
        assert_eq!(parsed.patch, 3);
        assert!(parsed.pre_release.is_empty());
    }

    /// REQ: PKG-310
    #[test]
    fn version_accepts_the_largest_major_boundary() {
        assert_eq!(version("2147483647.0.0").major, 2147483647);
    }

    /// REQ: PKG-310
    #[test]
    fn version_refuses_a_major_above_the_boundary() {
        assert_eq!(
            version_refusal("2147483648.0.0"),
            "manifest.version-invalid"
        );
    }

    /// REQ: PKG-310
    #[test]
    fn version_refuses_a_leading_zero_in_a_number() {
        assert_eq!(version_refusal("01.2.3"), "manifest.version-invalid");
    }

    /// REQ: PKG-310
    #[test]
    fn version_refuses_a_version_without_patch() {
        assert_eq!(version_refusal("1.2"), "manifest.version-invalid");
    }

    /// REQ: PKG-310
    #[test]
    fn version_refuses_empty_text() {
        assert_eq!(version_refusal(""), "manifest.version-invalid");
    }

    /// REQ: PKG-312
    #[test]
    fn version_refuses_a_leading_v() {
        assert_eq!(version_refusal("v1.2.3"), "manifest.version-invalid");
    }

    /// REQ: PKG-311
    #[test]
    fn version_splits_prerelease_identifiers() {
        assert_eq!(
            version("1.0.0-beta.2").pre_release,
            vec!["beta".to_string(), "2".to_string()]
        );
    }

    /// REQ: PKG-311
    #[test]
    fn version_refuses_a_numeric_prerelease_identifier_with_leading_zero() {
        assert_eq!(version_refusal("1.0.0-beta.01"), "manifest.version-invalid");
    }

    /// REQ: PKG-311
    #[test]
    fn version_refuses_an_illegal_prerelease_character() {
        assert_eq!(version_refusal("1.0.0-beta_1"), "manifest.version-invalid");
    }

    /// REQ: PKG-311
    #[test]
    fn version_accepts_exactly_64_characters() {
        let text = format!("1.0.0-{}", "a".repeat(58));
        assert_eq!(text.len(), 64);
        assert_eq!(version(&text).pre_release, vec!["a".repeat(58)]);
    }

    /// REQ: PKG-311
    #[test]
    fn version_refuses_65_characters() {
        let text = format!("1.0.0-{}", "a".repeat(59));
        assert_eq!(text.len(), 65);
        assert_eq!(version_refusal(&text), "manifest.version-invalid");
    }

    /// REQ: PKG-313
    #[test]
    fn build_metadata_is_ignored_in_equality() {
        assert_eq!(version("1.2.3+build.5"), version("1.2.3"));
    }

    /// REQ: PKG-313
    #[test]
    fn build_metadata_does_not_change_ordering() {
        assert!(version("1.0.0+exp.sha.5114f85") == version("1.0.0"));
        assert!(version("1.0.0+build.2") >= version("1.0.0"));
    }

    /// REQ: PKG-314
    #[test]
    fn precedence_follows_the_documented_ascending_order() {
        let ordered = [
            "0.9.0",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.1.0",
        ];
        for pair in ordered.windows(2) {
            assert!(
                version(pair[0]) < version(pair[1]),
                "{} should be lower than {}",
                pair[0],
                pair[1]
            );
        }
    }

    /// REQ: PKG-314
    #[test]
    fn prerelease_is_lower_than_the_release_with_the_same_numbers() {
        assert!(version("1.0.0-beta.2") < version("1.0.0"));
    }

    /// REQ: PKG-314
    #[test]
    fn numeric_prerelease_identifiers_compare_as_numbers() {
        assert!(version("1.0.0-beta.2") < version("1.0.0-beta.11"));
    }

    /// REQ: PKG-315
    #[test]
    fn range_accepts_every_documented_comparator_form() {
        for text in [
            "*",
            "=1.2.3",
            "1.2.3",
            ">1.2.3",
            ">=1.2.3",
            "<1.2.3",
            "<=1.2.3",
            "^1.2.3",
            "~1.2.3",
            ">=0.1.0 <1.0.0",
        ] {
            assert!(VersionRange::parse(text).is_ok(), "{text} should parse");
        }
    }

    /// REQ: PKG-315
    #[test]
    fn range_refuses_the_or_operator() {
        assert_eq!(range_refusal("^1.0 || ^2.0"), "manifest.requires-invalid");
    }

    /// REQ: PKG-315
    #[test]
    fn range_refuses_a_comparator_with_a_space_inside() {
        assert_eq!(range_refusal(">= 1.0"), "manifest.requires-invalid");
    }

    /// REQ: PKG-315
    #[test]
    fn range_refuses_empty_text() {
        assert_eq!(range_refusal(""), "manifest.requires-invalid");
    }

    /// REQ: PKG-322
    #[test]
    fn range_accepts_exactly_128_characters() {
        let text = format!(">=1.0.0-{}", "a".repeat(120));
        assert_eq!(text.len(), 128);
        assert!(VersionRange::parse(&text).is_ok());
    }

    /// REQ: PKG-322
    #[test]
    fn range_refuses_129_characters() {
        let text = format!(">=1.0.0-{}", "a".repeat(121));
        assert_eq!(text.len(), 129);
        assert_eq!(range_refusal(&text), "manifest.requires-invalid");
    }

    /// REQ: PKG-316
    #[test]
    fn explicit_comparators_match_their_bounds() {
        assert!(satisfies("=1.2.3", "1.2.3", false));
        assert!(!satisfies("=1.2.3", "1.2.4", false));
        assert!(satisfies(">1.2.3", "1.2.4", false));
        assert!(!satisfies(">1.2.3", "1.2.3", false));
        assert!(satisfies(">=1.2.3", "1.2.3", false));
        assert!(!satisfies(">=1.2.3", "1.2.2", false));
        assert!(satisfies("<1.2.3", "1.2.2", false));
        assert!(!satisfies("<1.2.3", "1.2.3", false));
        assert!(satisfies("<=1.2.3", "1.2.3", false));
        assert!(!satisfies("<=1.2.3", "1.2.4", false));
    }

    /// REQ: PKG-315
    #[test]
    fn space_separated_comparators_are_a_logical_and() {
        assert!(satisfies(">=0.1.0 <1.0.0", "0.1.0", false));
        assert!(satisfies(">=0.1.0 <1.0.0", "0.2.0", false));
        assert!(!satisfies(">=0.1.0 <1.0.0", "0.0.9", false));
        assert!(!satisfies(">=0.1.0 <1.0.0", "1.0.0", false));
    }

    /// REQ: PKG-318
    #[test]
    fn caret_keeps_the_left_most_non_zero_part_for_major_versions() {
        assert!(satisfies("^1.2.3", "1.2.3", false));
        assert!(satisfies("^1.2.3", "1.9.9", false));
        assert!(!satisfies("^1.2.3", "2.0.0", false));
        assert!(!satisfies("^1.2.3", "1.2.2", false));
    }

    /// REQ: PKG-318
    #[test]
    fn caret_keeps_the_minor_for_zero_major_versions() {
        assert!(satisfies("^0.2.3", "0.2.3", false));
        assert!(satisfies("^0.2.3", "0.2.9", false));
        assert!(!satisfies("^0.2.3", "0.3.0", false));
        assert!(!satisfies("^0.2.3", "0.2.2", false));
    }

    /// REQ: PKG-318
    #[test]
    fn caret_keeps_the_patch_for_zero_zero_patch_versions() {
        assert!(satisfies("^0.0.3", "0.0.3", false));
        assert!(!satisfies("^0.0.3", "0.0.4", false));
    }

    /// REQ: PKG-318
    #[test]
    fn caret_with_only_major_and_minor_keeps_the_minor() {
        assert!(satisfies("^0.0", "0.0.0", false));
        assert!(satisfies("^0.0", "0.0.9", false));
        assert!(!satisfies("^0.0", "0.1.0", false));
    }

    /// REQ: PKG-318
    #[test]
    fn caret_with_only_major_keeps_the_major() {
        assert!(satisfies("^0", "0.9.9", false));
        assert!(!satisfies("^0", "1.0.0", false));
    }

    /// REQ: PKG-319
    #[test]
    fn tilde_accepts_patch_changes_only() {
        assert!(satisfies("~1.2.3", "1.2.3", false));
        assert!(satisfies("~1.2.3", "1.2.9", false));
        assert!(!satisfies("~1.2.3", "1.3.0", false));
        assert!(!satisfies("~1.2.3", "1.2.2", false));
    }

    /// REQ: PKG-319
    #[test]
    fn tilde_with_major_and_minor_accepts_patch_changes() {
        assert!(satisfies("~1.2", "1.2.0", false));
        assert!(satisfies("~1.2", "1.2.9", false));
        assert!(!satisfies("~1.2", "1.3.0", false));
    }

    /// REQ: PKG-319
    #[test]
    fn tilde_with_only_major_accepts_minor_changes() {
        assert!(satisfies("~1", "1.0.0", false));
        assert!(satisfies("~1", "1.9.9", false));
        assert!(!satisfies("~1", "2.0.0", false));
    }

    /// REQ: PKG-317
    #[test]
    fn bare_version_is_a_caret_range() {
        assert!(satisfies("1.2.3", "1.9.0", false));
        assert!(!satisfies("1.2.3", "2.0.0", false));
        assert!(satisfies("1.2", "1.5.0", false));
        assert!(!satisfies("1.2", "2.0.0", false));
        assert!(satisfies("1", "1.9.9", false));
        assert!(!satisfies("1", "2.0.0", false));
    }

    /// REQ: PKG-317
    #[test]
    fn bare_zero_minor_version_is_the_same_as_its_caret() {
        assert!(satisfies("0.1.0", "0.1.4", false));
        assert!(!satisfies("0.1.0", "0.2.0", false));
        assert_eq!(
            satisfies("0.1.0", "0.1.4", false),
            satisfies("^0.1.0", "0.1.4", false)
        );
    }

    /// REQ: PKG-318
    #[test]
    fn manual_range_table_matches_the_documented_providers() {
        // Rows from the manual's range table; columns are providers 0.1.0, 0.1.4, 0.2.0, 1.0.0.
        let rows: [(&str, [bool; 4]); 5] = [
            ("0.1.0", [true, true, false, false]),
            ("^0.2", [false, false, true, false]),
            (">=0.1.0 <1.0.0", [true, true, true, false]),
            ("~0.1.2", [false, true, false, false]),
            ("*", [true, true, true, true]),
        ];
        let providers = ["0.1.0", "0.1.4", "0.2.0", "1.0.0"];
        for (range_text, expected) in rows {
            for (index, provider) in providers.iter().enumerate() {
                assert_eq!(
                    satisfies(range_text, provider, false),
                    expected[index],
                    "range {range_text} against provider {provider}"
                );
            }
        }
    }

    /// REQ: PKG-320
    #[test]
    fn star_accepts_every_release_version() {
        assert!(satisfies("*", "0.0.1", false));
        assert!(satisfies("*", "99.0.0", false));
    }

    /// REQ: PKG-321
    #[test]
    fn prerelease_is_refused_on_stable_when_no_comparator_carries_one() {
        assert!(!satisfies("^1.2.0", "1.3.0-beta.1", false));
    }

    /// REQ: PKG-321
    #[test]
    fn prerelease_is_allowed_when_the_resolver_runs_on_beta() {
        assert!(satisfies("^1.2.0", "1.3.0-beta.1", true));
    }

    /// REQ: PKG-321
    #[test]
    fn prerelease_matches_when_a_comparator_has_the_same_base_and_a_prerelease() {
        assert!(satisfies(">=1.3.0-beta.1", "1.3.0-beta.1", false));
        assert!(satisfies(">=1.3.0-beta.1", "1.3.0-beta.2", false));
        assert!(!satisfies(">=1.3.0-beta.1", "1.3.1-beta.1", false));
    }

    /// REQ: PKG-321
    #[test]
    fn release_versions_are_not_affected_by_the_prerelease_flag() {
        assert!(satisfies("^1.2.0", "1.3.0", false));
        assert!(satisfies("^1.2.0", "1.3.0", true));
    }

    /// REQ: PKG-327
    #[test]
    fn service_requirement_full_version_is_a_minimum_caret() {
        let requirement = parse_service_requirement("ilium:inference/provider@0.1.0")
            .expect("full service version should parse");
        assert_eq!(requirement.interface, "ilium:inference/provider");
        assert!(requirement.range.matches(&version("0.1.0"), false));
        assert!(requirement.range.matches(&version("0.1.4"), false));
        assert!(!requirement.range.matches(&version("0.2.0"), false));
    }

    /// REQ: PKG-327
    #[test]
    fn service_requirement_accepts_a_plain_name_with_a_full_version() {
        let requirement = parse_service_requirement("com.example.git/status@1.2.0")
            .expect("full service version should parse");
        assert_eq!(requirement.interface, "com.example.git/status");
        assert!(requirement.range.matches(&version("1.9.0"), false));
        assert!(!requirement.range.matches(&version("2.0.0"), false));
    }

    /// REQ: PKG-327
    #[test]
    fn service_requirement_refuses_a_short_form() {
        let refusal = parse_service_requirement("ilium:inference/provider@0.1")
            .expect_err("short service version should be refused");
        assert_eq!(refusal.code.as_str(), "manifest.service-version");
    }

    /// REQ: PKG-327
    #[test]
    fn service_requirement_refuses_a_caret_tilde_or_range() {
        for text in [
            "ilium:inference/provider@^0.1.0",
            "ilium:inference/provider@~0.1.0",
            "ilium:inference/provider@>=0.1.0",
        ] {
            let refusal = parse_service_requirement(text)
                .expect_err("non-full service version should be refused");
            assert_eq!(refusal.code.as_str(), "manifest.service-version", "{text}");
        }
    }

    /// REQ: PKG-328
    #[test]
    fn point_reference_splits_plugin_point_and_range() {
        let reference = parse_point_reference("ilium.core.smart-copy/after-copy@0.1.0")
            .expect("point reference should parse");
        assert_eq!(reference.plugin, "ilium.core.smart-copy");
        assert_eq!(reference.point, "after-copy");
        assert!(reference.range.matches(&version("0.1.4"), false));
        assert!(!reference.range.matches(&version("0.2.0"), false));
    }

    /// REQ: PKG-328
    #[test]
    fn point_reference_bare_version_is_a_caret_range() {
        let reference = parse_point_reference("com.example.git/status@1")
            .expect("point reference should parse");
        assert!(reference.range.matches(&version("1.9.0"), false));
        assert!(!reference.range.matches(&version("2.0.0"), false));
    }

    /// REQ: PKG-328
    #[test]
    fn point_reference_with_a_bad_range_is_refused_as_a_requires_error() {
        let refusal = parse_point_reference("com.example.git/status@>= 1.0")
            .expect_err("range with a space inside a comparator should be refused");
        assert_eq!(refusal.code.as_str(), "manifest.requires-invalid");
    }

    /// REQ: PKG-340
    #[test]
    fn chosen_source_sha_returns_the_digest_recorded_for_the_plugin() {
        let plan = ResolvedPlan {
            steps: Vec::new(),
            chosen_sources: vec![("com.example.git".to_string(), "sha-git".to_string())],
        };
        assert_eq!(
            plan.chosen_source_sha("com.example.git"),
            Some("sha-git".to_string())
        );
    }

    /// REQ: PKG-340
    #[test]
    fn chosen_source_sha_is_none_for_an_unknown_plugin() {
        let plan = ResolvedPlan {
            steps: Vec::new(),
            chosen_sources: vec![("com.example.git".to_string(), "sha-git".to_string())],
        };
        assert_eq!(plan.chosen_source_sha("com.example.other"), None);
    }

    /// REQ: PKG-340
    #[test]
    fn chosen_source_sha_is_none_for_an_empty_plan() {
        let plan = ResolvedPlan {
            steps: Vec::new(),
            chosen_sources: Vec::new(),
        };
        assert_eq!(plan.chosen_source_sha("com.example.git"), None);
    }

    /// REQ: PKG-330
    #[test]
    fn empty_request_yields_an_empty_plan() {
        let plan = resolve_plan(&request(Vec::new(), Vec::new(), Vec::new()))
            .expect("empty request should resolve");
        assert!(plan.steps.is_empty());
        assert!(plan.chosen_sources.is_empty());
    }

    /// REQ: PKG-338
    #[test]
    fn new_dependency_installs_the_highest_satisfying_version_before_its_dependant() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[("com.example.git", "^1.2", false)],
                None,
            )],
            Vec::new(),
            vec![
                source(
                    "com.example.git",
                    "1.2.0",
                    "sha-git-1.2.0",
                    "stable",
                    0,
                    &[],
                ),
                source(
                    "com.example.git",
                    "1.3.1",
                    "sha-git-1.3.1",
                    "stable",
                    0,
                    &[],
                ),
                source(
                    "com.example.git",
                    "2.0.0",
                    "sha-git-2.0.0",
                    "stable",
                    0,
                    &[],
                ),
            ],
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.steps,
            vec![
                step(PlanAction::Install, "com.example.git", "1.3.1"),
                step(PlanAction::Install, "com.example.reviews", "0.4.0"),
            ]
        );
        assert_eq!(
            plan.chosen_source_sha("com.example.git"),
            Some("sha-git-1.3.1".to_string())
        );
    }

    /// REQ: PKG-339
    #[test]
    fn satisfied_installed_dependency_is_kept_without_change() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[("com.example.git", "^1.0", false)],
                None,
            )],
            vec![plan_package("com.example.git", "1.0.0", &[], None)],
            Vec::new(),
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.steps,
            vec![
                step(PlanAction::Keep, "com.example.git", "1.0.0"),
                step(PlanAction::Install, "com.example.reviews", "0.4.0"),
            ]
        );
    }

    /// REQ: PKG-332
    #[test]
    fn installed_version_is_preferred_over_a_higher_source_when_not_updating() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[("com.example.git", "^1.0", false)],
                None,
            )],
            vec![plan_package("com.example.git", "1.0.0", &[], None)],
            vec![source(
                "com.example.git",
                "1.2.0",
                "sha-git-1.2.0",
                "stable",
                0,
                &[],
            )],
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.steps,
            vec![
                step(PlanAction::Keep, "com.example.git", "1.0.0"),
                step(PlanAction::Install, "com.example.reviews", "0.4.0"),
            ]
        );
    }

    /// REQ: PKG-340
    #[test]
    fn installed_dependency_is_updated_when_a_range_forces_it() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[("com.example.git", "^1.1", false)],
                None,
            )],
            vec![plan_package("com.example.git", "1.0.0", &[], None)],
            vec![source(
                "com.example.git",
                "1.2.0",
                "sha-git-1.2.0",
                "stable",
                0,
                &[],
            )],
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.steps,
            vec![
                step(PlanAction::Update, "com.example.git", "1.2.0"),
                step(PlanAction::Install, "com.example.reviews", "0.4.0"),
            ]
        );
        assert_eq!(
            plan.chosen_source_sha("com.example.git"),
            Some("sha-git-1.2.0".to_string())
        );
    }

    /// REQ: PKG-331
    #[test]
    fn highest_version_satisfying_all_ranges_is_chosen() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[("com.example.git", "^1.0", false)],
                None,
            )],
            Vec::new(),
            vec![
                source("com.example.git", "1.0.0", "sha-1.0.0", "stable", 0, &[]),
                source("com.example.git", "1.9.0", "sha-1.9.0", "stable", 0, &[]),
                source("com.example.git", "2.0.0", "sha-2.0.0", "stable", 0, &[]),
            ],
        );
        let plan = expect_plan(&input);
        assert!(plan
            .steps
            .contains(&step(PlanAction::Install, "com.example.git", "1.9.0")));
    }

    /// REQ: PKG-331
    #[test]
    fn yanked_version_is_never_chosen_for_a_new_install() {
        let mut yanked = source("com.example.git", "1.9.0", "sha-1.9.0", "stable", 0, &[]);
        yanked.yanked = true;
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[("com.example.git", "^1.0", false)],
                None,
            )],
            Vec::new(),
            vec![
                source("com.example.git", "1.0.0", "sha-1.0.0", "stable", 0, &[]),
                yanked,
            ],
        );
        let plan = expect_plan(&input);
        assert!(plan
            .steps
            .contains(&step(PlanAction::Install, "com.example.git", "1.0.0")));
        assert!(!plan
            .steps
            .iter()
            .any(|plan_step| plan_step.version == "1.9.0"));
    }

    /// REQ: PKG-336
    #[test]
    fn equal_candidates_prefer_the_lower_source_rank() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[("com.example.git", "^1.0", false)],
                None,
            )],
            Vec::new(),
            vec![
                source("com.example.git", "1.2.0", "sha-zzz", "stable", 1, &[]),
                source("com.example.git", "1.2.0", "sha-aaa", "stable", 0, &[]),
            ],
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.chosen_source_sha("com.example.git"),
            Some("sha-aaa".to_string())
        );
    }

    /// REQ: PKG-336
    #[test]
    fn equal_candidates_with_equal_rank_prefer_the_lowest_sha256() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[("com.example.git", "^1.0", false)],
                None,
            )],
            Vec::new(),
            vec![
                source("com.example.git", "1.2.0", "sha-bbb", "stable", 0, &[]),
                source("com.example.git", "1.2.0", "sha-aaa", "stable", 0, &[]),
            ],
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.chosen_source_sha("com.example.git"),
            Some("sha-aaa".to_string())
        );
    }

    /// REQ: PKG-333
    #[test]
    fn conflicting_ranges_are_refused_and_list_each_constraint() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[("com.example.git", "^2.0", false)],
                None,
            )],
            vec![plan_package(
                "com.example.lint",
                "1.0.0",
                &[("com.example.git", "^1.0", false)],
                None,
            )],
            vec![
                source("com.example.git", "1.0.0", "sha-1.0.0", "stable", 0, &[]),
                source("com.example.git", "2.0.0", "sha-2.0.0", "stable", 0, &[]),
            ],
        );
        let diagnostics = expect_refusal(&input);
        assert_refused_with(&diagnostics, "dependency.conflict");
        let messages = all_messages(&diagnostics);
        assert!(
            messages.contains("^1.0"),
            "message should list ^1.0: {messages}"
        );
        assert!(
            messages.contains("^2.0"),
            "message should list ^2.0: {messages}"
        );
    }

    /// REQ: PKG-341
    #[test]
    fn conflicting_ranges_name_both_source_plugins() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[("com.example.git", "^2.0", false)],
                None,
            )],
            vec![plan_package(
                "com.example.lint",
                "1.0.0",
                &[("com.example.git", "^1.0", false)],
                None,
            )],
            vec![
                source("com.example.git", "1.0.0", "sha-1.0.0", "stable", 0, &[]),
                source("com.example.git", "2.0.0", "sha-2.0.0", "stable", 0, &[]),
            ],
        );
        let diagnostics = expect_refusal(&input);
        let messages = all_messages(&diagnostics);
        assert!(messages.contains("com.example.lint"), "{messages}");
        assert!(messages.contains("com.example.reviews"), "{messages}");
    }

    /// REQ: PKG-334
    #[test]
    fn required_plugin_with_no_copy_and_no_source_is_unresolved() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[("com.example.git", "^1.0", false)],
                None,
            )],
            Vec::new(),
            Vec::new(),
        );
        let diagnostics = expect_refusal(&input);
        assert_refused_with(&diagnostics, "dependency.unresolved");
    }

    /// REQ: PKG-334
    #[test]
    fn every_missing_required_id_is_listed() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[
                    ("com.example.missing-one", "^1.0", false),
                    ("com.example.missing-two", "^2.0", false),
                ],
                None,
            )],
            Vec::new(),
            Vec::new(),
        );
        let diagnostics = expect_refusal(&input);
        assert_refused_with(&diagnostics, "dependency.unresolved");
        let messages = all_messages(&diagnostics);
        assert!(messages.contains("com.example.missing-one"), "{messages}");
        assert!(messages.contains("com.example.missing-two"), "{messages}");
    }

    /// REQ: PKG-344
    #[test]
    fn missing_optional_dependency_does_not_block_or_install() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[("com.example.theme-pack", "*", true)],
                None,
            )],
            Vec::new(),
            Vec::new(),
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.steps,
            vec![step(PlanAction::Install, "com.example.reviews", "0.4.0")]
        );
    }

    /// REQ: PKG-343
    #[test]
    fn hard_cycle_between_packages_is_refused() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[("com.example.git", "*", false)],
                None,
            )],
            Vec::new(),
            vec![source(
                "com.example.git",
                "1.0.0",
                "sha-git",
                "stable",
                0,
                &[("com.example.reviews", "*", false)],
            )],
        );
        let diagnostics = expect_refusal(&input);
        assert_refused_with(&diagnostics, "dependency.cycle");
    }

    /// REQ: PKG-335
    #[test]
    fn cycle_through_an_optional_edge_does_not_block_install() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[("com.example.git", "*", true)],
                None,
            )],
            Vec::new(),
            vec![source(
                "com.example.git",
                "1.0.0",
                "sha-git",
                "stable",
                0,
                &[("com.example.reviews", "*", false)],
            )],
        );
        assert!(resolve_plan(&input).is_ok());
    }

    /// REQ: PKG-346
    #[test]
    fn host_range_not_satisfied_by_the_running_release_is_refused() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[],
                Some(">=9.0.0"),
            )],
            Vec::new(),
            Vec::new(),
        );
        let diagnostics = expect_refusal(&input);
        assert_refused_with(&diagnostics, "dependency.host-version");
    }

    /// REQ: PKG-325
    #[test]
    fn host_range_satisfied_by_the_running_release_is_accepted() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[],
                Some(">=0.1.0"),
            )],
            Vec::new(),
            Vec::new(),
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.steps,
            vec![step(PlanAction::Install, "com.example.reviews", "0.4.0")]
        );
    }

    /// REQ: PKG-347
    #[test]
    fn prerelease_only_source_is_unresolved_on_the_stable_channel() {
        let input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[("com.example.git", "^1.0", false)],
                None,
            )],
            Vec::new(),
            vec![source(
                "com.example.git",
                "1.3.0-beta.1",
                "sha-beta",
                "beta",
                0,
                &[],
            )],
        );
        let diagnostics = expect_refusal(&input);
        assert_refused_with(&diagnostics, "dependency.unresolved");
    }

    /// REQ: PKG-354
    #[test]
    fn prerelease_source_is_offered_on_the_beta_channel() {
        let mut input = request(
            vec![plan_package(
                "com.example.reviews",
                "0.4.0",
                &[("com.example.git", "^1.0", false)],
                None,
            )],
            Vec::new(),
            vec![source(
                "com.example.git",
                "1.3.0-beta.1",
                "sha-beta",
                "beta",
                0,
                &[],
            )],
        );
        input.channel = "beta".to_string();
        let plan = expect_plan(&input);
        assert!(plan.steps.contains(&step(
            PlanAction::Install,
            "com.example.git",
            "1.3.0-beta.1"
        )));
    }

    /// REQ: PKG-348
    #[test]
    fn updating_a_dependency_out_of_an_installed_dependants_range_is_refused() {
        let mut input = request(
            vec![plan_package("com.example.git", "2.0.0", &[], None)],
            vec![plan_package(
                "com.example.reviews",
                "1.0.0",
                &[("com.example.git", "^1.0", false)],
                None,
            )],
            vec![source(
                "com.example.git",
                "2.0.0",
                "sha-git-2.0.0",
                "stable",
                0,
                &[],
            )],
        );
        input.is_update = true;
        let diagnostics = expect_refusal(&input);
        assert_refused_with(&diagnostics, "dependency.conflict");
    }

    /// REQ: PKG-350
    #[test]
    fn independent_packages_activate_in_plugin_id_byte_order() {
        let input = request(
            vec![
                plan_package("com.example.zeta", "1.0.0", &[], None),
                plan_package("com.example.alpha", "1.0.0", &[], None),
            ],
            Vec::new(),
            Vec::new(),
        );
        let plan = expect_plan(&input);
        assert_eq!(
            plan.steps,
            vec![
                step(PlanAction::Install, "com.example.alpha", "1.0.0"),
                step(PlanAction::Install, "com.example.zeta", "1.0.0"),
            ]
        );
    }

    /// REQ: PKG-350
    #[test]
    fn dependency_activates_before_its_dependant() {
        let input = request(
            vec![plan_package(
                "com.example.app",
                "1.0.0",
                &[("com.example.lib", "^1.0", false)],
                None,
            )],
            Vec::new(),
            vec![source(
                "com.example.lib",
                "1.0.0",
                "sha-lib",
                "stable",
                0,
                &[],
            )],
        );
        let plan = expect_plan(&input);
        let lib_index = plan
            .steps
            .iter()
            .position(|plan_step| plan_step.id == "com.example.lib")
            .expect("lib should be in the plan");
        let app_index = plan
            .steps
            .iter()
            .position(|plan_step| plan_step.id == "com.example.app")
            .expect("app should be in the plan");
        assert!(lib_index < app_index);
    }

    /// REQ: PKG-350
    #[test]
    fn activation_order_of_resolve_puts_a_dependency_first() {
        let packages = vec![
            manifest_requiring("com.example.app", &[("com.example.lib", false)]),
            manifest_requiring("com.example.lib", &[]),
        ];
        let plan = resolve(&packages, &[], api_version()).expect("acyclic graph should resolve");
        assert_eq!(
            plan.activation_order,
            vec!["com.example.lib".to_string(), "com.example.app".to_string()]
        );
    }

    /// REQ: PKG-350
    #[test]
    fn resolve_orders_independent_plugins_by_id() {
        let packages = vec![
            manifest_requiring("com.example.zeta", &[]),
            manifest_requiring("com.example.alpha", &[]),
        ];
        let plan =
            resolve(&packages, &[], api_version()).expect("independent plugins should resolve");
        assert_eq!(
            plan.activation_order,
            vec![
                "com.example.alpha".to_string(),
                "com.example.zeta".to_string()
            ]
        );
    }

    /// REQ: PKG-350
    #[test]
    fn resolve_of_no_packages_yields_an_empty_order() {
        let plan = resolve(&[], &[], api_version()).expect("empty set should resolve");
        assert!(plan.activation_order.is_empty());
    }

    /// REQ: PKG-335
    #[test]
    fn hard_cycle_in_resolve_is_refused_as_dependency_cycle() {
        let packages = vec![
            manifest_requiring("com.example.app", &[("com.example.lib", false)]),
            manifest_requiring("com.example.lib", &[("com.example.app", false)]),
        ];
        let diagnostics =
            resolve(&packages, &[], api_version()).expect_err("hard cycle should be refused");
        assert_refused_with(&diagnostics, "dependency.cycle");
        let messages = all_messages(&diagnostics);
        assert!(messages.contains("com.example.app"), "{messages}");
        assert!(messages.contains("com.example.lib"), "{messages}");
    }

    /// REQ: PKG-335
    #[test]
    fn optional_edge_closing_a_loop_is_not_a_cycle() {
        let packages = vec![
            manifest_requiring("com.example.app", &[("com.example.lib", true)]),
            manifest_requiring("com.example.lib", &[("com.example.app", false)]),
        ];
        assert!(resolve(&packages, &[], api_version()).is_ok());
    }

    /// REQ: PKG-334
    #[test]
    fn hard_requirement_missing_everywhere_is_unresolved_in_resolve() {
        let packages = vec![manifest_requiring(
            "com.example.app",
            &[("com.example.lib", false)],
        )];
        let diagnostics = resolve(&packages, &[], api_version())
            .expect_err("missing hard requirement should be refused");
        assert_refused_with(&diagnostics, "dependency.unresolved");
        assert!(all_messages(&diagnostics).contains("com.example.lib"));
    }

    /// REQ: PKG-334
    #[test]
    fn hard_requirement_satisfied_by_an_installed_copy_resolves() {
        let packages = vec![manifest_requiring(
            "com.example.app",
            &[("com.example.lib", false)],
        )];
        let installed = vec![InstalledSummary {
            id: "com.example.lib".to_string(),
            version: "1.0.0".to_string(),
            services: Vec::new(),
            settings_owner_tabs: Vec::new(),
        }];
        assert!(resolve(&packages, &installed, api_version()).is_ok());
    }
}
