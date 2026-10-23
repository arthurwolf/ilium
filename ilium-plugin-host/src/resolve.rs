//! Dependency and service resolution across a set of packages (reference-manifest.md PKG-650..660).

use ilium_plugin_api::manifest::{Diagnostic, InstalledSummary, Manifest};
use ilium_plugin_api::ApiVersion;
use std::collections::{BTreeMap, BTreeSet};
use std::cmp::Ordering;

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
    let incoming: Vec<PlanPackage> = packages.iter().map(|package| PlanPackage {
        id: package.id.clone(), version: package.version.clone(),
        needs: package.requires.as_ref().map(|r| r.plugins.iter().map(|p| (p.id.clone(), p.version.clone(), p.optional)).collect()).unwrap_or_default(),
        host: package.requires.as_ref().and_then(|r| r.host.clone()),
    }).collect();
    let incoming = incoming.into_iter().map(|mut package| {
        for (id, range, _) in &mut package.needs {
            let original_present = packages.iter().any(|p| p.id == *id) || installed.iter().any(|p| p.id == *id);
            if !original_present {
                if let Some(replacement) = packages.iter().filter(|p| p.replaces.contains(id)).find(|p| PluginVersion::parse(&p.version).is_ok_and(|v| VersionRange::parse(range).is_ok_and(|r| r.matches(&v, false)))) { *id = replacement.id.clone(); }
            }
        }
        package
    }).collect();
    let installed_packages = installed.iter().filter(|p| !packages.iter().any(|incoming| incoming.id == p.id)).map(|p| PlanPackage { id: p.id.clone(), version: p.version.clone(), needs: Vec::new(), host: None }).collect();
    let plan = resolve_plan(&ResolveInput { incoming, installed: installed_packages, sources: Vec::new(), channel: "stable".into(), host_release: format!("{}.{}.{}", host.major, host.minor, host.patch), is_update: false, pins: Vec::new() })?;
    let mut graph: BTreeMap<String, BTreeSet<String>> = plan.steps.iter().map(|step| (step.id.clone(), BTreeSet::new())).collect();
    let mut providers: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for package in installed.iter().filter(|p| !packages.iter().any(|incoming| incoming.id == p.id)) {
        for (name, version) in &package.services { providers.entry(name.clone()).or_default().push((package.id.clone(), version.clone())); }
    }
    for package in packages {
        if let Some(entries) = package.contributes.0.get("services").and_then(|v| v.get("provides")).and_then(serde_json::Value::as_array) {
            for entry in entries {
                let Some(name) = entry.get("name").and_then(serde_json::Value::as_str) else { return Err(vec![diagnostic("manifest.requires-invalid", "service provider missing name")]); };
                let Some(version) = entry.get("version").and_then(serde_json::Value::as_str) else { return Err(vec![diagnostic("manifest.service-version", "service provider missing version")]); };
                PluginVersion::parse(version).map_err(|e| vec![e])?;
                providers.entry(name.into()).or_default().push((package.id.clone(), version.into()));
            }
        }
    }
    let mut service_labels = BTreeMap::new();
    for package in packages {
        if let Some(requires) = &package.requires {
            if requires.services.len() > 32 { return Err(vec![diagnostic("manifest.requires-invalid", "at most 32 required services are allowed")]); }
            for need in requires.plugins.iter().filter(|p| !p.optional) {
                let target = if graph.contains_key(&need.id) { need.id.clone() } else {
                    packages.iter().find(|p| p.replaces.contains(&need.id) && PluginVersion::parse(&p.version).is_ok_and(|v| VersionRange::parse(&need.version).is_ok_and(|r| r.matches(&v, false)))).map(|p| p.id.clone()).unwrap_or_else(|| need.id.clone())
                };
                graph.entry(package.id.clone()).or_default().insert(target);
            }
            for text in &requires.services {
                let requirement = parse_service_requirement(text).map_err(|e| vec![e])?;
                let provider = providers.get(&requirement.interface).and_then(|options| options.iter().filter_map(|(id, version)| PluginVersion::parse(version).ok().filter(|v| requirement.range.matches(v, false)).map(|version| (id, version))).max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0))));
                if let Some((id, _)) = provider {
                    graph.entry(package.id.clone()).or_default().insert(id.clone());
                    service_labels.insert((package.id.clone(), id.clone()), requirement.interface);
                }
            }
        }
    }
    let order = topological_order(graph).map_err(|cycle| {
        let mut names: Vec<String> = cycle.windows(2).filter_map(|edge| service_labels.get(&(edge[0].clone(), edge[1].clone())).cloned()).collect();
        if let Some((index, _)) = names.iter().enumerate().min_by_key(|(_, name)| *name) { names.rotate_left(index); }
        vec![diagnostic(if names.is_empty() { "dependency.cycle" } else { "manifest.service-cycle" }, format!("cycle: {}", if names.is_empty() { cycle } else { names }.join(" -> ")))]
    })?;
    Ok(ResolutionPlan { activation_order: order })
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
        let parsed = semver::Version::parse(text).map_err(|_| diagnostic("manifest.version-invalid", format!("invalid version {text:?}")))?;
        if text.len() > 64 || [parsed.major, parsed.minor, parsed.patch].iter().any(|number| *number > 2_147_483_647) {
            return Err(diagnostic("manifest.version-invalid", format!("version outside limits: {text}")));
        }
        Ok(Self { major: parsed.major, minor: parsed.minor, patch: parsed.patch, pre_release: if parsed.pre.is_empty() { Vec::new() } else { parsed.pre.as_str().split('.').map(str::to_owned).collect() } })
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
        let invalid = || diagnostic("manifest.requires-invalid", format!("invalid range {text:?}"));
        if text.is_empty() || text.len() > 128 || text.contains("||") || text.contains(',') || text.split(' ').any(str::is_empty) || text.chars().any(|c| c.is_whitespace() && c != ' ') { return Err(invalid()); }
        let mut comparators = Vec::new();
        for token in text.split(' ') {
            if token == "*" { continue; }
            if token.contains('*') || token.contains('+') { return Err(invalid()); }
            let comparator = semver::Comparator::parse(token).map_err(|_| invalid())?;
            if comparator.op == semver::Op::Wildcard { return Err(invalid()); }
            if comparator.major > 2_147_483_647 || comparator.minor.is_some_and(|v| v > 2_147_483_647) || comparator.patch.is_some_and(|v| v > 2_147_483_647) { return Err(invalid()); }
            comparators.push(comparator);
        }
        Ok(Self { comparators })
    }

    /// Whether a version satisfies the range; pre-releases match only when allowed (PKG-315).
    pub fn matches(&self, version: &PluginVersion, allow_prerelease: bool) -> bool {
        if semver::Prerelease::new(&version.pre_release.join(".")).is_err() { return false; }
        let version = version.semver();
        if !version.pre.is_empty() && !allow_prerelease && !self.comparators.iter().any(|c| !c.pre.is_empty() && c.major == version.major && c.minor == Some(version.minor) && c.patch == Some(version.patch)) { return false; }
        self.comparators.iter().all(|comparator| comparator_matches(comparator, &version))
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
    let (interface, version) = text.rsplit_once('@').ok_or_else(|| diagnostic("manifest.service-version", "service requires a full version"))?;
    ilium_plugin_api::grants::Grant::parse(&format!("services.use:{interface}"))?;
    PluginVersion::parse(version).map_err(|_| diagnostic("manifest.service-version", "service requires a full version"))?;
    Ok(ServiceRequirement { interface: interface.to_owned(), range: VersionRange::parse(&format!("^{version}"))? })
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
    let invalid = || diagnostic("manifest.requires-invalid", format!("invalid point reference {text:?}"));
    let (name, range) = text.rsplit_once('@').ok_or_else(invalid)?;
    let (plugin, point) = name.split_once('/').ok_or_else(invalid)?;
    ilium_plugin_api::ids::parse_plugin_id(plugin).map_err(|_| invalid())?;
    ilium_plugin_api::grants::Grant::parse(&format!("extensions.provide:{name}")).map_err(|_| invalid())?;
    if point.is_empty() || !point.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') { return Err(invalid()); }
    Ok(PointReference { plugin: plugin.to_owned(), point: point.to_owned(), range: VersionRange::parse(range)? })
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
        self.chosen_sources.iter().find(|(plugin, _)| plugin == id).map(|(_, sha)| sha.clone())
    }
}

