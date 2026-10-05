//! End-to-end checks on a synthetic corpus: generate transcript lines from a
//! known process, parse them, compute statistics, replay the generating
//! trigger and optimise. Everything is synthetic; nothing here resembles real
//! prompts or paths.

use ilium_compaction_analysis::dedupe::dedupe_across_traces;
use ilium_compaction_analysis::optimize::{optimize, ObservedCompactions, OptimizeConfig};
use ilium_compaction_analysis::parse::{line_may_matter, TraceBuilder};
use ilium_compaction_analysis::price::{turn_cost, PriceWeights};
use ilium_compaction_analysis::replay::{simulate, DrawSource, SimConfig};
use ilium_compaction_analysis::rng::SeededRng;
use ilium_compaction_analysis::semantics::{
    claude_offset_observations, measure_claude_offset, AgentSemantics,
};
use ilium_compaction_analysis::stats::{CorpusStats, StatsConfig};
use ilium_compaction_analysis::trace::{CompactionTrigger, SessionTrace};
use ilium_compaction_analysis::AgentKind;

/// Parameters of the generating process.
#[derive(Clone, Copy)]
struct Process {
    sessions: usize,
    turns: usize,
    trigger: u32,
    start_context: u32,
    growth: u32,
    growth_jitter: u32,
    post_cache_read: u32,
    post_fresh: u32,
    epoch_seconds: i64,
    seed: u64,
}

fn iso(milliseconds: i64) -> String {
    let seconds = milliseconds.div_euclid(1_000);
    let millis = milliseconds.rem_euclid(1_000);
    let days = seconds.div_euclid(86_400);
    let remainder = seconds.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Hinnant).
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        remainder / 3_600,
        remainder % 3_600 / 60,
        remainder % 60
    )
}

fn claude_usage_line(id: &str, timestamp_ms: i64, read: u32, write: u32, output: u32) -> String {
    format!(
        r#"{{"type":"assistant","isSidechain":false,"timestamp":"{}","message":{{"id":"{id}","model":"claude-sonnet-5-5","role":"assistant","content":[{{"type":"text","text":"synthetic"}}],"usage":{{"input_tokens":2,"cache_read_input_tokens":{read},"cache_creation_input_tokens":{write},"output_tokens":{output},"cache_creation":{{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":{write}}}}}}}}}"#,
        iso(timestamp_ms)
    )
}

/// One Claude session: warm turns growing by `growth` (+ jitter) per request;
/// when the context reaches the trigger the agent compacts and the next
/// request rebuilds a small prefix.
fn claude_session(process: &Process, index: usize) -> String {
    let mut rng = SeededRng::new(process.seed.wrapping_add(index as u64 * 7_919));
    let mut lines = Vec::new();
    let mut clock = (process.epoch_seconds + index as i64 * 100_000) * 1_000;
    let mut context = process.start_context;
    let mut since_compaction = 100;
    let mut after_compaction = false;
    for turn in 0..process.turns {
        clock += 30_000;
        let id = format!("msg_{index}_{turn}");
        let (read, write) = if turn == 0 {
            (0, process.start_context)
        } else if after_compaction {
            (process.post_cache_read, process.post_fresh)
        } else {
            let growth = process.growth + rng.next_below(process.growth_jitter as usize + 1) as u32;
            (context, growth)
        };
        lines.push(claude_usage_line(&id, clock, read, write, 300));
        context = read + write + 2;
        after_compaction = false;
        since_compaction += 1;
        if context >= process.trigger && since_compaction >= 5 {
            lines.push(format!(
                r#"{{"type":"system","subtype":"compact_boundary","timestamp":"{}","compactMetadata":{{"trigger":"auto","preTokens":{context},"postTokens":15000,"durationMs":9000}}}}"#,
                iso(clock + 1_000)
            ));
            lines.push(format!(
                r#"{{"type":"user","isCompactSummary":true,"timestamp":"{}","message":{{"role":"user","content":"{}"}}}}"#,
                iso(clock + 1_100),
                "s".repeat(14_000)
            ));
            after_compaction = true;
            since_compaction = 0;
            clock += 20_000;
        }
    }
    lines.join("\n")
}

fn parse_claude(text: &str) -> SessionTrace {
    let mut builder = TraceBuilder::new(AgentKind::ClaudeCode);
    for line in text.split('\n') {
        if line_may_matter(AgentKind::ClaudeCode, line.as_bytes()) {
            builder.feed_line(line.as_bytes());
        }
    }
    builder.finish()
}

