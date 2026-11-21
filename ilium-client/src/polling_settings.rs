//! Shared row model for the polling-detection settings interface.
use ilium_core::polling_detection::{PollingScope, PollingSettings};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollingRow {
    Enabled,
    Interval,
    Window,
    Spacing,
    Filter,
    Confidence,
    EvidenceLimit,
    Scope,
    MonthlyLimit,
    UnlimitedBudget,
    AlertCooldown,
    ManageKey,
}

impl PollingRow {
    pub const ALL: [Self; 12] = [
        Self::Enabled,
        Self::Interval,
        Self::Window,
        Self::Spacing,
        Self::Filter,
        Self::Confidence,
        Self::EvidenceLimit,
        Self::Scope,
        Self::MonthlyLimit,
        Self::UnlimitedBudget,
        Self::AlertCooldown,
        Self::ManageKey,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Enabled => "Enable detection",
            Self::Interval => "Check every",
            Self::Window => "Recent window",
            Self::Spacing => "Minimum check spacing",
            Self::Filter => "Local candidate filter",
            Self::Confidence => "Decision threshold",
            Self::EvidenceLimit => "Maximum evidence",
            Self::Scope => "Scope",
            Self::MonthlyLimit => "Monthly limit",
            Self::UnlimitedBudget => "Unlimited budget",
            Self::AlertCooldown => "Alert cooldown",
            Self::ManageKey => "OpenAI credential",
        }
    }

    pub fn value(self, settings: &PollingSettings) -> String {
        match self {
            Self::Enabled => on_off(settings.enabled).into(),
            Self::Interval => format!("{} events", settings.event_interval),
            Self::Window => format!("{} events", settings.window_events),
            Self::Spacing => format!("{} s", settings.minimum_spacing_seconds),
            Self::Filter => on_off(settings.candidate_filter).into(),
            Self::Confidence => format!("{}% · provisional", settings.confidence_percent),
            Self::EvidenceLimit => format!("{} estimated tokens", settings.maximum_evidence_tokens),
            Self::Scope => match settings.scope {
                PollingScope::AllSessions => "All sessions",
                PollingScope::CurrentSession => "Current session",
            }
            .into(),
            Self::MonthlyLimit => {
                let dollars = settings.monthly_limit_microdollars / 1_000_000;
                let fraction = settings.monthly_limit_microdollars % 1_000_000;
                if fraction.is_multiple_of(10_000) {
                    format!("${dollars}.{:02}", fraction / 10_000)
                } else {
                    format!("${dollars}.{fraction:06}")
                }
            }
            Self::UnlimitedBudget => on_off(settings.unlimited_budget).into(),
            Self::AlertCooldown => format!("{} min", settings.alert_cooldown_minutes),
            Self::ManageKey => "Manage in Inference →".into(),
        }
    }

    /// Apply a bounded keyboard or mouse step; callers persist the result.
    pub fn adjust(self, settings: &mut PollingSettings, direction: i32) {
        let step = i64::from(direction.signum());
        let bounded = |value: i64, increment: i64, low: i64, high: i64| {
            (value + step * increment).clamp(low, high)
        };
        match self {
            Self::ManageKey => {}
            Self::Enabled => settings.enabled = !settings.enabled,
            Self::Interval => {
                settings.event_interval = bounded(settings.event_interval.into(), 1, 1, 100) as u8
            }
            Self::Window => {
                settings.window_events = bounded(settings.window_events.into(), 1, 5, 100) as u8
            }
            Self::Spacing => {
                settings.minimum_spacing_seconds =
                    bounded(settings.minimum_spacing_seconds.into(), 10, 0, 3600) as u16
            }
            Self::Filter => settings.candidate_filter = !settings.candidate_filter,
            Self::Confidence => {
                settings.confidence_percent =
                    bounded(settings.confidence_percent.into(), 1, 50, 99) as u8
            }
            Self::EvidenceLimit => {
                settings.maximum_evidence_tokens =
                    bounded(settings.maximum_evidence_tokens.into(), 100, 500, 12000) as u16
            }
            Self::Scope => {
                settings.scope = match settings.scope {
                    PollingScope::AllSessions => PollingScope::CurrentSession,
                    PollingScope::CurrentSession => PollingScope::AllSessions,
                }
            }
            Self::MonthlyLimit => {
                settings.monthly_limit_microdollars = bounded(
                    settings.monthly_limit_microdollars as i64,
                    1_000_000,
                    0,
                    i64::MAX / 1000,
                ) as u64
            }
            Self::UnlimitedBudget => settings.unlimited_budget = !settings.unlimited_budget,
            Self::AlertCooldown => {
                settings.alert_cooldown_minutes =
                    bounded(settings.alert_cooldown_minutes.into(), 1, 1, 1440) as u16
            }
        }
    }
}