/// Resolves a request into a plan; conflicts are diagnostics (PKG-330..348).
pub fn resolve_plan(input: &ResolveInput) -> Result<ResolvedPlan, Vec<Diagnostic>> {
    let host = PluginVersion::parse(&input.host_release).map_err(|error| vec![error])?;
    for packages in [&input.incoming, &input.installed] {
        let mut ids = BTreeSet::new();
        for package in packages {
            if !ids.insert(&package.id) { return Err(vec![diagnostic("dependency.conflict", format!("multiple versions requested for {}", package.id))]); }
        }
    }
    let mut candidates: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    for source in &input.sources {
        if source.yanked || !(source.channel == "stable" || input.channel == "beta" && source.channel == "beta") { continue; }
        let version = PluginVersion::parse(&source.package.version).map_err(|error| vec![error])?;
        if input.channel != "beta" && !version.pre_release.is_empty() { continue; }
        candidates.entry(source.package.id.clone()).or_default().push(Candidate { package: source.package.clone(), version, sha: Some(source.sha256.clone()), rank: source.source_rank, installed: false });
    }
    for package in &input.installed {
        candidates.entry(package.id.clone()).or_default().push(Candidate::local(package, true).map_err(|e| vec![e])?);
    }
    for options in candidates.values_mut() {
        options.sort_by(|left, right| {
            let installed = if input.is_update { Ordering::Equal } else { right.installed.cmp(&left.installed) };
            installed.then_with(|| right.version.cmp(&left.version)).then_with(|| left.rank.cmp(&right.rank)).then_with(|| left.sha.cmp(&right.sha))
        });
    }
    for id in &input.pins {
        if let Some(installed) = input.installed.iter().find(|p| &p.id == id) {
            let version = PluginVersion::parse(&installed.version).map_err(|e| vec![e])?;
            if let Some(options) = candidates.get_mut(id) { options.retain(|c| c.version == version); }
            if input.incoming.iter().any(|p| &p.id == id && PluginVersion::parse(&p.version).is_ok_and(|v| v != version)) {
                return Err(vec![diagnostic("dependency.conflict", format!("{id} is pinned to {}", installed.version))]);
            }
        }
    }
    // Incoming file choices are fixed; dependencies may never silently replace them.
    for package in &input.incoming {
        candidates.insert(package.id.clone(), vec![Candidate::local(package, false).map_err(|e| vec![e])?]);
    }
    for options in candidates.values() {
        for candidate in options {
            validate_package(&candidate.package).map_err(|e| vec![e])?;
        }
    }
    let roots: BTreeSet<String> = input.installed.iter().chain(&input.incoming).map(|p| p.id.clone()).collect();
    let mut selected = BTreeMap::new();
    let mut failures = Vec::new();
    if !search(&roots, &candidates, &mut selected, &host, input.channel == "beta", &mut failures) {
        if failures.is_empty() { failures.push(diagnostic("dependency.conflict", "no common version; use ilium plugin update --all")); }
        return Err(failures);
    }
    let order = activation_order(&selected).map_err(|e| vec![e])?;
    let steps = order.into_iter().map(|id| {
        let package = &selected[&id].package;
        let action = match input.installed.iter().find(|old| old.id == id) {
            None => PlanAction::Install,
            Some(old) if PluginVersion::parse(&old.version).ok() == Some(selected[&id].version.clone()) => PlanAction::Keep,
            Some(_) => PlanAction::Update,
        };
        PlanStep { action, id, version: package.version.clone() }
    }).collect();
    let chosen_sources = selected.iter().filter_map(|(id, candidate)| candidate.sha.as_ref().map(|sha| (id.clone(), sha.clone()))).collect();
    Ok(ResolvedPlan { steps, chosen_sources })
}


