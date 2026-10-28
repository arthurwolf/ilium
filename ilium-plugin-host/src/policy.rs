//! Pure administrator-policy evaluation (SEC-180..199). Filesystem ownership,
//! audit emission, revocation and review are supplied by the host, not simulated here.

use ilium_plugin_api::{
    grants::{self, Grant},
    Tier,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyVerdict {
    Allow,
    Deny(String),
}

/// Facts obtained from the platform's opened-file ownership/ACL inspection.
/// Do not derive these from the pathname or from the process's privileges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyOwnership {
    pub administrator_owned: bool,
    pub non_administrator_writable: bool,
}

/// A start-time snapshot. No evaluation method reopens or reloads files.
#[derive(Debug, Clone)]
pub enum PolicyInput {
    Missing,
    Unreadable(String),
    Document {
        text: String,
        ownership: Option<PolicyOwnership>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct PolicyEngine {
    layers: Vec<Policy>,
    invalid: Vec<String>,
    notices: Vec<String>,
}

/// Package metadata already established by signature/trust and source admission.
#[derive(Debug, Clone)]
pub struct InstallPolicyContext<'a> {
    pub plugin: &'a str,
    pub tier: Tier,
    pub publisher_key: Option<&'a str>,
    /// Canonical source identity established by filesystem/URL admission.
    pub source: &'a str,
    pub source_kind: PolicySourceKind,
    pub channel: &'a str,
    pub development: bool,
    pub replaces: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicySourceKind {
    File,
    Url,
    Registry,
    Bundle,
    Development,
}

/// Pure install/review decisions. Grant denials lock rows off; they do not turn
/// an otherwise admitted package into an install failure (SEC-198).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPolicyReview {
    pub installation: PolicyVerdict,
    pub grants: Vec<(Grant, PolicyVerdict)>,
}

impl PolicyEngine {
    pub fn new() -> Self {
        Self::default()
    }
    /// System ownership is mandatory; unverifiable ownership fails closed. A known
    /// insecure system file is ignored with a notice, as SEC-182 explicitly requires.
    pub fn from_inputs(system: PolicyInput, user: PolicyInput) -> Self {
        let mut engine = Self::default();
        for (is_system, input) in [(true, system), (false, user)] {
            match input {
                PolicyInput::Missing => {}
                PolicyInput::Unreadable(reason) => engine.invalid.push(reason),
                PolicyInput::Document { text, ownership } => {
                    if is_system {
                        match ownership {
                            Some(owner)
                                if !owner.administrator_owned
                                    || owner.non_administrator_writable =>
                            {
                                engine.notices.push(
                                    "Admin policy ignored: file is writable by non-administrators"
                                        .into(),
                                );
                                continue;
                            }
                            None => {
                                engine
                                    .invalid
                                    .push("system policy ownership was not verified".into());
                                continue;
                            }
                            _ => {}
                        }
                    }
                    match parse(&text) {
                        Ok(mut policy) => {
                            // User documents cannot manufacture administrator trust or
                            // preapproval. These are authority, not restrictions.
                            if !is_system {
                                policy.publishers.trusted_keys.clear();
                                policy.grants.preapproved.clear();
                                policy.network.ca_bundle = None;
                                policy.network.proxy = None;
                            }
                            engine.layers.push(policy);
                        }
                        Err(reason) => engine.invalid.push(reason),
                    }
                }
            }
        }
        engine
    }
    /// Administrative inspection only. Every layer is conjunctive; do not union
    /// its allow lists or pass this configuration text to plugin callers.
    pub fn configuration_layers(&self) -> Result<Vec<toml::Value>, String> {
        self.layers
            .iter()
            .map(|policy| toml::Value::try_from(policy).map_err(|e| e.to_string()))
            .collect()
    }
    pub fn notices(&self) -> &[String] {
        &self.notices
    }
    pub fn errors(&self) -> &[String] {
        &self.invalid
    }
    pub fn is_valid(&self) -> bool {
        self.invalid.is_empty()
    }

    /// Compatibility boundary with no authenticated tier: conservatively third-party.
    pub fn check_grant(&self, plugin: &str, grant: &Grant) -> PolicyVerdict {
        self.check_grant_for(plugin, Tier::T3, grant)
    }
    pub fn check_grant_for(&self, _plugin: &str, tier: Tier, grant: &Grant) -> PolicyVerdict {
        if tier != Tier::T1 && !self.is_valid() {
            return deny("policy.invalid");
        }
        let individual = self.check_individual_grant(tier, grant);
        if individual != PolicyVerdict::Allow {
            return individual;
        }
        // Only potential combination members need the missing same-side context.
        // This API cannot certify their aggregate authorization in isolation.
        if self.denies_combinations() && crate::review::grant_group(grant.as_str()).is_some() {
            return deny("policy.context-required");
        }
        PolicyVerdict::Allow
    }

