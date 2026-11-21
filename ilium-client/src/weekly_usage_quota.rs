//! Read-only adapter for the local Claude status-line quota observations.
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct Snapshot {
    version: u32,
    records: Vec<Record>,
}
#[derive(Debug, Clone, Deserialize)]
struct Record {
    account: String,
    window: String,
    used_percentage: f64,
    resets_at: i64,
    recorded_at: i64,
    source: String,
    conflict: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct QuotaDisplay {
    pub used_percent: f64,
    pub reset_at: i64,
    pub recorded_at_ms: i64,
    pub freshness: &'static str,
}

/// Only a unique, unconflicted, unexpired weekly record is displayable.
/// A shared arrival timestamp is explicitly not a provider freshness proof.
pub(crate) fn parse_claude_snapshot(
    data: &[u8],
    now_ms: i64,
) -> Result<Option<QuotaDisplay>, &'static str> {
    if data.len() > 1024 * 1024 {
        return Err("quota file exceeds size limit");
    }
    let snapshot: Snapshot = serde_json::from_slice(data).map_err(|_| "invalid quota file")?;
    if snapshot.version != 1 || snapshot.records.len() > 4096 {
        return Err("unsupported quota file");
    }
    let now_secs = now_ms / 1000;
    let mut selected: Option<&Record> = None;
    for record in &snapshot.records {
        if record.window != "seven_day" {
            continue;
        }
        if record.account.is_empty()
            || record.account.len() > 1024
            || !record.used_percentage.is_finite()
            || !(0.0..=100.0).contains(&record.used_percentage)
            || record.resets_at <= now_secs
            || record.resets_at > now_secs.saturating_add(7 * 86400 + 60)
            || record.recorded_at <= 0
            || record.recorded_at > now_ms
            || record.recorded_at
                < record
                    .resets_at
                    .saturating_sub(7 * 86400)
                    .saturating_mul(1000)
            || !matches!(record.source.as_str(), "shared" | "confirmed")
        {
            continue;
        }
        if record.conflict {
            return Err("conflicting weekly reading");
        }
        if let Some(previous) = selected {
            if previous.account != record.account
                || previous.resets_at != record.resets_at
                || previous.used_percentage != record.used_percentage
            {
                return Err("ambiguous account or conflicting weekly readings");
            }
            if previous.recorded_at >= record.recorded_at {
                continue;
            }
        }
        selected = Some(record);
    }
    Ok(selected.map(|record| QuotaDisplay {
        used_percent: record.used_percentage,
        reset_at: record.resets_at,
        recorded_at_ms: record.recorded_at,
        freshness: if record.source == "shared" {
            "freshness unknown (shared)"
        } else if now_ms.saturating_sub(record.recorded_at) > 5 * 60 * 1000 {
            "stale confirmed reading"
        } else {
            "recent confirmed reading"
        },
    }))
}

/// A single transcript observation; account identity is not encoded by the
/// transcript, so it must never be combined with readings from other panes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CodexQuota {
    pub used_percent: f64,
    pub reset_at: i64,
    pub observed_at_ms: i64,
}