fn diagnostic(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(ilium_plugin_api::error::Code::from_static(code), "requires", message)
}

impl PluginVersion {
    fn semver(&self) -> semver::Version {
        let mut version = semver::Version::new(self.major, self.minor, self.patch);
        // Public fields may be supplied by callers; malformed prereleases cannot match.
        if let Ok(pre) = semver::Prerelease::new(&self.pre_release.join(".")) { version.pre = pre; }
        version
    }
}
impl Ord for PluginVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        let core = (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch));
        if core != Ordering::Equal { return core; }
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
            if order != Ordering::Equal { return order; }
        }
        self.pre_release.len().cmp(&other.pre_release.len())
    }
}
impl PartialOrd for PluginVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
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
        Ok(Self { package: package.clone(), version: PluginVersion::parse(&package.version)?, sha: None, rank: usize::MAX, installed })
    }
}

fn validate_package(package: &PlanPackage) -> Result<(), Diagnostic> {
    let mut seen = BTreeSet::new();
    if package.needs.len() > 32 { return Err(diagnostic("manifest.requires-invalid", "at most 32 plugin requirements are allowed")); }
    for (id, range, _) in &package.needs {
        if id == &package.id { return Err(diagnostic("manifest.requires-self", format!("{} requires itself", package.id))); }
        if !seen.insert(id) { return Err(diagnostic("manifest.requires-invalid", format!("duplicate requirement {id}"))); }
        VersionRange::parse(range)?;
    }
    if let Some(range) = &package.host { VersionRange::parse(range)?; }
    Ok(())
}