    fn denies_combinations(&self) -> bool {
        self.layers
            .iter()
            .any(|layer| layer.grants.deny_combinations == Some(true))
    }

    fn check_individual_grant(&self, tier: Tier, grant: &Grant) -> PolicyVerdict {
        if tier != Tier::T1 && !self.is_valid() {
            return deny("policy.invalid");
        }
        for layer in &self.layers {
            if layer
                .grants
                .deny
                .iter()
                .any(|pattern| grant_matches(pattern, grant))
                || layer.grants.max_risk.as_deref() == Some("normal")
                    && grants::is_high_risk(grant.as_str())
                || grant.family() == "extensions.contribute"
                    && grant.argument().is_some_and(|point| {
                        layer.extensions.deny_points.iter().any(|p| p == point)
                    })
            {
                return deny("policy.grant-forbidden");
            }
        }
        PolicyVerdict::Allow
    }
    /// Evaluates a candidate against the actual held/requested grants of ONE side.
    /// Include every retained grant. The candidate is added if not already present,
    /// so callers cannot accidentally authorize a newly introduced toxic pair by
    /// supplying only the previously held set. This is policy, not grant possession.
    pub fn check_grant_in_set_for(
        &self,
        _plugin: &str,
        tier: Tier,
        grant: &Grant,
        same_side: &[Grant],
    ) -> PolicyVerdict {
        let individual = self.check_individual_grant(tier, grant);
        if individual != PolicyVerdict::Allow {
            return individual;
        }
        if !self.denies_combinations() || crate::review::grant_group(grant.as_str()).is_none() {
            return PolicyVerdict::Allow;
        }
        let mut combined = same_side.to_vec();
        if !combined.contains(grant) {
            combined.push(grant.clone());
        }
        if crate::review::has_toxic_combination(&combined) {
            return deny("policy.grant-forbidden");
        }
        PolicyVerdict::Allow
    }

    /// Install admission and the row decisions for ONE side, kept separate.
    /// Call once per side with its full requested set. No consent or installation
    /// is performed, and a denied grant does not reject the package.
    pub fn review_install_grants(
        &self,
        package: &InstallPolicyContext<'_>,
        requested: &[Grant],
    ) -> InstallPolicyReview {
        self.review_install_grants_with_retained(package, requested, &[])
    }

    /// Evaluates new requested rows against the actual grants retained on the
    /// same side. Retained grants must come from the host authorization store,
    /// not from a package claim. This does not confer possession of either set.
    pub fn review_install_grants_with_retained(
        &self,
        package: &InstallPolicyContext<'_>,
        requested: &[Grant],
        retained: &[Grant],
    ) -> InstallPolicyReview {
        let mut same_side = retained.to_vec();
        for grant in requested {
            if !same_side.contains(grant) {
                same_side.push(grant.clone());
            }
        }
        InstallPolicyReview {
            installation: self.check_install(package),
            grants: requested
                .iter()
                .map(|grant| {
                    (
                        grant.clone(),
                        self.check_grant_in_set_for(
                            package.plugin,
                            package.tier,
                            grant,
                            &same_side,
                        ),
                    )
                })
                .collect(),
        }
    }

