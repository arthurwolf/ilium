//! Capability grants and the grant table (reference-capabilities.md).

use serde::{Deserialize, Serialize};

use crate::error::UnknownName;
use crate::manifest::Diagnostic;
use crate::side::Side;
use crate::tier::Tier;

/// One requested capability, for example `tree.read` or `http:api.example.com`
/// (reference-capabilities.md).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Grant(String);

/// Risk class of a grant: normal grants are low, `H` grants are high (reference-capabilities.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Risk {
    /// Low risk (reference-capabilities.md).
    Low,
    /// Medium risk (reference-capabilities.md).
    Medium,
    /// High risk, asked for once at first activation for T1 (reference-capabilities.md).
    High,
}

impl Risk {
    /// Every risk class, lowest first (reference-capabilities.md).
    pub const ALL: [Risk; 3] = [Risk::Low, Risk::Medium, Risk::High];

    /// `low`, `medium` or `high` (reference-capabilities.md).
    pub fn as_str(self) -> &'static str {
        match self {
            Risk::Low => "low",
            Risk::Medium => "medium",
            Risk::High => "high",
        }
    }
}

impl std::fmt::Display for Risk {
    /// `low`, `medium` or `high` (reference-capabilities.md).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for Risk {
    type Err = UnknownName;

    /// Parses `low`, `medium` or `high` (reference-capabilities.md).
    fn from_str(text: &str) -> Result<Risk, UnknownName> {
        Risk::ALL
            .into_iter()
            .find(|risk| risk.as_str() == text)
            .ok_or_else(|| UnknownName::new("risk", text))
    }
}

/// One row of the grant table (reference-capabilities.md "Capability table").
#[derive(Debug, Clone)]
pub struct GrantInfo {
    /// The grant family, for example `http` or `tree.read`.
    pub family: &'static str,
    /// Whether the grant takes an argument after `:`.
    pub takes_argument: bool,
    /// Risk class.
    pub risk: Risk,
    /// Tiers that may request it.
    pub tiers: &'static [Tier],
    /// Sides that may request it.
    pub sides: &'static [Side],
    /// The unstable feature that gates it, if any (ABI-022).
    pub unstable: Option<&'static str>,
    /// The WIT interfaces the grant unlocks (ABI-023).
    pub world_interfaces: &'static [&'static str],
}

impl Grant {
    /// Parses a grant string; an unknown family is `manifest.grant-unknown` (ABI-023).
    pub fn parse(_text: &str) -> Result<Grant, Diagnostic> {
        unimplemented!("api::grants::Grant::parse")
    }

    /// The grant text as written in the manifest (reference-capabilities.md).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The family part before `:` (reference-capabilities.md).
    pub fn family(&self) -> &str {
        unimplemented!("api::grants::Grant::family")
    }

    /// The argument after `:`, if any (reference-capabilities.md).
    pub fn argument(&self) -> Option<&str> {
        unimplemented!("api::grants::Grant::argument")
    }

    /// Whether this grant allows what `requested` asks for, for example `*.example.com`
    /// covering `api.example.com` (reference-capabilities.md).
    pub fn covers(&self, _requested: &Grant) -> bool {
        unimplemented!("api::grants::Grant::covers")
    }
}

/// The grant table of this API version (reference-capabilities.md).
pub fn table() -> &'static [GrantInfo] {
    unimplemented!("api::grants::table")
}

/// The table row of a grant family (reference-capabilities.md).
pub fn info(_family: &str) -> Option<&'static GrantInfo> {
    unimplemented!("api::grants::info")
}

/// Returns the diagnostic code when the grant is not allowed for this tier and side
/// (reference-manifest-schema.md `manifest.grant-tier`, `manifest.grant-side`).
pub fn check_grant(_grant: &Grant, _tier: Tier, _side: Side) -> Result<(), Diagnostic> {
    unimplemented!("api::grants::check_grant")
}

/// The highest risk of a grant list (reference-capabilities.md).
pub fn risk_of(_grants: &[Grant]) -> Risk {
    unimplemented!("api::grants::risk_of")
}

