use mentalist::executor::{SandboxedExecutor, ExecutionMode, CommandValidator};

#[tokio::test]
async fn test_command_validator_blocking() {
    let validator = CommandValidator::new_default();
    
    // Assert blacklisted commands are blocked
    assert!(validator.validate("rm", &vec!["-rf".to_string(), "/".to_string()]).is_err());
    assert!(validator.validate("chmod", &vec!["777".to_string(), "file".to_string()]).is_err());
    assert!(validator.validate("mkfs", &vec!["/dev/sda1".to_string()]).is_err());
    
    // Assert safe commands are allowed
    assert!(validator.validate("ls", &vec!["-la".to_string()]).is_ok());
    assert!(validator.validate("cat", &vec!["README.md".to_string()]).is_ok());
}

#[tokio::test]
async fn test_local_executor_success() {
    let root = std::env::current_dir().unwrap();
    let executor = SandboxedExecutor::new(
        ExecutionMode::Local,
        root.clone(),
        None
    );
    
    // Run a simple echo command
    let result = executor.execute("echo", vec!["hello-gypsy".to_string()]).await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("hello-gypsy"));
}

#[tokio::test]
async fn test_local_executor_security_gate() {
    let root = std::env::current_dir().unwrap();
    let executor = SandboxedExecutor::new(
        ExecutionMode::Local,
        root.clone(),
        None
    );
    
    // Verify that even in Local mode, the validator blocks rm
    let result = executor.execute("rm", vec!["some_file".to_string()]).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("blacklisted"));
}

#[tokio::test]
async fn test_vault_working_directory() {
    let root = std::env::current_dir().unwrap();
    let vault = root.join("test_vault_tmp");
    std::fs::create_dir_all(&vault).unwrap();
    
    let executor = SandboxedExecutor::new(
        ExecutionMode::Local,
        root.clone(),
        Some(vault.clone())
    );
    
    // Create a file in vault and verify 'ls' sees it
    std::fs::write(vault.join("secret.txt"), "gypsy-data").unwrap();
    
    let result = executor.execute("ls", vec![]).await.unwrap();
    assert!(result.contains("secret.txt"));
    
    // Cleanup
    std::fs::remove_dir_all(&vault).unwrap();
}
