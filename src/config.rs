use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Central configuration for the Mentalist agentic framework.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MentalistConfig {
    pub agent: AgentConfig,
    pub executor: ExecutorConfig,
    pub security: SecurityConfig,
}

impl Default for MentalistConfig {
    fn default() -> Self {
        Self {
            agent: AgentConfig::default(),
            executor: ExecutorConfig::default(),
            security: SecurityConfig::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentConfig {
    pub max_turns: usize,
    pub timeout_seconds: u64,
    pub fail_on_limit: bool,
    pub max_retries: usize,
    pub max_context_items: usize,
    pub max_tool_calls_per_turn: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_turns: 10,
            timeout_seconds: 300,
            fail_on_limit: false,
            max_retries: 3,
            max_context_items: 50,
            max_tool_calls_per_turn: 20,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExecutorConfig {
    pub default_mode: String, // "local", "docker", "wasm"
    pub sandbox_root: PathBuf,
    pub vault_dir: Option<PathBuf>,
    pub docker_image: Option<String>,
    pub wasm_module_path: Option<PathBuf>,
    pub mcp_initialize_timeout_seconds: u64,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            default_mode: "local".to_string(),
            sandbox_root: PathBuf::from("./sandbox"),
            vault_dir: None,
            docker_image: Some("python:3.11-slim".to_string()),
            wasm_module_path: None,
            mcp_initialize_timeout_seconds: 60,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SecurityConfig {
    pub allowed_commands: Vec<String>,
    pub max_execution_time_seconds: u64,
    pub max_memory_mb: u64,
    pub enforce_sandboxing: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            allowed_commands: vec![
                "python".to_string(),
                "node".to_string(),
                "bash".to_string(),
                "sh".to_string(),
                "cat".to_string(),
                "ls".to_string(),
                "grep".to_string(),
                "find".to_string(),
                "echo".to_string(),
                "curl".to_string(),
                "wget".to_string(),
            ],
            max_execution_time_seconds: 60,
            max_memory_mb: 512,
            enforce_sandboxing: true,
        }
    }
}

impl MentalistConfig {
    /// Loads configuration from environment variables or defaults.
    pub fn from_env() -> Self {
        // Simple implementation for now, could use 'config' crate for more robust loading
        Self::default()
    }

    /// Loads configuration from a JSON or YAML file.
    pub async fn from_file(path: impl AsRef<std::path::Path>) -> crate::error::Result<Self> {
        let path = path.as_ref();
        let content = tokio::fs::read_to_string(path).await?;
        let config: Self = if path.extension().map_or(false, |e| e == "yaml" || e == "yml") {
            serde_yaml::from_str(&content).map_err(|e| crate::error::MentalistError::ConfigError(e.to_string()))?
        } else {
            serde_json::from_str(&content).map_err(|e| crate::error::MentalistError::ConfigError(e.to_string()))?
        };
        Ok(config)
    }
}