fn on_off(value: bool) -> &'static str {
    if value {
        "On"
    } else {
        "Off"
    }
}

/// Physical geometry shared by rendering, help, scrolling and hit testing.
pub(crate) struct PollingView {
    pub lines: Vec<ratatui::text::Line<'static>>,
    pub rows: Vec<PollingRowGeometry>,
}

pub(crate) struct PollingRowGeometry {
    pub first_line: usize,
    pub control_line: usize,
    pub control_x: u16,
    pub control_end: u16,
}

pub(crate) fn layout(app: &crate::app::App, selected: usize, width: u16) -> PollingView {
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};
    use unicode_width::UnicodeWidthStr;
    let mut lines = vec![Line::from("")];
    let mut append = |text: &str, style: Style| {
        for line in crate::settings_ui::wrapped_setting_description_with_indent(text, width, 2) {
            lines.push(Line::from(Span::styled(line, style)));
        }
    };
    append(
        "Polling detection",
        Style::new().add_modifier(Modifier::BOLD),
    );
    append(
        "Enabling sends bounded evidence to OpenAI for paid advisory review.",
        Style::new(),
    );
    append(
        "Includes short assistant messages and tool summaries; uses your saved OpenAI key.",
        Style::new(),
    );
    append("Detection never sends commands to an agent.", Style::new());
    lines.push(Line::from(""));
    let mut rows = Vec::new();
    for (index, row) in PollingRow::ALL.into_iter().enumerate() {
        let style = if index == selected {
            crate::theme::selected_style()
        } else {
            Style::new()
        };
        let label = row.label();
        let value = row.value(&app.polling_settings);
        let first_line = lines.len();
        let control_x = width
            .saturating_sub(2)
            .saturating_sub(value.width() as u16)
            .max(2);
        if label.width() + value.width() + 9 <= usize::from(width) {
            let gap = usize::from(control_x).saturating_sub(2 + label.width());
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(label, style),
                crate::value_control::leader_span(gap),
                Span::styled(value, style),
            ]));
        } else {
            for line in crate::settings_ui::wrapped_setting_description_with_indent(label, width, 2)
            {
                lines.push(Line::from(Span::styled(line, style)));
            }
            // Stack the value when a label/control pair cannot retain breathing room.
            for line in
                crate::settings_ui::wrapped_setting_description_with_indent(&value, width, 2)
            {
                lines.push(Line::from(Span::styled(line, style)));
            }
        }
        let control_line = lines.len() - 1;
        let control_x = if control_line == first_line {
            control_x
        } else {
            2
        };
        let control_end = lines[control_line].width().min(usize::from(u16::MAX)) as u16;
        rows.push(PollingRowGeometry {
            first_line,
            control_line,
            control_x,
            control_end,
        });
        lines.push(Line::from(""));
    }
    let status = match &app.polling_snapshot {
        Some(snapshot) => format!(
            "Detector status: {}",
            match snapshot.status {
                ilium_core::polling_detection::DetectorStatus::Off => "Off",
                ilium_core::polling_detection::DetectorStatus::Watching => "Watching",
                ilium_core::polling_detection::DetectorStatus::WaitingForActivity =>
                    "Waiting for activity",
                ilium_core::polling_detection::DetectorStatus::BudgetReached => "Budget reached",
                ilium_core::polling_detection::DetectorStatus::NoApiKey => "No API key",
                ilium_core::polling_detection::DetectorStatus::Offline => "Offline / backoff",
                ilium_core::polling_detection::DetectorStatus::HistoryUnavailable =>
                    "History unavailable",
                ilium_core::polling_detection::DetectorStatus::UnsupportedAgent =>
                    "Unsupported agent",
            }
        ),
        None => "Detector status: unavailable; server integration pending".into(),
    };
    let mut footer = vec![status, app.polling_save.label().into()];
    footer.extend(findings_lines(app.polling_snapshot.as_ref()));
    footer.push("Spending".into());
    if let Some(snapshot) = &app.polling_snapshot {
        for (label, accounting) in [
            ("Today", &snapshot.day),
            ("This month", &snapshot.month),
            ("All time", &snapshot.all_time),
        ] {
            footer.push(match accounting {
                Some(value)
                    if value.attempts == 0
                        && value.unknown_attempts == 0
                        && value.known_cost_nanodollars == 0
                        && value.reserved_nanodollars == 0
                        && value.input_tokens == 0 =>
                {
                    format!("{label}: No requests yet")
                }
                Some(value) => format!(
                    "{label}: known ${}.{:09}; {} unknown; reserved ${}.{:09}",
                    value.known_cost_nanodollars / 1_000_000_000,
                    value.known_cost_nanodollars % 1_000_000_000,
                    value.unknown_attempts,
                    value.reserved_nanodollars / 1_000_000_000,
                    value.reserved_nanodollars % 1_000_000_000
                ),
                None => format!("{label}: unavailable"),
            });
        }
    } else {
        footer.push("Accounting unavailable until the detector supplies a snapshot.".into());
    }
    for text in footer {
        for line in crate::settings_ui::wrapped_setting_description_with_indent(&text, width, 2) {
            lines.push(Line::from(line));
        }
    }
    PollingView { lines, rows }
}