fn codex_record(id: &str, timestamp_ms: i64, input: u32, cached: u32, output: u32) -> String {
    format!(
        r#"{{"timestamp":"{}","type":"token_usage_record","payload":{{"response_id":"{id}","usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"cache_write_input_tokens":0,"output_tokens":{output}}}}}}}"#,
        iso(timestamp_ms)
    )
}

/// One Codex session: the compaction is its own request (90 % cached input,
/// 4k summary output) followed by a `compacted` line and a rebuilt prefix.
fn codex_session(process: &Process, index: usize) -> String {
    let mut rng = SeededRng::new(process.seed.wrapping_add(index as u64 * 104_729));
    let mut lines = vec![
        r#"{"timestamp":"2026-09-01T00:00:00.000Z","type":"session_meta","payload":{"id":"synthetic","source":"cli"}}"#.to_string(),
        r#"{"timestamp":"2026-09-01T00:00:01.000Z","type":"turn_context","payload":{"model":"gpt-6.1-sol"}}"#.to_string(),
    ];
    let mut clock = (process.epoch_seconds + index as i64 * 100_000) * 1_000;
    let mut context = process.start_context;
    let mut cached = 0;
    let mut since_compaction = 100;
    let mut serial = 0;
    for turn in 0..process.turns {
        clock += 20_000;
        serial += 1;
        let id = format!("resp_{index}_{serial}");
        if turn > 0 {
            let growth = process.growth + rng.next_below(process.growth_jitter as usize + 1) as u32;
            cached = context;
            context += growth;
        }
        lines.push(codex_record(&id, clock, context, cached, 150));
        since_compaction += 1;
        if context + 150 >= process.trigger && since_compaction >= 5 {
            clock += 15_000;
            serial += 1;
            let request_id = format!("resp_{index}_{serial}");
            lines.push(codex_record(
                &request_id,
                clock,
                context + 200,
                context * 9 / 10,
                4_000,
            ));
            lines.push(format!(
                r#"{{"timestamp":"{}","type":"compacted","payload":{{"message":"","replacement_history":[],"compaction_response_id":"{request_id}"}}}}"#,
                iso(clock + 10)
            ));
            context = process.post_cache_read + process.post_fresh;
            cached = process.post_cache_read;
            since_compaction = 0;
            // The rebuilt request is the next record: it must not look like growth.
            clock += 10_000;
            serial += 1;
            let post_id = format!("resp_{index}_{serial}");
            lines.push(codex_record(&post_id, clock, context, cached, 150));
        }
    }
    lines.join("\n")
}

fn parse_codex(text: &str) -> SessionTrace {
    let mut builder = TraceBuilder::new(AgentKind::Codex);
    for line in text.split('\n') {
        if line_may_matter(AgentKind::Codex, line.as_bytes()) {
            builder.feed_line(line.as_bytes());
        }
    }
    builder.finish()
}

fn claude_process(trigger: u32, epoch_seconds: i64, seed: u64) -> Process {
    Process {
        sessions: 12,
        turns: 500,
        trigger,
        start_context: 20_000,
        growth: 2_000,
        growth_jitter: 1_000,
        post_cache_read: 15_000,
        post_fresh: 50_000,
        epoch_seconds,
        seed,
    }
}

fn claude_corpus(process: &Process) -> Vec<SessionTrace> {
    (0..process.sessions)
        .map(|index| parse_claude(&claude_session(process, index)))
        .collect()
}

#[test]
fn generated_claude_corpus_parses_into_the_expected_shape() {
    let process = claude_process(150_000, 1_780_000_000, 1);
    let traces = claude_corpus(&process);
    for trace in &traces {
        assert_eq!(trace.counters.lines_unparseable, 0);
        assert!(trace.compactions.len() >= 5);
        assert!(trace.compactions.iter().all(|event| event.is_measured()));
        assert!(trace
            .compactions
            .iter()
            .all(|event| event.pre_tokens >= 150_000));
        // 14,000 summary characters at 3.5 characters per token.
        assert!(trace
            .compactions
            .iter()
            .all(|event| event.summary_tokens == 4_000));
        assert!(trace
            .compactions
            .iter()
            .all(|event| event.post_context_tokens == 65_002));
    }
}

