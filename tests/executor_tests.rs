use mentalist::executor::{SandboxedExecutor, ExecutionMode, CommandValidator, ToolExecutor};
use serde_json::json;

#[tokio::test]
async fn test_command_validator_blocking() {
    let validator = CommandValidator::new_default();
    
    // Assert commands not in whitelist are blocked
    assert!(validator.validate("rm", &vec!["-rf".to_string(), "/".to_string()]).is_err());
    assert!(validator.validate("chmod", &vec!["777".to_string(), "file".to_string()]).is_err());
    assert!(validator.validate("mkfs", &vec!["/dev/sda1".to_string()]).is_err());
    
    // Assert safe whitelisted commands are allowed
    assert!(validator.validate("ls", &vec!["-la".to_string()]).is_ok());
    assert!(validator.validate("cat", &vec!["README.md".to_string()]).is_ok());
}

#[tokio::test]
async fn test_local_executor_success() {
    let root = std::env::current_dir().unwrap();
    let executor = SandboxedExecutor::new(
        ExecutionMode::Local,
        root.clone(),
        Some(root.clone())
    ).expect("Failed to create executor");
    
    // Run a simple echo command via a python script to avoid shell character validation
    let script_path = root.join("hello_gypsy.py");
    std::fs::write(&script_path, "print('hello-gypsy')").unwrap();
    
    let result: anyhow::Result<String> = executor.execute("python", json!(vec!["hello_gypsy.py".to_string()])).await;
    let _ = std::fs::remove_file(&script_path);
    
    match result {
        Ok(out) => assert!(out.contains("hello-gypsy")),
        Err(e) => panic!("Executor failed with error: {:?}. Please ensure python or python3 is in your PATH.", e),
    }
}

#[tokio::test]
async fn test_local_executor_security_gate() {
    let root = std::env::current_dir().unwrap();
    let executor = SandboxedExecutor::new(
        ExecutionMode::Local,
        root.clone(),
        Some(root.clone())
    ).expect("Failed to create executor");
    
    // Verify that even in Local mode, the validator blocks rm
    let result: anyhow::Result<String> = executor.execute("rm", json!(vec!["some_file".to_string()])).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not in the whitelist"));
}

#[tokio::test]
async fn test_vault_working_directory() {
    let root = std::env::current_dir().unwrap();
    let vault = root.join("test_vault_tmp");
    let _ = std::fs::remove_dir_all(&vault); 
    std::fs::create_dir_all(&vault).unwrap();
    
    let executor = SandboxedExecutor::new(
        ExecutionMode::Local,
        root.clone(),
        Some(vault.clone())
    ).expect("Failed to create executor");
    
    // Create a file in vault and verify it sees it via python script (avoiding forbidden chars in args)
    std::fs::write(vault.join("secret.txt"), "gypsy-data").unwrap();
    let list_script = vault.join("list_dir.py");
    std::fs::write(&list_script, "import os; print(os.listdir('.'))").unwrap();
    
    let result_str: String = match executor.execute("python", json!(vec!["list_dir.py".to_string()])).await {
        Ok(out) => out,
        Err(e) => panic!("Python execution failed in vault: {:?}. Ensure python/python3 is accessible.", e),
    };
    assert!(result_str.contains("secret.txt"));
    
    // Cleanup
    let _ = std::fs::remove_dir_all(&vault);
}