fn finding_label(kind: ilium_core::polling_detection::FindingKind) -> &'static str {
    use ilium_core::polling_detection::FindingKind;
    match kind {
        FindingKind::PossiblePolling => "Possible polling",
        FindingKind::RepeatedUpdates => "Repeated updates",
        FindingKind::WaitChurn => "Wait churn",
        FindingKind::NoFinding => "No finding",
        FindingKind::InsufficientEvidence => "Insufficient evidence",
    }
}

/// Plain-text findings, derived only from the server's projection. The model's
/// answer is an estimate and carries no rationale; the counted facts are local.
fn findings_lines(snapshot: Option<&ilium_ipc::polling::PollingSnapshot>) -> Vec<String> {
    use ilium_core::polling_detection::FindingKind;
    const SHOWN: usize = 8;
    let Some(snapshot) = snapshot else {
        return vec!["Findings: unavailable until the detector supplies a snapshot.".into()];
    };
    let review = snapshot
        .findings
        .iter()
        .filter(|finding| {
            !matches!(
                finding.kind,
                FindingKind::NoFinding | FindingKind::InsufficientEvidence
            )
        })
        .count();
    let mut lines = vec![if snapshot.findings.is_empty() {
        "Findings: none yet. Advisory only; nothing is ever sent to an agent.".to_string()
    } else {
        format!(
            "Findings: {review} need review, {} recorded. Advisory only; nothing is ever sent to an agent.",
            snapshot.findings.len()
        )
    }];
    // Newest first; the server keeps at most 256, so the list is bounded.
    for finding in snapshot.findings.iter().rev().take(SHOWN) {
        let estimate = finding
            .confidence_basis_points
            .map(|points| format!("{}.{}% estimate", points / 100, (points % 100) / 10))
            .unwrap_or_else(|| "no estimate".into());
        let mut parts = vec![
            finding_label(finding.kind).to_string(),
            format!("pane {} gen {}", finding.pane_id.0, finding.generation),
            format!("session {}", finding.session_id),
            estimate,
        ];
        if finding.repeated_calls > 1 {
            parts.push(format!("{} similar calls", finding.repeated_calls));
        }
        if finding.repeated_updates > 1 {
            parts.push(format!("{} identical updates", finding.repeated_updates));
        }
        if !finding.ownership_known {
            parts.push("who issued the checks is unknown".into());
        }
        if finding.evidence_incomplete {
            parts.push("evidence incomplete".into());
        }
        if let Some(observed) = &finding.observed_utc {
            parts.push(format!("seen {observed}"));
        }
        lines.push(format!("  {}", parts.join(" · ")));
    }
    if snapshot.findings.len() > SHOWN {
        lines.push(format!("  and {} older", snapshot.findings.len() - SHOWN));
    }
    if snapshot.coalesced > 0 || snapshot.dropped_capacity > 0 {
        lines.push(format!(
            "Queue: {} windows merged, {} dropped at capacity.",
            snapshot.coalesced, snapshot.dropped_capacity
        ));
    }
    lines
}