#[test]
fn corpus_statistics_report_regimes_and_post_sizes() {
    let mut traces = claude_corpus(&claude_process(300_000, 1_760_000_000, 2));
    traces.extend(claude_corpus(&claude_process(150_000, 1_780_000_000, 3)));
    let stats = CorpusStats::compute(
        AgentKind::ClaudeCode,
        &traces,
        &StatsConfig::default(),
        None,
    );
    assert_eq!(stats.counts.main_sessions, 24);
    assert_eq!(stats.counts.subagent_sessions, 0);
    assert_eq!(stats.counts.auto_compactions, stats.counts.compactions);
    assert_eq!(stats.counts.measured_compactions, stats.counts.compactions);
    assert_eq!(stats.regimes.regimes.len(), 2);
    let recent = stats.regimes.most_recent().unwrap();
    assert!((150_000..155_000).contains(&recent.center_tokens));
    let older = &stats.regimes.regimes[1];
    assert!(older.center_tokens >= 300_000);
    assert!(!older.is_most_recent);
    // Fixed prefix and the first post-compaction request.
    let first = stats.first_request_tokens.unwrap();
    assert_eq!(first.p50 as u32, 20_002);
    let post = stats.compactions.first_post_request_tokens.unwrap();
    assert_eq!(post.p50 as u32, 65_002);
    let share = stats.compactions.first_post_cache_read_share.unwrap();
    assert!((share.p50 - 15_000.0 / 65_002.0).abs() < 1e-6);
    // Growth bins: every request adds 2,000..3,000 tokens.
    let populated: Vec<_> = stats.growth.iter().filter_map(|bin| bin.growth).collect();
    assert!(!populated.is_empty());
    assert!(populated
        .iter()
        .all(|growth| (2_000.0..=3_000.0).contains(&growth.p50)));
    // Warm cache everywhere: no cold requests, no rewrite cost.
    assert_eq!(stats.cold_cache.cache_cold_turns, 0);
    assert_eq!(stats.cold_cache.rewrite_cost_share, 0.0);
    // Cycle lengths: about (150k - 65k) / 2.5k requests.
    let cycles = stats.cycles.turns.unwrap();
    assert!(
        cycles.p50 > 20.0 && cycles.p50 < 80.0,
        "median cycle {}",
        cycles.p50
    );
    assert!(stats.cost_mix.cache_read_share > 0.1);
    assert!(stats.cost_mix.total_weighted > 0.0);
    assert!(!stats.cost_mix.usd_complete);
    assert_eq!(stats.models.len(), 1);
    assert_eq!(stats.models[0].family, "sonnet");
}

#[test]
fn replaying_the_generating_trigger_reproduces_the_generating_cost() {
    let process = claude_process(150_000, 1_780_000_000, 4);
    let traces = claude_corpus(&process);
    let weights = PriceWeights::anthropic_research();
    let measured: f64 = traces
        .iter()
        .flat_map(|trace| trace.turns.iter())
        .map(|turn| turn_cost(turn, &weights))
        .sum();
    let mut config = SimConfig::for_agent(AgentKind::ClaudeCode);
    config.triggers = vec![100_000, 125_000, 150_000, 200_000, 300_000];
    let table = simulate(AgentKind::ClaudeCode, &traces, &config, None).unwrap();
    assert_eq!(table.draw_pool.source, DrawSource::RecentRegime);
    assert!((table.measured_total() - measured).abs() / measured < 1e-9);
    let index = table.trigger_index(150_000).unwrap();
    let replayed: f64 = table
        .sessions
        .iter()
        .map(|session| session.outcomes[index].cost)
        .sum();
    // The replay also prices the (unlogged) compaction request and summary,
    // so it may exceed the measured cost slightly; the research saw +2.8 %.
    let relative = (replayed - measured) / measured;
    println!(
        "claude replay vs measured at the generating trigger: {:+.2}%",
        relative * 100.0
    );
    assert!(
        relative.abs() < 0.08,
        "replay {replayed} vs measured {measured}: {relative}"
    );
    // Compaction counts match the generating process within one per session.
    let observed: u32 = table
        .sessions
        .iter()
        .map(|session| session.observed_compactions)
        .sum();
    let replayed_compactions: f64 = table
        .sessions
        .iter()
        .map(|session| session.outcomes[index].compactions)
        .sum();
    assert!(
        (replayed_compactions - f64::from(observed)).abs() <= table.sessions.len() as f64,
        "{replayed_compactions} vs {observed}"
    );
    // Compaction disabled costs more than any compacting trigger here.
    assert!(table
        .sessions
        .iter()
        .all(|session| session.no_compaction_cost > session.outcomes[index].cost));
}

