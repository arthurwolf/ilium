use super::*;
use crate::compaction_scan::fixtures::{claude_session, codex_session, Process};
use ilium_compaction_analysis::dedupe::dedupe_across_traces;
use ilium_compaction_analysis::parse::{line_may_matter, TraceBuilder};

fn parse(agent: AgentKind, text: &str, is_subagent: bool) -> SessionTrace {
    let mut builder = TraceBuilder::new(agent).with_subagent(is_subagent);
    for line in text.split('\n') {
        if line_may_matter(agent, line.as_bytes()) {
            builder.feed_line(line.as_bytes());
        }
    }
    builder.finish()
}

fn corpus(agent: AgentKind, sessions: usize) -> Vec<SessionTrace> {
    let claude = Process::claude();
    let codex = Process::codex();
    let mut traces: Vec<SessionTrace> = (0..sessions)
        .map(|index| match agent {
            AgentKind::ClaudeCode => parse(agent, &claude_session(&claude, index, "r"), false),
            AgentKind::Codex => parse(agent, &codex_session(&codex, index, "r"), false),
        })
        .collect();
    dedupe_across_traces(&mut traces);
    traces
}

fn build(
    agent: AgentKind,
    traces: &[SessionTrace],
    settings: &ScanSettingsInput,
    scan: &ScanSummary,
) -> CompactionReport {
    CompactionReport::build(&ReportInput {
        agent,
        traces,
        settings,
        scan,
        generated_at_unix_ms: 1_791_000_000_000,
    })
}

#[test]
fn a_claude_corpus_yields_a_complete_report() {
    let traces = corpus(AgentKind::ClaudeCode, 8);
    let scan = ScanSummary {
        files_listed: 8,
        files_parsed: 8,
        bytes_total: 4_000_000,
        ..ScanSummary::default()
    };
    let report = build(
        AgentKind::ClaudeCode,
        &traces,
        &ScanSettingsInput::default(),
        &scan,
    );
    assert_eq!(report.status, ReportStatus::Complete);
    assert!(report.status_message.is_none());
    assert_eq!(report.corpus.main_sessions, 8);
    assert_eq!(report.corpus.files_listed, 8);
    assert!(report.compactions.total >= 8);
    assert_eq!(report.compactions.auto, report.compactions.total);
    assert!(report.compactions.pre_tokens.is_some());
    assert!(report.fixed_prefix.first_request_tokens.is_some());
    assert!(report.fixed_prefix.rule_of_thumb.contains("10-15k"));
    assert!(report.cost_mix.cache_read_share > 0.0);
    assert!(!report.regimes.is_empty());
    assert!(report.regimes.iter().any(|regime| regime.is_most_recent));

    // Simulation rows: ascending triggers, 217k point present, one best.
    let triggers: Vec<u32> = report
        .simulation
        .iter()
        .map(|row| row.trigger_tokens)
        .collect();
    assert!(triggers.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(triggers.contains(&217_000) && triggers.contains(&120_000));
    assert!(triggers.contains(&967_000), "the CLI default is simulated");
    assert_eq!(
        report.simulation.iter().filter(|row| row.is_best).count(),
        1
    );
    assert!(report
        .simulation
        .iter()
        .any(|row| row.is_cli_default && row.trigger_tokens == 967_000));
    let best = report.simulation.iter().find(|row| row.is_best).unwrap();
    assert!(best.within_2_percent && best.within_5_percent);
    assert!(report
        .simulation
        .iter()
        .all(|row| row.within_2_percent <= row.within_5_percent));

    // Recommendation card.
    let card = report.recommendation.as_ref().expect("a recommendation");
    assert_eq!(card.setting_key, "autoCompactWindow");
    assert_eq!(card.setting_value, card.trigger_tokens + 33_000);
    assert!(card
        .headline_text
        .starts_with("Optimal: autoCompactWindow "));
    assert!(report
        .simulation
        .iter()
        .any(|row| row.is_pick && row.trigger_tokens == card.trigger_tokens));
    // Every observed compaction fired at 150k+, so levels far below are extrapolated.
    assert_eq!(card.extrapolated, card.observed_support_fraction < 0.01);
    if card.extrapolated {
        assert!(card.observed_support_text.starts_with("extrapolated: "));
        assert!(card.headline_text.contains("extrapolated"));
    }
    assert!(!card.confidence_label().is_empty());

    // Per-model table with the "cannot apply per model" message.
    assert!(!report.per_model.is_empty());
    assert!(report
        .per_model_note
        .contains("cannot be applied per model"));
    // Rework sensitivity 0.5x / 1x / 2x and the bootstrap.
    let multipliers: Vec<f64> = report.rework.iter().map(|row| row.multiplier).collect();
    assert_eq!(multipliers, vec![0.5, 1.0, 2.0]);
    assert!(report.rework_note.contains("research corpus"));
    assert!(report.bootstrap.as_ref().unwrap().resamples >= 200);

    // Three-way comparison without a current setting: default | recommended.
    assert_eq!(report.comparison.len(), 2);
    assert_eq!(report.comparison[0].kind, ComparisonKind::CliDefault);
    assert_eq!(report.comparison[0].trigger_tokens, 967_000);
    assert_eq!(report.comparison[1].kind, ComparisonKind::Recommended);
    assert!(card.current_vs_recommended.is_none());

    // Warnings: price caveat, priors, no duplicates.
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("API-price equivalents")));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("rework penalty")));
    let unique: BTreeSet<&String> = report.warnings.iter().collect();
    assert_eq!(unique.len(), report.warnings.len());
    assert_eq!(card.warnings, report.warnings);

    let text = report.to_plain_text();
    for needle in [
        "== Recommendation ==",
        "== Corpus ==",
        "== Compactions ==",
        "== Cost mix ==",
        "== Simulation ==",
        "== Warnings ==",
        "cannot be applied per model",
    ] {
        assert!(text.contains(needle), "missing {needle} in\n{text}");
    }
}

