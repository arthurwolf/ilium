use ilium_server::polling_config::load;

#[test]
fn reads_existing_openai_key_without_exposing_it_in_status() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("config.toml"), "[inference.openai]\napi_key = 'test-key'\n[polling_detection]\nenabled = true\nevent_interval = 7\n").unwrap();
    let loaded = load(dir.path()).unwrap();
    assert!(loaded.settings.enabled);
    assert_eq!(loaded.settings.event_interval, 7);
    assert!(loaded.has_openai_key());
    assert_eq!(loaded.openai_key(), "test-key");
}

#[test]
fn missing_file_is_off_and_invalid_settings_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load(dir.path()).unwrap();
    assert!(!loaded.settings.enabled);
    assert!(!loaded.has_openai_key());
    std::fs::write(
        dir.path().join("config.toml"),
        "[polling_detection]\nevent_interval = 0\n",
    )
    .unwrap();
    assert!(load(dir.path()).is_err());
}

#[test]
fn follower_settings_do_not_depend_on_deserializing_provider_credentials() {
    use ilium_server::polling_config::load_settings;
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("config.toml"),
        "[polling_detection]\nenabled = true\nevent_interval = 11\n[inference.openai]\napi_key = ['not', 'a', 'credential']\n").unwrap();
    let settings = load_settings(directory.path()).unwrap();
    assert!(settings.enabled);
    assert_eq!(settings.event_interval, 11);
    assert!(
        load(directory.path()).is_err(),
        "the leader must still validate its credential type"
    );
    std::fs::write(
        directory.path().join("config.toml"),
        "[polling_detection]\nevent_interval = 0\n",
    )
    .unwrap();
    assert!(load_settings(directory.path()).is_err());
}