fn constraints(selected: &BTreeMap<String, Candidate>, id: &str) -> Vec<(String, String)> {
    selected.values().flat_map(|candidate| candidate.package.needs.iter().filter(move |(need, _, optional)| !optional && need == id).map(move |(_, range, _)| (candidate.package.id.clone(), range.clone()))).collect()
}

fn accepts(candidate: &Candidate, constraints: &[(String, String)], beta: bool) -> bool {
    constraints.iter().all(|(_, text)| VersionRange::parse(text).is_ok_and(|range| range.matches(&candidate.version, beta)))
}

fn search(
    roots: &BTreeSet<String>, candidates: &BTreeMap<String, Vec<Candidate>>,
    selected: &mut BTreeMap<String, Candidate>, host: &PluginVersion, beta: bool,
    failures: &mut Vec<Diagnostic>,
) -> bool {
    // Every candidate's outgoing constraints are rechecked when a later node is assigned.
    for (id, candidate) in selected.iter() {
        if !accepts(candidate, &constraints(selected, id), beta) {
            *failures = vec![diagnostic("dependency.conflict", format!("{id} constraints {:?}; available {:?}; use ilium plugin update --all", constraints(selected, id), candidates.get(id).map(|options| options.iter().map(|c| &c.package.version).collect::<Vec<_>>())))];
            return false;
        }
    }
    let required: BTreeSet<String> = roots.iter().cloned().chain(selected.values().flat_map(|c| c.package.needs.iter().filter(|(_, _, optional)| !optional).map(|(id, _, _)| id.clone()))).collect();
    let missing: Vec<_> = required.iter().filter(|id| !selected.contains_key(*id) && !candidates.contains_key(*id)).collect();
    if !missing.is_empty() {
        *failures = missing.into_iter().map(|id| diagnostic("dependency.unresolved", format!("missing {id}: {:?}", constraints(selected, id)))).collect();
        return false;
    }
    let Some(id) = required.iter().find(|id| !selected.contains_key(*id)) else {
        return match activation_order(selected) {
            Ok(_) => true,
            Err(error) => { *failures = vec![error]; false }
        };
    };
    let requirements = constraints(selected, id);
    let mut host_failure = None;
    for candidate in &candidates[id] {
        if !accepts(candidate, &requirements, beta) { continue; }
        if candidate.package.host.as_ref().is_some_and(|text| !VersionRange::parse(text).is_ok_and(|range| range.matches(host, beta))) {
            host_failure = Some(diagnostic("dependency.host-version", format!("{} requires host {:?}, running {}.{}.{}", id, candidate.package.host, host.major, host.minor, host.patch)));
            continue;
        }
        selected.insert(id.clone(), candidate.clone());
        if search(roots, candidates, selected, host, beta, failures) { return true; }
        selected.remove(id);
    }
    if let Some(error) = host_failure { *failures = vec![error]; }
    else if failures.is_empty() {
        let all_constraints: Vec<_> = selected.values().flat_map(|c| c.package.needs.iter().map(move |(id, range, _)| format!("{} requires {id} {range}", c.package.id))).collect();
        *failures = vec![diagnostic("dependency.conflict", format!("constraints: {all_constraints:?}; available: {:?}; use ilium plugin update --all", candidates.iter().map(|(id, options)| (id, options.iter().map(|c| &c.package.version).collect::<Vec<_>>())).collect::<Vec<_>>()))];
    }
    false
}