#[test]
fn the_current_setting_becomes_a_third_comparison_row() {
    let traces = corpus(AgentKind::ClaudeCode, 8);
    let settings = ScanSettingsInput {
        current_setting_value: Some(283_000),
        ..ScanSettingsInput::default()
    };
    let report = build(
        AgentKind::ClaudeCode,
        &traces,
        &settings,
        &ScanSummary::default(),
    );
    assert_eq!(report.comparison.len(), 3);
    let kinds: Vec<ComparisonKind> = report.comparison.iter().map(|row| row.kind).collect();
    assert_eq!(
        kinds,
        vec![
            ComparisonKind::CliDefault,
            ComparisonKind::CurrentSetting,
            ComparisonKind::Recommended
        ]
    );
    let current = &report.comparison[1];
    assert_eq!(current.trigger_tokens, 250_000);
    assert_eq!(current.setting_value, Some(283_000));
    assert!(report.simulation.iter().any(|row| row.is_current));
    let card = report.recommendation.as_ref().unwrap();
    assert!(card.current_vs_recommended.is_some());
    // The recommended point is the reference: its own relative cost is zero.
    assert!(report.comparison[2].relative_to_recommended.abs() < 1e-9);
}

#[test]
fn a_codex_corpus_uses_the_codex_semantics() {
    let traces = corpus(AgentKind::Codex, 8);
    let report = build(
        AgentKind::Codex,
        &traces,
        &ScanSettingsInput::default(),
        &ScanSummary::default(),
    );
    assert_eq!(report.status, ReportStatus::Complete);
    assert_eq!(
        report.semantics.setting_key,
        "model_auto_compact_token_limit"
    );
    assert_eq!(report.semantics.allowed_setting_range.1, 232_560);
    assert_eq!(report.semantics.cli_default_trigger_tokens, 232_560);
    assert!(report.semantics.mapping_note.contains("clamped to 90%"));
    assert_eq!(
        report.compactions.auto, 0,
        "Codex logs do not state a trigger kind"
    );
    assert_eq!(report.compactions.unknown_trigger, report.compactions.total);
    assert!(report.to_plain_text().contains("of unstated trigger"));
    let card = report.recommendation.as_ref().unwrap();
    assert!(card.setting_value <= 232_560);
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("cached input at 10%")));
    let triggers: Vec<u32> = report
        .simulation
        .iter()
        .map(|row| row.trigger_tokens)
        .collect();
    assert!(triggers.contains(&100_000) && triggers.contains(&232_560));
    assert!(triggers.iter().all(|&trigger| trigger <= 232_560));
}

