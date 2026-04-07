use mentalist::config::RuntimeConfig;

#[test]
fn test_config_defaults() {
    let config = RuntimeConfig::default();
    assert_eq!(config.max_steps, 10);
    assert_eq!(config.timeout_seconds, 300);
}

#[test]
fn test_config_deserialization() {
    let json = r#"{
        "max_steps": 5,
        "timeout_seconds": 120,
        "session_id": "test_session"
    }"#;
    let config: RuntimeConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.max_steps, 5);
    assert_eq!(config.session_id, "test_session");
}
