use mentalist::config::{MentalistConfig, SecurityConfig};

#[test]
fn test_default_config() {
    let config = MentalistConfig::default();
    assert_eq!(config.agent.max_turns, 10);
    assert_eq!(config.agent.timeout_seconds, 300);
    assert_eq!(config.security.max_memory_mb, 512);
    assert!(config.security.enforce_sandboxing);
}

#[test]
fn test_config_serialization() {
    let mut config = MentalistConfig::default();
    config.agent.max_turns = 42;
    
    let json = serde_json::to_string(&config).unwrap();
    let deserialized: MentalistConfig = serde_json::from_str(&json).unwrap();
    
    assert_eq!(deserialized.agent.max_turns, 42);
}

#[test]
fn test_security_config_defaults() {
    let security = SecurityConfig::default();
    assert!(security.allowed_commands.contains(&"python".to_string()));
    assert!(security.allowed_commands.contains(&"ls".to_string()));
    assert_eq!(security.max_execution_time_seconds, 60);
}