#[test]
fn an_empty_corpus_is_a_clear_no_sessions_result() {
    for agent in AgentKind::ALL {
        let report = build(
            agent,
            &[],
            &ScanSettingsInput::default(),
            &ScanSummary::default(),
        );
        assert_eq!(report.status, ReportStatus::NoSessionsFound);
        assert!(report.recommendation.is_none());
        assert!(report.simulation.is_empty());
        assert!(report.comparison.is_empty());
        assert!(report
            .status_message
            .as_deref()
            .unwrap()
            .starts_with("No sessions found"));
        let text = report.to_plain_text();
        assert!(text.contains("No sessions found"));
        assert!(!report.warnings.is_empty(), "the price caveat still shows");
    }
}

#[test]
fn small_sessions_report_statistics_but_nothing_to_replay() {
    let mut process = Process::claude();
    process.turns = 20;
    process.trigger = 10_000_000;
    let traces: Vec<SessionTrace> = (0..3)
        .map(|index| {
            parse(
                AgentKind::ClaudeCode,
                &claude_session(&process, index, "small"),
                false,
            )
        })
        .collect();
    let report = build(
        AgentKind::ClaudeCode,
        &traces,
        &ScanSettingsInput::default(),
        &ScanSummary::default(),
    );
    assert_eq!(report.status, ReportStatus::NothingToReplay);
    assert_eq!(report.corpus.main_sessions, 3);
    assert!(report
        .status_message
        .as_deref()
        .unwrap()
        .contains("No session grew beyond 120k"));
    assert!(report.recommendation.is_none());
}

#[test]
fn scan_warnings_truncation_and_subagents_reach_the_report() {
    let mut traces = corpus(AgentKind::ClaudeCode, 6);
    traces.push(parse(
        AgentKind::ClaudeCode,
        &claude_session(&Process::claude(), 50, "sub"),
        true,
    ));
    let scan = ScanSummary {
        sessions_dropped_for_cap: 12,
        warnings: vec!["Skipped /synthetic/a.jsonl: gone".to_owned()],
        ..ScanSummary::default()
    };
    let report = build(
        AgentKind::ClaudeCode,
        &traces,
        &ScanSettingsInput::default(),
        &scan,
    );
    assert_eq!(report.corpus.subagent_sessions, 1);
    assert!(report.corpus.subagent_request_share > 0.0);
    assert_eq!(report.corpus.sessions_dropped_for_cap, 12);
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("larger than the retained-result cap")));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("a.jsonl")));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("subagent sessions are reported separately")));
}

#[test]
fn unpriced_models_are_named_in_a_warning() {
    let mut traces = corpus(AgentKind::Codex, 6);
    for trace in &mut traces {
        for model in &mut trace.models {
            *model = "gpt-mystery-model".to_owned();
        }
    }
    let report = build(
        AgentKind::Codex,
        &traces,
        &ScanSettingsInput::default(),
        &ScanSummary::default(),
    );
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("No price for gpt-mystery-model")));
    assert!(report.cost_mix.total_usd.is_none());
    assert!(report.simulation.iter().all(|row| row.cost_usd.is_none()));
}

#[test]
fn priced_models_produce_dollar_figures() {
    let traces = corpus(AgentKind::ClaudeCode, 6);
    let report = build(
        AgentKind::ClaudeCode,
        &traces,
        &ScanSettingsInput::default(),
        &ScanSummary::default(),
    );
    // claude-sonnet-5-5 is in the built-in price table.
    assert!(report.cost_mix.total_usd.is_some());
    assert!(report.simulation.iter().all(|row| row.cost_usd.is_some()));
    assert!(!report
        .warnings
        .iter()
        .any(|warning| warning.contains("No price for")));
}