#[test]
fn recommendation_is_deterministic_and_labels_extrapolation() {
    let traces = claude_corpus(&claude_process(300_000, 1_780_000_000, 5));
    let mut config = SimConfig::for_agent(AgentKind::ClaudeCode);
    config.triggers = vec![120_000, 150_000, 200_000, 250_000, 300_000, 400_000];
    let table = simulate(AgentKind::ClaudeCode, &traces, &config, None).unwrap();
    let observed = ObservedCompactions::from_traces(&traces, AgentKind::ClaudeCode);
    assert!(observed.count() > 30);
    let semantics = AgentSemantics::default_for(AgentKind::ClaudeCode);
    let optimize_config = OptimizeConfig::for_agent(AgentKind::ClaudeCode);
    let first = optimize(&table, &observed, &semantics, &optimize_config).unwrap();
    let second = optimize(&table, &observed, &semantics, &optimize_config).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.setting_value, first.trigger_tokens + 33_000);
    assert!(first.bootstrap.resamples >= 200);
    assert_eq!(first.per_model.len(), 1);
    // Every observed compaction fired at 300k or above, so any pick below
    // that is extrapolated and must say so.
    if first.trigger_tokens < 300_000 {
        assert!(first.extrapolated);
        assert!(first.basis.contains("extrapolated"));
    }
    assert!(first
        .candidates
        .iter()
        .any(|row| row.observed_support == 0.0));
}

#[test]
fn trace_cache_round_trip_preserves_the_analysis() {
    let process = claude_process(150_000, 1_780_000_000, 6);
    let traces = claude_corpus(&process);
    let revived: Vec<SessionTrace> = traces
        .iter()
        .map(|trace| SessionTrace::from_json_bytes(&trace.to_json_bytes().unwrap()).unwrap())
        .collect();
    assert_eq!(revived, traces);
    let config = SimConfig::for_agent(AgentKind::ClaudeCode);
    let before = simulate(AgentKind::ClaudeCode, &traces, &config, None).unwrap();
    let after = simulate(AgentKind::ClaudeCode, &revived, &config, None).unwrap();
    assert_eq!(before, after);
    // The cache stays small: well under 60 bytes per request.
    let bytes: usize = traces
        .iter()
        .map(|trace| trace.to_json_bytes().unwrap().len())
        .sum();
    let turns: usize = traces.iter().map(|trace| trace.turns.len()).sum();
    assert!(bytes / turns < 90, "{} bytes per request", bytes / turns);
}

#[test]
fn dollar_prices_flow_through_the_replay() {
    let traces = claude_corpus(&claude_process(150_000, 1_780_000_000, 7));
    let lookup = |model: &str| {
        model.contains("sonnet").then_some(PriceWeights {
            input: 3.0,
            output: 15.0,
            cache_read: 0.3,
            cache_write_5m: 3.75,
            cache_write_1h: 6.0,
        })
    };
    let mut config = SimConfig::for_agent(AgentKind::ClaudeCode);
    config.triggers = vec![150_000, 300_000];
    let table = simulate(AgentKind::ClaudeCode, &traces, &config, Some(&lookup)).unwrap();
    let session = &table.sessions[0];
    let measured_usd = session.measured_usd.unwrap();
    // Same tokens, so dollars = 3 per weighted-input-token million.
    assert!((measured_usd - session.measured_cost * 3.0 / 1.0e6).abs() < 1e-9);
    let usd = table.total_usd(&table.rework, &|_| true);
    let weighted = table.total_costs(&table.rework, &|_| true);
    for (dollars, cost) in usd.iter().zip(&weighted) {
        assert!((dollars.unwrap() - cost * 3.0 / 1.0e6).abs() < 1e-6);
    }
}