pub(crate) fn codex_weekly_reading(
    stats: &crate::session_stats::SessionStats,
    now_ms: i64,
) -> Option<CodexQuota> {
    let observed_at_ms = stats.rate_limits_at_ms?;
    if observed_at_ms <= 0 || observed_at_ms > now_ms {
        return None;
    }
    let now_secs = now_ms / 1000;
    let mut weekly = stats.rate_limits.iter().filter_map(|(_, window)| {
        let reset_at = window.resets_at_unix?;
        (window.window_minutes == Some(7 * 24 * 60)
            && window.used_percent.is_finite()
            && (0.0..=100.0).contains(&window.used_percent)
            && reset_at > now_secs
            && reset_at <= now_secs.saturating_add(7 * 86400 + 60)
            && observed_at_ms >= reset_at.saturating_sub(7 * 86400).saturating_mul(1000))
        .then_some(CodexQuota {
            used_percent: window.used_percent,
            reset_at,
            observed_at_ms,
        })
    });
    let result = weekly.next()?;
    if weekly.next().is_some() {
        return None;
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quota_snapshot_keeps_shared_freshness_unknown_and_rejects_conflicts() {
        let data = br#"{"version":1,"records":[{"account":"a","window":"seven_day","used_percentage":12.0,"resets_at":2000,"recorded_at":1000000,"source":"shared","conflict":false}]}"#;
        let quota = parse_claude_snapshot(data, 1_100_000).unwrap().unwrap();
        assert_eq!(quota.freshness, "freshness unknown (shared)");
        let conflict = br#"{"version":1,"records":[{"account":"a","window":"seven_day","used_percentage":12.0,"resets_at":2000,"recorded_at":1000000,"source":"confirmed","conflict":false},{"account":"b","window":"seven_day","used_percentage":13.0,"resets_at":2000,"recorded_at":1000000,"source":"confirmed","conflict":false}]}"#;
        assert!(parse_claude_snapshot(conflict, 1_100_000).is_err());
    }
    #[test]
    fn expired_bad_and_conflicting_records_are_never_fresh_totals() {
        let expired = br#"{"version":1,"records":[{"account":"a","window":"seven_day","used_percentage":12.0,"resets_at":1000,"recorded_at":900000,"source":"confirmed","conflict":false}]}"#;
        assert_eq!(parse_claude_snapshot(expired, 1_100_000).unwrap(), None);
        let stale = br#"{"version":1,"records":[{"account":"a","window":"seven_day","used_percentage":12.0,"resets_at":2000,"recorded_at":100000,"source":"confirmed","conflict":false}]}"#;
        assert_eq!(
            parse_claude_snapshot(stale, 1_100_000)
                .unwrap()
                .unwrap()
                .freshness,
            "stale confirmed reading"
        );
        let conflict = br#"{"version":1,"records":[{"account":"a","window":"seven_day","used_percentage":12.0,"resets_at":2000,"recorded_at":1000000,"source":"shared","conflict":true}]}"#;
        assert!(parse_claude_snapshot(conflict, 1_100_000).is_err());
        let prior_window = br#"{"version":1,"records":[{"account":"a","window":"seven_day","used_percentage":12.0,"resets_at":800000,"recorded_at":100000,"source":"confirmed","conflict":false}]}"#;
        assert_eq!(
            parse_claude_snapshot(prior_window, 500_000_000).unwrap(),
            None
        );
    }

    #[test]
    fn codex_selects_week_by_duration_and_requires_observation_time() {
        use crate::session_stats::{RateLimitWindow, SessionStats};
        let mut stats = SessionStats {
            rate_limits: vec![
                (
                    "primary".into(),
                    RateLimitWindow {
                        used_percent: 12.0,
                        window_minutes: Some(300),
                        resets_at_unix: Some(1500),
                    },
                ),
                (
                    "secondary".into(),
                    RateLimitWindow {
                        used_percent: 34.0,
                        window_minutes: Some(10080),
                        resets_at_unix: Some(2000),
                    },
                ),
            ],
            ..SessionStats::default()
        };
        assert!(codex_weekly_reading(&stats, 1_100_000).is_none());
        stats.rate_limits_at_ms = Some(1_000_000);
        assert_eq!(
            codex_weekly_reading(&stats, 1_100_000)
                .unwrap()
                .used_percent,
            34.0
        );
        assert!(codex_weekly_reading(&stats, 2_000_000).is_none());
        stats.rate_limits[1].1.resets_at_unix = Some(800_000);
        assert!(codex_weekly_reading(&stats, 500_000_000).is_none());
        stats.rate_limits_at_ms = Some(1_200_000);
        assert!(codex_weekly_reading(&stats, 1_100_000).is_none());
    }
}