#[test]
fn price_weights_map_the_table_rates() {
    let table = PriceTable::default();
    let weights = price_weights(&table, "claude-sonnet-5-5").unwrap();
    assert_eq!(weights.input, 2.0);
    assert_eq!(weights.output, 10.0);
    assert_eq!(weights.cache_read, 0.20);
    assert_eq!(weights.cache_write_5m, 2.5);
    assert_eq!(
        weights.cache_write_1h, 4.0,
        "the 1-hour tier is 2x the input price"
    );
    assert!(price_weights(&table, "unknown-model").is_none());
}

#[test]
fn grids_cover_the_documented_ranges() {
    let claude = default_grid(
        AgentKind::ClaudeCode,
        &AgentSemantics::default_for(AgentKind::ClaudeCode),
    );
    assert_eq!(claude.first(), Some(&120_000));
    assert_eq!(claude.last(), Some(&600_000));
    assert!(claude.contains(&567_000) && claude.contains(&217_000) && claude.contains(&125_000));
    let codex = default_grid(
        AgentKind::Codex,
        &AgentSemantics::default_for(AgentKind::Codex),
    );
    assert_eq!(codex.first(), Some(&100_000));
    assert_eq!(codex.last(), Some(&232_560), "capped at 90% of the window");
    assert!(codex.contains(&225_000));
}

#[test]
fn stated_codex_windows_set_the_cap() {
    let mut traces = corpus(AgentKind::Codex, 3);
    for trace in &mut traces {
        trace.context_window_tokens = Some(272_000);
    }
    let stats = CorpusStats::compute(AgentKind::Codex, &traces, &StatsConfig::default(), None);
    let semantics = semantics_for(AgentKind::Codex, &traces, &stats, None);
    assert_eq!(semantics.allowed_setting_range().1, 244_800);
}

#[test]
fn formatting_helpers_are_compact_and_stable() {
    assert_eq!(humanize_tokens(0.0), "0");
    assert_eq!(humanize_tokens(950.0), "950");
    assert_eq!(humanize_tokens(217_000.0), "217k");
    assert_eq!(humanize_tokens(217_500.0), "217.5k");
    assert_eq!(humanize_tokens(999_700.0), "1M");
    assert_eq!(humanize_tokens(1_250_000.0), "1.25M");
    assert_eq!(humanize_tokens(f64::NAN), "0");
    assert_eq!(group_thousands(0), "0");
    assert_eq!(group_thousands(250_000), "250,000");
    assert_eq!(group_thousands(1_234_567), "1,234,567");
    assert_eq!(format_usd(0.4), "$0.40");
    assert_eq!(format_usd(1_234.6), "$1,235");
    assert_eq!(format_percent(0.123), "12.3%");
    assert_eq!(format_percent(0.0), "0.0%");
    assert_eq!(format_percent(0.00001), "<0.1%");
    assert_eq!(format_signed_percent(0.032), "+3.2%");
    assert_eq!(format_signed_percent(-0.004), "-0.4%");
    assert_eq!(format_signed_percent(0.0), "0.0%");
    assert_eq!(format_bytes(812), "812 B");
    assert_eq!(format_bytes(3_400_000), "3.4 MB");
    assert_eq!(format_elapsed(5.0), "5s");
    assert_eq!(format_elapsed(185.0), "3m 05s");
    assert_eq!(format_elapsed(3_725.0), "1h 02m");
    let quantiles = Quantiles {
        n: 12,
        p10: 120_000.0,
        p50: 217_000.0,
        p90: 567_000.0,
        mean: 300_000.0,
    };
    assert_eq!(
        format_token_quantiles(&quantiles),
        "p10 120k | p50 217k | p90 567k (n=12)"
    );
}

fn measured_note(report: &CompactionReport) -> bool {
    report
        .semantics
        .mapping_note
        .contains("(measured on these logs)")
}

