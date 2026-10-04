//! Protected host-owned permission review. UI choices are intent, not grants:
//! the broker validates the immutable package, accepted plan and current epoch.

use super::display_text;
use ilium_animation_js::manifest::Capability;
use ratatui::{
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Paragraph, Wrap},
    Frame,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewIdentity {
    /// Full verified archive identity, never a manifest publisher field.
    pub package_digest: String,
    pub instance_generation: u64,
    pub plan_revision: u64,
    /// Native broker epoch captured for this review; copied fields never grant rights.
    pub authorization_epoch: u64,
}

#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub request_id: String,
    pub capability: Capability,
    pub required: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionChoice {
    AllowSession,
    AllowRemembered,
    #[default]
    DenySession,
    DenyRemembered,
}

impl PermissionChoice {
    pub const ALL: [Self; 4] = [
        Self::AllowSession,
        Self::AllowRemembered,
        Self::DenySession,
        Self::DenyRemembered,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::AllowSession => "Allow this session",
            Self::AllowRemembered => "Always allow this scope",
            Self::DenySession => "Deny this session",
            Self::DenyRemembered => "Always deny this scope",
        }
    }

    /// Translate host UI intent only. The broker must still check current
    /// authority; remembered choices require a separate durable acknowledgement.
    pub fn native_intent(self) -> ilium_animation_js::permissions::UserChoice {
        use ilium_animation_js::permissions::UserChoice;
        match self {
            Self::AllowSession => UserChoice::AllowSession,
            Self::AllowRemembered => UserChoice::AllowRemembered,
            Self::DenySession => UserChoice::DenySession,
            Self::DenyRemembered => UserChoice::DenyRemembered,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PackageOrigin {
    /// Only the runtime can supply this after comparing immutable package bytes
    /// with the authenticated Ilium release inventory or signing identity.
    VerifiedIlium,
    #[default]
    Unverified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RememberedDecision {
    pub package_digest: String,
    pub capability_id: String,
    pub scope: Value,
    /// Explicit denial persists too; verified origin must not override it.
    pub allowed: bool,
}

impl RememberedDecision {
    pub fn matches(&self, identity: &ReviewIdentity, request: &PermissionRequest) -> bool {
        self.package_digest == identity.package_digest
            && self.capability_id == request.capability.id
            && self.scope == request.capability.scope
    }
}

#[derive(Debug, Clone)]
pub struct PermissionDecision {
    pub request_id: String,
    pub capability: Capability,
    pub choice: PermissionChoice,
}

#[derive(Debug, Clone)]
pub struct PermissionReview {
    pub identity: ReviewIdentity,
    pub package_name: String,
    pub source: String,
    pub origin: PackageOrigin,
    pub requests: Vec<PermissionRequest>,
    choices: Vec<PermissionChoice>,
    reviewed: Vec<bool>,
    pub cursor: usize,
    pub detail_scroll: u16,
}

impl PermissionReview {
    /// Every request starts denied and unresolved. The caller separately applies
    /// previously broker-resolved decisions; a publisher label never auto-allows.
    pub fn new(
        identity: ReviewIdentity,
        package_name: &str,
        source: &str,
        origin: PackageOrigin,
        requests: Vec<PermissionRequest>,
    ) -> Result<Self, String> {
        if identity.package_digest.len() != 64
            || !identity
                .package_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || identity.instance_generation == 0
            || identity.plan_revision == 0
            || identity.authorization_epoch == 0
            || requests.len() > 64
        {
            return Err("Invalid permission review identity or request count".into());
        }
        let mut identities = BTreeSet::new();
        for request in &requests {
            request
                .capability
                .validate()
                .map_err(|error| error.to_string())?;
            if request.request_id.is_empty()
                || request.request_id.len() > 160
                || request.request_id.chars().any(char::is_control)
                || !identities.insert(request.request_id.clone())
                || request.reason.trim().is_empty()
                || request.reason.len() > 4096
                || request
                    .reason
                    .chars()
                    .any(|character| character.is_control() && character != '\n')
            {
                return Err("Invalid permission identity or application explanation".into());
            }
        }
        let count = requests.len();
        Ok(Self {
            identity,
            package_name: display_text(package_name, 160),
            source: display_text(source, 240),
            origin,
            requests,
            choices: vec![PermissionChoice::DenySession; count],
            reviewed: vec![false; count],
            cursor: 0,
            detail_scroll: 0,
        })
    }

    pub fn choice(&self, request: usize) -> Option<PermissionChoice> {
        self.choices.get(request).copied()
    }

    pub fn decide(&mut self, request: usize, choice: PermissionChoice) -> Result<(), String> {
        let target = self
            .choices
            .get_mut(request)
            .ok_or("Unknown permission request")?;
        *target = choice;
        self.reviewed[request] = true;
        Ok(())
    }

    pub fn is_complete(&self) -> bool {
        self.reviewed.iter().all(|reviewed| *reviewed)
    }

    pub fn required_denied(&self) -> bool {
        self.requests
            .iter()
            .zip(&self.choices)
            .any(|(request, choice)| {
                request.required
                    && matches!(
                        choice,
                        PermissionChoice::DenySession | PermissionChoice::DenyRemembered
                    )
            })
    }

    pub fn decisions(&self, current: &ReviewIdentity) -> Result<Vec<PermissionDecision>, String> {
        if current != &self.identity {
            return Err(
                "Animation package, plan or authorization changed; reopen permission review".into(),
            );
        }
        if !self.is_complete() {
            return Err("Review every requested right before continuing".into());
        }
        Ok(self
            .requests
            .iter()
            .zip(&self.choices)
            .map(|(request, choice)| PermissionDecision {
                request_id: request.request_id.clone(),
                capability: request.capability.clone(),
                choice: *choice,
            })
            .collect())
    }
}

/// Titles and authority descriptions are compiled host text, never supplied by JS.
pub fn permission_description(capability_id: &str) -> Option<(&'static str, &'static str)> {
    Some(match capability_id {
        "network.http" => ("HTTPS connections", "Download only from the listed HTTPS origins. Local-network access requires a separate right; redirects and DNS addresses are checked."),
        "network.local" => ("Local-network connections", "Connect to explicitly listed local/private HTTPS origins. This is separate from ordinary Internet access."),
        "disk.read" => ("Read selected files", "Read or list only files in the host-selected file/folder handle. No arbitrary paths or symlink escape."),
        "disk.write" => ("Write selected output", "Create or update bounded output in the host-selected handle. Read permission does not grant writes or deletion."),
        "audio.loopback" => ("System audio", "Observe the selected system-output source and requested analysis products. This does not grant microphone access."),
        "audio.microphone" => ("Microphone", "Capture the selected microphone source and requested analysis products."),
        "location.observer" => ("Observer location", "Read the location configured by the user for the animation. This does not grant device location tracking."),
        "input.pointer" => ("Animation pointer", "Observe coordinates/buttons inside the animation viewport. No global keyboard or text-input capture."),
        "screen.occlusion" => ("Screen occupancy", "Read occupancy masks for the animation viewport. No glyphs, styles, terminal text, pane titles or application tree."),
        "device.gpu" => ("Bounded graphics backend", "Use admitted host graphics kernels. No arbitrary native execution or raw device handles."),
        "state.persist" => ("Animation storage", "Save bounded state in this plugin's private namespace. No other plugin or application data."),
        _ => return None,
    })
}

pub fn scope_description(capability: &Capability) -> Result<String, String> {
    use ilium_animation_js::permissions::Scope;
    let right = ilium_animation_js::permission_projection::right(capability)
        .map_err(|error| error.to_string())?;
    Ok(match right.scope {
        Scope::Network { origins, methods } => format!(
            "{} · methods: {}",
            origins.into_iter().collect::<Vec<_>>().join(", "),
            methods
                .into_iter()
                .map(|method| format!("{method:?}").to_uppercase())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Scope::Disk { slot, selection } => format!(
            "User-selected {selection:?} · slot {} · {}",
            display_text(&slot, 160),
            if capability.id == "disk.write" {
                "write"
            } else {
                "read/list"
            }
        ),
        Scope::Audio { device, products } => format!(
            "{} · products: {}",
            display_text(&device, 160),
            products
                .into_iter()
                .map(|product| format!("{product:?}").to_lowercase())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Scope::AnimationViewport => "Animation viewport only".into(),
        Scope::Observer => "User-configured observer location".into(),
        Scope::Gpu { kernels } => format!(
            "Host kernels: {}",
            kernels.into_iter().collect::<Vec<_>>().join(", ")
        ),
        Scope::Namespace { name } => format!(
            "This plugin's private storage · {}",
            display_text(&name, 160)
        ),
    })
}

pub fn permission_choice_rect(area: Rect, choice: PermissionChoice) -> Option<Rect> {
    let index = PermissionChoice::ALL
        .iter()
        .position(|candidate| *candidate == choice)? as u16;
    let reserved_rows = PermissionChoice::ALL.len() as u16 + 1; // Choices and footer.
    if area.height < reserved_rows || area.width == 0 {
        return None;
    }
    Some(Rect::new(
        area.x,
        area.y.saturating_add(area.height - reserved_rows + index),
        area.width,
        1,
    ))
}

pub fn permission_choice_at(area: Rect, position: Position) -> Option<PermissionChoice> {
    PermissionChoice::ALL.into_iter().find(|choice| {
        permission_choice_rect(area, *choice).is_some_and(|rectangle| rectangle.contains(position))
    })
}

/// Render into a protected modal surface after composing the animation.
/// The parent uses a real dialog/action handler for the four explicit choices.
pub fn draw_permission_review(
    frame: &mut Frame<'_>,
    area: Rect,
    review: &PermissionReview,
    style: Style,
) {
    let Some(request) = review.requests.get(review.cursor) else {
        frame.render_widget(
            Paragraph::new("No unresolved permission requests").style(style),
            area,
        );
        return;
    };
    let (title, description) = permission_description(&request.capability.id).unwrap_or((
        "Unsupported right",
        "This capability cannot be granted by this host.",
    ));
    let origin = match review.origin {
        PackageOrigin::VerifiedIlium => "Verified Ilium package",
        PackageOrigin::Unverified => "Unverified package",
    };
    let scope = scope_description(&request.capability).unwrap_or_else(|_| "Invalid scope".into());
    let choice = review.choice(review.cursor).unwrap_or_default();
    let lines = vec![
        Line::from(format!(
            "{} · right {}/{}",
            review.package_name,
            review.cursor + 1,
            review.requests.len()
        )),
        Line::from(format!("{origin} · {}", review.source)),
        Line::from(""),
        Line::from(format!(
            "{} ({})",
            title,
            if request.required {
                "required"
            } else {
                "optional"
            }
        )),
        Line::from(format!("Scope: {scope}")),
        Line::from(description),
        Line::from(""),
        Line::from("Application explanation:"),
        Line::from(request.reason.as_str()),
    ];
    // Keep a spacer between scrollable details and the fixed action/footer rows.
    let reserved_rows = PermissionChoice::ALL.len() as u16 + 2;
    let details = Rect::new(
        area.x,
        area.y,
        area.width,
        area.height.saturating_sub(reserved_rows),
    );
    frame.render_widget(
        Paragraph::new(lines)
            .style(style)
            .wrap(Wrap { trim: false })
            .scroll((review.detail_scroll, 0)),
        details,
    );
    for candidate in PermissionChoice::ALL {
        if let Some(rectangle) = permission_choice_rect(area, candidate) {
            let candidate_style = if candidate == choice {
                style.add_modifier(Modifier::REVERSED)
            } else {
                style
            };
            frame.render_widget(
                Paragraph::new(candidate.label()).style(candidate_style),
                rectangle,
            );
        }
    }
    if area.height > 0 {
        frame.render_widget(
            Paragraph::new("Esc cancels · scroll for details").style(style),
            Rect::new(
                area.x,
                area.y.saturating_add(area.height - 1),
                area.width,
                1,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ReviewIdentity {
        ReviewIdentity {
            package_digest: "a".repeat(64),
            instance_generation: 1,
            plan_revision: 2,
            authorization_epoch: 3,
        }
    }

    fn request(id: &str, required: bool) -> PermissionRequest {
        PermissionRequest {
            request_id: id.into(),
            capability: Capability {
                id: "input.pointer".into(),
                scope: Value::String("animation_viewport".into()),
            },
            required,
            reason: "Move hunters toward the pointer.".into(),
        }
    }

    #[test]
    fn verified_label_does_not_grant_and_each_request_is_independent() {
        let mut review = PermissionReview::new(
            identity(),
            "Carpet",
            "package",
            PackageOrigin::VerifiedIlium,
            vec![request("pointer", true), request("pointer_optional", false)],
        )
        .expect("valid review");
        assert_eq!(review.choice(0), Some(PermissionChoice::DenySession));
        assert!(review.required_denied());
        assert!(review.decisions(&identity()).is_err());
        review
            .decide(0, PermissionChoice::AllowSession)
            .expect("choice");
        assert!(!review.is_complete());
        assert_eq!(review.choice(1), Some(PermissionChoice::DenySession));
        review
            .decide(1, PermissionChoice::DenySession)
            .expect("choice");
        assert_eq!(review.decisions(&identity()).expect("resolved").len(), 2);
        let mut stale = identity();
        stale.plan_revision += 1;
        assert!(review.decisions(&stale).is_err());
    }

    #[test]
    fn a_new_package_cannot_inherit_a_remembered_grant() {
        let decision = RememberedDecision {
            package_digest: identity().package_digest,
            capability_id: "input.pointer".into(),
            scope: Value::String("animation_viewport".into()),
            allowed: false,
        };
        assert!(decision.matches(&identity(), &request("pointer", true)));
        let mut replacement = identity();
        replacement.package_digest = "b".repeat(64);
        assert!(!decision.matches(&replacement, &request("pointer", true)));
    }

    #[test]
    fn explanations_cannot_inject_terminal_controls() {
        let mut injection = request("pointer", true);
        injection.reason = "Allow\u{1b}[2J".into();
        assert!(PermissionReview::new(
            identity(),
            "Carpet",
            "package",
            PackageOrigin::Unverified,
            vec![injection]
        )
        .is_err());
        assert!(permission_description("made.up").is_none());
    }

    #[test]
    fn all_four_choices_preserve_native_intent_and_serialized_lifetime() {
        use ilium_animation_js::permissions::UserChoice;
        let expected = [
            (
                PermissionChoice::AllowSession,
                UserChoice::AllowSession,
                "allow_session",
            ),
            (
                PermissionChoice::AllowRemembered,
                UserChoice::AllowRemembered,
                "allow_remembered",
            ),
            (
                PermissionChoice::DenySession,
                UserChoice::DenySession,
                "deny_session",
            ),
            (
                PermissionChoice::DenyRemembered,
                UserChoice::DenyRemembered,
                "deny_remembered",
            ),
        ];
        assert_eq!(PermissionChoice::ALL, expected.map(|(choice, _, _)| choice));
        assert_eq!(PermissionChoice::default(), PermissionChoice::DenySession);
        for (choice, intent, serialized) in expected {
            assert_eq!(choice.native_intent(), intent);
            assert_eq!(
                serde_json::to_value(choice).unwrap(),
                Value::String(serialized.into())
            );
            assert_eq!(
                serde_json::from_value::<PermissionChoice>(Value::String(serialized.into()))
                    .unwrap(),
                choice
            );
            let mut review = PermissionReview::new(
                identity(),
                "Carpet",
                "package",
                PackageOrigin::Unverified,
                vec![request("pointer", true)],
            )
            .unwrap();
            assert!(!review.is_complete());
            assert!(review.decisions(&identity()).is_err());
            review.decide(0, choice).unwrap();
            assert!(review.is_complete());
            assert_eq!(
                review.required_denied(),
                matches!(
                    choice,
                    PermissionChoice::DenySession | PermissionChoice::DenyRemembered
                )
            );
            assert_eq!(review.decisions(&identity()).unwrap()[0].choice, choice);
        }
        assert!(serde_json::from_str::<PermissionChoice>("\"deny\"").is_err());
    }

    #[test]
    fn review_fences_every_native_coordinate_and_rejects_zero_coordinates() {
        let mut review = PermissionReview::new(
            identity(),
            "Carpet",
            "package",
            PackageOrigin::Unverified,
            vec![request("pointer", true)],
        )
        .unwrap();
        review.decide(0, PermissionChoice::AllowRemembered).unwrap();
        let mut changed = [identity(), identity(), identity(), identity()];
        changed[0].package_digest = "b".repeat(64);
        changed[1].instance_generation += 1;
        changed[2].plan_revision += 1;
        changed[3].authorization_epoch += 1;
        for stale in changed {
            assert!(review.decisions(&stale).is_err());
        }
        for coordinate in 0..3 {
            let mut invalid = identity();
            match coordinate {
                0 => invalid.instance_generation = 0,
                1 => invalid.plan_revision = 0,
                _ => invalid.authorization_epoch = 0,
            }
            assert!(PermissionReview::new(
                invalid,
                "Carpet",
                "package",
                PackageOrigin::Unverified,
                vec![request("pointer", true)]
            )
            .is_err());
        }
        assert!(review.decide(1, PermissionChoice::AllowSession).is_err());
        assert_eq!(review.choice(1), None);
        assert_eq!(
            review.decisions(&identity()).unwrap()[0].choice,
            PermissionChoice::AllowRemembered
        );
    }

    #[test]
    fn optional_denials_do_not_block_required_allowance() {
        for denial in [
            PermissionChoice::DenySession,
            PermissionChoice::DenyRemembered,
        ] {
            let mut review = PermissionReview::new(
                identity(),
                "Carpet",
                "package",
                PackageOrigin::Unverified,
                vec![request("required", true), request("optional", false)],
            )
            .unwrap();
            review.decide(0, PermissionChoice::AllowSession).unwrap();
            review.decide(1, denial).unwrap();
            assert!(!review.required_denied());
            assert_eq!(review.decisions(&identity()).unwrap()[1].choice, denial);
        }
    }

    #[test]
    fn four_choice_geometry_matches_hitboxes_and_keeps_footer_unselectable() {
        for height in [5, 6, 12, 30] {
            let area = Rect::new(7, 9, 40, height);
            for (index, choice) in PermissionChoice::ALL.into_iter().enumerate() {
                let rectangle = permission_choice_rect(area, choice).unwrap();
                assert_eq!(
                    rectangle,
                    Rect::new(area.x, area.y + height - 5 + index as u16, area.width, 1)
                );
                assert_eq!(
                    permission_choice_at(area, Position::new(rectangle.x, rectangle.y)),
                    Some(choice)
                );
                assert_eq!(
                    permission_choice_at(area, Position::new(rectangle.right() - 1, rectangle.y)),
                    Some(choice)
                );
                assert_eq!(
                    permission_choice_at(area, Position::new(rectangle.right(), rectangle.y)),
                    None
                );
            }
            assert_eq!(
                permission_choice_at(area, Position::new(area.x, area.bottom() - 1)),
                None
            );
            assert_eq!(
                permission_choice_at(area, Position::new(area.x - 1, area.bottom() - 2)),
                None
            );
            assert_eq!(
                permission_choice_at(area, Position::new(area.x, area.bottom())),
                None
            );
            if height > 5 {
                assert_eq!(
                    permission_choice_at(area, Position::new(area.x, area.y + height - 6)),
                    None
                );
            }
        }
        for height in 0..5 {
            let area = Rect::new(7, 9, 40, height);
            for choice in PermissionChoice::ALL {
                assert_eq!(permission_choice_rect(area, choice), None);
            }
            for row in 0..height {
                assert_eq!(
                    permission_choice_at(area, Position::new(area.x, area.y + row)),
                    None
                );
            }
        }
        for choice in PermissionChoice::ALL {
            assert_eq!(permission_choice_rect(Rect::new(7, 9, 0, 20), choice), None);
        }
    }

    #[test]
    fn rendered_choice_rows_match_hitboxes_and_selected_intent() {
        use ratatui::{backend::TestBackend, Terminal};
        for height in [5, 6, 18] {
            for selected in PermissionChoice::ALL {
                let mut review = PermissionReview::new(
                    identity(),
                    "Carpet",
                    "package",
                    PackageOrigin::Unverified,
                    vec![request("pointer", true)],
                )
                .unwrap();
                review.decide(0, selected).unwrap();
                let mut terminal = Terminal::new(TestBackend::new(60, height)).unwrap();
                terminal
                    .draw(|frame| {
                        draw_permission_review(frame, frame.area(), &review, Style::default())
                    })
                    .unwrap();
                let buffer = terminal.backend().buffer();
                let area = Rect::new(0, 0, 60, height);
                for choice in PermissionChoice::ALL {
                    let row = permission_choice_rect(area, choice).unwrap();
                    let text: String = (row.x..row.right())
                        .map(|x| buffer[(x, row.y)].symbol())
                        .collect();
                    assert_eq!(text.trim_end(), choice.label());
                    assert_eq!(
                        buffer[(row.x, row.y)].modifier.contains(Modifier::REVERSED),
                        choice == selected
                    );
                    assert_eq!(
                        permission_choice_at(area, Position::new(row.x, row.y)),
                        Some(choice)
                    );
                }
                let footer: String = (0..60).map(|x| buffer[(x, height - 1)].symbol()).collect();
                assert!(footer.starts_with("Esc cancels"));
                if height > 5 {
                    let spacer: String =
                        (0..60).map(|x| buffer[(x, height - 6)].symbol()).collect();
                    assert!(spacer.trim().is_empty());
                }
            }
        }
    }

    #[test]
    fn short_views_render_footer_without_invisible_choice_targets() {
        use ratatui::{backend::TestBackend, Terminal};
        let review = PermissionReview::new(
            identity(),
            "Carpet",
            "package",
            PackageOrigin::Unverified,
            vec![request("pointer", true)],
        )
        .unwrap();
        for height in 1..5 {
            let mut terminal = Terminal::new(TestBackend::new(60, height)).unwrap();
            terminal
                .draw(|frame| {
                    draw_permission_review(frame, frame.area(), &review, Style::default())
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            let area = Rect::new(0, 0, 60, height);
            for y in 0..height {
                let text: String = (0..60).map(|x| buffer[(x, y)].symbol()).collect();
                if y == height - 1 {
                    assert!(text.starts_with("Esc cancels"));
                } else {
                    assert!(text.trim().is_empty());
                }
                assert_eq!(permission_choice_at(area, Position::new(0, y)), None);
            }
        }
    }
}