pub(crate) fn view(
    app: &crate::app::App,
    selected: usize,
    width: u16,
) -> Vec<ratatui::text::Line<'static>> {
    layout(app, selected, width).lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_polling_controls_fit_and_keep_values_separate_from_labels() {
        use unicode_width::UnicodeWidthStr;
        let app = crate::app::App::new("synthetic-polling-layout".into(), std::env::temp_dir());
        for width in [40, 60, 100] {
            let lines = view(&app, 0, width);
            for line in lines {
                assert!(
                    line.width() <= usize::from(width),
                    "polling content exceeds {width} cells: {line}"
                );
            }
            for (index, row) in PollingRow::ALL.into_iter().enumerate() {
                let start = layout(&app, index, width).rows[index].control_x;
                assert!(start >= 2, "polling control clips the left inset");
                let lines = view(&app, 0, width);
                let label_line = lines
                    .iter()
                    .find(|line| line.to_string().contains(row.label()))
                    .unwrap()
                    .to_string();
                let value = row.value(&app.polling_settings);
                if label_line.contains(&value) {
                    assert!(
                        usize::from(start) > UnicodeWidthStr::width(row.label()) + 2,
                        "polling control overlaps its label"
                    );
                } else {
                    assert!(
                        lines.iter().any(|line| line.to_string().contains(&value)),
                        "stacked polling value is missing"
                    );
                }
            }
        }
    }

    #[test]
    fn findings_render_estimates_and_local_facts_without_a_rationale() {
        use ilium_core::polling_detection::{
            DetectorStatus, Finding, FindingKind, PollingSettings,
        };
        let mut snapshot = ilium_ipc::polling::PollingSnapshot {
            revision: 1,
            status: DetectorStatus::Watching,
            settings: PollingSettings::default(),
            key_configured: true,
            findings: Vec::new(),
            day: None,
            month: None,
            all_time: None,
            coalesced: 2,
            dropped_capacity: 0,
        };
        assert!(findings_lines(Some(&snapshot))[0].contains("none yet"));
        snapshot.findings.push(Finding {
            finding_id: "synthetic-finding".into(),
            session_id: "synthetic-session".into(),
            session_incarnation: "synthetic-server".into(),
            pane_id: ilium_core::NodeId(7),
            generation: 3,
            kind: FindingKind::PossiblePolling,
            confidence_basis_points: Some(8750),
            observed_utc: Some("2026-11-18T10:00:00Z".into()),
            repeated_calls: 7,
            repeated_updates: 1,
            ownership_known: false,
            evidence_incomplete: false,
        });
        let lines = findings_lines(Some(&snapshot));
        assert!(lines[0].contains("1 need review"));
        assert!(lines[1].contains("Possible polling"));
        assert!(lines[1].contains("87.5% estimate"));
        assert!(lines[1].contains("7 similar calls"));
        assert!(lines[1].contains("unknown"));
        assert!(lines.last().unwrap().contains("2 windows merged"));
    }

    #[test]
    fn controls_stay_in_bounds_and_zero_limit_is_not_unlimited() {
        let mut settings = PollingSettings::default();
        for _ in 0..200 {
            PollingRow::Interval.adjust(&mut settings, -1);
        }
        assert_eq!(settings.event_interval, 1);
        for _ in 0..10 {
            PollingRow::MonthlyLimit.adjust(&mut settings, -1);
        }
        assert_eq!(settings.monthly_limit_microdollars, 0);
        assert!(!settings.unlimited_budget);
        assert!(settings.validate().is_ok());
        assert_eq!(PollingRow::MonthlyLimit.value(&settings), "$0.00");
    }
}