#[test]
fn a_plausible_current_setting_re_measures_the_claude_offset() {
    let traces = corpus(AgentKind::ClaudeCode, 8);
    // Compactions fire at 150k..153k; a 183,000 window means an offset of about 31k.
    let report = build(
        AgentKind::ClaudeCode,
        &traces,
        &ScanSettingsInput {
            current_setting_value: Some(183_000),
            ..ScanSettingsInput::default()
        },
        &ScanSummary::default(),
    );
    assert!(measured_note(&report), "{}", report.semantics.mapping_note);
    let card = report.recommendation.as_ref().unwrap();
    let offset = card.setting_value - card.trigger_tokens;
    assert!(
        (29_000..=33_000).contains(&offset),
        "measured offset {offset}"
    );
    assert!(!report
        .warnings
        .iter()
        .any(|warning| warning.contains("uses the researched default")));
}

#[test]
fn an_implausible_current_setting_keeps_the_default_mapping() {
    let traces = corpus(AgentKind::ClaudeCode, 8);
    // A 900,000 window cannot be what produced compactions at 150k: it is
    // another regime's setting, so the offset is not re-measured.
    let report = build(
        AgentKind::ClaudeCode,
        &traces,
        &ScanSettingsInput {
            current_setting_value: Some(900_000),
            ..ScanSettingsInput::default()
        },
        &ScanSummary::default(),
    );
    assert!(!measured_note(&report));
    let card = report.recommendation.as_ref().unwrap();
    assert_eq!(card.setting_value, card.trigger_tokens + 33_000);
}

#[test]
fn a_plausible_codex_limit_re_measures_the_realized_ratio() {
    let traces = corpus(AgentKind::Codex, 8);
    let report = build(
        AgentKind::Codex,
        &traces,
        &ScanSettingsInput {
            current_setting_value: Some(150_000),
            ..ScanSettingsInput::default()
        },
        &ScanSummary::default(),
    );
    assert!(measured_note(&report), "{}", report.semantics.mapping_note);
}

fn corpus_of(agent: AgentKind, process: Process, sessions: usize) -> Vec<SessionTrace> {
    (0..sessions)
        .map(|index| match agent {
            AgentKind::ClaudeCode => parse(agent, &claude_session(&process, index, "p"), false),
            AgentKind::Codex => parse(agent, &codex_session(&process, index, "p"), false),
        })
        .collect()
}

fn report_of(agent: AgentKind, traces: &[SessionTrace]) -> CompactionReport {
    build(
        agent,
        traces,
        &ScanSettingsInput::default(),
        &ScanSummary::default(),
    )
}

#[test]
fn plain_argmin_is_extrapolated_when_no_candidate_has_support() {
    // Codex compactions fire at 240k+, above every candidate (cap 232,560).
    let mut process = Process::codex();
    process.trigger = 240_000;
    process.turns = 200;
    let report = report_of(AgentKind::Codex, &corpus_of(AgentKind::Codex, process, 8));
    let card = report.recommendation.as_ref().unwrap();
    assert_eq!(card.pick_rule, PickRule::PlainArgmin);
    assert!(card.extrapolated);
    assert_eq!(card.pick_rule_text, "plain optimum, extrapolated");
    assert!(card.headline_text.contains("extrapolated"));
    assert!(card.observed_support_text.starts_with("extrapolated: "));
    assert!(card.unsupported_argmin_tokens.is_none());
    assert!(card.unsupported_relative_saving.is_none());
    assert!(report
        .simulation
        .iter()
        .all(|row| row.observed_support == 0.0));
    assert!(report
        .to_plain_text()
        .contains("Pick rule: plain optimum, extrapolated"));
}

#[test]
fn compactions_inside_the_two_percent_band_give_the_band_rule() {
    let mut process = Process::codex();
    process.trigger = 150_000;
    let report = report_of(AgentKind::Codex, &corpus_of(AgentKind::Codex, process, 8));
    let card = report.recommendation.as_ref().unwrap();
    assert_eq!(card.pick_rule, PickRule::BandWithSupport);
    assert!(!card.extrapolated);
    assert_eq!(
        card.pick_rule_text,
        "lowest candidate within 2% of best with observed support"
    );
    assert!(card.unsupported_argmin_tokens.is_none());
    assert!(card.observed_support_fraction >= 0.01);
}