fn activation_order(selected: &BTreeMap<String, Candidate>) -> Result<Vec<String>, Diagnostic> {
    let graph = selected.iter().map(|(id, c)| (id.clone(), c.package.needs.iter().filter(|(_, _, optional)| !optional).map(|(id, _, _)| id.clone()).collect())).collect();
    topological_order(graph).map_err(|cycle| diagnostic("dependency.cycle", format!("hard dependency cycle: {}", cycle.join(" -> "))))
}


fn comparator_matches(comparator: &semver::Comparator, version: &semver::Version) -> bool {
    use semver::Op;
    let mut lower = semver::Version::new(comparator.major, comparator.minor.unwrap_or(0), comparator.patch.unwrap_or(0));
    lower.pre = comparator.pre.clone();
    let upper = match comparator.op {
        Op::Caret if comparator.major > 0 || comparator.minor.is_none() => semver::Version::new(comparator.major + 1, 0, 0),
        Op::Caret if comparator.minor != Some(0) || comparator.patch.is_none() => semver::Version::new(0, comparator.minor.unwrap_or(0) + 1, 0),
        Op::Caret => semver::Version::new(0, 0, comparator.patch.unwrap_or(0) + 1),
        _ if comparator.minor.is_none() => semver::Version::new(comparator.major + 1, 0, 0),
        _ => semver::Version::new(comparator.major, comparator.minor.unwrap_or(0) + 1, 0),
    };
    match comparator.op {
        Op::Caret | Op::Tilde => version >= &lower && version < &upper,
        Op::Exact | Op::Wildcard => if comparator.patch.is_some() { version == &lower } else { version >= &lower && version < &upper },
        Op::Greater => if comparator.patch.is_some() { version > &lower } else { version >= &upper },
        Op::GreaterEq => version >= &lower,
        Op::Less => version < &lower,
        Op::LessEq => if comparator.patch.is_some() { version <= &lower } else { version < &upper },
        _ => false,
    }
}

fn topological_order(mut graph: BTreeMap<String, BTreeSet<String>>) -> Result<Vec<String>, Vec<String>> {
    let mut order = Vec::new();
    while !graph.is_empty() {
        let Some(id) = graph.iter().find(|(_, needs)| needs.is_empty()).map(|(id, _)| id.clone()) else {
            // Follow real graph edges so the diagnostic names a cycle, not downstream nodes.
            let Some(start) = graph.keys().next().cloned() else { return Ok(order); };
            let mut path = Vec::new();
            let mut current = start;
            loop {
                if let Some(index) = path.iter().position(|id| id == &current) {
                    let mut cycle = path[index..].to_vec();
                    if let Some((index, _)) = cycle.iter().enumerate().min_by_key(|(_, id)| *id) { cycle.rotate_left(index); }
                    if let Some(first) = cycle.first().cloned() { cycle.push(first); }
                    return Err(cycle);
                }
                path.push(current.clone());
                let Some(next) = graph.get(&current).and_then(|needs| needs.first()).cloned() else { return Err(path); };
                current = next;
            }
        };
        graph.remove(&id);
        for needs in graph.values_mut() { needs.remove(&id); }
        order.push(id);
    }
    Ok(order)
}