    /// Host policy only: this is not a DNS/private-address, TLS, grant or reserved-port check.
    pub fn check_egress(&self, plugin: &str, host: &str) -> PolicyVerdict {
        self.check_egress_for(plugin, Tier::T3, host)
    }
    pub fn check_egress_for(&self, _plugin: &str, tier: Tier, host: &str) -> PolicyVerdict {
        if tier != Tier::T1 && !self.is_valid() {
            return deny("policy.invalid");
        }
        // Parse a concrete destination as a grant argument; wildcard/setting targets
        // are scopes rather than actual destinations and cannot authorize egress.
        if host.contains('*')
            || host.starts_with("setting:")
            || Grant::parse(&format!("http:{host}")).is_err()
        {
            return deny("policy.host-invalid");
        }
        if self
            .layers
            .iter()
            .any(|layer| layer.network.offline.unwrap_or(false))
        {
            return deny("offline");
        }
        for layer in &self.layers {
            if layer
                .network
                .deny_hosts
                .iter()
                .any(|pattern| host_matches(pattern, host))
                || layer.network.allow_hosts.as_ref().is_some_and(|patterns| {
                    !patterns.iter().any(|pattern| host_matches(pattern, host))
                })
            {
                return deny("policy.host-denied");
            }
        }
        PolicyVerdict::Allow
    }
    /// Install restrictions, in USR-217's first-failure order. Denied grants are
    /// deliberately excluded: SEC-198 locks them off without failing installation.
    pub fn check_install(&self, package: &InstallPolicyContext<'_>) -> PolicyVerdict {
        if package.tier == Tier::T1 {
            return PolicyVerdict::Allow;
        }
        if !self.is_valid() {
            return deny("policy.invalid");
        }
        if self
            .layers
            .iter()
            .any(|p| p.packages.allow_third_party == Some(false))
        {
            return deny("policy.third-party-denied");
        }
        if self.layers.iter().any(|p| {
            p.plugins
                .deny_ids
                .iter()
                .any(|pattern| glob(pattern, package.plugin))
        }) {
            return deny("policy.id-denied");
        }
        if self.layers.iter().any(|p| {
            p.plugins.allow_ids.as_ref().is_some_and(|patterns| {
                !patterns.iter().any(|pattern| glob(pattern, package.plugin))
            })
        }) {
            return deny("policy.id-denied");
        }
        let tier = match package.tier {
            Tier::T1 => "t1",
            Tier::T2 => "t2",
            Tier::T3 => "t3",
        };
        if self.layers.iter().any(|p| {
            p.packages
                .allowed_tiers
                .as_ref()
                .is_some_and(|tiers| !tiers.iter().any(|allowed| allowed == tier))
        }) {
            return deny("policy.tier-denied");
        }
        if self.layers.iter().any(|p| {
            package
                .publisher_key
                .is_some_and(|key| p.publishers.deny.iter().any(|denied| denied == key))
                || p.publishers.allow.as_ref().is_some_and(|allowed| {
                    !package
                        .publisher_key
                        .is_some_and(|key| allowed.iter().any(|a| a == key))
                })
        }) {
            return deny("policy.publisher-denied");
        }
        for policy in &self.layers {
            if policy
                .packages
                .allowed_sources
                .as_ref()
                .is_some_and(|sources| {
                    !sources
                        .iter()
                        .any(|source| source_contains(source, package.source))
                })
            {
                return deny("policy.source-denied");
            }
            if policy.packages.allow_url_installs == Some(false)
                && matches!(
                    package.source_kind,
                    PolicySourceKind::File | PolicySourceKind::Url
                )
            {
                return deny("policy.source-denied");
            }
        }
        for policy in &self.layers {
            if package.tier == Tier::T3 && policy.packages.allow_unsigned == Some(false) {
                return deny("policy.unsigned-denied");
            }
            if package.development && policy.packages.allow_dev_mode == Some(false) {
                return deny("policy.dev-denied");
            }
            if policy
                .packages
                .allowed_channels
                .as_ref()
                .is_some_and(|channels| !channels.iter().any(|channel| channel == package.channel))
            {
                return deny("policy.channel-denied");
            }
            if package.replaces && policy.extensions.allow_replacement == Some(false) {
                return deny("policy.replacement");
            }
            // These require external authenticated documents/runtime inventory.
            if policy.packages.blocklist_file.is_some()
                || policy.packages.block_vulnerable_runtime == Some(true)
            {
                return deny("policy.context-required");
            }
        }
        PolicyVerdict::Allow
    }
    pub fn check_enabled_change(&self, plugin: &str, enabled: bool) -> PolicyVerdict {
        if !self.is_valid() {
            return deny("policy.invalid");
        }
        if self.layers.iter().any(|p| {
            if enabled {
                p.plugins.force_disabled.iter().any(|id| id == plugin)
                    || p.plugins
                        .deny_ids
                        .iter()
                        .any(|pattern| glob(pattern, plugin))
            } else {
                p.plugins.force_enabled.iter().any(|id| id == plugin)
                    && !self.layers.iter().any(|p| {
                        p.plugins.force_disabled.iter().any(|id| id == plugin)
                            || p.plugins
                                .deny_ids
                                .iter()
                                .any(|pattern| glob(pattern, plugin))
                    })
            }
        }) {
            return deny("policy.forced-state");
        }
        PolicyVerdict::Allow
    }
    pub fn allows_project_values(&self) -> bool {
        self.is_valid()
            && self
                .layers
                .iter()
                .all(|p| p.projects.allow_project_values != Some(false))
    }
    pub fn allows_provider(&self, provider: &str) -> bool {
        self.is_valid()
            && self.layers.iter().all(|p| {
                p.inference
                    .allowed_providers
                    .as_ref()
                    .is_none_or(|allowed| allowed.iter().any(|p| p == provider))
            })
    }
    pub fn denied_environment(&self) -> Vec<String> {
        self.layers
            .iter()
            .flat_map(|p| p.processes.deny_env.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    pub fn locked_value(&self, key: &str) -> Option<&toml::Value> {
        self.layers.iter().find_map(|p| p.settings.locked.get(key))
    }
    /// Preapproved grants from the complete requested set of ONE side.
    /// Administrator preapproval never overrides a restrictive policy layer.
    pub fn preapproved(&self, plugin: &str, requested: &[Grant]) -> Vec<Grant> {
        if !self.is_valid() {
            return Vec::new();
        }
        requested
            .iter()
            .filter(|grant| {
                self.layers.iter().any(|p| {
                    p.grants
                        .preapproved
                        .get(plugin)
                        .is_some_and(|approved| approved.iter().any(|s| s == grant.as_str()))
                }) && self.check_grant_in_set_for(plugin, Tier::T3, grant, requested)
                    == PolicyVerdict::Allow
            })
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Policy {
    version: Option<u32>,
    packages: Packages,
    publishers: Publishers,
    plugins: Plugins,
    grants: Grants,
    extensions: Extensions,
    network: Network,
    processes: Processes,
    settings: Settings,
    inference: Inference,
    projects: Projects,
    audit: Audit,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Packages {
    allow_third_party: Option<bool>,
    allowed_tiers: Option<Vec<String>>,
    allow_unsigned: Option<bool>,
    allow_dev_mode: Option<bool>,
    allowed_sources: Option<Vec<String>>,
    blocklist_file: Option<String>,
    precompiled_cache: Option<bool>,
    allow_url_installs: Option<bool>,
    allowed_channels: Option<Vec<String>>,
    block_vulnerable_runtime: Option<bool>,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Publishers {
    trusted_keys: Vec<String>,
    allow: Option<Vec<String>>,
    deny: Vec<String>,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Plugins {
    allow_ids: Option<Vec<String>>,
    deny_ids: Vec<String>,
    force_enabled: Vec<String>,
    force_disabled: Vec<String>,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Grants {
    deny: Vec<String>,
    max_risk: Option<String>,
    deny_combinations: Option<bool>,
    preapproved: BTreeMap<String, Vec<String>>,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Extensions {
    allow_replacement: Option<bool>,
    deny_points: Vec<String>,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Network {
    allow_hosts: Option<Vec<String>>,
    deny_hosts: Vec<String>,
    ca_bundle: Option<String>,
    proxy: Option<String>,
    offline: Option<bool>,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Processes {
    deny_env: Vec<String>,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Settings {
    locked: BTreeMap<String, toml::Value>,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Inference {
    allowed_providers: Option<Vec<String>>,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Projects {
    allow_project_values: Option<bool>,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Audit {
    sink: Option<String>,
    max_mib: Option<i64>,
    keep_days: Option<i64>,
}

fn deny(code: &str) -> PolicyVerdict {
    PolicyVerdict::Deny(code.into())
}
fn parse(text: &str) -> Result<Policy, String> {
    let policy: Policy = toml::from_str(text).map_err(|e| e.to_string())?;
    if policy.version != Some(1) {
        return Err("version must be 1".into());
    }
    if policy
        .packages
        .allowed_tiers
        .as_ref()
        .is_some_and(|a| a.iter().any(|s| !matches!(s.as_str(), "t1" | "t2" | "t3")))
        || policy
            .packages
            .allowed_channels
            .as_ref()
            .is_some_and(|a| a.iter().any(|s| !matches!(s.as_str(), "stable" | "beta")))
    {
        return Err("invalid tier or channel".into());
    }
    if policy
        .grants
        .max_risk
        .as_ref()
        .is_some_and(|s| !matches!(s.as_str(), "normal" | "high"))
    {
        return Err("grants.max_risk must be normal or high".into());
    }
    if policy.grants.deny.iter().any(|p| !valid_grant_pattern(p)) {
        return Err("invalid grants.deny pattern".into());
    }
    if policy
        .grants
        .preapproved
        .values()
        .flatten()
        .any(|g| Grant::parse(g).is_err())
    {
        return Err("invalid preapproved grant".into());
    }
    if policy
        .network
        .allow_hosts
        .iter()
        .flatten()
        .chain(&policy.network.deny_hosts)
        .any(|p| Grant::parse(&format!("http:{p}")).is_err() || p.starts_with("setting:"))
    {
        return Err("invalid network host pattern".into());
    }
    if policy
        .audit
        .sink
        .as_ref()
        .is_some_and(|s| !matches!(s.as_str(), "file" | "syslog" | "journald"))
    {
        return Err("invalid audit sink".into());
    }
    Ok(policy)
}
fn valid_grant_pattern(pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix(".*") {
        return !prefix.contains('*')
            && !prefix.contains(':')
            && grants::table()
                .iter()
                .any(|g| g.family.starts_with(&format!("{prefix}.")));
    }
    if let Some((name, arg)) = pattern.split_once(':') {
        return grants::info(name).is_some()
            && (arg == "*"
                || !arg.contains('*') && Grant::parse(pattern).is_ok()
                || name == "http"
                    && arg.starts_with("*.")
                    && !arg[2..].contains('*')
                    && Grant::parse(pattern).is_ok());
    }
    !pattern.contains('*') && grants::info(pattern).is_some()
}
fn grant_matches(pattern: &str, grant: &Grant) -> bool {
    if let Some(prefix) = pattern.strip_suffix(".*") {
        return grant.family().starts_with(&format!("{prefix}."));
    }
    match pattern.split_once(':') {
        None => grant.family() == pattern,
        Some((family, arg)) if family == grant.family() => {
            arg == "*"
                || grant.argument() == Some(arg)
                || family == "http"
                    && arg.starts_with("*.")
                    && grant.argument().is_some_and(|host| host_matches(arg, host))
        }
        _ => false,
    }
}
fn host_matches(pattern: &str, host: &str) -> bool {
    fn domain_port(value: &str) -> (&str, Option<&str>) {
        if !value.starts_with('[') {
            if let Some((domain, port)) = value.rsplit_once(':') {
                if port.bytes().all(|b| b.is_ascii_digit()) {
                    return (domain, Some(port));
                }
            }
        }
        (value, None)
    }
    let (pattern, expected_port) = domain_port(pattern);
    let (host, actual_port) = domain_port(host);
    if expected_port.is_some() && expected_port != actual_port {
        return false;
    }
    if let Some(domain) = pattern.strip_prefix("*.") {
        let host = host.strip_prefix("*.").unwrap_or(host);
        // Wildcards never authorize address literals or the domain apex.
        !host.contains(':')
            && host.parse::<std::net::IpAddr>().is_err()
            && host.len() > domain.len() + 1
            && host.ends_with(domain)
            && host.as_bytes()[host.len() - domain.len() - 1] == b'.'
    } else {
        pattern == host
    }
}

fn glob(pattern: &str, value: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    if !value.starts_with(first) {
        return false;
    }
    let mut rest = &value[first.len()..];
    let parts: Vec<_> = parts.collect();
    if parts.is_empty() {
        return rest.is_empty();
    }
    for (index, part) in parts.iter().enumerate() {
        if index + 1 == parts.len() {
            return rest.ends_with(part);
        }
        let Some(at) = rest.find(part) else {
            return false;
        };
        rest = &rest[at + part.len()..];
    }
    true
}
fn source_contains(prefix: &str, source: &str) -> bool {
    // This is an already-canonical source identity, not path canonicalization.
    // Dot segments, encoded separators and ambiguous origins must be admitted upstream.
    let prefix = prefix.strip_prefix("file://").unwrap_or(prefix);
    let source = source.strip_prefix("file://").unwrap_or(source);
    let safe = |value: &str| {
        !value.contains(['\\', '%', '?', '#'])
            && !value.chars().any(char::is_control)
            && !value.split('/').any(|part| matches!(part, "." | ".."))
    };
    if !safe(prefix) || !safe(source) {
        return false;
    }
    let prefix = prefix.trim_end_matches('/');
    source == prefix
        || source
            .strip_prefix(prefix)
            .is_some_and(|tail| tail.starts_with('/'))
}