#[test]
fn a_supported_optimum_reports_the_cheaper_unsupported_argmin() {
    // Codex-like: every compaction at about 195-200k, cost keeps falling below.
    let mut process = Process::codex();
    process.trigger = 195_000;
    process.turns = 200;
    let report = report_of(AgentKind::Codex, &corpus_of(AgentKind::Codex, process, 8));
    let card = report.recommendation.as_ref().unwrap();
    assert_eq!(card.pick_rule, PickRule::SupportedOptimum);
    assert!(!card.extrapolated, "the pick has observed support");
    assert!(card.observed_support_fraction >= 0.01);
    assert!(card.trigger_tokens >= 195_000);
    let cheaper = card.unsupported_argmin_tokens.expect("a cheaper trigger");
    assert!(cheaper < card.trigger_tokens);
    let saving = card.unsupported_relative_saving.expect("its saving");
    assert!(saving > 0.0 && saving < 1.0);
    assert!(card
        .pick_rule_text
        .starts_with("cheapest candidate with observed support; cost keeps falling below "));
    assert!(card.pick_rule_text.contains("cheaper in simulation"));
    assert!(card
        .pick_rule_text
        .ends_with("but no compactions observed there"));
    assert_ne!(card.confidence, Confidence::High);
    let text = report.to_plain_text();
    assert!(text.contains("Pick rule: cheapest candidate with observed support"));
    assert!(text.contains("Cheaper but unsupported: "));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("no observed compactions there")
            || warning.contains("but no observed compactions")));
}

#[test]
fn a_claude_corpus_compacting_at_567k_has_support_only_far_above_the_cheap_region() {
    // Every compaction at 567k+: the 575k/600k/967k candidates have support,
    // the cheaper levels (about 175k) do not, so the supported optimum is far
    // above the unsupported argmin and the saving is large.
    let mut process = Process::claude();
    process.turns = 400;
    process.trigger = 567_000;
    let report = report_of(
        AgentKind::ClaudeCode,
        &corpus_of(AgentKind::ClaudeCode, process, 6),
    );
    let card = report.recommendation.as_ref().unwrap();
    assert_eq!(card.pick_rule, PickRule::SupportedOptimum);
    assert!(card.trigger_tokens >= 567_000);
    assert!(card.unsupported_argmin_tokens.unwrap() < 300_000);
    assert!(card.unsupported_relative_saving.unwrap() > 0.2);
    assert_eq!(card.setting_key, "autoCompactWindow");
}

#[test]
fn support_counts_main_sessions_only() {
    // Main sessions compact at 240k+ (no candidate supported). Subagents
    // compacting at 150k must not create support.
    let mut main = Process::codex();
    main.trigger = 240_000;
    main.turns = 200;
    let mut traces = corpus_of(AgentKind::Codex, main, 8);
    let mut sub = Process::codex();
    sub.trigger = 150_000;
    for index in 0..4 {
        traces.push(parse(
            AgentKind::Codex,
            &codex_session(&sub, 40 + index, "sub"),
            true,
        ));
    }
    assert!(traces.iter().skip(8).all(|trace| trace.is_subagent));
    assert!(traces
        .iter()
        .skip(8)
        .all(|trace| !trace.compactions.is_empty()));
    let report = report_of(AgentKind::Codex, &traces);
    let card = report.recommendation.as_ref().unwrap();
    assert_eq!(card.pick_rule, PickRule::PlainArgmin);
    assert!(report
        .simulation
        .iter()
        .all(|row| row.observed_support == 0.0));
}