#[test]
fn claude_offset_is_remeasured_from_the_corpus() {
    // The generator fires at 150k; pretend the user's window setting was 183k.
    let traces = claude_corpus(&claude_process(150_000, 1_780_000_000, 8));
    let observations = claude_offset_observations(&traces, None, &|_| Some(183_000));
    assert!(observations.len() > 30);
    let measured = measure_claude_offset(&observations).unwrap();
    assert!(
        measured.value > 30_000.0 && measured.value < 33_100.0,
        "{}",
        measured.value
    );
}

#[test]
fn resumed_copies_are_removed_across_traces() {
    let process = claude_process(150_000, 1_780_000_000, 9);
    let original = parse_claude(&claude_session(&process, 0));
    let copy = original.clone();
    let mut traces = vec![original.clone(), copy];
    let report = dedupe_across_traces(&mut traces);
    assert_eq!(report.turns_removed, original.turns.len());
    assert_eq!(report.compactions_removed, original.compactions.len());
    assert!(traces[1].turns.is_empty());
    assert_eq!(traces[0], original);
}

fn codex_corpus(process: &Process) -> Vec<SessionTrace> {
    (0..process.sessions)
        .map(|index| parse_codex(&codex_session(process, index)))
        .collect()
}

#[test]
fn codex_pipeline_parses_replays_and_recommends() {
    let process = Process {
        sessions: 10,
        turns: 600,
        trigger: 200_000,
        start_context: 30_000,
        growth: 1_500,
        growth_jitter: 1_000,
        post_cache_read: 12_800,
        post_fresh: 38_400,
        epoch_seconds: 1_780_000_000,
        seed: 11,
    };
    let traces = codex_corpus(&process);
    for trace in &traces {
        assert_eq!(trace.agent, AgentKind::Codex);
        assert!(trace
            .compactions
            .iter()
            .all(|event| event.request_turn_index.is_some()));
        assert!(trace
            .compactions
            .iter()
            .all(|event| event.summary_tokens == 4_000 && !event.summary_estimated));
        assert!(trace
            .compactions
            .iter()
            .all(|event| event.trigger == CompactionTrigger::Unknown));
        assert!(trace
            .compactions
            .iter()
            .all(|event| event.pre_tokens >= 200_000));
        assert!(trace
            .compactions
            .iter()
            .all(|event| event.post_context_tokens == 51_200));
    }
    let stats = CorpusStats::compute(AgentKind::Codex, &traces, &StatsConfig::default(), None);
    assert!(stats.counts.compactions >= 20);
    assert_eq!(stats.compactions.summary_tokens.unwrap().p50 as u32, 4_000);

    let weights = PriceWeights::codex_research();
    let measured: f64 = traces
        .iter()
        .flat_map(|trace| trace.turns.iter())
        .map(|turn| turn_cost(turn, &weights))
        .sum();
    let mut config = SimConfig::for_agent(AgentKind::Codex);
    config.triggers = vec![120_000, 160_000, 200_000, 232_000];
    let table = simulate(AgentKind::Codex, &traces, &config, None).unwrap();
    assert!((table.measured_total() - measured).abs() / measured < 1e-9);
    let index = table.trigger_index(200_000).unwrap();
    let replayed: f64 = table
        .sessions
        .iter()
        .map(|session| session.outcomes[index].cost)
        .sum();
    // The Codex stream keeps its observed rework and compaction requests are
    // re-created by the replay, so the replay stays close to the measured cost.
    let relative = (replayed - measured) / measured;
    println!(
        "codex replay vs measured at the generating trigger: {:+.2}%",
        relative * 100.0
    );
    assert!(
        relative.abs() < 0.10,
        "replay {replayed} vs measured {measured}: {relative}"
    );
    // Differential rework: replaying exactly the observed count adds nothing.
    let observed: f64 = table
        .sessions
        .iter()
        .map(|session| f64::from(session.observed_compactions))
        .sum();
    let compactions: f64 = table
        .sessions
        .iter()
        .map(|session| session.outcomes[index].compactions)
        .sum();
    assert!((compactions - observed).abs() <= table.sessions.len() as f64);
    let observed_sizes = ObservedCompactions::from_traces(&traces, AgentKind::Codex);
    let recommendation = optimize(
        &table,
        &observed_sizes,
        &AgentSemantics::default_for(AgentKind::Codex),
        &OptimizeConfig::for_agent(AgentKind::Codex),
    )
    .unwrap();
    assert_eq!(
        recommendation.setting_name,
        "model_auto_compact_token_limit"
    );
    assert!(recommendation.setting_value <= 232_560);
}
