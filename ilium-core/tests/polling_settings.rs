use ilium_core::polling_detection::PollingSettings;

#[test]
fn defaults_are_opted_out_and_use_five_events_per_fifteen() {
    let settings = PollingSettings::default();
    assert!(!settings.enabled);
    assert_eq!(settings.event_interval, 5);
    assert_eq!(settings.window_events, 30);
    assert!(settings.validate().is_ok());
}

#[test]
fn invalid_bounds_are_rejected_not_silently_clamped() {
    let mut settings = PollingSettings {
        event_interval: 0,
        ..PollingSettings::default()
    };
    assert!(settings.validate().is_err());
    settings.event_interval = 5;
    settings.window_events = 101;
    assert!(settings.validate().is_err());
    settings.window_events = 15;
    settings.confidence_percent = 100;
    assert!(settings.validate().is_err());
}

#[test]
fn old_configuration_defaults_new_polling_settings() {
    let settings: PollingSettings = toml::from_str("").unwrap();
    assert_eq!(settings, PollingSettings::default());
}

#[test]
fn polling_scope_persists_its_origin_instead_of_following_the_leader() {
    use std::path::{Path, PathBuf};
    let mut settings = PollingSettings::default();
    assert!(
        settings.select_current_session(PathBuf::from("/synthetic/project-a"), "default".into())
    );
    let saved = toml::to_string(&settings).unwrap();
    let restored: PollingSettings = toml::from_str(&saved).unwrap();
    assert!(restored.includes_session(Path::new("/synthetic/project-a"), "default"));
    assert!(!restored.includes_session(Path::new("/synthetic/project-b"), "default"));
    assert!(!restored.includes_session(Path::new("/synthetic/project-a"), "another"));
    let original = settings.clone();
    assert!(!settings.select_current_session(PathBuf::from("relative-project"), "default".into()));
    assert_eq!(
        settings, original,
        "invalid scope must not replace the accepted origin"
    );
}

#[test]
fn polling_scope_rejects_unbound_current_session_and_applies_project_exclusions() {
    use ilium_core::polling_detection::PollingScope;
    use std::path::Path;
    let mut settings = PollingSettings {
        scope: PollingScope::CurrentSession,
        ..PollingSettings::default()
    };
    assert!(
        settings.validate().is_err(),
        "a leader cannot invent the originating session"
    );
    settings.scope = PollingScope::AllSessions;
    assert!(settings.includes_session(Path::new("/synthetic/project-a"), "default"));
    settings
        .excluded_projects
        .push("/synthetic/project-a".into());
    assert!(!settings.includes_session(Path::new("/synthetic/project-a"), "default"));
    assert!(settings.includes_session(Path::new("/synthetic/project-b"), "default"));
}