#[test]
fn an_optimum_at_the_grid_floor_widens_the_grid_once() {
    // Cheap rebuilds make ever lower triggers cheaper: the first optimum sits
    // at the 100k floor of the Codex grid.
    let mut process = Process::codex();
    process.trigger = 150_000;
    process.post_fresh = 2_000;
    process.post_cache_read = 15_000;
    process.growth = 3_000;
    let report = report_of(AgentKind::Codex, &corpus_of(AgentKind::Codex, process, 8));
    let card = report.recommendation.as_ref().unwrap();
    assert!(card.grid_widened);
    assert!(
        !card.grid_floor_limited,
        "the widened grid contains the optimum"
    );
    let triggers: Vec<u32> = report
        .simulation
        .iter()
        .map(|row| row.trigger_tokens)
        .collect();
    assert_eq!(triggers.first(), Some(&60_000), "halved toward 60k");
    assert!(triggers.windows(2).all(|pair| pair[0] < pair[1]));
    let best = report.simulation.iter().find(|row| row.is_best).unwrap();
    assert!(best.trigger_tokens > 60_000 && best.trigger_tokens < 100_000);
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("grid was widened to 60k-")));
    assert!(report
        .to_plain_text()
        .contains("The simulated grid was widened automatically."));
}

#[test]
fn claude_widening_stops_at_the_lowest_setting_the_agent_accepts() {
    let mut process = Process::claude();
    process.trigger = 150_000;
    process.post_fresh = 2_000;
    process.post_cache_read = 15_000;
    let report = report_of(
        AgentKind::ClaudeCode,
        &corpus_of(AgentKind::ClaudeCode, process, 8),
    );
    let card = report.recommendation.as_ref().unwrap();
    assert!(card.grid_widened);
    // autoCompactWindow accepts 100k at the lowest: a 67k trigger.
    assert_eq!(
        report.simulation.first().map(|row| row.trigger_tokens),
        Some(67_000)
    );
    assert!(report
        .simulation
        .iter()
        .all(|row| row.setting_value >= 100_000));
    assert!(card.setting_value >= 100_000);
}

#[test]
fn a_normal_optimum_does_not_widen_the_grid() {
    let report = report_of(
        AgentKind::Codex,
        &corpus_of(AgentKind::Codex, Process::codex(), 8),
    );
    let card = report.recommendation.as_ref().unwrap();
    assert!(!card.grid_widened && !card.grid_floor_limited);
    assert_eq!(
        report.simulation.first().map(|row| row.trigger_tokens),
        Some(100_000)
    );
    assert!(!report
        .warnings
        .iter()
        .any(|warning| warning.contains("widened")));
}

#[test]
fn widening_triggers_cover_floor_ceiling_and_limits() {
    let claude = AgentSemantics::default_for(AgentKind::ClaudeCode);
    let codex = AgentSemantics::default_for(AgentKind::Codex);
    let claude_grid = default_grid(AgentKind::ClaudeCode, &claude);
    // Floor: from the 67k trigger (100k setting minus the offset) up.
    let floor = widening_triggers(AgentKind::ClaudeCode, &claude, &claude_grid, 120_000);
    assert_eq!(floor, vec![67_000, 80_000, 93_000, 106_000]);
    // Ceiling: from the last point toward the 967k trigger (1M setting).
    let ceiling = widening_triggers(AgentKind::ClaudeCode, &claude, &claude_grid, 600_000);
    assert_eq!(ceiling.last(), Some(&967_000));
    assert!(ceiling.iter().all(|&point| point > 600_000));
    // Already at the limit: nothing to add.
    let wide = vec![67_000, 120_000, 967_000];
    assert!(widening_triggers(AgentKind::ClaudeCode, &claude, &wide, 967_000).is_empty());
    assert!(widening_triggers(AgentKind::ClaudeCode, &claude, &wide, 67_000).is_empty());
    // An argmin in the middle of the grid never widens.
    assert!(widening_triggers(AgentKind::ClaudeCode, &claude, &claude_grid, 250_000).is_empty());
    // Codex: the floor ladder is 60k..90k, the ceiling is the 90% window cap.
    let codex_grid = default_grid(AgentKind::Codex, &codex);
    assert_eq!(
        widening_triggers(AgentKind::Codex, &codex, &codex_grid, 100_000),
        vec![60_000, 70_000, 80_000, 90_000]
    );
    assert!(widening_triggers(AgentKind::Codex, &codex, &codex_grid, 232_560).is_empty());
    assert!(widening_triggers(AgentKind::Codex, &codex, &[], 0).is_empty());
}