/// Whether a grant name is high risk (reference-capabilities.md, api-panes.md PAN-032).
pub fn is_high_risk(_grant: &str) -> bool {
    unimplemented!("api::grants::is_high_risk")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes::manifest;
    use crate::side::Side;
    use crate::tier::Tier;
    use std::collections::HashSet;

    /// Builds a grant value directly, so tests of other functions do not depend on `parse`.
    fn grant(text: &str) -> Grant {
        Grant(text.to_owned())
    }

    /// Parses a grant the manual calls valid; a refusal fails the test and shows its diagnostic.
    fn parsed(text: &str) -> Grant {
        match Grant::parse(text) {
            Ok(parsed_grant) => parsed_grant,
            Err(diagnostic) => panic!("`{text}` should parse, but it was refused: {diagnostic}"),
        }
    }

    /// The code `parse` refuses `text` with, or `None` when it accepts the text.
    fn parse_refusal_code(text: &str) -> Option<String> {
        Grant::parse(text)
            .err()
            .map(|diagnostic| diagnostic.code.as_str().to_owned())
    }

    /// The code `check_grant` refuses the grant with, or `None` when it allows it.
    fn check_refusal_code(text: &str, tier: Tier, side: Side) -> Option<String> {
        check_grant(&grant(text), tier, side)
            .err()
            .map(|diagnostic| diagnostic.code.as_str().to_owned())
    }

    /// REQ: CAP-001
    #[test]
    fn parse_splits_at_first_colon_and_keeps_later_colons_in_argument() {
        let parsed_grant = parsed("services.use:ilium:inference/provider");
        assert_eq!(parsed_grant.family(), "services.use");
        assert_eq!(parsed_grant.argument(), Some("ilium:inference/provider"));
        assert_eq!(parsed_grant.as_str(), "services.use:ilium:inference/provider");
    }

    /// REQ: CAP-001
    #[test]
    fn parse_name_without_colon_has_family_and_no_argument() {
        let parsed_grant = parsed("tree.read");
        assert_eq!(parsed_grant.family(), "tree.read");
        assert_eq!(parsed_grant.argument(), None);
    }

    /// REQ: CAP-001
    #[test]
    fn parse_fs_path_template_keeps_braces_and_subtree_marker_in_argument() {
        let parsed_grant = parsed("fs.read:path:{home}/Pictures/**");
        assert_eq!(parsed_grant.family(), "fs.read");
        assert_eq!(parsed_grant.argument(), Some("path:{home}/Pictures/**"));
    }

    /// REQ: CAP-002
    #[test]
    fn parse_unknown_family_is_refused_with_grant_unknown() {
        assert_eq!(parse_refusal_code("files.read"), Some(manifest::GRANT_UNKNOWN.to_owned()));
    }

    /// REQ: CAP-002
    #[test]
    fn parse_reserved_grant_is_refused_with_grant_unknown() {
        assert_eq!(parse_refusal_code("editor.provide"), Some(manifest::GRANT_UNKNOWN.to_owned()));
    }

    /// REQ: CAP-002
    #[test]
    fn parse_retired_alias_is_refused_with_grant_unknown() {
        assert_eq!(
            parse_refusal_code("extensions.contribute-any"),
            Some(manifest::GRANT_UNKNOWN.to_owned())
        );
    }

    /// REQ: CAP-002
    #[test]
    fn parse_empty_text_is_refused_with_grant_unknown() {
        assert_eq!(parse_refusal_code(""), Some(manifest::GRANT_UNKNOWN.to_owned()));
    }

    /// REQ: CAP-004
    #[test]
    fn parse_argument_given_to_no_argument_grant_is_refused_with_grant_argument() {
        assert_eq!(parse_refusal_code("tree.read:all"), Some(manifest::GRANT_ARGUMENT.to_owned()));
    }

    /// REQ: CAP-004
    #[test]
    fn parse_clipboard_read_with_argument_is_refused_with_grant_argument() {
        assert_eq!(
            parse_refusal_code("clipboard.read:text"),
            Some(manifest::GRANT_ARGUMENT.to_owned())
        );
    }

    /// REQ: CAP-004
    #[test]
    fn parse_argument_grant_without_argument_is_refused_with_grant_argument_missing() {
        assert_eq!(
            parse_refusal_code("process.run"),
            Some(manifest::GRANT_ARGUMENT_MISSING.to_owned())
        );
    }

    /// REQ: CAP-004
    #[test]
    fn parse_kernel_settings_read_without_tab_id_is_refused_with_grant_argument_missing() {
        assert_eq!(
            parse_refusal_code("settings.read-kernel"),
            Some(manifest::GRANT_ARGUMENT_MISSING.to_owned())
        );
    }

    /// REQ: CAP-009
    #[test]
    fn parse_process_run_program_name_and_absolute_path_are_accepted() {
        assert!(Grant::parse("process.run:git").is_ok());
        assert!(Grant::parse("process.run:/usr/bin/git").is_ok());
    }

    /// REQ: CAP-009
    #[test]
    fn parse_process_run_with_arguments_is_refused_with_grant_program() {
        assert_eq!(
            parse_refusal_code("process.run:git status"),
            Some(manifest::GRANT_PROGRAM.to_owned())
        );
    }

    /// REQ: CAP-009
    #[test]
    fn parse_process_run_relative_path_with_separator_is_refused_with_grant_program() {
        assert_eq!(
            parse_refusal_code("process.run:./bin/tool"),
            Some(manifest::GRANT_PROGRAM.to_owned())
        );
        assert_eq!(
            parse_refusal_code("process.run:../git"),
            Some(manifest::GRANT_PROGRAM.to_owned())
        );
    }

    /// REQ: CAP-009
    #[test]
    fn parse_process_run_wildcard_or_empty_program_is_refused_with_grant_program() {
        assert_eq!(parse_refusal_code("process.run:*"), Some(manifest::GRANT_PROGRAM.to_owned()));
        assert_eq!(parse_refusal_code("process.run:"), Some(manifest::GRANT_PROGRAM.to_owned()));
    }

    /// REQ: CAP-003
    #[test]
    fn parse_command_plugin_wildcard_in_local_position_is_accepted() {
        assert!(Grant::parse("commands.invoke:com.example.notes/*").is_ok());
    }

    /// REQ: CAP-003
    #[test]
    fn parse_bare_star_for_commands_and_topics_is_accepted() {
        assert!(Grant::parse("commands.invoke:*").is_ok());
        assert!(Grant::parse("topics.subscribe:*").is_ok());
    }

    /// REQ: CAP-003
    #[test]
    fn parse_wildcard_in_plugin_id_or_inside_word_is_refused_with_grant_wildcard() {
        assert_eq!(
            parse_refusal_code("commands.invoke:*/notes"),
            Some(manifest::GRANT_WILDCARD.to_owned())
        );
        assert_eq!(
            parse_refusal_code("commands.invoke:ilium.git*"),
            Some(manifest::GRANT_WILDCARD.to_owned())
        );
        assert_eq!(
            parse_refusal_code("commands.invoke:ilium.git/prefix*"),
            Some(manifest::GRANT_WILDCARD.to_owned())
        );
    }

    /// REQ: CAP-003
    #[test]
    fn parse_wildcard_in_topic_messages_peer_or_http_host_is_refused_with_grant_wildcard() {
        assert_eq!(
            parse_refusal_code("topics.subscribe:ilium.*"),
            Some(manifest::GRANT_WILDCARD.to_owned())
        );
        assert_eq!(parse_refusal_code("messages.peer:*"), Some(manifest::GRANT_WILDCARD.to_owned()));
        assert_eq!(parse_refusal_code("http:tile.*.org"), Some(manifest::GRANT_WILDCARD.to_owned()));
    }

    /// REQ: CAP-008
    #[test]
    fn parse_read_scope_project_is_accepted_and_write_data_is_accepted() {
        assert!(Grant::parse("fs.read:project").is_ok());
        assert!(Grant::parse("fs.write:data").is_ok());
    }

    /// REQ: CAP-008
    #[test]
    fn parse_write_to_package_scope_is_refused_with_grant_scope() {
        assert_eq!(parse_refusal_code("fs.write:package"), Some(manifest::GRANT_SCOPE.to_owned()));
    }

    /// REQ: CAP-008
    #[test]
    fn parse_write_to_agent_sessions_or_agent_config_is_refused_with_grant_scope() {
        assert_eq!(
            parse_refusal_code("fs.write:agent-sessions:claude"),
            Some(manifest::GRANT_SCOPE.to_owned())
        );
        assert_eq!(
            parse_refusal_code("fs.write:agent-config:claude"),
            Some(manifest::GRANT_SCOPE.to_owned())
        );
    }

    /// REQ: CAP-008
    #[test]
    fn parse_watch_of_agent_config_is_refused_with_grant_scope() {
        assert_eq!(
            parse_refusal_code("fs.watch:agent-config:claude"),
            Some(manifest::GRANT_SCOPE.to_owned())
        );
    }

    /// REQ: CAP-008
    #[test]
    fn parse_absolute_path_without_path_template_is_refused_with_grant_scope() {
        assert_eq!(parse_refusal_code("fs.read:/srv/logs"), Some(manifest::GRANT_SCOPE.to_owned()));
    }

    /// REQ: CAP-886
    #[test]
    fn parse_agent_config_propose_names_one_known_agent() {
        assert!(Grant::parse("agent-config.propose:claude").is_ok());
        assert_eq!(
            parse_refusal_code("agent-config.propose"),
            Some(manifest::GRANT_ARGUMENT_MISSING.to_owned())
        );
        assert_eq!(
            parse_refusal_code("agent-config.propose:*"),
            Some(manifest::GRANT_WILDCARD.to_owned())
        );
        assert_eq!(
            parse_refusal_code("agent-config.propose:cursor"),
            Some(manifest::GRANT_SCOPE.to_owned())
        );
    }

    /// REQ: CAP-003
    #[test]
    fn covers_exact_host_only_matches_that_host() {
        assert!(grant("http:api.example.com").covers(&grant("http:api.example.com")));
        assert!(!grant("http:api.example.com").covers(&grant("http:api.example.org")));
    }

    /// REQ: CAP-003
    #[test]
    fn covers_subdomain_wildcard_matches_any_depth_but_not_bare_domain() {
        let pattern = grant("http:*.openstreetmap.org");
        assert!(pattern.covers(&grant("http:a.openstreetmap.org")));
        assert!(pattern.covers(&grant("http:a.b.openstreetmap.org")));
        assert!(!pattern.covers(&grant("http:openstreetmap.org")));
    }

    /// REQ: CAP-003
    #[test]
    fn covers_command_plugin_wildcard_matches_every_command_of_that_plugin_only() {
        let pattern = grant("commands.invoke:ilium.git/*");
        assert!(pattern.covers(&grant("commands.invoke:ilium.git/status")));
        assert!(!pattern.covers(&grant("commands.invoke:ilium.gitx/status")));
    }

    /// REQ: CAP-003
    #[test]
    fn covers_exact_command_matches_only_that_command() {
        let pattern = grant("commands.invoke:ilium.git/status");
        assert!(pattern.covers(&grant("commands.invoke:ilium.git/status")));
        assert!(!pattern.covers(&grant("commands.invoke:ilium.git/refresh")));
    }

    /// REQ: CAP-003
    #[test]
    fn covers_bare_star_matches_every_command() {
        assert!(grant("commands.invoke:*").covers(&grant("commands.invoke:ilium.git/status")));
    }

    /// REQ: CAP-003
    #[test]
    fn covers_topic_prefix_requires_text_past_the_slash() {
        let pattern = grant("topics.subscribe:ilium.progress/*");
        assert!(pattern.covers(&grant("topics.subscribe:ilium.progress/finished")));
        assert!(!pattern.covers(&grant("topics.subscribe:ilium.progress")));
    }

    /// REQ: CAP-001
    #[test]
    fn covers_is_false_across_different_families() {
        assert!(!grant("tree.read").covers(&grant("fs.read:project")));
    }

    /// REQ: CAP-005
    #[test]
    fn check_grant_normal_grant_is_allowed_for_every_tier_on_a_listed_side() {
        assert_eq!(check_refusal_code("tree.read", Tier::T1, Side::Server), None);
        assert_eq!(check_refusal_code("tree.read", Tier::T2, Side::Server), None);
        assert_eq!(check_refusal_code("tree.read", Tier::T3, Side::Client), None);
    }

    /// REQ: CAP-006
    #[test]
    fn check_grant_on_unlisted_side_is_refused_with_grant_side() {
        assert_eq!(
            check_refusal_code("tree.read", Tier::T1, Side::Scene),
            Some(manifest::GRANT_SIDE.to_owned())
        );
    }

    /// REQ: CAP-006
    #[test]
    fn check_grant_client_only_grant_on_server_is_refused_with_grant_side() {
        assert_eq!(
            check_refusal_code("ui.panels", Tier::T1, Side::Server),
            Some(manifest::GRANT_SIDE.to_owned())
        );
    }

    /// REQ: CAP-006
    #[test]
    fn check_grant_server_only_grant_on_client_is_refused_with_grant_side() {
        assert_eq!(
            check_refusal_code("sessions.cross", Tier::T1, Side::Client),
            Some(manifest::GRANT_SIDE.to_owned())
        );
    }

    /// REQ: CAP-005
    #[test]
    fn check_grant_bundled_only_grant_is_allowed_for_t1_on_its_side() {
        assert_eq!(check_refusal_code("settings.read-all", Tier::T1, Side::Client), None);
    }

    /// REQ: CAP-005
    #[test]
    fn check_grant_bundled_only_grant_is_refused_with_grant_tier_for_t2_and_t3() {
        assert_eq!(
            check_refusal_code("settings.read-all", Tier::T2, Side::Client),
            Some(manifest::GRANT_TIER.to_owned())
        );
        assert_eq!(
            check_refusal_code("plugins.manage", Tier::T3, Side::Client),
            Some(manifest::GRANT_TIER.to_owned())
        );
    }

    /// REQ: CAP-006
    #[test]
    fn check_grant_bundled_only_grant_on_unlisted_side_is_refused_with_grant_side() {
        assert_eq!(
            check_refusal_code("settings.read-all", Tier::T1, Side::Server),
            Some(manifest::GRANT_SIDE.to_owned())
        );
    }

    /// REQ: CAP-010
    #[test]
    fn info_high_risk_family_reports_argument_risk_and_sides() {
        let row = info("process.run").expect("process.run is in the grant table");
        assert!(row.takes_argument);
        assert_eq!(row.risk, Risk::High);
        assert!(row.sides.contains(&Side::Server));
        assert!(row.sides.contains(&Side::Client));
        assert!(!row.sides.contains(&Side::Scene));
    }

    /// REQ: CAP-010
    #[test]
    fn info_normal_family_takes_no_argument_and_is_not_high_risk() {
        let row = info("tree.read").expect("tree.read is in the grant table");
        assert!(!row.takes_argument);
        assert_ne!(row.risk, Risk::High);
    }

    /// REQ: CAP-005
    #[test]
    fn info_bundled_only_family_lists_only_t1_as_tier() {
        let row = info("settings.read-all").expect("settings.read-all is in the grant table");
        assert_eq!(row.tiers, [Tier::T1].as_slice());
    }

    /// REQ: CAP-002
    #[test]
    fn info_unknown_family_is_none() {
        assert!(info("files.read").is_none());
        assert!(info("").is_none());
    }

    /// REQ: ABI-022
    #[test]
    fn info_unstable_grant_names_its_feature_and_runs_in_scene_only() {
        let row = info("gpu.kernels").expect("gpu.kernels is in the grant table");
        assert_eq!(row.unstable, Some("gpu-kernels"));
        assert_eq!(row.sides, [Side::Scene].as_slice());
    }

    /// REQ: CAP-017
    #[test]
    fn table_lists_each_grant_family_once() {
        let families: Vec<&str> = table().iter().map(|row| row.family).collect();
        let unique: HashSet<&str> = families.iter().copied().collect();
        assert_eq!(unique.len(), families.len());
        assert!(unique.contains("tree.read"));
    }

    /// REQ: CAP-010
    #[test]
    fn risk_of_list_with_a_high_grant_is_high() {
        assert_eq!(risk_of(&[grant("tree.read"), grant("process.run:git")]), Risk::High);
        assert_eq!(risk_of(&[grant("commands.invoke:*")]), Risk::High);
    }

    /// REQ: CAP-010
    #[test]
    fn risk_of_list_of_normal_grants_is_below_high() {
        assert!(risk_of(&[grant("tree.read"), grant("fs.read:project")]) < Risk::High);
    }

    /// REQ: CAP-010
    #[test]
    fn risk_of_empty_list_is_below_high() {
        assert!(risk_of(&[]) < Risk::High);
    }

    /// REQ: CAP-010
    #[test]
    fn is_high_risk_for_process_run_and_agent_config_propose() {
        assert!(is_high_risk("process.run:git"));
        assert!(is_high_risk("agent-config.propose:claude"));
        assert!(!is_high_risk("tree.read"));
    }

    /// REQ: CAP-010
    #[test]
    fn is_high_risk_only_for_bare_star_commands_and_topics() {
        assert!(is_high_risk("commands.invoke:*"));
        assert!(is_high_risk("topics.subscribe:*"));
        assert!(!is_high_risk("commands.invoke:ilium.git/*"));
        assert!(!is_high_risk("topics.subscribe:ilium.progress/*"));
    }

    /// REQ: CAP-010
    #[test]
    fn is_high_risk_only_for_loopback_http_with_port() {
        assert!(is_high_risk("http:localhost:8080"));
        assert!(!is_high_risk("http:api.example.com"));
    }

    /// REQ: CAP-010
    #[test]
    fn is_high_risk_for_fs_write_outside_data_and_cache_only() {
        assert!(is_high_risk("fs.write:project"));
        assert!(!is_high_risk("fs.write:data"));
        assert!(!is_high_risk("fs.read:project"));
    }

    /// REQ: CAP-010
    #[test]
    fn is_high_risk_for_clipboard_read_but_not_clipboard_write() {
        assert!(is_high_risk("clipboard.read"));
        assert!(!is_high_risk("clipboard.write"));
    }

    /// REQ: CAP-010
    #[test]
    fn risk_names_round_trip_and_reject_other_text() {
        for risk in Risk::ALL {
            assert_eq!(risk.as_str().parse::<Risk>().ok(), Some(risk));
        }
        assert!("HIGH".parse::<Risk>().is_err());
        assert!("".parse::<Risk>().is_err());
    }
}