#[test]
fn the_claude_567k_corpus_offers_an_extrapolated_alternative() {
    let mut process = Process::claude();
    process.turns = 400;
    process.trigger = 567_000;
    let report = report_of(
        AgentKind::ClaudeCode,
        &corpus_of(AgentKind::ClaudeCode, process, 6),
    );
    let card = report.recommendation.as_ref().unwrap();
    assert_eq!(card.pick_rule, PickRule::SupportedOptimum);
    assert_eq!(card.trigger_tokens, 575_000);
    let alternative = card
        .extrapolated_alternative
        .as_ref()
        .expect("an alternative");
    assert!((175_000..=225_000).contains(&alternative.trigger_tokens));
    assert!(alternative.relative_saving > 0.30);
    assert_eq!(
        Some(alternative.trigger_tokens),
        card.unsupported_argmin_tokens
    );
    assert_eq!(
        Some(alternative.relative_saving),
        card.unsupported_relative_saving
    );
    assert_eq!(alternative.setting_key, "autoCompactWindow");
    assert_eq!(
        alternative.setting_value,
        u64::from(alternative.trigger_tokens) + 33_000
    );
    assert!(!alternative.setting_clamped);
    assert!(alternative
        .observed_support_text
        .starts_with("extrapolated: 0.0% of observed"));
    assert!(alternative
        .headline_text
        .starts_with("Simulated optimum: autoCompactWindow "));
    assert!(alternative
        .headline_text
        .contains("cheaper than the supported pick"));
    assert!(alternative
        .headline_text
        .ends_with("EXTRAPOLATED: no observed compactions at or below this level)"));
    // The supported pick is unchanged and not extrapolated.
    assert!(!card.extrapolated);
    let text = report.to_plain_text();
    assert!(text.contains(&format!("Alternative: {}", alternative.headline_text)));
}

#[test]
fn a_codex_supported_optimum_maps_its_alternative_through_the_semantics() {
    let mut process = Process::codex();
    process.trigger = 195_000;
    process.turns = 200;
    let report = report_of(AgentKind::Codex, &corpus_of(AgentKind::Codex, process, 8));
    let card = report.recommendation.as_ref().unwrap();
    assert_eq!(card.pick_rule, PickRule::SupportedOptimum);
    assert_eq!(card.trigger_tokens, 200_000);
    let alternative = card
        .extrapolated_alternative
        .as_ref()
        .expect("an alternative");
    assert_eq!(
        Some(alternative.trigger_tokens),
        card.unsupported_argmin_tokens
    );
    assert!(alternative.trigger_tokens < card.trigger_tokens);
    assert_eq!(alternative.setting_key, "model_auto_compact_token_limit");
    // Codex maps the trigger one to one (realized ratio 1.0) below the cap.
    assert_eq!(
        alternative.setting_value,
        u64::from(alternative.trigger_tokens)
    );
    assert!(alternative.relative_saving > 0.0);
    assert!(alternative
        .headline_text
        .contains("model_auto_compact_token_limit"));
}

#[test]
fn the_band_pick_without_a_cheaper_unsupported_trigger_has_no_alternative() {
    let mut process = Process::codex();
    process.trigger = 150_000;
    let report = report_of(AgentKind::Codex, &corpus_of(AgentKind::Codex, process, 8));
    let card = report.recommendation.as_ref().unwrap();
    assert_eq!(card.pick_rule, PickRule::BandWithSupport);
    assert!(card.unsupported_argmin_tokens.is_none());
    assert!(card.extrapolated_alternative.is_none());
    assert!(!report.to_plain_text().contains("Alternative:"));
}

#[test]
fn the_plain_argmin_pick_has_no_alternative() {
    let mut process = Process::codex();
    process.trigger = 240_000;
    process.turns = 200;
    let report = report_of(AgentKind::Codex, &corpus_of(AgentKind::Codex, process, 8));
    let card = report.recommendation.as_ref().unwrap();
    assert_eq!(card.pick_rule, PickRule::PlainArgmin);
    assert!(card.extrapolated_alternative.is_none());
}
